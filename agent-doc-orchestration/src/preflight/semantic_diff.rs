//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

/// Build durable op-log records from this cycle's semantic node events
/// (`#op-scoped-drift-1`). Preflight observes a snapshot↔document diff, so every
/// node op is classified as a `user` edit (the agent's committed output already
/// lives in the snapshot). Pure so it can be unit-tested without a database.
pub(crate) fn build_ops_from_semantic_diff(
    document_path: &str,
    origin_session: Option<&str>,
    recorded_at: &str,
    summary: &agent_doc_diff::semantic::SemanticDiffSummary,
) -> Vec<agent_doc_turn::op_log::DocumentOp> {
    use agent_doc_turn::op_log::{CausalClock, DocumentOp, OpSource, classify_actor};
    let actor = classify_actor(OpSource::SnapshotDiff);
    summary
        .node_events
        .iter()
        .map(|event| DocumentOp {
            document_path: document_path.to_string(),
            component: event.component.clone(),
            node_key: event.node_key.clone(),
            // Within-component node index: after-index for inserts/replaces,
            // before-index for removes. Feeds the exchange-tail narrowing in the
            // affectedness classifier (`#loop-guard-exchange-node-granularity`).
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

/// Persist the cycle's node ops to the durable sqlite op log. Best effort:
/// failures are logged to stderr and never propagate, so the durable substrate
/// can never block a preflight cycle.
pub(crate) fn persist_op_log(
    file: &Path,
    rc: &crate::graph::RunContext,
    origin_session: Option<&str>,
    summary: &agent_doc_diff::semantic::SemanticDiffSummary,
) {
    if summary.node_events.is_empty() {
        return;
    }
    let Some(project_root) = rc.project_root() else {
        return;
    };
    let document_path = file.to_string_lossy().to_string();
    let recorded_at = op_log_timestamp().to_string();
    let ops = build_ops_from_semantic_diff(&document_path, origin_session, &recorded_at, summary);
    if let Err(err) = agent_doc_sqlite::op_log::append_ops(&project_root, &ops) {
        eprintln!("[preflight] op-log persist skipped: {err}");
    }
}

pub(crate) fn op_log_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub(crate) fn push_unique_strings(target: &mut Vec<String>, extras: Vec<String>) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

pub(crate) fn push_unique_prompt_bearing_changes(
    target: &mut Vec<agent_doc_diff::PromptBearingChange>,
    extras: Vec<agent_doc_diff::PromptBearingChange>,
) {
    for value in extras {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_diff::semantic::semantic_diff_summary;
    use agent_doc_workflow::session_cycle::derive_turn_scope;

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
        // The turn is answering driver-a.
        let scope = derive_turn_scope(after, &["do [#driver-a]".to_string()]).unwrap();
        let affectedness = agent_doc_turn::turn_scope::classify_cycle(&ops, &scope);
        assert!(
            !affectedness.turn_affected,
            "a sibling queue insert must not affect the turn"
        );
        assert!(
            affectedness
                .classified
                .iter()
                .all(|op| op.class == agent_doc_turn::turn_scope::AffectednessClass::Independent)
        );
    }
    #[test]
    fn exchange_old_block_edit_is_independent_but_tail_append_affects() {
        // #loop-guard-exchange-node-granularity end-to-end: while the turn answers
        // a queue driver, an edit to an OLD bulleted exchange block must classify
        // Independent (must not preempt the auto-loop drain), while a genuine new
        // bulleted prompt appended at the exchange tail must still affect the turn.
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
        let targets = vec!["do [#driver]".to_string()];
        let scope = derive_turn_scope(base, &targets).expect("scope derived");
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
        let affectedness = agent_doc_turn::turn_scope::classify_cycle(&ops, &scope);
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
        let affectedness2 = agent_doc_turn::turn_scope::classify_cycle(&ops2, &scope);
        assert!(
            affectedness2.turn_affected,
            "a new tail-appended exchange prompt must still affect the turn: {:?}",
            affectedness2.classified
        );
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
        assert_eq!(beta.actor, agent_doc_turn::op_log::OpActor::User);
        assert_eq!(beta.op_kind, "insert");
        assert_eq!(beta.component, "queue");
        assert_eq!(beta.clock.origin_session.as_deref(), Some("sess-1"));
        // Lamport assignment is owned by the durable store; the builder leaves 0.
        assert_eq!(beta.clock.lamport, 0);
    }
}
