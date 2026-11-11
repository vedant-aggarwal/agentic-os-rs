"""Local triage service for the Agentic OS inbox.

POST /triage  {"message": str, "sender": str, "recent_chat": [str]}
           -> {"score": float, "read": bool, "ms": int}
GET  /health  -> {"ok": true, "model": "..."}

`score` is the probability that Nora should read the message. It comes from Laya (ConvAI
Innovations, ~421M parameters, Apache 2.0), which answers typed yes/no questions with calibrated
probabilities. Binds to 127.0.0.1 only. Message text is passed to the model as data inside the
state, never as instructions; the output is a probability, so the worst case is a wrong score.

Run:  python laya_service.py --port 8799
Env:  HF_HOME (model cache), AOS_LAYA_THRESHOLD (default 0.35, only used for the `read` flag)
"""
import argparse
import os
import time

import uvicorn
from fastapi import FastAPI
from pydantic import BaseModel, Field

MODEL_ID = os.environ.get("AOS_LAYA_MODEL", "convaiinnovations/laya")
THRESHOLD = float(os.environ.get("AOS_LAYA_THRESHOLD", "0.35"))

# The question is the whole "prompt". It asks for the *unwanted* class (automated, bulk, promo)
# and the score is inverted, because Laya answers narrow, concrete questions better than broad
# ones like "is this important".
QUESTIONS = {
    "skip": {
        "type": "noul",
        "instructions": (
            "Is `message` an automated notice, one-time password, promotion, advertisement, "
            "newsletter or bulk broadcast, rather than a personal message written by a person "
            "to the reader?"
        ),
    }
}

app = FastAPI(title="aos-laya-triage")
_agent = None


class Req(BaseModel):
    message: str = ""
    sender: str = ""
    recent_chat: list[str] = Field(default_factory=list)


def agent():
    global _agent
    if _agent is None:
        import laya

        _agent = laya.load(MODEL_ID)
    return _agent


def state_for(r: Req) -> str:
    # Short context: the last few lines, then the message. Kept small because latency grows with
    # tokens on CPU.
    ctx = " | ".join(t[:120] for t in r.recent_chat[-3:])
    head = f"sender: {r.sender[:60]}\n" if r.sender else ""
    body = r.message[:600]
    return f"{head}{('earlier: ' + ctx + chr(10)) if ctx else ''}{body}"


def score_one(r: Req) -> float:
    res = agent().decide(state_for(r), questions=QUESTIONS, return_details=True)
    p_skip = res.probabilities["skip"]["true"]
    return round(1.0 - float(p_skip), 4)


@app.get("/health")
def health():
    return {"ok": True, "model": MODEL_ID, "loaded": _agent is not None}


@app.post("/triage")
def triage(r: Req):
    t = time.perf_counter()
    score = score_one(r)
    return {"score": score, "read": score >= THRESHOLD, "ms": int((time.perf_counter() - t) * 1000)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8799)
    ap.add_argument("--no-warmup", action="store_true")
    a = ap.parse_args()
    if not a.no_warmup:
        # Load the weights and run one prediction before serving, so the first real call is fast.
        score_one(Req(message="warm up"))
    uvicorn.run(app, host="127.0.0.1", port=a.port, log_level="warning")


if __name__ == "__main__":
    main()
