# CMD_MANUAL.md

This document lists the canonical command sequences for the Headquarters (HQ) trading ecosystem. Update this file whenever new capabilities are added.

## Build and run

```bash path=null start=null
# Instruments config (tick/lot/min-notional per symbol)
$EDITOR hq/config/instruments.toml
```

```bash path=null start=null
# Edit policy thresholds and caps (optional)
$EDITOR hq/config/policy.toml
```

```bash path=null start=null
# From repo root (hq workspace)
cd hq
cargo build
RUST_LOG=info cargo run -p hq
```

### Feature flags and minimal/full runs

```bash path=null start=null
# Minimal core (no optional features)
RUST_LOG=info cargo run -p hq --no-default-features

# Full ecosystem (all optional features)
RUST_LOG=info cargo run -p hq --no-default-features --features full

# Run selected features (e.g., regime + pathfinder)
RUST_LOG=info cargo run -p hq --no-default-features --features regime,pathfinder
```

### Env overrides (quick price inputs)

```bash path=null start=null
# Provide inputs for one-shot run (symbol/price/MAs/ATR proxy)
HQ_SYMBOL=EUR_USD \
HQ_PRICE=1.1000 \
HQ_FAST=1.1005 \
HQ_SLOW=1.0990 \
HQ_VOL=0.0020 \
RUST_LOG=info cargo run -p hq --no-default-features --features regime
```

## Kill switch (global safety halt)

```bash path=null start=null
# Abort decisions immediately
cd hq
HQ_KILL=1 RUST_LOG=info cargo run -p hq
```

## Logs and audit

```bash path=null start=null
# Inspect latest audit (exit policy, brackets, bracket_close)
tail -n 100 logs/audit.jsonl

# Inspect TCA and ledger
tail -n 100 logs/tca.jsonl
sqlite3 logs/trade_ledger_paper.db 'SELECT * FROM orders ORDER BY ts_ms DESC LIMIT 5;'
sqlite3 logs/trade_ledger_paper.db 'SELECT * FROM fills ORDER BY ts_ms DESC LIMIT 5;'
```

```bash path=null start=null
# View TCA records
cd hq && tail -n 100 logs/tca.jsonl
```

```bash path=null start=null
# Egress allowlist
$EDITOR hq/config/egress_allowlist.txt
```

```bash path=null start=null
# Optional portfolio context used by RiskSmith
$EDITOR hq/config/portfolio.json
```

```bash path=null start=null
# Structured logs to stdout with RUST_LOG
RUST_LOG=info cargo run -p hq

# Audit trail (tamper-evident append-only file written by Ecosystem-Guardian)
# View the most recent audit events
cd hq
tail -n 100 logs/audit.jsonl
```

## Tests & CI

```bash path=null start=null
# Unit/integration tests (workspace)
cd hq && cargo test --workspace

# Lints (local)
cd hq && cargo fmt --all && cargo clippy --workspace -- -D warnings
```

```bash path=null start=null
# Run unit tests (RiskSmith, Guardian, etc.)
cd hq && cargo test
```

## Clean builds

```bash path=null start=null
cd hq
cargo clean && cargo build
```

## Sentinel/Oracle pipeline

```bash path=null start=null
# Configure egress allowlist for Sentinel collectors
$EDITOR hq/config/egress_allowlist.txt

# (Placeholder) export Oracle model
python3 hq/tools/sentiment_model_py/train_and_export.py
```

See also: hq/docs/SENTINEL_ORACLE.md

## Regime & Sentiment models (optional ONNX)

```bash path=null start=null
# Train/export regime model (placeholder)
python3 hq/tools/regime_model_py/train_and_export.py
# Train/export sentiment model (placeholder)
python3 hq/tools/sentiment_model_py/train_and_export.py

# Enable ONNX in HQ
cd hq && RUST_LOG=info cargo run -p hq --features regime-onnx --features sentiment-onnx
```

```bash path=null start=null
# Train/export regime model (placeholder)
python3 hq/tools/regime_model_py/train_and_export.py

# Enable regime ONNX in HQ
cd hq && RUST_LOG=info cargo run -p hq --features regime-onnx
```

## Live trading (BrokerGate → Oanda)

```bash path=null start=null
# Optional: pin Oanda server certificate (PEM)
export OANDA_CERT_PATH=hq/config/certs/oanda.pem

# Start HQ with BrokerGate (auto-starts Oanda transactions stream listener)
cd hq && HQ_MODE=live_oanda RUST_LOG=info cargo run -p hq
# Fills stream is appended to logs/broker_fills.jsonl
```

```bash path=null start=null
# Configure egress allowlist for Oanda
$EDITOR hq/config/egress_allowlist.txt

# Env vars (do not commit secrets)
export OANDA_HOST=api-fxpractice.oanda.com
export OANDA_ACCOUNT_ID={{OANDA_ACCOUNT_ID}}
export OANDA_API_KEY={{OANDA_API_KEY}}

# Run live via BrokerGate (Oanda)
cd hq && HQ_MODE=live_oanda RUST_LOG=info cargo run -p hq
```

### Execution gating and server-side brackets (Oanda)

- HQ fetches live bid/ask and aborts submits when either condition is true:
  - spread_bps > `exec.max_spread_bps` (default 20 bps)
  - price deviation vs reference > `exec.max_dev_bps` (default 25 bps)
- Market orders include server-side SL/TP (`takeProfitOnFill`/`stopLossOnFill`) when available.
- Before submit, HQ clamps quantity using broker open positions (mark-to-market) to respect symbol/portfolio caps.
- KPIs are written to audit as `exec_kpi` (mid, spread_bps, dev_bps) and gates as `exec_gate`.

Add to `hq/config/policy.toml` to tune thresholds:

```toml path=null start=null
[exec]
# Abort orders if spread exceeds 15 bps or if live mid deviates >20 bps from the provided price
max_spread_bps = 15.0
max_dev_bps    = 20.0
```

## Live trading (BrokerGate → Coinexx)

```bash path=null start=null
# Env vars (do not commit secrets)
export COINEXX_HOST=api.coinexx.com
export COINEXX_ACCOUNT_ID={{COINEXX_ACCOUNT_ID}}
export COINEXX_API_KEY={{COINEXX_API_KEY}}

# Run live via BrokerGate (Coinexx)
cd hq && HQ_MODE=live_coinexx RUST_LOG=info cargo run -p hq --no-default-features --features brokergate
```

## Backtest mode

```bash path=null start=null
# Enforce brackets on CSV/synthetic backtest and write realized TCA
HQ_MODE=backtest RUST_LOG=info cargo run -p hq --no-default-features
```

```bash path=null start=null
# Uses synthetic or data/sample_prices.csv; writes TCA and logs
cd hq && HQ_MODE=backtest RUST_LOG=info cargo run -p hq
```

## Run with meta-learner, RiskSmith, and Paper Execution (Pathfinder)

```bash path=null start=null
# Paper quick demo with regime+pathfinder and env overrides
HQ_SYMBOL=EUR_USD HQ_PRICE=1.1000 HQ_FAST=1.1005 HQ_SLOW=1.0990 HQ_VOL=0.0020 \
HQ_MODE=paper RUST_LOG=info cargo run -p hq --no-default-features --features regime,pathfinder
```

```bash path=null start=null
# Reset paper ledger (clear exposures) if clamped to 0
rm -f logs/trade_ledger_paper.db
```

```bash path=null start=null
# Optional: train/export execution impact model (placeholder)
python3 hq/tools/exec_model_py/train_and_export.py
```

```bash path=null start=null
# Enable ONNX-backed impact model in Pathfinder (optional)
cd hq && RUST_LOG=info cargo run -p hq --features pathfinder-onnx
```

```bash path=null start=null
# Paper mode simulates order fills via Pathfinder (OMS) and writes TCA to logs/tca.jsonl
cd hq && HQ_MODE=paper RUST_LOG=info cargo run -p hq
```

## Run with meta-learner and RiskSmith

```bash path=null start=null
# Meta-learner and RiskSmith are enabled by default
cd hq && RUST_LOG=info cargo run -p hq
```

## Alerts (macOS notifications)

```bash path=null start=null
# Enable local notifications for high-severity events
export ALERTS=1
```

## Health endpoints and metrics

```bash path=null start=null
# Health and readiness (default port 8088)
curl -s http://127.0.0.1:8088/healthz
curl -s http://127.0.0.1:8088/ready

# Prometheus metrics
curl -s http://127.0.0.1:8088/metrics
```

```bash path=null start=null
# Custom port example
HQ_HEALTH_PORT=8089 RUST_LOG=info cargo run -p hq
```


## Developer tips (local only)

```bash path=null start=null
# Faster rebuilds during iteration
cd hq
RUST_LOG=info cargo run -p hq

# Show backtraces on errors (if any)
RUST_BACKTRACE=1 cargo run -p hq
```

## Signals-only mode (emit signals, no live submits)

```bash path=null start=null
# Use SIGNALS_ONLY=1 to emit OS notifications and audit signals without placing live orders
export ALERTS=1 SIGNALS_ONLY=1
# Example: use live_oanda pipeline but skip submits; mirror signals on MT4 manually
HQ_SYMBOL=EUR_USD HQ_MODE=live_oanda RUST_LOG=info cargo run -p hq --no-default-features --features brokergate
```

## MT4 mapping (manual execution)

- Mapping file: `config/symbol_map.toml` (TradeEco → MT4 symbols).
- Signals appear in `logs/audit.jsonl` as `signal_mt4` with symbol/side/lots/SL/TP.

## Continuous demo signals (weekday rotation)

```bash path=null start=null
export ALERTS=1
while :; do D=$(date -u +%u); [ "$D" -ge 6 ] && { sleep 900; continue; };
  for S in EUR_USD USD_JPY EUR_JPY EUR_GBP USD_MXN XAU_USD US100; do
    HQ_SYMBOL=$S HQ_MODE=paper RUST_LOG=info cargo run -p hq --no-default-features --features regime,pathfinder || sleep 5;
  done; sleep 300;
done
```

## Troubleshooting & reset

```bash path=null start=null
# Reset paper ledger (clear exposures)
rm -f logs/trade_ledger_paper.db

# Ensure per-trade risk is set (adds 1% if missing)
grep -n 'per_trade_risk_pct' config/policy.toml || echo 'per_trade_risk_pct = 0.01' >> config/policy.toml

# View recent signals/activity
tail -n 100 logs/audit.jsonl | grep -E 'signal_mt4|bracket|exit_policy'
```

## Notes
- Headquarters orchestrates agents in-process; no extra commands are required to start agents today.
- Enable partial take-profit by setting `fta_partial_pct` under `[exit.structure]` in `config/exit_policy.toml`.
- Paper mode simulates bracket resolution over a short synthetic path (respects TTL and optional partial TP); backtest mode enforces exits over a price path.
- Use `--features` to enable optional agents (e.g., `regime,pathfinder`).
