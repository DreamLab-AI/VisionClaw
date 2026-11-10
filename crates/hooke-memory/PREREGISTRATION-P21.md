# Preregistration P21: is there headroom for a learned human-review gate on agent-proposed changes?

Recorded 10 November 2026, before any human review exists (no `reviews-r1.jsonl` or `reviews-r2.jsonl`; the review
tool refuses to start until this file is recorded). P21 is a headroom measurement, not a test of a learned policy:
one sitting yields at most about 45 human verdicts, too few to train or confirm anything. It asks whether a blind
human reviewer disagrees with the dream cycle's deterministic gate often enough for a learned allocation of human
review to have anything to gain. The rule from the audit of P1-P18 applies: measure the gap before proposing to close it.

## Data
The nightly dream cycle's candidate changes, 6 October to 10 November 2026: 123 candidates whose gated patch is
joined to its ledger row by recomputed witness (gate verdict ACCEPT 83, REJECT 31, BLOCKED-ENV 7, INCONCLUSIVE 2).
Review session files digest (all files in `gate/session`, sorted, sha256 of sha256 list): `4623c844008d717e4d917f6a463479f136984eaa4ada460fcb3d475e71bc8f9d`.
The presentation order is fixed and seeded; all 40 non-ACCEPT candidates fall in positions 1-80, alternating by block
with 40 ACCEPT candidates. Analysis is confined to positions 1-80.

## Procedure
* Reviewer 1: the operator (also the project lead; stated as a limit). Command:
  `python3 review_gate.py --reviewer NAME --hide-evaluators`. Evaluator results are hidden because they determine
  the gate verdict exactly (123 of 123); gate verdict, pull request state and earlier answers are hidden by the tool.
* Items are taken in the fixed order with no skipping. Stop rule: 150 active minutes or position 80, whichever comes
  first; the pilot is valid only with at least 30 recorded verdicts.
* Reviewer 2, if available: positions 1-30 in an independent order, same flags plus `--second`.
* No model sees any diff or finding text. No model baseline is part of P21.

## Measures
Human verdict is binarised: approve against reject or needs-changes; cannot-judge is excluded and its count reported.
Gate verdict is binarised: ACCEPT against the rest. Primary: the disagreement rate between human and gate over the
reviewed items, with a Wilson 95% interval, and split into its two directions (human approves a gate non-ACCEPT;
human does not approve a gate ACCEPT). Also reported: active minutes per item (median, quartiles); confidence;
Cohen's kappa between reviewers if a second reviewer takes part.

## Criteria (fixed now)
* Headroom: disagreement is at least 20% and the lower Wilson bound is above 10%. Reported as: the deterministic gate
  and a blind human differ often enough that deciding where to spend human review is a real problem on this data.
  It is not evidence that any learned policy solves it.
* No headroom: disagreement is at most 10%. Reported as: the deterministic gate already matches the human on this
  data, a learned gate has nothing to gain here, and the human gate is not offered as a test bed.
* Otherwise: indeterminate; reported with the interval and not used as support.
* Label quality: if a second reviewer takes part and kappa is below 0.4, the human verdict is too unreliable to serve
  as the oracle and the result is reported as indeterminate whatever the disagreement rate.

## Exploratory, no pass mark, labelled as such wherever reported
Leave-one-out logistic regression predicting human non-approval from evidence available before review (diff size,
files, file types, whether tests, CI, config or docs are touched, repository), with and without the gate verdict,
against always-approve and gate-verdict-only. Agreement of the agent-decided merge outcome with the human verdict,
restricted to pull requests whose diff matches the gated patch.

## Limits
One reviewer who is the project lead; one estate; five weeks of nights; diff-only review without test results is
harder than normal review and may raise disagreement; INCONCLUSIVE candidates are nearly absent (2).

## Note added 10 November 2026, before any human review exists
The engine log shows that on 10 November two checkouts without a git identity failed their candidate commit, and a
gate ACCEPT was recorded as BLOCKED-ENV for that reason. Six of the seven BLOCKED-ENV candidates in the review set
come from those two checkouts, so their non-ACCEPT verdict may reflect an environment fault and not the gate's
judgement of the change. The primary analysis is unchanged (ACCEPT against the rest). A sensitivity analysis excluding
all BLOCKED-ENV candidates is reported beside it, and the headroom statement is made only if both agree.
