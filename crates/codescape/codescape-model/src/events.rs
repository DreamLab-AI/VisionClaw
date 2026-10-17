//! JSON event shapes for the desktop `/ws/events` channel.
//!
//! Binary position frames carry geometry; everything richer (presence, captions,
//! query results, floor state) is JSON here. The TypeScript client mirrors these
//! types in `client/src/codescape/protocol.ts`.

use serde::{Deserialize, Serialize};

use crate::delta::GraphDelta;

/// What an actor did to a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceKind {
    /// Read, Grep, Glob, hover, select.
    Query,
    /// Edit of an existing file.
    Update,
    /// Write of a new file.
    Create,
    /// Deletion.
    Delete,
    /// A spoken or typed utterance anchored at the actor's focus.
    Say,
    /// Focus moved (camera or selection).
    Focus,
}

/// One actor action resolved onto graph nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceEvent {
    /// Interned actor id (shares the node id space, flagged as agent on the binary wire).
    pub actor: u32,
    /// Display name (`claude:tab0`, `john`…).
    pub actor_label: String,
    /// `agent` or `human`.
    pub actor_kind: String,
    /// What happened.
    pub kind: PresenceKind,
    /// Wire ids the action touched (files or symbols).
    pub node_ids: Vec<u32>,
    /// Repository-relative paths, when the action had them.
    pub paths: Vec<String>,
    /// Redacted text for `Say`, or the tool name otherwise.
    pub text: Option<String>,
    /// Unix milliseconds.
    pub ts_ms: u64,
}

/// Live caption from the voice pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionEvent {
    /// Speaker actor id.
    pub actor: u32,
    /// Transcript so far.
    pub text: String,
    /// `true` while words are still arriving.
    pub partial: bool,
    /// Unix milliseconds.
    pub ts_ms: u64,
}

/// Who holds the single ASR floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum FloorState {
    /// Nobody is speaking.
    Free,
    /// `holder` has the floor; `queue` waits behind.
    Held {
        /// Actor holding the floor.
        holder: u32,
        /// Actors waiting, in order.
        queue: Vec<u32>,
    },
    /// The shared ASR is serving another consumer (agentbox voice).
    Busy,
}

/// Result of a graph query (`focus`, `callers`, `affected`, `path`, `explain`…).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct QueryResult {
    /// The command as parsed, for the UI to echo.
    pub command: String,
    /// Nodes to highlight, most relevant first.
    pub node_ids: Vec<u32>,
    /// Edges to highlight as `(source, target)`.
    pub edge_pairs: Vec<(u32, u32)>,
    /// Ordered path when the query was a path search.
    pub path: Vec<u32>,
    /// Candidates offered for disambiguation (`label`, id).
    pub candidates: Vec<(String, u32)>,
    /// Spoken/printed answer.
    pub answer: String,
}

/// Every message the server sends on `/ws/events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ServerEvent {
    /// First message after connect.
    Hello {
        /// Repository label.
        repo: String,
        /// Interner epoch of the ids in use.
        epoch: u32,
        /// Current revision.
        revision: u64,
        /// Node and edge counts.
        nodes: usize,
        /// Edge count.
        edges: usize,
        /// `cpu` or `cuda`.
        engine: String,
        /// Server version.
        version: String,
    },
    /// Topology changed.
    Delta(GraphDelta),
    /// Layout has settled (kinetic energy below threshold).
    Settled {
        /// Revision the layout settled for.
        revision: u64,
    },
    /// Someone touched the graph.
    Presence(PresenceEvent),
    /// Live caption.
    Caption(CaptionEvent),
    /// Floor state changed.
    Floor(FloorState),
    /// Answer to a query from this client.
    Query(QueryResult),
    /// The client is behind by this many frames; it may be downgraded to keyframes.
    Lag {
        /// Frames outstanding.
        behind: u32,
    },
    /// Extraction progress during a cold build.
    Progress {
        /// Phase name.
        phase: String,
        /// Done so far.
        done: u64,
        /// Total when known.
        total: Option<u64>,
    },
    /// Something went wrong with the client's last request.
    Error {
        /// Human-readable message.
        message: String,
    },
}

/// Every message a client sends on `/ws/events`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ClientEvent {
    /// Identify (display name; `agent` or `human`).
    Hello {
        /// Display name.
        name: String,
        /// Actor kind.
        kind: String,
    },
    /// Typed command in the same grammar as voice.
    Command {
        /// The utterance.
        text: String,
    },
    /// Selection or camera focus moved.
    Focus {
        /// Focused node, 0 to clear.
        node_id: u32,
    },
    /// Request the ASR floor (push-to-talk pressed) or release it.
    Floor {
        /// `true` to request, `false` to release.
        request: bool,
    },
    /// Which LOD tier and focus files this client wants on the binary channel.
    View {
        /// `l0`, `l1` or `l2`.
        tier: String,
        /// Files whose symbols should be included.
        focus_files: Vec<u32>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_tagged_snake_case_json() {
        let e = ServerEvent::Floor(FloorState::Held {
            holder: 3,
            queue: vec![4],
        });
        let j = serde_json::to_string(&e).unwrap();
        assert_eq!(
            j,
            r#"{"type":"floor","state":"held","holder":3,"queue":[4]}"#
        );
        let c: ClientEvent =
            serde_json::from_str(r#"{"type":"command","text":"focus main"}"#).unwrap();
        assert_eq!(
            c,
            ClientEvent::Command {
                text: "focus main".into()
            }
        );
        let back: ServerEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(back, e);
    }
}
