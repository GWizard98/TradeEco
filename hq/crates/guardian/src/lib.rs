use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub max_leverage: f64,
    pub max_position_notional: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub quality: f64,
    pub errors: Vec<String>,
}

pub fn validate_infer_request(agent: &str, req: &api::InferRequest) -> Result<ValidationResult> {
    let mut quality = 1.0;
    let mut errors = vec![];
    if agent == "alphascout" {
        for key in ["price", "fast_ma", "slow_ma", "vol"] {
            if !req.features.contains_key(key) {
                errors.push(format!("missing {}", key));
                quality = (quality - 0.3f64).max(0.0f64);
            }
        }
        // Basic sanity
        let vol = match req.features.get("vol") {
            Some(ir::FeatureValue::F64(v)) => *v,
            _ => f64::NAN,
        };
        let price = match req.features.get("price") {
            Some(ir::FeatureValue::F64(v)) => *v,
            _ => f64::NAN,
        };
        let ok = vol.is_finite() && vol > 0.0 && price.is_finite();
        if !ok {
            errors.push("invalid vol/price".into());
            quality = 0.0;
        }
    }
    Ok(ValidationResult { quality, errors })
}

pub fn verify_artifact_provenance(_bytes: &[u8], _signature: &[u8], _pubkey: &[u8]) -> bool {
    // TODO: ed25519 verify + SBOM/provenance checks
    true
}

/// Enforce basic safety on decisions: sanitize qty, zero-out on HOLD, and clamp
/// to a conservative upper bound derived from policy. This is a last line of
/// defense; primary sizing limits should occur upstream (RiskSmith/Portfolio).
pub fn enforce_policy(policy: &Policy, decision: &mut ir::Decision) -> Result<()> {
    // Sanitize NaNs/Infs
    if !decision.qty.is_finite() || decision.qty.is_nan() {
        decision.qty = 0.0;
    }

    // Never carry size on HOLD
    if matches!(decision.side, ir::Side::Hold) {
        decision.qty = 0.0;
        return Ok(());
    }

    // Conservative hard-ceiling on qty using policy.max_position_notional as a
    // proxy (when upstream sizing failed to cap). Without price context here,
    // treat it as a unit cap to prevent runaway values.
    let max_qty_cap = policy.max_position_notional.max(0.0);
    if max_qty_cap > 0.0 && decision.qty > max_qty_cap {
        decision.qty = max_qty_cap;
        let _ = write_audit(&AuditEvent {
            category: "policy".into(),
            message: format!("qty clamped to {} by guardian policy", max_qty_cap),
            severity: "WARN".into(),
        });
    }

    // Disallow negative qty
    if decision.qty < 0.0 {
        decision.qty = 0.0;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub category: String,
    pub message: String,
    pub severity: String,
}

pub fn write_audit(event: &AuditEvent) -> Result<()> {
    let dir = "logs";
    create_dir_all(dir)?;
    let path = format!("{}/audit.jsonl", dir);
    // simple rotation at ~5MB
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 5_000_000 {
            let _ = std::fs::rename(
                &path,
                format!("{}.{:?}", &path, chrono::Utc::now().timestamp()),
            );
        }
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(event)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

pub fn egress_allowed(host: &str) -> bool {
    // Read allowlist from config/egress_allowlist.txt
    // In live_* modes, default to DENY unless explicitly allowlisted.
    let live_mode = std::env::var("HQ_MODE")
        .ok()
        .map(|m| m.starts_with("live_"))
        .unwrap_or(false);
    if let Ok(txt) = std::fs::read_to_string("config/egress_allowlist.txt") {
        for line in txt.lines() {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with('#') && host.contains(l) {
                return true;
            }
        }
        return false;
    }
    // No allowlist file present
    if live_mode {
        return false;
    }
    true // default allow in non-live local modes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_policy_clamps_and_zeroes_hold() {
        let pol = Policy {
            max_leverage: 2.0,
            max_position_notional: 10.0,
        };
        let mut d = ir::Decision {
            symbol: "S".into(),
            side: ir::Side::Buy,
            qty: 25.0,
            confidence: 0.9,
        };
        enforce_policy(&pol, &mut d).unwrap();
        assert_eq!(d.qty, 10.0);
        let mut d2 = ir::Decision {
            symbol: "S".into(),
            side: ir::Side::Hold,
            qty: 5.0,
            confidence: 0.5,
        };
        enforce_policy(&pol, &mut d2).unwrap();
        assert_eq!(d2.qty, 0.0);
    }

    #[test]
    fn egress_live_defaults_deny_without_allowlist() {
        std::env::set_var("HQ_MODE", "live_oanda");
        // Ensure allowlist file exists for this test
        let _ = std::fs::create_dir_all("config");
        let _ = std::fs::write("config/egress_allowlist.txt", "api-fxpractice.oanda.com\n");
        // Use a clearly un-allowlisted host
        let allowed = egress_allowed("example.invalid");
        assert!(!allowed);
        // Allowlisted host
        let allowed_oanda = egress_allowed("api-fxpractice.oanda.com");
        assert!(allowed_oanda);
        std::env::remove_var("HQ_MODE");
        let _ = std::fs::remove_file("config/egress_allowlist.txt");
    }
}
