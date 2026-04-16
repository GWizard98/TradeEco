use anyhow::Result;

mod ml;

// Simple router: enforce tick/lot/min, slice if qty > chunk, then send to mock broker
pub fn route_and_execute(
    order: &api::OrderRequest,
    spec: &api::InstrumentSpec,
    mid_price: f64,
) -> Result<api::ExecReport> {
    let mut remaining = order.qty.max(0.0);

    // Use impact model (if enabled) to determine chunk fraction [0.05..0.25]
    let model = ml::get_model();
    let impact = model.score(mid_price, remaining);
    let frac = (0.15 - impact * 0.1).clamp(0.05, 0.25);

    let chunk = (spec.max_qty * frac).max(spec.min_qty);
    let mut filled = 0.0;
    let mut vwap = 0.0;
    let mut total_slip = 0.0;
    let mut n = 0.0;
    while remaining > 0.0 {
        let q = remaining.min(chunk);
        let child = api::OrderRequest {
            qty: q,
            ..order.clone()
        };
        let rpt = broker_mock::execute(&child, spec, mid_price)?;
        if rpt.filled_qty <= 0.0 && rpt.status.starts_with("REJECT") {
            break;
        }
        filled += rpt.filled_qty;
        vwap += rpt.filled_qty * rpt.avg_price;
        total_slip += rpt.slippage_bps * rpt.filled_qty;
        n += rpt.filled_qty;
        remaining -= q;
        // Stop if TTL/other constraints would require
        if n >= order.qty {
            break;
        }
    }
    let avg = if filled > 0.0 { vwap / filled } else { 0.0 };
    let slip = if n > 0.0 { total_slip / n } else { 0.0 };
    Ok(api::ExecReport {
        idempotency_key: order.idempotency_key.clone(),
        side: match order.side { api::OrderSide::Buy => "Buy".to_string(), api::OrderSide::Sell => "Sell".to_string() },
        symbol: order.symbol.clone(),
        filled_qty: filled,
        avg_price: avg,
        slippage_bps: slip,
        status: if filled > 0.0 {
            "FILLED".into()
        } else {
            "REJECT".into()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_do_not_exceed_order_qty() {
        let spec = api::InstrumentSpec {
            tick_size: 0.01,
            lot_step: 1.0,
            min_qty: 1.0,
            max_qty: 1000.0,
            min_notional: 10.0,
        };
        let order = api::OrderRequest {
            idempotency_key: "k".into(),
            symbol: "TEST".into(),
            side: api::OrderSide::Buy,
            qty: 50.0,
            order_type: api::OrderType::Market,
            ttl_sec: 60,
            stop_loss: None,
            take_profit: None,
            tp_partial_pct: None,
        };
        let rpt = route_and_execute(&order, &spec, 100.0).unwrap();
        assert!(rpt.filled_qty <= order.qty + 1e-6);
        assert!(rpt.filled_qty >= spec.min_qty);
    }
}
