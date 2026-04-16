# CG Automated Trader (HQ)

Rust-based trading HQ orchestrating agents for signal, risk, execution, and audit. Python CLI was removed to downsize; use the Rust binary.

## Getting Started
- Install Rust toolchain (stable)
- Build and run (minimal core):
  - `cd hq && RUST_LOG=info cargo run -p hq --no-default-features`
- Full ecosystem (all agents/services):
  - `cd hq && RUST_LOG=info cargo run -p hq --no-default-features --features full`

## Structure
- `hq/crates/hq` – entrypoint binary (Headquarters)
- Core agents always included: AlphaScout (signals), RiskSmith (sizing), Guardian (validation/audit), Ledger, Portfolio, Meta
- Optional features: `regime`, `sentiment` (with `sentinel` collectors), `pathfinder` (paper exec + TCA), `brokergate` (live Oanda), `alerts`, `metrics`

## Modes
- Backtest: `HQ_MODE=backtest`
- Paper: `HQ_MODE=paper` (requires `--features pathfinder`)
- Live Oanda: `HQ_MODE=live_oanda` (requires `--features brokergate`)
