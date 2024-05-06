use anyhow::Result;
use core::str::FromStr;
use scale_value::Value;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use subxt::{tx::TxStatus::*, OnlineClient, SubstrateConfig};
use subxt_signer::{sr25519::Keypair, SecretUri};
use core::future::Future;
use std::thread::sleep;
use std::time::{Instant, SystemTime};
use subxt::lightclient::LightClient;

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    page: String,
    transactions: Vec<NetworkConfig>,
    #[serde(skip)]
    secrets: Secrets,
    interval_sec: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct NetworkConfig {
    rpc: String,
    metrics: Metric,
}

#[derive(Debug, Serialize, Deserialize)]
struct Metric {
    /// Metric ID for included TX.
    inclusion: String,
    /// Metric ID for finalized TX.
    finalization: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Secrets {
    #[serde(skip)]
    instatus_key: String,
    #[serde(skip)]
    substrate_uri: String,
}

#[derive(Debug)]
struct TxTiming {
    when: i64,
    inclusion: Duration,
    finalization: Duration,
}

#[derive(Debug)]
struct SyncTiming {
    when: i64,
    warp: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config()?;

    loop {
        for net in config.transactions.iter() {
            let timing = retry(|| send_tx(&net, &config.secrets)).await?;
            log::info!("TX to {} took {:?}", net.rpc, timing);
            timing.upload(&config, &net.metrics).await?;
            sleep(Duration::from_secs(5));
        }

        log::info!("Sleeping for {} seconds until next round-robin", config.interval_sec);
        sleep(Duration::from_secs(config.interval_sec as u64));
    }
}

// TODO use
async fn sync(_tx: &NetworkConfig) -> Result<SyncTiming> {
    const POLKADOT_SPEC: &str = include_str!("../specs/polkadot-relay.json");

    // curl -H "Content-Type: application/json" -d '{"id":1, "jsonrpc":"2.0", "method": "sync_state_genSyncSpec", "params":[true]}' https://rococo-rpc.polkadot.io | jq .result > chain_spec.json

    log::info!("Starting Polkadot light client sync");
    let when = unix_ms();
    let now = Instant::now();
    let (_lightclient, polkadot_rpc) = LightClient::relay_chain(POLKADOT_SPEC)?;
    let _api = OnlineClient::<SubstrateConfig>::from_rpc_client(polkadot_rpc).await?;

    Ok(SyncTiming {
        when,
        warp: now.elapsed(),
    })
}

async fn send_tx(tx: &NetworkConfig, sk: &Secrets) -> Result<TxTiming> {
    let api = OnlineClient::<SubstrateConfig>::from_url(&tx.rpc).await?;
    let uri = SecretUri::from_str(&sk.substrate_uri)?;
    let keypair = Keypair::from_uri(&uri)?;

    let call = subxt::dynamic::tx("System", "remark", vec![Value::from_bytes(b"test")]);

    let extrinsic = api
        .tx()
        .create_signed(&call, &keypair, Default::default())
        .await?;

    log::info!(
        "Sending TX to {} from acc {}",
        tx.rpc,
        keypair.public_key().to_account_id().to_string()
    );

    let when = unix_ms();
    let start = std::time::Instant::now();
    let mut subscription = extrinsic.submit_and_watch().await?;

    let mut inclusion = None;
    let mut finalization = None;

    while let Some(status) = subscription.next().await {
        match status? {
            InBestBlock(_) => {
                if inclusion.is_some() {
                    log::warn!("TX included multiple times; fork?");
                    continue;
                }
                inclusion = Some(start.elapsed());
                log::info!("TX included after {} ms", inclusion.unwrap().as_millis());
            }
            InFinalizedBlock(_) => {
                finalization = Some(start.elapsed());
                log::info!(
                    "TX finalized after {} ms",
                    finalization.unwrap().as_millis()
                );
            }
            Validated | Broadcasted { .. } | NoLongerInBestBlock => {}
            status => {
                log::error!("Unexpected status: {:?}", status);
                inclusion = Some(inclusion.unwrap_or(Duration::from_secs(60)));
                finalization = Some(finalization.unwrap_or(Duration::from_secs(60)));
            }
        }
    }

    Ok(TxTiming {
        when,
        inclusion: inclusion.or(finalization).ok_or_else(|| anyhow::anyhow!("Not included"))?,
        finalization: finalization.ok_or_else(|| anyhow::anyhow!("Not finalized"))?,
    })
}

impl TxTiming {
    pub async fn upload(&self, config: &Config, metrics: &Metric) -> Result<()> {
        retry(|| upload_metric(
            &config.page,
            &metrics.inclusion,
            &config.secrets,
            self.when,
            self.inclusion,
        )).await?;

        sleep(Duration::from_secs(5));
        
        retry(|| upload_metric(
            &config.page,
            &metrics.finalization,
            &config.secrets,
            self.when,
            self.finalization,
        )).await?;
        Ok(())
    }
}

async fn upload_metric(
    page: &str,
    metric: &str,
    secret: &Secrets,
    when: i64,
    what: Duration,
) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("https://api.instatus.com/v1/{page}/metrics/{metric}",);

    let body = serde_json::json!({
        "timestamp": when,
        "value": what.as_millis(),
    });

    let res = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", &secret.instatus_key))
        .json(&body)
        .send()
        .await?;

    if res.status().is_success() {
        log::info!("Uploaded metric for {}: {:?}", metric, what);
    } else {
        log::error!(
            "Failed to upload metric for {}: {:?}",
            metric,
            res.text().await?
        );
    }

    Ok(())
}

fn load_config() -> Result<Config> {
    std::env::set_var("RUST_LOG", "info");
    dotenv::dotenv().ok();
    env_logger::init();

    let config = std::fs::read_to_string("config.json")?;
    let mut config: Config = serde_json::from_str(&config)?;

    config.secrets.instatus_key = std::env::var("INSTATUS_KEY")?;
    config.secrets.substrate_uri = std::env::var("SUBSTRATE_URI")?;

    Ok(config)
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn retry<T, E, Fut, F: FnMut() -> Fut>(mut f: F) -> Result<T, E>
where
    Fut: Future<Output = Result<T, E>>,
    E: core::fmt::Debug,
    T: core::fmt::Debug,
{
    let mut count = 0;
    loop {
        let result = f().await;

        if result.is_ok() {
            break result;
        } else {
            log::error!("Retry #{} failed: {:?}", count + 1, result);
            sleep(Duration::from_secs(15));
            if count > 5 {
                break result;
            }
            count += 1;
        }
    }
}
