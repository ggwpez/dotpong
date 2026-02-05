use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Timing data from a single transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxTiming {
    /// Unix timestamp in milliseconds when measurement started
    pub timestamp: i64,
    /// Time until transaction was included in best block
    pub inclusion_ms: u64,
    /// Time until transaction was finalized
    pub finalization_ms: u64,
}

impl TxTiming {
    pub fn new(inclusion_ms: u64, finalization_ms: u64) -> Self {
        Self {
            timestamp: unix_ms(),
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
        "INSERT INTO timings (timestamp, inclusion_ms, finalization_ms) VALUES (?1, ?2, ?3)",
        [timing.timestamp, timing.inclusion_ms as i64, timing.finalization_ms as i64],
    )
    .context("Failed to insert timing")?;
    Ok(())
}

pub fn get_timings(conn: &Connection, limit: Option<u32>) -> Result<Vec<TxTiming>> {
    let limit = limit.unwrap_or(1000);
    let mut stmt = conn.prepare(
        "SELECT timestamp, inclusion_ms, finalization_ms FROM timings ORDER BY timestamp DESC LIMIT ?1"
    )?;

    let timings = stmt
        .query_map([limit], |row| {
            Ok(TxTiming {
                timestamp: row.get(0)?,
                inclusion_ms: row.get::<_, i64>(1)? as u64,
                finalization_ms: row.get::<_, i64>(2)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(timings)
}

/// Aggregated timing data for a time bucket
#[derive(Debug, Clone, Serialize)]
pub struct AggregatedTiming {
    /// Start of the time bucket (unix ms)
    pub timestamp: i64,
    /// Average inclusion time in ms
    pub avg_inclusion_ms: u64,
    /// Average finalization time in ms
    pub avg_finalization_ms: u64,
    /// Number of samples in this bucket
    pub sample_count: u32,
}

/// Get hourly aggregates for the last 24 hours
pub fn get_hourly(conn: &Connection) -> Result<Vec<AggregatedTiming>> {
    let hour_ms: i64 = 60 * 60 * 1000;
    let now = unix_ms();
    let start = now - (24 * hour_ms);

    get_aggregated(conn, start, hour_ms)
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
                avg_inclusion_ms: row.get::<_, f64>(1)? as u64,
                avg_finalization_ms: row.get::<_, f64>(2)? as u64,
                sample_count: row.get(3)?,
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
