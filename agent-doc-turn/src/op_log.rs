//! # Module: op_log
//!
//! Durable operation-log data types — actor + causal (Lamport / session-origin)
//! tagging for document node operations (`#op-scoped-drift-1`, phase 1 of the
//! operation-scoped drift model in `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - `OpActor` names who produced a document operation: the managing `agent`,
//!   the `user`, a concurrent `foreign_supervisor`, or a lagging `live_buffer`
//!   editor sidecar.
//! - `OpSource` is the signal a caller already has about where an op came from;
//!   `classify_actor` maps it to an `OpActor`. Phase-1 rules are source-driven:
//!   a snapshot↔document divergence observed at preflight is a `user` edit,
//!   because the agent's own committed output already lives in the snapshot.
//! - `CausalClock` carries the Lamport logical tick plus the originating
//!   session id. The durable store (`agent-doc-sqlite`) owns Lamport assignment;
//!   callers populate `origin_session` and leave `lamport` as a placeholder.
//! - `DocumentOp` is the durable record persisted to the sqlite op log. It is
//!   pure data so the sqlite writer and orchestration callers share one schema.
//!
//! ## Agentic Contracts
//! - Actor classification never blocks a cycle; it only annotates ops.
//! - The op-log substrate is additive: phase 2 (TurnScope) and phase 3 (the
//!   affectedness classifier) read these records but phase 1 only writes them.

use serde::{Deserialize, Serialize};

/// Who produced a document operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpActor {
    /// The managing agent's own write-back (response append, status, pending).
    Agent,
    /// A genuine human edit observed between cycles.
    User,
    /// A concurrent agent-doc supervisor writing the same document.
    ForeignSupervisor,
    /// A lagging editor live-buffer sidecar disk write (provenance-spoofed drift).
    LiveBuffer,
}

impl OpActor {
    /// Stable lowercase string form used in the durable store and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            OpActor::Agent => "agent",
            OpActor::User => "user",
            OpActor::ForeignSupervisor => "foreign_supervisor",
            OpActor::LiveBuffer => "live_buffer",
        }
    }

    /// Parse the stable string form back into an actor.
    pub fn from_str_lenient(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(OpActor::Agent),
            "user" => Some(OpActor::User),
            "foreign_supervisor" => Some(OpActor::ForeignSupervisor),
            "live_buffer" => Some(OpActor::LiveBuffer),
            _ => None,
        }
    }
}

/// The provenance signal a caller has when it observes a batch of ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpSource {
    /// snapshot↔document divergence observed at preflight (a user edit).
    SnapshotDiff,
    /// a lagging live-buffer sidecar disk write (class-5 spoofed drift).
    LiveBufferDrift,
    /// a concurrent foreign supervisor write contending the same document.
    ForeignSupervisorWrite,
    /// the managing agent's own write-back.
    AgentWrite,
}

/// Map a provenance signal to the actor that produced the op.
///
/// Phase-1 rules are intentionally source-driven. Component-role refinement
/// (e.g. distinguishing an agent output append from a user edit *within* the
/// same snapshot diff) is deferred to the phase-3 affectedness classifier;
/// at preflight the committed snapshot already contains the agent's last
/// output, so any snapshot↔document divergence is a user edit.
pub fn classify_actor(source: OpSource) -> OpActor {
    match source {
        OpSource::SnapshotDiff => OpActor::User,
        OpSource::LiveBufferDrift => OpActor::LiveBuffer,
        OpSource::ForeignSupervisorWrite => OpActor::ForeignSupervisor,
        OpSource::AgentWrite => OpActor::Agent,
    }
}

/// Lamport logical clock plus the originating session id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CausalClock {
    /// Monotonic per-document logical tick. The durable store assigns the
    /// authoritative value; callers may leave this `0`.
    pub lamport: u64,
    /// `agent_doc_session` of the session that produced the op, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_session: Option<String>,
}

/// A durable record of one node-keyed document operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentOp {
    /// Path of the session document the op applies to.
    pub document_path: String,
    /// Component the node lives in (`queue`, `exchange`, `backlog`, …).
    pub component: String,
    /// Stable node key (`component:occurrence:item-id:dup`).
    pub node_key: String,
    /// Position of the node within its component (after-index for inserts/
    /// replaces, before-index for removes). Carries the tail-vs-old-block signal
    /// the affectedness classifier needs to narrow `exchange` to its active tail
    /// (`#loop-guard-exchange-node-granularity`). `None` when the source did not
    /// supply an index (e.g. an op replayed from the durable store, which never
    /// feeds the live classifier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_index: Option<usize>,
    /// Backlog/queue item id, when the node carries one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item_id: String,
    /// Operation kind string (`insert`, `remove`, `replace`, `move`, `strike`,
    /// `unstrike`) mirroring the markdown-AST node-event vocabulary.
    pub op_kind: String,
    /// Who produced the op.
    pub actor: OpActor,
    /// Lamport tick + originating session.
    pub clock: CausalClock,
    /// Bounded preview of the node content before the op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_preview: Option<String>,
    /// Bounded preview of the node content after the op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_preview: Option<String>,
    /// Wall-clock timestamp the op was recorded, when the caller supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
}

impl DocumentOp {
    /// True when two ops describe the same node-level mutation (ignoring the
    /// writer-assigned Lamport tick and recording time). Used by the durable
    /// store to keep repeated preflight passes over the same uncommitted diff
    /// idempotent.
    pub fn same_mutation(&self, other: &DocumentOp) -> bool {
        self.document_path == other.document_path
            && self.component == other.component
            && self.node_key == other.node_key
            && self.op_kind == other.op_kind
            && self.before_preview == other.before_preview
            && self.after_preview == other.after_preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_actor_is_source_driven() {
        assert_eq!(classify_actor(OpSource::SnapshotDiff), OpActor::User);
        assert_eq!(
            classify_actor(OpSource::LiveBufferDrift),
            OpActor::LiveBuffer
        );
        assert_eq!(
            classify_actor(OpSource::ForeignSupervisorWrite),
            OpActor::ForeignSupervisor
        );
        assert_eq!(classify_actor(OpSource::AgentWrite), OpActor::Agent);
    }

    #[test]
    fn op_actor_string_round_trips() {
        for actor in [
            OpActor::Agent,
            OpActor::User,
            OpActor::ForeignSupervisor,
            OpActor::LiveBuffer,
        ] {
            assert_eq!(OpActor::from_str_lenient(actor.as_str()), Some(actor));
        }
        assert_eq!(OpActor::from_str_lenient("nonsense"), None);
    }

    #[test]
    fn document_op_serde_round_trips() {
        let op = DocumentOp {
            document_path: "plan.md".to_string(),
            component: "queue".to_string(),
            node_key: "queue:0:beta:0".to_string(),
            node_index: Some(1),
            item_id: "beta".to_string(),
            op_kind: "insert".to_string(),
            actor: OpActor::User,
            clock: CausalClock {
                lamport: 0,
                origin_session: Some("sess-1".to_string()),
            },
            before_preview: None,
            after_preview: Some("- do [#beta]".to_string()),
            recorded_at: Some("123".to_string()),
        };
        let json = serde_json::to_string(&op).unwrap();
        let parsed: DocumentOp = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, op);
        // actor renders as the stable snake_case string in JSON.
        assert!(json.contains("\"actor\":\"user\""));
    }

    #[test]
    fn same_mutation_ignores_clock_and_time() {
        let base = DocumentOp {
            document_path: "plan.md".to_string(),
            component: "queue".to_string(),
            node_key: "queue:0:beta:0".to_string(),
            node_index: None,
            item_id: "beta".to_string(),
            op_kind: "insert".to_string(),
            actor: OpActor::User,
            clock: CausalClock {
                lamport: 5,
                origin_session: Some("sess-1".to_string()),
            },
            before_preview: None,
            after_preview: Some("- do [#beta]".to_string()),
            recorded_at: Some("100".to_string()),
        };
        let mut later = base.clone();
        later.clock.lamport = 99;
        later.recorded_at = Some("200".to_string());
        assert!(base.same_mutation(&later));

        let mut different = base.clone();
        different.after_preview = Some("- do [#gamma]".to_string());
        assert!(!base.same_mutation(&different));
    }
}
