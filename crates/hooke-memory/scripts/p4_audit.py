"""P4-A: powered empirical audit of the input-level Gaussian mechanism (PREREGISTRATION-P4.md).

Usage: p4_audit.py <validation embedding dir> <out.json>

Same estimator as p2_audit.py (optimal projection test on neighbouring collections, Clopper-Pearson
95% bounds), with two changes that P2's T2 lacked:
  * the test statistic is computed in projected space. The attacker's optimal test projects the
    observation onto u = (d - d') / |d - d'|; the projection of isotropic N(0, sigma^2 I) noise onto
    a unit vector is N(0, sigma^2), so s = (x - mid).u + sigma z with scalar z is the same statistic
    without materialising a trials x 2560 matrix. That allows TRIALS = 2,000,000 per estimate.
  * R independent repetitions per (sigma, design, k) measure the estimator's own spread, and the
    stability criteria are statistical tests over those repetitions, fixed here before running.
Criteria (frozen):
  bound_holds   every stored-once estimate (all reps, k, sigma) <= accountant epsilon
  flat_in_k     stored-once: one-sided Spearman(k, eps) over the R x |K| estimates per sigma has
                p >= 0.05, and |mean(k=64) - mean(k=1)| <= 2 x pooled SD across reps
  control_grows per-query: one-sided Spearman(k, eps) has p < 0.05 at both sigma
"""
import json, math, os, sys
import numpy as np
from scipy.stats import beta, spearmanr
from safetensors.numpy import load_file
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from p2_audit import accountant_eps, clip, CLIP, DELTA, SIGMAS, KS  # noqa: E402

TRIALS, REPS, PAIRS, SEED = 2_000_000, 20, 50, 2026


def eps_lower(d, dp, sigma, k, design, rng):
    u = (d - dp) / max(np.linalg.norm(d - dp), 1e-12)
    mid = (d + dp) / 2
    scale = sigma if design == "once" else sigma / math.sqrt(k)
    s1 = float((d - mid) @ u) + scale * rng.standard_normal(TRIALS)
    s0 = float((dp - mid) @ u) + scale * rng.standard_normal(TRIALS)
    h = TRIALS // 2
    cand = np.quantile(np.concatenate([s0[:h], s1[:h]]), np.linspace(0.5, 0.9999, 200))
    best = max(cand, key=lambda t: np.log(max((s1[:h] > t).mean(), 1e-9)) - np.log(max((s0[:h] > t).mean(), 1e-9)))
    tp, fp, m = int((s1[h:] > best).sum()), int((s0[h:] > best).sum()), TRIALS - h
    tpr_lo = beta.ppf(0.025, tp, m - tp + 1) if tp > 0 else 0.0
    fpr_hi = beta.ppf(0.975, fp + 1, m - fp) if fp < m else 1.0
    return max(0.0, math.log(max(tpr_lo - DELTA, 1e-12) / fpr_hi))


def one_sided_spearman(ks, eps):
    rho, p_two = spearmanr(ks, eps)
    if np.isnan(rho):
        return 0.0, 1.0
    p_one = p_two / 2 if rho > 0 else 1 - p_two / 2
    return float(rho), float(p_one)


def main():
    emb = load_file(f"{sys.argv[1]}/documents-00000.safetensors")["embeddings"].astype(np.float64)
    rng = np.random.default_rng(SEED)
    idx = rng.choice(len(emb), size=(PAIRS, 2), replace=False)
    canary = clip(emb[idx[0, 0]])
    out = {"clip": CLIP, "delta": DELTA, "trials": TRIALS, "reps": REPS, "pairs": PAIRS, "results": [], "tests": []}
    for sigma in SIGMAS:
        ub = accountant_eps(sigma)
        for design in ["once", "per_query"]:
            for k in KS:
                worst = [eps_lower(canary, -canary, sigma, k, design, rng) for _ in range(REPS)]
                real = [eps_lower(clip(emb[a]), clip(emb[b]), sigma, k, design, rng) for a, b in idx]
                row = {"sigma": sigma, "eps_upper": round(ub, 3), "design": design, "k": k,
                       "worst_case_reps": [round(x, 3) for x in worst],
                       "worst_case_mean": round(float(np.mean(worst)), 3), "worst_case_sd": round(float(np.std(worst, ddof=1)), 3),
                       "real_median": round(float(np.median(real)), 3), "real_max": round(float(np.max(real)), 3)}
                out["results"].append(row)
                print(json.dumps({k_: v for k_, v in row.items() if k_ != "worst_case_reps"}), flush=True)
    bound = True
    flat = True
    control = True
    for sigma in SIGMAS:
        for design in ["once", "per_query"]:
            rows = [r for r in out["results"] if r["sigma"] == sigma and r["design"] == design]
            ks = [r["k"] for r in rows for _ in r["worst_case_reps"]]
            eps = [x for r in rows for x in r["worst_case_reps"]]
            rho, p = one_sided_spearman(ks, eps)
            pooled_sd = float(np.sqrt(np.mean([r["worst_case_sd"] ** 2 for r in rows])))
            gap = rows[-1]["worst_case_mean"] - rows[0]["worst_case_mean"]
            t = {"sigma": sigma, "design": design, "spearman_rho": round(rho, 4), "p_one_sided": round(p, 6),
                 "gap_k64_minus_k1": round(gap, 3), "pooled_sd": round(pooled_sd, 3)}
            out["tests"].append(t)
            print(json.dumps(t), flush=True)
            if design == "once":
                bound &= all(x <= rows[0]["eps_upper"] for r in rows for x in r["worst_case_reps"])
                flat &= (p >= 0.05) and (abs(gap) <= 2 * pooled_sd)
            else:
                control &= p < 0.05
    out["pass_bound_holds"], out["pass_flat_in_k"], out["pass_control_grows"] = bound, flat, control
    print(json.dumps({"pass_bound_holds": bound, "pass_flat_in_k": flat, "pass_control_grows": control}), flush=True)
    json.dump(out, open(sys.argv[2], "w"), indent=2)


if __name__ == "__main__":
    main()
