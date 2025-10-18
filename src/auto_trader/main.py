import argparse
import os
from .cg_model_adapter import CGModelAdapter


def run_backtest():
    adapter = CGModelAdapter()
    # Example: fetch historical features and get predictions (placeholder)
    features = {"price": 100.0, "volume": 1000}
    score = adapter.predict(features)
    action = "BUY" if (score or 0) > 0.5 else "SELL"
    print(f"Backtest decision: {action} (score={score})")


def run_live():
    adapter = CGModelAdapter()
    # Example: realtime tick features (placeholder)
    features = {"price": 101.2, "volume": 1200}
    score = adapter.predict(features)
    action = "HOLD" if score is None else ("BUY" if score > 0.6 else "SELL")
    print(f"Live decision: {action} (score={score})")


def main():
    parser = argparse.ArgumentParser(description="CG-based Automated Trader")
    parser.add_argument("mode", choices=["backtest", "live"], help="Run mode")
    args = parser.parse_args()

    if args.mode == "backtest":
        run_backtest()
    else:
        run_live()


if __name__ == "__main__":
    main()
