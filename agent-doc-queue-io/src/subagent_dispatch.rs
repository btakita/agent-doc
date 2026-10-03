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
}
