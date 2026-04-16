// Placeholder collectors for external sources; implement HTTP fetch + parsing
// Ensure domains are in Guardian egress allowlist.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static RL: Lazy<Mutex<HashMap<String, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const MIN_INTERVAL: Duration = Duration::from_secs(60);

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    let tl = text.to_lowercase();
    keywords.iter().any(|k| tl.contains(k))
}

fn calculate_severity(text: &str) -> f64 {
    let high_severity_words = [
        "crash", "crisis", "emergency", "collapse", "plunge", "surge", "breaking",
        "alert", "warning", "shock", "unprecedented", "massive", "historic"
    ];
    let medium_severity_words = [
        "significant", "major", "important", "concern", "impact", "change",
        "announce", "report", "update", "move", "shift"
    ];
    
    let text_lower = text.to_lowercase();
    let mut high_count = 0;
    let mut med_count = 0;
    
    for word in high_severity_words.iter() {
        if text_lower.contains(word) {
            high_count += 1;
        }
    }
    
    for word in medium_severity_words.iter() {
        if text_lower.contains(word) {
            med_count += 1;
        }
    }
    
    if high_count > 0 {
        (0.8 + (high_count as f64 * 0.1)).min(1.0)
    } else if med_count > 0 {
        (0.4 + (med_count as f64 * 0.1)).min(0.7)
    } else {
        0.2
    }
}

fn parse_rss(
    url: &str,
    source: api::NewsSource,
    source_quality: f64,
    symbol_filter: Option<&str>,
) -> Vec<api::NewsEvent> {
    let host_owned;
    let host = match reqwest::Url::parse(url) {
        Ok(u) => {
            host_owned = u.host_str().unwrap_or("").to_string();
            &host_owned
        }
        Err(_) => {
            return vec![];
        },
    };
    if !guardian::egress_allowed(host) {
        return vec![];
    }
    // Simple per-host rate limit
    {
        let mut rl = RL.lock().unwrap();
        if let Some(last) = rl.get(host) {
            if last.elapsed() < MIN_INTERVAL {
                return vec![];
            }
        }
        rl.insert(host.to_string(), Instant::now());
    }

    let mut out = vec![];
    
    // Build HTTP client with timeout
    let client = match reqwest::blocking::ClientBuilder::new()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.3 Safari/605.1.15")
        .build() {
        Ok(c) => c,
        Err(_e) => {
            return vec![];
        }
    };
    
    let resp = match client
        .get(url)
        .header("Accept", "application/rss+xml, application/xml, text/xml;q=0.9, */*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Cache-Control", "no-cache")
        .send() {
        Ok(r) => {
            if !r.status().is_success() {
                return vec![];
            }
            r
        },
        Err(_e) => {
            return vec![];
        }
    };
    
    let bytes = match resp.bytes() {
        Ok(b) => b,
        Err(_e) => {
            return vec![];
        }
    };
    
    let channel = match rss::Channel::read_from(&bytes[..]) {
        Ok(c) => c,
        Err(_e) => {
            return vec![];
        }
    };
    
    for item in channel.items() {
                    let title = item.title().unwrap_or("").to_string();
                    let full_text = item.description().unwrap_or(&title).to_string();
                    if let Some(sym) = symbol_filter {
                        if !title.contains(sym) && !full_text.contains(sym) {
                            continue;
                        }
                    }
                    let pub_ms = item
                        .pub_date()
                        .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                        .map(|dt| dt.timestamp_millis())
                        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
                    // Cross-run dedupe by hash key
                    let key = crate::make_key(&source, &full_text);
                    crate::load_seen();
                    {
                        let seen = crate::SEEN.lock().unwrap();
                        if seen.contains(&key) {
                            continue;
                        }
                    }
                    // Determine severity based on keywords
                    let severity = calculate_severity(&full_text);
                    
                    out.push(api::NewsEvent {
                        symbol: symbol_filter.unwrap_or("").to_string(),
                        text: full_text,
                        source: source.clone(),
                        severity,
                        source_quality,
                        published_ms: pub_ms,
                    });
                    crate::persist_seen(&key);
    }
    out
}

pub mod yahoo {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        let url = format!(
            "https://feeds.finance.yahoo.com/rss/2.0/headline?s={}&lang=en-US",
            symbol
        );
        let mut events = parse_rss(&url, api::NewsSource::YahooFinance, 0.7, Some(symbol));
        events.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico"]));
        events
    }
}

pub mod forexnews {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        // Try multiple FX news sources with graceful fallback
        let mut all = vec![];
        // Original ForexNews
        {
            let mut ev = parse_rss(
                "https://www.forexnews.com/feed/",
                api::NewsSource::ForexNews,
                0.6,
                None,
            );
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        // ForexLive
        {
            let mut ev = parse_rss(
                "https://www.forexlive.com/feed/",
                api::NewsSource::Other("ForexLive".into()),
                0.7,
                None,
            );
            ev.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico peso"]));
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        // DailyFX market news
        {
            let mut ev = parse_rss(
                "https://www.dailyfx.com/feeds/market-news",
                api::NewsSource::Other("DailyFX".into()),
                0.7,
                None,
            );
            ev.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico peso"]));
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        all
    }
}

pub mod tradingview {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        // TradingView may not expose RSS for news; placeholder
        let url = "https://www.tradingview.com/rss/markets/";
        let mut events = parse_rss(url, api::NewsSource::TradingView, 0.5, None);
        for e in &mut events {
            e.symbol = symbol.to_string();
        }
        events
    }
}

pub mod fxstreet {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        let url = "https://www.fxstreet.com/rss";
        let mut events = parse_rss(url, api::NewsSource::FXStreet, 0.7, None);
        for e in &mut events {
            e.symbol = symbol.to_string();
        }
        events
    }
}

pub mod marketwatch {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        let url = format!("https://feeds.content.dowjones.io/public/rss/mw_topstories");
        let mut events = parse_rss(&url, api::NewsSource::Other("MarketWatch".to_string()), 0.8, Some(symbol));
        for e in &mut events {
            if e.symbol.is_empty() {
                e.symbol = symbol.to_string();
            }
        }
        events
    }
}

pub mod reuters {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        let mut all = vec![];
        // Legacy Reuters feed
        {
            let mut ev = parse_rss(
                "https://feeds.reuters.com/reuters/businessNews",
                api::NewsSource::Other("Reuters".to_string()),
                0.9,
                None,
            );
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        // New Reuters markets feed (Americas)
        {
            let mut ev = parse_rss(
                "https://www.reuters.com/markets/americas/rss",
                api::NewsSource::Other("Reuters".to_string()),
                0.9,
                None,
            );
            ev.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico"]));
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        // Reuters world/americas feed
        {
            let mut ev = parse_rss(
                "https://www.reuters.com/world/americas/rss",
                api::NewsSource::Other("Reuters".to_string()),
                0.9,
                None,
            );
            ev.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico"]));
            for e in &mut ev { e.symbol = symbol.to_string(); }
            all.extend(ev);
        }
        all
    }
}

pub mod bloomberg {
    use super::*;
    pub fn fetch_latest(symbol: &str) -> Vec<api::NewsEvent> {
        // Bloomberg RSS is subscription-based, using a fallback URL
        let url = "https://feeds.bloomberg.com/markets/news.rss";
        let mut events = parse_rss(url, api::NewsSource::Bloomberg, 0.9, None);
        events.retain(|e| contains_any(&e.text, &["mxn", "usd/mxn", "mexican peso", "banxico", "mexico"]));
        for e in &mut events {
            e.symbol = symbol.to_string();
        }
        events
    }
}

pub fn collect_all_sources(symbol: &str) -> Vec<api::NewsEvent> {
    let mut all_events = Vec::new();
    
    // Collect from all sources in parallel would be better, but keeping simple for now
    all_events.extend(yahoo::fetch_latest(symbol));
    all_events.extend(forexnews::fetch_latest(symbol));
    all_events.extend(fxstreet::fetch_latest(symbol));
    all_events.extend(marketwatch::fetch_latest(symbol));
    all_events.extend(reuters::fetch_latest(symbol));
    all_events.extend(bloomberg::fetch_latest(symbol));
    
    // Deduplicate
    crate::dedupe_by_text(&mut all_events);
    
    all_events
}
