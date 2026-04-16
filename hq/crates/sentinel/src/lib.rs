use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Mutex;

pub mod collectors;

static SEEN: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static SEEN_PATH: &str = "logs/sentinel_seen.jsonl";

fn load_seen() {
    let mut guard = SEEN.lock().unwrap();
    if let Ok(txt) = std::fs::read_to_string(SEEN_PATH) {
        for line in txt.lines() {
            guard.insert(line.trim().to_string());
        }
    }
}

fn persist_seen(key: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(SEEN_PATH)
    {
        let _ = std::io::Write::write_all(&mut f, format!("{}\n", key).as_bytes());
    }
}

pub fn make_key(source: &api::NewsSource, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{:?}|{}", source, text).as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn baseline_sentiment(text: &str) -> f64 {
    let positive_words = [
        "bullish", "surge", "rally", "gains", "up", "rise", "soar", "boost", "strong", "positive",
        "buy", "outperform", "upgrade", "beat", "exceed", "growth", "profit", "bull", "upside",
        "optimistic", "confidence", "strength", "momentum", "breakout", "advance"
    ];
    let negative_words = [
        "bearish", "plunge", "crash", "losses", "down", "fall", "drop", "decline", "weak", "negative",
        "sell", "underperform", "downgrade", "miss", "below", "recession", "loss", "bear", "downside",
        "pessimistic", "concern", "weakness", "selloff", "breakdown", "retreat"
    ];
    
    let text_lower = text.to_lowercase();
    let mut pos_count = 0;
    let mut neg_count = 0;
    
    for word in positive_words.iter() {
        if text_lower.contains(word) {
            pos_count += 1;
        }
    }
    
    for word in negative_words.iter() {
        if text_lower.contains(word) {
            neg_count += 1;
        }
    }
    
    if pos_count == 0 && neg_count == 0 {
        return 0.0;
    }
    
    let total = pos_count + neg_count;
    let sentiment = (pos_count as f64 - neg_count as f64) / total as f64;
    sentiment.clamp(-1.0, 1.0)
}

pub fn dedupe_by_text(events: &mut Vec<api::NewsEvent>) {
    // In-memory dedupe for this batch
    let mut seen = HashSet::new();
    events.retain(|e| seen.insert((format!("{:?}", e.source), e.text.clone())));
}

pub fn events_to_infer(symbol: &str, events: &[api::NewsEvent], now_ms: i64) -> api::InferRequest {
    let mut severity: f64 = 0.0;
    let mut quality: f64 = 0.0;
    let mut recency_min = f64::INFINITY;
    let mut s_acc = 0.0;
    let mut n = 0.0;
    for e in events.iter().filter(|e| e.symbol == symbol) {
        severity = severity.max(e.severity);
        quality = quality.max(e.source_quality);
        let dt_min = ((now_ms - e.published_ms).max(0) as f64) / 60000.0;
        recency_min = recency_min.min(dt_min);
        s_acc += baseline_sentiment(&e.text);
        n += 1.0;
    }
    let sentiment = if n > 0.0 {
        (s_acc / n).clamp(-1.0, 1.0)
    } else {
        0.0
    };

    let mut features = std::collections::BTreeMap::new();
    features.insert("sentiment".into(), ir::FeatureValue::F64(sentiment));
    features.insert("severity".into(), ir::FeatureValue::F64(severity));
    features.insert("source_quality".into(), ir::FeatureValue::F64(quality));
    features.insert(
        "recency_min".into(),
        ir::FeatureValue::F64(if recency_min.is_finite() {
            recency_min
        } else {
            999.0
        }),
    );
    api::InferRequest {
        symbol: symbol.into(),
        features,
        metadata: std::collections::BTreeMap::new(),
    }
}
