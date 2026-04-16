pub fn maybe_notify(title: &str, body: &str, severity: &str) {
    if std::env::var("ALERTS").ok().as_deref() != Some("1") {
        return;
    }
    // Only high severity
    if severity != "ERROR" && severity != "WARN" {
        return;
    }
    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .show();
}
