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

    let interval_ms: i64 = 10 * 60 * 1000;
    let entries_per_day = 24 * 6;
    let total_entries = entries_per_day * args.days as usize;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    let start_ms = now_ms - (args.days as i64 * 24 * 60 * 60 * 1000);

    println!(
        "Backfilling {} entries ({} days) for network '{}'",
        total_entries, args.days, args.network
    );

    for i in 0..total_entries {
        let timestamp = start_ms + (i as i64 * interval_ms);

        // sending (connect + sign + submit): 100-500ms
        let sending_ms = rng.gen_range(100..=500);
        // in-block: 1-5 seconds
        let inclusion_ms = rng.gen_range(1000..=5000);
        // finalization: 10-50 seconds
        let finalization_ms = rng.gen_range(10000..=50000);

        let timing = TxTiming {
            timestamp,
            sending_ms,
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
