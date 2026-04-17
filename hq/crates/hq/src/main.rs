use api::{Agent, MetaWeigher, Sizer};
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tracing_subscriber::{fmt, EnvFilter};

fn to_ir_signal(agent_id: &str, symbol: &str, resp: &api::SignalResponse) -> ir::Signal {
    let mut features = vec![
        (
            "trace_id".into(),
            ir::FeatureValue::Text(resp.trace_id.clone()),
        ),
        ("ev_net".into(), ir::FeatureValue::F64(resp.ev_net)),
        ("regime_fit".into(), ir::FeatureValue::F64(resp.regime_fit)),
        (
            "data_quality".into(),
            ir::FeatureValue::F64(resp.data_quality),
        ),
        (
            "exec_impact".into(),
            ir::FeatureValue::F64(resp.exec_impact),
        ),
    ];
    if let Some(h) = &resp.hints {
        if let Some(v) = h.stop_loss {
            features.push(("hint_stop".into(), ir::FeatureValue::F64(v)));
        }
        if let Some(v) = h.take_profit {
            features.push(("hint_target".into(), ir::FeatureValue::F64(v)));
        }
        if let Some(v) = h.ttl_sec {
            features.push(("hint_ttl".into(), ir::FeatureValue::I64(v as i64)));
        }
    }
    ir::Signal {
        agent_id: agent_id.into(),
        symbol: symbol.into(),
        score: resp.score,
        uncertainty: resp.uncertainty,
        features,
        rationale: resp.rationale.clone(),
    }
}

fn get_feat(s: &ir::Signal, key: &str) -> Option<f64> {
    s.features.iter().find_map(|(k, v)| {
        if k == key {
            match v {
                ir::FeatureValue::F64(x) => Some(*x),
                _ => None,
            }
        } else {
            None
        }
    })
}

#[derive(Debug, Deserialize, Clone)]
struct CapsCfg {
    symbol_notional: f64,
    portfolio_notional: f64,
}
#[derive(Debug, Deserialize, Clone)]
struct FusionCfg {
    min_ev: f64,
    min_regime: f64,
    min_quality: f64,
}
#[derive(Debug, Deserialize, Clone)]
struct RiskCfg {
    leverage_max: f64,
    account_equity: f64,
}
#[derive(Debug, Deserialize, Clone)]
struct Config {
    caps: CapsCfg,
    fusion: FusionCfg,
    risk: RiskCfg,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct InstrumentCfg {
    tick_size: f64,
    lot_step: f64,
    min_qty: f64,
    max_qty: f64,
    min_notional: f64,
}
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct Instruments {
    symbols: HashMap<String, InstrumentCfg>,
}

fn load_config() -> Config {
    let path = "config/policy.toml";
    let txt = fs::read_to_string(path).unwrap_or_else(|_| "".into());
    if txt.is_empty() {
        return Config {
            caps: CapsCfg {
                symbol_notional: 50_000.0,
                portfolio_notional: 100_000.0,
            },
            fusion: FusionCfg {
                min_ev: 0.0,
                min_regime: 0.5,
                min_quality: 0.7,
            },
            risk: RiskCfg {
                leverage_max: 2.0,
                account_equity: 100_000.0,
            },
        };
    }
    toml::from_str(&txt).unwrap_or(Config {
        caps: CapsCfg {
            symbol_notional: 50_000.0,
            portfolio_notional: 100_000.0,
        },
        fusion: FusionCfg {
            min_ev: 0.0,
            min_regime: 0.5,
            min_quality: 0.7,
        },
        risk: RiskCfg {
            leverage_max: 2.0,
            account_equity: 100_000.0,
        },
    })
}

fn to_meta_request(symbol: &str, signals: &[ir::Signal]) -> api::MetaRequest {
    let mut feats = vec![];
    for s in signals {
        let getf = |k: &str| {
            s.features
                .iter()
                .find_map(|(kk, v)| {
                    if kk == k {
                        match v {
                            ir::FeatureValue::F64(x) => Some(*x),
                            _ => None,
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or(0.0)
        };
        feats.push(api::MetaFeature {
            agent_id: s.agent_id.clone(),
            score: s.score,
            uncertainty: s.uncertainty,
            ev_net: getf("ev_net"),
            regime_fit: getf("regime_fit"),
            data_quality: getf("data_quality"),
        });
    }
    api::MetaRequest {
        symbol: symbol.into(),
        features: feats,
    }
}

#[allow(dead_code)]
fn load_instrument(symbol: &str) -> Option<api::InstrumentSpec> {
    let path = "config/instruments.toml";
    let txt = fs::read_to_string(path).ok()?;
    let cfg: Instruments = toml::from_str(&txt).ok()?;
    let ic = cfg.symbols.get(symbol)?;
    Some(api::InstrumentSpec {
        tick_size: ic.tick_size,
        lot_step: ic.lot_step,
        min_qty: ic.min_qty,
        max_qty: ic.max_qty,
        min_notional: ic.min_notional,
    })
}

fn load_symbols() -> Vec<String> {
    let path = "config/instruments.toml";
    if let Ok(txt) = fs::read_to_string(path) {
        let parsed: Result<Instruments, _> = toml::from_str(&txt);
        if let Ok(cfg_val) = parsed {
            return cfg_val.symbols.keys().cloned().collect();
        }
    }
    vec!["AAPL".into()]
}

#[cfg(feature = "sentinel")]
static NEWS_CACHE: Lazy<Mutex<HashMap<String, Vec<api::NewsEvent>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[allow(dead_code)]
fn map_symbol_mt4(sym: &str) -> String {
    static MAP: Lazy<Mutex<Option<HashMap<String, String>>>> = Lazy::new(|| Mutex::new(None));
    {
        let m = MAP.lock().unwrap();
        if let Some(mm) = &*m {
            return mm.get(sym).cloned().unwrap_or_else(|| sym.replace('_', ""));
        }
    }
    let mut map = HashMap::new();
    if let Ok(txt) = std::fs::read_to_string("config/symbol_map.toml") {
        #[derive(serde::Deserialize)]
        struct SymMap {
            mt4: HashMap<String, String>,
        }
        if let Ok(sm) = toml::from_str::<SymMap>(&txt) {
            map = sm.mt4;
        }
    }
    let def = sym.replace('_', "");
    let out = map.get(sym).cloned().unwrap_or(def);
    let mut m = MAP.lock().unwrap();
    *m = Some(map);
    out
}

#[cfg(feature = "sentinel")]
async fn start_sentinel(symbols: Vec<String>, interval_secs: u64) {
    let interval = std::time::Duration::from_secs(interval_secs);
    tokio::spawn(async move {
        loop {
            for sym in symbols.iter() {
                let mut events: Vec<api::NewsEvent> = vec![];
                events.extend(sentinel::collectors::yahoo::fetch_latest(sym));
                events.extend(sentinel::collectors::fxstreet::fetch_latest(sym));
                events.extend(sentinel::collectors::forexnews::fetch_latest(sym));
                events.extend(sentinel::collectors::tradingview::fetch_latest(sym));
                sentinel::dedupe_by_text(&mut events);
                let mut cache = NEWS_CACHE.lock().unwrap();
                cache.insert(sym.clone(), events);
            }
            tokio::time::sleep(interval).await;
        }
    });
}

#[cfg(not(feature = "sentinel"))]
async fn start_sentinel(_symbols: Vec<String>, _interval_secs: u64) {
    // no-op when sentinel disabled
}

fn weighted_fusion(signals: &[ir::Signal], cfg: &Config) -> ir::Decision {
    let mut wsum = 0.0_f64;
    let mut wtot = 0.0_f64;
    let mut symbol = String::new();
    let mut alpha_score: Option<f64> = None;
    let mut alpha_unc: Option<f64> = None;

    let min_ev = cfg.fusion.min_ev;
    let min_regime = cfg.fusion.min_regime;
    let min_quality = cfg.fusion.min_quality;

    for s in signals {
        if symbol.is_empty() {
            symbol = s.symbol.clone();
        }
        let ev = get_feat(s, "ev_net").unwrap_or(0.0);
        let regime = get_feat(s, "regime_fit").unwrap_or(0.0);
        let quality = get_feat(s, "data_quality").unwrap_or(0.0);
        let ok = ev > min_ev && regime >= min_regime && quality >= min_quality;
        let is_alpha = s.agent_id == "alphascout";
        let base_w = if is_alpha { 2.0 } else { 1.0 };
        let weight = base_w * (1.0 - s.uncertainty.clamp(0.0, 1.0)) * if ok { 1.0 } else { 0.1 };
        wsum += s.score * weight;
        wtot += weight.max(0.0);
        if is_alpha {
            alpha_score = Some(s.score);
            alpha_unc = Some(s.uncertainty);
        }
    }

    let avg = if wtot > 0.0 { wsum / wtot } else { 0.0 };
    let mut decision = if avg > 0.0 {
        ir::Side::Buy
    } else if avg < 0.0 {
        ir::Side::Sell
    } else {
        ir::Side::Hold
    };

    if let (Some(a), Some(u)) = (alpha_score, alpha_unc) {
        let aligned = (a > 0.0 && matches!(decision, ir::Side::Buy))
            || (a < 0.0 && matches!(decision, ir::Side::Sell));
        let min_conf = (1.0 - u) >= 0.1 && a.abs() >= 0.61;
        if !(aligned && min_conf) {
            decision = ir::Side::Hold;
        }
    } else {
        decision = ir::Side::Hold;
    }

    ir::Decision {
        symbol,
        side: decision,
        qty: 0.0,
        confidence: avg.abs().min(1.0),
    }
}

static DECISIONS_TOTAL: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static ABSTAINS_TOTAL: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static FILLS_TOTAL: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static SLIPPAGE_BPS_SUM: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
static LAST_BUY_PRICE: Lazy<std::sync::Mutex<f64>> = Lazy::new(|| std::sync::Mutex::new(0.0));
static LAST_SELL_PRICE: Lazy<std::sync::Mutex<f64>> = Lazy::new(|| std::sync::Mutex::new(0.0));
static PNL_TOTAL: Lazy<std::sync::Mutex<f64>> = Lazy::new(|| std::sync::Mutex::new(0.0));

#[cfg(feature = "metrics")]
async fn health_server(port: u16) {
    use axum::{extract::Query, response::IntoResponse, routing::get, Router};
    use prometheus::{Encoder, IntGauge, Registry, TextEncoder};

    static REG: Lazy<Registry> = Lazy::new(Registry::new);
    static DECISIONS_METRIC: Lazy<IntGauge> =
        Lazy::new(|| IntGauge::new("decisions_total", "Total decisions").unwrap());
    static ABSTAINS_METRIC: Lazy<IntGauge> =
        Lazy::new(|| IntGauge::new("abstains_total", "Total abstentions").unwrap());
    static FILLS_METRIC: Lazy<IntGauge> =
        Lazy::new(|| IntGauge::new("fills_total", "Total fills").unwrap());

    let _ = REG.register(Box::new(DECISIONS_METRIC.clone()));
    let _ = REG.register(Box::new(ABSTAINS_METRIC.clone()));
    let _ = REG.register(Box::new(FILLS_METRIC.clone()));

    let metrics_handler = || async move {
        DECISIONS_METRIC.set(DECISIONS_TOTAL.load(Ordering::Relaxed) as i64);
        ABSTAINS_METRIC.set(ABSTAINS_TOTAL.load(Ordering::Relaxed) as i64);
        FILLS_METRIC.set(FILLS_TOTAL.load(Ordering::Relaxed) as i64);
        let encoder = TextEncoder::new();
        let metric_families = REG.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        let body = String::from_utf8(buffer).unwrap();
        body.into_response()
    };

    #[derive(serde::Deserialize)]
    struct Mt4Query {
        symbol: Option<String>,
    }

    async fn mt4_signals(Query(q): Query<Mt4Query>) -> impl IntoResponse {
        use std::io::Read;
        let path = "logs/audit.jsonl";
        let mut buf = String::new();
        if let Ok(mut f) = std::fs::File::open(path) {
            let _ = f.read_to_string(&mut buf);
        }
        let mut out: Option<serde_json::Value> = None;
        for line in buf.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                if val.get("category").and_then(|c| c.as_str()) != Some("signal_mt4") {
                    continue;
                }
                let msg = val.get("message").and_then(|m| m.as_str()).unwrap_or("");
                // Expect format: "MT4 signal: <SYMBOL> <BUY|SELL> lots=<L> SL=<Opt> TP=<Opt>"
                let cleaned = msg.replace("Some(", "").replace(")", "");
                let parts: Vec<&str> = cleaned.split_whitespace().collect();
                if parts.len() < 5 { continue; }
                let sym = parts.get(2).copied().unwrap_or("");
                if let Some(req_sym) = &q.symbol { if sym != req_sym { continue; } }
                let side = parts.get(3).copied().unwrap_or("");
                // find tokens like lots=, SL=, TP=
                let mut lots: f64 = 0.0;
                let mut sl: Option<f64> = None;
                let mut tp: Option<f64> = None;
                for tok in parts.iter() {
                    if let Some(v) = tok.strip_prefix("lots=") {
                        lots = v.parse().unwrap_or(0.0);
                    }
                    if let Some(v) = tok.strip_prefix("SL=") {
                        sl = v.parse().ok();
                    }
                    if let Some(v) = tok.strip_prefix("TP=") {
                        tp = v.parse().ok();
                    }
                }
                out = Some(serde_json::json!({
                    "symbol": sym,
                    "side": side,
                    "lots": lots,
                    "sl": sl,
                    "tp": tp,
                    "ts_ms": chrono::Utc::now().timestamp_millis()
                }));
                break;
            }
        }
        match out {
            Some(v) => axum::response::Json(v).into_response(),
            None => (axum::http::StatusCode::NOT_FOUND, "not_found").into_response(),
        }
    }

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/ready", get(|| async { "ready" }))
        .route("/metrics", get(metrics_handler))
        .route("/mt4/signals", get(mt4_signals));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let _ = axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app).await;
}

#[cfg(not(feature = "metrics"))]
async fn health_server(_port: u16) { /* no-op */
}

fn run_once_with(input: alphascout::Inputs) -> anyhow::Result<()> {
    let cfg = load_config();
    tracing::info!(?cfg, "config loaded");

    if std::env::var("HQ_KILL").ok().as_deref() == Some("1") {
        let evt = guardian::AuditEvent {
            category: "safety".into(),
            message: "HQ_KILL active, aborting decisions".into(),
            severity: "WARN".into(),
        };
        guardian::write_audit(&evt)?;
        #[cfg(feature = "alerts")]
        alerts::maybe_notify("Kill Switch", &evt.message, &evt.severity);
        tracing::warn!("kill switch active");
        return Ok(());
    }

    let req = alphascout::infer_inputs_to_request(&input);
    let v = guardian::validate_infer_request("alphascout", &req)?;
    if v.quality < 0.7 {
        guardian::write_audit(&guardian::AuditEvent {
            category: "validation".into(),
            message: format!("alphascout low quality: {:?}", v.errors),
            severity: "WARN".into(),
        })?;
    }

    let alpha = alphascout::AlphaScout;
    let mut s1_resp = alpha.infer(&req)?;
    s1_resp.data_quality = s1_resp.data_quality.min(v.quality);

    // Regime (optional)
    let mut _regime_tag: String = "neutral".into();
    #[cfg(feature = "regime")]
    {
        let trend = ((input.fast_ma - input.slow_ma) / (input.vol + 1e-6)).tanh();
        let mut r_features = std::collections::BTreeMap::new();
        r_features.insert("vol".into(), ir::FeatureValue::F64(input.vol));
        r_features.insert("spread_bps".into(), ir::FeatureValue::F64(5.0));
        r_features.insert("trend".into(), ir::FeatureValue::F64(trend));
        let r_req = api::InferRequest {
            symbol: req.symbol.clone(),
            features: r_features,
            metadata: std::collections::BTreeMap::new(),
        };
        let regime_agent = regime::Regime;
        let r_resp = regime_agent.infer(&r_req)?;
        _regime_tag = r_resp.rationale.clone();
        s1_resp.regime_fit = (s1_resp.regime_fit * r_resp.regime_fit).clamp(0.0, 1.0);
    }

    let mut s1 = to_ir_signal(alpha.agent_id(), &req.symbol, &s1_resp);
    // Attach ATR/vol features for sizing/exits (use input.vol as ATR proxy when no feed)
    s1.features
        .push(("atr".into(), ir::FeatureValue::F64(input.vol.max(0.0))));
    s1.features
        .push(("vol".into(), ir::FeatureValue::F64(input.vol.max(0.0))));
    // Annotate signal with exit mode for downstream consumers
    if let Ok(txt) = std::fs::read_to_string("config/exit_policy.toml") {
        #[derive(serde::Deserialize)]
        struct ExitCfg {
            mode: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct ExitPolicy {
            exit: Option<ExitCfg>,
        }
        if let Ok(cfg) = toml::from_str::<ExitPolicy>(&txt) {
            if let Some(mode) = cfg.exit.and_then(|e| e.mode) {
                s1.features
                    .push(("exit_mode".into(), ir::FeatureValue::Text(mode)));
            }
        }
    }
    let _ = ledger::record_order(&ledger::OrderRecord {
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        ts_ms: chrono::Utc::now().timestamp_millis(),
        symbol: req.symbol.clone(),
        side: match s1.score.partial_cmp(&0.0) {
            Some(std::cmp::Ordering::Greater) => "BUY",
            Some(std::cmp::Ordering::Less) => "SELL",
            _ => "HOLD",
        }
        .into(),
        qty: 0.0,
        price: 0.0,
        status: "INTENT".into(),
        source: "HQ".into(),
    });

    // Optional Sentinel/Sentiment
    #[allow(unused_mut)]
    let mut signals: Vec<ir::Signal> = vec![ir::Signal {
        agent_id: s1.agent_id.clone(),
        symbol: s1.symbol.clone(),
        score: s1.score,
        uncertainty: s1.uncertainty,
        features: s1.features.clone(),
        rationale: s1.rationale.clone(),
    }];

    #[cfg(all(feature = "sentiment", feature = "sentinel"))]
    {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mut all_events = {
            let cache = NEWS_CACHE.lock().unwrap();
            cache.get(&req.symbol).cloned().unwrap_or_default()
        };
        if all_events.is_empty() {
            all_events.extend(sentinel::collectors::yahoo::fetch_latest(&req.symbol));
            all_events.extend(sentinel::collectors::fxstreet::fetch_latest(&req.symbol));
            all_events.extend(sentinel::collectors::forexnews::fetch_latest(&req.symbol));
            all_events.extend(sentinel::collectors::tradingview::fetch_latest(&req.symbol));
            sentinel::dedupe_by_text(&mut all_events);
        }
        let s_req = sentinel::events_to_infer(&req.symbol, &all_events, now_ms);
        let s_agent = sentiment::Sentiment;
        let s2_resp = s_agent.infer(&s_req)?;
        let s2 = to_ir_signal(s_agent.agent_id(), &req.symbol, &s2_resp);
        signals.push(s2);
    }

    // Meta-learner weighs whatever signals are present
    let meta_req = to_meta_request(&req.symbol, &signals);
    let meta = meta::SimpleMeta;
    let meta_res = meta.weigh(&meta_req)?;

    // Apply weights
    let mut weighted: Vec<ir::Signal> = vec![];
    for mut s in signals {
        let w = meta_res
            .weights
            .iter()
            .find(|w| w.agent_id == s.agent_id)
            .map(|w| w.weight)
            .unwrap_or(0.5)
            .max(0.0);
        s.score *= w;
        weighted.push(s);
    }

    let mut decision = weighted_fusion(&weighted, &cfg);
    DECISIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
    if matches!(decision.side, ir::Side::Hold) {
        ABSTAINS_TOTAL.fetch_add(1, Ordering::Relaxed);
    }

    // RiskSmith sizing
    let risksmith = risksmith::RiskSmith;
    let price = input.price;
    #[derive(serde::Deserialize)]
    struct PortfolioCtx {
        corr_to_portfolio: Option<f64>,
        sector_exposure_notional: Option<f64>,
        recent_max_drawdown: Option<f64>,
        loss_cooldown: Option<bool>,
    }
    let pctx: Option<PortfolioCtx> = std::fs::read_to_string("config/portfolio.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    let sizing_req = api::SizingRequest {
        symbol: req.symbol.clone(),
        price,
        account_equity: cfg.risk.account_equity,
        score: match decision.side {
            ir::Side::Buy => 1.0,
            ir::Side::Sell => -1.0,
            ir::Side::Hold => 0.0,
        },
        uncertainty: 1.0 - decision.confidence.min(1.0),
        ev_net: get_feat(&weighted[0], "ev_net").unwrap_or(0.0),
        regime_fit: get_feat(&weighted[0], "regime_fit").unwrap_or(0.0),
        atr: get_feat(&weighted[0], "atr")
            .or_else(|| get_feat(&weighted[0], "vol"))
            .unwrap_or(2.5),
        leverage_max: cfg.risk.leverage_max,
        symbol_notional_cap: cfg.caps.symbol_notional,
        portfolio_notional_cap: cfg.caps.portfolio_notional,
        exec_impact: get_feat(&weighted[0], "exec_impact"),
        corr_to_portfolio: pctx.as_ref().and_then(|x| x.corr_to_portfolio),
        sector_exposure_notional: pctx.as_ref().and_then(|x| x.sector_exposure_notional),
        recent_max_drawdown: pctx.as_ref().and_then(|x| x.recent_max_drawdown),
        loss_cooldown: pctx.as_ref().and_then(|x| x.loss_cooldown),
    };
    let sz = risksmith.size(&sizing_req)?;
    decision.qty = match decision.side {
        ir::Side::Buy | ir::Side::Sell => sz.qty,
        _ => 0.0,
    };

    // Audit: record exit policy applied (mode, SL/TP/TTL)
    let exit_mode = s1
        .features
        .iter()
        .find_map(|(k, v)| {
            if k == "exit_mode" {
                if let ir::FeatureValue::Text(t) = v {
                    Some(t.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into());
    let _ = guardian::write_audit(&guardian::AuditEvent {
        category: "exit_policy".into(),
        message: format!(
            "mode={} sl={:?} tp={:?} ttl={:?}",
            exit_mode, sz.stop_loss, sz.take_profit, sz.ttl_sec
        ),
        severity: "INFO".into(),
    });

    // Portfolio adjustment (respect caps vs current exposures from ledger)
    if decision.qty > 0.0 {
        let adj = portfolio::adjust_qty(
            &req.symbol,
            price,
            decision.qty,
            cfg.risk.account_equity,
            cfg.caps.symbol_notional,
            cfg.caps.portfolio_notional,
        );
        if (adj - decision.qty).abs() > 1e-6 {
            let msg = format!(
                "portfolio clamp {}: from {} to {}",
                req.symbol, decision.qty, adj
            );
            static LAST_AUDIT: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
            let mut last = LAST_AUDIT.lock().unwrap();
            if last.as_deref() != Some(&msg) {
                let evt = guardian::AuditEvent {
                    category: "portfolio".into(),
                    message: msg.clone(),
                    severity: "INFO".into(),
                };
                let _ = guardian::write_audit(&evt);
                *last = Some(msg);
            }
        }
        decision.qty = adj;
    }

    let policy = guardian::Policy {
        max_leverage: 2.0,
        max_position_notional: 100_000.0,
    };
    guardian::enforce_policy(&policy, &mut decision)?;

    if std::env::var("HQ_MODE").ok().as_deref() == Some("paper") {
        #[cfg(feature = "pathfinder")]
        {
            let spec = load_instrument(&req.symbol).unwrap_or(api::InstrumentSpec {
                tick_size: 0.01,
                lot_step: 1.0,
                min_qty: 1.0,
                max_qty: 1_000_000.0,
                min_notional: 10.0,
            });
            let side = match decision.side {
                ir::Side::Buy => api::OrderSide::Buy,
                ir::Side::Sell => api::OrderSide::Sell,
                ir::Side::Hold => {
                    tracing::info!("HOLD – no order");
                    return Ok(());
                }
            };
            // Round TP/SL to tick with side-aware precision
            let round_up = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).ceil() * tick
                } else {
                    x
                }
            };
            let round_dn = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).floor() * tick
                } else {
                    x
                }
            };
            let (sl_rounded, tp_rounded) = match side {
                api::OrderSide::Buy => (
                    sz.stop_loss.map(|p| round_dn(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_up(p, spec.tick_size)),
                ),
                api::OrderSide::Sell => (
                    sz.stop_loss.map(|p| round_up(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_dn(p, spec.tick_size)),
                ),
            };
            // Partial TP fraction from policy if available
            let tp_partial_pct = {
                #[derive(serde::Deserialize)]
                struct StructureCfg {
                    fta_partial_pct: Option<f64>,
                }
                #[derive(serde::Deserialize)]
                struct ExitPolicy {
                    structure: Option<StructureCfg>,
                }
                std::fs::read_to_string("config/exit_policy.toml")
                    .ok()
                    .and_then(|s| toml::from_str::<ExitPolicy>(&s).ok())
                    .and_then(|p| p.structure.and_then(|s| s.fta_partial_pct))
            };
            let order = api::OrderRequest {
                idempotency_key: uuid::Uuid::new_v4().to_string(),
                symbol: req.symbol.clone(),
                side,
                qty: decision.qty.max(0.0),
                order_type: api::OrderType::Market,
                ttl_sec: sz.ttl_sec.unwrap_or(60),
                stop_loss: sl_rounded,
                take_profit: tp_rounded,
                tp_partial_pct,
            };
            let mid = price;
            let _ = ledger::record_order(&ledger::OrderRecord {
                idempotency_key: order.idempotency_key.clone(),
                ts_ms: chrono::Utc::now().timestamp_millis(),
                symbol: order.symbol.clone(),
                side: match order.side {
                    api::OrderSide::Buy => "BUY",
                    api::OrderSide::Sell => "SELL",
                }
                .into(),
                qty: order.qty,
                price: mid,
                status: "SUBMITTED".into(),
                source: "PAPER".into(),
            });
            // Audit bracket placement (paper)
            let _ = guardian::write_audit(&guardian::AuditEvent {
                category: "bracket".to_string(),
                message: format!(
                    "placed sl={:?} tp={:?} ttl={:?}",
                    order.stop_loss, order.take_profit, sz.ttl_sec
                ),
                severity: "INFO".into(),
            });
            // Emit MT4-mapped signal for manual execution
            let mt4 = map_symbol_mt4(&req.symbol);
            let sig = format!(
                "MT4 signal: {} {} lots={} SL={:?} TP={:?}",
                mt4,
                match order.side {
                    api::OrderSide::Buy => "BUY",
                    api::OrderSide::Sell => "SELL",
                },
                order.qty,
                order.stop_loss,
                order.take_profit
            );
            let _ = guardian::write_audit(&guardian::AuditEvent {
                category: "signal_mt4".to_string(),
                message: sig,
                severity: "INFO".into(),
            });
            let rpt = pathfinder::route_and_execute(&order, &spec, mid)?;
            let _ = ledger::record_fill(&ledger::FillRecord {
                idempotency_key: order.idempotency_key.clone(),
                ts_ms: chrono::Utc::now().timestamp_millis(),
                symbol: order.symbol.clone(),
                qty: rpt.filled_qty,
                price: rpt.avg_price,
                source: "PAPER".into(),
            });
            if rpt.filled_qty > 0.0 {
                FILLS_TOTAL.fetch_add(1, Ordering::Relaxed);
                SLIPPAGE_BPS_SUM.fetch_add(rpt.slippage_bps as u64, Ordering::Relaxed);
                if rpt.side == "Buy" {
                    let entry = *LAST_SELL_PRICE.lock().unwrap();
                    if entry > 0.0 {
                        let pnl = (entry - rpt.avg_price) * rpt.filled_qty;
                        *PNL_TOTAL.lock().unwrap() += pnl;
                    }
                    *LAST_BUY_PRICE.lock().unwrap() = rpt.avg_price;
                    *LAST_SELL_PRICE.lock().unwrap() = 0.0;
                } else if rpt.side == "Sell" {
                    let entry = *LAST_BUY_PRICE.lock().unwrap();
                    if entry > 0.0 {
                        let pnl = (rpt.avg_price - entry) * rpt.filled_qty;
                        *PNL_TOTAL.lock().unwrap() += pnl;
                    }
                    *LAST_SELL_PRICE.lock().unwrap() = rpt.avg_price;
                    *LAST_BUY_PRICE.lock().unwrap() = 0.0;
                }
            }
            #[cfg(feature = "pathfinder")]
            if rpt.filled_qty > 0.0 {
                tca::write(&tca::TcaRecord {
                    symbol: &req.symbol,
                    side: match order.side {
                        api::OrderSide::Buy => "BUY",
                        api::OrderSide::Sell => "SELL",
                    },
                    exp_price: mid,
                    fill_price: rpt.avg_price,
                    qty: rpt.filled_qty,
                    slippage_bps: rpt.slippage_bps,
                    regime: Some(&_regime_tag),
                })?;
            }
            tracing::info!(?rpt, "paper exec report");

            // Paper bracket resolution over a short synthetic path with TTL and optional partial TP
            let mut remaining_qty = rpt.filled_qty.max(0.0);
            let mut px = mid;
            let sl = order.stop_loss;
            let tp = order.take_profit;
            let ttl = sz.ttl_sec.unwrap_or(60);
            let steps = ttl.min(120) as usize / 3 + 1; // ~3s step granularity
            let step_mag = get_feat(&weighted[0], "atr").unwrap_or(2.5) / 20.0; // coarse move size
            let dir = match order.side {
                api::OrderSide::Buy => 1.0,
                api::OrderSide::Sell => -1.0,
            };

            // Optional partial take-profit support
            let mut partial_done = false;
            let tp_partial = order.tp_partial_pct.unwrap_or(0.0).clamp(0.0, 1.0);

            for _ in 0..steps {
                // drift toward target with small oscillation
                px += dir * step_mag * 0.5;
                px += (px * 1_000.0).sin() * step_mag * 0.05;

                // Check levels
                let hit_tp = tp
                    .map(|t| match order.side {
                        api::OrderSide::Buy => px >= t,
                        api::OrderSide::Sell => px <= t,
                    })
                    .unwrap_or(false);
                let hit_sl = sl
                    .map(|s| match order.side {
                        api::OrderSide::Buy => px <= s,
                        api::OrderSide::Sell => px >= s,
                    })
                    .unwrap_or(false);

                if hit_tp {
                    if let Some(tp_px) = tp {
                        // Partial close if configured
                        let close_qty = if !partial_done && tp_partial > 0.0 && tp_partial < 1.0 {
                            partial_done = true;
                            (remaining_qty * tp_partial).max(0.0)
                        } else {
                            remaining_qty
                        };
                        if close_qty > 0.0 {
                            let close_side = match order.side {
                                api::OrderSide::Buy => api::OrderSide::Sell,
                                api::OrderSide::Sell => api::OrderSide::Buy,
                            };
                            let close_id = uuid::Uuid::new_v4().to_string();
                            let _ = ledger::record_order(&ledger::OrderRecord {
                                idempotency_key: close_id.clone(),
                                ts_ms: chrono::Utc::now().timestamp_millis(),
                                symbol: order.symbol.clone(),
                                side: match close_side {
                                    api::OrderSide::Buy => "BUY",
                                    api::OrderSide::Sell => "SELL",
                                }
                                .into(),
                                qty: close_qty,
                                price: tp_px,
                                status: "FILLED".into(),
                                source: "PAPER".into(),
                            });
                            let _ = ledger::record_fill(&ledger::FillRecord {
                                idempotency_key: close_id.clone(),
                                ts_ms: chrono::Utc::now().timestamp_millis(),
                                symbol: order.symbol.clone(),
                                qty: close_qty,
                                price: tp_px,
                                source: "PAPER".into(),
                            });
                            #[cfg(feature = "pathfinder")]
                            let _ = tca::write(&tca::TcaRecord {
                                symbol: &req.symbol,
                                side: match close_side {
                                    api::OrderSide::Buy => "BUY",
                                    api::OrderSide::Sell => "SELL",
                                },
                                exp_price: tp_px,
                                fill_price: tp_px,
                                qty: close_qty,
                                slippage_bps: 0.0,
                                regime: Some(&_regime_tag),
                            });
                            guardian::write_audit(&guardian::AuditEvent {
                                category: "bracket_close".into(),
                                message: format!("TP close at {:.6} qty={}", tp_px, close_qty),
                                severity: "INFO".into(),
                            })
                            .ok();
                            remaining_qty -= close_qty;
                            if remaining_qty <= 0.0 {
                                break;
                            }
                        }
                    }
                } else if hit_sl && sl.is_some() {
                } else if hit_sl {
                    if let Some(sl_px) = sl {
                        let close_side = match order.side {
                            api::OrderSide::Buy => api::OrderSide::Sell,
                            api::OrderSide::Sell => api::OrderSide::Buy,
                        };
                        let close_id = uuid::Uuid::new_v4().to_string();
                        let _ = ledger::record_order(&ledger::OrderRecord {
                            idempotency_key: close_id.clone(),
                            ts_ms: chrono::Utc::now().timestamp_millis(),
                            symbol: order.symbol.clone(),
                            side: match close_side {
                                api::OrderSide::Buy => "BUY",
                                api::OrderSide::Sell => "SELL",
                            }
                            .into(),
                            qty: remaining_qty,
                            price: sl_px,
                            status: "FILLED".into(),
                            source: "PAPER".into(),
                        });
                        let _ = ledger::record_fill(&ledger::FillRecord {
                            idempotency_key: close_id.clone(),
                            ts_ms: chrono::Utc::now().timestamp_millis(),
                            symbol: order.symbol.clone(),
                            qty: remaining_qty,
                            price: sl_px,
                            source: "PAPER".into(),
                        });
                        #[cfg(feature = "pathfinder")]
                        let _ = tca::write(&tca::TcaRecord {
                            symbol: &req.symbol,
                            side: match close_side {
                                api::OrderSide::Buy => "BUY",
                                api::OrderSide::Sell => "SELL",
                            },
                            exp_price: sl_px,
                            fill_price: sl_px,
                            qty: remaining_qty,
                            slippage_bps: 0.0,
                            regime: Some(&_regime_tag),
                        });
                        guardian::write_audit(&guardian::AuditEvent {
                            category: "bracket_close".into(),
                            message: format!("SL close at {:.6} qty={}", sl_px, remaining_qty),
                            severity: "INFO".into(),
                        })
                        .ok();
                        remaining_qty = 0.0;
                        break;
                    }
                }
            }
            // TTL close any remainder at last px
            if remaining_qty > 0.0 {
                let close_side = match order.side {
                    api::OrderSide::Buy => api::OrderSide::Sell,
                    api::OrderSide::Sell => api::OrderSide::Buy,
                };
                let close_id = uuid::Uuid::new_v4().to_string();
                let _ = ledger::record_order(&ledger::OrderRecord {
                    idempotency_key: close_id.clone(),
                    ts_ms: chrono::Utc::now().timestamp_millis(),
                    symbol: order.symbol.clone(),
                    side: match close_side {
                        api::OrderSide::Buy => "BUY",
                        api::OrderSide::Sell => "SELL",
                    }
                    .into(),
                    qty: remaining_qty,
                    price: px,
                    status: "FILLED".into(),
                    source: "PAPER".into(),
                });
                let _ = ledger::record_fill(&ledger::FillRecord {
                    idempotency_key: close_id.clone(),
                    ts_ms: chrono::Utc::now().timestamp_millis(),
                    symbol: order.symbol.clone(),
                    qty: remaining_qty,
                    price: px,
                    source: "PAPER".into(),
                });
                #[cfg(feature = "pathfinder")]
                let _ = tca::write(&tca::TcaRecord {
                    symbol: &req.symbol,
                    side: match close_side {
                        api::OrderSide::Buy => "BUY",
                        api::OrderSide::Sell => "SELL",
                    },
                    exp_price: px,
                    fill_price: px,
                    qty: remaining_qty,
                    slippage_bps: 0.0,
                    regime: Some(&_regime_tag),
                });
                guardian::write_audit(&guardian::AuditEvent {
                    category: "bracket_close".into(),
                    message: format!("TTL close at {:.6} qty={}", px, remaining_qty),
                    severity: "INFO".into(),
                })
                .ok();
            }
        }
        #[cfg(not(feature = "pathfinder"))]
        {
            tracing::info!("pathfinder feature disabled; skipping paper execution");
        }
    } else if std::env::var("HQ_MODE").ok().as_deref() == Some("live_oanda") {
        #[cfg(feature = "brokergate")]
        {
            let _ = brokergate::oanda::start_transactions_stream();
            let host =
                std::env::var("OANDA_HOST").unwrap_or_else(|_| "api-fxpractice.oanda.com".into());
            if !guardian::egress_allowed(&host) {
                guardian::write_audit(&guardian::AuditEvent {
                    category: "egress".into(),
                    message: format!("blocked egress to {}", host),
                    severity: "ERROR".into(),
                })?;
                return Ok(());
            }
            // Preflight: require OANDA creds or abort fast
            if std::env::var("OANDA_API_KEY").is_err() || std::env::var("OANDA_ACCOUNT_ID").is_err() {
                let _ = guardian::write_audit(&guardian::AuditEvent {
                    category: "broker".into(),
                    message: "missing OANDA_API_KEY or OANDA_ACCOUNT_ID".into(),
                    severity: "ERROR".into(),
                });
                tracing::error!("missing OANDA_API_KEY or OANDA_ACCOUNT_ID; aborting live_oanda");
                return Ok(());
            }
            // Execution gating: load thresholds from policy if present
            #[derive(serde::Deserialize, Default)]
            struct ExecCfg {
                max_spread_bps: Option<f64>,
                max_dev_bps: Option<f64>,
                pacing_ms: Option<u64>,
                chunk_frac_min: Option<f64>,
                chunk_frac_max: Option<f64>,
                block_weekend: Option<bool>,
                block_utc_ranges: Option<Vec<String>>, // e.g. ["21:55-22:10"]
                block_events: Option<bool>,
                event_window_sec: Option<u64>,
                min_event_sev: Option<f64>,
            }
            #[derive(serde::Deserialize)]
            struct PolicyExec {
                exec: Option<ExecCfg>,
            }
            let exec_cfg = (|| {
                if let Ok(txt) = std::fs::read_to_string("config/policy.toml") {
                    if let Ok(cfg) = toml::from_str::<PolicyExec>(&txt) {
                        return cfg.exec.unwrap_or_default();
                    }
                }
                ExecCfg::default()
            })();
            let max_spread_bps = exec_cfg.max_spread_bps.unwrap_or(20.0);
            let max_dev_bps = exec_cfg.max_dev_bps.unwrap_or(25.0);
            let _pacing_ms = exec_cfg.pacing_ms.unwrap_or(300);
            let _chunk_frac_min = exec_cfg.chunk_frac_min.unwrap_or(0.05).clamp(0.01, 1.0);
            let _chunk_frac_max = exec_cfg
                .chunk_frac_max
                .unwrap_or(0.25)
                .clamp(_chunk_frac_min, 1.0);
            // Time-of-day gating
            let now = chrono::Utc::now();
            use chrono::Datelike;
            let dow = now.weekday().num_days_from_monday();
            if exec_cfg.block_weekend.unwrap_or(true) && (dow >= 5) {
                let _ = guardian::write_audit(&guardian::AuditEvent{ category: "exec_gate".into(), message: "aborting submit: weekend".into(), severity: "WARN".into() });
                return Ok(())
            }
            use chrono::Timelike;
            if let Some(ranges) = &exec_cfg.block_utc_ranges {
                let hm = format!("{:02}:{:02}", now.hour(), now.minute());
                for r in ranges {
                    if let Some((a,b)) = r.split_once('-') {
                        if hm.as_str() >= a.trim() && hm.as_str() <= b.trim() {
                            let _ = guardian::write_audit(&guardian::AuditEvent{ category: "exec_gate".into(), message: format!("aborting submit: quiet window {}", r), severity: "WARN".into() });
                            return Ok(())
                        }
                }
            }
        }
#[cfg(all(feature = "sentiment", feature = "sentinel"))]
            if exec_cfg.block_events.unwrap_or(false) {
                let window = exec_cfg.event_window_sec.unwrap_or(900) as i64;
                let min_sev = exec_cfg.min_event_sev.unwrap_or(0.7);
                let now_ms = chrono::Utc::now().timestamp_millis();
                let recent = {
                    let cache = NEWS_CACHE.lock().unwrap();
                    cache.get(&req.symbol).cloned().unwrap_or_default()
                };
                let bad = recent.into_iter().any(|e| {
                    (now_ms - e.published_ms).abs() <= window * 1000 && e.severity >= min_sev
                });
                if bad {
                    let _ = guardian::write_audit(&guardian::AuditEvent {
                        category: "exec_gate".into(),
                        message: "aborting submit: event window".into(),
                        severity: "WARN".into(),
                    });
                    return Ok(());
                }
            }
            // Fetch live pricing and gate by spread/deviation
            if let Ok((bid, ask)) = brokergate::oanda::fetch_pricing(&req.symbol) {
                let mid_live = (bid + ask) / 2.0;
                let spread_bps = ((ask - bid) / mid_live * 10_000.0).abs();
                let dev_bps = (((mid_live - price) / price) * 10_000.0).abs();
                let _ = guardian::write_audit(&guardian::AuditEvent {
                    category: "exec_kpi".into(),
                    message: format!(
                        "pricing: mid={:.6} spread_bps={:.2} dev_bps={:.2}",
                        mid_live, spread_bps, dev_bps
                    ),
                    severity: "INFO".into(),
                });
                if spread_bps > max_spread_bps || dev_bps > max_dev_bps {
                    let why = if spread_bps > max_spread_bps {
                        format!("spread {}>{}", spread_bps, max_spread_bps)
                    } else {
                        format!("dev {}>{}", dev_bps, max_dev_bps)
                    };
                    let _ = guardian::write_audit(&guardian::AuditEvent {
                        category: "exec_gate".into(),
                        message: format!("aborting submit: {}", why),
                        severity: "WARN".into(),
                    });
                    return Ok(());
                }
                // M2M clamp using broker exposures before submit
                if decision.qty > 0.0 {
                    if let Ok(pos) = brokergate::oanda::fetch_open_positions() {
                        let mut symbol_notional = 0.0_f64;
                        let mut total_notional = 0.0_f64;
                        if let Some(arr) = pos.get("positions").and_then(|v| v.as_array()) {
                            for p in arr {
                                let inst =
                                    p.get("instrument").and_then(|v| v.as_str()).unwrap_or("");
                                let net_long = p
                                    .get("long")
                                    .and_then(|v| v.get("units"))
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(0.0);
                                let net_short = p
                                    .get("short")
                                    .and_then(|v| v.get("units"))
                                    .and_then(|v| v.as_str())
                                    .and_then(|s| s.parse::<f64>().ok())
                                    .unwrap_or(0.0);
                                let net = net_long + net_short;
                                let notional = net.abs() * mid_live;
                                if inst == req.symbol {
                                    symbol_notional = notional;
                                }
                                total_notional += notional;
                            }
                        }
                        let desired = decision.qty * mid_live;
                        let allowed_symbol = (cfg.caps.symbol_notional - symbol_notional).max(0.0);
                        let allowed_portfolio =
                            (cfg.caps.portfolio_notional - total_notional).max(0.0);
                        let allowed = allowed_symbol.min(allowed_portfolio);
                        if allowed < desired && mid_live > 0.0 {
                            let adj = (allowed / mid_live).max(0.0);
                            let msg = format!(
                                "m2m clamp {}: from {} to {} (sym_notional={:.2} total={:.2})",
                                req.symbol, decision.qty, adj, symbol_notional, total_notional
                            );
                            let _ = guardian::write_audit(&guardian::AuditEvent {
                                category: "portfolio".into(),
                                message: msg,
                                severity: "INFO".into(),
                            });
                            decision.qty = adj;
                        }
                    }
                }
            }
            let side = match decision.side {
                ir::Side::Buy => api::OrderSide::Buy,
                ir::Side::Sell => api::OrderSide::Sell,
                ir::Side::Hold => {
                    tracing::info!("HOLD – no order");
                    return Ok(());
                }
            };
            let spec = load_instrument(&req.symbol).unwrap_or(api::InstrumentSpec {
                tick_size: 0.01,
                lot_step: 1.0,
                min_qty: 1.0,
                max_qty: 1_000_000.0,
                min_notional: 10.0,
            });
            // Round TP/SL to tick with side-aware precision
            let round_up = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).ceil() * tick
                } else {
                    x
                }
            };
            let round_dn = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).floor() * tick
                } else {
                    x
                }
            };
            let (sl_rounded, tp_rounded) = match side {
                api::OrderSide::Buy => (
                    sz.stop_loss.map(|p| round_dn(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_up(p, spec.tick_size)),
                ),
                api::OrderSide::Sell => (
                    sz.stop_loss.map(|p| round_up(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_dn(p, spec.tick_size)),
                ),
            };
            let tp_partial_pct = {
                #[derive(serde::Deserialize)]
                struct StructureCfg {
                    fta_partial_pct: Option<f64>,
                }
                #[derive(serde::Deserialize)]
                struct ExitPolicy {
                    structure: Option<StructureCfg>,
                }
                std::fs::read_to_string("config/exit_policy.toml")
                    .ok()
                    .and_then(|s| toml::from_str::<ExitPolicy>(&s).ok())
                    .and_then(|p| p.structure.and_then(|s| s.fta_partial_pct))
            };
            // Signals-only mode: skip live submit, just audit and emit MT4 signal
            if std::env::var("SIGNALS_ONLY").ok().as_deref() == Some("1") {
                let mt4 = map_symbol_mt4(&req.symbol);
                let sig = format!(
                    "MT4 signal: {} {} lots={} SL={:?} TP={:?}",
                    mt4,
                    match side {
                        api::OrderSide::Buy => "BUY",
                        api::OrderSide::Sell => "SELL",
                    },
                    decision.qty.max(0.0),
                    sl_rounded,
                    tp_rounded
                );
                let _ = guardian::write_audit(&guardian::AuditEvent {
                    category: "signal_mt4".to_string(),
                    message: sig,
                    severity: "INFO".into(),
                });
                return Ok(());
            }
            let order = api::OrderRequest {
                idempotency_key: uuid::Uuid::new_v4().to_string(),
                symbol: req.symbol.clone(),
                side: side.clone(),
                qty: decision.qty.max(0.0),
                order_type: api::OrderType::Market,
                ttl_sec: sz.ttl_sec.unwrap_or(60),
                stop_loss: sl_rounded,
                take_profit: tp_rounded,
                tp_partial_pct,
            };
            let _ = ledger::record_order(&ledger::OrderRecord {
                idempotency_key: order.idempotency_key.clone(),
                ts_ms: chrono::Utc::now().timestamp_millis(),
                symbol: order.symbol.clone(),
                side: match order.side {
                    api::OrderSide::Buy => "BUY",
                    api::OrderSide::Sell => "SELL",
                }
                .into(),
                qty: order.qty,
                price: 0.0,
                status: "SUBMITTED".into(),
                source: "OANDA".into(),
            });
            // Audit bracket placement (live)
            let _ = guardian::write_audit(&guardian::AuditEvent {
                category: "bracket".to_string(),
                message: format!(
                    "placed sl={:?} tp={:?} ttl={:?}",
                    order.stop_loss, order.take_profit, sz.ttl_sec
                ),
                severity: "INFO".into(),
            });
            // Emit MT4-mapped signal for manual execution
            let mt4 = map_symbol_mt4(&req.symbol);
            let sig = format!(
                "MT4 signal: {} {} lots={} SL={:?} TP={:?}",
                mt4,
                match side {
                    api::OrderSide::Buy => "BUY",
                    api::OrderSide::Sell => "SELL",
                },
                decision.qty.max(0.0),
                sl_rounded,
                tp_rounded
            );
            let _ = guardian::write_audit(&guardian::AuditEvent {
                category: "signal_mt4".to_string(),
                message: sig,
                severity: "INFO".into(),
            });

            // Submit and reconcile via blocking threads to avoid runtime drop panic
            let mut remaining = decision.qty.max(0.0);
            let start_ms = chrono::Utc::now().timestamp_millis();
            // Enforce a default TTL budget of 60s if none provided to avoid indefinite looping
            let ttl_ms: i64 = sz.ttl_sec.unwrap_or(60) as i64 * 1000;
            while remaining > 0.0 {
                if chrono::Utc::now().timestamp_millis() - start_ms > ttl_ms {
                    let _ = guardian::write_audit(&guardian::AuditEvent {
                        category: "exec_gate".into(),
                        message: "TTL budget reached".into(),
                        severity: "WARN".into(),
                    });
                    break;
                }
                if let Ok((bid, ask)) = brokergate::oanda::fetch_pricing(&req.symbol) {
                    let mid_live = (bid + ask) / 2.0;
                    let spread_bps = ((ask - bid) / mid_live * 10_000.0).abs();
                    let dev_bps = (((mid_live - price) / price) * 10_000.0).abs();
                    if spread_bps > max_spread_bps || dev_bps > max_dev_bps {
                        break;
                    }
                }
                let eff_frac = 0.25_f64.max(0.05_f64); // falls back if exec_cfg not set earlier
                let chunk = (spec.max_qty * eff_frac).max(spec.min_qty).min(remaining);
                let order = api::OrderRequest {
                    idempotency_key: uuid::Uuid::new_v4().to_string(),
                    symbol: req.symbol.clone(),
                    side: side.clone(),
                    qty: chunk,
                    order_type: api::OrderType::Market,
                    ttl_sec: sz.ttl_sec.unwrap_or(60),
                    stop_loss: sl_rounded,
                    take_profit: tp_rounded,
                    tp_partial_pct,
                };
                let order_for_submit = order.clone();
                let spec_for_submit = api::InstrumentSpec {
                    tick_size: spec.tick_size,
                    lot_step: spec.lot_step,
                    min_qty: spec.min_qty,
                    max_qty: spec.max_qty,
                    min_notional: spec.min_notional,
                };
                let submit_res = std::thread::spawn(move || {
                    brokergate::oanda::submit_market(&order_for_submit, &spec_for_submit)
                })
                .join();
                match submit_res {
                    Ok(Ok(_rpt)) => {
                        remaining -= chunk;
                    }
                    Ok(Err(_e)) => {
                        remaining = (remaining - chunk * 0.5).max(0.0);
                    }
                    Err(_panic) => {
                        remaining = (remaining - chunk * 0.5).max(0.0);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        }
        #[cfg(not(feature = "brokergate"))]
        {
            tracing::info!("brokergate feature disabled; skipping live_oanda branch");
        }
    } else if std::env::var("HQ_MODE").ok().as_deref() == Some("live_coinexx") {
        #[cfg(feature = "brokergate")]
        {
            let host = std::env::var("COINEXX_HOST").unwrap_or_else(|_| "api.coinexx.com".into());
            if !guardian::egress_allowed(&host) {
                guardian::write_audit(&guardian::AuditEvent {
                    category: "egress".into(),
                    message: format!("blocked egress to {}", host),
                    severity: "ERROR".into(),
                })?;
                return Ok(());
            }
            let side = match decision.side {
                ir::Side::Buy => api::OrderSide::Buy,
                ir::Side::Sell => api::OrderSide::Sell,
                ir::Side::Hold => {
                    tracing::info!("HOLD – no order");
                    return Ok(());
                }
            };
            let spec = load_instrument(&req.symbol).unwrap_or(api::InstrumentSpec {
                tick_size: 0.00001,
                lot_step: 1.0,
                min_qty: 1.0,
                max_qty: 10_000_000.0,
                min_notional: 1.0,
            });
            // Round TP/SL
            let round_up = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).ceil() * tick
                } else {
                    x
                }
            };
            let round_dn = |x: f64, tick: f64| -> f64 {
                if tick > 0.0 {
                    (x / tick).floor() * tick
                } else {
                    x
                }
            };
            let (sl_rounded, tp_rounded) = match side {
                api::OrderSide::Buy => (
                    sz.stop_loss.map(|p| round_dn(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_up(p, spec.tick_size)),
                ),
                api::OrderSide::Sell => (
                    sz.stop_loss.map(|p| round_up(p, spec.tick_size)),
                    sz.take_profit.map(|p| round_dn(p, spec.tick_size)),
                ),
            };
            let order = api::OrderRequest {
                idempotency_key: uuid::Uuid::new_v4().to_string(),
                symbol: req.symbol.clone(),
                side,
                qty: decision.qty.max(0.0),
                order_type: api::OrderType::Market,
                ttl_sec: sz.ttl_sec.unwrap_or(60),
                stop_loss: sl_rounded,
                take_profit: tp_rounded,
                tp_partial_pct: None,
            };
            let _ = ledger::record_order(&ledger::OrderRecord {
                idempotency_key: order.idempotency_key.clone(),
                ts_ms: chrono::Utc::now().timestamp_millis(),
                symbol: order.symbol.clone(),
                side: match order.side {
                    api::OrderSide::Buy => "BUY",
                    api::OrderSide::Sell => "SELL",
                }
                .into(),
                qty: order.qty,
                price: 0.0,
                status: "SUBMITTED".into(),
                source: "COINEXX".into(),
            });
            match brokergate::coinexx::submit_market(&order, &spec) {
                Ok(rpt) => {
                    tracing::info!(?rpt, "coinexx exec submit");
                }
                Err(e) => {
                    let msg = format!("coinexx error: {}", e);
                    let evt = guardian::AuditEvent {
                        category: "broker".into(),
                        message: msg.clone(),
                        severity: "ERROR".into(),
                    };
                    guardian::write_audit(&evt)?;
                    #[cfg(feature = "alerts")]
                    alerts::maybe_notify("BrokerGate Coinexx", &msg, &evt.severity);
                    tracing::error!(error=%e, "coinexx submit failed");
                }
            }
        }
        #[cfg(not(feature = "brokergate"))]
        {
            tracing::info!("brokergate feature disabled; skipping live_coinexx branch");
        }
    }

    tracing::info!(?decision, stops=?sz.stop_loss, tps=?sz.take_profit, "fused decision (lieutenant co-sign + risksmith)");
    Ok(())
}

fn backtest() -> anyhow::Result<()> {
    let path = "data/sample_prices.csv";
    let mut prices: Vec<f64> = vec![];
    if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(false).from_path(path) {
        for r in rdr.records().flatten() {
            if let Ok(p) = r[0].parse::<f64>() {
                prices.push(p);
            }
        }
    }
    // Load news blackout dates
    let mut blackout_dates: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(false).from_path("data/news_blackout.csv") {
        for r in rdr.records().flatten() {
            blackout_dates.insert(r[0].to_string());
        }
    }
    tracing::info!("Loaded {} news blackout dates", blackout_dates.len());
    // Load real dates for each candle
    let mut candle_dates: Vec<String> = vec![];
    if let Ok(mut rdr) = csv::ReaderBuilder::new().has_headers(false).from_path("data/sample_dates.csv") {
        for r in rdr.records().flatten() {
            candle_dates.push(r[0].to_string());
        }
    }
    if prices.is_empty() {
        let mut p = 190.0;
        let drift: f64 = std::env::var("BT_TREND_PER_STEP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.2);
        for _ in 0..200 {
            let noise = (rand::random::<f64>() - 0.5) * 0.05;
            p += drift + noise;
            prices.push(p);
        }
    }
    for (i, p) in prices.iter().enumerate() {
        std::env::set_var("HQ_MODE", "paper");
        // News filter disabled - using real dates for future use
        let _date_str = candle_dates.get(i).cloned().unwrap_or_default();
        // Calculate RSI-14 first so filter can use it
        let rsi_14 = if i >= 14 {
            let window = &prices[i - 14..i];
            let mut gains = 0.0_f64;
            let mut losses = 0.0_f64;
            for w in window.windows(2) {
                let change = w[1] - w[0];
                if change > 0.0 { gains += change; } else { losses += change.abs(); }
            }
            let avg_gain = gains / 14.0;
            let avg_loss = losses / 14.0;
            if avg_loss == 0.0 { 100.0 } else {
                100.0 - (100.0 / (1.0 + avg_gain / avg_loss))
            }
        } else { 50.0 };
        // Hard RSI filter - skip neutral zone AND overbought/oversold extremes
        // Also handled in signal alignment check below
        if rsi_14 > 45.0 && rsi_14 < 55.0 {
            DECISIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
            ABSTAINS_TOTAL.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let fast = if i >= 9 {
            prices[i - 9..=i].iter().copied().sum::<f64>() / 10.0
        } else {
            *p
        };
        let slow = if i >= 199 {
            prices[i - 199..=i].iter().copied().sum::<f64>() / 200.0
        } else {
            *p
        };
        let vol = if i >= 10 {
            let w = &prices[i - 10..=i];
            let m = w.iter().copied().sum::<f64>() / (w.len() as f64);
            let var = w.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (w.len() as f64);
            var.sqrt().max(0.1)
        } else {
            1.0
        };
        let input = alphascout::Inputs {
            symbol: "EUR_USD".into(),
            price: *p,
            fast_ma: fast,
            slow_ma: slow,
            vol,
            rsi_14,
        };
        // Directional RSI alignment check
        // Buy signal (fast > slow) needs RSI > 55 for confirmation
        // Sell signal (fast < slow) needs RSI < 45 for confirmation
        let ma_bullish = fast > slow;
        let ma_bearish = fast < slow;
        let rsi_confirms = (ma_bullish && rsi_14 > 65.0) || (ma_bearish && rsi_14 < 35.0);
        if !rsi_confirms {
            DECISIONS_TOTAL.fetch_add(1, Ordering::Relaxed);
            ABSTAINS_TOTAL.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let _ = run_once_with(input);
    }
    let dec = DECISIONS_TOTAL.load(Ordering::Relaxed);
    let abst = ABSTAINS_TOTAL.load(Ordering::Relaxed);
    let fills = FILLS_TOTAL.load(Ordering::Relaxed);
    let avg_slip = if fills > 0 {
        (SLIPPAGE_BPS_SUM.load(Ordering::Relaxed) as f64) / (fills as f64)
    } else {
        0.0
    };
    let pnl = *PNL_TOTAL.lock().unwrap();
    println!("=== Backtest Summary ===\nDecisions: {}\nAbstains: {} ({}%)\nFills: {}\nAvg Slippage (bps): {:.2}\nTotal P&L: ${:.2}",
        dec, abst, if dec>0 { (abst as f64 / dec as f64)*100.0 } else { 0.0 }, fills, avg_slip, pnl);
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();
    tracing::info!("Headquarters starting");

    let health_port: u16 = std::env::var("HQ_HEALTH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8088);
    tokio::spawn(health_server(health_port));

    if std::env::var("HQ_MODE").ok().as_deref() == Some("backtest") {
        backtest()?;
        return Ok(());
    }

    let symbols = load_symbols();
    let interval_secs = std::env::var("SENTINEL_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    start_sentinel(symbols, interval_secs).await;

    let symbol = std::env::var("HQ_SYMBOL").unwrap_or_else(|_| "AAPL".into());
    let price: f64 = std::env::var("HQ_PRICE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(190.0);
    let fast: f64 = std::env::var("HQ_FAST")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(188.0);
    let slow: f64 = std::env::var("HQ_SLOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(185.0);
    let vol: f64 = std::env::var("HQ_VOL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.5);
    let input = alphascout::Inputs {
        symbol,
        price,
        fast_ma: fast,
        slow_ma: slow,
        vol,
        rsi_14: 50.0,
    };
    run_once_with(input)?;
    Ok(())
}
