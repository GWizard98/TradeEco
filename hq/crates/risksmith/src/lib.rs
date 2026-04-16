use anyhow::Result;

#[derive(Default, Debug)]
pub struct RiskSmith;

impl api::Sizer for RiskSmith {
    fn name(&self) -> &'static str {
        "risksmith"
    }
    fn size(&self, req: &api::SizingRequest) -> Result<api::SizingResponse> {
        // 0) Load exit policy (best-effort)
        #[derive(serde::Deserialize, Clone)]
        struct ExitCfg {
            mode: Option<String>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct ExitPolicy {
            exit: Option<ExitCfg>,
            overrides: Option<Overrides>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct Overrides {
            fixed_tp_atr_mult: Option<f64>,
            fixed_sl_atr_mult: Option<f64>,
            stop_pips: Option<f64>,
            tp_pips: Option<f64>,
        }
        let (exit_mode, tp_mult, sl_mult, stop_pips, tp_pips) = (|| {
            if let Ok(txt) = std::fs::read_to_string("config/exit_policy.toml") {
                if let Ok(cfg) = toml::from_str::<ExitPolicy>(&txt) {
                    let mode = cfg
                        .exit
                        .and_then(|e| e.mode)
                        .unwrap_or_else(|| "swing".into());
                    let ov = cfg.overrides.unwrap_or(Overrides {
                        fixed_tp_atr_mult: None,
                        fixed_sl_atr_mult: None,
                        stop_pips: None,
                        tp_pips: None,
                    });
                    let tp = ov.fixed_tp_atr_mult.unwrap_or(2.0);
                    let sl = ov.fixed_sl_atr_mult.unwrap_or(2.0);
                    return (mode, tp, sl, ov.stop_pips, ov.tp_pips);
                }
            }
            ("swing".into(), 2.0, 2.0, None, None)
        })();

        // 0b) Load risk policy (per-trade risk pct)
        #[derive(serde::Deserialize, Clone)]
        struct RiskCaps {
            per_trade_risk_pct: Option<f64>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct Policy {
            risk: Option<RiskCaps>,
        }
        let per_trade_risk_frac: f64 = (|| {
            if let Ok(txt) = std::fs::read_to_string("config/policy.toml") {
                if let Ok(cfg) = toml::from_str::<Policy>(&txt) {
                    if let Some(r) = cfg.risk {
                        return r.per_trade_risk_pct.unwrap_or(0.0).max(0.0);
                    }
                }
            }
            0.0
        })();

        // 1) Base risk dollars
        let conf = (1.0 - req.uncertainty).clamp(0.0, 1.0);
        let regime = req.regime_fit.clamp(0.0, 1.0);
        let edge_ok = req.ev_net > 0.0 && conf > 0.2;
        let mut risk_dollars = if per_trade_risk_frac > 0.0 {
            req.account_equity * per_trade_risk_frac
        } else {
            // legacy fraction
            let mut risk_frac = 0.01 * conf * regime;
            if !edge_ok {
                risk_frac = 0.0;
            }
            req.account_equity * risk_frac
        };

        // 2) Temper risk dollars with cooldown/drawdown/liquidity/correlation
        if req.loss_cooldown.unwrap_or(false) {
            risk_dollars *= 0.25;
        }
        if let Some(dd) = req.recent_max_drawdown {
            if dd > 0.1 {
                risk_dollars *= 0.5;
            }
            if dd > 0.2 {
                risk_dollars *= 0.25;
            }
        }
        if let Some(impact) = req.exec_impact {
            if impact > 0.3 {
                risk_dollars *= 0.5;
            }
        }
        if let Some(corr) = req.corr_to_portfolio {
            if corr > 0.7 {
                risk_dollars *= 0.5;
            }
        }
        if let Some(sector_exp) = req.sector_exposure_notional {
            if sector_exp > 0.3 * req.portfolio_notional_cap {
                risk_dollars *= 0.7;
            }
        }

        // 3) ATR baseline
        // If ATR missing and FX pip_size known, default to 10 pips as coarse ATR
        #[derive(serde::Deserialize)]
        struct InstCfg {
            pip_size: Option<f64>,
            pip_value_per_unit: Option<f64>,
        }
        #[derive(serde::Deserialize)]
        struct Instruments {
            symbols: std::collections::HashMap<String, InstCfg>,
        }
        let (pip_size, pip_val_per_unit) = (|| {
            if let Ok(txt) = std::fs::read_to_string("config/instruments.toml") {
                if let Ok(cfg) = toml::from_str::<Instruments>(&txt) {
                    if let Some(ic) = cfg.symbols.get(&req.symbol) {
                        return (
                            ic.pip_size.unwrap_or(0.0),
                            ic.pip_value_per_unit.unwrap_or(0.0),
                        );
                    }
                }
            }
            (0.0, 0.0)
        })();
        let mut atr = if req.atr.is_finite() && req.atr > 0.0 {
            req.atr
        } else {
            0.0
        };
        if atr <= 0.0 {
            atr = if pip_size > 0.0 {
                10.0 * pip_size
            } else {
                req.price * 0.01
            };
        }
        let base_stop = 2.0 * atr;
        if base_stop <= 0.0 || req.price <= 0.0 {
            return Ok(api::SizingResponse {
                qty: 0.0,
                stop_loss: None,
                take_profit: None,
                ttl_sec: Some(3600),
                rationale: "invalid inputs".into(),
            });
        }

        // 4) Exits per policy -> propose SL/TP/TTL
        let (sl, tp, ttl) = match exit_mode.as_str() {
            "swing" => {
                // Prefer explicit pips if provided and pip_size known
                let sl = if let Some(sp) = stop_pips.filter(|_| pip_size > 0.0) {
                    let d = sp.abs() * pip_size;
                    if req.score >= 0.0 {
                        Some((req.price - d).max(0.0))
                    } else {
                        Some(req.price + d)
                    }
                } else if req.score >= 0.0 {
                    Some((req.price - sl_mult * atr).max(0.0))
                } else {
                    Some(req.price + sl_mult * atr)
                };
                let tp = if let Some(tpip) = tp_pips.filter(|_| pip_size > 0.0) {
                    let d = tpip.abs() * pip_size;
                    if req.score >= 0.0 {
                        Some(req.price + d)
                    } else {
                        Some((req.price - d).max(0.0))
                    }
                } else if req.score >= 0.0 {
                    Some(req.price + tp_mult * atr)
                } else {
                    Some((req.price - tp_mult * atr).max(0.0))
                };
                (sl, tp, Some(2 * 3600))
            }
            "trend_ma" | "trend_structure" => {
                let k = 3.0_f64.max(sl_mult);
                let sl = if req.score >= 0.0 {
                    Some((req.price - k * atr).max(0.0))
                } else {
                    Some(req.price + k * atr)
                };
                (sl, None, Some(6 * 3600))
            }
            _ => {
                let sl = if req.score >= 0.0 {
                    Some((req.price - sl_mult * atr).max(0.0))
                } else {
                    Some(req.price + sl_mult * atr)
                };
                let tp = if req.score >= 0.0 {
                    Some(req.price + tp_mult * atr)
                } else {
                    Some((req.price - tp_mult * atr).max(0.0))
                };
                (sl, tp, Some(2 * 3600))
            }
        };
        let stop_dist = sl.map(|s| (req.price - s).abs()).unwrap_or(base_stop);

        // 5) FX pip-based sizing if available (reuse pip_size/pip_val_per_unit)
        let mut qty = 0.0;
        if risk_dollars > 0.0 {
            if pip_size > 0.0 && pip_val_per_unit > 0.0 {
                let stop_pips = (stop_dist / pip_size).abs();
                if stop_pips > 0.0 {
                    qty = risk_dollars / (stop_pips * pip_val_per_unit);
                }
            }
            if qty <= 0.0 && stop_dist > 0.0 {
                // generic fall-back: price-distance sizing
                qty = risk_dollars / stop_dist;
            }
        }

        // 6) Leverage and notional caps
        let notional = qty * req.price;
        let max_by_leverage = req.account_equity * req.leverage_max;
        let max_notional = max_by_leverage
            .min(req.symbol_notional_cap)
            .min(req.portfolio_notional_cap);
        let mut qty_clamped = qty;
        if notional > max_notional && req.price > 0.0 {
            qty_clamped = max_notional / req.price;
        }

        Ok(api::SizingResponse {
            qty: qty_clamped.max(0.0),
            stop_loss: sl,
            take_profit: tp,
            ttl_sec: ttl,
            rationale: format!(
                "risk sizing (exit_policy={}{}), atr={:.3}",
                exit_mode,
                if per_trade_risk_frac > 0.0 {
                    format!(", per_trade_risk_pct={:.2}%", per_trade_risk_frac * 100.0)
                } else {
                    "".into()
                },
                atr
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::Sizer;

    #[test]
    fn zero_edge_abstains() {
        let s = RiskSmith::default();
        let req = api::SizingRequest {
            symbol: "TEST".into(),
            price: 100.0,
            account_equity: 100_000.0,
            score: 0.0,
            uncertainty: 0.9,
            ev_net: -0.01,
            regime_fit: 0.5,
            atr: 1.0,
            leverage_max: 2.0,
            symbol_notional_cap: 50_000.0,
            portfolio_notional_cap: 100_000.0,
            exec_impact: Some(0.1),
            corr_to_portfolio: None,
            sector_exposure_notional: None,
            recent_max_drawdown: None,
            loss_cooldown: None,
        };
        let out = s.size(&req).unwrap();
        assert_eq!(out.qty, 0.0);
    }

    #[test]
    fn higher_conf_more_size() {
        let s = RiskSmith::default();
        let mut req = api::SizingRequest {
            symbol: "TEST".into(),
            price: 100.0,
            account_equity: 100_000.0,
            score: 1.0,
            uncertainty: 0.5,
            ev_net: 0.02,
            regime_fit: 0.8,
            atr: 2.0,
            leverage_max: 2.0,
            symbol_notional_cap: 50_000.0,
            portfolio_notional_cap: 100_000.0,
            exec_impact: Some(0.05),
            corr_to_portfolio: Some(0.2),
            sector_exposure_notional: Some(10000.0),
            recent_max_drawdown: Some(0.0),
            loss_cooldown: Some(false),
        };
        let a = s.size(&req).unwrap().qty;
        req.uncertainty = 0.2;
        let b = s.size(&req).unwrap().qty;
        assert!(b > a);
    }
}
