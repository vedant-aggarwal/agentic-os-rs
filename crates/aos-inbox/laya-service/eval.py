"""Measure latency and accuracy of the triage service on the made-up samples.

    python eval.py [--port 8799] [--threshold 0.35]

Expects the service to be running. Prints one line per sample plus a summary.
"""
import argparse
import json
import statistics
import time
import urllib.request
from pathlib import Path

ap = argparse.ArgumentParser()
ap.add_argument("--port", type=int, default=8799)
ap.add_argument("--threshold", type=float, default=0.35)
a = ap.parse_args()

samples = json.loads((Path(__file__).parent / "eval_samples.json").read_text(encoding="utf-8"))
lat, right = [], 0
for s in samples:
    body = json.dumps({"message": s["message"], "sender": s["sender"], "recent_chat": []}).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{a.port}/triage", body, {"Content-Type": "application/json"})
    t = time.perf_counter()
    out = json.loads(urllib.request.urlopen(req, timeout=60).read())
    ms = (time.perf_counter() - t) * 1000
    lat.append(ms)
    ok = (out["score"] >= a.threshold) == s["read"]
    right += ok
    print(f"{'ok ' if ok else 'BAD'} score={out['score']:.3f} want_read={s['read']!s:5} {ms:6.0f} ms  {s['message'][:48]}")
lat.sort()
print(f"accuracy {right}/{len(samples)}  latency median {statistics.median(lat):.0f} ms  max {lat[-1]:.0f} ms")
