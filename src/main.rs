use anyhow::{Context, Result};
use clap::Parser;
use core::str::FromStr;
use dotpong::data::{init_database, store_timing, TxTiming};
use dotpong::web;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use subxt::{tx::TxStatus::*, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};

#[derive(Parser)]
#[command(name = "dotpong")]
#[command(about = "Measure transaction finality times on Polkadot networks")]
struct Args {
    /// Network to monitor (must match a key in config.json)
    #[arg(short, long)]
    network: String,

    /// Delay between measurements in seconds
    #[arg(short, long, default_value = "600")]
    delay: u64,

    /// Port to serve web UI on
    #[arg(short, long, default_value = "8080")]
    port: u16,
}

/// Application configuration
#[derive(Debug, Deserialize)]
struct Config {
    /// Available networks, keyed by name
    networks: HashMap<String, NetworkConfig>,
}

/// Configuration for a single network
#[derive(Debug, Deserialize)]
struct NetworkConfig {
    /// RPC endpoints (tried in order for failover)
    rpc_endpoints: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args = Args::parse();
    let config = load_config()?;

    let net_config = get_network(&config, &args.network)?;
    let keypair = load_keypair()?;
    let db = init_database(&args.network)?;

    let state = Arc::new(web::AppState {
        db: Mutex::new(db),
        network: args.network.clone(),
    });

    log::info!(
        "Starting dotpong for '{}' with {} RPC endpoints, {}s interval, serving on port {}",
        args.network,
        net_config.rpc_endpoints.len(),
        args.delay,
        args.port
    );

    // Spawn collector task
    let endpoints = net_config.rpc_endpoints.clone();
    let collector_state = state.clone();
    let delay = args.delay;
    tokio::spawn(async move {
        loop {
            match measure_with_failover(&endpoints, &keypair).await {
                Ok(timing) => {
                    log::info!(
                        "Measurement: inclusion={}ms, finalization={}ms",
                        timing.inclusion_ms,
                        timing.finalization_ms
                    );
                    let db = collector_state.db.lock().unwrap();
                    if let Err(e) = store_timing(&db, &timing) {
                        log::error!("Failed to store timing: {e}");
                    }
                }
                Err(e) => {
                    log::error!("All RPC endpoints failed: {e}");
                }
            }

            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    });

    // Run web server on main task
    web::serve(state, args.port).await?;

    Ok(())
}

fn get_network<'a>(config: &'a Config, network: &str) -> Result<&'a NetworkConfig> {
    config.networks.get(network).with_context(|| {
        let available: Vec<_> = config.networks.keys().collect();
        format!("Unknown network '{}'. Available: {:?}", network, available)
    })
}

/// Try each RPC endpoint in order until one succeeds
async fn measure_with_failover(endpoints: &[String], keypair: &Keypair) -> Result<TxTiming> {
    let mut last_error = None;

    for (i, rpc) in endpoints.iter().enumerate() {
        log::info!("Trying RPC endpoint {}/{}: {}", i + 1, endpoints.len(), rpc);

        match send_tx(rpc, keypair).await {
            Ok(timing) => return Ok(timing),
            Err(e) => {
                log::warn!("RPC {} failed: {e}", rpc);
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("No RPC endpoints configured")))
}

/// Send a transaction and measure inclusion + finalization times
async fn send_tx(rpc: &str, keypair: &Keypair) -> Result<TxTiming> {
    let api = OnlineClient::<SubstrateConfig>::from_url(rpc)
        .await
        .context("Failed to connect to RPC")?;

    let call = subxt::dynamic::tx(
        "System",
        "remark",
        vec![subxt::dynamic::Value::from_bytes(b"dotpong")],
    );

    let extrinsic = api
        .tx()
        .create_signed(&call, keypair, Default::default())
        .await
        .context("Failed to create signed transaction")?;

    log::info!(
        "Submitting tx from {}",
        keypair.public_key().to_account_id()
    );

    let start = Instant::now();
    let mut subscription = extrinsic
        .submit_and_watch()
        .await
        .context("Failed to submit transaction")?;

    let mut inclusion = None;
    let mut finalization = None;

    while let Some(status) = subscription.next().await {
        match status.context("Transaction status error")? {
            InBestBlock(_) => {
                if inclusion.is_none() {
                    inclusion = Some(start.elapsed());
                    log::info!("Included in best block after {}ms", inclusion.unwrap().as_millis());
                }
            }
            InFinalizedBlock(_) => {
                finalization = Some(start.elapsed());
                log::info!("Finalized after {}ms", finalization.unwrap().as_millis());
                break;
            }
            Validated | Broadcasted { .. } | NoLongerInBestBlock => {}
            status => {
                log::warn!("Unexpected status: {:?}", status);
            }
        }
    }

    let finalization = finalization.context("Transaction was not finalized")?;
    let inclusion = inclusion.unwrap_or(finalization);

    Ok(TxTiming::new(
        inclusion.as_millis() as u64,
        finalization.as_millis() as u64,
    ))
}

fn init_logging() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
}

fn load_config() -> Result<Config> {
    let config_str = std::fs::read_to_string("config.json")
        .context("Failed to read config.json")?;
    serde_json::from_str(&config_str).context("Failed to parse config.json")
}

fn load_keypair() -> Result<Keypair> {
    dotenv::dotenv().ok();
    let uri_str = std::env::var("SUBSTRATE_URI")
        .context("SUBSTRATE_URI environment variable not set")?;
    let uri = SecretUri::from_str(&uri_str)
        .context("Invalid SUBSTRATE_URI")?;
    Keypair::from_uri(&uri).context("Failed to create keypair from URI")
}
