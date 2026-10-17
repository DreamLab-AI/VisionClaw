//! Per-client position streams: projection, SSSP overlay, frame selection and
//! ack pacing, producing V5 frames. Shared by `/ws/positions` and `/wss`.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use codescape_model::wire::{decode_ack, v5_frame};
use codescape_model::NodeRecord;
use codescape_server::wire::{
    AckWindow, Admission, FrameSelector, SeqCounter, MAX_RECORDS_PER_FRAME,
};

use crate::engine::RepoEngine;
use crate::state::SharedView;

/// One client's binary stream.
pub struct PositionStream {
    repo: Arc<RepoEngine>,
    view: Arc<SharedView>,
    selector: FrameSelector,
    window: AckWindow,
    seq: SeqCounter,
    projection: Arc<Vec<u32>>,
    projection_key: (u64, u64),
    sssp: crate::engine::SsspOverlay,
    sssp_key: Option<(u32, u64)>,
    /// Frames sent so far.
    pub frames_sent: u64,
}

impl PositionStream {
    /// A stream that starts with a keyframe.
    pub fn new(repo: Arc<RepoEngine>, view: Arc<SharedView>) -> Self {
        Self {
            repo,
            view,
            selector: FrameSelector::default(),
            window: AckWindow::default(),
            seq: SeqCounter::default(),
            projection: Arc::new(Vec::new()),
            projection_key: (u64::MAX, 0),
            sssp: Arc::new(HashMap::new()),
            sssp_key: None,
            frames_sent: 0,
        }
    }

    /// The ids currently projected for this client (for `initialGraphLoad`).
    pub fn projection_ids(&mut self) -> Arc<Vec<u32>> {
        self.refresh_projection();
        self.projection.clone()
    }

    fn refresh_projection(&mut self) {
        let published = self.repo.current();
        let (v, version) = self.view.get();
        let key = (published.snapshot.revision, version);
        if key != self.projection_key {
            // Shared: every client with the same tier and focus set gets the same Arc.
            self.projection = self.repo.projection_for(v.tier, &v.focus_files);
            self.projection_key = key;
        }
    }

    fn refresh_sssp(&mut self, revision: u64) {
        let (v, _) = self.view.get();
        let want = v.focus.map(|f| (f, revision));
        if want == self.sssp_key {
            return;
        }
        match want {
            None => {
                self.sssp = Arc::new(HashMap::new());
                self.sssp_key = None;
            }
            Some((f, rev)) => {
                // Memoised per focus across clients; `None` during a re-extract: retry next tick.
                if let Some(v) = self.repo.sssp_for(f, rev) {
                    self.sssp = v;
                    self.sssp_key = want;
                }
            }
        }
    }

    /// Feed a binary message from the client. Returns `true` when it was an ack.
    pub fn on_binary(&mut self, bytes: &[u8]) -> bool {
        match decode_ack(bytes) {
            Some((seq, _, _)) => {
                if !self.window.on_ack(seq, self.seq.last()) {
                    tracing::debug!(seq, "ack for unknown sequence ignored");
                }
                true
            }
            None => false,
        }
    }

    /// Make the next frame a keyframe.
    pub fn force_keyframe(&mut self) {
        self.selector.force_keyframe();
    }

    /// One tick: the V5 frame to send, if any.
    pub fn tick(&mut self, now_ms: u64) -> Option<Vec<u8>> {
        match self.window.admit(now_ms) {
            Admission::Skip { behind, skipped } => {
                self.view.lag.store(behind, Ordering::Relaxed);
                if skipped % 50 == 1 {
                    tracing::debug!(behind, skipped, "client behind; skipping frames");
                }
                return None;
            }
            Admission::Recover => {
                tracing::debug!("ack timeout; recovering with a keyframe");
                self.selector.force_keyframe();
            }
            Admission::Send => {}
        }
        self.view.lag.store(0, Ordering::Relaxed);
        self.refresh_projection();
        let base = self.repo.base_records(now_ms);
        self.refresh_sssp(base.published.snapshot.revision);
        let mut current: Vec<NodeRecord> =
            Vec::with_capacity(self.projection.len() + base.actors.len());
        for id in self.projection.iter() {
            let Some(&i) = base.published.index.get(id) else {
                continue;
            };
            let Some(mut r) = base.records.get(i).copied().flatten() else {
                continue;
            };
            if let Some(&(d, p)) = self.sssp.get(id) {
                r.sssp_distance = d;
                r.sssp_parent = p;
            }
            current.push(r);
        }
        current.extend(base.actors.iter().copied());
        let sel = self
            .selector
            .select(&current, now_ms, MAX_RECORDS_PER_FRAME);
        if sel.records.is_empty() {
            return None;
        }
        let seq = self.seq.advance();
        self.window.on_sent(seq, now_ms);
        self.frames_sent += 1;
        Some(v5_frame(seq, &sel.records))
    }
}
