use std::collections::HashSet;
use std::env;
use std::thread;
use std::time::Duration;

fn main() {
    let symbol = env::args().nth(1).unwrap_or_else(|| "USDMXN".to_string());
    let threshold: f64 = env::var("NEWS_SEVERITY_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.7);
    let window_min: f64 = env::var("NEWS_WINDOW_MIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90.0);
    let interval_secs: u64 = env::var("NEWS_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(180);
    let once = env::var("ONCE").ok().as_deref() == Some("1");

    let mut notified: HashSet<String> = HashSet::new();

    loop {
        let mut events = sentinel::collectors::collect_all_sources(&symbol);
        sentinel::dedupe_by_text(&mut events);
        let now = chrono::Utc::now().timestamp_millis();
        let mut hits = 0;
        for e in events.iter().filter(|e| e.symbol == symbol) {
            let age_min = ((now - e.published_ms).max(0) as f64) / 60000.0;
            if age_min > window_min { continue; }
            if e.severity < threshold { continue; }
            let key = sentinel::make_key(&e.source, &e.text);
            if notified.contains(&key) { continue; }
            let s = sentinel::baseline_sentiment(&e.text);
            let msg = format!(
                "NEWS ALERT {}: src={:?} sev={:.2} qual={:.2} sent={:+.2} age={:.1}m | {}",
                symbol, e.source, e.severity, e.source_quality, s, age_min, e.text.replace('\n', " ")
            );
            let evt = guardian::AuditEvent { category: "news_alert".into(), message: msg.clone(), severity: "WARN".into() };
            let _ = guardian::write_audit(&evt);
            #[cfg(feature = "alerts")]
            alerts::maybe_notify("MXN News Alert", &msg, &evt.severity);
            println!("{}", msg);
            notified.insert(key);
            hits += 1;
        }
        if hits == 0 {
            println!("{}: no qualifying news (sev>={:.2} within {}m)", symbol, threshold, window_min);
        }
        if once { break; }
        thread::sleep(Duration::from_secs(interval_secs));
    }
}
