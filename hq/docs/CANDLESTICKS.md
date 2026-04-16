# Candlestick Patterns (Cheat Sheet)

This guide maps common price-action candlestick patterns to TradeEco exits. Use with `exit_policy.toml` (`[exit.structure] patterns.*`).

Core patterns
- Pin bar (hammer/shooting star)
  - Long rejection wick against the trend; potential reversal. Near resistance (for longs) tighten stop or take profit.
- Engulfing (bullish/bearish)
  - Body fully engulfs prior body; momentum shift. Use as confirmation to trail to swing.
- Inside bar / Breakout
  - Consolidation; breakout continuation or reversal. Tighten S/L under/over mother bar; partial at first trouble area.
- Doji / Spinning top
  - Indecision; context-dependent. Near strong S/R, consider partial exits.

How TradeEco can use them
- Enable `patterns.use_engulfing` / `patterns.use_pinbar` in `config/exit_policy.toml`.
- With structure mode, when a bearish pattern appears near resistance (in a long), trail to last swing or take partial (FTA). Reverse roles for shorts.
- Combine with channels: if at band edge + bearish pattern (long), prefer exit/partial.

Notes
- Patterns are context-driven (trend, S/R); avoid using them in isolation.
- HQ doesn’t parse raw bars yet; provide pattern flags via your feature pipeline or set conservative buffers (`buffer_pips`/`buffer_atr_mult`) and partials.