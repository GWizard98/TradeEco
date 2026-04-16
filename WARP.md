# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Quick start (Headquarters ecosystem, Rust)

```bash path=null start=null
# Edit policy thresholds and caps (optional)
$EDITOR hq/config/policy.toml

# Run Headquarters
cd hq && RUST_LOG=info cargo run -p hq

# Kill switch (abort decisions)
cd hq && HQ_KILL=1 cargo run -p hq
```

See full command reference: `hq/docs/CMD_MANUAL.md`.

## Repository status
- Language/runtime: Python (3.10+ recommended)
- Layout: src-based package under `src/auto_trader`
- Key dependencies (requirements.txt): httpx, pydantic, pandas, numpy, python-dotenv
- Tests: none present; no test runner configured
- Lint/format: no configured tools found

## Commands

Rust HQ

```bash path=null start=null
# Minimal build/runtime (core agents: AlphaScout, RiskSmith, Guardian, Ledger, Portfolio, Meta)
cd hq && RUST_LOG=info cargo run -p hq --no-default-features

# Full ecosystem (all agents/services: regime, sentiment/oracle+sentinel, pathfinder paper, brokergate live, alerts, metrics)
cd hq && RUST_LOG=info cargo run -p hq --no-default-features --features full

# Paper mode (requires pathfinder feature)
cd hq && HQ_MODE=paper RUST_LOG=info cargo run -p hq --no-default-features --features pathfinder

# Live Oanda (requires brokergate feature)
cd hq && HQ_MODE=live_oanda RUST_LOG=info cargo run -p hq --no-default-features --features brokergate
```

## High-level architecture
- CLI entrypoint: `src/auto_trader/main.py`
  - Modes: `backtest` and `live`
  - Both construct a `CGModelAdapter` and request a prediction to decide an action (BUY/SELL/HOLD)
- Model adapter: `src/auto_trader/cg_model_adapter.py`
  - Reads `ML_SERVICE_URL`; if set, issues `POST {ML_SERVICE_URL}/predict` with feature payload
  - Expects a JSON response like `{ "score": float }`; on errors or if unset, returns `None`

Data flow

```text path=null start=null
main.py (backtest|live)
  -> build simple feature dict (price, volume, etc.)
  -> CGModelAdapter.predict(features)
      -> if ML_SERVICE_URL is set: HTTP POST /predict -> {score}
      -> else/any error: None
  -> choose action based on score threshold, print decision
```

Prediction contract (expected)

```json path=null start=null
// Request sent by adapter
{
  "price": 101.2,
  "volume": 1200
}
```

```json path=null start=null
// Response expected from service
{ "score": 0.73 }
```

## Notes
- If `ML_SERVICE_URL` is unset, the adapter returns `None` and the CLI will use fallback logic.
- README.md mirrors the basic setup and run commands; prefer the commands above for consistency here.
- Exit strategy reference lives in `hq/docs/EXIT_STRATEGIES.md`; configuration template at `hq/config/exit_policy.toml` (may require code wiring to take effect).
