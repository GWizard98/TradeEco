# Exit Strategies (TradeEco)

This document defines two exit approaches the ecosystem can reference when forming stops/targets. It is a human-readable spec and a config companion (hq/config/exit_policy.toml). Wiring into runtime sizing/execution may require code changes.

1) Swing Capture (one-move exits)
- Intent: take profit before likely reversal; less heat, risk of cutting winners.
- Triggers (any):
  - Price reaches recent swing low/high in opposing direction
  - Price tags/support-resistance zone (local SR window)
  - Price reaches channel boundary (e.g., lower band in up-move, upper band in down-move)
- Guidance:
  - Use structure: exit longs near prior swing high; exit shorts near prior swing low
  - Use channel lookback 20–50 bars; SR lookback 50–200 bars
- Pros: lower drawdowns, faster recycle of capital
- Cons: miss extended trends; more frequent trades

2)
 Trend Riding (trailing exits)
- Intent: stay in trend until meaningful break; tolerate pullbacks.
- Methods:
  A) Moving-average trail (e.g., 200-day)
     - Long: trail stop to MA; exit on close below MA (buffer optional)
     - Short: trail stop to MA; exit on close above MA
  B) Market structure trail
     - Long: trail to prior swing low (n-back); exit on break of last swing low
     - Short: trail to prior swing high (n-back); exit on break of last swing high
- Pros: participates in large moves; fewer decisions
- Cons: giveback on reversals; whipsaw risk

3) Structure-based exits (price action)
- Uptrend (higher highs/lows):
  - Trail stop below the prior swing low (n-back) with a buffer; exit on close below that level.
  - Take partial profits at the first trouble area (nearest resistance/previous swing high), and let remainder run.
- Downtrend (lower highs/lows):
  - Trail above the prior swing high (n-back) with a buffer; exit on close above that level.
  - Take partial profits at the next support/previous swing low.
- Channels: if price reaches the opposing channel boundary (Donchian/Bollinger/Keltner), take profit or tighten trailing stop.
- Candlestick confirmation (optional):
  - Longs: tighten/exit on bearish engulfing or pin-bar (long upper wick) near resistance.
  - Shorts: tighten/exit on bullish engulfing or pin-bar (long lower wick) near support.
  - See `hq/docs/CANDLESTICKS.md` for a quick reference.

Configuration mapping
- `exit.trend_ma.*`: MA period, confirm_on_close, buffer_bps.
- `exit.trend_structure.*`: `trail_n_swings`, `confirm_on_close`.
- `exit.structure.*`: price-action exits:
  - `trail_n_swings` (default 1), `confirm_on_close`, `buffer_pips` or `buffer_atr_mult`.
  - `sr_lookback_bars` (S/R window), `fta_partial_pct` (take-profit fraction at first trouble area).
  - `channel.enabled`, `channel.type` (donchian|bollinger|keltner), `channel.lookback_bars`.
  - `patterns.use_engulfing`, `patterns.use_pinbar`, `patterns.min_body_frac`.
- Overrides:
  - `overrides.stop_pips`, `overrides.tp_pips` (forces absolute pip distances for swing mode when pip_size known).

Implementation notes
- Position sizing supports fixed per-trade risk (e.g., 1%) when set in policy: `risk.per_trade_risk_pct = 0.01`.
- For FX, you can specify pip fields in `hq/config/instruments.toml`: `pip_size` and `pip_value_per_unit`; RiskSmith will size by `risk_dollars / (stop_pips * pip_value_per_unit)` when available.
- Exit overrides: under `[overrides]`, you can set `stop_pips` and `tp_pips` to force absolute pip distances for swing exits; otherwise ATR multiples are used.
- ATR/vol feed: HQ now attaches an `atr` feature (uses input vol as proxy if none). For FX without ATR, defaults to ~10 pips; you can override via `overrides.stop_pips`/`tp_pips`.
- Suggested precedence: explicit trailing exit overrides fixed TP; hard policy kill switch always applies.
- Indicators (MA, swings, channels) must be provided by the signal stack or a feature pipeline.