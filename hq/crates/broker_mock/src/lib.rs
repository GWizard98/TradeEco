use anyhow::Result;

pub fn execute(
    order: &api::OrderRequest,
    spec: &api::InstrumentSpec,
    mid_price: f64,
) -> Result<api::ExecReport> {
    // Enforce lot step and min/max
    let step_qty = api::round_step_floor(order.qty, spec.lot_step);
    let notional = step_qty * mid_price;
    if step_qty < spec.min_qty || notional < spec.min_notional {
        return Ok(api::ExecReport {
            idempotency_key: order.idempotency_key.clone(),
            side: match order.side { api::OrderSide::Buy => "Buy".to_string(), api::OrderSide::Sell => "Sell".to_string() },
            symbol: order.symbol.clone(),
            filled_qty: 0.0,
            avg_price: 0.0,
            slippage_bps: 0.0,
            status: "REJECT_MIN".into(),
        });
    }

    // Dynamic slippage model: base + size impact
    let size_frac = if spec.max_qty > 0.0 {
        (step_qty / spec.max_qty).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let slippage_bps = 2.0 + 25.0 * size_frac;
    let price = mid_price
        * (1.0
            + slippage_bps / 10_000.0
                * match order.side {
                    api::OrderSide::Buy => 1.0,
                    api::OrderSide::Sell => -1.0,
                });

    Ok(api::ExecReport {
        idempotency_key: order.idempotency_key.clone(),
        side: match order.side { api::OrderSide::Buy => "Buy".to_string(), api::OrderSide::Sell => "Sell".to_string() },
        symbol: order.symbol.clone(),
        filled_qty: step_qty,
        avg_price: price,
        slippage_bps,
        status: "FILLED".into(),
    })
}
