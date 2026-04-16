use anyhow::Result;
use rusqlite::{params, Connection};

fn ledger_db_path() -> String {
    if let Ok(p) = std::env::var("LEDGER_DB_PATH") {
        return p;
    }
    let mode = std::env::var("HQ_MODE").unwrap_or_default();
    if mode.is_empty() {
        return "logs/trade_ledger.db".into();
    }
    let suffix = match mode.as_str() {
        "backtest" => "backtest",
        "paper" => "paper",
        "live_oanda" => "live_oanda",
        _ => "default",
    };
    format!("logs/trade_ledger_{}.db", suffix)
}

fn conn() -> Result<Connection> {
    std::fs::create_dir_all("logs").ok();
    let c = Connection::open(ledger_db_path())?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
            id TEXT PRIMARY KEY,
            ts_ms INTEGER,
            symbol TEXT,
            side TEXT,
            qty REAL,
            price REAL,
            status TEXT,
            source TEXT
        );
        CREATE TABLE IF NOT EXISTS fills (
            id TEXT,
            ts_ms INTEGER,
            symbol TEXT,
            qty REAL,
            price REAL,
            source TEXT
        );",
    )?;
    Ok(c)
}

pub struct OrderRecord {
    pub idempotency_key: String,
    pub ts_ms: i64,
    pub symbol: String,
    pub side: String,
    pub qty: f64,
    pub price: f64,
    pub status: String,
    pub source: String,
}

pub struct FillRecord {
    pub idempotency_key: String,
    pub ts_ms: i64,
    pub symbol: String,
    pub qty: f64,
    pub price: f64,
    pub source: String,
}

pub fn record_order(or: &OrderRecord) -> Result<()> {
    let c = conn()?;
    c.execute(
        "INSERT OR REPLACE INTO orders(id, ts_ms, symbol, side, qty, price, status, source)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            or.idempotency_key,
            or.ts_ms,
            or.symbol,
            or.side,
            or.qty,
            or.price,
            or.status,
            or.source
        ],
    )?;
    Ok(())
}

pub fn record_fill(fr: &FillRecord) -> Result<()> {
    let c = conn()?;
    c.execute(
        "INSERT INTO fills(id, ts_ms, symbol, qty, price, source)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            fr.idempotency_key,
            fr.ts_ms,
            fr.symbol,
            fr.qty,
            fr.price,
            fr.source
        ],
    )?;
    Ok(())
}
