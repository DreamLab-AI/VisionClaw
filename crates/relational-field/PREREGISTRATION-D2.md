# Preregistration deviation D2: H1 becomes temporal forecasting

Recorded 5 November 2026, before any experiment. The frozen H1 preregistration (`crates/relational-field/PREREGISTRATION.md` on VisionClaw `feat/relational-field`) is unchanged and kept as history.

**What changes.** H1 moves from a single static split of one graph to prospective forecasting on the versioned visionGraph corpus: train at cut dates (February, June and September 2026), score typed links present at `d8277151a` between pages that existed at the cut, with type-constrained hard negatives, five seeds.

**Why.** The round 10 rebuild puts real data at the gate. The corpus history (typed ontology pages from November 2025) gives a prospective test that a static split cannot, following Mežnar et al. (2021).

**New gates** (frozen at G1, week 2 of the project): typed forecast AUC RF − B0 ≥ 0.05; RF ≥ B2 − 0.02; RF − B3 ≥ 0.03; parameters-only simulation accuracy RF-sim − B0 ≥ 0.08; degree-preserving rewiring leaves an RF advantage ≤ 0.01. Prerequisite: extractor triple-level F1 ≥ 0.99 across the October 2026 format migration.

**Registration.** The new version is committed as a separate preregistration file on the learner branch at G1, before held-out evaluation, with this record cited. The original six gates are not reinterpreted.
