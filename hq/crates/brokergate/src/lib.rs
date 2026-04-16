use anyhow::{anyhow, Result};
use serde::Serialize;

pub mod oanda {
    use super::*;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    fn build_client() -> Result<reqwest::blocking::Client> {
        let mut builder = reqwest::blocking::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20));
        if let Ok(cert_path) = std::env::var("OANDA_CERT_PATH") {
            if let Ok(pem) = std::fs::read(&cert_path) {
                let cert = reqwest::Certificate::from_pem(&pem)
                    .map_err(|e| anyhow!("invalid PEM at {}: {}", cert_path, e))?;
                builder = builder.add_root_certificate(cert);
            }
        }
        builder = builder.user_agent("TradeEco-BrokerGate/0.1");
        builder
            .build()
            .map_err(|e| anyhow!("tls client build: {}", e))
    }

    #[derive(Serialize)]
    struct OrderRequest {
        order: Order,
    }
    #[derive(Serialize)]
    struct Order {
        instrument: String,
        units: String,
        r#type: String,
        #[serde(rename = "timeInForce")]
        time_in_force: String,
        #[serde(rename = "positionFill")]
        position_fill: String,
        #[serde(rename = "clientExtensions")]
        client_extensions: ClientExt,
        #[serde(rename = "takeProfitOnFill", skip_serializing_if = "Option::is_none")]
        take_profit_on_fill: Option<TpOnFill>,
        #[serde(rename = "stopLossOnFill", skip_serializing_if = "Option::is_none")]
        stop_loss_on_fill: Option<SlOnFill>,
        #[serde(skip_serializing_if = "Option::is_none")]
        price: Option<String>,
    }
    #[derive(Serialize)]
    struct ClientExt {
        #[serde(rename = "clientOrderID")]
        client_order_id: String,
    }
    #[derive(Serialize)]
    struct TpOnFill {
        price: String,
    }
    #[derive(Serialize)]
    struct SlOnFill {
        price: String,
    }

    pub fn submit_market(
        order: &api::OrderRequest,
        _spec: &api::InstrumentSpec,
    ) -> Result<api::ExecReport> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }

        // Instrument mapping (assumes symbol is valid Oanda instrument name)
        let instrument = order.symbol.clone();
        let units = match order.side {
            api::OrderSide::Buy => order.qty,
            api::OrderSide::Sell => -order.qty,
        };
        let body = OrderRequest {
            order: Order {
                instrument,
                units: format!("{}", units as i64),
                r#type: "MARKET".into(),
                time_in_force: "FOK".into(),
                position_fill: "DEFAULT".into(),
                client_extensions: ClientExt {
                    client_order_id: order.idempotency_key.clone(),
                },
                take_profit_on_fill: order.take_profit.map(|p| TpOnFill {
                    price: format!("{}", p),
                }),
                stop_loss_on_fill: order.stop_loss.map(|p| SlOnFill {
                    price: format!("{}", p),
                }),
                price: None,
            },
        };
        let url = format!("https://{}/v3/accounts/{}/orders", host, account);
        let client = build_client()?;

        // Simple retry with backoff
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let send = client
                .post(&url)
                .bearer_auth(&api_key)
                .header("Accept", "application/json")
                .json(&body)
                .send();
            match send {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(api::ExecReport {
                            idempotency_key: order.idempotency_key.clone(),
                            symbol: order.symbol.clone(),
                            filled_qty: order.qty,
                            avg_price: 0.0,
                            slippage_bps: 0.0,
                            status: "SUBMITTED".into(),
                        });
                    } else {
                        last_err =
                            Some(anyhow!("oanda error: {}", resp.text().unwrap_or_default()));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!("oanda send: {}", e));
                }
            }
            std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("oanda unknown error")))
    }
    
    pub fn submit_limit(
        order: &api::OrderRequest,
        limit_price: f64,
        _spec: &api::InstrumentSpec,
    ) -> Result<api::ExecReport> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }

        let instrument = order.symbol.clone();
        let units = match order.side {
            api::OrderSide::Buy => order.qty,
            api::OrderSide::Sell => -order.qty,
        };
        
        let body = OrderRequest {
            order: Order {
                instrument,
                units: format!("{}", units as i64),
                r#type: "LIMIT".into(),
                time_in_force: "GTC".into(), // Good Till Cancelled
                position_fill: "DEFAULT".into(),
                client_extensions: ClientExt {
                    client_order_id: order.idempotency_key.clone(),
                },
                take_profit_on_fill: order.take_profit.map(|p| TpOnFill {
                    price: format!("{}", p),
                }),
                stop_loss_on_fill: order.stop_loss.map(|p| SlOnFill {
                    price: format!("{}", p),
                }),
                price: Some(format!("{}", limit_price)),
            },
        };
        
        let url = format!("https://{}/v3/accounts/{}/orders", host, account);
        let client = build_client()?;
        
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let send = client
                .post(&url)
                .bearer_auth(&api_key)
                .header("Accept", "application/json")
                .json(&body)
                .send();
            match send {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(api::ExecReport {
                            idempotency_key: order.idempotency_key.clone(),
                            symbol: order.symbol.clone(),
                            filled_qty: 0.0, // Limit orders may not fill immediately
                            avg_price: limit_price,
                            slippage_bps: 0.0,
                            status: "PENDING".into(),
                        });
                    } else {
                        last_err =
                            Some(anyhow!("oanda limit error: {}", resp.text().unwrap_or_default()));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!("oanda limit send: {}", e));
                }
            }
            std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("oanda limit unknown error")))
    }
    
    pub fn submit_ioc(
        order: &api::OrderRequest,
        limit_price: f64,
        _spec: &api::InstrumentSpec,
    ) -> Result<api::ExecReport> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }

        let instrument = order.symbol.clone();
        let units = match order.side {
            api::OrderSide::Buy => order.qty,
            api::OrderSide::Sell => -order.qty,
        };
        
        let body = OrderRequest {
            order: Order {
                instrument,
                units: format!("{}", units as i64),
                r#type: "LIMIT".into(),
                time_in_force: "IOC".into(), // Immediate or Cancel
                position_fill: "DEFAULT".into(),
                client_extensions: ClientExt {
                    client_order_id: order.idempotency_key.clone(),
                },
                take_profit_on_fill: order.take_profit.map(|p| TpOnFill {
                    price: format!("{}", p),
                }),
                stop_loss_on_fill: order.stop_loss.map(|p| SlOnFill {
                    price: format!("{}", p),
                }),
                price: Some(format!("{}", limit_price)),
            },
        };
        
        let url = format!("https://{}/v3/accounts/{}/orders", host, account);
        let client = build_client()?;
        
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let send = client
                .post(&url)
                .bearer_auth(&api_key)
                .header("Accept", "application/json")
                .json(&body)
                .send();
            match send {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(api::ExecReport {
                            idempotency_key: order.idempotency_key.clone(),
                            symbol: order.symbol.clone(),
                            filled_qty: order.qty, // IOC fills immediately or cancels
                            avg_price: limit_price,
                            slippage_bps: 0.0,
                            status: "FILLED_OR_CANCELLED".into(),
                        });
                    } else {
                        last_err =
                            Some(anyhow!("oanda ioc error: {}", resp.text().unwrap_or_default()));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!("oanda ioc send: {}", e));
                }
            }
            std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("oanda ioc unknown error")))
    }

    static NET_POS: Lazy<Mutex<HashMap<String, f64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

    pub fn get_net_positions() -> HashMap<String, f64> {
        NET_POS.lock().unwrap().clone()
    }

    pub fn start_transactions_stream() -> Result<()> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }
        let url = format!(
            "https://{}/v3/accounts/{}/transactions/stream",
            host, account
        );
        std::thread::spawn(move || {
            if let Ok(client) = build_client() {
                if let Ok(resp) = client
                    .get(&url)
                    .bearer_auth(api_key)
                    .header("Accept", "application/json")
                    .send()
                {
                    if let Ok(buf) = resp.text() {
                        for l in buf.lines() {
                            if l.trim().is_empty() {
                                continue;
                            }
                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(l) {
                                if val.get("type").and_then(|t| t.as_str()) == Some("ORDER_FILL") {
                                    let instrument = val
                                        .get("instrument")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let units = val
                                        .get("units")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("0")
                                        .parse::<f64>()
                                        .unwrap_or(0.0);
                                    let price = val
                                        .get("price")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(0.0);
                                    {
                                        let mut net = NET_POS.lock().unwrap();
                                        *net.entry(instrument.clone()).or_insert(0.0) += units;
                                    }
                                    let rec = serde_json::json!({
                                        "type": "ORDER_FILL",
                                        "instrument": instrument,
                                        "units": units,
                                        "price": price,
                                        "ts_ms": chrono::Utc::now().timestamp_millis()
                                    });
                                    let _ = std::fs::create_dir_all("logs");
                                    if let Ok(mut f) = std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open("logs/broker_fills.jsonl")
                                    {
                                        let _ = std::io::Write::write_all(
                                            &mut f,
                                            rec.to_string().as_bytes(),
                                        );
                                        let _ = std::io::Write::write_all(&mut f, b"\n");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(())
    }

    pub fn fetch_open_positions() -> Result<serde_json::Value> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }
        let url = format!("https://{}/v3/accounts/{}/openPositions", host, account);
        let client = build_client()?;
        // One retry for GET
        for attempt in 0..2 {
            let resp = client
                .get(&url)
                .bearer_auth(&api_key)
                .header("Accept", "application/json")
                .send();
            match resp {
                Ok(r) => {
                    if !r.status().is_success() {
                        return Err(anyhow!(r.text().unwrap_or_default()));
                    }
                    let json: serde_json::Value = r.json()?;
                    return Ok(json);
                }
                Err(e) => {
                    if attempt == 1 {
                        return Err(anyhow!("oanda get positions: {}", e));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
        Err(anyhow!("oanda get positions failed"))
    }
    // Fetch current pricing (bid/ask) for an instrument
    pub fn fetch_pricing(symbol: &str) -> Result<(f64, f64)> {
        let api_key =
            std::env::var("OANDA_API_KEY").map_err(|_| anyhow!("missing OANDA_API_KEY"))?;
        let account =
            std::env::var("OANDA_ACCOUNT_ID").map_err(|_| anyhow!("missing OANDA_ACCOUNT_ID"))?;
        let host =
            std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }
        let client = build_client()?;
        let url = format!(
            "https://{}/v3/accounts/{}/pricing?instruments={}",
            host, account, symbol
        );
        let resp = client
            .get(&url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(resp.text().unwrap_or_default()));
        }
        let json: serde_json::Value = resp.json()?;
        let prices = json
            .get("prices")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if prices.is_empty() {
            return Err(anyhow!("no pricing"));
        }
        let p0 = &prices[0];
        let bid = p0
            .get("bids")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|q| q.get("price"))
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let ask = p0
            .get("asks")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|q| q.get("price"))
            .and_then(|s| s.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        if bid <= 0.0 || ask <= 0.0 {
            return Err(anyhow!("invalid pricing"));
        }
        Ok((bid, ask))
    }
}

pub mod coinexx {
    use super::*;
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Duration;

    fn build_client() -> Result<reqwest::blocking::Client> {
        let mut builder = reqwest::blocking::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent("TradeEco-BrokerGate/0.1");
        if let Ok(cert_path) = std::env::var("COINEXX_CERT_PATH") {
            if let Ok(pem) = std::fs::read(&cert_path) {
                if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                    builder = builder.add_root_certificate(cert);
                }
            }
        }
        builder
            .build()
            .map_err(|e| anyhow!("tls client build: {}", e))
    }

    static NET_POS: Lazy<Mutex<HashMap<String, f64>>> = Lazy::new(|| Mutex::new(HashMap::new()));

    pub fn get_net_positions() -> HashMap<String, f64> {
        NET_POS.lock().unwrap().clone()
    }

    pub fn submit_market(
        order: &api::OrderRequest,
        _spec: &api::InstrumentSpec,
    ) -> Result<api::ExecReport> {
        let api_key =
            std::env::var("COINEXX_API_KEY").map_err(|_| anyhow!("missing COINEXX_API_KEY"))?;
        let account = std::env::var("COINEXX_ACCOUNT_ID")
            .map_err(|_| anyhow!("missing COINEXX_ACCOUNT_ID"))?;
        let host = std::env::var("COINEXX_HOST").unwrap_or_else(|_| "api.coinexx.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }

        let instrument = order.symbol.clone();
        let side = match order.side {
            api::OrderSide::Buy => "BUY",
            api::OrderSide::Sell => "SELL",
        };
        
        // Coinexx API structure (based on MT4/MT5 broker APIs)
        let body = serde_json::json!({
            "accountId": account,
            "symbol": instrument,
            "volume": order.qty,
            "cmd": if side == "BUY" { 0 } else { 1 }, // 0=BUY, 1=SELL for market orders
            "type": 0, // 0=market, 1=pending
            "comment": format!("CG-{}", order.idempotency_key),
            "sl": order.stop_loss.unwrap_or(0.0),
            "tp": order.take_profit.unwrap_or(0.0),
            "magic": 12345
        });
        
        let url = format!("https://{}/api/v1/trade/market_order", host);
        let client = build_client()?;
        
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .json(&body)
                .send();
            match resp {
                Ok(r) => {
                    if r.status().is_success() {
                        // Parse response to get actual fill data
                        if let Ok(resp_json) = r.json::<serde_json::Value>() {
                            let fill_price = resp_json
                                .get("price")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            let actual_qty = resp_json
                                .get("volume")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(order.qty);
                            let order_id = resp_json
                                .get("ticket")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&order.idempotency_key);
                            
                            // Update position tracking
                            {
                                let mut net = NET_POS.lock().unwrap();
                                let pos_change = if side == "BUY" { actual_qty } else { -actual_qty };
                                *net.entry(instrument.clone()).or_insert(0.0) += pos_change;
                            }
                            
                            return Ok(api::ExecReport {
                                idempotency_key: order_id.to_string(),
                                symbol: order.symbol.clone(),
                                filled_qty: actual_qty,
                                avg_price: fill_price,
                                slippage_bps: 0.0, // Calculate later if needed
                                status: "FILLED".into(),
                            });
                        } else {
                            return Ok(api::ExecReport {
                                idempotency_key: order.idempotency_key.clone(),
                                symbol: order.symbol.clone(),
                                filled_qty: order.qty,
                                avg_price: 0.0,
                                slippage_bps: 0.0,
                                status: "SUBMITTED".into(),
                            });
                        }
                    } else {
                        let status = r.status();
                        let error_text = r.text().unwrap_or_default();
                        last_err = Some(anyhow!("coinexx error {}: {}", status, error_text));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!("coinexx send: {}", e));
                }
            }
            std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("coinexx unknown error")))
    }
    
    pub fn submit_limit(
        order: &api::OrderRequest,
        limit_price: f64,
        _spec: &api::InstrumentSpec,
    ) -> Result<api::ExecReport> {
        let api_key =
            std::env::var("COINEXX_API_KEY").map_err(|_| anyhow!("missing COINEXX_API_KEY"))?;
        let account = std::env::var("COINEXX_ACCOUNT_ID")
            .map_err(|_| anyhow!("missing COINEXX_ACCOUNT_ID"))?;
        let host = std::env::var("COINEXX_HOST").unwrap_or_else(|_| "api.coinexx.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }

        let instrument = order.symbol.clone();
        let side = match order.side {
            api::OrderSide::Buy => "BUY",
            api::OrderSide::Sell => "SELL",
        };
        
        let body = serde_json::json!({
            "accountId": account,
            "symbol": instrument,
            "volume": order.qty,
            "cmd": if side == "BUY" { 2 } else { 3 }, // 2=BUY_LIMIT, 3=SELL_LIMIT
            "type": 1, // pending order
            "price": limit_price,
            "comment": format!("CG-LIMIT-{}", order.idempotency_key),
            "sl": order.stop_loss.unwrap_or(0.0),
            "tp": order.take_profit.unwrap_or(0.0),
            "magic": 12345,
            "expiration": 0 // GTC
        });
        
        let url = format!("https://{}/api/v1/trade/pending_order", host);
        let client = build_client()?;
        
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .json(&body)
                .send();
            match resp {
                Ok(r) => {
                    if r.status().is_success() {
                        return Ok(api::ExecReport {
                            idempotency_key: order.idempotency_key.clone(),
                            symbol: order.symbol.clone(),
                            filled_qty: 0.0,
                            avg_price: limit_price,
                            slippage_bps: 0.0,
                            status: "PENDING".into(),
                        });
                    } else {
                        let status = r.status();
                        let error_text = r.text().unwrap_or_default();
                        last_err = Some(anyhow!("coinexx limit error {}: {}", status, error_text));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!("coinexx limit send: {}", e));
                }
            }
            std::thread::sleep(Duration::from_millis(200 * (1 << attempt)));
        }
        Err(last_err.unwrap_or_else(|| anyhow!("coinexx limit unknown error")))
    }
    
    pub fn fetch_pricing(symbol: &str) -> Result<(f64, f64)> {
        let api_key =
            std::env::var("COINEXX_API_KEY").map_err(|_| anyhow!("missing COINEXX_API_KEY"))?;
        let host = std::env::var("COINEXX_HOST").unwrap_or_else(|_| "api.coinexx.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }
        
        let client = build_client()?;
        let url = format!("https://{}/api/v1/market/symbol_info?symbol={}", host, symbol);
        
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .send()?;
            
        if !resp.status().is_success() {
            return Err(anyhow!(resp.text().unwrap_or_default()));
        }
        
        let json: serde_json::Value = resp.json()?;
        let bid = json.get("bid").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let ask = json.get("ask").and_then(|v| v.as_f64()).unwrap_or(0.0);
        
        if bid <= 0.0 || ask <= 0.0 {
            return Err(anyhow!("invalid pricing data"));
        }
        
        Ok((bid, ask))
    }

    pub fn fetch_open_positions() -> Result<serde_json::Value> {
        let api_key =
            std::env::var("COINEXX_API_KEY").map_err(|_| anyhow!("missing COINEXX_API_KEY"))?;
        let account = std::env::var("COINEXX_ACCOUNT_ID")
            .map_err(|_| anyhow!("missing COINEXX_ACCOUNT_ID"))?;
        let host = std::env::var("COINEXX_HOST").unwrap_or_else(|_| "api.coinexx.com".into());
        if !guardian::egress_allowed(&host) {
            return Err(anyhow!("egress not allowed"));
        }
        let url = format!("https://{}/v1/accounts/{}/positions", host, account);
        let client = build_client()?;
        let resp = client
            .get(&url)
            .bearer_auth(&api_key)
            .header("Accept", "application/json")
            .send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(resp.text().unwrap_or_default()));
        }
        let json: serde_json::Value = resp.json()?;
        Ok(json)
    }

    pub fn start_transactions_stream() -> Result<()> {
        // Placeholder: no-op or poll endpoint if available
        Ok(())
    }
}
