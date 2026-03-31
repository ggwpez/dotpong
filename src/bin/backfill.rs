use anyhow::Result;
use clap::Parser;
use dotpong::data::{init_database, store_result, unix_ms, TxResult};
use rand::Rng;

#[derive(Parser)]
#[command(name = "backfill")]
#[command(about = "Populate database with random historical data")]
struct Args {
    /// Network to populate
    #[arg(short, long)]
    network: String,

    /// Number of days to backfill
    #[arg(short = 'D', long, default_value = "7")]
    days: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let db = init_database(&args.network)?;

    let mut rng = rand::thread_rng();

    let interval_ms: i64 = 10 * 60 * 1000;
    let entries_per_day = 24 * 6;
    let total_entries = entries_per_day * args.days as usize;

    let now_ms = unix_ms();
    let start_ms = now_ms - (args.days as i64 * 24 * 60 * 60 * 1000);

    let mut errors = 0;

    println!(
        "Backfilling {} entries ({} days) for network '{}'",
        total_entries, args.days, args.network
    );

    for i in 0..total_entries {
        let timestamp = start_ms + (i as i64 * interval_ms);

        let result = if rng.gen_ratio(1, 20) {
            // ~5% error entries
            errors += 1;
            let errors_pool = [
                "RPC connect: timed out after 60s",
                "RPC error: -32000 Transaction pool is full",
                "WebSocket connection closed unexpectedly",
                "Wait for finalization: timed out after 60s",
                "All RPC endpoints failed: connection refused",
            ];
            TxResult {
                timestamp,
                sending_ms: 0,
                inclusion_ms: 0,
                finalization_ms: 0,
                error: Some(errors_pool[rng.gen_range(0..errors_pool.len())].to_string()),
            }
        } else {
            TxResult {
                timestamp,
                sending_ms: rng.gen_range(100..=500),
                inclusion_ms: rng.gen_range(1000..=5000),
                finalization_ms: rng.gen_range(10000..=50000),
                error: None,
            }
        };

        store_result(&db, &result)?;

        if (i + 1) % 100 == 0 {
            println!("  {} / {} entries", i + 1, total_entries);
        }
    }

    println!("Done! Inserted {} entries ({} errors).", total_entries, errors);
    Ok(())
}
