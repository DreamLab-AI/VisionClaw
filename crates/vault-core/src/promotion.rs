//! The promotion state machine (PRD §3.3).
//!
//! ```text
//! draft ──propose──► machine-confirmed ──approve(human)──► human-reviewed
//!   ▲                      │                                    │
//!   └──────expire──────────┘                                    │
//!                                                          demote(human)
//!                                                               ▼
//!                                                          deprecated
//! ```
//!
//! Two rules make this worth being a type rather than a convention:
//!
//! 1. **A blocker is not an opinion.** Whelk inconsistency, a subclass cycle, a
//!    relation contradiction or a vocabulary violation *prevents* a transition;
//!    it is never something a human can wave through (decision Q6).
//! 2. **Only a human promotes.** [`Transition::Approve`] and
//!    [`Transition::Demote`] require an [`Actor::Human`]; a process stamp can
//!    reach `machine-confirmed` and no further (decision Q7).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::okf::{Actor, OkfBlock, Stamp, Status};

/// Where a page sits in the governance loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromotionState {
    /// Authored or proposed; no attestation at all.
    Draft,
    /// Attested by a process or agent, awaiting a human.
    MachineConfirmed,
    /// Attested by a human; this is what `status: stable` means.
    HumanReviewed,
    /// Withdrawn.
    Deprecated,
}

impl PromotionState {
    /// The kebab-case name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::MachineConfirmed => "machine-confirmed",
            Self::HumanReviewed => "human-reviewed",
            Self::Deprecated => "deprecated",
        }
    }

    /// The `status` a page in this state must carry.
    #[must_use]
    pub fn status(self) -> Status {
        match self {
            Self::Draft | Self::MachineConfirmed => Status::Draft,
            Self::HumanReviewed => Status::Stable,
            Self::Deprecated => Status::Deprecated,
        }
    }

    /// Read the state off a page's OKF block.
    ///
    /// `deprecated` wins outright; otherwise a human attestation means
    /// `human-reviewed`, any other attestation means `machine-confirmed`, and
    /// nothing means `draft`. `status: stable` without any attestation is
    /// reported as `draft` — the attestation, not the label, is the evidence.
    #[must_use]
    pub fn of(okf: &OkfBlock) -> Self {
        if okf.status == Some(Status::Deprecated) {
            return Self::Deprecated;
        }
        if okf.is_human_verified() {
            Self::HumanReviewed
        } else if okf.verified.is_empty() {
            Self::Draft
        } else {
            Self::MachineConfirmed
        }
    }
}

impl fmt::Display for PromotionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A move through the state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "transition")]
pub enum Transition {
    /// `vault propose`: a process attests the page.
    Propose {
        /// The proposing process or agent.
        by: Actor,
        /// ISO-8601 instant.
        at: String,
    },
    /// A human 31403 `Promote`.
    Approve {
        /// Must be [`Actor::Human`].
        by: Actor,
        /// ISO-8601 instant.
        at: String,
    },
    /// A human 31403 `Demote`.
    Demote {
        /// Must be [`Actor::Human`].
        by: Actor,
        /// ISO-8601 instant.
        at: String,
    },
    /// The proposal's `stale_after` passed with no response.
    Expire,
}

impl Transition {
    /// The actor driving this transition, when there is one.
    #[must_use]
    pub fn actor(&self) -> Option<&Actor> {
        match self {
            Self::Propose { by, .. } | Self::Approve { by, .. } | Self::Demote { by, .. } => {
                Some(by)
            }
            Self::Expire => None,
        }
    }
}

/// A machine-checkable reason a transition cannot happen.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Blocker {
    /// A stable code, e.g. `SUBCLASS_CYCLE` or `WHELK_INCONSISTENT`.
    pub code: String,
    /// One line a human can act on.
    pub detail: String,
}

impl Blocker {
    /// Build a blocker.
    #[must_use]
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
        }
    }
}

impl fmt::Display for Blocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.detail)
    }
}

/// Why a transition was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PromotionError {
    /// The machine has no such edge from the current state.
    #[error("cannot {transition} from {from}")]
    IllegalTransition {
        /// Where the page is.
        from: PromotionState,
        /// What was attempted.
        transition: &'static str,
    },
    /// A process or agent tried to do a human's job.
    #[error("{actor} is not human; only a human can {transition}")]
    NotHuman {
        /// The offending actor.
        actor: String,
        /// What was attempted.
        transition: &'static str,
    },
    /// One or more automatic blockers are outstanding.
    #[error("blocked: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Blocked(Vec<Blocker>),
}

/// The effect of a successful transition: the new state, and the attestation
/// (if any) to append to `verified`.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The resulting state.
    pub state: PromotionState,
    /// The stamp to append to `verified`, if the transition produced one.
    pub stamp: Option<Stamp>,
    /// The `status` the page must now carry.
    pub status: Status,
}

/// Apply `transition` to `from`, refusing on an illegal edge, a non-human
/// signer, or any outstanding `blockers`.
///
/// Blockers are checked **first and unconditionally**: no transition, not even
/// an expiry, proceeds while the graph is known-broken.
///
/// # Errors
/// [`PromotionError`] describing exactly which of the three rules failed.
///
/// # Examples
///
/// ```
/// use vault_core::okf::{Actor, Status};
/// use vault_core::promotion::{apply, PromotionState, Transition};
///
/// let human = Actor::Human("npub1abc".into());
/// let out = apply(
///     PromotionState::MachineConfirmed,
///     &Transition::Approve { by: human, at: "2026-09-22T00:00:00Z".into() },
///     &[],
/// ).unwrap();
/// assert_eq!(out.state, PromotionState::HumanReviewed);
/// assert_eq!(out.status, Status::Stable);
/// assert!(out.stamp.unwrap().by.is_human());
/// ```
pub fn apply(
    from: PromotionState,
    transition: &Transition,
    blockers: &[Blocker],
) -> Result<Outcome, PromotionError> {
    if !blockers.is_empty() {
        return Err(PromotionError::Blocked(blockers.to_vec()));
    }

    let require_human = |by: &Actor, name: &'static str| -> Result<(), PromotionError> {
        if by.is_human() {
            Ok(())
        } else {
            Err(PromotionError::NotHuman {
                actor: by.as_string(),
                transition: name,
            })
        }
    };

    let state = match (from, transition) {
        (PromotionState::Draft, Transition::Propose { by, at }) => {
            if by.is_human() {
                return Err(PromotionError::IllegalTransition {
                    from,
                    transition: "propose as a human (use approve)",
                });
            }
            return Ok(Outcome {
                state: PromotionState::MachineConfirmed,
                stamp: Some(Stamp::new(by.clone(), at.clone())),
                status: Status::Draft,
            });
        }
        (
            PromotionState::MachineConfirmed | PromotionState::Deprecated,
            Transition::Approve { by, at },
        ) => {
            require_human(by, "approve")?;
            return Ok(Outcome {
                state: PromotionState::HumanReviewed,
                stamp: Some(Stamp::new(by.clone(), at.clone())),
                status: Status::Stable,
            });
        }
        (PromotionState::HumanReviewed, Transition::Demote { by, .. })
        | (PromotionState::MachineConfirmed, Transition::Demote { by, .. }) => {
            require_human(by, "demote")?;
            PromotionState::Deprecated
        }
        (PromotionState::MachineConfirmed, Transition::Expire) => PromotionState::Draft,
        (from, t) => {
            return Err(PromotionError::IllegalTransition {
                from,
                transition: match t {
                    Transition::Propose { .. } => "propose",
                    Transition::Approve { .. } => "approve",
                    Transition::Demote { .. } => "demote",
                    Transition::Expire => "expire",
                },
            })
        }
    };

    Ok(Outcome {
        state,
        stamp: None,
        status: state.status(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::Frontmatter;

    fn human() -> Actor {
        Actor::Human("npub1abc".into())
    }

    fn process() -> Actor {
        Actor::Process {
            name: "vault".into(),
            version: "1.0".into(),
        }
    }

    const NOW: &str = "2026-09-22T00:00:00Z";

    #[test]
    fn state_is_read_from_attestations_not_from_the_label() {
        let fm = Frontmatter::parse("status: stable\n").unwrap();
        assert_eq!(
            PromotionState::of(&OkfBlock::from_frontmatter(&fm)),
            PromotionState::Draft
        );
        let fm = Frontmatter::parse(
            "status: stable\nverified: [{ by: human:npub1, at: 2026-01-01T00:00:00Z }]\n",
        )
        .unwrap();
        assert_eq!(
            PromotionState::of(&OkfBlock::from_frontmatter(&fm)),
            PromotionState::HumanReviewed
        );
    }

    #[test]
    fn deprecated_wins_over_any_attestation() {
        let fm = Frontmatter::parse(
            "status: deprecated\nverified: [{ by: human:npub1, at: 2026-01-01T00:00:00Z }]\n",
        )
        .unwrap();
        assert_eq!(
            PromotionState::of(&OkfBlock::from_frontmatter(&fm)),
            PromotionState::Deprecated
        );
    }

    #[test]
    fn propose_moves_draft_to_machine_confirmed() {
        let out = apply(
            PromotionState::Draft,
            &Transition::Propose {
                by: process(),
                at: NOW.into(),
            },
            &[],
        )
        .unwrap();
        assert_eq!(out.state, PromotionState::MachineConfirmed);
        assert_eq!(out.status, Status::Draft);
        assert!(!out.stamp.unwrap().by.is_human());
    }

    #[test]
    fn a_process_cannot_approve() {
        let err = apply(
            PromotionState::MachineConfirmed,
            &Transition::Approve {
                by: process(),
                at: NOW.into(),
            },
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, PromotionError::NotHuman { .. }));
    }

    #[test]
    fn blockers_refuse_every_transition() {
        let blockers = vec![Blocker::new("SUBCLASS_CYCLE", "A -> B -> A")];
        for t in [
            Transition::Propose {
                by: process(),
                at: NOW.into(),
            },
            Transition::Approve {
                by: human(),
                at: NOW.into(),
            },
            Transition::Expire,
        ] {
            let err = apply(PromotionState::MachineConfirmed, &t, &blockers).unwrap_err();
            assert!(matches!(err, PromotionError::Blocked(_)), "{err}");
        }
    }

    #[test]
    fn expiry_returns_a_proposal_to_draft() {
        let out = apply(PromotionState::MachineConfirmed, &Transition::Expire, &[]).unwrap();
        assert_eq!(out.state, PromotionState::Draft);
        assert!(out.stamp.is_none());
    }

    #[test]
    fn demote_requires_a_human_and_reaches_deprecated() {
        let out = apply(
            PromotionState::HumanReviewed,
            &Transition::Demote {
                by: human(),
                at: NOW.into(),
            },
            &[],
        )
        .unwrap();
        assert_eq!(out.state, PromotionState::Deprecated);
        assert_eq!(out.status, Status::Deprecated);
    }

    #[test]
    fn a_draft_cannot_be_approved_directly() {
        let err = apply(
            PromotionState::Draft,
            &Transition::Approve {
                by: human(),
                at: NOW.into(),
            },
            &[],
        )
        .unwrap_err();
        assert!(matches!(err, PromotionError::IllegalTransition { .. }));
    }

    #[test]
    fn a_deprecated_page_can_be_reinstated_by_a_human() {
        let out = apply(
            PromotionState::Deprecated,
            &Transition::Approve {
                by: human(),
                at: NOW.into(),
            },
            &[],
        )
        .unwrap();
        assert_eq!(out.state, PromotionState::HumanReviewed);
    }
}
