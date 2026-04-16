# Sentinel (collector) and Oracle (NLP) pipeline

- Sentinel: collects news (YouTube transcripts, Bloomberg/FXStreet/Yahoo Finance/ForexNews/TradingView RSS/APIs), dedupes, scores source_quality/severity, and normalizes to NewsEvent.
- Oracle: scores sentiment with ML (ONNX optional) and produces score/uncertainty/regime_fit for HQ.

Commands

```bash
# Edit egress allowlist for collector domains
$EDITOR hq/config/egress_allowlist.txt

# (Placeholder) Collector scripts
python3 hq/tools/sentiment_model_py/train_and_export.py  # Oracle model export
# Add your own collector to write NewsEvent JSONL, then transform via Sentinel
```
