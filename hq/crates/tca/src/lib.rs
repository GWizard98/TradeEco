use anyhow::Result;
use serde::Serialize;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;

#[derive(Debug, Serialize)]
pub struct TcaRecord<'a> {
    pub symbol: &'a str,
    pub side: &'a str,
    pub exp_price: f64,
    pub fill_price: f64,
    pub qty: f64,
    pub slippage_bps: f64,
    pub regime: Option<&'a str>,
}

pub fn write(record: &TcaRecord) -> Result<()> {
    create_dir_all("logs")?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs/tca.jsonl")?;
    let line = serde_json::to_string(record)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}
