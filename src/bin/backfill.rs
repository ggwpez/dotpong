use anyhow::Result;
use clap::Parser;
use dotpong::data::{init_database, store_timing, TxTiming};
use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

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

    // 10 minutes = 600_000 ms
    let interval_ms: i64 = 10 * 60 * 1000;
    let entries_per_day = 24 * 6; // 144 entries per day
    let total_entries = entries_per_day * args.days as usize;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Start from (days ago) and work forward
    let start_ms = now_ms - (args.days as i64 * 24 * 60 * 60 * 1000);

    println!(
        "Backfilling {} entries ({} days) for network '{}'",
        total_entries, args.days, args.network
    );

    for i in 0..total_entries {
        let timestamp = start_ms + (i as i64 * interval_ms);

        // Random in-block: 1-5 seconds (1000-5000ms)
        let inclusion_ms = rng.gen_range(1000..=5000);

        // Random finalization: 10-50 seconds (10000-50000ms)
        let finalization_ms = rng.gen_range(10000..=50000);

        let timing = TxTiming {
            timestamp,
            inclusion_ms,
            finalization_ms,
        };

        store_timing(&db, &timing)?;

        if (i + 1) % 100 == 0 {
            println!("  {} / {} entries", i + 1, total_entries);
        }
    }

    println!("Done! Inserted {} entries.", total_entries);
    Ok(())
}
