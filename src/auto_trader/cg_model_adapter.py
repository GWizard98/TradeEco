import os
import json
from typing import Optional, Dict, Any

import httpx


class CGModelAdapter:
    """
    Adapter to interface with a copy of Cyber-Guardian's AI/ML model.
    If ML_SERVICE_URL is set, it will call the remote service's /predict endpoint.
    Otherwise, it returns a placeholder score (None) indicating no signal.
    """

    def __init__(self):
        self.service_url = os.getenv("ML_SERVICE_URL")  # e.g., http://localhost:8080

    def predict(self, features: Dict[str, Any]) -> Optional[float]:
        if not self.service_url:
            return None
        url = self.service_url.rstrip("/") + "/predict"
        try:
            resp = httpx.post(url, json=features, timeout=10)
            resp.raise_for_status()
            data = resp.json()
            # Expecting {"score": float}
            return float(data.get("score"))
        except Exception:
            return None
