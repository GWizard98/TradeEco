# TradeEco Commands

Below are common commands side-by-side with descriptions. Run from repo root unless noted; you are currently in `hq/` so omit leading `cd hq &&`.

| Command | Description |
|---|---|
| `RUST_LOG=info cargo run -p hq --no-default-features` | Run HQ minimal core (AlphaScout, RiskSmith, Guardian, Ledger, Portfolio, Meta). |
| `RUST_LOG=info cargo run -p hq --no-default-features --features full` | Run full ecosystem (adds Regime, Sentiment/Oracle+Sentinel, Pathfinder paper exec, BrokerGate, Alerts, Metrics). |
| `HQ_MODE=backtest RUST_LOG=info cargo run -p hq --no-default-features` | Backtest (minimal build). Writes summary to stdout. |
| `HQ_MODE=paper RUST_LOG=info cargo run -p hq --no-default-features --features pathfinder` | Paper trading in simulator; writes fills (ledger), TCA, audit logs. |
| `HQ_MODE=live_oanda RUST_LOG=info cargo run -p hq --no-default-features --features brokergate` | Live trading via Oanda (requires `OANDA_*` env vars and allowlisted egress). |
| `export ALERTS=1` | Enable local notifications for high-severity audit events (requires `--features alerts`). |
| `curl -s http://127.0.0.1:8088/healthz` | Health endpoint (requires `--features metrics`). |
| `curl -s http://127.0.0.1:8088/ready` | Readiness endpoint (requires `--features metrics`). |
| `curl -s http://127.0.0.1:8088/metrics` | Prometheus metrics (requires `--features metrics`). |
| `HQ_HEALTH_PORT=8089 RUST_LOG=info cargo run -p hq --no-default-features --features metrics` | Run metrics server on custom port 8089. |
| `tail -n 100 logs/audit.jsonl` | View recent audit events. |
| `tail -n 100 logs/tca.jsonl` | View recent TCA records (paper mode). |
| `sqlite3 logs/trade_ledger_paper.db '.tables'` | Inspect ledger DB tables for paper mode. |
| `sqlite3 logs/trade_ledger_paper.db 'SELECT * FROM orders ORDER BY ts_ms DESC LIMIT 5;'` | Show last 5 orders (paper). |
| `cargo build -p hq --no-default-features [--features full]` | Build HQ (minimal or full). |
| `cargo test -p hq --no-default-features [--features full]` | Run HQ tests (both configs). |
| `cargo fmt --all -- --check` | Check formatting. |
| `cargo clippy -p hq --no-default-features [--features full] -- -D warnings` | Lint with clippy (treat warnings as errors). |
| `cargo clean` | Clean build artifacts. |
| `RUST_LOG=info cargo run -p hq --no-default-features --features regime-onnx` | Enable ONNX for Regime (if models provided). |
| `RUST_LOG=info cargo run -p hq --no-default-features --features sentiment-onnx` | Enable ONNX for Sentiment (if models provided). |
| `RUST_LOG=info cargo run -p hq --no-default-features --features pathfinder-onnx` | Enable ONNX for Pathfinder impact model (if models provided). |
| `HQ_KILL=1 RUST_LOG=info cargo run -p hq ...` | Kill switch: runs but aborts decisions (safety off switch). |
| `$EDITOR hq/config/policy.toml` | Edit policy thresholds/caps. |
| `$EDITOR hq/config/instruments.toml` | Edit instrument specs (tick/lot/min-notional). |

Notes
- Run minimal core unless you need optional agents. Add features selectively (e.g., `--features pathfinder`), or use `--features full`.
- For live Oanda, set: `export OANDA_HOST=api-fxpractice.oanda.com; export OANDA_ACCOUNT_ID={{OANDA_ACCOUNT_ID}}; export OANDA_API_KEY={{OANDA_API_KEY}}`.
