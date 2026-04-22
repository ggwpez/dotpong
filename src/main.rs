use anyhow::{Context, Result};
use clap::Parser;
use core::str::FromStr;
use dotpong::data::{init_database, store_result, TxResult};
use dotpong::logs::InMemoryLogger;
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
    let logger = InMemoryLogger::init();

    let args = Args::parse();
    let config = load_config()?;

    let net_config = get_network(&config, &args.network)?;
    let keypair = load_keypair()?;
    let db = init_database(&args.network)?;

    let state = Arc::new(web::AppState {
        db: Mutex::new(db),
        network: args.network.clone(),
        logger,
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
        let mut ticker = AlignedTicker::new(delay);

        loop {
            ticker.wait().await;

            match measure_with_failover(&endpoints, &keypair).await {
                Ok(result) => {
                    if let Some(ref err) = result.error {
                        log::error!("Measurement failed: {err}");
                    } else {
                        log::info!(
                            "Measurement: sending={}ms, inclusion={}ms, finalization={}ms, total={}ms",
                            result.sending_ms, result.inclusion_ms, result.finalization_ms,
                            result.sending_ms + result.inclusion_ms + result.finalization_ms
                        );
                    }
                    let db = collector_state.db.lock().unwrap();
                    if let Err(e) = store_result(&db, &result) {
                        log::error!("Failed to store result: {e}");
                    }
                }
                Err(e) => {
                    log::error!("All RPC endpoints failed: {e}");
                    let entry = TxResult::err(format!("{e}"));
                    let db = collector_state.db.lock().unwrap();
                    if let Err(e) = store_result(&db, &entry) {
                        log::error!("Failed to store error result: {e}");
                    }
                }
            }

            ticker.advance();
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
async fn measure_with_failover(endpoints: &[String], keypair: &Keypair) -> Result<TxResult> {
    let mut errors = Vec::new();

    for (i, rpc) in endpoints.iter().enumerate() {
        log::debug!("Trying RPC endpoint {}/{}: {}", i + 1, endpoints.len(), rpc);

        match send_tx(rpc, keypair).await {
            Ok(timing) => return Ok(timing),
            Err(e) => {
                log::warn!("RPC {} failed: {e}", rpc);
                errors.push(format!("{rpc}: {e}"));
            }
        }
    }

    Err(anyhow::anyhow!("{}", errors.join("; ")))
}

async fn with_timeout<T>(secs: u64, step: &str, fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .map_err(|_| anyhow::anyhow!("{step}: timed out after {secs}s"))?
}

/// Send a transaction and measure all timing segments
async fn send_tx(rpc: &str, keypair: &Keypair) -> Result<TxResult> {
    let start = Instant::now();

    let api = with_timeout(60, "RPC connect", async {
        OnlineClient::<SubstrateConfig>::from_url(rpc)
            .await
            .context("Failed to connect to RPC")
    }).await?;

    let call = subxt::dynamic::tx(
        "System",
        "remark",
        vec![subxt::dynamic::Value::from_bytes(b"")],
    );

    let extrinsic = with_timeout(60, "Sign transaction", async {
        api.tx()
            .create_signed(&call, keypair, Default::default())
            .await
            .context("Failed to create signed transaction")
    }).await?;

    log::debug!(
        "Submitting tx from {}",
        keypair.public_key().to_account_id()
    );

    let mut subscription = with_timeout(60, "Submit transaction", async {
        extrinsic
            .submit_and_watch()
            .await
            .context("Failed to submit transaction")
    }).await?;
    let sending_elapsed = start.elapsed();
    log::debug!("Sending took {}ms (connect + sign + submit)", sending_elapsed.as_millis());

    let mut inclusion_at = None;
    let mut finalization_at = None;

    with_timeout(60, "Wait for finalization", async {
        while let Some(status) = subscription.next().await {
            match status.context("Transaction status error")? {
                InBestBlock(_) => {
                    if inclusion_at.is_none() {
                        inclusion_at = Some(start.elapsed());
                        log::debug!("Included in best block after {}ms", inclusion_at.unwrap().as_millis());
                    }
                }
                InFinalizedBlock(_) => {
                    finalization_at = Some(start.elapsed());
                    log::debug!("Finalized after {}ms", finalization_at.unwrap().as_millis());
                    break;
                }
                Validated | Broadcasted { .. } | NoLongerInBestBlock => {}
                status => {
                    log::warn!("Unexpected status: {:?}", status);
                }
            }
        }
        Ok(())
    }).await?;

    let finalization_at = finalization_at.context("Transaction was not finalized")?;
    let inclusion_at = inclusion_at.unwrap_or(finalization_at);

    // Store as segments: sending | inclusion | finalization
    let sending_ms = sending_elapsed.as_millis() as u64;
    let inclusion_ms = (inclusion_at - sending_elapsed).as_millis() as u64;
    let finalization_ms = (finalization_at - inclusion_at).as_millis() as u64;

    Ok(TxResult::ok(sending_ms, inclusion_ms, finalization_ms))
}

/// Tick scheduler aligned to unix epoch multiples of a given interval.
struct AlignedTicker {
    interval_secs: u64,
    next_tick: u64,
}

impl AlignedTicker {
    fn new(interval_secs: u64) -> Self {
        let now = Self::now_secs();
        Self {
            interval_secs,
            next_tick: now - (now % interval_secs) + interval_secs,
        }
    }

    async fn wait(&self) {
        let now = Self::now_secs();
        if self.next_tick > now {
            let wait = self.next_tick - now;
            log::info!("Waiting {}s until next measurement", wait);
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    }

    fn advance(&mut self) {
        self.next_tick += self.interval_secs;
        let now = Self::now_secs();
        while self.next_tick <= now {
            self.next_tick += self.interval_secs;
        }
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
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
