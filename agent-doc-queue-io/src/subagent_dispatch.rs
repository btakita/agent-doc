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
//! queue (a reported release means the subagent finished and the in-session
//! loop now closes the item). A retained `Lost` attempt is offered again only
//! when its exact lineage proves it never reached worker attachment. A started
//! or provenance-unknown loss stays reconciliation debt.
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

fn executable_row_keys(content: &str) -> Vec<(String, String)> {
    agent_doc_queue::queue_continuation::all_executable_row_keys(content).unwrap_or_default()
}

/// The string `queue_subagent_dispatch` lists for a lineage claim key.
///
/// A unique leaf is listed as that leaf. The same leaf under two parents stays
/// the nest vector, so the two heads do not collapse (`#nestlistlineage`).
/// Claim identity and the submitted prompt stay the vector either way.
fn visible_dispatch_head(keys: &[(String, String)], lineage_head: &str) -> String {
    let Some((display, _)) = keys.iter().find(|(_, lineage)| lineage == lineage_head) else {
        return lineage_head.to_string();
    };
    let unique = keys.iter().filter(|(other, _)| other == display).count() == 1;
    if unique {
        display.clone()
    } else {
        lineage_head.to_string()
    }
}

/// Lineage claim key for a dispatch string, which may already be that key or
/// the unique leaf `visible_dispatch_head` listed.
fn lineage_hold_for_head(keys: &[(String, String)], head: &str) -> String {
    if let Some((_, lineage)) = keys.iter().find(|(_, lineage)| lineage == head) {
        return lineage.clone();
    }
    let mut matched = keys.iter().filter(|(display, _)| display == head);
    match (matched.next(), matched.next()) {
        (Some((_, lineage)), None) => lineage.clone(),
        _ => head.to_string(),
    }
}

/// Live, unclaimed queue lines with subagent intent that are new since the
/// previous cycle's seed, in queue order.
fn subagent_dispatch_candidates_for_content(file: &Path, content: &str) -> Result<Vec<String>> {
    let _guard = crate::queue_claim::steer_guard_for(file, content);
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    // `#reportedrelease`: a line whose worker already reported (its claim was
    // released with a receipt) is the coordinator's to persist, never a new
    // worker's: offering it again dispatched a duplicate subagent for an
    // answered question (haiven-dev tasks/docs.md, 2026-10-15).
    let reported = crate::claim_release_receipt::reported_items_for_content(file, content);
    // `#waitingitemclaimmarker`: an identity whose newest attempt was refused
    // admission for a deterministic projection blocker is held, never
    // re-offered: the same row would be refused again, every cycle.
    let refused = admission_refused_holds(file);
    let keys = executable_row_keys(content);
    if let Some(plan) = queue_attr_subagent_plan_with_presets(file, content, &prompt_presets)? {
        return Ok(plan
            .dispatch
            .into_iter()
            .filter(|head| !reported.claims(head) && !refused.holds(file, head))
            .map(|head| visible_dispatch_head(&keys, &head))
            .collect());
    }
    let reference = reference_queue_for_dispatch(file)?;
    let fresh =
        core::subagent_dispatch_heads_with_presets(content, reference.as_deref(), &prompt_presets);
    let fresh_identities = fresh
        .iter()
        .map(|head| agent_doc_queue::queue_claim::claim_identity(head))
        .collect::<Vec<_>>();
    let lost = crate::queue_claim::claim_states_for_content(file, content)?
        .into_iter()
        .filter(|claim| claim.state == agent_doc_queue::queue_claim::QueueClaimState::Lost)
        .collect::<Vec<_>>();
    // `#subagentworkpool`: an attempt that expired with its worker still bound
    // and no recorded stop may still be working, so its row is never offered.
    let unconfirmed = lost
        .iter()
        .filter(|claim| claim.worker.is_some() && claim.frozen.is_none())
        .map(|claim| claim.identity.clone())
        .collect::<Vec<_>>();
    let lineage = crate::queue_lineage::load(file)?;
    let lost_identities = lost
        .into_iter()
        .filter(|claim| {
            crate::queue_lineage::attempt_never_started(&lineage, claim)
        })
        .map(|claim| claim.identity)
        .chain(crate::queue_lineage::latest_unstarted_identities(&lineage))
        .collect::<Vec<_>>();
    if fresh_identities.is_empty() && lost_identities.is_empty() {
        return Ok(Vec::new());
    }
    // Start from the full eligible list so a recovered old row keeps document
    // order relative to genuinely new rows. Identity comparison also preserves
    // nested lineage semantics and de-duplicates a row that is both new and
    // backed by a retained Lost attempt.
    let heads = core::subagent_dispatch_heads_with_presets(content, None, &prompt_presets)
        .into_iter()
        .filter(|head| {
            let identity = agent_doc_queue::queue_claim::claim_identity(head);
            !unconfirmed.contains(&identity)
                && (fresh_identities.iter().any(|fresh| fresh == &identity)
                    || lost_identities.iter().any(|lost| lost == &identity))
        });
    let claimed = crate::queue_claim::claimed_items_for_content(file, content);
    Ok(heads
        .filter(|head| {
            !claimed.claims(head) && !reported.claims(head) && !refused.holds(file, head)
        })
        .map(|head| visible_dispatch_head(&keys, &head))
        .collect())
}

/// Identities held from subagent dispatch by a refused claim admission.
struct AdmissionRefusedHolds(Vec<(agent_doc_element_queue::QueueItemIdentity, String)>);

impl AdmissionRefusedHolds {
    fn holds(&self, file: &Path, head: &str) -> bool {
        let identity = agent_doc_queue::queue_claim::claim_identity(head);
        let Some((_, blocker)) = self.0.iter().find(|(held, _)| held == &identity) else {
            return false;
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_subagent_dispatch_held reason=admission_refused blocker={blocker} item_bytes={} (#waitingitemclaimmarker)",
                head.len()
            ),
        );
        true
    }
}

/// Lineage keys of the live executable rows held by a refused claim admission
/// (`#waitingitemclaimmarker`). They stay out of the in-session loop, the
/// inline-answer path, and the Stop-hook continuation exactly like a
/// concurrency-held head: the coordinator reports the named blocker once
/// instead of claiming, refusing, and re-claiming every cycle.
pub fn admission_refused_heads_for_content(file: &Path, content: &str) -> Vec<String> {
    let refused = admission_refused_holds(file);
    if refused.0.is_empty() {
        return Vec::new();
    }
    executable_row_keys(content)
        .into_iter()
        .map(|(_, lineage)| lineage)
        .filter(|lineage| {
            let identity = agent_doc_queue::queue_claim::claim_identity(lineage);
            refused.0.iter().any(|(held, _)| held == &identity)
        })
        .collect()
}

/// An unreadable lineage ledger is reported and holds nothing: the failure
/// direction is the pre-hold behaviour, never a stranded queue.
fn admission_refused_holds(file: &Path) -> AdmissionRefusedHolds {
    match crate::queue_lineage::load(file) {
        Ok(ledger) => {
            AdmissionRefusedHolds(crate::queue_lineage::admission_refused_identities(&ledger))
        }
        Err(err) => {
            eprintln!(
                "[queue] WARNING: could not read queue lineage for {}; refused claim admissions are not held from dispatch: {err:#}",
                file.display()
            );
            AdmissionRefusedHolds(Vec::new())
        }
    }
}

pub fn reported_writeback_pending_for_content(file: &Path, content: &str) -> Result<bool> {
    let active = crate::queue_claim::claim_states_for_content(file, content)?
        .into_iter()
        .any(|claim| claim.state == agent_doc_queue::queue_claim::QueueClaimState::Reported);
    Ok(active
        || !crate::claim_release_receipt::reported_items_for_content(file, content).is_empty())
}

pub fn pending_subagent_dispatch_for_content(file: &Path, content: &str) -> Result<Vec<String>> {
    // A reported attempt is integration debt for that exact row, not a
    // document-wide dispatch lock. `subagent_dispatch_candidates_for_content`
    // already excludes rows held by active claims and durable reported
    // receipts, so unrelated ready siblings remain safe to start while the
    // coordinator reconciles the handback (`#queueprojectiondrift`).
    subagent_dispatch_candidates_for_content(file, content)
}

/// Document-wide coordinator boundary projection (`#coordinatordrain`).
///
/// This consumes the exact project-wide sync payload preflight emits after its
/// repair pass: current and related documents, every active/inactive run,
/// durable attempts, and fail-closed scan errors. Session-check and Stop must
/// not infer completion from one selected prompt or one active run tree.
pub fn coordinator_drain_decision_for_content(
    file: &Path,
    content: &str,
) -> Result<agent_doc_queue::coordinator_drain::CoordinatorDrainDecision> {
    let previous = agent_doc_snapshot_io::load_document_baseline(file)?;
    let snapshot =
        crate::sync_payload::build_project_queue_sync_payload(file, content, previous.as_deref())?;
    let scope = agent_doc_state_scope::DocumentScope::new();
    Ok(
        agent_doc_queue::coordinator_drain::CoordinatorDrainMachine::new_in(&scope)
            .observe(snapshot),
    )
}

/// Synchronous closeout hands queue reconciliation to the document supervisor.
///
/// The committed closeout event is the actor ingress that wakes queue work.
/// Stop/session-check must not rebuild the project payload, inspect worker
/// integration debt, or project queue state while the UI is waiting for the
/// response to persist. Returning the terminal identity here keeps that
/// ownership boundary typed at every synchronous consumer; the background
/// supervisor observes and drains the real coordinator state independently.
pub fn terminal_queue_drain_handoff() -> agent_doc_queue::coordinator_drain::CoordinatorDrainDecision
{
    agent_doc_queue::coordinator_drain::CoordinatorDrainDecision::default()
}

/// The queue-level subagents attribute (`<!-- agent:queue subagents=N -->`)
/// planned over every independent `subagents` queue against the raw claim
/// ledger: which heads to dispatch now and which to hold (an explicit `=N`
/// cap is full, or an item `after=` predecessor is still queued). A bare
/// attribute dispatches every eligible head of that queue. Sibling
/// `subagents` queues are offered in the same cycle; an `after=` queue waits
/// until every predecessor is empty, and an earlier inline queue that still
/// has owner-available work gates later queues. A paused earlier queue does
/// not: a `pause` control (a legacy marker `stop` included) leaves the other
/// runs in that turn. `None` when no non-empty queue has a valid attribute,
/// so the per-line "new since seed" path still runs. Invalid or near-miss
/// subagent ownership attributes return an error; they never mean inline mode.
pub fn queue_attr_subagent_plan(
    file: &Path,
    content: &str,
) -> Result<Option<agent_doc_queue::subagent_intent::QueueSubagentPlan>> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    queue_attr_subagent_plan_with_presets(file, content, &prompt_presets)
}

/// `#subagentsalwaysparallel`: subagent dispatch is enabled for this
/// document's queue work when any queue or run component (live or drained,
/// since the operator may steer a new prompt into it mid-turn) runs in
/// subagents at queue scope ([`core::queue_component_runs_in_subagents`]).
/// Item-level `#subagents` intent is the caller's dispatch list.
pub fn queue_subagent_dispatch_enabled_for_content(file: &Path, content: &str) -> Result<bool> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    let components = agent_doc_element::element::parse(content)?;
    let graph = agent_doc_queue::queue_set::QueueSet::parse(content, &components)?;
    Ok(graph.blocks().iter().any(|block| {
        core::queue_component_runs_in_subagents(content, block.component, &prompt_presets)
    }))
}

/// `#queueskipsubagents`: lowercased `#id`s of live queue rows routed to
/// subagent dispatch (never to the inline turn), resolved with this file's
/// effective prompt presets. See [`core::subagent_routed_queue_head_ids`].
pub fn subagent_routed_head_ids_for_content(
    file: &Path,
    content: &str,
) -> Result<std::collections::HashSet<String>> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    Ok(core::subagent_routed_queue_head_ids(content, &prompt_presets))
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
        .with_heads(&crate::queue_claim::completion_evidence_hold_texts(content))
}

fn queue_attr_subagent_plan_with_presets(
    file: &Path,
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Result<Option<agent_doc_queue::subagent_intent::QueueSubagentPlan>> {
    queue_attr_subagent_plan_with_pools(file, content, prompt_presets, None)
}

/// [`queue_attr_subagent_plan`] that also reports each dispatching queue's
/// work-pool occupancy (`#subagentworkpool`).
pub fn queue_attr_subagent_plan_and_pools(
    file: &Path,
    content: &str,
) -> Result<(
    Option<agent_doc_queue::subagent_intent::QueueSubagentPlan>,
    Vec<agent_doc_queue::subagent_pool::PoolStatus>,
)> {
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(content, file)?;
    let mut pools = Vec::new();
    let plan = queue_attr_subagent_plan_with_pools(file, content, &prompt_presets, Some(&mut pools))?;
    Ok((plan, pools))
}

fn queue_attr_subagent_plan_with_pools(
    file: &Path,
    content: &str,
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
    mut pools: Option<&mut Vec<agent_doc_queue::subagent_pool::PoolStatus>>,
) -> Result<Option<agent_doc_queue::subagent_intent::QueueSubagentPlan>> {
    let components = agent_doc_element::element::parse(content)?;
    let graph = agent_doc_queue::queue_set::QueueSet::parse(content, &components)?;
    // `#queueschedule`: a closed schedule window dispatches nothing.
    // Do not call `claimed_items_for_content` here: it folds this plan's held
    // heads back in and would recurse.
    let schedule_held = crate::queue_schedule::schedule_held_heads(file, content);
    let mut excluded = crate::queue_claim::ledger_claimed_items_for_content(file, content)
        .with_heads(&schedule_held)
        .with_heads(&crate::queue_claim::completion_evidence_hold_texts(content))
        .with_claimed(&crate::claim_release_receipt::reported_items_for_content(
            file, content,
        ));
    let claim_states = crate::queue_claim::claim_states_for_content(file, content)?;
    // `#subagentworkpool`: a dispatching claim and a live running worker hold
    // a pool slot; a frozen worker frees its slot but keeps its claim (it
    // stays in `excluded` through the ledger claims).
    let now = crate::queue_claim::claim_clock_now_secs();
    let frozen_secs = crate::subagent_pool::frozen_secs();
    let default_pool = crate::subagent_pool::default_pool_size();
    let occupied = agent_doc_queue::queue_claim::ClaimedQueueItems::from_identities(
        claim_states
            .iter()
            .filter(|claim| {
                agent_doc_queue::subagent_pool::occupies_pool_slot(claim, now, frozen_secs)
            })
            .map(|claim| claim.identity.clone()),
    );
    let lost = claim_states
        .iter()
        .filter(|claim| claim.state == agent_doc_queue::queue_claim::QueueClaimState::Lost)
        .cloned()
        .collect::<Vec<_>>();
    let lineage = (!lost.is_empty())
        .then(|| crate::queue_lineage::load(file))
        .transpose()?;
    let unsafe_lost = lost
        .into_iter()
        .filter(|claim| {
            !lineage
                .as_ref()
                .is_some_and(|lineage| crate::queue_lineage::attempt_never_started(lineage, claim))
        })
        .map(|claim| claim.item_text)
        .collect::<Vec<_>>();
    excluded = excluded.with_heads(&unsafe_lost);
    let evidence_items = agent_doc_document::subagent_completion::completion_evidence(content)
        .into_iter()
        .map(|evidence| evidence.item)
        .collect::<Vec<_>>();
    if !evidence_items.is_empty() {
        let mut matched = Vec::new();
        for block in graph.blocks() {
            let entries =
                agent_doc_queue::nested_claim_steer::expand_slice_if_installed(&block.entries);
            for index in
                agent_doc_queue::document_queue::executable_prompt_indexes(entries.as_ref())
            {
                let Some(lineage) =
                    agent_doc_queue::document_queue::lineage_claim_text(entries.as_ref(), index)
                else {
                    continue;
                };
                let agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) = &entries[index]
                else {
                    continue;
                };
                let raw = agent_doc_document::queue_projection::strip_in_progress_marker(
                    prompt.text.trim(),
                );
                let norm = core::normalize_queue_text(&raw);
                let hits = evidence_items.iter().any(|item| {
                    agent_doc_queue::queue_claim::completion_evidence_matches_row(item, &norm)
                        || (lineage != raw
                            && agent_doc_queue::queue_claim::completion_evidence_matches_row(
                                item, &lineage,
                            ))
                });
                if hits {
                    // A top-level row's hold is the normalized head. A nested
                    // non-tracked leaf is held as its nest vector, so one
                    // parent's `#rebase` does not cover another's.
                    matched.push(if lineage != raw { lineage } else { norm });
                }
            }
        }
        excluded = excluded.with_heads(&matched);
    }
    let after_deps = agent_doc_queue::backlog_sync::collect_after_deps(&components, content);
    let queue_control = agent_doc_queue::queue_continuation::document_queue_control(content);
    let mut document_facts: Option<core::QueueDispatchDocumentFacts> = None;
    let mut combined = agent_doc_queue::subagent_intent::QueueSubagentPlan::default();
    let mut saw_attr = false;
    for block in graph.blocks() {
        agent_doc_queue::subagent_intent::validate_queue_subagents_attrs(
            &block.component.attrs,
        )
        .map_err(anyhow::Error::msg)
        .with_context(|| {
            format!(
                "refusing to plan queue `{}` with ambiguous subagent ownership",
                block.id
            )
        })?;
        let predecessor_live = block.dependencies.iter().any(|dependency| {
            graph.block(dependency).is_some_and(|predecessor| {
                predecessor.state() != agent_doc_queue::queue_set::QueueNodeState::Empty
            })
        });
        if predecessor_live {
            continue;
        }
        let owner_state =
            block.state_excluding_claimed_with_control(&excluded, queue_control.as_deref());
        // `#preflightqueuescale`: the document-wide dispatch facts are derived
        // once per plan, not once per queue block.
        let heads = agent_doc_queue::subagent_intent::queue_subagents_mode(&block.component.attrs)
            .and_then(|_| {
                let facts = document_facts
                    .get_or_insert_with(|| core::QueueDispatchDocumentFacts::from_content(content));
                core::queue_component_subagent_heads_with_facts(
                    content,
                    block.component,
                    prompt_presets,
                    facts,
                )
            });
        let Some((mode, eligible, live)) = heads else {
            // Inline owner work gates later queues. A paused or empty inline
            // queue does not.
            if owner_state == agent_doc_queue::queue_set::QueueNodeState::Ready {
                break;
            }
            continue;
        };
        // A drained `subagents` queue must not suppress the per-line path on
        // a later queue that has no attribute of its own.
        if block.state() == agent_doc_queue::queue_set::QueueNodeState::Empty {
            continue;
        }
        // A control-paused queue stays live for `after=` and does not dispatch
        // its own heads. It still owns the attribute path, so those heads do
        // not fall through to per-line dispatch. Later runs are still offered.
        // Claimed, reported, and operator-gated heads keep the existing plan.
        if agent_doc_queue::control_binding::explicit_queue_pause_mode(
            &block.component.attrs,
            queue_control.as_deref(),
        ) {
            saw_attr = true;
            continue;
        }
        saw_attr = true;
        let pool = agent_doc_queue::subagent_pool::resolve_pool_size(mode.capacity, default_pool);
        if let Some(pools) = pools.as_deref_mut() {
            let live_identities = live
                .iter()
                .map(|head| agent_doc_queue::queue_claim::claim_identity(head))
                .collect::<Vec<_>>();
            pools.push(agent_doc_queue::subagent_pool::PoolStatus {
                queue_id: block.id.to_string(),
                occupancy: agent_doc_queue::subagent_pool::pool_occupancy(
                    pool,
                    claim_states
                        .iter()
                        .filter(|claim| live_identities.contains(&claim.identity)),
                    now,
                    frozen_secs,
                ),
            });
        }
        let plan = agent_doc_queue::subagent_intent::plan_queue_subagent_dispatch(
            mode,
            &eligible,
            &live,
            &excluded,
            &occupied,
            &after_deps,
            Some(default_pool.size),
        );
        combined.dispatch.extend(plan.dispatch);
        combined.held.extend(plan.held);
    }
    Ok(saw_attr.then_some(combined))
}

/// Every executable row in `content`, addressed by its lineage claim key.
/// This is the conservative hold set when a non-Result compatibility caller
/// encounters a malformed subagent ownership marker: no row may drain inline.
fn fail_closed_queue_heads(content: &str) -> Vec<String> {
    executable_row_keys(content)
        .into_iter()
        .map(|(_, lineage)| lineage)
        .collect()
}

/// [`pending_subagent_dispatch_for_content`] that reports a failure and emits
/// no spawn authorization. The out-of-loop ownership projection independently
/// holds every queue head on the same error, so none can drain inline.
pub fn pending_subagent_dispatch_or_warn(file: &Path, content: &str) -> Vec<String> {
    match pending_subagent_dispatch_for_content(file, content) {
        Ok(heads) => heads,
        Err(err) => {
            eprintln!(
                "[queue] ERROR: could not compute subagent dispatch intent for {}; no worker \
                 spawn is authorized and queue heads remain held: {err:#}",
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

fn subagent_dispatch_candidates_or_warn(file: &Path, content: &str) -> Vec<String> {
    match subagent_dispatch_candidates_for_content(file, content) {
        Ok(heads) => heads,
        Err(err) => {
            eprintln!(
                "[queue] ERROR: could not compute subagent dispatch ownership for {}; no worker \
                 spawn is authorized and queue heads remain held: {err:#}",
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
    let in_flight = crate::queue_claim::in_flight_claimed_items_for_content(file, content);
    let worker_owned = crate::queue_claim::worker_owned_items_for_content(file, content);
    let claimed = in_flight.with_claimed(&worker_owned);
    // Fresh candidates remain worker-owned and out of the inline drain loop
    // while the coordinator persists any independent Reported closeout.
    let dispatch = subagent_dispatch_candidates_or_warn(file, content);
    let reported = crate::claim_release_receipt::reported_items_for_content(file, content);
    let keys = executable_row_keys(content);
    let dispatch_holds: Vec<String> = dispatch
        .iter()
        .map(|head| lineage_hold_for_head(&keys, head))
        .collect();
    let held = match queue_attr_subagent_plan(file, content) {
        Ok(Some(plan)) => plan.held,
        Ok(None) => Vec::new(),
        Err(error) => {
            eprintln!(
                "[queue] ERROR: holding every queue head because subagent ownership planning failed for {}: {error:#}",
                file.display()
            );
            fail_closed_queue_heads(content)
        }
    }
        .into_iter()
        .filter(|head| !reported.claims(head))
        .collect::<Vec<_>>();
    claimed
        .with_claimed(&reported)
        .with_heads(&dispatch_holds)
        .with_heads(&held)
        .with_heads(&admission_refused_heads_for_content(file, content))
}

/// Live heads a queue-level `subagents` / `fan-out` attribute (or subagent
/// intent) owns for dispatch that no worker attempt backs
/// (`#subagentinlineveto`): the dispatchable and concurrency-held heads of
/// [`out_of_loop_items_for_content`], minus every head with a claim attempt
/// (any state, expired included), a reported-release receipt, or a binary
/// completion cell. An inline answer never closes one of these; the
/// answered-free-text strike keeps it queued and reports it.
pub fn inline_answer_vetoed_heads_for_content(file: &Path, content: &str) -> Vec<String> {
    let mut owned = subagent_dispatch_candidates_or_warn(file, content);
    owned.extend(admission_refused_heads_for_content(file, content));
    match queue_attr_subagent_plan(file, content) {
        Ok(Some(plan)) => owned.extend(plan.held),
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "[queue] ERROR: vetoing inline closeout because subagent ownership planning failed for {}: {error:#}",
                file.display()
            );
            owned.extend(fail_closed_queue_heads(content));
        }
    }
    if owned.is_empty() {
        return owned;
    }
    let keys = executable_row_keys(content);
    let backed = agent_doc_queue::queue_claim::ClaimedQueueItems::none()
        .with_heads(&crate::queue_claim::claim_attempt_head_texts_for_content(
            file, content,
        ))
        .with_heads(&crate::claim_release_receipt::reported_head_texts_for_content(file, content))
        .with_heads(&crate::queue_claim::completion_evidence_hold_texts(content));
    let mut seen = agent_doc_queue::queue_claim::ClaimedQueueItems::none();
    let mut vetoed = Vec::new();
    for head in owned {
        let hold = lineage_hold_for_head(&keys, &head);
        if backed.claims(&hold) || seen.claims(&hold) {
            continue;
        }
        seen = seen.with_heads(std::iter::once(&hold));
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
    let submitted =
        agent_doc_queue::queue_response::submitted_queue_prompt_with_context(content, item);
    let mut authorization = agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
        &file.display().to_string(),
        content,
        item,
        &components,
        &prompt_presets,
        Some(submitted.as_str()),
    );
    crate::goal_loop_handoff::attach_prior_attempt(file, item, &mut authorization);
    Ok(authorization)
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
    let mut authorization = agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
        &file.display().to_string(),
        content,
        item,
        &components,
        &prompt_presets,
        Some(submitted_prompt),
    );
    crate::goal_loop_handoff::attach_prior_attempt(file, item, &mut authorization);
    Ok(authorization)
}

/// Compatibility fallback for a sibling exposed from an older folded
/// `#nestedclaimsteer` entry.
fn expanded_claim_head(file: &Path, item: &str, content: &str) -> Option<String> {
    let _guard = crate::queue_claim::steer_guard_for(file, content)?;
    let heads =
        agent_doc_queue::queue_continuation::all_queue_head_texts(content).unwrap_or_default();
    agent_doc_queue::queue_claim::resolve_claim_target(item, &heads).ok()
}

/// `agent-doc queue brief`: resolve `item` (`#id` / `do [#id]` / the line's
/// text) to the live queue head it names in the editor authority's current cut,
/// or on disk while detached, and build that exact view's authorization.
pub fn subagent_authorization_for_item(
    file: &Path,
    item: &str,
) -> Result<agent_doc_queue::subagent_brief::SubagentAuthorization> {
    if item.trim().is_empty() {
        anyhow::bail!("queue brief needs a non-empty --item (`#id` or the queue line's text)");
    }
    let disk = std::fs::read_to_string(file)
        .with_context(|| format!("read {} to brief queue item {item:?}", file.display()))?;
    let authority = crate::queue_claim::editor_authority_text(file, "queue_brief");
    let (content, head) = resolve_brief_view(file, item, disk, authority)?;
    let head = agent_doc_document::queue_projection::strip_priority_markers(&head)
        .trim()
        .to_string();
    let _guard = crate::queue_claim::steer_guard_for(file, &content);
    let prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(&content, file)?;
    let components = agent_doc_element::element::parse(&content).unwrap_or_default();
    let submitted =
        agent_doc_queue::queue_response::submitted_queue_prompt_with_context(&content, &head);
    let mut authorization = agent_doc_queue::subagent_brief::subagent_authorization_with_presets(
        &file.display().to_string(),
        &content,
        &head,
        &components,
        &prompt_presets,
        Some(submitted.as_str()),
    );
    crate::goal_loop_handoff::attach_prior_attempt(file, &head, &mut authorization);
    Ok(authorization)
}

/// Resolve a brief against the same live-document boundary as `queue claim`:
/// an attached editor/CRDT cut is authoritative, and disk is authoritative
/// only while detached. Returning the chosen content with the head keeps queue
/// identity, presets, and submitted lineage on that one exact cut.
fn resolve_brief_view(
    file: &Path,
    item: &str,
    disk: String,
    authority: Option<String>,
) -> Result<(String, String)> {
    let content = authority.unwrap_or(disk);
    let heads =
        agent_doc_queue::queue_continuation::all_queue_head_texts(&content).unwrap_or_default();
    match agent_doc_queue::queue_claim::resolve_claim_target(item, &heads) {
        Ok(head) => Ok((content, head)),
        Err(miss) if miss.ambiguous.is_empty() => expanded_claim_head(file, item, &content)
            .map(|head| (content, head))
            .ok_or_else(|| miss.into()),
        Err(miss) => Err(miss.into()),
    }
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
    fn brief_uses_live_authority_queue_id_instead_of_stale_disk_kind_alias() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let item = "do [#canonical-origin]";
        let disk = format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             ## Run <!-- agent:run subagents -->\n- {item}\n<!-- /agent:run -->\n\n\
             ## Loop <!-- agent:loop interval=\"1h\" -->\n- watch\n<!-- /agent:loop -->\n"
        );
        let authority = disk.replace("agent:run subagents", "agent:run id=\"main\" subagents");
        std::fs::write(&file, &disk).unwrap();

        let (content, head) =
            resolve_brief_view(&file, item, disk, Some(authority.clone())).unwrap();
        assert_eq!(content, authority);
        assert_eq!(head, item);

        let authorization = subagent_authorization_for_content(&file, &content, &head).unwrap();
        assert_eq!(authorization.queue_id.as_deref(), Some("main"));
        assert!(
            authorization
                .subagent_prompt_preamble
                .contains("Origin queue id (dispatch-time provenance):\n> main")
        );
        assert!(
            authorization
                .subagent_prompt_preamble
                .contains("--id main --after <origin>")
        );
        assert!(
            !authorization
                .subagent_prompt_preamble
                .contains("--id run --after <origin>")
        );
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

    /// A running claim in the first run must not make pending rows in sibling
    /// or later runs disappear at the coordinator boundary.
    #[test]
    fn coordinator_drain_observes_running_and_inactive_run_pending_rows_together() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = r#"---
session: sid
agent_doc_format: template
queue: go
---

## Active <!-- agent:run id="active" subagents go -->
- do [#a]
<!-- /agent:run -->

## Other <!-- agent:run id="other" subagents -->
- do [#b]
<!-- /agent:run -->

## Inactive <!-- agent:run id="inactive" subagents -->
- do [#c]
<!-- /agent:run -->
"#;
        std::fs::write(&file, content).unwrap();
        let (_, claim) = crate::queue_claim::claim(&file, "#a", "subagent:a", 600).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            "#a",
            "subagent:a",
            "worker-a",
            600,
            claim.generation,
        )
        .unwrap();

        let decision = coordinator_drain_decision_for_content(&file, content).unwrap();
        assert_eq!(
            decision.phase,
            agent_doc_queue::coordinator_drain::CoordinatorDrainPhase::ActionRequired
        );
        assert_eq!(decision.snapshot.running.len(), 1);
        assert_eq!(
            decision
                .snapshot
                .pending
                .iter()
                .map(|item| (item.item.as_str(), item.queue_id.as_str()))
                .collect::<Vec<_>>(),
            vec![("do [#b]", "other"), ("do [#c]", "inactive")]
        );
        assert!(!decision.may_yield());
    }

    #[test]
    fn synchronous_terminal_handoff_never_scans_or_blocks_on_queue_debt() {
        let decision = terminal_queue_drain_handoff();

        assert!(decision.is_terminal());
        assert!(decision.snapshot.is_empty());
        let source = include_str!("subagent_dispatch.rs");
        let body = source
            .split("pub fn terminal_queue_drain_handoff()")
            .nth(1)
            .and_then(|tail| tail.split("\n}\n").next())
            .expect("terminal handoff function body");
        assert!(!body.contains("build_project_queue_sync_payload"));
        assert!(!body.contains("load_document_baseline"));
    }

    /// Exact `#inactive-run-stranding` repro from api.md: a pending nested
    /// rebase child in an inactive subagent run is still project action debt.
    #[test]
    fn coordinator_drain_observes_inactive_pr809_nested_pending_rebase() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("api.md");
        let content = concat!(
            "---\nagent_doc_session: api\n---\n\n",
            "<!-- agent:run id=active subagents go -->\n",
            "- 🚧 do [#active]\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:run id=api-prs subagents -->\n",
            "- https://github.com/haiven-dev/haiven-api/pull/809\n",
            "  - 📥 #rebase\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&file, content).unwrap();

        let decision = coordinator_drain_decision_for_content(&file, content).unwrap();
        assert_eq!(
            decision.phase,
            agent_doc_queue::coordinator_drain::CoordinatorDrainPhase::ActionRequired
        );
        let stranded = decision
            .snapshot
            .pending
            .iter()
            .find(|item| item.queue_id == "api-prs")
            .expect("inactive api-prs pending row must remain visible");
        assert_eq!(stranded.item, "📥 #rebase", "{stranded:?}");
        assert!(
            content.contains("- https://github.com/haiven-dev/haiven-api/pull/809\n  - 📥 #rebase"),
            "test must preserve the operator's exact nested repro"
        );
        assert_eq!(
            stranded.execution_mode,
            agent_doc_queue::queue_sync_payload::QueueExecutionMode::SubagentDispatch
        );
        assert!(!decision.may_yield());
    }

    /// `#losslessqueuerecovery`: expiry ends ownership but does not erase the
    /// durable worker attachment. A started attempt fails closed for recovery
    /// instead of being offered as replacement work.
    #[test]
    fn expired_started_nested_subagent_item_is_not_redispatched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let item = "re: [#grok-live-editor]: Also validate real-time steering parity";
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## Run\n\n<!-- agent:run subagents go -->\n",
            "- do [#grok-live-editor]\n",
            "  - re: [#grok-live-editor]: Also validate real-time steering parity\n",
            "<!-- /agent:run -->\n",
        )
        .to_string();
        std::fs::write(&file, &content).unwrap();
        seed(&file, "cycle-prev", &content);

        let pinned = 1_000_000;
        let _claimed_at = crate::queue_claim::pin_claim_clock_for_current_thread(pinned);
        let (_, prior) =
            crate::queue_claim::claim(&file, item, "subagent:rtsteeringparity", 10).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            item,
            "subagent:rtsteeringparity",
            "/root/rtsteeringparity",
            10,
            prior.generation,
        )
        .unwrap();
        assert!(
            pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty(),
            "an active prior worker remains fenced"
        );

        let _expired = crate::queue_claim::pin_claim_clock_for_current_thread(pinned + 10);
        assert!(
            pending_subagent_dispatch_for_content(&file, &content)
                .unwrap()
                .is_empty(),
            "a bound worker makes the lost attempt reconciliation debt, not retry work"
        );
    }

    #[test]
    fn abandoned_unstarted_per_line_item_bypasses_new_since_seed_gate() {
        use crate::claim_release_receipt::{ReleaseKind, release_attempt};

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let item = "#subagents retry unavailable worker";
        let content = doc(&[item]);
        std::fs::write(&file, &content).unwrap();
        seed(&file, "cycle-prev", &content);

        let (_, prior) = crate::queue_claim::claim(&file, item, "subagent:prior", 600).unwrap();
        release_attempt(&file, item, ReleaseKind::Requeue, Some(prior.generation)).unwrap();

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec![item.to_string()]
        );
    }

    /// `#nestlistlineage`: two live `#rebase` rows are two claims. A short
    /// leaf binds the next unclaimed parent and leaves every row in the
    /// document. One parent is still claimed by `#rebase`.
    #[test]
    fn shared_rebase_claim_binds_each_parent_and_keeps_every_row() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("api.md");
        let api = "https://github.com/haiven-dev/haiven-api/pull/767: #rebase";
        let contracts = "https://github.com/haiven-dev/haiven-contracts/pull/162: #rebase";
        let content = "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue subagents go -->\n- https://github.com/haiven-dev/haiven-api/pull/767\n  - #rebase\n- https://github.com/haiven-dev/haiven-contracts/pull/162\n  - #rebase\n<!-- /agent:queue -->\n"
            .to_string();
        std::fs::write(&file, &content).unwrap();
        let listed = pending_subagent_dispatch_for_content(&file, &content).unwrap();
        assert_eq!(listed, vec![api.to_string(), contracts.to_string()]);
        assert!(
            claim_command("api.md", &listed[0]).contains(&format!("--item '{api}'")),
            "{}",
            claim_command("api.md", &listed[0])
        );

        let (_, first) = crate::queue_claim::claim(&file, "#rebase", "subagent:a", 600).unwrap();
        assert_eq!(first.item_text, api);
        let (_, second) = crate::queue_claim::claim(&file, "#rebase", "subagent:b", 600).unwrap();
        assert_eq!(second.item_text, contracts);
        let err = crate::queue_claim::claim(&file, "#rebase", "subagent:c", 600).unwrap_err();
        assert!(
            err.to_string().contains("already held"),
            "a shared leaf is refused only once every parent is held: {err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);

        let unique_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(unique_dir.path().join(".agent-doc")).unwrap();
        let unique_file = unique_dir.path().join("api.md");
        let unique = "---\nsession: sid\nagent_doc_format: template\n---\n\n## Queue\n\n<!-- agent:queue subagents go -->\n- https://github.com/haiven-dev/haiven-api/pull/767\n  - #rebase\n<!-- /agent:queue -->\n"
            .to_string();
        std::fs::write(&unique_file, &unique).unwrap();
        let listed = pending_subagent_dispatch_for_content(&unique_file, &unique).unwrap();
        assert_eq!(listed, vec!["#rebase".to_string()]);
        let (_, claim) =
            crate::queue_claim::claim(&unique_file, "#rebase", "subagent:a", 600).unwrap();
        assert_eq!(claim.item_text, api);
        assert_eq!(std::fs::read_to_string(&unique_file).unwrap(), unique);

        let exact_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(exact_dir.path().join(".agent-doc")).unwrap();
        let exact_file = exact_dir.path().join("api.md");
        std::fs::write(&exact_file, &content).unwrap();
        let (_, exact) =
            crate::queue_claim::claim(&exact_file, contracts, "subagent:a", 600).unwrap();
        assert_eq!(exact.item_text, contracts);
        let (_, other) =
            crate::queue_claim::claim(&exact_file, "#rebase", "subagent:b", 600).unwrap();
        assert_eq!(other.item_text, api);
        assert_eq!(std::fs::read_to_string(&exact_file).unwrap(), content);

        let sibling = "---\nsession: sid\nagent_doc_format: template\n---\n\n## API\n\n<!-- agent:queue id=api-prs subagents go -->\n- https://github.com/haiven-dev/haiven-api/pull/767\n  - #rebase\n<!-- /agent:queue -->\n\n## Contracts\n\n<!-- agent:queue id=contracts-prs subagents -->\n- https://github.com/haiven-dev/haiven-contracts/pull/162\n  - #rebase\n<!-- /agent:queue -->\n"
            .to_string();
        let sibling_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(sibling_dir.path().join(".agent-doc")).unwrap();
        let sibling_file = sibling_dir.path().join("api.md");
        std::fs::write(&sibling_file, &sibling).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&sibling_file, &sibling).unwrap(),
            vec![api.to_string(), contracts.to_string()]
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

    /// `#queueprojectiondrift` (haiven-dev `tasks/cli.md`, 2026-11-01): one
    /// REVIEW-CLEAN worker was reported while another PR row was still ready.
    /// The reported handback remains visible integration debt, but it must not
    /// make the ready sibling disappear from the dispatch plan.
    #[test]
    fn reported_handoff_does_not_hide_an_unrelated_ready_sibling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("cli.md");
        let reported = "https://github.com/haiven-dev/haiven-cli/pull/10: #adversarial-pr";
        let ready = "https://github.com/haiven-dev/haiven-cli/pull/11: #adversarial-pr";
        let content = attr_doc(
            "subagents=2",
            &[
                "https://github.com/haiven-dev/haiven-cli/pull/10\n  - #adversarial-pr",
                "https://github.com/haiven-dev/haiven-cli/pull/11\n  - #adversarial-pr",
            ],
        );
        std::fs::write(&file, &content).unwrap();

        let owner = "subagent:adversarial-pr-cli10";
        let (_, claim) = crate::queue_claim::claim(&file, reported, owner, 600).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            reported,
            owner,
            "/root/adversarial-pr-cli10",
            600,
            claim.generation,
        )
        .unwrap();
        crate::queue_claim::report_attempt(&file, reported, Some(claim.generation)).unwrap();

        assert!(reported_writeback_pending_for_content(&file, &content).unwrap());
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec![ready.to_string()],
            "the exact reported vector stays excluded while its ready sibling dispatches"
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
    fn pending_dispatch_rejects_subagent_marker_near_miss_before_inline_selection() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = attr_doc("sub-agent", &["review Unreal"]);
        std::fs::write(&file, &content).unwrap();

        let error = pending_subagent_dispatch_for_content(&file, &content)
            .expect_err("ambiguous ownership must block pending dispatch planning");
        let message = format!("{error:#}");
        assert!(message.contains("ambiguous subagent ownership"), "{message}");
        assert!(message.contains("use canonical `subagents`"), "{message}");

        let error = queue_attr_subagent_plan(&file, &content)
            .expect_err("the public plan boundary must propagate the same error");
        assert!(
            format!("{error:#}").contains("use canonical `subagents`"),
            "{error:#}"
        );
    }

    /// `#preflightqueuescale`: one plan derives the document-wide dispatch
    /// facts (operator-awaiting heads, blockers, answer routing) once, however
    /// many `subagents` queue blocks the document has. Each derivation is a
    /// whole-document pass, so per-block derivation made every plan cost
    /// `blocks x document`; the plan runs many times per preflight.
    #[test]
    fn subagent_plan_derives_document_facts_once_for_any_number_of_queue_blocks() {
        fn doc(blocks: usize) -> String {
            let mut out = String::from("---\nagent_doc_format: template\n---\n\n");
            for block in 0..blocks {
                out.push_str(&format!(
                    "## Run {block} <!-- agent:run id=\"r{block}\" subagents go -->\n"
                ));
                for row in 0..4 {
                    out.push_str(&format!("- do [#b{block}r{row}]\n"));
                }
                out.push_str("<!-- /agent:run -->\n\n");
            }
            out.push_str("## Todo <!-- agent:todo -->\n");
            for todo in 0..10 {
                out.push_str(&format!("- [ ] [#t{todo}] operator step {todo}\n"));
            }
            out.push_str("<!-- /agent:todo -->\n");
            out
        }
        let derivations_for = |blocks: usize| {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
            let file = dir.path().join("task.md");
            let content = doc(blocks);
            std::fs::write(&file, &content).unwrap();
            let before = core::QueueDispatchDocumentFacts::derivations();
            let plan = queue_attr_subagent_plan(&file, &content).unwrap().unwrap();
            assert!(!plan.dispatch.is_empty(), "the fixture must dispatch work");
            core::QueueDispatchDocumentFacts::derivations() - before
        };
        assert_eq!(derivations_for(1), 1);
        assert_eq!(
            derivations_for(12),
            1,
            "document facts must be derived once per plan, not once per queue block"
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
        let plan = queue_attr_subagent_plan(&file, &content)
            .unwrap()
            .unwrap();
        assert!(plan.held.is_empty(), "a bare attr has no capacity hold");
    }

    #[test]
    fn queue_attr_dispatches_parent_with_only_comment_children() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## API PRs\n\n",
            "<!-- agent:run id=api-prs subagents presets=\"#push: #auth\" priority go -->\n",
            "- https://github.com/haiven-dev/haiven-api/pull/717\n",
            "  - %%waiting for review%%\n",
            "- https://github.com/haiven-dev/haiven-api/pull/758\n",
            "  - %%one note%%\n",
            "  - [//]: # (another note)\n",
            "<!-- /agent:run -->\n"
        );
        std::fs::write(&file, content).unwrap();

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, content).unwrap(),
            vec![
                "https://github.com/haiven-dev/haiven-api/pull/717".to_string(),
                "https://github.com/haiven-dev/haiven-api/pull/758".to_string(),
            ],
            "comment children are inert annotations, not descendants that make the parent a group label"
        );
    }

    #[test]
    fn queue_attr_does_not_dispatch_parents_with_parked_comment_children() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let content = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "## haiven-docs PRs\n\n",
            "<!-- agent:run id=haiven-docs-prs subagents presets=\"#auth\" keep go -->\n",
            "- https://github.com/haiven-dev/haiven-docs/pull/39\n",
            "  - %% parked %%\n",
            "- https://github.com/haiven-dev/haiven-docs/pull/40\n",
            "  - %% parked %%\n",
            "<!-- /agent:run -->\n"
        );
        std::fs::write(&file, content).unwrap();

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, content).unwrap(),
            Vec::<String>::new(),
            "the explicit parked sentinel must hold its parent out of dispatch"
        );
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
        let plan = queue_attr_subagent_plan(&file, &content)
            .unwrap()
            .unwrap();
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
        let plan = queue_attr_subagent_plan(&file, &content)
            .unwrap()
            .unwrap();
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

    fn three_runs(
        first_attr: &str,
        first_body: &str,
        second_attr: &str,
        second_body: &str,
    ) -> String {
        format!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n\
             <!-- agent:run id=first {first_attr} -->\n{first_body}\
             <!-- /agent:run -->\n\n\
             <!-- agent:run id=second {second_attr} -->\n{second_body}\
             <!-- /agent:run -->\n\n\
             <!-- agent:run id=third subagents -->\n- third work\n\
             <!-- /agent:run -->\n"
        )
    }

    fn offered_heads(content: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        std::fs::write(&file, content).unwrap();
        pending_subagent_dispatch_for_content(&file, content).unwrap()
    }

    #[test]
    fn paused_earlier_runs_still_offer_the_other_two() {
        let listed = offered_heads(&three_runs(
            "pause subagents",
            "- paused-work\n",
            "subagents",
            "- second work\n",
        ));
        assert_eq!(
            listed,
            vec!["second work".to_string(), "third work".to_string()],
            "a pause control on the first run does not hide the other two: {listed:?}"
        );

        let stopped = offered_heads(&three_runs(
            "stop subagents",
            "- paused-work\n",
            "subagents",
            "- second work\n",
        ));
        assert_eq!(
            stopped,
            vec!["second work".to_string(), "third work".to_string()],
            "a legacy stop marker does not hide the other two: {stopped:?}"
        );

        let inline = offered_heads(&three_runs(
            "",
            "- finish the inline work\n",
            "subagents",
            "- second work\n",
        ));
        assert!(
            inline.is_empty(),
            "owner-available inline work gates the later runs: {inline:?}"
        );

        let dependent = offered_heads(&three_runs(
            "pause subagents",
            "- paused-work\n",
            "after=first subagents",
            "- dependent work\n",
        ));
        assert_eq!(
            dependent,
            vec!["third work".to_string()],
            "after= still waits on a paused predecessor: {dependent:?}"
        );

        let frontmatter = concat!(
            "---\nsession: sid\nagent_doc_format: template\nqueue: pause\n---\n\n",
            "<!-- agent:run id=first subagents -->\n- paused-work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=second subagents go -->\n- second work\n<!-- /agent:run -->\n\n",
            "<!-- agent:run id=third subagents go -->\n- third work\n<!-- /agent:run -->\n",
        );
        let held = offered_heads(frontmatter);
        assert_eq!(
            held,
            vec!["second work".to_string(), "third work".to_string()],
            "frontmatter pause holds the first run and still offers the others: {held:?}"
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
    /// keeps that exact line out of dispatch while unrelated work remains
    /// eligible; a `--requeue` release returns the reported line too.
    #[test]
    fn reported_release_is_not_redispatched_but_a_requeue_is() {
        use crate::claim_release_receipt::{ReleaseKind, release};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let question = "Does the page status enum have an archived value?";
        let content = attr_doc(
            "subagents",
            &[question, "pinned follow-up", "pull 9 continuation"],
        );
        std::fs::write(&file, &content).unwrap();
        let all = vec![
            question.to_string(),
            "pinned follow-up".to_string(),
            "pull 9 continuation".to_string(),
        ];
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            all
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
        crate::claim_release_receipt::capture_response_for_attempt(
            &file,
            question,
            1,
            "page status response",
        )
        .unwrap();
        release(&file, question, ReleaseKind::Reported).unwrap();
        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec![
                "pinned follow-up".to_string(),
                "pull 9 continuation".to_string()
            ],
            "reported writeback debt excludes only its own row"
        );
        let held = out_of_loop_items_for_content(&file, &content);
        assert!(
            held.claims("pinned follow-up") && held.claims("pull 9 continuation"),
            "fresh dispatch remains held out of the inline loop while awaiting spawn"
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
            all,
            "a requeued line is dispatchable again"
        );
    }

    /// `#reportedwritebackdebt`: `report-complete` retains the attempt as
    /// Reported until verified coordinator closeout. At the next safe
    /// boundary it keeps its own row fenced without suppressing independent
    /// dispatch or making later worker-owned rows eligible for inline drain.
    #[test]
    fn active_reported_attempt_stays_open_while_fresh_dispatch_proceeds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("task.md");
        let question = "Integrate the completed worker change";
        let content = attr_doc("subagents=1", &[question, "later worker task"]);
        std::fs::write(&file, &content).unwrap();

        crate::queue_claim::claim(&file, question, "subagent:integrated-change", 3600).unwrap();
        crate::queue_claim::start_attempt(
            &file,
            question,
            "subagent:integrated-change",
            "worker:integrated-change",
            3600,
            1,
        )
        .unwrap();
        crate::queue_claim::report_attempt(&file, question, Some(1)).unwrap();

        assert_eq!(
            pending_subagent_dispatch_for_content(&file, &content).unwrap(),
            vec!["later worker task".to_string()],
            "a Reported attempt no longer occupies the queue's only worker slot"
        );
        let claims = crate::queue_claim::claim_states_for_content(&file, &content).unwrap();
        assert!(
            claims.iter().any(|claim| {
                claim.item_text == question
                    && claim.state == agent_doc_queue::queue_claim::QueueClaimState::Reported
            }),
            "the reported row remains open for coordinator integration: {claims:?}"
        );
        let held = out_of_loop_items_for_content(&file, &content);
        assert!(
            held.claims("later worker task"),
            "fresh dispatch stays out of the inline loop while awaiting spawn"
        );
        assert!(
            held.claims(question),
            "reported coordinator debt is out of the fresh drain loop"
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
