use std::env;

fn main() {
    let symbol = env::args().nth(1).unwrap_or_else(|| "USDMXN".to_string());
    let mut events = sentinel::collectors::collect_all_sources(&symbol);
    // Sort by freshness desc
    events.sort_by_key(|e| std::cmp::Reverse(e.published_ms));

    println!("symbol={} total_events={}", symbol, events.len());
    let now = chrono::Utc::now().timestamp_millis();
    let mut shown = 0;
    for e in events.iter() {
        // Filter for MXN-specific content
        let tl = e.text.to_lowercase();
        let is_mxn = tl.contains("mxn") || tl.contains("usd/mxn") || tl.contains("mexico") || tl.contains("mexican peso") || tl.contains("banxico") || tl.contains("peso");
        if !is_mxn { continue; }
        let age_min = ((now - e.published_ms).max(0) as f64) / 60000.0;
        let s = sentinel::baseline_sentiment(&e.text);
        println!(
            "{:>6.1}m | src={:?} sev={:.2} qual={:.2} sent={:+.2} | {}",
            age_min, e.source, e.severity, e.source_quality, s, e.text.replace('\n', " ")
        );
        shown += 1;
        if shown >= 12 { break; }
    }
    if shown == 0 { println!("(no MXN-specific headlines in recent fetch)"); }
}
