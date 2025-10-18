# CG Automated Trader

This project adapts a copy of Cyber-Guardian's AI/ML algorithm to drive an automated trading system. The ML model is integrated via an adapter so it can be swapped between a local copy or a remote service.

## Getting Started
- Python >= 3.10 recommended
- Create and activate a virtual environment
- Install dependencies: `pip install -r requirements.txt`
- Run CLI: `python -m src.auto_trader.main --help`

## Structure
- `src/auto_trader/main.py` – CLI entry for backtesting/live trading
- `src/auto_trader/cg_model_adapter.py` – Adapter to the Cyber-Guardian model/service

## Notes
- Place your CG model artifacts or configure `ML_SERVICE_URL` to point to a running service.
