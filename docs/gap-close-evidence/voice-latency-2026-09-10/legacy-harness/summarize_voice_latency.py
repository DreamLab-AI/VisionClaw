"""Summarize JSONL measurements; p95 is nearest-rank (max for n<=19)."""
import argparse
import json
import math
import statistics
from pathlib import Path


def summary(path):
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    groups = sorted({r.get("scenario", "interaction") for r in rows})
    output = {}
    for group in groups:
        items = [r for r in rows if r.get("scenario", "interaction") == group]
        good = [r for r in items if "error" not in r]
        metrics = {}
        for key in sorted({k for r in good for k in r if k.endswith("_ms") or k in ("audio_s", "stale_frames_after_ack")}):
            values = sorted(r[key] for r in good if isinstance(r.get(key), (int, float)))
            if values:
                metrics[key] = dict(median=round(statistics.median(values), 2), p95=round(values[math.ceil(.95*len(values))-1], 2), max=round(max(values), 2))
        output[group] = dict(n=len(items), errors=len(items)-len(good), metrics=metrics)
    return output


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("files", type=Path, nargs="+")
    args = p.parse_args()
    print(json.dumps({f.name: summary(f) for f in args.files}, indent=2))
