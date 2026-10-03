//! Preflight subagent-dispatch intent for new queue items (`#closeout-steering`).
//!
//! An operator who adds `#subagents do [#id]` (or a line under a preset that
//! expands to subagent work) while a turn runs means "dispatch this now", not
//! "drain it inline when its turn in queue order comes". The mid-turn
//! PostToolUse hook and the closeout report carry that intent when they run;
//! this module is the next-preflight backstop: a live, unclaimed queue line
//! with subagent intent that is NEW since the previous cycle's steering seed
//! is listed in the cycle contract as `queue_subagent_dispatch` and kept out
//! of `selected_queue_prompts`, so the in-session agent claims and dispatches
//! it instead of executing it inline.
//!
//! "New" is judged against the previous cycle's seed so an item gets one
//! dispatch-intent window: once claimed it is excluded by the claim, and once
//! a later cycle has seeded with it present it drains through the normal
//! queue (a released claim means the subagent reported back and the
//! in-session loop now closes the item).
//!
//! The seed record lives in `state.db` `project_runtime_state`; the key format
//! is shared with `agent-doc-session-check-io::midturn_steering`.

use std::path::Path;

use anyhow::{Context, Result};

use agent_doc_document_realtime::midturn_steering::{self as core, SteeringWatermark};

/// Key prefix of every mid-turn steering record in `project_runtime_state`.
pub const MIDTURN_STEERING_KEY_PREFIX: &str = "midturn_steering";

fn document_key(file: &Path) -> String {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    agent_doc_hash::content_hash(&canonical.display().to_string())
}

/// `project_runtime_state` key for one mid-turn steering record of `file`
/// (`kind` = `base`, or a consumer name).
pub fn midturn_steering_state_key(kind: &str, file: &Path) -> String {
    format!(
        "{MIDTURN_STEERING_KEY_PREFIX}:{kind}:{}",
        document_key(file)
    )
}

/// The seeded base watermark of `file`, if preflight ever seeded one.
pub fn load_midturn_base(file: &Path) -> Result<Option<SteeringWatermark>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        &midturn_steering_state_key("base", file),
    )?
    .map(|raw| serde_json::from_str(&raw).context("parse mid-turn steering base watermark"))
    .transpose()
}

/// The queue view "new" is judged against: the previous cycle's seed queue.
///
/// When the stored base belongs to a cycle that is still open (a re-entrant
/// preflight already re-seeded it), the previous cycle's queue is the base's
/// `prior_cycle_queue`; otherwise the base itself is the previous cycle's
/// seed. `None` when no reference exists.
pub fn reference_queue_for_dispatch(file: &Path) -> Result<Option<Vec<String>>> {
    let Some(base) = load_midturn_base(file)? else {
        return Ok(None);
    };
    let reentrant = agent_doc_cycle_state_io::load(file)?.is_some_and(|cycle| {
        cycle.cycle_id == base.cycle_id
            && matches!(cycle.phase, agent_doc_turn::CyclePhase::PreflightStarted)
    });
    Ok(if reentrant {
        base.prior_cycle_queue
    } else {
        Some(base.acknowledged_queue)
    })
}

/// Live, unclaimed queue lines with subagent intent that are new since the
/// previous cycle's seed, in queue order.
pub fn pending_subagent_dispatch_for_content(file: &Path, content: &str) -> Result<Vec<String>> {
    if let Some(plan) = queue_attr_subagent_plan(file, content) {
        return Ok(plan.dispatch);
    }
    let reference = reference_queue_for_dispatch(file)?;
    let heads = core::subagent_dispatch_heads(content, reference.as_deref());
    if heads.is_empty() {
        return Ok(heads);
    }
    let claimed = crate::queue_claim::claimed_items_for_content(file, content);
    Ok(heads
        .into_iter()
        .filter(|head| !claimed.claims(head))
        .collect())
}

/// The queue-level subagents attribute (`<!-- agent:queue subagents=N -->`)
/// planned over the current queue against the raw claim ledger: which heads
/// to dispatch now and which to hold (cap full, or an `after=` predecessor
/// still queued). `None` when the attribute is absent or invalid.
pub fn queue_attr_subagent_plan(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::subagent_intent::QueueSubagentPlan> {
    let (mode, eligible, live) = core::queue_attr_subagent_heads(content)?;
    let claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content);
    let after_deps = agent_doc_element::element::parse(content)
        .map(|components| agent_doc_queue::backlog_sync::collect_after_deps(&components, content))
        .unwrap_or_default();
    Some(
        agent_doc_queue::subagent_intent::plan_queue_subagent_dispatch(
            mode,
            &eligible,
            &live,
            &claimed,
            &after_deps,
        ),
    )
}

/// [`pending_subagent_dispatch_for_content`] that reports a failure and
/// degrades to "no dispatch items" (the pre-feature behaviour: the line drains
/// in queue order) instead of refusing the cycle.
pub fn pending_subagent_dispatch_or_warn(file: &Path, content: &str) -> Vec<String> {
    match pending_subagent_dispatch_for_content(file, content) {
        Ok(heads) => heads,
        Err(err) => {
            eprintln!(
                "[queue] WARNING: could not compute subagent dispatch intent for {}; new \
                 subagent-tagged items drain in queue order this cycle: {err:#}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_subagent_dispatch_error file={} error={err:#}",
                    file.display()
                ),
            );
            Vec::new()
        }
    }
}

/// The exact `agent-doc queue claim` command for dispatching `item`.
pub fn claim_command(document: &str, item: &str) -> String {
    core::claim_command_for(document, item)
}

/// Whether `prompt` is one of the `dispatch` lines (marker-invariant identity).
pub fn is_dispatch_item(dispatch: &[String], prompt: &str) -> bool {
    if dispatch.is_empty() {
        return false;
    }
    let identity = agent_doc_queue::queue_claim::claim_identity(prompt);
    dispatch
        .iter()
        .any(|item| agent_doc_queue::queue_claim::claim_identity(item) == identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(prompts: &[&str]) -> String {
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    fn seed(file: &Path, cycle_id: &str, baseline: &str) {
        let root = agent_doc_fs::find_project_root(file).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        let watermark = SteeringWatermark::seed(cycle_id, baseline, None, Vec::new());
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &midturn_steering_state_key("base", file),
            &serde_json::to_string(&watermark).unwrap(),
            1,
        )
        .unwrap();
    }

    #[test]
    fn new_subagent_items_are_listed_until_claimed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let before = doc(&["current task", "#subagents do [#old1]"]);
        std::fs::write(&file, &before).unwrap();
        seed(&file, "cycle-prev", &before);

        let after = doc(&[
            "current task",
            "#subagents do [#old1]",
            "#subagents do [#preflightdeadline]",
            "plain follow-up",
        ]);
        std::fs::write(&file, &after).unwrap();
        let listed = pending_subagent_dispatch_for_content(&file, &after).unwrap();
        assert_eq!(
            listed,
            vec!["#subagents do [#preflightdeadline]".to_string()]
        );
        assert!(is_dispatch_item(
            &listed,
            "🚧 #subagents do [#preflightdeadline]"
        ));
        assert!(!is_dispatch_item(&listed, "plain follow-up"));

        crate::queue_claim::claim(&file, "#preflightdeadline", "subagent:x", 600).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &after)
                .unwrap()
                .is_empty(),
            "a claimed item is already dispatched"
        );
    }

    /// `#claimdispatchidentity` (live 2026-10-03, agent-doc-bugs2): the
    /// operator edited the claimed `#subagents: <url>` into
    /// `#subagents: #gh-fix <url>` while its subagent worked, and the line
    /// re-appeared in `queue_subagent_dispatch` as new and unclaimed. The
    /// claim must follow the edit; a retarget to another issue must not.
    #[test]
    fn edited_claimed_line_keeps_its_claim_and_is_not_redispatched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let url = "https://github.com/btakita/agent-doc/issues/120";
        let before = doc(&["current task"]);
        std::fs::write(&file, &before).unwrap();
        seed(&file, "cycle-prev", &before);

        let added = doc(&["current task", &format!("#subagents: {url}")]);
        std::fs::write(&file, &added).unwrap();
        let offered = pending_subagent_dispatch_for_content(&file, &added).unwrap();
        assert_eq!(offered, vec![format!("#subagents: {url}")]);
        let handle = core::claim_item_handle(&offered[0]);
        crate::queue_claim::claim(&file, &handle, "subagent:gh120", 600).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &added)
                .unwrap()
                .is_empty()
        );

        let edited_line = format!("#subagents: #gh-fix {url}");
        let edited = doc(&["current task", &edited_line]);
        std::fs::write(&file, &edited).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &edited)
                .unwrap()
                .is_empty(),
            "the edited line is still claimed, not new dispatch work"
        );
        let claimed = crate::queue_claim::claimed_items_for_content(&file, &edited);
        assert!(claimed.claims(&edited_line));
        assert!(claimed.claims(&format!("🚧 {edited_line}")));
        assert_eq!(
            crate::queue_claim::active_claims_for_content(&file, &edited)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            crate::queue_claim::prune_closed_claims(&file, &edited).unwrap(),
            0,
            "closeout must not prune the edited line's claim as closed"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::drainable_head_count_excluding_claimed(
                &edited, &claimed
            ),
            1,
            "only the inline head is drainable while the subagent works"
        );
        // The subagent can still refresh/release by the text it was handed.
        crate::queue_claim::refresh(&file, &handle, "subagent:gh120", 600).unwrap();

        let retarget = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/121";
        let retargeted = doc(&["current task", retarget]);
        std::fs::write(&file, &retargeted).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &retargeted).unwrap(),
            vec![retarget.to_string()],
            "a different issue is a different task: unclaimed, offered"
        );
        assert!(
            !crate::queue_claim::claimed_items_for_content(&file, &retargeted).claims(retarget)
        );
    }

    /// GH #124 ask 5: every head `queue_subagent_dispatch` offers is one
    /// `queue claim` accepts through the handle it prints, and once claimed,
    /// dispatch, drainability, preflight selection and the steering claim
    /// filter all treat that same head as claimed.
    #[test]
    fn claim_dispatch_steering_and_drainability_agree_on_heads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = doc(&[
            "inline task",
            "🚧 #subagents do [#x]",
            "#subagent: https://x/issues/7",
            "#bug: The tmux panes are swapped. #subagents",
            "#subagents: #gh-fix https://x/issues/8",
            // A nesting parent is a group label, not a claimable head.
            "#subagents: epic label\n  - #subagents do [#child]",
        ]);
        std::fs::write(&file, &content).unwrap();

        let offered = pending_subagent_dispatch_for_content(&file, &content).unwrap();
        assert_eq!(offered.len(), 5, "{offered:?}");
        let live = agent_doc_queue::queue_continuation::live_queue_head_texts(&content).unwrap();
        let drainable_before =
            agent_doc_queue::queue_continuation::drainable_head_count_excluding_claimed(
                &content,
                &crate::queue_claim::claimed_items_for_content(&file, &content),
            );
        assert_eq!(drainable_before, 6, "nothing claimed yet");
        for (n, head) in offered.iter().enumerate() {
            let handle = core::claim_item_handle(head);
            agent_doc_queue::queue_claim::resolve_claim_target(&handle, &live)
                .unwrap_or_else(|miss| panic!("dispatch offered {head:?}; claim refused: {miss}"));
            crate::queue_claim::claim(&file, &handle, &format!("subagent:{n}"), 600).unwrap();
            let claimed = crate::queue_claim::claimed_items_for_content(&file, &content);
            assert!(claimed.claims(head), "{head:?}");
            assert!(
                live.iter()
                    .any(|raw| claimed.claims(raw)
                        && is_dispatch_item(std::slice::from_ref(head), raw)),
                "the raw live line (with markers) is the claimed, dispatched head: {head:?}"
            );
            assert!(
                !pending_subagent_dispatch_for_content(&file, &content)
                    .unwrap()
                    .contains(head),
                "{head:?}"
            );
            assert_eq!(
                agent_doc_queue::queue_continuation::drainable_head_count_excluding_claimed(
                    &content, &claimed
                ),
                drainable_before.saturating_sub(n + 1),
                "drainability drops exactly the claimed head {head:?}"
            );
        }
        assert!(
            pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty()
        );
    }

    fn attr_doc(attr: &str, prompts: &[&str]) -> String {
        let queue: String = prompts.iter().map(|p| format!("- {p}\n")).collect();
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue {attr} go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    /// `#planattributeauto` phase 2: under the queue attribute, dispatch is
    /// state-based (a head already in the seed is still offered), capped by
    /// `subagents=N` minus live claims, and every held head is out of the
    /// in-session loop exactly like a claimed one.
    #[test]
    fn queue_attr_dispatch_is_state_based_capped_and_holds_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = attr_doc(
            "subagents=2",
            &["do [#a]", "do [#b]", "do [#c] [inline]", "do [#d]"],
        );
        std::fs::write(&file, &content).unwrap();
        // The previous cycle already saw every line: the per-item tag path
        // would offer nothing; the attribute still offers them.
        seed(&file, "cycle-prev", &content);

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["do [#a]".to_string(), "do [#b]".to_string()]
        );
        let excluded = crate::queue_claim::claimed_items_for_content(&file, &content);
        assert!(excluded.claims("do [#d]"), "over-cap head is held");
        assert!(!excluded.claims("do [#a]"), "dispatch heads stay visible");
        assert!(
            !excluded.claims("do [#c] [inline]"),
            "opt-out drains inline"
        );

        // A claimed head occupies a slot: one left, so `d` is still held.
        crate::queue_claim::claim(&file, "#a", "subagent:a", 600).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["do [#b]".to_string()]
        );
        // Both slots full: nothing more to dispatch, and `d` stays held.
        crate::queue_claim::claim(&file, "#b", "subagent:b", 600).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty()
        );
        let excluded = crate::queue_claim::claimed_items_for_content(&file, &content);
        assert!(excluded.claims("do [#d]"));
        assert_eq!(
            agent_doc_queue::queue_continuation::drainable_head_count_excluding_claimed(
                &content, &excluded
            ),
            1,
            "only the [inline] head is drainable in the session"
        );
    }
}
