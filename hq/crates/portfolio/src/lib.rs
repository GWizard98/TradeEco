use std::collections::HashMap;

pub fn adjust_qty(
    symbol: &str,
    price: f64,
    proposed_qty: f64,
    _account_equity: f64,
    symbol_cap: f64,
    portfolio_cap: f64,
) -> f64 {
    adjust_qty_with_prices(
        symbol,
        price,
        proposed_qty,
        symbol_cap,
        portfolio_cap,
        &HashMap::new(),
    )
}

pub fn adjust_qty_with_prices(
    symbol: &str,
    symbol_price: f64,
    proposed_qty: f64,
    symbol_cap: f64,
    portfolio_cap: f64,
    price_map: &HashMap<String, f64>,
) -> f64 {
    if symbol_price <= 0.0 || proposed_qty <= 0.0 {
        return proposed_qty.max(0.0);
    }
    // Compute current notionals from ledger fills joined with orders to get side
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
    let conn = rusqlite::Connection::open(ledger_db_path());
    let (mut symbol_notional, mut total_notional) = (0.0_f64, 0.0_f64);
    if let Ok(c) = conn {
        let q = "SELECT o.symbol, SUM(CASE WHEN o.side='SELL' THEN -f.qty ELSE f.qty END) as net_qty FROM fills f JOIN orders o ON f.id = o.id GROUP BY o.symbol";
        if let Ok(mut s) = c.prepare(q) {
            if let Ok(mut rs) = s.query([]) {
                while let Ok(Some(row)) = rs.next() {
                    let sym: String = row.get(0).unwrap_or_default();
                    let net_qty: f64 = row
                        .get::<usize, Option<f64>>(1)
                        .unwrap_or(Some(0.0))
                        .unwrap_or(0.0);
                    let p = if sym == symbol {
                        symbol_price
                    } else {
                        price_map.get(&sym).copied().unwrap_or(symbol_price)
                    };
                    let notional = net_qty.abs() * p.max(0.0);
                    if sym == symbol {
                        symbol_notional = notional;
                    }
                    total_notional += notional;
                }
            }
        }
    }
    // Desired notional for this order
    let desired = proposed_qty * symbol_price;
    let max_symbol = symbol_cap.max(0.0);
    let max_portfolio = portfolio_cap.max(0.0);
    let allowed_symbol = (max_symbol - symbol_notional).max(0.0);
    let allowed_portfolio = (max_portfolio - total_notional).max(0.0);
    let allowed = allowed_symbol.min(allowed_portfolio);
    if allowed <= 0.0 {
        return 0.0;
    }
    let adjusted_notional = desired.min(allowed);
    (adjusted_notional / symbol_price).floor() // round down to units; Pathfinder will enforce lot step
}
