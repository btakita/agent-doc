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
    let _guard = crate::queue_claim::steer_guard_for(file, content);
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    // `#reportedrelease`: a line whose worker already reported (its claim was
    // released with a receipt) is the coordinator's to persist, never a new
    // worker's: offering it again dispatched a duplicate subagent for an
    // answered question (haiven-dev tasks/docs.md, 2026-10-15).
    let reported = crate::claim_release_receipt::reported_items_for_content(file, content);
    if let Some(plan) = queue_attr_subagent_plan_with_presets(file, content, &prompt_presets) {
        return Ok(plan
            .dispatch
            .into_iter()
            .filter(|head| !reported.claims(head))
            .collect());
    }
    let reference = reference_queue_for_dispatch(file)?;
    let heads =
        core::subagent_dispatch_heads_with_presets(content, reference.as_deref(), &prompt_presets);
    if heads.is_empty() {
        return Ok(heads);
    }
    let claimed = crate::queue_claim::claimed_items_for_content(file, content);
    Ok(heads
        .into_iter()
        .filter(|head| !claimed.claims(head) && !reported.claims(head))
        .collect())
}

/// The queue-level subagents attribute (`<!-- agent:queue subagents=N -->`)
/// planned over every independent `subagents` queue against the raw claim
/// ledger: which heads to dispatch now and which to hold (an explicit `=N`
/// cap is full, or an item `after=` predecessor is still queued). A bare
/// attribute dispatches every eligible head of that queue. Sibling
/// `subagents` queues are offered in the same cycle; an `after=` queue waits
/// until every predecessor is empty, and an earlier inline queue that still
/// has owner-available work gates later queues. `None` when no non-empty
/// queue has a valid attribute, so the per-line "new since seed" path still
/// runs.
pub fn queue_attr_subagent_plan(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::subagent_intent::QueueSubagentPlan> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file).ok()?;
    queue_attr_subagent_plan_with_presets(file, content, &prompt_presets)
}

/// Claims that choose which queue owns this cycle: ledger attempts (including
/// `Reported`), closed schedule windows, and completion evidence. This cycle's
/// fresh dispatch heads are not included, so naming them does not pause the
/// queue just selected.
pub fn owner_selection_claims_for_content(
    file: &Path,
    content: &str,
) -> agent_doc_queue::queue_claim::ClaimedQueueItems {
    let schedule_held = crate::queue_schedule::schedule_held_heads(file, content);
    crate::queue_claim::ledger_claimed_items_for_content(file, content)
        .with_heads(&schedule_held)
        .with_heads(&crate::queue_claim::completion_evidence_head_texts(content))
}

fn queue_attr_subagent_plan_with_presets(
    file: &Path,
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Option<agent_doc_queue::subagent_intent::QueueSubagentPlan> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let graph = agent_doc_queue::queue_set::QueueSet::parse(content, &components).ok()?;
    // `#queueschedule`: a closed schedule window dispatches nothing.
    // Do not call `claimed_items_for_content` here: it folds this plan's held
    // heads back in and would recurse.
    let schedule_held = crate::queue_schedule::schedule_held_heads(file, content);
    let mut claimed = crate::queue_claim::ledger_claimed_items_for_content(file, content)
        .with_heads(&schedule_held)
        .with_heads(&crate::queue_claim::completion_evidence_head_texts(content));
    let evidence_items = agent_doc_document::subagent_completion::completion_evidence(content)
        .into_iter()
        .map(|evidence| evidence.item)
        .collect::<Vec<_>>();
    if !evidence_items.is_empty() {
        let mut matched = Vec::new();
        for block in graph.blocks() {
            for head in core::queue_component_live_heads(content, block.component) {
                if evidence_items.iter().any(|item| {
                    agent_doc_queue::queue_claim::completion_evidence_matches_row(item, &head)
                }) {
                    matched.push(head);
                }
            }
        }
        claimed = claimed.with_heads(&matched);
    }
    let after_deps = agent_doc_queue::backlog_sync::collect_after_deps(&components, content);
    let mut combined = agent_doc_queue::subagent_intent::QueueSubagentPlan::default();
    let mut saw_attr = false;
    for block in graph.blocks() {
        let predecessor_live = block.dependencies.iter().any(|dependency| {
            graph.block(dependency).is_some_and(|predecessor| {
                predecessor.state() != agent_doc_queue::queue_set::QueueNodeState::Empty
            })
        });
        if predecessor_live {
            continue;
        }
        let Some((mode, eligible, live)) =
            core::queue_component_subagent_heads(content, block.component, prompt_presets)
        else {
            // Inline owner work gates later queues. A paused or empty inline
            // queue does not.
            if block.state_excluding_claimed(&claimed)
                == agent_doc_queue::queue_set::QueueNodeState::Ready
            {
                break;
            }
            continue;
        };
        // A drained `subagents` queue must not suppress the per-line path on
        // a later queue that has no attribute of its own.
        if block.state() == agent_doc_queue::queue_set::QueueNodeState::Empty {
            continue;
        }
        saw_attr = true;
        let plan = agent_doc_queue::subagent_intent::plan_queue_subagent_dispatch(
            mode,
            &eligible,
            &live,
            &claimed,
            &after_deps,
        );
        combined.dispatch.extend(plan.dispatch);
        combined.held.extend(plan.held);
    }
    saw_attr.then_some(combined)
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

/// Queue heads owned outside the in-session drain loop.
///
/// This single derived projection combines in-flight worker claims with both
/// dispatchable and concurrency-held queue-level subagent heads. Reported
/// worker results are deliberately absent whether the attempt remains claimed
/// or has a report-release receipt: the worker is finished and the coordinator
/// must select the row for integration/writeback (`#reportedwritebackdebt`).
pub fn out_of_loop_items_for_content(
    file: &Path,
    content: &str,
) -> agent_doc_queue::queue_claim::ClaimedQueueItems {
    let claimed = crate::queue_claim::in_flight_claimed_items_for_content(file, content);
    let dispatch = pending_subagent_dispatch_or_warn(file, content);
    let reported = crate::claim_release_receipt::reported_items_for_content(file, content);
    let held = queue_attr_subagent_plan(file, content)
        .map(|plan| plan.held)
        .unwrap_or_default()
        .into_iter()
        .filter(|head| !reported.claims(head))
        .collect::<Vec<_>>();
    claimed.with_heads(&dispatch).with_heads(&held)
}

/// Live heads a queue-level `subagents` / `fan-out` attribute (or subagent
/// intent) owns for dispatch that no worker attempt backs
/// (`#subagentinlineveto`): the dispatchable and concurrency-held heads of
/// [`out_of_loop_items_for_content`], minus every head with a claim attempt
/// (any state, expired included), a reported-release receipt, or a binary
/// completion cell. An inline answer never closes one of these; the
/// answered-free-text strike keeps it queued and reports it.
pub fn inline_answer_vetoed_heads_for_content(file: &Path, content: &str) -> Vec<String> {
    let mut owned = pending_subagent_dispatch_or_warn(file, content);
    if let Some(plan) = queue_attr_subagent_plan(file, content) {
        owned.extend(plan.held);
    }
    if owned.is_empty() {
        return owned;
    }
    let backed = agent_doc_queue::queue_claim::ClaimedQueueItems::none()
        .with_heads(&crate::queue_claim::claim_attempt_head_texts_for_content(
            file, content,
        ))
        .with_heads(&crate::claim_release_receipt::reported_head_texts_for_content(file, content))
        .with_heads(&crate::queue_claim::completion_evidence_head_texts(content));
    let mut seen = agent_doc_queue::queue_claim::ClaimedQueueItems::none();
    let mut vetoed = Vec::new();
    for head in owned {
        if backed.claims(&head) || seen.claims(&head) {
            continue;
        }
        seen = seen.with_heads(std::iter::once(&head));
        vetoed.push(head);
    }
    vetoed
}

/// The exact `agent-doc queue claim` command for dispatching `item`.
pub fn claim_command(document: &str, item: &str) -> String {
    core::claim_command_for(document, item)
}

/// The `agent-doc queue start` template run immediately after a successful
/// harness spawn binds its returned worker identity to the claim generation.
pub fn start_command(document: &str, item: &str) -> String {
    core::start_command_for(document, item)
}

/// The `agent-doc queue brief` command that prints `item`'s subagent
/// authorization preamble (`#waypostauthorization`).
pub fn brief_command(document: &str, item: &str) -> String {
    core::brief_command_for(document, item)
}

/// The operator authorization for dispatch item `item` of `content`
/// (`#waypostauthorization`), as carried in `queue_subagent_dispatch`.
pub fn subagent_authorization_for_content(
    file: &Path,
    content: &str,
    item: &str,
) -> Result<agent_doc_queue::subagent_brief::SubagentAuthorization> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    let components = agent_doc_element::element::parse(content).unwrap_or_default();
    let _guard = crate::queue_claim::steer_guard_for(file, content);
    let submitted = agent_doc_queue::queue_response::submitted_queue_prompt(content, item);
    Ok(
        agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
            &file.display().to_string(),
            content,
            item,
            &components,
            &prompt_presets,
            Some(submitted.as_str()),
        ),
    )
}

/// [`subagent_authorization_for_content`] with a caller-resolved `#nestsubmit`
/// prompt. A dispatch batch passes each head's own parent lineage.
pub fn subagent_authorization_for_content_submitted(
    file: &Path,
    content: &str,
    item: &str,
    submitted_prompt: &str,
) -> Result<agent_doc_queue::subagent_brief::SubagentAuthorization> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    let components = agent_doc_element::element::parse(content).unwrap_or_default();
    Ok(
        agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
            &file.display().to_string(),
            content,
            item,
            &components,
            &prompt_presets,
            Some(submitted_prompt),
        ),
    )
}

/// Folded resolve missed. A `#nestedclaimsteer` sibling is a live head only
/// while the claim-ledger view is installed, so try that after the folded miss.
fn expanded_claim_head(file: &Path, item: &str, content: &str) -> Option<String> {
    let _guard = crate::queue_claim::steer_guard_for(file, content)?;
    let heads =
        agent_doc_queue::queue_continuation::live_queue_head_texts(content).unwrap_or_default();
    agent_doc_queue::queue_claim::resolve_claim_target(item, &heads).ok()
}

fn plain_or_expanded_claim_head(file: &Path, item: &str, content: &str) -> Option<String> {
    let heads =
        agent_doc_queue::queue_continuation::live_queue_head_texts(content).unwrap_or_default();
    agent_doc_queue::queue_claim::resolve_claim_target(item, &heads)
        .ok()
        .or_else(|| expanded_claim_head(file, item, content))
}

/// `agent-doc queue brief`: resolve `item` (`#id` / `do [#id]` / the line's
/// text) to the live queue head it names, on disk or, while an editor save is
/// deferred, in the editor authority's text (the view preflight dispatched
/// from), and build that head's authorization.
pub fn subagent_authorization_for_item(
    file: &Path,
    item: &str,
) -> Result<agent_doc_queue::subagent_brief::SubagentAuthorization> {
    if item.trim().is_empty() {
        anyhow::bail!("queue brief needs a non-empty --item (`#id` or the queue line's text)");
    }
    let disk = std::fs::read_to_string(file)
        .with_context(|| format!("read {} to brief queue item {item:?}", file.display()))?;
    let disk_heads =
        agent_doc_queue::queue_continuation::live_queue_head_texts(&disk).unwrap_or_default();
    let (content, head) =
        match agent_doc_queue::queue_claim::resolve_claim_target(item, &disk_heads) {
            Ok(head) => (disk, head),
            Err(miss) if miss.ambiguous.is_empty() => {
                if let Some(head) = expanded_claim_head(file, item, &disk) {
                    (disk, head)
                } else if let Some(authority) =
                    crate::queue_claim::editor_authority_text(file, "queue_brief")
                {
                    let Some(head) = plain_or_expanded_claim_head(file, item, &authority) else {
                        return Err(miss.into());
                    };
                    (authority, head)
                } else {
                    return Err(miss.into());
                }
            }
            Err(miss) => return Err(miss.into()),
        };
    let head = agent_doc_document::queue_projection::strip_priority_markers(&head)
        .trim()
        .to_string();
    let _guard = crate::queue_claim::steer_guard_for(file, &content);
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(&content, file)?;
    let components = agent_doc_element::element::parse(&content).unwrap_or_default();
    let submitted = agent_doc_queue::queue_response::submitted_queue_prompt(&content, &head);
    Ok(
        agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
            &file.display().to_string(),
            &content,
            &head,
            &components,
            &prompt_presets,
            Some(submitted.as_str()),
        ),
    )
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

    #[test]
    fn ancestor_project_preset_uses_nested_project_overrides_without_local_copy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/config.toml"), "").unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#delegate': 'run #project item in a subagent'\n  '#project': default\n---\n",
        )
        .unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        std::fs::write(nested.join(".agent-doc/config.toml"), "").unwrap();
        std::fs::write(
            nested.join(".agent-doc/project.md"),
            "---\npresets:\n  '#project': agent-doc\n---\n",
        )
        .unwrap();
        let file = nested.join("task.md");
        let content = doc(&["#delegate do [#work]"]);
        std::fs::write(&file, &content).unwrap();

        let listed = pending_subagent_dispatch_for_content(&file, &content).unwrap();
        assert_eq!(listed, vec!["#delegate do [#work]".to_string()]);
        let authorization =
            subagent_authorization_for_content(&file, &content, "#delegate do [#work]").unwrap();
        assert_eq!(authorization.presets.len(), 1);
        assert_eq!(authorization.presets[0].name, "#delegate");
        assert_eq!(
            authorization.presets[0].body.as_deref(),
            Some("run agent-doc item in a subagent")
        );
        assert!(
            !std::fs::read_to_string(&file)
                .unwrap()
                .contains("run agent-doc item in a subagent")
        );
    }

    #[test]
    fn invalid_nested_project_preset_fails_authorization_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/config.toml"), "").unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#delegate': 'run {{#missing}} item'\n---\n",
        )
        .unwrap();
        let file = dir.path().join("task.md");
        let content = doc(&["#delegate do [#work]"]);
        std::fs::write(&file, &content).unwrap();

        let err = subagent_authorization_for_content(&file, &content, "#delegate do [#work]")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("unknown nested prompt preset reference #missing"),
            "{err:#}"
        );
    }

    /// `#claimdispatchidentity` (live 2026-10-03, agent-doc-bugs2): the
    /// operator edited the claimed `#subagents: <url>` into
    /// `#subagents: #gh-fix <url>` while its subagent worked, and the line
    /// re-appeared in `queue_subagent_dispatch` as new and unclaimed. The
    /// claim must follow the edit, including an in-place URL edit
    /// (`#claimfollowedit`); a line added elsewhere must not inherit it.
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
        crate::queue_claim::start_attempt(&file, &handle, "subagent:gh120", "worker:gh120", 600, 1)
            .unwrap();
        crate::queue_claim::refresh(&file, &handle, "subagent:gh120", 600).unwrap();

        // `#claimfollowedit`: an in-place edit of the claimed line's URL is
        // still the claimed line. The claim follows it, and steering forwards
        // the edit to the owner instead of offering a second worker.
        let in_place = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/121";
        let edited_in_place = doc(&["current task", in_place]);
        std::fs::write(&file, &edited_in_place).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &edited_in_place)
                .unwrap()
                .is_empty(),
            "an in-place edit of a claimed line is never new dispatch work"
        );
        assert!(
            crate::queue_claim::claimed_items_for_content(&file, &edited_in_place).claims(in_place)
        );
        assert_eq!(
            crate::queue_claim::prune_closed_claims(&file, &edited_in_place).unwrap(),
            0
        );

        // Deleting the claimed line and adding another issue elsewhere is a
        // closed line plus new work: the claim is released, the new line is
        // offered.
        let other = "#subagents: #gh-fix https://github.com/btakita/agent-doc/issues/122";
        let replaced = doc(&[other, "current task"]);
        std::fs::write(&file, &replaced).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &replaced).unwrap(),
            vec![other.to_string()],
            "a line added elsewhere is a different task: unclaimed, offered"
        );
        assert!(!crate::queue_claim::claimed_items_for_content(&file, &replaced).claims(other));
        assert_eq!(
            crate::queue_claim::prune_closed_claims(&file, &replaced).unwrap(),
            1
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

    /// `#queueblocked` (haiven-dev `tasks/sdk/sdk-demo.md`, 2026-10-27):
    /// `queue_subagent_dispatch` re-offered every blocked head each cycle. A
    /// head whose tracked item waits on an open blocker is neither dispatched
    /// nor allowed to fill a `subagents=N` slot, so the cap goes to runnable
    /// work; it is offered again once its blocker closes.
    #[test]
    fn queue_attr_dispatch_never_offers_a_blocked_head() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = format!(
            "{}\n<!-- agent:todo -->\n- [ ] [#deploy] pick a deploy path\n<!-- /agent:todo -->\n\n\
             <!-- agent:backlog -->\n\
             - [ ] [#sbx] provision (stays open: blocked behind #deploy)\n\
             - [ ] [#stg] repeat in STG (stays open: blocked behind #sbx)\n\
             - [ ] [#a] runnable\n\
             - [ ] [#b] runnable\n\
             <!-- /agent:backlog -->\n",
            attr_doc(
                "subagents=1",
                &["⏭\u{fe0f} do [#stg]", "📥 do [#sbx]", "do [#a]", "do [#b]"],
            )
        );
        std::fs::write(&file, &content).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["do [#a]".to_string()],
            "the single slot goes to the first runnable head"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::drainable_head_count(&content),
            2,
            "blocked heads are not drainable"
        );

        let unblocked = content.replace("- [ ] [#deploy]", "- [x] [#deploy]");
        std::fs::write(&file, &unblocked).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &unblocked).unwrap(),
            vec!["do [#sbx]".to_string()],
            "#sbx returns once its blocker closes; #stg still waits on #sbx"
        );
    }

    #[test]
    fn bare_queue_attr_dispatches_all_eligible_heads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = attr_doc(
            "subagents",
            &[
                "do [#a]",
                "do [#b]",
                "do [#c] [inline]",
                "do [#d]",
                "do [#e]",
            ],
        );
        std::fs::write(&file, &content).unwrap();

        let expected = vec![
            "do [#a]".to_string(),
            "do [#b]".to_string(),
            "do [#d]".to_string(),
            "do [#e]".to_string(),
        ];
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            expected
        );
        let plan = queue_attr_subagent_plan(&file, &content).unwrap();
        assert!(plan.held.is_empty(), "a bare attr has no capacity hold");
    }

    fn two_runs(main_attr: &str, main_body: &str, prs_attr: &str, prs_body: &str) -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             ## Run\n\n<!-- agent:run id=main {main_attr} go -->\n{main_body}\
             <!-- /agent:run -->\n\n\
             ## Run PRs\n\n<!-- agent:run id=prs {prs_attr} go -->\n{prs_body}\
             <!-- /agent:run -->\n"
        )
    }

    /// A claimed earlier `agent:run subagents` queue stays the claim-blind
    /// selection. A nested row under `do [#id]` is its own live head (the
    /// directive is not a free-text parent). Neither that head nor the claim
    /// hides a later independent `subagents` queue. Each queue keeps its own cap.
    #[test]
    fn claimed_subagents_queue_does_not_hide_a_later_independent_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = two_runs(
            "subagents",
            "- do [#owned]\n- do [#still-open]\n- do [#parent]\n  - merge-794-then-requeue-759\n",
            "subagents=1",
            "- https://github.com/haiven-dev/haiven-api/pull/759\n\
             - https://github.com/haiven-dev/haiven-api/pull/763\n",
        );
        std::fs::write(&file, &content).unwrap();
        crate::queue_claim::claim(&file, "do [#owned]", "subagent:owned", 600).unwrap();

        let listed = pending_subagent_dispatch_for_content(&file, &content).unwrap();
        assert!(
            listed.iter().any(|head| head.contains("do [#still-open]")),
            "an unclaimed head on the claimed queue stays dispatchable: {listed:?}"
        );
        assert!(
            listed
                .iter()
                .any(|head| head == "merge-794-then-requeue-759"),
            "a nested row under do [#id] is its own unclaimed head: {listed:?}"
        );
        assert!(
            listed
                .iter()
                .any(|head| head.contains("haiven-api/pull/759")),
            "the later run's first head is offered with the nested row: {listed:?}"
        );
        assert!(
            listed
                .iter()
                .all(|head| !head.contains("haiven-api/pull/763")),
            "the later queue's own cap holds its second head: {listed:?}"
        );
        assert!(
            listed.iter().all(|head| !head.contains("do [#owned]")),
            "the claimed head stays off the list: {listed:?}"
        );
        let plan = queue_attr_subagent_plan(&file, &content).unwrap();
        assert!(
            plan.held
                .iter()
                .any(|head| head.contains("haiven-api/pull/763")),
            "the held head stays on its own queue's plan: {plan:?}"
        );
    }

    #[test]
    fn sibling_subagents_queues_keep_independent_caps() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = two_runs(
            "subagents=1",
            "- do [#a]\n- do [#b]\n",
            "subagents=1",
            "- do [#c]\n- do [#d]\n",
        );
        std::fs::write(&file, &content).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["do [#a]".to_string(), "do [#c]".to_string()]
        );
        let plan = queue_attr_subagent_plan(&file, &content).unwrap();
        assert_eq!(
            plan.held,
            vec!["do [#b]".to_string(), "do [#d]".to_string()]
        );
    }

    #[test]
    fn after_dependency_blocks_while_a_claimed_predecessor_is_live() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let waiting = two_runs(
            "subagents",
            "- do [#owned]\n",
            "after=main subagents",
            "- https://github.com/haiven-dev/haiven-api/pull/759\n",
        );
        std::fs::write(&file, &waiting).unwrap();
        crate::queue_claim::claim(&file, "do [#owned]", "subagent:owned", 600).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &waiting)
                .unwrap()
                .is_empty(),
            "a claimed predecessor is not empty, so the dependent run waits"
        );

        let drained = two_runs(
            "subagents",
            "~~- do [#owned]~~\n",
            "after=main subagents",
            "- https://github.com/haiven-dev/haiven-api/pull/759\n",
        );
        std::fs::write(&file, &drained).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &drained).unwrap(),
            vec!["https://github.com/haiven-dev/haiven-api/pull/759".to_string()]
        );
    }

    #[test]
    fn inline_ready_queue_gates_a_later_subagents_queue() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = two_runs(
            "",
            "- finish the inline work\n",
            "subagents",
            "- https://github.com/haiven-dev/haiven-api/pull/759\n",
        );
        std::fs::write(&file, &content).unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty(),
            "owner-available inline work gates the later subagents queue"
        );

        crate::queue_claim::claim(&file, "finish the inline work", "subagent:inline", 600).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["https://github.com/haiven-dev/haiven-api/pull/759".to_string()],
            "a paused inline queue does not hide the later subagents queue"
        );
    }

    #[test]
    fn drained_subagents_queue_leaves_the_per_line_path_for_a_later_queue() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = two_runs(
            "subagents",
            "~~- shipped~~\n",
            "",
            "- #subagents do [#fresh]\n",
        );
        std::fs::write(&file, &content).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["#subagents do [#fresh]".to_string()]
        );
    }

    /// `#reportedrelease` (haiven-dev tasks/docs.md, 2026-10-15): under the
    /// queue `subagents` attribute, releasing a reported worker's claim put
    /// the answered line straight back into the dispatch set, and the
    /// coordinator dispatched a second worker for it. A reported release
    /// keeps the line out of dispatch (the coordinator persists it); a
    /// `--requeue` release returns it to dispatch.
    #[test]
    fn reported_release_is_not_redispatched_but_a_requeue_is() {
        use crate::claim_release_receipt::{ReleaseKind, release};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let question = "Does the page status enum have an archived value?";
        let content = attr_doc("subagents", &[question, "other question"]);
        std::fs::write(&file, &content).unwrap();
        let both = vec![question.to_string(), "other question".to_string()];
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            both
        );

        crate::queue_claim::claim(&file, question, "subagent:page-status-enum", 3600).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            question,
            "subagent:page-status-enum",
            "worker:page-status-enum",
            3600,
            1,
        )
        .unwrap();
        release(&file, question, ReleaseKind::Reported).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["other question".to_string()],
            "a reported line must not be offered to a new worker"
        );
        let excluded = crate::queue_claim::claimed_items_for_content(&file, &content);
        assert!(
            !excluded.claims(question),
            "the reported line stays drainable for the coordinator to persist"
        );

        crate::queue_claim::claim(&file, question, "subagent:retry", 3600).unwrap();
        release(&file, question, ReleaseKind::Requeue).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            both,
            "a requeued line is dispatchable again"
        );
    }

    /// `#waypostauthorization`: the dispatch entry's authorization and the
    /// `queue brief` lookup agree, resolve the queue preset and the backlog
    /// text of an id head, and fail closed on a missing preset.
    #[test]
    fn queue_brief_resolves_preset_and_backlog_text_and_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = |preset: &str| {
            format!(
                "---\nsession: sid\nagent_doc_format: template\nprompt_presets:\n  \"#ship\": \"commit + push\"\n---\n\n\
                 <!-- agent:backlog -->\n- [ ] [#a] add the thing\n<!-- /agent:backlog -->\n\n\
                 <!-- agent:queue subagents preset=\"{preset}\" go -->\n- do [#a]\n- tidy docs\n<!-- /agent:queue -->\n"
            )
        };
        let resolved = content("#ship");
        std::fs::write(&file, &resolved).unwrap();
        let dispatch = pending_subagent_dispatch_for_content(&file, &resolved).unwrap();
        assert_eq!(
            dispatch,
            vec!["do [#a]".to_string(), "tidy docs".to_string()]
        );
        let from_dispatch =
            subagent_authorization_for_content(&file, &resolved, &dispatch[0]).unwrap();
        let from_brief = subagent_authorization_for_item(&file, "#a").unwrap();
        assert_eq!(from_dispatch, from_brief);
        assert_eq!(
            from_brief.status,
            agent_doc_queue::subagent_brief::AuthorizationStatus::Resolved
        );
        assert_eq!(from_brief.tracked_items[0].text, "add the thing");
        assert_eq!(from_brief.presets[0].body.as_deref(), Some("commit + push"));
        let free = subagent_authorization_for_item(&file, "tidy docs").unwrap();
        assert!(free.tracked_items.is_empty());
        assert!(brief_command("task.md", "do [#a]").ends_with("--item '#a'"));

        let missing = content("#nosuch");
        std::fs::write(&file, &missing).unwrap();
        let brief = subagent_authorization_for_item(&file, "#a").unwrap();
        assert_eq!(
            brief.status,
            agent_doc_queue::subagent_brief::AuthorizationStatus::UnresolvedPreset
        );
        assert_eq!(brief.presets[0].body, None);
        assert!(
            brief
                .subagent_prompt_preamble
                .contains(agent_doc_queue::subagent_brief::UNRESOLVED_AUTHORIZATION_MARKER)
        );
        assert!(!brief.subagent_prompt_preamble.contains("commit + push"));
        // The preflight `queue_subagent_dispatch[].authorization` path and the
        // fail-closed preamble both carry the shared scratch-isolation rule.
        assert!(
            brief
                .subagent_prompt_preamble
                .contains(agent_doc_queue::subagent_brief::SUBAGENT_SCRATCH_ISOLATION_RULE)
        );
        assert!(subagent_authorization_for_item(&file, "#zzz").is_err());
    }

    #[test]
    fn queue_brief_preserves_quoted_operator_answer_evidence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let item = concat!(
            "In sdk-demo.md, no queue item status despite agent running: `- ",
            "[#subagent-operator-request-publish-ci-pr166]: **Commit and push to PR #166**`",
        );
        let content = attr_doc("subagents", &[item]);
        std::fs::write(&file, &content).unwrap();

        let brief = subagent_authorization_for_item(&file, item).unwrap();
        assert_eq!(brief.item, item);
        assert!(brief.subagent_prompt_preamble.contains(item));
    }

    #[test]
    fn out_of_loop_projection_holds_dispatchable_and_capacity_waiting_heads() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue subagents=1 go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&file, content).unwrap();

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, content).unwrap(),
            vec!["do [#a]".to_string()]
        );
        let excluded = out_of_loop_items_for_content(&file, content);
        assert!(excluded.claims("do [#a]"));
        assert!(excluded.claims("do [#b]"));
        assert!(!excluded.claims("not queued"));
    }
}
