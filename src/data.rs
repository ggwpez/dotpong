use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxResult {
    pub timestamp: i64,
    pub sending_ms: u64,
    pub inclusion_ms: u64,
    pub finalization_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TxResult {
    pub fn ok(sending_ms: u64, inclusion_ms: u64, finalization_ms: u64) -> Self {
        Self {
            timestamp: unix_ms(),
            sending_ms,
            inclusion_ms,
            finalization_ms,
            error: None,
        }
    }

    pub fn err(error: String) -> Self {
        Self {
            timestamp: unix_ms(),
            sending_ms: 0,
            inclusion_ms: 0,
            finalization_ms: 0,
            error: Some(error),
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

    // Add error column idempotently
    match conn.execute("ALTER TABLE timings ADD COLUMN error TEXT", []) {
        Ok(_) => log::info!("Added 'error' column to timings table"),
        Err(e) => {
            let msg = e.to_string();
            if !msg.contains("duplicate column") {
                return Err(e).context("Failed to add error column");
            }
        }
    }

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM timings", [], |row| row.get(0))
        .unwrap_or(0);

    log::info!("Database initialized: {} ({} entries)", db_path, count);
    Ok(conn)
}

pub fn store_result(conn: &Connection, result: &TxResult) -> Result<()> {
    conn.execute(
        "INSERT INTO timings (timestamp, sending_ms, inclusion_ms, finalization_ms, error) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            result.timestamp,
            result.sending_ms as i64,
            result.inclusion_ms as i64,
            result.finalization_ms as i64,
            result.error,
        ],
    )?;
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
    pub error_count: u32,
}

/// Get raw data points for the last 24 hours
pub fn get_hourly(conn: &Connection) -> Result<Vec<TxResult>> {
    let now = unix_ms();
    let start = now - (24 * 60 * 60 * 1000);

    let mut stmt = conn.prepare(
        "SELECT timestamp, sending_ms, inclusion_ms, finalization_ms, error
         FROM timings
         WHERE timestamp >= ?1
         ORDER BY timestamp ASC"
    )?;

    let results = stmt
        .query_map([start], |row| {
            Ok(TxResult {
                timestamp: row.get(0)?,
                sending_ms: row.get::<_, i64>(1)? as u64,
                inclusion_ms: row.get::<_, i64>(2)? as u64,
                finalization_ms: row.get::<_, i64>(3)? as u64,
                error: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
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
            AVG(CASE WHEN error IS NULL THEN sending_ms END) as avg_send,
            AVG(CASE WHEN error IS NULL THEN inclusion_ms END) as avg_inc,
            AVG(CASE WHEN error IS NULL THEN finalization_ms END) as avg_fin,
            SUM(CASE WHEN error IS NULL THEN 1 ELSE 0 END) as ok_cnt,
            SUM(CASE WHEN error IS NOT NULL THEN 1 ELSE 0 END) as err_cnt
         FROM timings
         WHERE timestamp >= ?2
         GROUP BY bucket
         ORDER BY bucket ASC"
    )?;

    let timings = stmt
        .query_map([bucket_ms, start], |row| {
            Ok(AggregatedTiming {
                timestamp: row.get(0)?,
                avg_sending_ms: row.get::<_, Option<f64>>(1)?.unwrap_or(0.0) as u64,
                avg_inclusion_ms: row.get::<_, Option<f64>>(2)?.unwrap_or(0.0) as u64,
                avg_finalization_ms: row.get::<_, Option<f64>>(3)?.unwrap_or(0.0) as u64,
                sample_count: row.get(4)?,
                error_count: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(timings)
}

pub fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
