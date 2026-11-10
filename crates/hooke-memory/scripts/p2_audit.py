"""P2 T2: empirical privacy audit of the input-level Gaussian mechanism (PREREGISTRATION-P2.md).

Usage: p2_audit.py <validation embedding dir> <out.json>

Neighbouring collections differ in one document: d in one world, d' in the other. The attacker
knows both candidates and sees the released (privatised) embedding k times. Two release designs:

  once      noise drawn once per document at ingestion and stored; every query sees the same copy
  per_query fresh noise for every query (the counterfactual this design avoids)

The optimal test projects the observation (averaged over the k views) onto d - d'. Half the trials
choose the threshold, the other half measure TPR and FPR; Clopper-Pearson 95% bounds give an
empirical lower bound eps_lb = ln((TPR_lo - delta) / FPR_hi), compared with the accountant's
upper bound. The audit runs at the embedding level; anything released downstream (memory tokens,
answers, receipts) is post-processing of this vector and so cannot leak more.
"""
import json, math, sys
import numpy as np
from scipy.stats import beta
from safetensors.numpy import load_file
import dp_accounting as dpa
from dp_accounting.pld import pld_privacy_accountant as pld

SIGMAS, CLIP, DELTA, KS = [1.0, 1.5], 1.0, 1e-5, [1, 4, 16, 64]
TRIALS, PAIRS, SEED = 20000, 50, 2026


def accountant_eps(sigma):
    a = pld.PLDAccountant()
    a.compose(dpa.GaussianDpEvent(sigma / (2 * CLIP)))
    return a.get_epsilon(DELTA)


def clip(x):
    n = np.linalg.norm(x, axis=-1, keepdims=True)
    return x * np.minimum(1.0, CLIP / np.maximum(n, 1e-12))


def eps_lower(d, dp, sigma, k, design, rng):
    u = (d - dp) / max(np.linalg.norm(d - dp), 1e-12)
    mid = (d + dp) / 2
    w = d.shape[0]

    def views(x):
        if design == "once":
            # One stored privatised copy; the k views are identical, so their mean is that copy.
            return x + sigma * rng.standard_normal((TRIALS, w))
        return x + sigma * rng.standard_normal((TRIALS, w)) / math.sqrt(k)

    s1 = (views(d) - mid) @ u
    s0 = (views(dp) - mid) @ u
    h = TRIALS // 2
    cand = np.quantile(np.concatenate([s0[:h], s1[:h]]), np.linspace(0.5, 0.9999, 200))
    best = max(cand, key=lambda t: np.log(max((s1[:h] > t).mean(), 1e-9)) - np.log(max((s0[:h] > t).mean(), 1e-9)))
    tp, fp, m = int((s1[h:] > best).sum()), int((s0[h:] > best).sum()), TRIALS - h
    tpr_lo = beta.ppf(0.025, tp, m - tp + 1) if tp > 0 else 0.0
    fpr_hi = beta.ppf(0.975, fp + 1, m - fp) if fp < m else 1.0
    return max(0.0, math.log(max(tpr_lo - DELTA, 1e-12) / fpr_hi))


def main():
    emb = load_file(f"{sys.argv[1]}/documents-00000.safetensors")["embeddings"].astype(np.float64)
    rng = np.random.default_rng(SEED)
    idx = rng.choice(len(emb), size=(PAIRS, 2), replace=False)
    canary = clip(emb[idx[0, 0]])
    out = {"clip": CLIP, "delta": DELTA, "trials": TRIALS, "pairs": PAIRS, "results": []}
    for sigma in SIGMAS:
        ub = accountant_eps(sigma)
        for design in ["once", "per_query"]:
            for k in KS:
                worst = eps_lower(canary, -canary, sigma, k, design, rng)
                real = [eps_lower(clip(emb[a]), clip(emb[b]), sigma, k, design, rng) for a, b in idx]
                row = {"sigma": sigma, "eps_upper": round(ub, 3), "design": design, "k": k,
                       "eps_lower_worst_case": round(worst, 3), "eps_lower_real_median": round(float(np.median(real)), 3),
                       "eps_lower_real_max": round(float(np.max(real)), 3)}
                out["results"].append(row)
                print(json.dumps(row), flush=True)
    once = [r for r in out["results"] if r["design"] == "once"]
    out["pass_bound_holds"] = all(r["eps_lower_worst_case"] <= r["eps_upper"] for r in once)
    out["pass_flat_in_k"] = all(
        abs(r["eps_lower_worst_case"] - next(x for x in once if x["sigma"] == r["sigma"] and x["k"] == 1)["eps_lower_worst_case"]) <= 0.5
        for r in once)
    json.dump(out, open(sys.argv[2], "w"), indent=2)
    print("pass_bound_holds", out["pass_bound_holds"], "pass_flat_in_k", out["pass_flat_in_k"])


if __name__ == "__main__":
    main()
