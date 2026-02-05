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

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
