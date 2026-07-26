//! # Module: op_log
//!
//! Durable operation-log data types — actor + causal (Lamport / session-origin)
//! tagging for document node operations (`#op-scoped-drift-1`, phase 1 of the
//! operation-scoped drift model in `tasks/agent-doc/plan-operation-scoped-drift.md`).
//!
//! ## Spec
//! - `OpActor` names who produced a document operation: the managing `agent`,
//!   the `user`, a concurrent `foreign_supervisor`, or a lagging recovery projection.
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

macro_rules! ops_log_events {
    ($( $variant:ident => $token:literal ),+ $(,)?) => {
        /// Stable event names that agent-doc code parses from `ops.log`.
        ///
        /// This intentionally excludes human-only diagnostics. Adding a parsed event here makes
        /// producer/consumer renames compiler-visible instead of relying on duplicated string
        /// literals whose mismatch silently evaluates to `false`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum OpsLogEvent {
            $( $variant, )+
        }

        impl OpsLogEvent {
            /// Complete vocabulary of event names parsed by agent-doc code.
            pub const PARSED_EVENTS: &'static [Self] = &[
                $( Self::$variant, )+
            ];

            /// Stable first-token representation written to `ops.log`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$variant => $token, )+
                }
            }

            /// Parse one exact event-name token.
            pub fn from_token(token: &str) -> Option<Self> {
                match token {
                    $( $token => Some(Self::$variant), )+
                    _ => None,
                }
            }

            /// Parse the first event token from a complete, optionally timestamped log line.
            pub fn from_line(line: &str) -> Option<Self> {
                Self::from_token(event_name(strip_timestamp_prefix(line)))
            }

            /// True when this is the leading event token of `line`.
            pub fn is_line(self, line: &str) -> bool {
                Self::from_line(line) == Some(self)
            }

            /// True when this event occurs as an exact whitespace-delimited token,
            /// either standalone or as a `key=<event>` field value.
            ///
            /// Some legacy doctor records embed a causal event name in a field instead of using
            /// it as the line's leading token. This helper keeps those records readable without
            /// returning to substring matching.
            pub fn is_line_or_field_value(self, line: &str) -> bool {
                if self.is_line(line) {
                    return true;
                }
                let expected = self.as_str();
            strip_timestamp_prefix(line)
                .split_ascii_whitespace()
                .map(|field| {
                    field
                        .split_once('=')
                        .map(|(_, value)| value)
                        .unwrap_or(field)
                })
                .map(|value| value.trim_matches([',', ';', '"', '\'']))
                .any(|value| value == expected)
            }
        }

        impl std::fmt::Display for OpsLogEvent {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

ops_log_events! {
    PreflightDiffStart => "preflight_diff_start",
    IpcWriteConsumed => "ipc_write_consumed",
    IpcProofInsufficient => "ipc_proof_insufficient",
    RealtimeDocResolve => "realtime_doc_resolve",
    RealtimeDocResolveCrdtError => "realtime_doc_resolve_crdt_error",
    CrdtCurrentTextUnavailable => "crdt_current_text_unavailable",
    DocumentModelEnsureStart => "document_model_ensure_start",
    DocumentModelEnsurePublishRequested => "document_model_ensure_publish_requested",
    DocumentModelEnsureFailed => "document_model_ensure_failed",
    CommitSuccess => "commit_success",
    RepairCommitBoundaryRecovered => "repair_commit_boundary_recovered",
    RecursiveDirectInvocationBlocked => "recursive_direct_invocation_blocked",
    FlowEvent => "flow_event",
    InterruptedCycleDetected => "interrupted_cycle_detected",
    LateFallbackPatchRejected => "late_fallback_patch_rejected",
    StaleSnapshotResetDriftBlocked => "stale_snapshot_reset_drift_blocked",
    CommitBlockedMissingCapturedResponse => "commit_blocked_missing_captured_response",
    SessionCheckCommitBoundaryRecovered => "session_check_commit_boundary_recovered",
    CommitNoop => "commit_noop",
    RouteDispatchStartProven => "route_dispatch_start_proven",
    RouteSubmitIssue => "route_submit_issue",
    PostCommitUserFollowUp => "post_commit_user_follow_up",
    PostCommitLocalDrift => "post_commit_local_drift",
    SessionClearActivePaneAllowed => "session_clear_active_pane_allowed",
    SessionClearProtectedInputGuardRefused => "session_clear_protected_input_guard_refused",
    SessionClearLiveBusyGuardBypassed => "session_clear_live_busy_guard_bypassed",
    SessionClearLiveBusyGuardRefused => "session_clear_live_busy_guard_refused",
    SessionClearLiveBusyGuardBlocked => "session_clear_live_busy_guard_blocked",
    RouteAuthoritativeActorStartingNotReady => "route_authoritative_actor_starting_not_ready",
    RouteStartingActorTimeoutCoalesced => "route_starting_actor_timeout_coalesced",
    RouteCycleStartMissing => "route_cycle_start_missing",
    RouteCycleStartMissingAfterFreshRestartOptimistic =>
        "route_cycle_start_missing_after_fresh_restart_optimistic",
    RouteCycleStartMissingOptimistic => "route_cycle_start_missing_optimistic",
    RouteDispatchStartUnprovenButAccepted => "route_dispatch_start_unproven_but_accepted",
    RouteDispatchOnlySent => "route_dispatch_only_sent",
    RouteDispatchOnlySubmitUnproven => "route_dispatch_only_submit_unproven",
    RunPreflightTimeout => "run_preflight_timeout",
    DirectInvocationTimeout => "direct_invocation_timeout",
    SyncLatency => "sync_latency",
    SqliteLogCounts => "sqlite_log_counts",
    SqliteLogCount => "sqlite_log_count",
    SessionReviewGuard => "session_review_guard",
    CodexThreadStarted => "codex_thread_started",
    ClaudeJsonlHookMarker => "claude_jsonl_hook_marker",
    AgentDocCycleMarker => "agent_doc_cycle_marker",
    SupervisorBinaryStale => "supervisor_binary_stale",
    RetryOnCurrentGeneration => "retry_on_current_generation",
    SupervisorRestartRedirect => "supervisor_restart_redirect",
    StaleGeneration => "stale_generation",
    EditorConvergenceAckMismatch => "editor_convergence_ack_mismatch",
    EditorConvergenceNoAck => "editor_convergence_no_ack",
    LivePromptDriftAfterPreflight => "live_prompt_drift_after_preflight",
    EditorOpRecorded => "editor_op_recorded",
    EditorOpsForBase => "editor_ops_for_base",
    EditorOpRecordFailed => "editor_op_record_failed",
    ConvergenceGateBlocked => "convergence_gate_blocked",
}

/// Event name prefix emitted by preflight when a cycle starts.
pub const PREFLIGHT_START_EVENT: &str = OpsLogEvent::PreflightDiffStart.as_str();
/// Event name emitted when an IPC write was consumed and awaits commit proof.
pub const IPC_WRITE_CONSUMED_EVENT: &str = OpsLogEvent::IpcWriteConsumed.as_str();
/// Event name emitted when IPC response-materialization proof is insufficient.
pub const IPC_PROOF_INSUFFICIENT_EVENT: &str = OpsLogEvent::IpcProofInsufficient.as_str();

/// Strip a leading `[NNN] ` timestamp prefix from an ops-log line.
pub fn strip_timestamp_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(close) = rest.find("] ")
    {
        return &rest[close + 2..];
    }
    line
}

/// True when an ops-log event proves a write landed but commit proof is missing.
pub fn is_write_completed_commit_missing_event(event: &str) -> bool {
    OpsLogEvent::IpcWriteConsumed.is_line(event)
}

/// Return the first whitespace-delimited event token from an ops-log event.
pub fn event_name(event: &str) -> &str {
    event.split_whitespace().next().unwrap_or(event)
}

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
    /// A lagging recovery projection disk write (provenance-spoofed drift).
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
    /// a lagging recovery projection disk write (class-5 spoofed drift).
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

/// Build durable op-log records from semantic node events.
///
/// Preflight observes a snapshot-to-document diff, so every node op is
/// classified as a `user` edit: the agent's committed output already lives in
/// the snapshot. The durable store owns Lamport assignment; this builder leaves
/// the placeholder clock at `0`.
pub fn build_ops_from_semantic_diff(
    document_path: &str,
    origin_session: Option<&str>,
    recorded_at: &str,
    summary: &agent_doc_diff::semantic::SemanticDiffSummary,
) -> Vec<DocumentOp> {
    let actor = classify_actor(OpSource::SnapshotDiff);
    summary
        .node_events
        .iter()
        .map(|event| DocumentOp {
            document_path: document_path.to_string(),
            component: event.component.clone(),
            node_key: event.node_key.clone(),
            // Within-component node index: after-index for inserts/replaces,
            // before-index for removes. This feeds the exchange-tail
            // affectedness classifier.
            node_index: event.after_index.or(event.before_index),
            item_id: event.item_id.clone(),
            op_kind: event.op.clone(),
            actor,
            clock: CausalClock {
                lamport: 0,
                origin_session: origin_session.map(str::to_string),
            },
            before_preview: event.before_preview.clone(),
            after_preview: event.after_preview.clone(),
            recorded_at: Some(recorded_at.to_string()),
        })
        .collect()
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
    use crate::turn_scope::{Address, AffectednessClass, TurnScope, classify_cycle};
    use agent_doc_diff::semantic::semantic_diff_summary;

    #[test]
    fn ops_log_line_event_helpers_are_stable() {
        assert_eq!(
            strip_timestamp_prefix("[1700000000] preflight_diff_start file=/x"),
            "preflight_diff_start file=/x"
        );
        assert_eq!(strip_timestamp_prefix("no bracket"), "no bracket");
        assert!(is_write_completed_commit_missing_event(
            "ipc_write_consumed file=x patches=1"
        ));
        assert!(!is_write_completed_commit_missing_event(
            "preflight_diff_start file=x"
        ));
        assert_eq!(
            event_name("ipc_write_consumed file=x"),
            IPC_WRITE_CONSUMED_EVENT
        );
        assert_eq!(event_name(""), "");
        assert_eq!(PREFLIGHT_START_EVENT, "preflight_diff_start");
        assert_eq!(IPC_PROOF_INSUFFICIENT_EVENT, "ipc_proof_insufficient");
        assert_eq!(
            OpsLogEvent::FlowEvent.as_str(),
            agent_doc_flow::types::FLOW_EVENT_LOG_NAME
        );
        assert!(
            agent_doc_diff::line_is_binary_authored_ipc_proof_diagnostic(&format!(
                "{} invariant=visible_write recovery=retry_without_disk_write",
                OpsLogEvent::IpcProofInsufficient
            ))
        );
    }

    #[test]
    fn parsed_ops_log_event_vocabulary_is_unique_and_round_trips() {
        let mut tokens = std::collections::BTreeSet::new();
        for event in OpsLogEvent::PARSED_EVENTS {
            assert!(
                tokens.insert(event.as_str()),
                "duplicate parsed ops-log token {}",
                event.as_str()
            );
            assert_eq!(OpsLogEvent::from_token(event.as_str()), Some(*event));
            assert_eq!(
                OpsLogEvent::from_line(&format!("[1700000000] {event} file=/x")),
                Some(*event)
            );
        }
        assert_eq!(
            OpsLogEvent::from_line("[1700000000] human_only_diagnostic file=/x"),
            None
        );
    }

    #[test]
    fn parsed_event_matching_is_token_exact() {
        let event = OpsLogEvent::SupervisorBinaryStale;
        assert!(event.is_line("supervisor_binary_stale file=/x"));
        assert!(event.is_line_or_field_value("route_failed supervisor_binary_stale file=/x"));
        assert!(event.is_line_or_field_value("route_failed reason=supervisor_binary_stale"));
        assert!(!event.is_line("prefix_supervisor_binary_stale file=/x"));
        assert!(
            !event.is_line_or_field_value("route_failed reason=prefix_supervisor_binary_stale")
        );
    }

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

    #[test]
    fn build_ops_from_semantic_diff_tags_user_actor_and_session() {
        let before = "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n";
        let after = "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n";
        let summary = semantic_diff_summary(before, after, &[]).unwrap();
        let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "100", &summary);
        assert!(!ops.is_empty());
        let beta = ops
            .iter()
            .find(|op| op.node_key == "queue:0:beta:0")
            .expect("beta op present");
        assert_eq!(beta.actor, OpActor::User);
        assert_eq!(beta.op_kind, "insert");
        assert_eq!(beta.component, "queue");
        assert_eq!(beta.clock.origin_session.as_deref(), Some("sess-1"));
        // Lamport assignment is owned by the durable store; the builder leaves 0.
        assert_eq!(beta.clock.lamport, 0);
    }

    #[test]
    fn sibling_queue_insert_beside_driver_is_independent() {
        // The motivating case: the turn answers queue item A while the user
        // inserts queue item B beside it. B must classify Independent and the
        // turn must not be affected (#op-scoped-drift-3).
        let before = "<!-- agent:queue -->\n- do [#driver-a]\n<!-- /agent:queue -->\n";
        let after =
            "<!-- agent:queue -->\n- do [#driver-a]\n- do [#sibling-b]\n<!-- /agent:queue -->\n";
        let summary = semantic_diff_summary(before, after, &[]).unwrap();
        let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
        let scope = TurnScope::for_driver(Some(Address::node("queue", 0, "queue:0:driver-a:0")));
        let affectedness = classify_cycle(&ops, &scope);
        assert!(
            !affectedness.turn_affected,
            "a sibling queue insert must not affect the turn"
        );
        assert!(
            affectedness
                .classified
                .iter()
                .all(|op| op.class == AffectednessClass::Independent)
        );
    }

    #[test]
    fn exchange_old_block_edit_is_independent_but_tail_append_affects() {
        // #loop-guard-exchange-node-granularity end-to-end: while the turn
        // answers a queue driver, an edit to an OLD bulleted exchange block must
        // classify Independent (must not preempt the auto-loop drain), while a
        // genuine new bulleted prompt appended at the exchange tail must still
        // affect the turn.
        let base = "\
<!-- agent:exchange -->
### Re: prior topic

- old context bullet one
- old context bullet two
<!-- agent:boundary:b1 -->
<!-- /agent:exchange -->

<!-- agent:queue go -->
- do [#driver]
<!-- /agent:queue -->
";
        let scope = TurnScope::for_driver_with_exchange_tail(
            Some(Address::node("queue", 0, "queue:0:driver:0")),
            Some(2),
        );
        assert_eq!(
            scope.exchange_tail_floor,
            Some(2),
            "two committed exchange bullets => tail floor 2"
        );

        // Old-block edit: change the FIRST (index 0) exchange bullet.
        let old_edit = base.replace(
            "- old context bullet one",
            "- old context bullet one EDITED",
        );
        let summary = semantic_diff_summary(base, &old_edit, &[]).unwrap();
        let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
        let affectedness = classify_cycle(&ops, &scope);
        assert!(
            !affectedness.turn_affected,
            "editing an old exchange block must not affect the turn: {:?}",
            affectedness.classified
        );

        // Tail append: a new bulleted prompt after the last committed bullet.
        let tail_append = base.replace(
            "- old context bullet two\n",
            "- old context bullet two\n- please also cover the retry path\n",
        );
        let summary2 = semantic_diff_summary(base, &tail_append, &[]).unwrap();
        let ops2 = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary2);
        let affectedness2 = classify_cycle(&ops2, &scope);
        assert!(
            affectedness2.turn_affected,
            "a new tail-appended exchange prompt must still affect the turn: {:?}",
            affectedness2.classified
        );
    }
}
