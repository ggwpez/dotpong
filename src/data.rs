use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Timing data from a single transaction.
/// All durations are segments (not cumulative):
///   total = sending_ms + inclusion_ms + finalization_ms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxTiming {
    /// Unix timestamp in milliseconds when measurement started
    pub timestamp: i64,
    /// Time for RPC connect + tx signing + submit_and_watch
    pub sending_ms: u64,
    /// Time from submission to included in best block
    pub inclusion_ms: u64,
    /// Time from in-block to finalized
    pub finalization_ms: u64,
}

impl TxTiming {
    pub fn new(sending_ms: u64, inclusion_ms: u64, finalization_ms: u64) -> Self {
        Self {
            timestamp: unix_ms(),
            sending_ms,
            inclusion_ms,
            finalization_ms,
        }
    }
}

pub fn init_database(network: &str) -> Result<Connection> {
    let db_path = format!("dotpong-{}.db", network);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open database: {}", db_path))?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS timings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            sending_ms INTEGER NOT NULL,
            inclusion_ms INTEGER NOT NULL,
            finalization_ms INTEGER NOT NULL
        )",
        [],
    )
    .context("Failed to create timings table")?;

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM timings", [], |row| row.get(0))
        .unwrap_or(0);

    log::info!("Database initialized: {} ({} entries)", db_path, count);
    Ok(conn)
}

pub fn store_timing(conn: &Connection, timing: &TxTiming) -> Result<()> {
    conn.execute(
        "INSERT INTO timings (timestamp, sending_ms, inclusion_ms, finalization_ms) VALUES (?1, ?2, ?3, ?4)",
        [timing.timestamp, timing.sending_ms as i64, timing.inclusion_ms as i64, timing.finalization_ms as i64],
    )
    .context("Failed to insert timing")?;
    Ok(())
}

/// Aggregated timing data for a time bucket
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedTiming {
    pub timestamp: i64,
    pub avg_sending_ms: u64,
    pub avg_inclusion_ms: u64,
    pub avg_finalization_ms: u64,
    pub sample_count: u32,
}

/// Get raw data points for the last 24 hours
pub fn get_hourly(conn: &Connection) -> Result<Vec<TxTiming>> {
    let now = unix_ms();
    let start = now - (24 * 60 * 60 * 1000);

    let mut stmt = conn.prepare(
        "SELECT timestamp, sending_ms, inclusion_ms, finalization_ms
         FROM timings
         WHERE timestamp >= ?1
         ORDER BY timestamp ASC"
    )?;

    let timings = stmt
        .query_map([start], |row| {
            Ok(TxTiming {
                timestamp: row.get(0)?,
                sending_ms: row.get::<_, i64>(1)? as u64,
                inclusion_ms: row.get::<_, i64>(2)? as u64,
                finalization_ms: row.get::<_, i64>(3)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(timings)
}

/// Get daily aggregates for the last 7 days
pub fn get_daily(conn: &Connection) -> Result<Vec<AggregatedTiming>> {
    let day_ms: i64 = 24 * 60 * 60 * 1000;
    let now = unix_ms();
    let start = now - (7 * day_ms);

    get_aggregated(conn, start, day_ms)
}

/// Get weekly aggregates for the last 4 weeks
pub fn get_weekly(conn: &Connection) -> Result<Vec<AggregatedTiming>> {
    let week_ms: i64 = 7 * 24 * 60 * 60 * 1000;
    let now = unix_ms();
    let start = now - (4 * week_ms);

    get_aggregated(conn, start, week_ms)
}

fn get_aggregated(conn: &Connection, start: i64, bucket_ms: i64) -> Result<Vec<AggregatedTiming>> {
    let mut stmt = conn.prepare(
        "SELECT
            (timestamp / ?1) * ?1 as bucket,
            AVG(sending_ms) as avg_send,
            AVG(inclusion_ms) as avg_inc,
            AVG(finalization_ms) as avg_fin,
            COUNT(*) as cnt
         FROM timings
         WHERE timestamp >= ?2
         GROUP BY bucket
         ORDER BY bucket ASC"
    )?;

    let timings = stmt
        .query_map([bucket_ms, start], |row| {
            Ok(AggregatedTiming {
                timestamp: row.get(0)?,
                avg_sending_ms: row.get::<_, f64>(1)? as u64,
                avg_inclusion_ms: row.get::<_, f64>(2)? as u64,
                avg_finalization_ms: row.get::<_, f64>(3)? as u64,
                sample_count: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(timings)
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
