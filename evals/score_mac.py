#!/usr/bin/env python3
"""Build the macOS model scoreboard from bake-off report.json cells.

Reads per-engine, per-set reports. For the 3 freshly measured engines it reads
evals/runs/bakeoff-2026-10-23-mac/<engine>/<set>-off/report.json; for the
already-measured engines it falls back to evals/runs/bakeoff/<engine>/... .
Applies one rating rubric to every engine so scores are comparable.
"""
import json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NEW = os.path.join(ROOT, "evals/runs/bakeoff-2026-10-23-mac")
OLD = os.path.join(ROOT, "evals/runs/bakeoff")

ENGLISH = ["librispeech-clean", "librispeech-other", "cv", "fleurs-en", "ami"]
NOISE = ["noise-10db", "noise-5db", "noise-0db"]
ALL_SETS = ENGLISH + NOISE

# (engine id, size MB, languages, hidden_on_mac)
ENGINES = [
    ("parakeet-ultra-coreml", 600, "25 European", False),
    ("parakeet-v3-coreml", 600, "25 European", False),
    ("parakeet-tdt-v3", 640, "25 European", False),
    ("parakeet-tdt-v3-q4", 356, "25 European", False),
    ("fluid-speech-mini", 347, "English", False),
    ("large-v3-turbo-q5", 547, "99", False),
    ("sherpa-parakeet-v3-int8", 465, "25 European", True),
    ("sherpa-parakeet-v2-int8", 461, "English", True),
]

def load(engine, s):
    for base in (os.path.join(NEW, engine), os.path.join(OLD, engine)):
        p = os.path.join(base, f"{s}-off", "report.json")
        if os.path.isfile(p):
            try:
                return json.load(open(p))
            except Exception:
                return None
    return None

def wer_pct(d):
    v = d.get("wer_verbatim_micro")
    return v * 100.0 if isinstance(v, (int, float)) else None

def acc_rating(wer_e):
    if wer_e is None: return None
    for lim, s in [(1.7,10),(2.0,9),(3.0,8),(5.0,7),(8.0,6),(12.0,5),(18.0,4)]:
        if wer_e <= lim: return s
    return max(1, round(4 - (wer_e-18.0)/4.0))

def lat_rating(p50):
    if p50 is None: return None
    for lim, s in [(60,10),(120,9),(200,8),(350,7),(600,6),(900,5),(1500,4)]:
        if p50 <= lim: return s
    return max(1, round(4 - (p50-1500)/1000.0))

rows = []
for engine, size, langs, hidden in ENGINES:
    ws, lat = [], {}
    cells_run = 0
    for s in ALL_SETS:
        d = load(engine, s)
        if d is None: continue
        cells_run += 1
        w = wer_pct(d)
        l = d.get("latency_asr") or {}
        lat[s] = (l.get("p50"), l.get("p95"))
        if s in ENGLISH and w is not None: ws.append(w)
    wer_e = sum(ws)/len(ws) if ws else None
    clean = lat.get("librispeech-clean", (None,None))
    n0 = lat.get("noise-0db", (None,None))
    n0w = wer_pct(load(engine,"noise-0db")) if load(engine,"noise-0db") else None
    rows.append(dict(engine=engine, size=size, langs=langs, hidden=hidden,
        cells=cells_run, wer_e=wer_e, acc=acc_rating(wer_e),
        p50=clean[0], p95=clean[1], lat=lat_rating(clean[0]),
        n0w=n0w, load=(load(engine,"librispeech-clean") or {}).get("model_load_ms")))

print("| model | English WER% (mean) | noise-0db WER% | p50/p95 ms | acc/10 | lat/10 | size MB | langs |")
print("|---|---|---|---|---|---|---|---|")
for r in sorted(rows, key=lambda x:(x['acc'] or 0)+(x['lat'] or 0), reverse=True):
    def f(v,n=2): return "-" if v is None else (f"{v:.{n}f}" if isinstance(v,float) else str(v))
    tag = " *(hidden on Mac)*" if r['hidden'] else ""
    print(f"| `{r['engine']}`{tag} | {f(r['wer_e'])} | {f(r['n0w'])} | {f(r['p50'],0)}/{f(r['p95'],0)} | {r['acc'] if r['acc'] is not None else '-'} | {r['lat'] if r['lat'] is not None else '-'} | {r['size']} | {r['langs']} |")

print("\ncells_run per engine:", {r['engine']:r['cells'] for r in rows})
