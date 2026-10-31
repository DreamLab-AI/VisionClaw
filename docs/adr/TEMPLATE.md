---
id: ADR-NNNN
title: Short imperative title
date: YYYY-MM-DD
decision_status: proposed        # proposed | accepted | rejected | superseded
implementation_status: none      # none | partial | complete
activation_status: inactive      # inactive | staged | live
supersedes: []                   # e.g. [ADR-0042]
superseded_by: []                # e.g. [ADR-2050]
verified_commit:                 # git sha at which implementation_status was verified (full 40-char SHA arms staleness checking)
verified_paths: []               # optional: repo-relative paths the verification inspected; CI fails if they change after verified_commit
owner:                           # accountable person/handle
review_trigger:                  # the event/date that forces a re-review of this ADR
repo: visionclaw                 # visionclaw | agentbox
evidence_class:                  # required when implementation_status becomes complete or activation_status becomes live (adr-ratchet, ADR-2007): source | test | ci | deployment | runtime (join with +)
acceptance_boundary:             # required with evidence_class: what the evidence does and does not establish
---

# ADR-NNNN — Short imperative title

## Context
<!-- Max 10 lines. The forces at play: the problem, constraints, prior state.
     State facts, not narrative. If it needs more than 10 lines, it is two ADRs. -->

## Decision
<!-- The meat. What we are doing, stated in the present tense as policy.
     Be specific enough that a reader can tell whether a given change complies. -->

## Consequences
<!-- What becomes true, easier, harder, or forbidden as a result.
     Include the costs and the follow-on work, not just the upside. -->

## Verification
<!-- How implementation_status above was established: the command, test, grep,
     or artefact inspected, and at which verified_commit. This is the evidence
     that the ADR describes reality rather than intent. -->
