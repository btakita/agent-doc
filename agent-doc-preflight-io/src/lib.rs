//! Preflight maintenance I/O adapters.

pub mod admission_replica_hold;
pub mod debounce;
pub mod gc;
pub mod layout;
pub mod sweep;
pub mod warnings;

use agent_doc_document::queue_projection::{
    set_work_item_lifecycle_markers, strip_in_progress_marker, strip_priority_markers,
    sync_in_progress_marker_regions,
};
use agent_doc_element::element::{
    is_backlog_component, is_review_component, is_tracked_work_component,
};
use agent_doc_element_backlog::backlog::{
    component_matches_tracked_surface, ensure_no_completed_tracked_items, format_shadow_refs,
    maintenance_surface_label, review_counts, should_reap_already_done_mirrors,
    should_reap_ops_proof_completions, tracked_body_for_reorder,
};
use agent_doc_frontmatter::frontmatter;
use agent_doc_queue::{
    backlog_sync::AutoBacklogQueueSyncPolicy,
    control_binding::{
        converge_queue_control_binding_content, explicit_queue_go_mode, explicit_queue_pause_mode,
        explicit_queue_stop_mode, frontmatter_queue_control, infer_queue_go_from_prompt_edit,
        strip_queue_activation_tokens_in_content,
    },
    free_text_admission::{
        FreeTextAdmissionExecution, FreeTextAdmissionScope, FreeTextAdmissionTarget,
        FreeTextCoinGate, append_empty_agent_component, collect_actionable_free_text_prompts,
        gate_free_text_admission_scope, prepare_free_text_admission_for_queue_with_reserved,
        queue_currently_active_for_free_text_admission, queue_free_text_admission_scope,
    },
    queue_convergence::{
        inactive_queue_changed_vs_snapshot, queue_body_clear_is_lossless,
        queue_entries_are_drained_residue, queue_region_differs_from_snapshot,
        selected_queue_head_unchanged_in_snapshot,
    },
    queue_response::queue_prompt_text_is_free_text,
};
use agent_doc_turn::op_log::OpsLogEvent;
use agent_doc_workflow::preflight_policy::ResolvedFreeTextExecution;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightWarning {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_harness: Option<String>,
}

impl From<agent_doc_workflow::preflight_policy::PreflightPolicyWarning> for PreflightWarning {
    fn from(warning: agent_doc_workflow::preflight_policy::PreflightPolicyWarning) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
            document_agent: None,
            active_harness: None,
        }
    }
}

fn log_snapshot_recovery_warning(file: &Path, context: &str, detail: impl Display) {
    eprintln!("[preflight] snapshot recovery warning during {context}: {detail}");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "snapshot_recovery_warning file={} context={} detail={}",
            file.display(),
            context,
            detail
        ),
    );
}

fn current_text_via_preflight_authority(
    file: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    #[cfg(test)]
    if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return Ok(Some(
            agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file)?,
        ));
    }
    agent_doc_controller_io::project_controller::current_text_via_controller_model_read_for_doc(
        file, source,
    )
}

/// `#px82` — how many bounded authority observations queue maintenance makes
/// before it gives up and discards the recomputed queue. Override with
/// `AGENT_DOC_QUEUE_AUTHORITY_ATTEMPTS`.
const DEFAULT_QUEUE_AUTHORITY_ATTEMPTS: u32 = 3;
const QUEUE_AUTHORITY_ATTEMPTS_ENV: &str = "AGENT_DOC_QUEUE_AUTHORITY_ATTEMPTS";
/// Backoff between authority observations. Deliberately short: the replica
/// re-registration below is asynchronous, and a live editor typically re-attaches
/// within a few hundred milliseconds.
const QUEUE_AUTHORITY_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(250);

fn queue_authority_attempts() -> u32 {
    std::env::var(QUEUE_AUTHORITY_ATTEMPTS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|attempts| *attempts >= 1)
        .unwrap_or(DEFAULT_QUEUE_AUTHORITY_ATTEMPTS)
}

/// Observation status token for a single authority read, used for the per-attempt
/// `source=<..> status=<..>` instrumentation `#px82` asks for.
fn current_text_status_token(
    observed: &Result<Option<agent_doc_crdt_relay_io::CurrentText>>,
) -> String {
    match observed {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { .. })) => "current".to_string(),
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) => "detached".to_string(),
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)) => {
            "editor_attached_model_missing".to_string()
        }
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending)) => {
            "editor_sync_pending".to_string()
        }
        Ok(None) => "none".to_string(),
        Err(err) => format!("error:{}", format!("{err:#}").replace('\n', "\\n")),
    }
}

/// `#px82` / `#bn41` — observe the authority with bounded retry instead of
/// discarding the recomputed queue on the first non-current status.
///
/// The editor-authority failure is INTERMITTENT (observed alternating FAIL/OK
/// across back-to-back preflights), so a single bounded observation is a coin
/// flip that silently disarms the drain. Two things change here:
///
/// 1. Every attempt logs the `source`+`status` pair it observed, so the retry is
///    diagnosable from `ops.log` instead of only from the final error string.
/// 2. When the status is `editor_attached_model_missing`, request the replica
///    re-registration the binary already reports as
///    `editor_replica_reregister=requested` BEFORE surfacing the error, so this
///    self-heals instead of needing manual `admin recycle` + `admin reload-lib`.
///
/// A `Current`/`Detached`/`None` observation returns immediately. Transient
/// editor-authority statuses and controller observation errors consume the
/// bounded retry budget.
fn current_text_via_preflight_authority_retrying(
    file: &Path,
    source: &str,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    observe_current_text_with_bounded_retry(file, source, queue_authority_attempts(), |file| {
        current_text_via_preflight_authority(file, source)
    })
}

fn observe_current_text_with_bounded_retry(
    file: &Path,
    source: &str,
    attempts: u32,
    mut observe: impl FnMut(&Path) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>>,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    let attempts = attempts.max(1);
    let mut observed = observe(file);
    for attempt in 1..=attempts {
        let status = current_text_status_token(&observed);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_authority_observation file={} source={} attempt={}/{} status={} (#px82)",
                file.display(),
                source,
                attempt,
                attempts,
                status
            ),
        );
        use agent_doc_turn::authority_recovery::{
            AuthorityObservation, AuthorityRecoveryDecision, AuthorityRecoveryFacts,
            decide_authority_recovery,
        };
        let observation = match &observed {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { .. })) => {
                AuthorityObservation::Current
            }
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) => {
                AuthorityObservation::Detached
            }
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica)) => {
                AuthorityObservation::MissingReplica
            }
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::EditorSyncPending)) => {
                AuthorityObservation::SyncPending
            }
            Ok(None) | Err(_) => AuthorityObservation::Error,
        };
        let decision = decide_authority_recovery(AuthorityRecoveryFacts {
            observation,
            editor_open: matches!(
                observation,
                AuthorityObservation::MissingReplica | AuthorityObservation::SyncPending
            ),
            retries_remaining: attempt < attempts,
            // Preflight's bounded loop owns both transient retries. It returns
            // the exhausted observation to the caller rather than duplicating
            // realtime-io's model-rebuild effect.
            rebuild_after_retry_exhaustion: false,
            // Preflight never sends the re-registration event, so it observes no
            // receipt and holds no refusal proof. The realtime resolver owns that
            // fact; claiming it here would descend to disk on someone else's
            // evidence.
            endpoint_definitively_refused: false,
        });
        let AuthorityRecoveryDecision::Retry {
            request_plugin_refresh,
        } = decision
        else {
            return observed;
        };
        // `#bn41`: the missing piece is the editor REPLICA, not the controller.
        // Ask for re-registration and then re-observe rather than reporting an
        // error the operator has to clear with two manual admin commands.
        if request_plugin_refresh {
            let reregister = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
                file,
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::EditorReplicaReregister,
                0,
            ) {
                Ok(()) => "requested".to_string(),
                Err(err) => format!("failed:{}", format!("{err:#}").replace('\n', "\\n")),
            };
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_authority_replica_reregister file={} source={} attempt={} status={} (#bn41)",
                    file.display(),
                    source,
                    attempt,
                    reregister
                ),
            );
        }
        std::thread::sleep(QUEUE_AUTHORITY_RETRY_BACKOFF);
        observed = observe(file);
    }
    observed
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GateVerifyResult {
    pub id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_resolved: bool,
}

/// `#todoids`: one `agent:todo` item whose link to tracked work calls for an
/// action this cycle. Derived from document state only, so it repeats every
/// preflight until the state changes (the check is idempotent, not an event).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoSignal {
    /// `tracked_work_missing_todo_backlink`, `legacy_source_unqualified`,
    /// `operator_done_review_open`, `operator_done_backlog_open`, `operator_done_icebox_open`,
    /// `linked_work_resolved`, or `cited_work_resolved`.
    pub kind: String,
    /// The `--todo-*` selector of the item (its own id, else its text).
    pub todo: String,
    /// The tracked-work ids the signal is about.
    pub ids: Vec<String>,
    /// What the agent does about it.
    pub action: String,
}

/// `#todoids`: render the agent-facing action for one link finding.
pub fn todo_signal_from_finding(finding: &agent_doc_element::todo::TodoLinkFinding) -> TodoSignal {
    use agent_doc_element::todo::TodoLinkFindingKind as Kind;
    let ids = finding
        .ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let selector = if finding.selector.contains(char::is_whitespace) {
        format!("'{}'", finding.selector.replace('\'', "'\\''"))
    } else {
        finding.selector.clone()
    };
    let action = match finding.kind {
        Kind::TrackedWorkMissingTodoBacklink => format!(
            "Open agent:{component} item {ids} has no backlink to an existing Todo. Add or reuse an operator Todo, then put `(todos: [#todo-id])` on {ids} or put `(source: agent:{component} [#{id}])` on that Todo. Metadata naming a missing Todo does not satisfy the invariant.",
            component = finding.component.as_deref().unwrap_or("tracked-work"),
            id = finding.ids.first().map(String::as_str).unwrap_or_default(),
        ),
        Kind::LegacySourceUnqualified => format!(
            "Legacy todo {label} has one tracked-work source ({component} {ids}) but no canonical visible locator. Preflight maintenance qualifies it as (source: agent:{component} [#{id}]); if it remains unqualified, wait for the operator typing gate to settle and rerun preflight.",
            label = finding.label,
            component = finding.component.as_deref().unwrap_or("tracked-work"),
            id = finding.ids.first().map(String::as_str).unwrap_or_default(),
        ),
        Kind::OperatorDoneReviewOpen => format!(
            "The operator checked todo {label}, the operator prompt for review gate {ids}. Read any operator note, then resolve the gate this cycle (--review-resolve {id}) or record the follow-up; agent-doc never resolves a gate from a checkbox, because which outcome the operator chose is judgement.",
            label = finding.label,
            id = finding.ids.first().map(String::as_str).unwrap_or_default(),
        ),
        Kind::OperatorDoneBacklogOpen => format!(
            "The operator checked todo {label}, the operator prerequisite for backlog {ids}. Treat {ids} as unblocked (--backlog-ungate {id} if it is gated).",
            label = finding.label,
            id = finding.ids.first().map(String::as_str).unwrap_or_default(),
        ),
        Kind::OperatorDoneIceboxOpen => format!(
            "The operator checked todo {label}, the operator decision on parked icebox item {ids}. Read any operator note, then act on it this cycle (close it with --done {id}, move it into active work, or record the follow-up); agent-doc never moves or closes parked work from a checkbox.",
            label = finding.label,
            id = finding.ids.first().map(String::as_str).unwrap_or_default(),
        ),
        Kind::LinkedWorkResolved => format!(
            "Tracked work {ids} is resolved, so todo {label} is finished or moot. The next closeout that carries tracked-work flags checks it off automatically; to do it now: --todo-done {selector}.",
            label = finding.label,
        ),
        Kind::CitedWorkResolved => format!(
            "Every tracked item todo {label} cites ({ids}) is resolved. If the operator action is finished, check it off: --todo-done {selector}. agent-doc never auto-completes a todo that only cites work.",
            label = finding.label,
        ),
    };
    TodoSignal {
        kind: finding.kind.as_str().to_string(),
        todo: finding.selector.clone(),
        ids: finding.ids.clone(),
        action,
    }
}

/// `#todoidbackfill`: give every id-less `agent:todo` row an own id this
/// preflight, the way queue maintenance coins free-text queue lines into
/// backlog ids. The same pass qualifies the visible tracked-work source of
/// every legacy singly linked row (`#todocontextmigration`). A row typed by
/// hand, written by an older binary, or added through `patch:todo` then has an
/// id a closeout can complete and a component/id locator the operator can
/// follow.
///
/// Runs before queue maintenance, against the same editor/CP authority, behind
/// the same typing gate (`coin_gate`): a row the operator is still typing keeps
/// no id or source qualifier until a later pass. Existing ids are preserved;
/// maintenance changes only a missing id token and/or the canonical trailing
/// source locator. The snapshot's todo body follows when it matched the
/// pre-maintenance body, so the repair does not read as an operator diff. Fail
/// closed: unreadable tracked-work evidence, an editor head that is not
/// current, or a refused write changes nothing this pass. Returns ids assigned
/// during this pass.
pub fn run_todo_id_maintenance(file: &Path, coin_gate: &FreeTextCoinGate) -> Result<Vec<String>> {
    run_todo_id_maintenance_with_options(file, coin_gate, false, None)
}

/// Force-disk counterpart used by an explicitly force-disk strict closeout.
/// The caller supplies the same write effects as pending maintenance so write
/// provenance remains complete even though editor authority is bypassed.
pub fn run_todo_id_maintenance_force_disk(
    file: &Path,
    coin_gate: &FreeTextCoinGate,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<Vec<String>> {
    run_todo_id_maintenance_with_options(file, coin_gate, true, Some(write_effects))
}

fn run_todo_id_maintenance_with_options(
    file: &Path,
    coin_gate: &FreeTextCoinGate,
    force_disk: bool,
    write_effects: Option<&dyn PreflightMaintenanceWriteEffects>,
) -> Result<Vec<String>> {
    let source = "preflight_todo_id_maintenance";
    let content = if force_disk {
        match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(_) => return Ok(Vec::new()),
        }
    } else {
        match current_text_via_preflight_authority_retrying(file, source)? {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
            Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
                match std::fs::read_to_string(file) {
                    Ok(text) => text,
                    Err(_) => return Ok(Vec::new()),
                }
            }
            Some(_) => return Ok(Vec::new()),
        }
    };
    if agent_doc_element::todo::find_todo_component(&content)
        .ok()
        .flatten()
        .is_none()
    {
        return Ok(Vec::new());
    }
    let links = match agent_doc_element_backlog_io::backlog_cmd::todo_link_index_in_content(
        file,
        &content,
        true,
        &[],
    ) {
        Ok(links) => links,
        Err(err) => {
            eprintln!(
                "[preflight] todo: tracked-work evidence unreadable ({err:#}); todo id backfill skipped"
            );
            return Ok(Vec::new());
        }
    };
    let reserved = agent_doc_element_backlog::backlog::document_reserved_identity_ids(&content);
    let doc_id = agent_doc_hash::document_id_for_path(file);
    let id_for = |action: &str, counter: u64| {
        agent_doc_element_backlog::backlog::generate_hash_n(action, &doc_id, counter, 4)
    };
    let id_outcome = agent_doc_element::todo::assign_missing_todo_ids(
        &content,
        &id_for,
        &reserved,
        &links,
        &|line| coin_gate.allows_coining(line),
    )?;
    let source_outcome = agent_doc_element::todo::qualify_todo_source_references(
        &id_outcome.content,
        &links,
        &|line| coin_gate.allows_coining(line),
    )?;
    if id_outcome.assigned.is_empty() && source_outcome.qualified.is_empty() {
        for finding in &source_outcome.findings {
            eprintln!(
                "[preflight] todo: legacy source audit {} -> agent:{} #{} held by typing gate (#todocontextmigration)",
                finding.selector, finding.component, finding.id
            );
        }
        return Ok(Vec::new());
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file);
    let persisted = if force_disk {
        persist_pending_maintenance_doc(
            file,
            &content,
            &source_outcome.content,
            source,
            true,
            write_effects.context("force-disk todo-id maintenance requires write effects")?,
        )?;
        source_outcome.content.clone()
    } else {
        persist_queue_maintenance_doc(
            file,
            &source_outcome.content,
            &content,
            project_root.as_deref(),
            source,
        )?
    };
    if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
        && let Some(new_snapshot) =
            agent_doc_element::todo::carry_todo_body_into_snapshot(&snapshot, &content, &persisted)
        && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &new_snapshot,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] todo: snapshot sync warning: {err}");
    }
    let mut ids = Vec::with_capacity(id_outcome.assigned.len());
    for assignment in &id_outcome.assigned {
        let short: String = assignment.action.chars().take(60).collect();
        eprintln!(
            "[preflight] todo: assigned own id [#{}] to \"{short}\" (#todoidbackfill)",
            assignment.id
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "todo_id_assigned file={} id={}",
                file.display(),
                assignment.id,
            ),
        );
        ids.push(assignment.id.clone());
    }
    for finding in &source_outcome.findings {
        let migrated = source_outcome
            .qualified
            .iter()
            .any(|selector| selector == &finding.selector);
        let status = if migrated {
            "migrated"
        } else {
            "held_by_typing_gate"
        };
        eprintln!(
            "[preflight] todo: legacy source audit {} -> agent:{} #{} {status} (#todocontextmigration)",
            finding.selector, finding.component, finding.id
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "todo_source_qualification file={} todo={} id={} component={} status={status}",
                file.display(),
                finding.selector.replace(char::is_whitespace, "_"),
                finding.id,
                finding.component,
            ),
        );
    }
    Ok(ids)
}

/// `#todoids`: evaluate the document's `agent:todo` against its tracked work
/// (including the external done archive). Fail closed: unreadable evidence
/// yields no signal and a stderr note, never a "nothing to do" that could be
/// mistaken for a verdict.
pub fn todo_signals_for_document(file: &Path, content: &str) -> Vec<TodoSignal> {
    let links = match agent_doc_element_backlog_io::backlog_cmd::todo_link_index_in_content(
        file,
        content,
        true,
        &[],
    ) {
        Ok(links) => links,
        Err(err) => {
            eprintln!(
                "[preflight] todo: tracked-work evidence unreadable ({err:#}); todo link checks skipped"
            );
            return Vec::new();
        }
    };
    match agent_doc_element::todo::todo_link_findings(content, &links) {
        Ok(findings) => {
            let mut signals = findings
                .iter()
                .map(todo_signal_from_finding)
                .collect::<Vec<_>>();
            // GH #381: project-inherited presets are invocations too; they are
            // never copied into the document, so resolve the effective map here.
            let presets = agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(
                content, file,
            );
            let preset_only =
                agent_doc_queue::queue_continuation::preset_only_identity_ids_with_presets(
                    content, &presets,
                );
            append_unlinked_queue_signals(content, &links, &preset_only, &mut signals);
            signals
        }
        Err(err) => {
            eprintln!(
                "[preflight] todo: agent:todo unreadable ({err:#}); todo link checks skipped"
            );
            Vec::new()
        }
    }
}

/// Queue heads normally inherit the Todo backlink of the tracked id they name.
/// Active free-text heads are first promoted to stable backlog ids by queue
/// maintenance. This audit catches any free-text head held from promotion (for
/// example by pause/typing gates), plus orphan id heads, so no queue item can
/// silently escape the backlink invariant.
///
/// GH #381: a head whose leading `#name` is an effective preset (`preset_only`:
/// a document or `.agent-doc/project.md` preset that is not also open tracked
/// work) is a preset invocation. It names no tracked work, free-text admission
/// never promotes a `#`-led line, and the preset contract never asks for a
/// backing item, so it owes no Todo backlink and is not reported.
fn append_unlinked_queue_signals(
    content: &str,
    links: &agent_doc_element::todo::TodoLinkIndex,
    preset_only: &std::collections::HashSet<String>,
    signals: &mut Vec<TodoSignal>,
) {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return;
    };
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let Ok(entries) = agent_doc_queue::document_queue::parse(component.content(content)) else {
            continue;
        };
        for prompt in agent_doc_queue::document_queue::prompts(&entries) {
            if agent_doc_queue::queue_continuation::head_leads_with_preset(
                &prompt.text,
                preset_only,
            ) {
                continue;
            }
            match agent_doc_element_queue::QueueItemIdentity::from_prompt(&prompt.text) {
                agent_doc_element_queue::QueueItemIdentity::Id(id)
                    if links.knows(&id) || links.has_todo(&id) => {}
                agent_doc_element_queue::QueueItemIdentity::Id(id) => {
                    signals.push(TodoSignal {
                        kind: "tracked_work_missing_todo_backlink".to_string(),
                        todo: id.clone(),
                        ids: vec![id.clone()],
                        action: format!(
                            "Queue head #{id} names neither tracked work with an inherited Todo backlink nor an existing Todo. Add the backing backlog/icebox item and Todo backlink, or link the head to an existing Todo."
                        ),
                    });
                }
                agent_doc_element_queue::QueueItemIdentity::FreeText(text) => {
                    signals.push(TodoSignal {
                        kind: "tracked_work_missing_todo_backlink".to_string(),
                        todo: text.clone(),
                        ids: Vec::new(),
                        action: format!(
                            "Free-text queue head {text:?} has no stable Todo backlink. Let preflight promote it to a backlog surrogate id, then add/reuse a Todo and record `(todos: [#todo-id])`; if promotion is held, finish the edit or unpause and rerun preflight."
                        ),
                    });
                }
            }
        }
    }
}

/// A change detected in a related document since the last cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedDocChange {
    /// Path to the related document (as declared in frontmatter).
    pub path: String,
    /// Human-readable summary of what changed.
    pub summary: String,
    /// Whether the related document exists on disk.
    pub exists: bool,
}

/// Resolve the links cache directory, creating it if needed.
pub fn links_cache_dir(file: &Path) -> Option<std::path::PathBuf> {
    let mut search = file.parent();
    while let Some(d) = search {
        let candidate = d.join(".agent-doc");
        if candidate.is_dir() {
            let cache = candidate.join("links_cache");
            std::fs::create_dir_all(&cache).ok()?;
            return Some(cache);
        }
        search = d.parent();
    }
    None
}

/// Fetch a URL and compare against cached content. Returns a change entry if content differs.
pub fn check_url_link(url: &str, cache_dir: &Path) -> RelatedDocChange {
    let cache_path = agent_doc_workflow::preflight_policy::url_cache_path(cache_dir, url);
    let cached = std::fs::read_to_string(&cache_path).ok();

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();
    let response = agent.get(url).call();

    match response {
        Ok(mut resp) => {
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let body = match resp.body_mut().read_to_string() {
                Ok(b) => b,
                Err(e) => {
                    return RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("fetch error: {}", e),
                        exists: false,
                    };
                }
            };

            let content = if agent_doc_workflow::preflight_policy::is_html_content(&content_type) {
                agent_doc_workflow::preflight_policy::html_to_markdown(&body)
            } else {
                body
            };

            match cached {
                Some(ref old) if old == &content => RelatedDocChange {
                    path: url.to_string(),
                    summary: String::new(),
                    exists: true,
                },
                Some(_) => {
                    let _ = std::fs::write(&cache_path, &content);
                    RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("content changed ({} bytes)", content.len()),
                        exists: true,
                    }
                }
                None => {
                    let _ = std::fs::write(&cache_path, &content);
                    RelatedDocChange {
                        path: url.to_string(),
                        summary: format!("initial fetch ({} bytes)", content.len()),
                        exists: true,
                    }
                }
            }
        }
        Err(e) => RelatedDocChange {
            path: url.to_string(),
            summary: format!("fetch failed: {}", e),
            exists: false,
        },
    }
}

pub fn check_linked_docs(file: &Path) -> Vec<RelatedDocChange> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let fm = match frontmatter::parse(&content) {
        Ok((fm, _)) => fm,
        Err(_) => return vec![],
    };
    if fm.links.is_empty() {
        return vec![];
    }

    let our_baseline_mtime = agent_doc_git_io::revision::last_commit_mtime(file)
        .ok()
        .flatten();

    let doc_dir = match file.parent() {
        Some(d) => d,
        None => return vec![],
    };

    let cache_dir = links_cache_dir(file);

    let mut changes = Vec::new();
    for link in &fm.links {
        if agent_doc_workflow::preflight_policy::is_url(link) {
            if let Some(ref cache) = cache_dir {
                let change = check_url_link(link, cache);
                if !change.summary.is_empty() {
                    changes.push(change);
                }
            } else {
                eprintln!(
                    "[preflight] warning: cannot resolve links cache for URL: {}",
                    link
                );
            }
            continue;
        }

        let resolved = doc_dir.join(link);
        if !resolved.exists() {
            changes.push(RelatedDocChange {
                path: link.clone(),
                summary: "file not found".to_string(),
                exists: false,
            });
            continue;
        }

        let related_mtime = match agent_doc_git_io::revision::last_commit_mtime(&resolved) {
            Ok(Some(t)) => t,
            _ => continue,
        };

        let is_newer = match our_baseline_mtime {
            Some(snap_time) => related_mtime > snap_time,
            None => true,
        };

        if !is_newer {
            continue;
        }

        let summary = recent_commit_summary(&resolved, our_baseline_mtime);
        changes.push(RelatedDocChange {
            path: link.clone(),
            summary,
            exists: true,
        });
    }

    changes
}

/// Get a human-readable summary of recent commits for a file.
pub fn recent_commit_summary(file: &Path, since: Option<std::time::SystemTime>) -> String {
    match agent_doc_git_io::revision::recent_commit_lines(file, since, 5) {
        agent_doc_git_io::revision::RecentCommitLog::Lines(lines) => lines.join("; "),
        agent_doc_git_io::revision::RecentCommitLog::Empty => "changed".to_string(),
        agent_doc_git_io::revision::RecentCommitLog::GitUnavailable => {
            "changed (git unavailable)".to_string()
        }
        agent_doc_git_io::revision::RecentCommitLog::LogFailed => {
            "changed (git log failed)".to_string()
        }
    }
}

pub fn claims_log_path(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_project_root_io::project_root_containing(&canonical)?;

    Some(root.join(".agent-doc/claims.log"))
}

/// Read the claims log without mutating it. Returns non-empty lines.
pub fn read_claims(file: &Path) -> Vec<String> {
    let Some(log_path) = claims_log_path(file) else {
        return vec![];
    };

    let contents = match std::fs::read_to_string(&log_path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    if contents.is_empty() {
        return vec![];
    }

    contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Read the claims log and truncate it. Returns non-empty lines.
pub fn read_and_truncate_claims(file: &Path) -> Vec<String> {
    let Some(log_path) = claims_log_path(file) else {
        return vec![];
    };

    let claims = read_claims(file);
    if claims.is_empty() {
        return claims;
    }

    if let Err(e) = std::fs::write(&log_path, "") {
        eprintln!("[preflight] failed to truncate claims log: {}", e);
    }

    claims
}

pub fn checkpoint_baseline_content(file: &Path, content: &str) -> bool {
    match agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        content,
        agent_doc_ops_log_io::log_op,
    ) {
        Ok(()) => {
            eprintln!("[preflight] document baseline checkpointed in state.db");
            true
        }
        Err(e) => {
            eprintln!("[preflight] failed to checkpoint document baseline: {}", e);
            false
        }
    }
}

pub fn explicit_backlog_target_requirements(
    source_file: &Path,
    source_frontmatter: &frontmatter::Frontmatter,
    targets: &[PathBuf],
) -> Result<Vec<agent_doc_cycle_state_io::BacklogTargetRequirement>> {
    let mut requirements = Vec::new();
    for target in targets {
        let target_existing = if target.exists() {
            Some(
                std::fs::read_to_string(target)
                    .with_context(|| format!("failed to read {}", target.display()))?,
            )
        } else {
            None
        };
        let target_frontmatter = if let Some(content) = target_existing.as_ref() {
            Some(agent_doc_frontmatter_io::session::parse_for_file(content, target)?.0)
        } else {
            None
        };
        agent_doc_frontmatter_io::security_review::enforce_cross_document_review(
            "preflight prompt contract",
            source_file,
            source_frontmatter,
            target,
            target_frontmatter.as_ref(),
        )?;
        let fingerprint = match target_existing.as_deref() {
            Some(content) => {
                agent_doc_document::tracked_work_projection::tracked_work_fingerprint(content)?
            }
            None => agent_doc_document::tracked_work_projection::TrackedWorkFingerprint::empty(),
        };
        requirements.push(agent_doc_cycle_state_io::BacklogTargetRequirement {
            path: std::fs::canonicalize(target)
                .unwrap_or_else(|_| target.to_path_buf())
                .display()
                .to_string(),
            component: fingerprint.component,
            baseline_hash: fingerprint.baseline_hash,
            baseline_item_ids: fingerprint.baseline_item_ids,
        });
    }
    Ok(requirements)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightResponseContract {
    pub mode: String,
    pub required_option: String,
    pub opening_marker: String,
    pub closing_marker: String,
    pub plain_stdin_allowed: bool,
}

impl PreflightResponseContract {
    pub fn strict_template() -> Self {
        Self {
            mode: "strict_template".to_string(),
            required_option: "--template".to_string(),
            opening_marker: "<!-- patch:exchange -->".to_string(),
            closing_marker: "<!-- /patch:exchange -->".to_string(),
            plain_stdin_allowed: false,
        }
    }
}

/// One queue line the cycle must dispatch to a subagent (`#closeout-steering`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSubagentDispatch {
    /// The queue line, verbatim (markers stripped).
    pub item: String,
    /// The claim to run BEFORE dispatching.
    pub claim_command: String,
    /// The acknowledgement to run AFTER successful dispatch, filling in the
    /// printed generation and returned worker identity.
    pub start_command: String,
    /// The command that re-prints [`Self::authorization`]'s preamble later
    /// (`agent-doc queue brief`), e.g. after the claim.
    pub brief_command: String,
    /// Resolved harness model override. Omitted to inherit harness defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Resolved harness reasoning-effort override. Omitted to inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Prior Lost attempt that caused this otherwise-old row to be offered
    /// again. This makes replacement dispatch explicit instead of leaving the
    /// visible warning marker without a cause or predecessor identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<QueueSubagentRecovery>,
    /// The operator's authorization for this item, resolved from the document
    /// (`#waypostauthorization`): the verbatim item, the backlog text an id
    /// head names, and the queue/item preset bodies. Its
    /// `subagent_prompt_preamble` is pasted verbatim into the subagent prompt.
    pub authorization: agent_doc_queue::subagent_brief::SubagentAuthorization,
}

/// Durable predecessor evidence for a replacement subagent dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueSubagentRecovery {
    pub reason: String,
    pub prior_owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_worker_id: Option<String>,
    pub prior_generation: u64,
}

/// Build the `queue_subagent_dispatch` contract entries for `file`, resolving
/// each item's authorization from `content` (the view dispatch was computed
/// from).
pub fn queue_subagent_dispatch_entries(
    file: &Path,
    content: &str,
    items: &[String],
) -> anyhow::Result<Vec<QueueSubagentDispatch>> {
    let _guard = agent_doc_queue_io::queue_claim::nested_claim_steer_for(file, content);
    let document = file.display().to_string();
    // Recovery context is diagnostic enrichment. Some pure contract callers
    // build entries before persisting their fixture/document; absence or an
    // unreadable ledger must retain the pre-existing dispatch contract rather
    // than fail a newly eligible item closed.
    let claim_reports =
        agent_doc_queue_io::queue_claim::claim_reports_for_content(file, content)
            .unwrap_or_default();
    let submitted_prompts =
        agent_doc_queue::queue_response::submitted_prompts_for_heads(content, items);
    items
        .iter()
        .zip(submitted_prompts.iter())
        .map(|(item, submitted)| {
            let authorization = agent_doc_queue_io::subagent_dispatch::subagent_authorization_for_content_submitted(
                file,
                content,
                item,
                submitted,
            )?;
            let identity = agent_doc_queue::queue_claim::claim_identity(item);
            let recovery = claim_reports
                .iter()
                .find(|report| {
                    report.claim.identity == identity
                        && report.claim.state
                            == agent_doc_queue::queue_claim::QueueClaimState::Lost
                })
                .and_then(|report| {
                    report
                        .lost_recovery
                        .as_ref()
                        .map(|lost| QueueSubagentRecovery {
                            reason: lost.reason.to_string(),
                            prior_owner: report.claim.owner.clone(),
                            prior_worker_id: report
                                .claim
                                .worker
                                .as_ref()
                                .map(|worker| worker.worker_id.clone()),
                            prior_generation: report.claim.generation,
                        })
                });
            Ok(QueueSubagentDispatch {
                item: item.clone(),
                claim_command: agent_doc_queue_io::subagent_dispatch::claim_command(
                    &document, item,
                ),
                start_command: agent_doc_queue_io::subagent_dispatch::start_command(
                    &document, item,
                ),
                brief_command: agent_doc_queue_io::subagent_dispatch::brief_command(
                    &document, item,
                ),
                model: authorization.model.clone(),
                effort: authorization.effort.clone(),
                recovery,
                authorization,
            })
        })
        .collect()
}

/// Guidance for a non-empty `queue_subagent_dispatch`.
pub const QUEUE_SUBAGENT_DISPATCH_GUIDANCE: &str = "The operator added these queue items with \
subagent intent. For EACH: run its `claim_command` (fill in `<label>`), then dispatch it NOW to \
its own background subagent (its own git worktree outside the IDE-watched project if it touches \
a repository). Only after the harness returns a worker identity, run its `start_command`, filling \
in the claim's printed generation and that returned identity; this binary acknowledgement changes \
🟡 dispatching to 🚧 running and is mandatory. Paste the entry's \
`authorization.subagent_prompt_preamble` VERBATIM at the top of \
the subagent prompt (`#waypostauthorization`): it quotes the operator's authorization (item, \
backlog text, preset body) and the coordinator rules; never paraphrase it, and when its \
`authorization.status` is `unresolved_preset` do not add authorization it lacks. \
Pass an entry's optional `model` and `effort` to the harness subagent launch (Claude Code Agent \
`model` / `effort`; Codex `spawn_agent` `model` / `reasoning_effort`). Omitted fields inherit the \
harness defaults. When an entry carries `recovery`, this is a replacement for the named prior \
Lost attempt; report its `reason` and prior worker/generation rather than implying that old worker \
is still active. Do NOT execute \
them inline in queue order; they are excluded from `selected_queue_prompts`. An inline answer \
never closes one: closeout keeps an echoed dispatch item queued and offers it for dispatch again \
(`#subagentinlineveto`). If this harness cannot spawn a subagent, do not run the item inline: \
leave it queued and say so for that item with a `**Deferred:** no subagent available` line. If \
you judge an item blocked, still name it in the response with why. If a worker sends a \
`[#subagent-operator-request-<unique-id>]` packet, copy the complete packet (request, context, and every choice) verbatim \
into `agent:exchange`, present it from the main session, and return the operator's answer to the \
same worker when the harness can continue it. The closeout that presents a blocking packet must \
also record its concrete operator action in `agent:todo` with `--todo-add`; when the answer and \
worker result complete that action, check it off with `--todo-done`. Never treat a chat answer as \
a tool permission grant; the harness must grant the exact protected action. Keep the generation \
printed by the claim. When the subagent reports back, run `agent-doc queue report-complete <FILE> \
--item <item> --generation <generation> --response '<complete worker response>'`; integrate and close \
the item normally. Empty responses and placeholder-only reports are refused. Use `queue release \
--requeue --generation <generation>` only for explicitly abandoned work.";

/// One exchange prompt delegated by `<!-- agent:exchange subagents -->`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeSubagentDispatch {
    /// The exchange prompt, verbatim.
    pub item: String,
}

pub fn exchange_subagent_dispatch_entries(items: &[String]) -> Vec<ExchangeSubagentDispatch> {
    items
        .iter()
        .map(|item| ExchangeSubagentDispatch { item: item.clone() })
        .collect()
}

/// Guidance for a non-empty `exchange_subagent_dispatch`.
pub const EXCHANGE_SUBAGENT_DISPATCH_GUIDANCE: &str = concat!(
    "The `agent:exchange` marker declares \
subagent intent. For EACH listed exchange prompt, dispatch it NOW to its own background subagent \
(and use a dedicated worktree outside the IDE-watched project if it touches a repository). The \
parent retains this response cycle: review the worker result, answer the prompt in the exchange, \
and perform the normal binary-owned closeout. No queue claim/release applies to exchange prompts. \
Exchange prompts have no preamble, so put these rules in each subagent prompt yourself: ",
    agent_doc_queue::subagent_scratch_isolation_rule!(),
    " ",
    agent_doc_queue::subagent_tmux_isolation_rule!()
);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreflightOutput {
    /// Non-blocking warnings the skill should surface before responding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<PreflightWarning>,
    /// Tmux layout issues found (empty = healthy).
    /// #per-cycle-protocol-output-overhead: omit when empty so a healthy cycle
    /// does not spend per-cycle context bytes on `"layout_issues": []`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_issues: Vec<String>,
    /// Whether an orphaned pending response was recovered and applied.
    pub recovered: bool,
    /// Whether a git commit was made for the previous cycle.
    pub committed: bool,
    /// Closeout syntax that the document requires. Omitted for inline/append
    /// documents so the routine preflight payload stays small.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_contract: Option<PreflightResponseContract>,
    /// Lines from `.agent-doc/claims.log` (truncated after read).
    /// #per-cycle-protocol-output-overhead: omit when empty (the common case).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<String>,
    /// Unified diff text, or `null` if there are no changes.
    pub diff: Option<String>,
    /// True when the snapshot matches the document (no new user input).
    pub no_changes: bool,
    /// `#noopnamefault`: on a `no_changes` cycle, which source preflight read
    /// (live editor buffer or disk), any items waiting in a stopped
    /// `agent:queue`, and the guidance to relay. Agents relay this instead of
    /// inventing a cause such as "save the file".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_changes_explanation:
        Option<agent_doc_queue::no_changes_explanation::NoChangesExplanation>,
    /// Changes detected in linked documents since last cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_changes: Vec<RelatedDocChange>,
    /// Classification of the diff for skill routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_type: Option<String>,
    /// Reason for the diff classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_type_reason: Option<String>,
    /// Annotated diff with content-source markers (`[agent]`, `[user+]`, `[user-]`, `[user~]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotated_diff: Option<String>,
    /// Structured semantic navigation for the same diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_diff: Option<agent_doc_diff::semantic::SemanticDiffSummary>,
    /// Operation manifest for the current turn (`#op-scoped-drift-2`): the
    /// driver node plus the read/write addresses the turn touches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_scope: Option<agent_doc_turn::turn_scope::TurnScope>,
    /// Affectedness classification of this cycle's node ops against `turn_scope`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_affectedness: Option<agent_doc_turn::turn_scope::CycleAffectedness>,
    /// Skill slash commands found in user-added diff lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slash_commands: Vec<String>,
    /// Claude Code built-in commands found in user-added diff lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builtin_commands: Vec<String>,
    /// Natural-language orchestration request detected from the user diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_request: Option<agent_doc_diff::OrchestrationRequest>,
    /// Prompt preset references requested from the changed exchange content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_presets_requested: Vec<String>,
    /// The resolved BODY of each entry in [`Self::prompt_presets_requested`], in
    /// the same order (`#orchestratepresetexpand`).
    ///
    /// Preset bodies live in the document's `prompt_presets` frontmatter, which
    /// preflight has already parsed, resolved through
    /// `resolve_prompt_preset_key`, and validated (a missing preset fails the
    /// cycle before this point). Emitting the bodies with the request is what
    /// makes a second "expansion" command unnecessary: the agent reads the
    /// preset text straight out of the sealed cycle contract.
    ///
    /// The command SKILL.md used to route this through,
    /// `agent-doc orchestrate <FILE> --from-exchange`, never read frontmatter at
    /// all — it parsed the PREVIOUS response into tasks and, worse, abandoned the
    /// live `PreflightStarted` cycle the hook had just sealed for the turn
    /// (`close_open_preflight_handoff_cycle`, correct for a real dispatch, wrong
    /// for an expansion step). Measured 2026-09-20 on `#actionable-review`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_preset_expansions: Vec<PromptPresetExpansion>,
    /// Explicit cross-document backlog targets resolved from prompt/preset text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explicit_backlog_targets: Vec<String>,
    /// Resolved model tier the skill should use to gate this cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_tier: Option<String>,
    /// Hard-gate tier from model component or frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_tier: Option<String>,
    /// Advisory tier computed from diff structural signals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_tier: Option<String>,
    /// Concrete model name from an inline `/model <x>` command in the diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch: Option<String>,
    /// Resolved tier for `model_switch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_switch_tier: Option<String>,
    /// Pending callback requests from `agent-doc cleanup` or other IPC callers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_callbacks: Vec<agent_doc_ipc_protocol::PendingCallback>,
    /// Structured owner-pane self-invocation contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_pane_self_invocation:
        Option<agent_doc_workflow::owner_pane_self_invocation::OwnedPaneSelfInvocation>,
    /// Environment variables from frontmatter `env` field (unexpanded).
    #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
    pub env: indexmap::IndexMap<String, Option<String>>,
    /// True when the backlog component's id order changed between snapshot and current.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub backlog_reordered: bool,
    /// Count of backlog items currently in `[/]` gated state.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub backlog_gated_count: usize,
    /// Count of non-done items currently in `agent:review`.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub review_count: usize,
    /// Count of review items currently in `[/]` gated state.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub review_gated_count: usize,
    /// Opportunistic gated-review verification results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate_verify: Vec<GateVerifyResult>,
    /// `#todoids`: `agent:todo` items whose tracked-work link calls for an
    /// action (operator-checked gates to resolve, finished links to check off).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todo_signals: Vec<TodoSignal>,
    /// Persistent operator instruction from `agent:status sync="..."`.
    /// The agent applies its result through `write --status`; the status marker's
    /// `sync-mode` determines additive managed-block vs protected replace behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_sync: Option<agent_doc_document::status_projection::StatusSyncDirective>,
    /// `#statussyncrevert`: the managed `status:sync` payload no longer holds
    /// the latest value a status writer synced (an editor undo or hand edit put
    /// other text there). The block is derived state owned by `sync`, so this
    /// cycle must re-derive it and write it with `agent-doc status set` even if
    /// the answer equals `synced_payload`; the operator's change does not stick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_sync_revert: Option<agent_doc_document::status_projection::StatusSyncRevert>,
    /// Plain-language instruction for [`Self::status_sync_revert`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_sync_revert_guidance: Option<String>,
    /// Composed `agent:rules` for this turn, in order.
    ///
    /// Every body in each existing `.agent-doc/project.md` comes first, farthest
    /// ancestor first and in file order within that file, then every body in
    /// the session document, in file order. A nested `.git` does not hide an
    /// ancestor project file. Session rules add to project rules. A list item
    /// is one entry. Free text that contains blank lines stays one entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_rules: Vec<String>,
    /// How to apply [`Self::agent_rules`]. Present only when that list is non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_rules_guidance: Option<String>,
    /// Canonical serialized list of user-authored changes that should preempt
    /// or guide the current response cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_intent_prompt_changes: Vec<agent_doc_diff::PromptBearingChange>,
    /// `#chatprompt` (GH #125): operator prompts typed in the harness chat
    /// since the session's last trigger that the document does not record.
    /// They are this cycle's work (also listed in `user_intent_prompt_changes`):
    /// begin the `patch:exchange` response with
    /// `> **Chat prompt (#chatprompt):** <verbatim>` for each, answer it, and
    /// persist through `respond` / `write --commit`. Non-empty means the cycle
    /// is never `no_changes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chat_prompts: Vec<String>,
    /// `#steerbaselineabsorb`: operator prompts a previous turn-boundary
    /// steering report announced ("answer it in the next cycle") whose text an
    /// earlier closeout commit already absorbed into the committed baseline, so
    /// the document diff cannot show them. They are this cycle's work (also
    /// listed in `user_intent_prompt_changes`): answer each in the
    /// `patch:exchange` response. The text is already in the document; do not
    /// re-insert it. Non-empty means the cycle is never `no_changes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absorbed_steering_prompts: Vec<String>,
    /// Legacy compatibility field: inline user edits inside prior agent responses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_annotations: Vec<String>,
    /// Short model name for attribution in `### Re:` response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_model: Option<String>,
    /// Ordered prompt texts from the `agent:queue` component.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_prompts: Vec<String>,
    /// Realtime-selected active queue prompts for this cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_queue_prompts: Vec<String>,
    /// `#closeout-steering`: new queue lines with subagent intent (`#subagents`
    /// tag or a preset expanding to subagent work) that no worker has claimed.
    /// They are NOT in `selected_queue_prompts`: claim each with its
    /// `claim_command`, dispatch it to its own background subagent, then run
    /// `start_command` with the returned worker identity
    /// instead of executing it inline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_subagent_dispatch: Vec<QueueSubagentDispatch>,
    /// Exchange prompt targets delegated by the exchange marker's `subagents`
    /// / `fan-out` attribute.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exchange_subagent_dispatch: Vec<ExchangeSubagentDispatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange_subagent_dispatch_guidance: Option<String>,
    /// How to handle `queue_subagent_dispatch`; present only when non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_subagent_dispatch_guidance: Option<String>,
    /// `#qheadannotation`: operator text attached to a selected id-backed head
    /// (`do [#id]: <question>`), separated from the canonical directive. Each
    /// annotation is an operator directive to answer in this turn alongside the
    /// backlog item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_head_annotations: Vec<agent_doc_queue::queue_head_annotation::QueueHeadAnnotation>,
    /// How to address `queue_head_annotations`; present only when non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_head_annotation_guidance: Option<String>,
    /// Whether the queue is currently active (consuming prompts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_active: Option<bool>,
    /// True when a time-gated start fence defers activation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_deferred: bool,
    /// Raw datetime string from `--- start at <time>` when deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_start_at: Option<String>,
    /// How the queue was activated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_trigger: Option<agent_doc_queue::document_queue::QueueTrigger>,
    /// If non-null, the queue was halted this cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_halted: Option<String>,
    /// True when an accepted controller pause is the effective queue-control state.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_paused: bool,
    /// Controller-recorded pause reason when `queue_paused` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pause_reason: Option<String>,
    /// Count of agent-drainable heads at the active queue head.
    #[serde(default)]
    pub queue_drainable_head_count: usize,
    /// Whether the queue has agent-drainable continuation work this session.
    #[serde(default)]
    pub queue_continuation_required: bool,
    /// Explicit non-stall guidance when continuation is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_continuation_guidance: Option<String>,
    /// `#bgclearpolicy`: whether the document's `agent:queue` resets agent
    /// context between items (`true` by default; `false` when the marker carries
    /// `disable-auto-clear`). Absent when the document has no queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_auto_clear: Option<bool>,
    /// `#bgclearpolicy`: the in-session drain yields this item boundary to the
    /// ready supervisor, which clears the context and dispatches the next head.
    /// When true, `queue_continuation_required` is false and the guidance says
    /// to end the turn without scheduling `/loop`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_auto_clear_yield: bool,
    /// Bounded session-growth / churn advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_accretion: Option<agent_doc_session_accretion::SessionAccretionReport>,
    /// Live finalize-pipeline state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<agent_doc_frontmatter::frontmatter::AgentDocPipeline>,
}

fn is_zero_usize(n: &usize) -> bool {
    *n == 0
}

pub trait PreflightMaintenanceWriteEffects {
    fn record_document_write_provenance(&self, file: &Path, content: &str);

    fn guard_visible_write_expected_current(
        &self,
        file: &Path,
        source: &str,
        expected_current: &str,
    ) -> Result<()>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        source: &str,
    ) -> Result<()>;
}

pub trait PreflightCycleCompletionEffects {
    fn repair(&self, file: &Path) -> Result<agent_doc_turn::repair::RepairOutcome>;

    fn commit(&self, file: &Path) -> Result<bool>;

    /// Replay a stranded retained write over the settled current authority
    /// before the gate decides whether a new cycle may open (`#0dsr`).
    fn recover_retained_document_write(&self, file: &Path) -> Result<bool>;

    /// Whether a retained document write genuinely blocks a new cycle.
    ///
    /// `#retainedclearreactive`: reading this **is** the settlement. The
    /// implementation reads the shared derived verdict, and the controller has
    /// a per-document `Effect` subscribed to that same verdict cell, so a
    /// `Satisfied` intent is cleared as a consequence of the read rather than
    /// by a companion `settle_*` call the gate had to remember. The trait used
    /// to carry that companion (`settle_retained_document_write`, added by
    /// `#preflightsettleparity` because preflight forgot to make it); a third
    /// consumer would have forgotten it too.
    fn retained_document_write(&self, file: &Path) -> bool;

    fn session_interruption(&self, file: &Path) -> Result<Option<String>>;

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>>;
}

/// Whether a prior cycle is evidence about the head it carried
/// (`#adpreflightreentryskip`).
///
/// `#queueskip`'s own rule is "dispatched last cycle and **came back**
/// unconsumed". *Came back* is the load-bearing half: the turn ran and committed
/// a response that simply did not resolve the id, so it left a capture or a
/// response hash behind. A cycle that only ever reached `preflight_started` and
/// was closed without hosting a response proves nothing about its head.
///
/// Accepting one anyway is what let a **second `agent-doc preflight` in the same
/// turn** — a mid-turn re-check, a recovery pass, an agent reading the contract
/// twice — "confirm" the current head as stalled. Repair closes the first
/// preflight's cycle at step 1, so by the time the skip set is derived that
/// seconds-old cycle *is* "the prior cycle". Each extra call then walked the
/// queue forward one head, stamping `⏭️` on work nobody ran and re-pointing
/// `pipeline.turn_id` at a head the turn never touched. Observed 2026-07-26 on
/// `tasks/agent-doc/agent-doc-bugs2.md`: three preflight runs skipped two heads
/// and mis-attributed the turn to a third.
///
/// **A terminal phase is not that evidence, and accepting it reopened the bug
/// this guard exists to close.** Preflight's own step 2 commits the document —
/// including the `🚧`/`⏭️` queue-marker mutation preflight just made — which
/// promotes an *empty* `preflight_started` cycle straight to `committed`. So the
/// re-entrant preflight the guard was written for still satisfied it: reproduced
/// 2026-07-26 on this same document with the guard already in place
/// (`document_cycles` rows `cycle-1785091429188` and `cycle-1785091492165`, both
/// `committed`, neither with a capture in the ledger). Only a capture or a
/// response hash proves a turn ran; a phase proves only that *something*
/// committed, and preflight itself is one of the things that commits.
///
/// The asymmetry settles the tie. A false skip silently drops real work; a false
/// non-skip costs one re-dispatch of a dead ref, which the next *completed*
/// cycle then skips for real. So require the evidence.
fn prior_cycle_hosted_a_turn(prior: &agent_doc_cycle_state_io::CycleState) -> bool {
    prior.capture_id.is_some() || prior.response_sha256.is_some()
}

/// Fold the prior cycle's evidence into the carried skip set.
///
/// Pure so `#adpreflightreentryskip` is testable with fixed inputs instead of a
/// live three-preflight session. `prior_dispatched` is already gated on
/// [`prior_cycle_hosted_a_turn`]; passing `None` models both "no prior cycle"
/// and "the prior cycle never ran".
///
/// **Progress clears the carried set.** A skip says "this head was dispatched
/// and came back unconsumed", which is a statement about a *moment* — usually an
/// unfinished dependency. Retaining it merely because the id is still live makes
/// it permanent: the head is never selected again, so it can never resolve
/// itself out of the set, and an operator's top queue item silently stops being
/// worked. Once any id resolves, the queue has moved and the old stall claim is
/// stale, so re-offer the skipped heads. By the asymmetry above that costs at
/// most one re-dispatch each, and a head that really is still stalled is skipped
/// again by the very next completed cycle.
/// `#presetargdedup`: normalized `#id`s whose ONLY active meaning in the
/// document is a prompt preset (no backlog/review/icebox item carries them).
/// `presets` is the effective map, so project-inherited presets count too
/// (GH #381).
fn preset_only_identity_ids(
    content: &str,
    presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> std::collections::HashSet<String> {
    agent_doc_queue::queue_continuation::preset_only_identity_ids_with_presets(content, presets)
}

/// The tracked id the prior cycle actually dispatched, if any
/// (`#releaseskipblame`).
///
/// `active_queue_heads` records **every** live queue line, not just the one the
/// turn ran: preflight start captures them all, and the write boundary adds any
/// it sees. Taking "the first id-backed head that is not preset-only" therefore
/// walked PAST the head that actually ran whenever that head was a preset
/// invocation (`#gh-fix <url>`) or free text, and blamed the next tracked line
/// instead. Observed 2026-10-01 on `tasks/agent-doc/agent-doc-bugs.md`: each
/// `#gh-fix` turn left the operator's `:pin: #release` marked `⏭️`, so the
/// release was never offered.
///
/// So identify the dispatched head first, then ask whether it is a tracked id.
/// The head carrying the `🚧` in-progress marker is the one the turn ran; with
/// no marker recorded, it is the first head selection would not pass over. A
/// preset-only, free-text, or non-directive head means no tracked id was
/// dispatched, and nothing is skipped: a false non-skip costs one
/// re-dispatch, while a false skip silently drops the operator's work.
///
/// `exempt` (`#claimskipmarker`) holds ids that were never the inline turn's to
/// run: heads a worker holds a live claim on (their `🚧` is the worker's, not
/// the turn's) and heads waiting on the operator (`#queueawaitop`, never
/// dispatched by any agent drain). Neither is a candidate for the blame.
fn prior_dispatched_queue_id(
    heads: &[String],
    carried: &std::collections::HashSet<String>,
    preset_only: &std::collections::HashSet<String>,
    exempt: &std::collections::HashSet<String>,
) -> Option<String> {
    let head_id = |head: &str| {
        let bare = agent_doc_element_queue::strip_priority_markers(head);
        agent_doc_queue::queue_heads::is_do_directive(&bare)
            .then(|| agent_doc_queue::queue_response::queue_prompt_done_id(&bare))
            .flatten()
    };
    // Only `🚧` marks the running head; `⏭️` shares the same leading marker
    // region but means the opposite.
    let in_progress = |head: &str| {
        let head = head.trim();
        let bare = agent_doc_element_queue::strip_priority_markers(head);
        head.strip_suffix(bare.as_str())
            .is_some_and(|markers| markers.contains('🚧'))
    };
    let is_exempt = |head: &str| head_id(head).is_some_and(|id| exempt.contains(&id));
    let dispatched = heads
        .iter()
        .find(|head| in_progress(head) && !is_exempt(head))
        .or_else(|| {
            heads.iter().find(|head| {
                !is_exempt(head) && head_id(head).is_none_or(|id| !carried.contains(&id))
            })
        })?;
    head_id(dispatched).filter(|id| !preset_only.contains(id) && !carried.contains(id))
}

fn advance_skipped_queue_head_ids(
    carried: std::collections::HashSet<String>,
    prior_dispatched: Option<&str>,
    prior_resolved: &std::collections::HashSet<String>,
    current_live_ids: &std::collections::HashSet<String>,
    operator_unskipped: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut fresh = if prior_resolved.is_empty() {
        carried
    } else {
        std::collections::HashSet::new()
    };
    if let Some(id) = prior_dispatched
        && !prior_resolved.contains(id)
        && current_live_ids.contains(id)
    {
        fresh.insert(id.to_string());
    }
    // Clear ids that are no longer live heads or were just consumed.
    fresh.retain(|id| current_live_ids.contains(id) && !prior_resolved.contains(id));
    // `#releaseskipcarried`: an operator who removed a head's `⏭️` overrules the
    // stall claim, including the prior cycle's evidence, which predates that edit.
    fresh.retain(|id| !operator_unskipped.contains(id));
    fresh
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightCycleCompletionMode {
    RecoverInterrupted,
    PreserveFreshOwnedPaneReentry,
}

pub fn enforce_cycle_completion(
    file: &Path,
    effects: &impl PreflightCycleCompletionEffects,
) -> Result<(bool, bool)> {
    enforce_cycle_completion_with_mode(
        file,
        effects,
        PreflightCycleCompletionMode::RecoverInterrupted,
    )
}

pub fn enforce_cycle_completion_with_mode(
    file: &Path,
    effects: &impl PreflightCycleCompletionEffects,
    mode: PreflightCycleCompletionMode,
) -> Result<(bool, bool)> {
    // A retained semantic journal used to resume only when an editor
    // reconnected or closed. A healthy long-lived editor could therefore leave
    // a safe replay stranded forever. Run the typed recovery transition before
    // reading the shared gate; it is fail-closed and preserves a delivery whose
    // authority/disk planes still differ (`#0dsr`).
    let retained_recovered = effects.recover_retained_document_write(file)?;

    // A retained document-write effect is an unfinished durable sink, even
    // when an older repair accidentally made its closeout cycle look terminal.
    // Reading the gate settles a `Satisfied` intent through the controller's
    // subscribed clear (`#retainedclearreactive`). When the recovery attempt
    // leaves it unsettled, fail immediately: calling full session-check from
    // preflight repeats the same controller/editor observations and can stack
    // several RPC deadlines behind the MCP tools/call deadline. Reconciliation
    // is resumable, so one bounded recovery transition is the whole preflight
    // budget for an already-owned durable sink.
    if effects.retained_document_write(file) {
        anyhow::bail!(
            "retained document-write reconciliation is still pending for {file}; preflight performed one bounded recovery transition and will not run session-check inline because nested controller/editor observations can hold the MCP call open. \
             Do not start a new response or replace the queued edit. Let controller reconciliation settle, then run only `agent-doc session-check {file}`.",
            file = file.display(),
        );
    }

    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    if mode == PreflightCycleCompletionMode::PreserveFreshOwnedPaneReentry
        && let Some(open) = state.as_ref()
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if open.reusable_by_reentrant_preflight(
            0,
            now,
            agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_SECS,
        ) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_owned_pane_reentry_preserved file={} cycle_id={} phase={:?} actor_transport_authority=current_pane",
                    file.display(),
                    open.cycle_id,
                    open.phase,
                ),
            );
            return Ok((retained_recovered, false));
        }
    }
    let missing_commit_event = if state.as_ref().map(|state| state.is_open()).unwrap_or(false) {
        None
    } else {
        agent_doc_ops_log_io::detect_write_completed_commit_missing(file)?
    };
    // Fold the binary-owned diagnostic into the open recovery image before
    // repair can transition that cycle to `committed`. Appending it afterwards
    // created a working-tree exchange change whose owning cycle was already
    // closed, so the following commit correctly refused it and the next
    // preflight demanded a duplicate response cycle.
    let ipc_dogfood_note_appended =
        if state.as_ref().is_some_and(|state| state.is_open()) || missing_commit_event.is_some() {
            match append_latest_ipc_dogfood_note(file) {
                Ok(appended) => appended,
                Err(e) => {
                    eprintln!("[preflight] IPC dogfood note warning: {}", e);
                    false
                }
            }
        } else {
            false
        };
    if let Some(event) = missing_commit_event.as_deref() {
        eprintln!(
            "[preflight] WARNING: previous cycle wrote the response but no commit followed ({}) - attempting commit-boundary recovery before diff",
            event
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "write_completed_commit_missing file={} last_event={}",
                file.display(),
                event
            ),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "resume_commit_attempt file={} last_event={}",
                file.display(),
                event
            ),
        );

        let recovered = match effects.repair(file) {
            Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightCycleAbandoned) => {
                // Abandonment preserves an unanswered prompt, not a response
                // image to commit. The next preflight must still see its diff.
                return Ok((true, false));
            }
            Ok(outcome) => outcome.repaired(),
            Err(e) => {
                let message = e.to_string();
                if message
                    .contains(agent_doc_turn::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
                    || message
                        .contains(agent_doc_turn::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR)
                    || message
                        .contains(agent_doc_turn::repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
                {
                    anyhow::bail!("{}", e);
                }
                eprintln!("[preflight] interrupted-cycle repair warning: {}", e);
                false
            }
        };

        let committed = match effects.commit(file) {
            Ok(did_commit) => did_commit,
            Err(e) => {
                eprintln!("[preflight] interrupted-cycle commit warning: {}", e);
                false
            }
        };

        if let Some(reason) = effects.session_interruption(file)? {
            let reason = reason.replace('\n', " ");
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "resume_commit_blocked_drift file={} reason={}",
                    file.display(),
                    reason
                ),
            );
            anyhow::bail!("{}", reason);
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!("resume_commit_success file={}", file.display()),
        );
        return Ok((
            retained_recovered || recovered || ipc_dogfood_note_appended,
            committed,
        ));
    }

    let Some(state) = state else {
        return Ok((retained_recovered, false));
    };
    if !state.is_open() {
        return Ok((retained_recovered, false));
    }

    let ipc_hint = agent_doc_ops_log_io::latest_ipc_proof_diagnostic_hint(file)?
        .map(|hint| format!("; {hint}"))
        .unwrap_or_default();
    eprintln!(
        "[preflight] WARNING: previous cycle `{}` is still `{}` ({}){} - attempting recovery before diff",
        state.cycle_id,
        match state.phase {
            agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
            agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
            agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
            agent_doc_turn::CyclePhase::Committed => "committed",
            agent_doc_turn::CyclePhase::Abandoned => "abandoned",
        },
        state.last_event,
        ipc_hint
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{} file={} cycle_id={} phase={:?} event={}",
            OpsLogEvent::InterruptedCycleDetected,
            file.display(),
            state.cycle_id,
            state.phase,
            state.last_event
        ),
    );
    if matches!(state.phase, agent_doc_turn::CyclePhase::WriteApplied) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "resume_commit_attempt file={} cycle_id={}",
                file.display(),
                state.cycle_id
            ),
        );
    }

    let recovered = match effects.repair(file) {
        Ok(agent_doc_turn::repair::RepairOutcome::StalePreflightCycleAbandoned) => {
            // Abandonment preserves an unanswered prompt, not a response
            // image to commit. The next preflight must still see its diff.
            return Ok((true, false));
        }
        Ok(outcome) => outcome.repaired(),
        Err(e) => {
            let message = e.to_string();
            if message.contains(agent_doc_turn::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR)
                || message.contains(agent_doc_turn::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR)
                || message
                    .contains(agent_doc_turn::repair::EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR)
            {
                anyhow::bail!("{}", e);
            }
            eprintln!("[preflight] interrupted-cycle repair warning: {}", e);
            false
        }
    };
    let committed = match effects.commit(file) {
        Ok(did_commit) => did_commit,
        Err(e) => {
            eprintln!("[preflight] interrupted-cycle commit warning: {}", e);
            false
        }
    };

    let mut self_healed_abandoned = false;
    if let Some(after) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        && after.is_open()
    {
        // #capturebacklogatomic: a ResponseCaptured cycle whose active prompt requested
        // a backlog capture (`requires_backlog_capture`) but recorded no backlog mutation
        // (`!pending_added_this_cycle`), and whose response the recovery commit could not
        // land (`!committed`), is UNRECOVERABLE — replaying it re-trips the backlog gate
        // forever, which used to demand a manual `mv` of the capture/cycle-state aside
        // (the exact dead-end hit live 2026-07-10). The captured response was never
        // committed (regenerable) and the operator prompt that requested the backlog is
        // preserved uncommitted on disk, so self-heal by abandoning the stuck cycle
        // (terminal; repair's `state_is_open` replay guard then skips it) and continue to
        // a clean diff. A fresh cycle re-generates the response with its backlog. This is
        // tightly gated so it only fires on the backlog dead-end, never on an otherwise
        // recoverable capture (which still fails closed below).
        if !committed
            && after.phase == agent_doc_turn::CyclePhase::ResponseCaptured
            && after.requires_backlog_capture
            && !after.pending_added_this_cycle
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_abandoned_uncommittable_backlog_capture file={} cycle={} reason=requires_backlog_capture_unsatisfiable_commit_refused recovery=abandon_and_regenerate",
                    file.display(),
                    after.cycle_id
                ),
            );
            match agent_doc_cycle_state_io::mark_abandoned(
                file,
                "uncommittable_backlog_capture_self_heal",
                None,
                None,
            ) {
                Ok(_) => self_healed_abandoned = true,
                Err(e) => {
                    eprintln!("[preflight] self-heal abandon warning: {e}");
                }
            }
        }
    }
    if !self_healed_abandoned
        && let Some(after) = agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        && after.is_open()
    {
        let marker_note = if matches!(after.phase, agent_doc_turn::CyclePhase::PreflightStarted) {
            effects
                .detect_bypassed_response_write(file)?
                .map(|marker| format!("; found likely direct response patchback: {}", marker))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let ipc_hint = agent_doc_ops_log_io::latest_ipc_proof_diagnostic_hint(file)?
            .map(|hint| format!("; {hint}"))
            .unwrap_or_default();
        anyhow::bail!(
            "previous cycle `{}` is still `{}` after recovery/commit ({}){}{}",
            after.cycle_id,
            match after.phase {
                agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
                agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
                agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
                agent_doc_turn::CyclePhase::Committed => "committed",
                agent_doc_turn::CyclePhase::Abandoned => "abandoned",
            },
            after.last_event,
            marker_note,
            ipc_hint
        );
    }

    if matches!(state.phase, agent_doc_turn::CyclePhase::WriteApplied) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!("resume_commit_success file={}", file.display()),
        );
    }

    Ok((
        retained_recovered || recovered || ipc_dogfood_note_appended || self_healed_abandoned,
        committed,
    ))
}

pub fn append_latest_ipc_dogfood_note(file: &Path) -> Result<bool> {
    // Only agent-doc's OWN dogfood sessions may have an IPC diagnostic folded into
    // the document exchange. A user's document (e.g. a recruiting doc that merely
    // lives in a superproject alongside `src/agent-doc`) must never have binary
    // IPC diagnostics written into its content - for those the diagnostic stays in
    // ops.log only. Without this gate the interrupted-cycle recovery pollutes and
    // re-duplicates diagnostics into real user documents. Single source of truth:
    // `project_controller::rpc::dogfood_agent_doc_crate_root` (None => not dogfood).
    if agent_doc_controller_io::project_controller::dogfood_agent_doc_crate_root(file).is_none() {
        return Ok(false);
    }
    let Some(diagnostic) = agent_doc_ops_log_io::latest_ipc_proof_diagnostic(file)? else {
        return Ok(false);
    };
    append_ipc_dogfood_note_for_diagnostic(file, &diagnostic)
}

pub fn append_ipc_dogfood_note_for_diagnostic(file: &Path, diagnostic: &str) -> Result<bool> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} for IPC dogfood note", file.display()))?;
    let note = agent_doc_workflow::preflight_policy::format_ipc_dogfood_note(diagnostic);
    let Some(updated) = agent_doc_element_exchange::append_deduped_content_to_exchange(
        &content, diagnostic, &note,
    )?
    else {
        return Ok(false);
    };
    std::fs::write(file, updated)
        .with_context(|| format!("failed to write IPC dogfood note to {}", file.display()))?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!("ipc_dogfood_note_appended file={}", file.display()),
    );
    eprintln!(
        "[preflight] IPC dogfood note appended to {}",
        file.display()
    );
    Ok(true)
}

/// Resolve the live finalize-pipeline view surfaced in preflight output
/// (`#fmrunid-wire`). Cycle-state is authoritative; the document
/// `agent_doc_pipeline:` frontmatter block is only a fallback hint when no live
/// cycle-state exists (e.g. a crash that wiped `.agent-doc/state` but left the
/// document mirror behind). Returns `None` when neither is present.
pub fn resolve_pipeline_state(
    file: &Path,
) -> Result<Option<agent_doc_frontmatter::frontmatter::AgentDocPipeline>> {
    if let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? {
        return Ok(Some(state.to_pipeline()));
    }
    let current = std::fs::read_to_string(file).unwrap_or_default();
    Ok(match agent_doc_frontmatter::frontmatter::parse(&current) {
        Ok((fm, _)) if !fm.pipeline.is_empty() => Some(fm.pipeline),
        _ => None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct PendingMaintenanceReport {
    pub reordered: bool,
    pub backlog_gated_count: usize,
    pub review_count: usize,
    pub review_gated_count: usize,
    pub legacy_gated_in_backlog_count: usize,
    pub terminal_debris_lines_pruned: usize,
    /// `#todoids`: todo link signals for the preflight output.
    pub todo_signals: Vec<TodoSignal>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PendingMaintenanceOptions {
    force_disk: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct GateVerifyOptions {
    force_disk: bool,
}

/// Run pending-component maintenance: lazy backfill, reap `[x]`, and reorder detection.
///
/// Any write-through (backfill / reap) is persisted and committed in the same pass.
/// Silent no-op when the document has no tracked-work component.
pub fn run_pending_maintenance(
    file: &Path,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<PendingMaintenanceReport> {
    run_pending_maintenance_with_options(file, PendingMaintenanceOptions::default(), write_effects)
}

pub fn run_pending_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<PendingMaintenanceReport> {
    run_pending_maintenance_with_options(
        file,
        PendingMaintenanceOptions { force_disk: true },
        write_effects,
    )
}

fn run_pending_maintenance_with_options(
    file: &Path,
    options: PendingMaintenanceOptions,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<PendingMaintenanceReport> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(PendingMaintenanceReport::default()),
    };
    let tracked_surfaces: Vec<String> = components
        .iter()
        .filter(|c| is_tracked_work_component(&c.name))
        .map(|c| c.name.clone())
        .collect();
    if tracked_surfaces.is_empty() {
        return Ok(PendingMaintenanceReport::default());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let doc_id = agent_doc_fs::document_state_hash(&canonical)
        .unwrap_or_else(|_| file.display().to_string());

    let mut current_content = content.clone();
    let mut snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)?;
    // Reorder detection (step 4) compares the file's backlog order against the
    // snapshot as it was at cycle start. Capture it before the loop re-syncs the
    // snapshot to the file (#pending-gate-snapshot-desync), otherwise the synced
    // snapshot masks a same-cycle reorder.
    let snapshot_at_start = snapshot_content.clone();
    let mut mutated = false;
    // `#retaineddeferisnotafailure`: set when the maintenance write was deferred
    // rather than persisted. The reap verification below asserts that completed
    // items are GONE from the working tree; when the write never reached disk
    // that assertion is guaranteed to fail and would abort preflight anyway —
    // reinstating the exact wedge the defer exists to avoid.
    let mut maintenance_deferred = false;
    // #pending-gate-snapshot-desync: the snapshot may need re-syncing to the
    // file's tracked surfaces even when maintenance itself makes no change —
    // the write phase can apply --pending-gate / --pending-edit / --review-add
    // to the file without those reaching the content_ours snapshot. Tracked
    // separately from `mutated` so the snapshot is re-saved without an
    // unnecessary working-tree rewrite.
    let mut snapshot_mutated = false;
    let mut terminal_debris_lines_pruned = 0usize;
    if let Some(pruned) =
        agent_doc_element_done::prune_proven_redundant_terminal_debris(&current_content)
    {
        terminal_debris_lines_pruned = pruned.removed_line_count;
        current_content = pruned.content;
        mutated = true;
        eprintln!(
            "[preflight] terminal boundary: pruned {} proven-redundant debris line(s)",
            terminal_debris_lines_pruned
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "terminal_debris_pruned file={} lines={} authority=maintenance_convergence",
                file.display(),
                terminal_debris_lines_pruned
            ),
        );
    }
    if let Some(ref mut snapshot) = snapshot_content
        && let Some(pruned) =
            agent_doc_element_done::prune_proven_redundant_terminal_debris(snapshot)
    {
        *snapshot = pruned.content;
        snapshot_mutated = true;
    }
    let mut saw_completed_before = false;
    advise_misplaced_legacy_done_archive(file, &content);
    let already_done_ids = collect_agent_done_ids(file, &content)?;
    let done_item_texts = collect_agent_done_texts(file, &content)?;

    for surface in &tracked_surfaces {
        let components = agent_doc_element::element::parse(&current_content)
            .with_context(|| format!("failed to parse components while maintaining {}", surface))?;
        let comp = components
            .into_iter()
            .find(|c| component_matches_tracked_surface(&c.name, surface))
            .with_context(|| format!("document is missing the {} component", surface))?;
        let body = comp.content(&current_content);

        let mut current_body = body.to_string();
        let surface_label = maintenance_surface_label(surface);
        saw_completed_before |=
            !agent_doc_element_backlog::backlog::completed_items(&current_body).is_empty();

        let (after_backfill, changed) = agent_doc_element_backlog::backlog::backfill(
            &current_body,
            &doc_id,
            &std::collections::HashSet::new(),
        );
        if changed {
            eprintln!(
                "[preflight] {}: backfilled missing hash ids / checkboxes",
                surface_label
            );
            current_body = after_backfill;
            mutated = true;
        }

        // #reviewrm: collapse identical same-id entries an interleaved finalize
        // can leave behind (the duplicate `[/] #id` pair preflight flags as
        // preset_item_id_collision). Only exact duplicates are removed; distinct
        // items that merely share an id are preserved so the ambiguity warning
        // still surfaces.
        let (after_dedupe, deduped_ids) =
            agent_doc_element_backlog::backlog::op_dedupe_identical_items(&current_body);
        if !deduped_ids.is_empty() {
            eprintln!(
                "[preflight] {}: deduped {} duplicate same-id entr{}: {}",
                surface_label,
                deduped_ids.len(),
                if deduped_ids.len() == 1 { "y" } else { "ies" },
                deduped_ids.join(", ")
            );
            current_body = after_dedupe;
            mutated = true;
        }

        if should_reap_already_done_mirrors(surface) && !already_done_ids.is_empty() {
            // `#donemirrorbytext`: an id match alone is not proof that an active
            // item re-adds completed work. Reap only items whose text agrees with
            // the archived entry; keep (and report) id collisions.
            let (_, id_matches) = agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                &current_body,
                &already_done_ids,
            );
            let mirror_ids: std::collections::HashSet<String> = id_matches
                .iter()
                .filter(|item| {
                    let mirrored = done_item_texts
                        .get(&item.id.to_ascii_lowercase())
                        .is_none_or(|done| {
                            agent_doc_element_done::done_entry_matches_item(done, &item.text)
                        });
                    if !mirrored {
                        eprintln!(
                            "[preflight] {}: kept #{} although a done entry shares its id: the texts describe different work (id collision)",
                            surface_label, item.id
                        );
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "done_mirror_reap_skipped_id_collision file={} id={} texts_differ=true",
                                file.display(),
                                item.id
                            ),
                        );
                    }
                    mirrored
                })
                .map(|item| item.id.clone())
                .collect();
            let (after_mirror_reap, mirror_items) =
                agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                    &current_body,
                    &mirror_ids,
                );
            if !mirror_items.is_empty() {
                let removed_ids: Vec<String> = mirror_items.iter().map(|i| i.id.clone()).collect();
                eprintln!(
                    "[preflight] {}: reaped {} already-done mirror item(s): {}",
                    surface_label,
                    mirror_items.len(),
                    removed_ids.join(", ")
                );
                current_body = after_mirror_reap;
                mutated = true;
            }
        }

        let mut removed_items = Vec::new();
        if should_reap_ops_proof_completions(surface) {
            // #opsproof-falsepos: never auto-archive an item that was added this
            // same cycle. A brand-new add is absent from the post-commit snapshot
            // captured at cycle start; such items describe just-landed dependency
            // work and must be closed explicitly, not reaped on the cycle they
            // appear. Only apply the guard when we have a snapshot baseline to
            // compare against (untracked scaffold docs have none).
            let snapshot_baseline = snapshot_at_start
                .as_deref()
                .filter(|s| !s.trim().is_empty());
            let snapshot_ids = snapshot_baseline.map(|snap| {
                agent_doc_element_backlog::ops_proof::surface_pending_ids(snap, surface)
            });
            // `#opsproof-samecycle-add`: the snapshot baseline alone is not enough.
            // In the `write`/`finalize` path the same invocation that adds an item
            // via `--review-add` / `--pending-add*` also re-syncs the on-disk
            // snapshot, so a brand-new same-cycle add is already present in
            // `snapshot_ids` and the snapshot test cannot exclude it. Cross-check
            // the ids cycle-state recorded as added this cycle and never reap them.
            let added_this_cycle = agent_doc_cycle_state_io::pending_added_ids(file);
            let ops_proof_completions: Vec<
                agent_doc_element_backlog::ops_proof::OpsProofCompletion,
            > = agent_doc_element_backlog::ops_proof::ops_proof_completion_candidates(
                &current_body,
            )
            .into_iter()
            .filter(|candidate| {
                snapshot_ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&candidate.id))
            })
            .filter(|candidate| !added_this_cycle.contains(&candidate.id))
            .collect();
            if !ops_proof_completions.is_empty() {
                let evidence_by_id: HashMap<String, String> = ops_proof_completions
                    .iter()
                    .map(|candidate| (candidate.id.clone(), candidate.evidence.clone()))
                    .collect();
                let ids: HashSet<String> = ops_proof_completions
                    .iter()
                    .map(|candidate| candidate.id.clone())
                    .collect();
                let (after_ops_proof_reap, mut ops_proof_items) =
                    agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                        &current_body,
                        &ids,
                    );
                if !ops_proof_items.is_empty() {
                    let removed_ids: Vec<String> =
                        ops_proof_items.iter().map(|i| i.id.clone()).collect();
                    for item in &mut ops_proof_items {
                        item.state = agent_doc_element_backlog::backlog::PendingState::Done;
                        item.gate_type = None;
                    }
                    eprintln!(
                        "[preflight] {}: auto-completed {} ops-proof item(s): {}",
                        surface_label,
                        ops_proof_items.len(),
                        removed_ids.join(", ")
                    );
                    for item in &ops_proof_items {
                        let evidence = evidence_by_id
                            .get(&item.id)
                            .map(String::as_str)
                            .unwrap_or("ops_proof");
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "auto_complete_ops_proof file={} id={} surface={} evidence={}",
                                file.display(),
                                item.id,
                                surface_label,
                                evidence
                            ),
                        );
                    }
                    let _ = agent_doc_cycle_state_io::record_pending_done_ids(file, &removed_ids);
                    let _ = agent_doc_cycle_state_io::record_reaped_pending_ids(file, &removed_ids);
                    let _ = agent_doc_cycle_state_io::mark_pending_mutations(file);
                    current_body = after_ops_proof_reap;
                    mutated = true;
                    removed_items.extend(ops_proof_items);
                }
            }
        }

        let (after_reap, reaped_items) =
            agent_doc_element_backlog::backlog::reap_with_items(&current_body)?;
        if !reaped_items.is_empty() {
            let removed_ids: Vec<String> = reaped_items.iter().map(|i| i.id.clone()).collect();
            eprintln!(
                "[preflight] {}: reaped {} item(s): {}",
                surface_label,
                reaped_items.len(),
                removed_ids.join(", ")
            );
            let _ = agent_doc_cycle_state_io::record_reaped_pending_ids(file, &removed_ids);
            current_body = after_reap;
            mutated = true;
        }
        removed_items.extend(reaped_items);

        // Priority sort (#backlog-priority-attribute, #reviewpriority): when the
        // backlog/icebox/review marker carries `priority`, stable-sort items by
        // their per-item `priority=<1..9>` token (1 = highest; absent = lowest) so
        // a downstream `agent:queue` sync inherits the prioritized order (review
        // has no queue sync; its sort only orders the gated list in place).
        if comp.attrs.contains_key("priority")
            && let Some(sorted) =
                agent_doc_element_backlog::backlog::sort_by_priority(&current_body)
        {
            eprintln!("[preflight] {}: sorted by priority", surface_label);
            current_body = sorted;
            mutated = true;
        }

        // Re-sync the snapshot's tracked surface to the file's body whenever the
        // two diverge — even if maintenance made no change to it this pass. The
        // write phase persists --pending-gate / --pending-edit / --review-add to
        // the file but saves the content_ours snapshot (baseline + response)
        // before those mutations, so a pure gate/edit/review-add would otherwise
        // leave the snapshot stale and the mutation stranded as post-commit drift
        // (#pending-gate-snapshot-desync). --done already reaches this via reap,
        // which sets `mutated`; this also covers the no-reap mutations.
        if let Some(ref mut snap_content) = snapshot_content {
            let snap_comps = agent_doc_element::element::parse(snap_content).ok();
            if let Some(snap_comp) = snap_comps.and_then(|cs| {
                cs.into_iter()
                    .find(|c| component_matches_tracked_surface(&c.name, surface))
            }) {
                let snap_body = snap_comp.content(snap_content).to_string();
                if snap_body != current_body {
                    *snap_content = snap_comp.replace_content(snap_content, &current_body);
                    snapshot_mutated = true;
                }
                if !removed_items.is_empty()
                    && let Some(archived) =
                        agent_doc_element_backlog_io::done_archive::archive_pending_done(
                            file,
                            snap_content,
                            &removed_items,
                        )?
                {
                    *snap_content = archived;
                    snapshot_mutated = true;
                }
            } else {
                log_snapshot_recovery_warning(
                    file,
                    "pending maintenance tracked-surface sync",
                    format!("snapshot is missing the {} component", surface),
                );
            }
        }

        if current_body == body {
            continue;
        }

        current_content = comp.replace_content(&current_content, &current_body);
        if !removed_items.is_empty()
            && let Some(archived) =
                agent_doc_element_backlog_io::done_archive::archive_pending_done(
                    file,
                    &current_content,
                    &removed_items,
                )?
        {
            current_content = archived;
        }
    }

    if let Some(archived) = agent_doc_element_backlog_io::done_archive::archive_pending_done(
        file,
        &current_content,
        &[],
    )? && archived != current_content
    {
        current_content = archived;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(archived) = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            file,
            snap_content,
            &[],
        )?
        && archived != *snap_content
    {
        *snap_content = archived;
        snapshot_mutated = true;
    }

    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(
            &current_content,
        )?
    {
        eprintln!("[preflight] status: reconciled stale top-backlog marker");
        current_content = reconciled;
        mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(
                snap_content,
            )?
    {
        *snap_content = reconciled;
        snapshot_mutated = true;
    }

    // `#queuehandoffstatus`: a queue made entirely (or mostly) of 📥 rows can
    // otherwise look inert even though every row is a live coordinator obligation.
    // Project one aggregate line into agent:status; the pane title's existing
    // turn-active marker remains the truthful signal that a coordinator turn is
    // currently executing, so no synthetic actionable queue row is invented.
    let reported_count =
        match agent_doc_queue_io::queue_claim::claim_states_for_content(file, &current_content) {
            Ok(claims) => {
                let mut reported_identities = claims
                    .into_iter()
                    .filter(|claim| {
                        claim.state == agent_doc_queue::queue_claim::QueueClaimState::Reported
                    })
                    .map(|claim| claim.identity)
                    .collect::<std::collections::HashSet<_>>();
                reported_identities.extend(
                    agent_doc_queue_io::claim_release_receipt::reported_head_texts_for_content(
                        file,
                        &current_content,
                    )
                    .into_iter()
                    .map(|head| agent_doc_queue::queue_claim::claim_identity(&head)),
                );
                Some(reported_identities.len())
            }
            Err(err) => {
                // An uncertain ledger must never erase a previously projected handoff.
                eprintln!("[preflight] status: failed to read reported queue claims ({err:#})");
                None
            }
        };
    if let Some(reported_count) = reported_count {
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_reported_queue_status_content(
                &current_content,
                reported_count,
            )?
        {
            eprintln!(
                "[preflight] status: reconciled reported queue handoff count={reported_count}"
            );
            current_content = reconciled;
            mutated = true;
        }
        if let Some(ref mut snap_content) = snapshot_content
            && let Some(reconciled) =
                agent_doc_document::status_projection::reconcile_reported_queue_status_content(
                    snap_content,
                    reported_count,
                )?
        {
            *snap_content = reconciled;
            snapshot_mutated = true;
        }
    }

    let stale_marker_before = (
        current_content.clone(),
        snapshot_content.clone(),
        mutated,
        snapshot_mutated,
    );
    let mut stale_supervisor_marker_mutated = false;
    // Historical versions wrote an operator-facing stale-supervisor marker into
    // the session document. Staleness now schedules an automatic safe-boundary
    // recycle at every turn stage, so document maintenance only removes that
    // legacy marker and never makes supervisor health part of user content.
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_stale_supervisor_status_content(
            &current_content,
            false,
        )?
    {
        eprintln!("[preflight] status: cleared legacy stale-supervisor marker");
        current_content = reconciled;
        mutated = true;
        stale_supervisor_marker_mutated = true;
    }
    if let Some(ref mut snap_content) = snapshot_content
        && let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_stale_supervisor_status_content(
                snap_content,
                false,
            )?
    {
        *snap_content = reconciled;
        snapshot_mutated = true;
        stale_supervisor_marker_mutated = true;
    }

    // 3. Persist any mutations to the working tree file and/or the snapshot.
    //    Writing to both (surgically, via component replace) keeps the two in
    //    sync so the upcoming step-2 `git::commit` stages the reaped+archived
    //    snapshot in a single commit. We no longer call `git::commit` here —
    //    see #64mb: calling commit inside maintenance produced a second commit
    //    per preflight whenever anything mutated. The snapshot is saved
    //    independently of the file write so a write-phase pending mutation that
    //    only diverged the snapshot (gate/edit/review-add) is still committed
    //    rather than stranded (#pending-gate-snapshot-desync).
    if mutated {
        // `#fcc0`: converge the reconciled document through the editor IPC when a
        // live JB listener is active so per-cycle pending maintenance never raises a
        // `File Cache Conflict`; `content` is the pre-maintenance on-disk baseline.
        // Falls back to the same plain disk write otherwise. The post-write reap
        // verification below reads `current_content` (not disk), so converging here
        // introduces no read-after-write race.
        let persist_result = persist_pending_maintenance_doc(
            file,
            &content,
            &current_content,
            "pending_maintenance",
            options.force_disk,
            write_effects,
        );
        if let Err(err) = persist_result {
            let err_message = err.to_string();
            let deferable_status_error = err_message.contains("failed to resolve editor authority")
                || err_message.contains("editor authority unavailable")
                || err_message.contains("editor convergence did not complete");
            // #realtime-maintenance-defer: pending maintenance (mirror reap,
            // dedupe, backfill, status-marker reconcile) is idempotent
            // bookkeeping — it is re-derived from scratch every preflight. When
            // the visible write cannot land because the realtime editor buffer
            // was resolved but is mid-reconcile (the typing debounce has not
            // settled, or the last committed response is still being reconciled
            // back into the live buffer), that drift belongs to the realtime
            // document model, NOT to preflight. Failing the whole preflight here
            // strands the operator even though they are not typing. Defer the
            // maintenance write to a later cycle and continue so the diff /
            // realtime-steering feed still reaches the agent this cycle; the reap
            // re-applies once the buffer is idle.
            //
            // This is deliberately NARROWER than `deferable_status_error`: an
            // *unresolvable* editor authority (unreachable relay/listener) must
            // still fail closed so a closeout never writes behind an active
            // listener (see `force_disk_closeout_pending_maintenance_bypasses_active_listener`).
            // Realtime drift means the editor WAS resolved and is simply busy.
            let deferable_realtime_drift = err_message
                .contains("document changed after the response merge was computed")
                || err_message.contains("editor typing did not settle");
            // `#retaineddeferisnotafailure`: a retained-write refusal is the SAME
            // class, and missing it wedged turn admission outright.
            //
            // `converge_or_disk_write` reports this as `Err`, but the write did
            // not fail: `try_editor_converge` already returned true, so the
            // maintenance content is in the editor's CRDT authority and disk was
            // deliberately untouched. Only the secondary snapshot/commit boundary
            // is waiting for the delivery projection to converge. Every other
            // branch of the same guard already defers — an unconverged projection
            // is the one that did not, because the predicate matched prose
            // instead of the token the refusal actually stamps.
            //
            // Observed 2026-09-12 on `tasks/agent-doc/agent-doc-bugs.md`: the
            // relay logged `delivery_converged=true delivery_version=19` in the
            // same second the guard observed `18`, so a 100ms projection-edge
            // grace that gets ~2 IPC round trips deferred a projection that
            // converged immediately afterwards. That deferral is correct on its
            // own; escalating it to a preflight failure is not. Preflight refused
            // admission, so `/agent-doc <FILE>` emitted no cycle contract at all
            // and the session could not start — a whole session lost to
            // idempotent bookkeeping that is re-derived from scratch next cycle.
            //
            // Narrow deliberately: the retained-write class spans three refusals
            // and only this one has a live replica. An attached editor with no
            // registered replica, and editor sync pending, both mean the editor
            // authority is UNREACHABLE — the case
            // `force_disk_closeout_pending_maintenance_bypasses_active_listener`
            // exists to keep failing closed, so a closeout never continues behind
            // an active listener. Deferring those too made that test go green in
            // the wrong direction.
            let deferable_retained_projection =
                agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(
                    &err_message,
                );
            if stale_supervisor_marker_mutated && !stale_marker_before.2 && deferable_status_error {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "stale_supervisor_status_update_deferred file={} source=pending_maintenance error={}",
                        file.display(),
                        err_message.replace('\n', " ")
                    ),
                );
                eprintln!(
                    "[preflight] status: deferred legacy stale-supervisor marker removal for {}: {}",
                    file.display(),
                    err
                );
                current_content = stale_marker_before.0;
                snapshot_content = stale_marker_before.1;
                mutated = stale_marker_before.2;
                snapshot_mutated = stale_marker_before.3;
            } else if deferable_realtime_drift || deferable_retained_projection {
                let reason = if deferable_retained_projection {
                    "retained_delivery_projection"
                } else {
                    "realtime_buffer_busy"
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_maintenance_deferred_{reason} file={} source=pending_maintenance error={}",
                        file.display(),
                        err_message.replace('\n', " ")
                    ),
                );
                eprintln!(
                    "[preflight] pending: deferred maintenance write for {} ({reason}; not aborting preflight): {}",
                    file.display(),
                    err
                );
                // Revert to the pre-maintenance baseline so nothing is
                // half-persisted: the visible write either failed before touching
                // the file or was retained by the delivery projection, and
                // skipping the snapshot save keeps the two in sync. Maintenance
                // is re-derived from scratch next cycle either way.
                current_content = content.clone();
                snapshot_content = snapshot_at_start.clone();
                mutated = false;
                snapshot_mutated = false;
                maintenance_deferred = true;
            } else {
                return Err(err);
            }
        }
    }
    if (mutated || snapshot_mutated)
        && let Some(snap_content) = &snapshot_content
        && let Err(e) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            snap_content,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] pending: snapshot sync warning: {}", e);
    }

    if saw_completed_before && !maintenance_deferred {
        let persisted_content = if mutated {
            current_content.clone()
        } else {
            std::fs::read_to_string(file)
                .with_context(|| format!("failed to verify reap in {}", file.display()))?
        };
        ensure_no_completed_tracked_items(&persisted_content, "working tree")?;

        match agent_doc_snapshot_io::load_document_baseline(file) {
            Ok(Some(snapshot_content)) => {
                if let Err(err) = ensure_no_completed_tracked_items(&snapshot_content, "snapshot") {
                    log_snapshot_recovery_warning(
                        file,
                        "pending maintenance reap verification",
                        err,
                    );
                }
            }
            Ok(None) => log_snapshot_recovery_warning(
                file,
                "pending maintenance reap verification",
                "snapshot is missing after completed tracked items were reaped",
            ),
            Err(err) => {
                log_snapshot_recovery_warning(file, "pending maintenance reap verification", err)
            }
        }
    }

    // 4. Reorder detection: compare the cycle-start snapshot's pending component
    //    to the current body. Uses the pre-sync snapshot (`snapshot_at_start`)
    //    rather than re-loading from disk, since step 3 may have re-synced the
    //    on-disk snapshot to the file (#pending-gate-snapshot-desync) which would
    //    otherwise hide a same-cycle reorder.
    let current_body = tracked_body_for_reorder(&current_content);
    let reordered = match snapshot_at_start {
        Some(snap) => {
            let snap_comp = agent_doc_element::element::parse(&snap)
                .ok()
                .and_then(|comps| comps.into_iter().find(|c| is_backlog_component(&c.name)));
            if let (Some(sc), Some(current_body)) = (snap_comp, current_body) {
                let snap_body = &snap[sc.open_end..sc.close_start];
                agent_doc_element_backlog::backlog::detect_reorder(snap_body, current_body)
                    .is_some()
            } else {
                false
            }
        }
        None => false,
    };
    if reordered {
        eprintln!("[preflight] backlog: reorder detected (skill must not reorder this cycle)");
    }

    // 5. Count legacy gated items in backlog and review items in review.
    let backlog_gated_count = current_body
        .map(|body| {
            let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
            items
                .iter()
                .filter(|i| {
                    matches!(
                        i.state,
                        agent_doc_element_backlog::backlog::PendingState::Gated
                    )
                })
                .count()
        })
        .unwrap_or(0);
    if backlog_gated_count > 0 {
        eprintln!("[preflight] backlog: {} gated item(s)", backlog_gated_count);
    }

    let (review_count, review_gated_count) = review_counts(&current_content);
    if review_count > 0 {
        eprintln!(
            "[preflight] review: {} item(s), {} gated",
            review_count, review_gated_count
        );
    }

    // 6. `#todoids`: operator todo checks at a cheap, deterministic event.
    let todo_signals = todo_signals_for_document(file, &current_content);
    for signal in &todo_signals {
        eprintln!(
            "[preflight] todo: {} {}: {}",
            signal.kind, signal.todo, signal.action
        );
    }

    Ok(PendingMaintenanceReport {
        reordered,
        backlog_gated_count,
        review_count,
        review_gated_count,
        legacy_gated_in_backlog_count: backlog_gated_count,
        terminal_debris_lines_pruned,
        todo_signals,
    })
}

/// Completed-work ids for `file`: the inline `agent:done` bodies plus every
/// done archive [`read_done_archive_sources`] locates.
///
/// GH #325: archives resolve through the writer's resolver
/// (`done_archive::read_done_archive_sources`), never `project_root.join(raw)`.
/// The raw join read the project root directory for a bare `archive` (the
/// documented default) and the literal `<root>/document:x.done.md` for an
/// anchored value, so done-mirror suppression and the preflight done-strike
/// treated every externally archived item as never done.
///
/// [`read_done_archive_sources`]: agent_doc_element_backlog_io::done_archive::read_done_archive_sources
fn collect_agent_done_ids(file: &Path, content: &str) -> Result<std::collections::HashSet<String>> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Ok(ids);
    };
    for comp in &components {
        if !agent_doc_element::element::is_backlog_done_component(&comp.name) {
            continue;
        }
        for id in agent_doc_element_done::collect_done_component_own_ids(content, comp) {
            ids.insert(id);
        }
    }
    for source in
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content)?
    {
        ids.extend(agent_doc_element_done::collect_done_item_own_ids(
            &source.content,
        ));
    }
    Ok(ids)
}

/// `#donemirrorbytext`: done id -> recorded text, from the same sources as
/// [`collect_agent_done_ids`]. Inline entries win over archived ones.
fn collect_agent_done_texts(
    file: &Path,
    content: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut texts = std::collections::HashMap::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Ok(texts);
    };
    for comp in &components {
        if !agent_doc_element::element::is_backlog_done_component(&comp.name) {
            continue;
        }
        for (id, text) in agent_doc_element_done::collect_done_item_texts(comp.content(content)) {
            texts.entry(id).or_insert(text);
        }
    }
    for source in
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content)?
    {
        for (id, text) in agent_doc_element_done::collect_done_item_texts(&source.content) {
            texts.entry(id).or_insert(text);
        }
    }
    Ok(texts)
}

/// GH #325: name, once per stale file, the archive 0.35.521–0.35.523 wrote at
/// `<doc dir>/<project-relative archive path>`. No resolver reads it, so it
/// otherwise sits in HEAD as a second archive nobody notices. Advisory only:
/// the file is never moved or deleted, and a failure here never blocks
/// preflight.
fn advise_misplaced_legacy_done_archive(file: &Path, content: &str) {
    let legacy = match agent_doc_element_backlog_io::done_archive::misplaced_legacy_done_archive(
        file, content,
    ) {
        Ok(Some(legacy)) => legacy,
        Ok(None) | Err(_) => return,
    };
    let Some(root) = file
        .canonicalize()
        .ok()
        .and_then(|canonical| agent_doc_project_root_io::project_root_containing(&canonical))
    else {
        return;
    };
    let Ok(key) = agent_doc_fs::document_state_hash(&legacy) else {
        return;
    };
    let marker_dir = root.join(".agent-doc/advisories");
    let marker = marker_dir.join(format!("legacy-done-archive-{key}"));
    if marker.exists() {
        return;
    }
    let message = format!(
        "misplaced legacy done archive {} (written by agent-doc 0.35.521-0.35.523; no resolver reads it). Merge any entries missing from the resolved archive, then remove it (GH #325)",
        legacy.display()
    );
    eprintln!("[preflight] done archive: {message}");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "legacy_done_archive_advisory file={} legacy={}",
            file.display(),
            legacy.display()
        ),
    );
    if std::fs::create_dir_all(&marker_dir).is_ok() {
        let _ = std::fs::write(&marker, format!("{}\n", legacy.display()));
    }
}

fn snapshot_proves_queue_was_active(file: &Path) -> bool {
    let Ok(Some(snapshot_content)) = agent_doc_snapshot_io::load_document_baseline(file) else {
        return false;
    };
    let Ok((fm, _)) = frontmatter::parse(&snapshot_content) else {
        return false;
    };
    if fm.queue_active.unwrap_or(false) {
        return true;
    }
    let Ok(components) = agent_doc_element::element::parse(&snapshot_content) else {
        return false;
    };
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return false;
    };
    let body = &snapshot_content[queue_component.open_end..queue_component.close_start];
    let Ok(entries) = agent_doc_queue::document_queue::parse(body) else {
        return false;
    };
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    agent_doc_queue::document_queue::resolve_activation(&entries, has_auto, false, false).active
}

fn persist_pending_maintenance_doc(
    file: &Path,
    current: &str,
    target: &str,
    source: &str,
    force_disk: bool,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<()> {
    if force_disk {
        std::fs::write(file, target)
            .with_context(|| format!("{source}: failed to write {}", file.display()))?;
        write_effects.record_document_write_provenance(file, target);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{source}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                target.len(),
                agent_doc_hash::content_hash(target)
            ),
        );
        return Ok(());
    }

    write_effects.guard_visible_write_expected_current(file, source, current)?;
    write_effects.converge_or_disk_write(file, current, target, source)
}

/// Opportunistic gated-review auto-verification (`#optverify` / `#optv3`).
///
/// For each gated `[/]` review item carrying a verify predicate, scan `ops.log`
/// and surface `provable` / `failed` / `pending`. When `autoverify` is true and
/// an item is `provable`, flip it `[/]→[x]` in place (persisting to both the
/// working-tree file and the snapshot, mirroring pending maintenance), so the
/// existing reap pass archives it on a later cycle. Default off — without the
/// opt-in the gate is only surfaced, never silently flipped.
///
/// Returns the per-item results for the preflight output. Best-effort: a missing
/// `ops.log`, no review component, or no predicates yields an empty vector.
pub fn run_gate_verify(
    file: &Path,
    autoverify: bool,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<Vec<GateVerifyResult>> {
    run_gate_verify_with_options(
        file,
        autoverify,
        GateVerifyOptions::default(),
        write_effects,
    )
}

#[cfg(test)]
fn run_gate_verify_force_disk(
    file: &Path,
    autoverify: bool,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<Vec<GateVerifyResult>> {
    run_gate_verify_with_options(
        file,
        autoverify,
        GateVerifyOptions { force_disk: true },
        write_effects,
    )
}

fn run_gate_verify_with_options(
    file: &Path,
    autoverify: bool,
    options: GateVerifyOptions,
    write_effects: &dyn PreflightMaintenanceWriteEffects,
) -> Result<Vec<GateVerifyResult>> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(review) = components
        .iter()
        .find(|c| is_review_component(&c.name))
        .cloned()
    else {
        return Ok(Vec::new());
    };
    let body = review.content(&content).to_string();
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(&body);

    // Gather predicate-bearing gated items.
    let predicates: Vec<(
        String,
        agent_doc_element_backlog::gate_verify::GatePredicate,
    )> = items
        .iter()
        .filter(|item| {
            matches!(
                item.state,
                agent_doc_element_backlog::backlog::PendingState::Gated
            )
        })
        .filter_map(|item| {
            agent_doc_element_backlog::gate_verify::parse_gate_predicate(&item.text)
                .filter(|p| p.is_actionable())
                .map(|p| (item.id.clone(), p))
        })
        .collect();
    if predicates.is_empty() {
        return Ok(Vec::new());
    }

    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let ops_log = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
        .and_then(|root| {
            agent_doc_ops_log_io::read_rotated_log(&root.join(".agent-doc/logs/ops.log"))
                .ok()
                .flatten()
        })
        .unwrap_or_default();

    let mut results = Vec::new();
    let mut to_resolve: Vec<String> = Vec::new();
    for (id, predicate) in &predicates {
        let outcome = agent_doc_element_backlog::gate_verify::scan_ops_log(predicate, &ops_log);
        let (marker, at) = match &outcome {
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Failed { marker, at } => {
                (Some(marker.clone()), Some(*at))
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Pending => (None, None),
        };
        let status = outcome.status_str().to_string();
        let provable = matches!(
            outcome,
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { .. }
        );
        let auto_resolved = autoverify && provable;
        if auto_resolved {
            to_resolve.push(id.clone());
        }
        match &outcome {
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Provable { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} provable (marker {:?} @ {})",
                    id, marker, at
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "optverify review={} status=provable marker={:?} at={} auto_resolved={}",
                        id, marker, at, auto_resolved
                    ),
                );
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Failed { marker, at } => {
                eprintln!(
                    "[preflight] optverify: review #{} FAILED (disproof {:?} @ {}) — file a bug",
                    id, marker, at
                );
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "optverify review={} status=failed marker={:?} at={}",
                        id, marker, at
                    ),
                );
            }
            agent_doc_element_backlog::gate_verify::VerifyOutcome::Pending => {}
        }
        results.push(GateVerifyResult {
            id: id.clone(),
            status,
            marker,
            at,
            auto_resolved,
        });
    }

    // Opt-in transition: flip provable gates [/]→[x] in place, persisting to
    // both the working-tree file and the snapshot.
    if !to_resolve.is_empty() {
        let mut new_body = body.clone();
        for id in &to_resolve {
            new_body = agent_doc_element_backlog::backlog::op_done(&new_body, id)?;
        }
        let new_content = review.replace_content(&content, &new_body);
        persist_pending_maintenance_doc(
            file,
            &content,
            &new_content,
            "optverify_resolve",
            options.force_disk,
            write_effects,
        )?;
        // Keep the snapshot in lockstep when possible; it is recovery state, so a
        // missing/malformed sidecar must not veto the document mutation.
        match agent_doc_snapshot_io::load_document_baseline(file) {
            Ok(Some(snap)) => {
                if let Ok(snap_comps) = agent_doc_element::element::parse(&snap) {
                    if let Some(snap_review) =
                        snap_comps.iter().find(|c| is_review_component(&c.name))
                    {
                        let snap_new = snap_review.replace_content(&snap, &new_body);
                        if let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
                            file,
                            &snap_new,
                            agent_doc_ops_log_io::log_op,
                        ) {
                            log_snapshot_recovery_warning(file, "optverify snapshot sync", err);
                        }
                    }
                } else {
                    log_snapshot_recovery_warning(
                        file,
                        "optverify snapshot sync",
                        "failed to parse snapshot components",
                    );
                }
            }
            Ok(None) => log_snapshot_recovery_warning(
                file,
                "optverify snapshot sync",
                "snapshot is missing",
            ),
            Err(err) => log_snapshot_recovery_warning(file, "optverify snapshot sync", err),
        }
        eprintln!(
            "[preflight] optverify: auto-resolved {} provable gate(s): {}",
            to_resolve.len(),
            to_resolve.join(", ")
        );
    }

    Ok(results)
}

/// Warning code preflight emits when it admits a turn over shadow-only open
/// backlog rows (`#rundocsstop`).
pub const SHADOW_OPEN_BACKLOG_REPAIR_WARNING: &str = "shadow_open_backlog_repair";

/// Refuse when an open backlog row exists only outside the live
/// `agent:backlog`. The strict form of [`shadow_open_backlog_admission`], for a
/// caller that has no operator intent to honour.
pub fn enforce_no_shadow_open_backlog(file: &Path) -> Result<()> {
    shadow_open_backlog_admission(file, None).map(|_| ())
}

/// The shadow-backlog preflight gate (`#rundocsstop`).
///
/// A shadow-only open row is a placement finding, not lost data, so it defers to
/// operator intent through [`agent_doc_turn::turn_admission::TurnAdmission::repairable_finding`]:
/// with the operator's edit or explicit route as this turn's prompt, preflight
/// admits and returns a repair directive for the cycle contract instead of
/// refusing and stranding that prompt; otherwise it refuses as before.
pub fn shadow_open_backlog_admission(
    file: &Path,
    entry_admission: Option<agent_doc_turn::turn_admission::TurnAdmission>,
) -> Result<Option<PreflightWarning>> {
    let content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog shadow state in {}",
            file.display()
        )
    })?;
    let report = agent_doc_element_backlog::backlog::detect_shadow_open_items(&content)?;
    if !report.duplicated_in_live_backlog.is_empty() {
        eprintln!(
            "[preflight] pending shadow warning: open backlog item(s) also appear outside live agent:backlog: {}",
            format_shadow_refs(&report.duplicated_in_live_backlog)
        );
    }
    if report.shadow_only.is_empty() {
        return Ok(None);
    }
    let refs = format_shadow_refs(&report.shadow_only);
    let verdict = agent_doc_turn::turn_admission::RepairableFindingAdmission::for_entry_admission(
        entry_admission,
    );
    if !verdict.admits() {
        anyhow::bail!(
            "open backlog item(s) exist only outside live agent:backlog: {refs}. Move them back into the live backlog or mark them complete before continuing"
        );
    }
    let admission = entry_admission.map_or("none", |admission| admission.as_str());
    eprintln!(
        "[preflight] shadow backlog admitted as a repair directive ({admission}): open backlog item(s) exist only outside live agent:backlog: {refs}"
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "preflight_shadow_backlog_admitted file={} admission={admission} shadow_only={} (#rundocsstop)",
            file.display(),
            report.shadow_only.len()
        ),
    );
    Ok(Some(PreflightWarning {
        code: SHADOW_OPEN_BACKLOG_REPAIR_WARNING.to_string(),
        message: format!(
            "open backlog item(s) exist only outside live agent:backlog: {refs}. Preflight admitted this turn anyway because the operator's edit is its prompt (`#rundocsstop`): answer that edit, and in this same turn move each listed item back into agent:backlog or mark it complete. The closeout session-check keeps reporting them until they are restored."
        ),
        document_agent: None,
        active_harness: None,
    }))
}

pub fn enforce_no_dropped_backlog(
    file: &Path,
    head_content: Option<&str>,
    operator_entry_content: &str,
) -> Result<()> {
    let head_content = match head_content {
        Some(content) => content,
        None => return Ok(()),
    };
    let current_content = std::fs::read_to_string(file).with_context(|| {
        format!(
            "failed to inspect backlog replay state in {}",
            file.display()
        )
    })?;
    let resolved_ids = agent_doc_cycle_state_io::resolved_pending_ids(file)?;

    let mut external_current_ids =
        agent_doc_element_backlog_io::done_archive::external_done_archive_ids(
            file,
            &current_content,
        )?;
    let initial_report = agent_doc_element_backlog::dropped_from_history_report(
        &current_content,
        head_content,
        &resolved_ids,
        &external_current_ids,
    )?;
    if !initial_report.dropped.is_empty() {
        let candidates = initial_report
            .dropped
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let transfer_evidence = agent_doc_element_backlog_io::cross_document::transferred_open_ids(
            file,
            &current_content,
            &candidates,
        )?;
        for (id, destination) in &transfer_evidence.destinations {
            eprintln!(
                "[preflight] backlog transfer: #{} remains open in {}",
                id,
                destination.display()
            );
        }
        external_current_ids.extend(transfer_evidence.ids());
    }
    let report = agent_doc_element_backlog::dropped_from_history_report(
        &current_content,
        head_content,
        &resolved_ids,
        &external_current_ids,
    )?;
    if !report.dropped.is_empty() {
        let evidence =
            agent_doc_element_backlog_io::deletion_authority::classify_operator_deletion(
                file,
                head_content,
                operator_entry_content,
                Some(head_content),
                &report,
            )?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "backlog_deletion_authority file={} ids={} evidence={}",
                file.display(),
                report
                    .dropped
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                evidence.as_str(),
            ),
        );
        match agent_doc_element_backlog_io::deletion_authority::operator_deletion_outcome(
            &report, evidence,
        ) {
            agent_doc_element_backlog_io::deletion_authority::OperatorDeletionOutcome::Pass => {}
            agent_doc_element_backlog_io::deletion_authority::OperatorDeletionOutcome::Warn(
                lines,
            ) => {
                for line in lines {
                    eprintln!("{line}");
                }
            }
            agent_doc_element_backlog_io::deletion_authority::OperatorDeletionOutcome::Interrupt(
                message,
            ) => {
                anyhow::bail!(message);
            }
        }
    }
    Ok(())
}

/// Queue component state extracted during maintenance.
///
/// Returned by `run_queue_maintenance` for later composition into `PreflightOutput`.
/// The `queue_prompts` are only populated when the queue is active.
#[derive(Debug, Default)]
pub struct QueueState {
    /// The coherent document projection from which this queue state was derived.
    ///
    /// Queue maintenance may rewrite the queue after preflight's initial diff
    /// cut. Downstream queue policy must consume this settled projection rather
    /// than reusing the stale pre-maintenance cut (GH #161).
    pub authoritative_content: Option<String>,
    pub queue_prompts: Vec<String>,
    pub selected_queue_prompts: Vec<String>,
    pub queue_active: Option<bool>,
    pub queue_deferred: bool,
    pub queue_start_at: Option<String>,
    pub queue_trigger: Option<agent_doc_queue::document_queue::QueueTrigger>,
    pub queue_halted: Option<String>,
    /// `#qpausego`: true when an accepted controller `admin queue pause` is the
    /// effective queue-control state. Surfaced for visibility and consumed by the
    /// supervisor idle-watch auto-injection guard; it does NOT gate
    /// `queue_continuation_required` / `queue_drainable_head_count` (the attended
    /// in-session `/loop` keeps draining real work). Cleared by `admin queue resume`.
    pub queue_paused: bool,
    /// `#qpausemix`: the controller-recorded pause reason when `queue_paused` is
    /// true (empty string when the pause carried none); `None` when not paused.
    /// Surfaced so the agent can see *why* the queue was paused instead of reading
    /// `queue_paused` + `queue_continuation_required` as a contradictory "mixed
    /// signal". Feeds the pause-aware `queue_continuation_guidance`.
    pub queue_pause_reason: Option<String>,
    /// `#cleardrainsignal`: count of agent-drainable heads (not deferred/noise) in
    /// the active queue. 0 while `queue_active` is `Some(true)` means a no-op churn
    /// cycle — the agent/auto-loop must NOT loop.
    pub queue_drainable_head_count: usize,
    /// `#cleardrainsignal`: whether the queue has agent-drainable continuation work
    /// this session. False when inactive OR every remaining head is deferred/noise.
    pub queue_continuation_required: bool,
    /// `#rt83`: whether the active queue head is drainable in the SUPERVISOR scope
    /// (defers `[operator-verify]`/noise only; `[focused-cycle]`/`[clean-session]`
    /// stay drainable because the supervisor force-`/clear`s + re-dispatches them).
    /// Gates the preflight synthetic queue-head diff: a head that no drainer (neither
    /// the in-session `/loop` nor the supervisor) will act on must NOT synthesize a
    /// phantom `+:pushpin: do [#id]` prompt diff, which previously kept
    /// `no_changes:false` every preflight and sustained the qchurn flood.
    pub queue_supervisor_drainable: bool,
    /// `#regressionawaitingoperat`: the head `queue_supervisor_drainable`
    /// found — the first unclaimed live head drainable in the SUPERVISOR scope,
    /// as marker-stripped prompt text. [`QueueState::continuation_head`] falls
    /// back to it, so the synthetic continuation diff names a head some drainer
    /// will act on, never an `[operator-verify]` (`⏳`) head queued above it.
    pub queue_supervisor_continuation_head: Option<String>,
    /// `#qmaintorphan`: whether queue maintenance mutated the document this
    /// preflight. Queue maintenance runs at step 4b2, *after* the single step-2
    /// commit, so its mutation is normally committed by the cycle's own
    /// `respond` / `write --commit`. On a `no_changes` cycle the skill tells the
    /// agent to stop without persisting, which orphaned that mutation in the
    /// working tree + snapshot while HEAD stayed behind — a three-plane split
    /// that `doctor` reports as recoverable `SnapshotDiffersFromHead` and that
    /// the next commit path can classify as an unanswered typed-component edit.
    /// `run` commits maintenance itself when this is set and nothing else
    /// changed.
    pub maintenance_mutated: bool,
    /// `#explicitrunqueue`: live queue heads held by an unexpired worker claim
    /// (owner + expiry). They are excluded from selection, drainability, and
    /// the synthetic queue-head diff; a `no_changes` preflight names them so an
    /// explicit Run Agent Doc answers "claimed by <owner>, expires <t>" instead
    /// of opening a no-op cycle for in-flight work.
    pub claimed_queue_heads: Vec<agent_doc_queue::no_changes_explanation::ClaimedQueueItemFact>,
    pub synced_queue_ids: Vec<String>,
    /// `#capturegatedispatched`: `(source_text, id)` receipts of the free-text
    /// queue heads this maintenance pass promoted into `do [#id]` heads, so a
    /// caller that observed the pre-promotion line can name the live head.
    pub free_text_promotions: Vec<(String, String)>,
    pub warnings: Vec<PreflightWarning>,
}

impl QueueState {
    /// Whether `prompt` is a live head held by a worker claim.
    pub fn head_is_claimed(&self, prompt: &str) -> bool {
        let identity = agent_doc_queue::queue_claim::claim_identity(prompt);
        self.claimed_queue_heads
            .iter()
            .any(|claimed| agent_doc_queue::queue_claim::claim_identity(&claimed.item) == identity)
    }

    /// The queue head this cycle would continue on: the first selected head,
    /// else the first unclaimed live head drainable in the supervisor scope
    /// (`#explicitrunqueue`, `#regressionawaitingoperat`). A claimed head is in
    /// flight elsewhere and an `[operator-verify]` / gated-review head waits on
    /// the operator (`⏳`); neither is ever a continuation head.
    pub fn continuation_head(&self) -> Option<&String> {
        self.selected_queue_prompts
            .first()
            .or(self.queue_supervisor_continuation_head.as_ref())
    }
}

/// Owner + expiry of every live queue head an unexpired worker claim holds
/// (`#explicitrunqueue`). An unreadable ledger reports no claims, matching
/// [`agent_doc_queue_io::queue_claim::claimed_items_for_content`].
fn claimed_queue_head_facts(
    file: &Path,
    content: &str,
) -> Vec<agent_doc_queue::no_changes_explanation::ClaimedQueueItemFact> {
    let claims = agent_doc_queue_io::queue_claim::active_claims_for_content(file, content)
        .unwrap_or_default();
    let heads =
        agent_doc_queue::queue_continuation::live_queue_head_texts(content).unwrap_or_default();
    claims
        .into_iter()
        .map(|claim| {
            let item = heads
                .iter()
                .find(|head| agent_doc_queue::queue_claim::claim_identity(head) == claim.identity)
                .map(|head| strip_in_progress_marker(head))
                .unwrap_or_else(|| claim.item_text.trim().to_string());
            let (owner_session_state, owner_session_ended) =
                match agent_doc_queue_io::queue_claim::claim_owner_liveness(file, &claim) {
                    agent_doc_queue::queue_claim::ClaimOwnerLiveness::Live => ("live", None),
                    agent_doc_queue::queue_claim::ClaimOwnerLiveness::Unrecorded => {
                        ("unrecorded", None)
                    }
                    agent_doc_queue::queue_claim::ClaimOwnerLiveness::Ended(reason) => {
                        ("ended", Some(stale_claim_fact(file, &claim, reason)))
                    }
                };
            agent_doc_queue::no_changes_explanation::ClaimedQueueItemFact {
                item,
                owner: claim.owner,
                generation: claim.generation,
                state: claim.state,
                worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
                worker_started_at: claim
                    .worker
                    .as_ref()
                    .map(|worker| agent_doc_log_time::format_log_timestamp(worker.started_at_secs)),
                owner_session_state: owner_session_state.to_string(),
                expires_at: agent_doc_log_time::format_log_timestamp(claim.expires_at_secs),
                owner_session_ended,
            }
        })
        .collect()
}

/// `#claimownerliveness`: the adopt/release exits for a claim whose owner
/// session ended, as runnable commands for this document.
fn stale_claim_fact(
    file: &Path,
    claim: &agent_doc_queue::queue_claim::QueueClaim,
    reason: agent_doc_queue::queue_claim::OwnerSessionEndReason,
) -> agent_doc_queue::no_changes_explanation::StaleClaimFact {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let doc = quote(&file.display().to_string());
    let handle = quote(&agent_doc_queue::queue_claim::claim_item_handle(
        &claim.item_text,
    ));
    let generation = if claim.generation > 0 {
        format!(" --generation {}", claim.generation)
    } else {
        String::new()
    };
    agent_doc_queue::no_changes_explanation::StaleClaimFact {
        reason: reason.as_str().to_string(),
        detail: reason.describe().to_string(),
        adopt_command: format!(
            "agent-doc queue claim {doc} --item {handle} --owner {} --refresh{generation}",
            quote(&claim.owner),
        ),
        release_command: format!("agent-doc queue release {doc} --item {handle}{generation}"),
    }
}

fn record_selected_queue_head_state(
    file: &Path,
    content: &str,
    head_text: &str,
    drainable: bool,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(());
    };
    let Some(node_key) =
        agent_doc_queue::queue_projection::selected_queue_head_node_key(content, head_text)
    else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let content_hash = agent_doc_hash::content_hash(head_text);
    let queue_generation =
        agent_doc_queue::queue_projection::queue_worklist_hash_for_document(content)
            .unwrap_or_else(|| agent_doc_hash::content_hash(content));
    let event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "queue-head-selected:{document_hash}:{node_key}:0:{content_hash}:{queue_generation}"
        ),
        agent_doc_state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable,
            hosting_epoch: None,
        },
    );
    let inserted = publish_reactive_state_event(&project_root, &event)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_selected_state_event_recorded file={} event_id={} inserted={} document_hash={} node_id={} drainable={}",
            file.display(),
            event.event_id,
            inserted,
            document_hash,
            node_key,
            drainable
        ),
    );
    Ok(())
}

fn record_deferred_queue_head_state(
    file: &Path,
    content: &str,
    head_text: &str,
    reason: &str,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(());
    };
    let Some(node_key) =
        agent_doc_queue::queue_projection::selected_queue_head_node_key(content, head_text)
    else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let content_hash = agent_doc_hash::content_hash(head_text);
    let queue_generation =
        agent_doc_queue::queue_projection::queue_worklist_hash_for_document(content)
            .unwrap_or_else(|| agent_doc_hash::content_hash(content));
    let selected_event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "queue-head-deferred-selected:{document_hash}:{node_key}:0:{content_hash}:{queue_generation}"
        ),
        agent_doc_state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable: false,
            hosting_epoch: None,
        },
    );
    let selected_inserted = publish_reactive_state_event(&project_root, &selected_event)?;
    let reason_hash = agent_doc_hash::content_hash(reason);
    let deferred_event = agent_doc_state_backbone::StateEvent::new(
        format!(
            "queue-head-deferred:{document_hash}:{node_key}:0:{reason_hash}:{content_hash}:{queue_generation}"
        ),
        agent_doc_state_backbone::StateFact::QueueHeadDeferred {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            reason: reason.to_string(),
            hosting_epoch: None,
        },
    );
    let deferred_inserted = publish_reactive_state_event(&project_root, &deferred_event)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_deferred_state_event_recorded file={} selected_event_id={} selected_inserted={} deferred_event_id={} deferred_inserted={} document_hash={} node_id={} reason={}",
            file.display(),
            selected_event.event_id,
            selected_inserted,
            deferred_event.event_id,
            deferred_inserted,
            document_hash,
            node_key,
            reason
        ),
    );
    Ok(())
}

fn record_queue_worklist_state(
    file: &Path,
    content: &str,
    entries: &[agent_doc_queue::document_queue::QueueEntry],
    active: bool,
) -> Result<()> {
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "queue maintenance: failed to canonicalize {}",
            file.display()
        )
    })?;
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(());
    };
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    let queue_hash = agent_doc_queue::queue_projection::queue_worklist_hash(entries);
    let worklist_entries = if active {
        agent_doc_queue::queue_projection::queue_worklist_entries(content, entries)
            .into_iter()
            .map(|entry| agent_doc_state_backbone::QueueWorklistEntry {
                kind: match entry.kind {
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Prompt => {
                        agent_doc_state_backbone::QueueWorklistEntryKind::Prompt
                    }
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Preset => {
                        agent_doc_state_backbone::QueueWorklistEntryKind::Preset
                    }
                    agent_doc_queue::queue_projection::QueueWorklistEntryKind::Dispatch => {
                        agent_doc_state_backbone::QueueWorklistEntryKind::Dispatch
                    }
                },
                text: entry.text,
                node_key: entry.node_key,
                backlog_id: entry.backlog_id,
                drainable: entry.drainable,
            })
            .collect()
    } else {
        Vec::new()
    };
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("queue-worklist-projected:{document_hash}:{active}:{queue_hash}"),
        agent_doc_state_backbone::StateFact::QueueWorklistProjected {
            document_hash: document_hash.clone(),
            queue_hash: queue_hash.clone(),
            entries: worklist_entries,
            active,
            hosting_epoch: None,
        },
    );
    let inserted = publish_reactive_state_event(&project_root, &event)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_worklist_state_event_recorded file={} event_id={} inserted={} document_hash={} queue_hash={} active={}",
            file.display(),
            event.event_id,
            inserted,
            document_hash,
            queue_hash,
            active
        ),
    );
    Ok(())
}

#[cfg(not(test))]
fn publish_reactive_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    agent_doc_controller_io::project_controller::publish_state_event(project_root, event)
}

#[cfg(test)]
fn publish_reactive_state_event(
    project_root: &Path,
    event: &agent_doc_state_backbone::StateEvent,
) -> Result<bool> {
    agent_doc_controller_io::project_controller::append_state_event_for_test(project_root, event)
}

/// Fold a CP-proven editor-buffer queue deletion into the preflight queue source.
///
/// Queue maintenance normally starts from disk and then converges that queue
/// shape back into a live editor buffer. When the operator deletes queue rows in
/// the editor during a turn, that delete may be unsaved: blindly starting from
/// disk re-pushes the stale rows and makes them "reappear". Only adopt the CP
/// current document when its queue is a count-wise subset of disk after
/// stripping cosmetic progress/pin markers. That covers deleting one duplicate
/// row or all copies of a row without treating same-cycle queue additions as an
/// implicit merge. Plugin sidecars are not consulted.
/// Read-only queue inspection for `preflight --probe`.
///
/// This intentionally does not run queue convergence, backlog mirroring,
/// in-progress marker updates, journals, or snapshot/frontmatter writes. It only
/// computes the queue facts needed for preflight JSON from the current document.
/// Queue entries minus heads the in-session loop must not select: claimed
/// heads (`#queueclaim`) and new subagent-dispatch lines (`#closeout-steering`).
fn entries_without_out_of_loop_heads(
    entries: &[agent_doc_queue::document_queue::QueueEntry],
    claimed: &agent_doc_queue::queue_claim::ClaimedQueueItems,
    dispatch: &[String],
) -> Vec<agent_doc_queue::document_queue::QueueEntry> {
    let claimed_at: Vec<bool> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| match entry {
            agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                let display = agent_doc_document::queue_projection::strip_in_progress_marker(
                    prompt.text.trim(),
                );
                let lineage =
                    agent_doc_queue::document_queue::lineage_claim_text(entries, index);
                claimed.claims_at(entries, index)
                    || dispatch.iter().any(|item| {
                        item == &display
                            || lineage.as_ref() == Some(item)
                            || agent_doc_queue::queue_claim::claim_identity_at(entries, index)
                                .is_some_and(|identity| {
                                    agent_doc_queue::queue_claim::claim_identity(item) == identity
                                })
                    })
            }
            _ => false,
        })
        .collect();
    entries
        .iter()
        .enumerate()
        .filter(|(index, _)| !claimed_at[*index])
        .map(|(_, entry)| entry.clone())
        .collect()
}

pub fn inspect_queue_state(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    let content = match std::fs::read_to_string(file) {
        Ok(content) => content,
        Err(_) => return Ok(QueueState::default()),
    };
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let (content, _) =
        converge_queue_control_binding_content(&content, snapshot_content.as_deref())?;
    let (content, _) = infer_queue_go_from_prompt_edit(&content, snapshot_content.as_deref())?;
    let _nested_claim_steer =
        agent_doc_queue_io::queue_claim::nested_claim_steer_for(file, &content);
    let components = match agent_doc_element::element::parse(&content) {
        Ok(components) => components,
        Err(_) => return Ok(QueueState::default()),
    };
    let owner_selection =
        agent_doc_queue_io::subagent_dispatch::owner_selection_claims_for_content(file, &content);
    let selection_out_of_loop_items =
        agent_doc_queue_io::subagent_dispatch::out_of_loop_items_for_content(file, &content);
    let queue_control = agent_doc_queue::queue_continuation::document_queue_control(&content);
    let comp = match agent_doc_queue::queue_set::QueueSet::parse(&content, &components) {
        Ok(queues) => match queues
            .selected_reaching_inline_work_with_control(
                &owner_selection,
                queue_control.as_deref(),
            )
            // When every queue is held, keep exposing the ordinary selected
            // queue's blocked facts instead of degrading to an empty probe.
            .or_else(|| queues.selected())
        {
            Some(queue) => queue.component,
            None => return Ok(QueueState::default()),
        },
        Err(err) => {
            eprintln!("[preflight] queue probe topology warning: {err}");
            return Ok(QueueState::default());
        }
    };

    let body = &content[comp.open_end..comp.close_start];
    let entries = match agent_doc_queue::document_queue::parse(body) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("[preflight] queue probe parse warning: {err}");
            return Ok(QueueState::default());
        }
    };

    let marker_control = agent_doc_queue::document_queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Pause)
    );
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff
        .map(agent_doc_diff::detect_queue_trigger)
        .unwrap_or(false);
    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, frontmatter_queue_control(&fm));
    let persisted_activation =
        queue_control_activation(&comp.attrs, frontmatter_queue_control(&fm));

    let mut activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_auto,
        exchange_triggered,
        persisted_activation,
    );
    if marker_stop && activation.active {
        activation = agent_doc_queue::document_queue::QueueActivation {
            entries_after: activation.entries_after,
            ..Default::default()
        };
    }

    if activation.active
        && agent_doc_queue::document_queue::has_stop_fence_at_head(&activation.entries_after)
    {
        return Ok(QueueState {
            authoritative_content: Some(content),
            queue_prompts: vec![],
            selected_queue_prompts: vec![],
            queue_active: Some(false),
            queue_deferred: false,
            queue_start_at: None,
            queue_trigger: activation.trigger,
            queue_halted: Some("stop_fence".to_string()),
            queue_paused: false,
            queue_pause_reason: None,
            queue_drainable_head_count: 0,
            queue_continuation_required: false,
            queue_supervisor_drainable: false,
            queue_supervisor_continuation_head: None,
            // `#qmaintorphan`: read-only / pre-persist path — no plane split to commit.
            maintenance_mutated: false,
            claimed_queue_heads: vec![],
            synced_queue_ids: vec![],
            free_text_promotions: vec![],
            warnings: vec![],
        });
    }

    if activation.active
        && let Some(start_at) =
            agent_doc_queue::document_queue::time_gate_at_head(&activation.entries_after)
    {
        return Ok(QueueState {
            authoritative_content: Some(content),
            queue_prompts: vec![],
            selected_queue_prompts: vec![],
            queue_active: None,
            queue_deferred: true,
            queue_start_at: Some(start_at.to_string()),
            queue_trigger: activation.trigger,
            queue_halted: None,
            queue_paused: false,
            queue_pause_reason: None,
            queue_drainable_head_count: 0,
            queue_continuation_required: false,
            queue_supervisor_drainable: false,
            queue_supervisor_continuation_head: None,
            // `#qmaintorphan`: read-only / pre-persist path — no plane split to commit.
            maintenance_mutated: false,
            claimed_queue_heads: vec![],
            synced_queue_ids: vec![],
            free_text_promotions: vec![],
            warnings: vec![],
        });
    }

    let queue_prompts = if activation.active {
        agent_doc_queue::document_queue::prompts(&activation.entries_after)
            .iter()
            .map(|prompt| strip_in_progress_marker(&prompt.text))
            .collect()
    } else {
        vec![]
    };
    let queue_pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    let queue_paused = queue_pause_reason.is_some();
    let drainability_content = if activation.active {
        let body = agent_doc_queue::document_queue::render(&activation.entries_after);
        let projected = comp.replace_content(&content, &body);
        frontmatter::merge_queue_state(&projected, true).unwrap_or(projected)
    } else {
        content.clone()
    };
    // `#queueclaim`: heads whose workers are still claimed/dispatching or
    // running outside the in-session loop are in flight, so they are neither
    // counted nor selected for this cycle. Reported attempts are deliberately
    // not excluded: their workers are finished and coordinator
    // integration/writeback is now the drainable work (`#reportedwritebackdebt`).
    let in_flight_queue_items = if activation.active {
        agent_doc_queue_io::queue_claim::in_flight_claimed_items_for_content(file, &content)
    } else {
        agent_doc_queue::queue_claim::ClaimedQueueItems::none()
    };
    let subagent_dispatch = if activation.active {
        agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(file, &content)
    } else {
        Vec::new()
    };
    let out_of_loop_queue_items = if activation.active {
        selection_out_of_loop_items
    } else {
        agent_doc_queue::queue_claim::ClaimedQueueItems::none()
    };
    // GH #381: a head naming a `.agent-doc/project.md` preset is a preset
    // invocation, not a mirror of a missing backlog item.
    let drain_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(&content, file);
    let queue_drainable_head_count = if activation.active {
        agent_doc_queue::queue_continuation::drainable_head_count_for_owner(
            &drainability_content,
            &owner_selection,
            &out_of_loop_queue_items,
            &drain_presets,
        )
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    let queue_supervisor_continuation_head = if activation.active {
        agent_doc_queue::queue_continuation::live_drainable_head_prompt_text_for_owner(
            &drainability_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            &owner_selection,
            &out_of_loop_queue_items,
            &drain_presets,
        )
    } else {
        None
    };
    let queue_supervisor_drainable = queue_supervisor_continuation_head.is_some();
    let claimed_queue_heads = if activation.active {
        claimed_queue_head_facts(file, &content)
    } else {
        Vec::new()
    };
    let skipped_queue_head_ids: std::collections::HashSet<String> =
        agent_doc_cycle_state_io::load(file)
            .ok()
            .flatten()
            .map(|state| state.skipped_queue_head_ids.into_iter().collect())
            .unwrap_or_default();
    let projected_in_progress_queue_heads = load_projected_in_progress_queue_heads(file);
    let projection_entries = entries_without_out_of_loop_heads(
        &activation.entries_after,
        &in_flight_queue_items,
        &subagent_dispatch,
    );
    let selected_queue_prompts = if activation.active {
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &drainability_content,
            &projection_entries,
            &agent_doc_queue::backlog_sync::collect_after_deps(&components, &content),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &drainability_content,
                &activation.entries_after,
                &projected_in_progress_queue_heads,
            ),
            &skipped_queue_head_ids,
        )
        .prompts
        .into_iter()
        .filter(|prompt| !in_flight_queue_items.claims(prompt))
        // `#closeout-steering`: a new subagent-intent line is dispatched, not
        // drained inline (`queue_subagent_dispatch`).
        .filter(|prompt| {
            !agent_doc_queue_io::subagent_dispatch::is_dispatch_item(&subagent_dispatch, prompt)
        })
        .collect()
    } else {
        Vec::new()
    };

    Ok(QueueState {
        authoritative_content: Some(content),
        queue_prompts,
        selected_queue_prompts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active || explicit_stop {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        queue_paused,
        queue_pause_reason,
        queue_drainable_head_count,
        queue_continuation_required,
        queue_supervisor_drainable,
        queue_supervisor_continuation_head,
        // `#qmaintorphan`: read-only / pre-persist path — no plane split to commit.
        maintenance_mutated: false,
        claimed_queue_heads,
        synced_queue_ids: vec![],
        free_text_promotions: vec![],
        warnings: vec![],
    })
}

/// `#qstartinert`: resolve queue activation from the operator's explicit control.
///
/// `queue:` (frontmatter) and the `start`/`go` marker tokens are the canonical
/// activation control — `control_binding` converges the two and writes only
/// `queue: <mode>`, treating `queue_active:` as a legacy field it reads solely
/// when `queue:` is absent. Activation must therefore key off that control alone.
/// Requiring a persisted `queue_active: true` in addition left every document
/// whose only control was `queue: start` permanently unarmed: entries mirrored in,
/// but `queue_drainable_head_count` stayed `0` and the auto-loop never got a head.
///
/// This is a strict widening of the old predicate. `queue_active: true` was only
/// ever honored *alongside* a `start`/`go` token, so a legacy-flag-only document
/// stays inactive exactly as before, and `stop` still dominates both spellings.
fn queue_control_activation(
    attrs: &std::collections::HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    agent_doc_queue::control_binding::queue_control_activation(attrs, frontmatter_queue)
}

/// Queue maintenance cannot truthfully classify the queue until the retained
/// editor authority is current. Callers must fail admission on this error;
/// substituting an empty `QueueState` makes a retained queue edit look drained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueAuthorityUnavailable {
    reason: String,
}

impl QueueAuthorityUnavailable {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for QueueAuthorityUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for QueueAuthorityUnavailable {}

/// Return the disk text only when it proves the registered live replica is an
/// older baseline cut. The baseline is the common ancestor: equality on the
/// durable authority branch proves that only the durable disk branch advanced.
///
/// The classification and the action both come from
/// `agent_doc_document::admission_divergence`, shared with `session-check` so the
/// refusal and the remedy printed for it cannot drift apart. This function is the
/// IO shell: it loads the three planes, records the assessment, and maps it.
fn disk_edit_newer_than_registered_authority(
    file: &Path,
    authority: &str,
) -> Result<Option<String>> {
    let Some(baseline) = agent_doc_snapshot_io::load_document_baseline(file)? else {
        return Ok(None);
    };
    let editor_ops = agent_doc_op_capture_io::editor_ops_for_base_or_retained(file, &baseline)
        .ok()
        .flatten();
    disk_edit_newer_than_registered_authority_with_lineage(
        file,
        authority,
        &baseline,
        authority,
        editor_ops.as_deref(),
    )
}

fn disk_edit_newer_than_registered_authority_with_lineage(
    file: &Path,
    authority: &str,
    baseline: &str,
    op_replayed_authority: &str,
    editor_ops: Option<&[agent_doc_merge::crdt::EditorOp]>,
) -> Result<Option<String>> {
    let disk = std::fs::read_to_string(file)?;
    let assessment =
        agent_doc_document::admission_divergence::assess(Some(baseline), Some(authority), &disk);
    log_admission_divergence_assessment(file, baseline, authority, &disk, assessment);
    match assessment.adoption {
        agent_doc_document::admission_divergence::DiskAdoption::ProceedOnAuthority => Ok(None),
        agent_doc_document::admission_divergence::DiskAdoption::FastForwardAuthorityToDisk => {
            Ok(Some(disk))
        }
        agent_doc_document::admission_divergence::DiskAdoption::MergeDiskIntoAuthority => {
            merge_admission_three_way_split(
                file,
                baseline,
                authority,
                &disk,
                op_replayed_authority,
                editor_ops,
            )
        }
    }
}

/// `#admissionmergedup`: the list-item count-conservation guard every admission
/// merge rung must clear. For each identity-bearing list component, the merged
/// revision may hold at most `baseline + additions(authority) + additions(disk)`
/// items, where an addition is growth over the baseline. An edit is not an
/// addition, so two sides editing one item must still yield one item.
///
/// Live 2026-09-30 on infra.md: the line-based `Semantic` rung kept both of two
/// different edits of one queue item, adopting it twice; the authority
/// subsumption check cannot see an addition. `formal/tla/AdmissionSplitMerge.tla`
/// proves this guard restores `NoDuplicateItem` for two edits of one item (its
/// GuardOffWedge config shows stopping on `equals_authority` is not sufficient).
/// It cannot see a revision twin -- two revisions of one item BOTH sides added --
/// because the twin is itself growth over the baseline (`typingforkdup`,
/// TwinWedge); `agent_doc_merge::merge` projects those out of disk first, and
/// callers must pass the projected disk (`MergePlan::merged_agent`) here.
fn merge_invents_list_items(
    baseline: &str,
    authority: &str,
    disk: &str,
    merged: &str,
) -> Option<String> {
    let count = |content: &str, component: &str| -> Option<usize> {
        agent_doc_markdown_ast::mutations::item_nodes(content, component)
            .ok()
            .map(|nodes| nodes.len())
    };
    ["queue", "backlog"].into_iter().find_map(|component| {
        let (Some(base), Some(ours), Some(theirs), Some(out)) = (
            count(baseline, component),
            count(authority, component),
            count(disk, component),
            count(merged, component),
        ) else {
            return None;
        };
        let bound = base + ours.saturating_sub(base) + theirs.saturating_sub(base);
        (out > bound).then(|| format!("{component}:{out}>{bound}"))
    })
}

/// Reconcile a genuine two-writer split into one revision (`#admissionsplitmerge`).
///
/// This replaces a refusal. Both branches durably advanced and the live authority
/// does not already contain disk, so there is no fast-forward available — but the
/// baseline is by definition their common ancestor, which is exactly the input a
/// three-way merge takes. `agent_doc_merge` is the same pure engine the ordinary
/// write path merges concurrent editor and agent revisions with; the returned
/// candidate is adopted through the same compare-and-swap editor write that adopts
/// a proven newer durable save, so the authority cut is rechecked at the mutation
/// edge.
///
/// Disk is the `agent` side and the live editor buffer is the `operator` side. That
/// orientation is load-bearing: on a same-node conflict the merge keeps the
/// operator value, which is the codebase-wide rule that the in-editor document is
/// the source of truth.
///
/// The engines are tried in a ladder, `Cell` first. `Cell` merges node-by-node and
/// preserves each side's authored order, which is what `#qauthorder` requires of a
/// diverged queue — the shape of both live incidents. It also reports `fell_back`
/// when it declines, and its own contract says a caller must then pick another pure
/// strategy. `Semantic` is that second rung, but it is NOT a safe first choice here:
/// it rebuilds prose from the operator skeleton, and on the probe fixture below it
/// emitted an EMPTY `agent:exchange` where both sides had a body. Adopting that would
/// be precisely the dropped-writer outcome the old refusal existed to prevent, which
/// is why every rung must clear `merge_preserves_authority` before it is adopted.
///
/// Returning `Ok(None)` means "proceed on the authority unchanged". That is the
/// non-stalling outcome when no rung produces an adoptable revision: disk then
/// converges at the next commit, exactly as it does for `AuthoritySubsumesDisk`.
/// It is never an error — an admission that cannot merge still must not refuse.
///
/// `#queuecutexclusive`: admission only runs with a live editor, and agent-doc
/// never writes disk behind a live buffer, so disk and the authority are two
/// cuts of ONE editor history, not two writers. Every rung therefore sees disk
/// with each component BOTH planes changed reverted to the baseline (the
/// authority's version is kept and the editor's own ops roll it forward), and
/// every candidate must clear `merge_mixes_component_versions`: such a
/// component is exactly one plane's body, never a union of the two cuts. The
/// count, revision-twin and op-lineage guards below are textual classifiers
/// that each missed the next incident shape; they stay as defense in depth for
/// components only disk changed. Formal model:
/// `formal/tla/AdmissionCutExclusivity.tla`.
fn merge_admission_three_way_split(
    file: &Path,
    baseline: &str,
    authority: &str,
    disk: &str,
    op_replayed_authority: &str,
    authority_ops: Option<&[agent_doc_merge::crdt::EditorOp]>,
) -> Result<Option<String>> {
    let durable = agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
    let durable_authority = durable(authority);
    // `#keylogrebase`: provenance for a component only DISK changed. Cut
    // exclusivity cannot see it -- text alone cannot tell the editor's
    // in-flight typing on disk from independent disk work -- so it used to be
    // merged as new CRDT items and then land again when the in-flight ops
    // arrived. A correctly based, editor-attested op ledger whose replay passes
    // through the authority's cut tells them apart: a disk-only component whose
    // body is a state of that lineage is the editor's own text, and keeps the
    // authority's body. Formal model: `formal/tla/AdmissionCutExclusivity.tla`
    // (`AdmissionCutExclusivityResidual.cfg`).
    let provenance =
        editor_provenance_projection(file, baseline, authority, disk, op_replayed_authority);
    let disk_after_provenance = provenance
        .as_ref()
        .map_or(disk, |projection| projection.disk_text.as_str());
    let editor_attributed = provenance
        .as_ref()
        .map(|projection| projection.editor_attributed.clone())
        .unwrap_or_default();
    let lineage_projection = authority_ops
        .map(|ops| {
            agent_doc_merge::document_cell::suppress_superseded_disk_queue_revisions(
                baseline,
                disk_after_provenance,
                authority,
                op_replayed_authority,
                ops,
            )
        })
        .unwrap_or_else(
            || agent_doc_merge::document_cell::AdmissionDiskLineageProjection {
                disk_text: disk_after_provenance.to_string(),
                suppressed_queue_revisions: 0,
                authority_lineage_proven: false,
            },
        );
    // `#queuecutexclusive`: with a live editor, disk and the authority are two
    // cuts of ONE editor history. A component both changed keeps the
    // authority's version (the editor's own ops roll it forward exactly once);
    // disk's version of it reverts to the baseline before any rung runs, so no
    // rung can union the two cuts. Formal model:
    // `formal/tla/AdmissionCutExclusivity.tla`.
    let cut_exclusive = agent_doc_document::component_exclusivity::project_disk_for_cut_exclusivity(
        baseline,
        authority,
        &lineage_projection.disk_text,
    );
    let merge_disk = cut_exclusive.disk_text.as_str();
    let durable_disk = durable(merge_disk);

    let mut adopted = None;
    let mut rungs = Vec::new();
    if !editor_attributed.is_empty() {
        rungs.push(format!(
            "EditorProvenance=authority_kept({})",
            editor_attributed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if lineage_projection.suppressed_queue_revisions > 0 {
        rungs.push(format!(
            "EditorOpLineage=suppressed_queue_revisions({})",
            lineage_projection.suppressed_queue_revisions
        ));
    }
    if !cut_exclusive.authority_kept.is_empty() {
        rungs.push(format!(
            "CutExclusive=authority_kept({})",
            cut_exclusive
                .authority_kept
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    let mut twins_logged = false;
    for engine in [
        agent_doc_merge::MergeRequest::cell(baseline, merge_disk, authority),
        agent_doc_merge::MergeRequest::semantic(baseline, merge_disk, authority),
    ] {
        let plan = agent_doc_merge::merge(engine);
        if plan.revision_twins_collapsed > 0 && !twins_logged {
            // `typingforkdup`: disk carried an earlier revision of an item the
            // operator added (a mid-typing native save). `merge` projected it
            // out of disk before any rung saw it.
            rungs.push(format!(
                "RevisionTwin=collapsed({})",
                plan.revision_twins_collapsed
            ));
            twins_logged = true;
        }
        let durable_merged = durable(&plan.merged_doc);
        let outcome = if plan.structure_refusal.is_some() {
            // `#componentownclose`: an input's component markers do not pair, so
            // no span-based merge may rewrite it; proceed on the authority.
            "component_structure_refused"
        } else if plan.fell_back {
            // The per-cell engine declined; its contract requires the next strategy.
            "engine_fell_back"
        } else if let Err(err) = agent_doc_element::element::parse(&plan.merged_doc) {
            // A structurally invalid merge must never reach the editor.
            rungs.push(format!("{:?}=not_structurally_valid({err:#})", plan.engine));
            continue;
        } else if !agent_doc_document::admission_divergence::authority_subsumes_disk(
            &durable_authority,
            &durable_merged,
        ) {
            // The merge would regress the editor plane. Never clobber a live buffer.
            "would_drop_authority_content"
        } else if durable_merged == durable_authority {
            // Nothing from disk survived: the compare-and-swap write is a no-op.
            "equals_authority"
        } else if let Some(mixed) =
            agent_doc_document::component_exclusivity::merge_mixes_component_versions(
                baseline,
                authority,
                disk,
                &plan.merged_doc,
            )
        {
            // `#queuecutexclusive` fail-closed guard, checked against the
            // ORIGINAL disk: a component both planes changed must be exactly one
            // plane's body. A body built from both cuts is the duplicate every
            // incident since 2026-09-30 adopted; refusing it proceeds on the
            // authority, which never stalls.
            rungs.push(format!(
                "{:?}=would_mix_component_versions({mixed})",
                plan.engine
            ));
            continue;
        } else if let Some(changed) =
            agent_doc_document::component_exclusivity::merge_changes_editor_attributed(
                authority,
                &plan.merged_doc,
                &editor_attributed,
            )
        {
            // `#keylogrebase` fail-closed guard: a component the provenance
            // projection attributed to the editor's in-flight typing reaches
            // the authority only through the editor's own ops.
            rungs.push(format!(
                "{:?}=would_readd_editor_typing({changed})",
                plan.engine
            ));
            continue;
        } else if let Some(invented) =
            merge_invents_list_items(baseline, authority, &plan.merged_agent, &plan.merged_doc)
        {
            // `#admissionmergedup`: count conservation. A merge may not hold more
            // items than baseline + each side's additions; formal model
            // `formal/tla/AdmissionSplitMerge.tla`. Disk's additions are counted
            // on the side the engine MERGED (`merged_agent`): a revision twin
            // projected out of disk must not leave a spare slot in this bound.
            rungs.push(format!(
                "{:?}=would_duplicate_items({invented})",
                plan.engine
            ));
            continue;
        } else if let Some(invented) = agent_doc_merge::revision_twin::merge_invents_revision_twins(
            baseline,
            &plan.merged_agent,
            authority,
            &plan.merged_doc,
        ) {
            // `typingforkdup`: the merge put two revisions of one authored item
            // side by side that neither input authored as a pair (e.g. disk holds
            // a NEWER revision than the authority's replica). The live editor
            // owns that item's text; proceed on the authority rather than adopt
            // a duplicate.
            rungs.push(format!(
                "{:?}=would_duplicate_revision({invented})",
                plan.engine
            ));
            continue;
        } else {
            // Coarse diagnostic only: strict line subsumption, so an *edited* disk
            // line legitimately reads `false` (the operator's newer version of that
            // line is what survived). It is not a "nothing was lost" proof, and must
            // not be read as one — hence the name.
            let disk_lines_all_verbatim =
                agent_doc_document::admission_divergence::authority_subsumes_disk(
                    &durable_disk,
                    &durable_merged,
                );
            rungs.push(format!(
                "{:?}=adopted(disk_lines_all_verbatim={disk_lines_all_verbatim} advisories={} conflicts={})",
                plan.engine,
                plan.conflict_advisories.len(),
                plan.conflicts.len()
            ));
            adopted = Some(plan.merged_doc);
            break;
        };
        rungs.push(format!("{:?}={outcome}", plan.engine));
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "admission_three_way_merge file={} outcome={} rungs=[{}] baseline_hash={} authority_hash={} disk_hash={} merged_hash={} (#admissionsplitmerge)",
            file.display(),
            if adopted.is_some() {
                "adopted"
            } else {
                "proceed_on_authority"
            },
            rungs.join(" "),
            durable_short_content_hash(baseline),
            durable_short_content_hash(authority),
            durable_short_content_hash(disk),
            adopted
                .as_deref()
                .map(agent_doc_hash::short_content_hash)
                .unwrap_or_else(|| "none".to_string()),
        ),
    );
    Ok(adopted)
}

/// `#keylogrebase`: the provenance projection for admission, or `None` when no
/// provenance exists.
///
/// Provenance requires all three of: an editor-op epoch based on the baseline
/// whose replay reproduces the editor's attested tip
/// ([`agent_doc_op_capture_io::verified_editor_ops_for_base`]), and a replay that
/// passes through the authority's cut. A ledger labelled with the wrong base --
/// the pre-`#keylogrebase` failure after a compaction -- fails the first two, so
/// admission keeps the provenance-free behaviour instead of trusting it.
fn editor_provenance_projection(
    file: &Path,
    baseline: &str,
    authority: &str,
    disk: &str,
    op_replayed_authority: &str,
) -> Option<agent_doc_document::component_exclusivity::EditorProvenanceProjection> {
    let ops = match agent_doc_op_capture_io::verified_editor_ops_for_base(file, baseline) {
        Ok(Some(ops)) => ops,
        Ok(None) => return None,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "admission_editor_provenance unavailable=ledger_read_failed error={err:#} #keylogrebase"
                ),
            );
            return None;
        }
    };
    if !agent_doc_merge::document_cell::replay_reaches_cut(baseline, &ops, op_replayed_authority) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "admission_editor_provenance unavailable=lineage_misses_authority_cut ops={} #keylogrebase",
                ops.len()
            ),
        );
        return None;
    }
    Some(
        agent_doc_document::component_exclusivity::project_disk_for_editor_provenance(
            baseline,
            authority,
            disk,
            |visit| {
                agent_doc_merge::crdt::for_each_editor_replay_state(baseline, &ops, visit);
            },
        ),
    )
}

/// Short hash of a plane's DURABLE content — transient agent-doc markers removed.
///
/// A refusal that prints only raw hashes cannot be told apart from a marker-only
/// false split without reconstructing byte lengths from surrounding
/// `commit_staging` / `document_baseline_checkpoint` lines. Printing both domains
/// makes the two shapes decidable from the refusal text alone.
fn durable_short_content_hash(content: &str) -> String {
    agent_doc_hash::short_content_hash(
        &agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(content),
    )
}

/// Record every non-verbatim three-plane observation with its class and action.
///
/// Verbatim-resolvable observations are the common case on every preflight and
/// are not logged; the three interesting classes are rare and each one names
/// itself, so `grep admission_divergence` decides a wedge without replaying the
/// surrounding log.
fn log_admission_divergence_assessment(
    file: &Path,
    baseline: &str,
    authority: &str,
    disk: &str,
    assessment: agent_doc_document::admission_divergence::AdmissionAssessment,
) {
    if assessment.divergence
        == agent_doc_document::admission_divergence::AdmissionDivergence::Resolvable
    {
        return;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "admission_divergence file={} {} baseline_hash={} authority_hash={} disk_hash={} durable_baseline_hash={} durable_authority_hash={} durable_disk_hash={} baseline_len={} authority_len={} disk_len={} (#admissionmarkersplit)",
            file.display(),
            assessment.diagnostic_fields(),
            agent_doc_hash::short_content_hash(baseline),
            agent_doc_hash::short_content_hash(authority),
            agent_doc_hash::short_content_hash(disk),
            durable_short_content_hash(baseline),
            durable_short_content_hash(authority),
            durable_short_content_hash(disk),
            baseline.len(),
            authority.len(),
            disk.len(),
        ),
    );
}

/// Adopt a reconciled revision into the live CRDT authority.
///
/// Two callers reach this, and neither is a generic disk-wins rule: a proven newer
/// durable save whose authority branch is unchanged in the DURABLE domain
/// (`FastForwardAuthorityToDisk`), and the merged revision produced by
/// `merge_admission_three_way_split` when both branches advanced
/// (`MergeDiskIntoAuthority`, `#admissionsplitmerge`). In both cases the candidate
/// is already the reconciled result, so the compare-and-swap below is what rechecks
/// the authority cut at the mutation edge. The companion `admission_divergence` /
/// `admission_three_way_merge` log lines name which class produced the candidate.
/// When
/// the authority differed from the baseline only by transient agent-doc markers,
/// this fast-forward drops those markers from the live buffer — they are
/// transport state the next write re-applies, never operator text.
fn adopt_proven_newer_disk_save(
    file: &Path,
    authority: &str,
    disk: &str,
    source: &str,
) -> Result<()> {
    agent_doc_element::element::parse(disk).map_err(|err| {
        QueueAuthorityUnavailable::new(format!(
            "newer durable queue save is not a structurally valid agent document: {err:#}"
        ))
    })?;
    let write = if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        agent_doc_crdt_relay_io::apply_cp_write_for_file(file, authority, disk, source)?
            .ok_or_else(|| {
                QueueAuthorityUnavailable::new(
                    "embedded editor authority refused the proven durable-save fast-forward",
                )
            })?
    } else {
        agent_doc_controller_io::project_controller::apply_cp_write_via_controller_model_for_doc(
            file, authority, disk, source,
        )?
        .ok_or_else(|| {
            QueueAuthorityUnavailable::new(
                "attached editor authority refused the proven durable-save fast-forward",
            )
        })?
    };
    anyhow::ensure!(
        write.content_hash == agent_doc_hash::content_hash(disk),
        "durable-save fast-forward returned the wrong canonical revision"
    );
    Ok(())
}

/// A native editor save can reach disk one observation before the editor's
/// registered CRDT cut catches up. Fast-forward the validated durable branch
/// only from its exact unchanged baseline, then give CRDT/editor projection the
/// same bounded chance to converge that other attached writes receive.
fn observe_queue_authority_after_native_save_with_bounded_retry(
    file: &Path,
    source: &str,
    attempts: u32,
    observe: impl FnMut(&Path) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>>,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    observe_queue_authority_after_native_save_with_bounded_retry_and_adopt(
        file,
        source,
        attempts,
        observe,
        adopt_proven_newer_disk_save,
    )
}

fn observe_queue_authority_after_native_save_with_bounded_retry_and_adopt(
    file: &Path,
    source: &str,
    attempts: u32,
    mut observe: impl FnMut(&Path) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>>,
    mut adopt: impl FnMut(&Path, &str, &str, &str) -> Result<()>,
) -> Result<Option<agent_doc_crdt_relay_io::CurrentText>> {
    let attempts = attempts.max(1);
    let mut observed = observe(file);
    // Preserve one exact-base operation epoch across this bounded reconciliation.
    // The first CP projection fences the active epoch, but subsequent observations
    // are still retries of the same admission split and must use the same durable
    // lineage proof rather than reclassifying stale disk as an independent writer.
    let initial_operator_cut = match &observed {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => Some(text.clone()),
        _ => None,
    };
    let admission_lineage = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
        .and_then(|baseline| {
            agent_doc_op_capture_io::editor_ops_for_base_or_retained(file, &baseline)
                .ok()
                .flatten()
                .zip(initial_operator_cut)
                .map(|(ops, operator_cut)| (baseline, operator_cut, ops))
        });
    for attempt in 1..=attempts {
        let newer_disk = match &observed {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
                text, live_editors, ..
            })) if *live_editors > 0 => {
                let assessed =
                    if let Some((baseline, operator_cut, ops)) = admission_lineage.as_ref() {
                        disk_edit_newer_than_registered_authority_with_lineage(
                            file,
                            text,
                            baseline,
                            operator_cut,
                            Some(ops),
                        )
                    } else {
                        disk_edit_newer_than_registered_authority(file, text)
                    };
                assessed.map_err(|err| {
                    QueueAuthorityUnavailable::new(format!(
                        "could not verify retained queue authority: {err:#}"
                    ))
                })?
            }
            _ => return observed,
        };
        let Some(disk) = newer_disk else {
            return observed;
        };
        let authority = match &observed {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => text,
            _ => unreachable!("newer_disk is only populated for a live current authority"),
        };
        adopt(file, authority, &disk, source).map_err(|err| {
            QueueAuthorityUnavailable::new(format!(
                "could not fast-forward the proven newer durable queue save: {err:#}"
            ))
        })?;
        let admission = if attempt < attempts {
            "retrying"
        } else {
            "refused"
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_authority_stale_behind_disk file={} source={} authority_hash={} disk_hash={} attempt={}/{} durable_save_fast_forward=applied admission={}",
                file.display(),
                source,
                agent_doc_hash::short_content_hash(authority),
                agent_doc_hash::short_content_hash(&disk),
                attempt,
                attempts,
                admission,
            ),
        );
        if attempt == attempts {
            return Err(QueueAuthorityUnavailable::new(format!(
                "registered editor authority remained an older baseline cut after the proven newer durable save was fast-forwarded and observed {attempts} times"
            ))
            .into());
        }
        std::thread::sleep(QUEUE_AUTHORITY_RETRY_BACKOFF);
        observed = observe(file);
    }
    observed
}

/// Parse the `agent:queue` entries of `content` (empty when absent/unparseable).
fn queue_entries_in_content(content: &str) -> Vec<agent_doc_queue::document_queue::QueueEntry> {
    agent_doc_element::element::parse(content)
        .ok()
        .and_then(|comps| {
            comps
                .iter()
                .find(|c| c.name == "queue")
                .map(|q| content[q.open_end..q.close_start].to_string())
        })
        .and_then(|body| agent_doc_queue::document_queue::parse(&body).ok())
        .unwrap_or_default()
}

fn queue_entries_for_id(
    content: &str,
    queue_id: &str,
) -> Vec<agent_doc_queue::document_queue::QueueEntry> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return Vec::new();
    };
    let Ok(Some(queue)) =
        agent_doc_queue::queue_set::component_for_id(content, &components, queue_id)
    else {
        return Vec::new();
    };
    agent_doc_queue::document_queue::parse(queue.content(content)).unwrap_or_default()
}

/// `#unstrikelost`: snapshot-struck queue identities the operator re-armed.
///
/// The durable editor-op epoch records only operator keystrokes, captured
/// against the base the editor last saw. When that base is the committed
/// snapshot, replaying the ops onto it reconstructs the operator's own queue cut
/// independently of any stale editor buffer or agent projection. A head that is
/// struck in the snapshot but live in that operator cut was un-struck (or
/// re-typed) by the operator, so queue maintenance must not re-strike it. The
/// active epoch is preferred; the newest retained checkpoint covers an epoch a
/// non-operator projection already closed before this maintenance pass ran.
/// A head the operator typed again as a fresh line in the active epoch is a
/// re-arm as well (`#retypestruck`), whatever the epoch's base. With no
/// matching op evidence the set is empty and `#qeditdupguard` applies.
///
/// `#unstrikelost` (unreadable evidence): an epoch that cannot be READ is an error, never
/// an empty set. Empty evidence re-strikes every contested head, so mapping a
/// dead controller (or an unreadable `state.db`) to "no evidence" silently
/// re-struck the operator's unstrike and persisted it.
fn operator_rearmed_queue_identities(
    file: &Path,
    snapshot_text: &str,
    snapshot_queue_entries: &[agent_doc_queue::document_queue::QueueEntry],
) -> Result<std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity>> {
    #[cfg(test)]
    rearm_evidence_fault::take()?;
    let active_ops = agent_doc_op_capture_io::editor_ops_for_base(file, snapshot_text)
        .context("read the active editor-op epoch")?;
    let operator_cut = match active_ops
        .and_then(|ops| agent_doc_merge::crdt::replay_editor_ops(snapshot_text, &ops))
    {
        Some(cut) => Some(cut),
        None => agent_doc_op_capture_io::last_editor_text_for_base(file, snapshot_text)
            .context("read the retained editor-op checkpoint")?,
    };
    // `#retypestruck`: a head the operator typed again as a fresh line is a
    // re-arm too. This evidence is base-independent, so it survives the common
    // case where the active epoch's base is not the committed snapshot and the
    // exact-base replay above has nothing to offer.
    let retyped = agent_doc_queue::document_queue::operator_retyped_queue_identities(
        snapshot_queue_entries,
        &agent_doc_op_capture_io::active_operator_inserted_runs(file)
            .context("read the active editor-op epoch's inserted runs")?,
    );
    if !retyped.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_operator_retype_preserved file={} heads={} (#retypestruck)",
                file.display(),
                retyped.len()
            ),
        );
    }
    let Some(operator_cut) = operator_cut else {
        return Ok(retyped);
    };
    let mut rearmed = agent_doc_queue::document_queue::operator_rearmed_queue_identities(
        snapshot_queue_entries,
        &queue_entries_in_content(&operator_cut),
    );
    if !rearmed.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_operator_rearm_preserved file={} heads={} (#unstrikelost)",
                file.display(),
                rearmed.len()
            ),
        );
    }
    rearmed.extend(retyped);
    Ok(rearmed)
}

/// Operator re-arm evidence for the live heads the snapshot holds struck
/// (`#unstrikelost`, unreadable evidence).
///
/// Reads nothing when no live head is snapshot-struck: the evidence cannot
/// change the outcome there. Otherwise the read is retried a bounded number of
/// times (re-ensuring the controller in between, since the usual cause is a
/// controller that died after this pass launched it) and then refused with
/// [`QueueAuthorityUnavailable`]: preflight must not converge, and so must not
/// re-strike or resurrect a contested head, on evidence it could not read.
fn operator_rearm_evidence_for_maintenance(
    file: &Path,
    snapshot_text: &str,
    snapshot_queue_entries: &[agent_doc_queue::document_queue::QueueEntry],
    live_entries: &[agent_doc_queue::document_queue::QueueEntry],
) -> Result<std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity>> {
    let contested = agent_doc_queue::document_queue::live_snapshot_struck_identities(
        live_entries,
        snapshot_queue_entries,
    );
    if contested.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let attempts = queue_authority_attempts();
    let mut attempt = 1;
    loop {
        match operator_rearmed_queue_identities(file, snapshot_text, snapshot_queue_entries) {
            Ok(rearmed) => return Ok(rearmed),
            Err(err) => {
                let admission = if attempt < attempts {
                    "retrying"
                } else {
                    "refused"
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_rearm_evidence_unavailable file={} contested={} attempt={attempt}/{attempts} admission={admission} error={} (#unstrikelost)",
                        file.display(),
                        contested.len(),
                        compact_error_for_log(&err),
                    ),
                );
                if attempt >= attempts {
                    return Err(QueueAuthorityUnavailable::new(format!(
                        "operator re-arm evidence for {} snapshot-struck live queue head(s) \
                         is unreadable after {attempts} attempt(s); converging without it \
                         would re-strike an operator unstrike: {err:#}",
                        contested.len()
                    ))
                    .into());
                }
            }
        }
        // Best effort: the read retries either way, and its error is what
        // the refusal reports.
        let _ensured =
            agent_doc_controller_io::project_controller::ensure_controller_running_for_file(file);
        std::thread::sleep(QUEUE_AUTHORITY_RETRY_BACKOFF);
        attempt += 1;
    }
}

fn compact_error_for_log(err: &anyhow::Error) -> String {
    format!("{err:#}").replace(['\n', '\r'], " ")
}

/// Deterministic fault injection for the re-arm evidence read
/// (`#unstrikelost`, unreadable evidence): the live trigger is a controller that dies
/// between this pass launching it and the evidence read, which no test can
/// schedule. Thread-local, because queue maintenance runs on the test thread.
#[cfg(test)]
mod rearm_evidence_fault {
    use std::cell::Cell;

    thread_local! {
        static REMAINING: Cell<u32> = const { Cell::new(0) };
        static TAKEN: Cell<u32> = const { Cell::new(0) };
    }

    /// Fail the next `count` evidence reads on this thread.
    pub(super) fn arm(count: u32) {
        REMAINING.with(|remaining| remaining.set(count));
        TAKEN.with(|taken| taken.set(0));
    }

    /// Evidence reads attempted on this thread since [`arm`].
    pub(super) fn taken() -> u32 {
        TAKEN.with(Cell::get)
    }

    pub(super) fn take() -> anyhow::Result<()> {
        TAKEN.with(|taken| taken.set(taken.get() + 1));
        REMAINING.with(|remaining| {
            let left = remaining.get();
            if left == 0 {
                return Ok(());
            }
            remaining.set(left - 1);
            Err(anyhow::anyhow!(
                "injected: Lazily actor socket refused the editor-op projection read"
            ))
        })
    }
}

pub fn run_queue_maintenance(file: &Path, diff: Option<&str>) -> Result<QueueState> {
    run_queue_maintenance_with_coin_gate(file, diff, &FreeTextCoinGate::structural_only())
}

/// [`run_queue_maintenance`] with the typing-gate evidence preflight's
/// operator-edit quiescence wait gathered (`#halftypedcoin`): queue free text
/// the gate still holds is left as free text instead of being coined into a
/// backlog id this pass.
pub fn run_queue_maintenance_with_coin_gate(
    file: &Path,
    diff: Option<&str>,
    coin_gate: &FreeTextCoinGate,
) -> Result<QueueState> {
    // #sqedit-race Phase 2: defer ALL queue maintenance mutation while a different,
    // live process holds a fresh queue-edit lease (a direct `queue prune-noise` /
    // `queue consume` in flight). Round-tripping a torn intermediate queue through
    // the #mirrorall mirror / backlog→queue sync / #7r2s pin / dedup re-mangles
    // entries (double-pins, dropped heads). The brief lease makes this a yield, not
    // a stall: the direct edit completes in well under a TTL and the next preflight
    // performs maintenance normally on the settled queue.
    if let Some(holder_pid) =
        agent_doc_queue_io::queue_edit_owner::foreign_queue_edit_in_flight(&file.to_string_lossy())
    {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_deferred reason=queue_edit_lease holder_pid={holder_pid} (#sqedit-race)"
            ),
        );
        eprintln!(
            "[preflight] queue: deferring maintenance — direct queue edit in flight (pid {holder_pid}; #sqedit-race)"
        );
        return Ok(QueueState::default());
    }
    let (mut content, authority_queue_unresolved_prompts) =
        match observe_queue_authority_after_native_save_with_bounded_retry(
            file,
            "preflight_queue_maintenance",
            queue_authority_attempts(),
            |file| {
                current_text_via_preflight_authority_retrying(file, "preflight_queue_maintenance")
            },
        ) {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
                text, semantics, ..
            })) => (
                text,
                semantics.map(|semantics| semantics.queue_unresolved_prompts),
            ),
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) | Ok(None) => {
                match std::fs::read_to_string(file) {
                    Ok(content) => (content, None),
                    Err(_) => return Ok(QueueState::default()),
                }
            }
            Ok(Some(
                agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
                | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending,
            )) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_deferred file={} reason=editor_authority_not_current",
                        file.display()
                    ),
                );
                return Err(QueueAuthorityUnavailable::new(
                    "editor authority is attached but its current replica cut is unavailable after bounded recovery",
                )
                .into());
            }
            Err(err) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_deferred file={} reason=current_text_unavailable error={}",
                        file.display(),
                        err
                    ),
                );
                return Err(QueueAuthorityUnavailable::new(format!(
                    "current queue authority is unavailable after bounded recovery: {err:#}"
                ))
                .into());
            }
        };
    // `content` already came from the CP/editor authority above. Do not reread
    // disk or run a second queue-only merge here: that used to compare the live
    // frontier with itself, making the supposed deletion-adoption branch dead
    // while obscuring which authority maintenance actually mutated.
    let authority_baseline = content.clone();
    let _nested_claim_steer =
        agent_doc_queue_io::queue_claim::nested_claim_steer_for(file, &content);
    let mut current_content = content.clone();
    let effective_prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(
            &current_content,
            file,
        )?;
    let mut mutated = false;
    let mut components = match agent_doc_element::element::parse(&current_content) {
        Ok(cs) => cs,
        Err(_) => return Ok(QueueState::default()),
    };
    let exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&current_content);
    if components.iter().all(|c| c.name != "queue")
        && collect_actionable_free_text_prompts(
            exchange_prompt.as_deref(),
            &[],
            &FreeTextAdmissionScope::None,
        )
        .has_work()
    {
        current_content = append_empty_agent_component(&current_content, "queue");
        content = current_content.clone();
        components = match agent_doc_element::element::parse(&current_content) {
            Ok(cs) => cs,
            Err(_) => return Ok(QueueState::default()),
        };
        mutated = true;
        eprintln!("[preflight] queue: created agent:queue for admitted free-text work");
    }
    // A queue id is durable work identity, not a parser fallback. As soon as a
    // second queue exists, assign every omitted id through the same binary-owned
    // maintenance write as the rest of preflight. All graph readers in this pass
    // then see the exact projection that will be persisted; subsequent passes are
    // idempotent because only an absent `id` is eligible for backfill.
    match agent_doc_queue::queue_set::backfill_missing_queue_ids(&current_content, &components) {
        Ok(Some(normalized)) => {
            current_content = normalized;
            content = current_content.clone();
            components = agent_doc_element::element::parse(&current_content)?;
            mutated = true;
            eprintln!(
                "[preflight] queue: assigned deterministic ids to unkeyed multi-queue components"
            );
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[preflight] queue topology warning: {err}");
            return Ok(QueueState::default());
        }
    }
    let owner_selection = agent_doc_queue_io::subagent_dispatch::owner_selection_claims_for_content(
        file,
        &current_content,
    );
    let queue_control =
        agent_doc_queue::queue_continuation::document_queue_control(&current_content);
    let selected_queue_id =
        match agent_doc_queue::queue_set::QueueSet::parse(&current_content, &components) {
            Ok(queues) => match queues
                .selected_reaching_inline_work_with_control(
                    &owner_selection,
                    queue_control.as_deref(),
                )
                .or_else(|| queues.selected())
                .or_else(|| queues.blocks().first())
            {
                Some(queue) => queue.id.clone(),
                None => return Ok(QueueState::default()),
            },
            Err(err) => {
                eprintln!("[preflight] queue topology warning: {err}");
                return Ok(QueueState::default());
            }
        };
    let mut comp = agent_doc_queue::queue_set::component_for_id(
        &current_content,
        &components,
        &selected_queue_id,
    )?
    .context("queue maintenance: selected queue component vanished")?
    .clone();

    let body = &current_content[comp.open_end..comp.close_start];
    let entries = match agent_doc_queue::document_queue::parse(body) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[preflight] queue parse warning: {}", e);
            return Ok(QueueState::default());
        }
    };
    // Publish authoritative markdown before any worklist projection. The
    // controller's document-scoped Computed persists terminal lifecycle facts
    // and returns only after its durable receipt feeds back into projection.
    agent_doc_queue_io::queue_consume::observe_authoritative_queue_state(file, &current_content)?;

    let mut entries = entries;
    let mut free_text_promotions: Vec<(String, String)> = Vec::new();
    let mut queue_warnings = Vec::new();
    let mut synced_queue_ids = Vec::new();
    let mut freshly_admitted_queue_ids = std::collections::HashSet::new();
    let mut source_queue_priority = false;
    let mut queue_tag_attrs_normalized = false;

    let raw_queue_tag = &current_content[comp.open_start..comp.open_end];
    let normalized_queue_tag =
        agent_doc_queue::document_queue::normalize_queue_tag_attrs(raw_queue_tag);
    if normalized_queue_tag != raw_queue_tag {
        let mut rebuilt = String::with_capacity(current_content.len());
        rebuilt.push_str(&current_content[..comp.open_start]);
        rebuilt.push_str(&normalized_queue_tag);
        rebuilt.push_str(&current_content[comp.open_end..]);
        current_content = rebuilt;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &components,
            &selected_queue_id,
        )?
        .context("queue maintenance: queue component vanished after tag normalization")?
        .clone();
        mutated = true;
        queue_tag_attrs_normalized = true;
        eprintln!("[preflight] queue: normalized malformed queue marker attributes");
    }
    // `#qfoldedhead`: an editor paste can land one `do [#id]` per line with every
    // bullet but the first replaced by continuation indent. Those lines parse as
    // inert `Freeform` residue — never drainable, unreachable by strike, and
    // invisible to the backlog mirror (which counts their ids as already
    // present), so the fold silently swallows every id after the first and never
    // heals itself. Re-segment before any worklist projection reads the queue.
    if let Some(healed) = agent_doc_queue::document_queue::heal_folded_id_per_line_prompts(&entries)
    {
        let promoted = healed.len() - entries.len();
        let new_body = agent_doc_queue::document_queue::render(&healed);
        current_content = comp.replace_content(&current_content, &new_body);
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &components,
            &selected_queue_id,
        )?
        .context("queue maintenance: queue component vanished after folded-head heal")?
        .clone();
        entries = healed;
        mutated = true;
        queue_warnings.push(PreflightWarning {
            code: "queue_folded_head_recovered".to_string(),
            message: format!(
                "{}: recovered {promoted} queue head(s) that an editor paste folded into \
                 bullet-less continuation lines. Until re-segmented they were inert: not \
                 drainable, unreachable by strike, and counted as already-present by the \
                 backlog→queue mirror (#qfoldedhead).",
                file.display()
            ),
            document_agent: None,
            active_harness: None,
        });
        eprintln!(
            "[preflight] queue: re-segmented {promoted} folded queue head(s) into addressable prompts (#qfoldedhead)"
        );
    }
    let persisted_active_before_binding = frontmatter::parse(&current_content)
        .ok()
        .and_then(|(fm, _)| fm.queue_active)
        .unwrap_or(false);
    let control_snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    if let (projected, true) = converge_queue_control_binding_content(
        &current_content,
        control_snapshot_content.as_deref(),
    )? {
        current_content = projected;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &components,
            &selected_queue_id,
        )?
        .context("queue maintenance: queue component vanished after control binding sync")?
        .clone();
        mutated = true;
        eprintln!("[preflight] queue: synchronized queue marker/frontmatter control binding");
    }
    // `#queueeditgo`: an operator edit to the queue's prompts arms `go` unless
    // the operator holds the queue with `queue: pause`.
    if let (projected, true) =
        infer_queue_go_from_prompt_edit(&current_content, control_snapshot_content.as_deref())?
    {
        current_content = projected;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &components,
            &selected_queue_id,
        )?
        .context("queue maintenance: queue component vanished after queue-edit go inference")?
        .clone();
        mutated = true;
        eprintln!("[preflight] queue: operator queue edit armed `queue: go` (#queueeditgo)");
        agent_doc_ops_log_io::log_op(
            file,
            &format!("queue_edit_go_inferred file={}", file.display()),
        );
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_project_root_io::project_root_containing(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let queue_active_for_free_text =
        queue_currently_active_for_free_text_admission(&current_content, &comp.attrs);
    let queue_paused_by_operator = {
        let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
        explicit_queue_pause_mode(&comp.attrs, frontmatter_queue_control(&fm))
    };
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let queue_free_text_scope = queue_free_text_admission_scope(
        &current_content,
        &comp.attrs,
        &entries,
        snapshot_content.as_deref(),
    );
    // `#halftypedcoin`: never coin a queue line the typing gate still holds.
    let (queue_free_text_scope, held_free_text) =
        gate_free_text_admission_scope(queue_free_text_scope, &entries, coin_gate);
    if !held_free_text.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "free_text_coin_held file={} held={} observed_typing={} (#halftypedcoin)",
                file.display(),
                held_free_text.len(),
                coin_gate.observed.is_some(),
            ),
        );
        eprintln!(
            "[preflight] queue: held {} free-text line(s) from backlog coining; still being typed (#halftypedcoin)",
            held_free_text.len()
        );
    }
    let exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&current_content);
    let document_id = agent_doc_hash::document_id_for_path(file);
    // Reading an external archive belongs only to a pass that may mint an id;
    // unrelated preflight maintenance must not acquire a new archive failure.
    let archived_identity_ids = if collect_actionable_free_text_prompts(
        exchange_prompt.as_deref(),
        &entries,
        &queue_free_text_scope,
    )
    .has_work()
    {
        agent_doc_element_backlog_io::done_archive::external_done_archive_ids(
            file,
            &current_content,
        )?
    } else {
        HashSet::new()
    };
    if let Some(prepared_admission) = prepare_free_text_admission_for_queue_with_reserved(
        &current_content,
        &entries,
        exchange_prompt.as_deref(),
        &queue_free_text_scope,
        // `#queueeditgo`: an operator `queue: pause` holds the queue; admission
        // may still file the free text, but it must not start the queue.
        !queue_active_for_free_text && !queue_paused_by_operator,
        FreeTextAdmissionTarget {
            document_id: &document_id,
            queue_id: &selected_queue_id,
        },
        &archived_identity_ids,
    )? {
        queue_warnings.extend(
            prepared_admission
                .warnings
                .iter()
                .map(|message| PreflightWarning {
                    code: "free_text_admission_item_skipped".to_string(),
                    message: message.clone(),
                    document_agent: None,
                    active_harness: None,
                }),
        );
        let (execution, warnings) = resolve_free_text_execution(
            file,
            &prepared_admission.content,
            project_root.as_deref(),
            &prepared_admission.unique_ids,
        )?;
        let execution = match execution {
            ResolvedFreeTextExecution::Goal => FreeTextAdmissionExecution::Goal,
            ResolvedFreeTextExecution::Queue => FreeTextAdmissionExecution::Queue,
        };
        let promotions = prepared_admission
            .promotions
            .iter()
            .map(|promotion| (promotion.source_text.clone(), promotion.id.clone()))
            .collect::<Vec<_>>();
        let admission = prepared_admission.finish(execution)?;
        // `#freetextqueue`: a claimed free-text head promoted into `do [#id]`
        // keeps its owner — the claim follows the exact promotion receipt, so
        // the new id head never resurfaces as unclaimed dispatch work.
        // Best-effort: a claim-store failure must not abort maintenance.
        if let Err(err) =
            agent_doc_queue_io::queue_claim::transfer_promoted_claims(file, &promotions)
        {
            eprintln!(
                "[preflight] queue: WARNING: could not carry claims onto promoted heads: {err:#}"
            );
        }
        free_text_promotions.extend(promotions);
        current_content = admission.content;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &components,
            &selected_queue_id,
        )?
        .context("queue maintenance: queue component vanished after free-text admission")?
        .clone();
        let body = &current_content[comp.open_end..comp.close_start];
        entries = agent_doc_queue::document_queue::parse(body)
            .context("queue maintenance: failed to parse queue after free-text admission")?;
        freshly_admitted_queue_ids.extend(
            admission
                .queued_ids
                .iter()
                .map(|id| id.trim().to_ascii_lowercase()),
        );
        synced_queue_ids.extend(admission.queued_ids);
        queue_warnings.extend(warnings);
        mutated = true;
        eprintln!(
            "[preflight] queue: admitted {} free-text prompt(s) into backlog ({})",
            admission.admitted_count, admission.execution_label
        );
    }

    // `#ynra`: collect `agent:done` ids ONCE up front. Ordinarily the
    // backlog→queue sync below must never re-mint a `do [#id]` whose id is
    // already completed (archived in `agent:done`) — otherwise the strike pass
    // removes it every cycle, the sync re-injects it the next cycle, and the
    // queue churns forever on a completed ref.
    //
    // `#qrepeatid`: an operator may intentionally add the same work again. A
    // newly admitted queue id, or any currently-open backlog item with that id,
    // is a fresh incarnation even when an older incarnation with the same
    // stable id exists in `agent:done`. Exclude those ids from both the mirror
    // suppression and the preflight done-strike so the repeat reaches the
    // agent. The live backlog checkbox is the durable incarnation boundary, so
    // maintenance retries preserve the repeat until closeout reaps that item.
    let done_ids = collect_agent_done_ids(file, &content)?;
    let mut reactivated_done_ids = freshly_admitted_queue_ids;
    for id in &done_ids {
        let active_now =
            agent_doc_queue::queue_consume::head_id_names_open_backlog_item(&current_content, id);
        if active_now {
            reactivated_done_ids.insert(id.trim().to_ascii_lowercase());
        }
    }
    reactivated_done_ids.retain(|id| {
        done_ids
            .iter()
            .any(|done_id| done_id.eq_ignore_ascii_case(id))
    });
    let sync_done_ids = done_ids
        .iter()
        .filter(|id| !reactivated_done_ids.contains(&id.trim().to_ascii_lowercase()))
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    if !reactivated_done_ids.is_empty() {
        eprintln!(
            "[preflight] queue: preserving {} repeated completed id incarnation(s) (#qrepeatid)",
            reactivated_done_ids.len()
        );
    }

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog`
    // component carries a `queue` attribute, regenerate the queue `do [#id]`
    // prompts from its active items BEFORE activation so a freshly synced queue
    // can auto-activate on the same cycle. `agent:icebox` is intentionally not a
    // component-level sync source; parked work must be moved to backlog or
    // explicitly marked for enqueue. Per-item enqueue markers
    // (#queue-enqueue-action) append marked ids without requiring the component
    // attribute.
    // `#queuestopretire`: backlog ids an active non-`go` queue held out this
    // cycle. A drain may clear the queue control only when none were held: with
    // no control the queue defaults to `go`, so the next cycle would mirror these
    // held ids and run them, silently upgrading a `start` drain into a
    // continuous backlog loop.
    let mut backlog_ids_held_out_of_queue = 0usize;
    // `#loopnoinsert`: the mirror writes into the SELECTED queue. When every
    // `agent:run` head is claimed, selection falls through to an `agent:loop`;
    // a backlog id mirrored there becomes a recurring loop row that every later
    // pass restores (2026-10-26: 12 one-off rows resurrected into `issues`).
    // Skip the mirror for a selected loop; it lands in `agent:run` on the next
    // pass that selects one.
    let selected_queue_is_loop = agent_doc_queue::queue_graph::is_loop_queue(&comp);
    if selected_queue_is_loop
        && agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content)
            .is_some()
    {
        eprintln!(
            "[preflight] queue: backlog mirror skipped for agent:loop id `{selected_queue_id}`; loops never receive mirrored ids (#loopnoinsert)"
        );
    }
    if let Some(sync_request) = (!selected_queue_is_loop)
        .then(|| agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content))
        .flatten()
    {
        let mode = sync_request.mode;
        source_queue_priority = sync_request.priority;
        // #provauth2: honor operator queue deletes. An id the operator deleted
        // from the live queue (active in the committed snapshot, now entirely
        // gone — not merely struck/consumed) is tombstoned so the backlog→queue
        // mirror does not resurrect it ("I deleted items but they reappeared").
        // The tombstone self-clears when the operator re-adds the id as an active
        // head. This makes an operator delete authoritative, the same way #ynra
        // keeps *completed* ids out — but for *operator-deleted* uncompleted ids.
        let tombstones = {
            let snapshot_active_ids: std::collections::HashSet<String> =
                agent_doc_snapshot_io::load_document_baseline(file)
                    .ok()
                    .flatten()
                    .and_then(|snap| {
                        let comps = agent_doc_element::element::parse(&snap).ok()?;
                        let q = agent_doc_queue::queue_set::component_for_id(
                            &snap,
                            &comps,
                            &selected_queue_id,
                        )
                        .ok()??;
                        let body = &snap[q.open_end..q.close_start];
                        let snap_entries = agent_doc_queue::document_queue::parse(body).ok()?;
                        Some(
                            snap_entries
                                .iter()
                                .filter(|e| {
                                    matches!(
                                        e,
                                        agent_doc_queue::document_queue::QueueEntry::Prompt(_)
                                    )
                                })
                                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
            let current_all_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect();
            let current_active_ids: std::collections::HashSet<String> = entries
                .iter()
                .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect();
            agent_doc_queue_io::queue_tombstone::reconcile_for_file(
                file,
                &snapshot_active_ids,
                &current_all_ids,
                &current_active_ids,
            )
        };
        // #backlog-queue-sync-pending-add-amplification (decision B/C): while the
        // queue is already running (persisted-active auto-loop), do NOT promote
        // freshly-added backlog items into the live queue. Re-mirroring on every
        // cycle injected each new `--pending-add` as a `do [#id]` head, growing
        // the queue unboundedly and tripping pending_done_guard on each finalize.
        // Restrict the sync to ids already present as queue heads so captured
        // follow-ups wait for the NEXT activation instead of joining mid-loop. A
        // fresh activation (queue not yet active) still mirrors the full backlog.
        let incoming_frontmatter = frontmatter::parse(&content).ok().map(|(fm, _)| fm);
        let persisted_active_incoming = incoming_frontmatter
            .as_ref()
            .and_then(|fm| fm.queue_active)
            .unwrap_or(false);
        // `#backlog-queue-empty-active-repopulate`: gate the empty-active-queue
        // repopulation on the queue's explicit `go` control. `go` (frontmatter
        // `queue: go` or a marker-side `go` token) opts
        // into continuous-backlog-loop: when the live queue is fully drained (0
        // un-struck prompts), repopulate from the full active backlog instead of
        // holding. Without `go` (including a plain `queue: start` activation or
        // persisted-active queue), keep the drain-then-stop hold.
        let queue_go_mode = explicit_queue_go_mode(
            &comp.attrs,
            incoming_frontmatter
                .as_ref()
                .and_then(frontmatter_queue_control),
        );
        let queue_explicitly_stopped = explicit_queue_stop_mode(
            &comp.attrs,
            incoming_frontmatter
                .as_ref()
                .and_then(frontmatter_queue_control),
        );
        let sync_plan = agent_doc_queue::backlog_sync::plan_auto_backlog_queue_sync_ids(
            agent_doc_queue::backlog_sync::AutoBacklogQueueSyncInput {
                requested_ids: &sync_request.ids,
                enqueue_ids: &sync_request.enqueue_ids,
                done_ids: &sync_done_ids,
                tombstones: &tombstones,
                entries: &entries,
                persisted_active_incoming,
                persisted_active_before_binding,
                queue_go_mode,
                queue_explicitly_stopped,
            },
        );
        if sync_plan.completed_excluded_count > 0 {
            eprintln!(
                "[preflight] queue: excluded {} completed id(s) from backlog→queue sync (already in agent:done; #ynra)",
                sync_plan.completed_excluded_count
            );
        }
        if sync_plan.tombstone_suppressed_count > 0 {
            eprintln!(
                "[preflight] queue: suppressed {} operator-deleted id(s) \
                 from backlog→queue mirror (#provauth2 tombstone)",
                sync_plan.tombstone_suppressed_count
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_mirror_tombstone_suppressed file={} count={} (#provauth2)",
                    file.display(),
                    sync_plan.tombstone_suppressed_count
                ),
            );
        }
        match sync_plan.active_policy {
            AutoBacklogQueueSyncPolicy::ExplicitlyStopped if sync_plan.active_held_count > 0 => {
                eprintln!(
                    "[preflight] queue: held {} backlog id(s) out of explicitly stopped queue binding",
                    sync_plan.active_held_count
                );
            }
            AutoBacklogQueueSyncPolicy::HoldFreshIds if sync_plan.active_held_count > 0 => {
                backlog_ids_held_out_of_queue = sync_plan.active_held_count;
                eprintln!(
                    "[preflight] queue: held {} freshly-added backlog id(s) out of the active auto-loop \
                     (they sync at the next activation; #backlog-queue-sync-pending-add-amplification)",
                    sync_plan.active_held_count
                );
            }
            AutoBacklogQueueSyncPolicy::GoModeAppend => {
                eprintln!(
                    "[preflight] queue: go-mode active queue — appending fresh backlog `queue`-attr id(s) \
                     (continuous-backlog-loop; #backlog-queue-attr-populates-in-go-mode)"
                );
            }
            AutoBacklogQueueSyncPolicy::FreshActivation
            | AutoBacklogQueueSyncPolicy::ExplicitlyStopped
            | AutoBacklogQueueSyncPolicy::HoldFreshIds => {}
        }
        let backlog_ids = sync_plan.ids;
        // #goqueuestall: keep agent-undrainable heads out of the auto-drain queue
        // so a `go`-mode queue does not perpetually re-mirror items it cannot run
        // in the current session type. `[operator-verify]` items are always
        // skipped (they need a human); `[clean-session]` items are skipped only
        // while a live editor-IPC listener is active (running them live risks
        // closeout corruption — a clean session re-queues them next cycle).
        {
            let exec_ctxs = agent_doc_queue::queue_continuation::collect_backlog_execution_contexts(
                &components,
                &content,
            );
            if exec_ctxs.values().any(|c| c.is_deferred()) {
                let live_ipc = agent_doc_crdt_relay_io::reliable_sync_editor_live_for_file(file);
                let (_drainable, skipped) =
                    agent_doc_queue::queue_continuation::partition_drainable_backlog_ids(
                        &backlog_ids,
                        &exec_ctxs,
                    );
                if !skipped.is_empty() {
                    let session_label = if live_ipc { "live_ipc" } else { "clean" };
                    for skip in &skipped {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "go_queue_mirror_deferred id=#{} reason={} session={} (#mirrorall)",
                                skip.id, skip.reason, session_label
                            ),
                        );
                    }
                    // #mirrorall (operator directive 2026-06-18): mirror ALL open
                    // `queue`-attr backlog ids into the queue, INCLUDING `[operator-verify]`
                    // items, so the queue is a complete worklist (an operator-verify head
                    // surfaces the operator instructions carried in the item text). Crucially
                    // `backlog_ids` is NOT narrowed to the drainable subset: `head_is_drainable`
                    // already defers operator-verify ids (via `deferred_backlog_ids`), so
                    // mirroring them does NOT re-arm the in-session auto-drain loop
                    // (`queue_drainable_head_count` still excludes them). The supervisor
                    // idle-watch must apply the same drainability defer before a mirrored queue
                    // is resumed (#rz3a), else operator-verify-only heads re-injection-thrash;
                    // the queue stays operator-paused until that companion lands. This
                    // supersedes the prior #goqueuestall/#qcontdrain queue-skip that kept
                    // operator-verify items out of the queue entirely.
                    eprintln!(
                        "[preflight] queue: mirrored {} operator-verify backlog head(s) into the queue \
                         (deferred from auto-drain; #mirrorall)",
                        skipped.len()
                    );
                }
            }
        }
        let sync_components = agent_doc_element::element::parse(&current_content)?;
        let sync_target = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &sync_components,
            &selected_queue_id,
        )?
        .context("queue maintenance: selected queue vanished before backlog sync")?;
        let sibling_ids = agent_doc_queue::backlog_sync::sibling_run_queue_reference_ids(
            &sync_components,
            &current_content,
            sync_target.open_start,
        )?;
        if let Some(synced) = agent_doc_queue::document_queue::sync_backlog_into_queue_with_external_ids(
            &entries,
            &backlog_ids,
            mode,
            &sibling_ids,
        ) {
            let pre_sync_ids = entries
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .collect::<std::collections::HashSet<String>>();
            let mut seen_synced_ids = std::collections::HashSet::new();
            synced_queue_ids = synced
                .iter()
                .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
                .filter(|id| !pre_sync_ids.contains(id))
                .filter(|id| seen_synced_ids.insert(id.clone()))
                .collect();
            let new_body = agent_doc_queue::document_queue::render(&synced);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = agent_doc_queue::queue_set::component_for_id(
                    &current_content,
                    &comps,
                    &selected_queue_id,
                )?
                .context("queue maintenance: selected queue vanished during backlog sync")?;
                q.replace_content(&current_content, &new_body)
            };
            let pre_sync_prompt_count = entries
                .iter()
                .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
                .count();
            eprintln!(
                "[preflight] queue: synced backlog → queue ({:?}, {} active id(s))",
                mode,
                backlog_ids.len()
            );
            if pre_sync_prompt_count == 0 {
                queue_warnings.push(PreflightWarning {
                    code: "backlog_queue_sync_pending".to_string(),
                    message: format!(
                        "{}: a backlog/pending queue sync request populated an empty queue. \
                         The binary synced {} item(s) this cycle. \
                         For manual one-shot sync outside binary preflight: `agent-doc queue sync <FILE>`.",
                        file.display(),
                        synced_queue_ids.len()
                    ),
                    document_agent: None,
                    active_harness: None,
                });
            }
            entries = synced;
            mutated = true;
        }

        // `#fr79` head provenance: record which heads the backlog mirror owns.
        //
        // Ownership is "this id is in the mirror's source set AND has a head" —
        // not merely "inserted this cycle". The mirror is idempotent and
        // restores a head for every open queue-attr backlog id, so it owns those
        // heads continuously; recording only fresh insertions would leave every
        // pre-existing head permanently unattributed.
        //
        // This records provenance ONLY. Nothing is struck here — the strike
        // decision is `queue_head_is_strikable_drift`, and heads with no
        // recorded provenance stay operator-authored and untouchable
        // (`#qauthorder`).
        let mirror_owned: Vec<String> = entries
            .iter()
            .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
            .filter(|id| backlog_ids.iter().any(|b| b.eq_ignore_ascii_case(id)))
            .map(|id| id.to_ascii_lowercase())
            .collect::<std::collections::HashSet<String>>()
            .into_iter()
            .collect();
        if !mirror_owned.is_empty()
            && let Err(err) = agent_doc_project_root_io::project_root_containing(file)
                .context("queue head provenance: no project root for state.db")
                .and_then(|root| {
                    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
                    agent_doc_sqlite::state_store::record_mirrored_queue_heads_in_db(
                        &conn,
                        &file.display().to_string(),
                        &mirror_owned,
                    )
                })
        {
            // Provenance is an optimization for a later strike decision, never a
            // correctness gate: a miss just leaves heads unattributed, which
            // fails safe (never struck). Warn rather than fail the cycle.
            eprintln!(
                "[preflight] warning: failed to record queue head provenance for {}: {err:#}",
                file.display()
            );
        }
    }

    // Queue priority ordering (#backlog-priority-attribute): when the queue
    // marker carries `priority`, stable-sort its do-prompts by the priority of
    // the matching backlog/icebox item so append-built or manual queues come out
    // prioritized. The backlog itself is priority-sorted earlier in the pipeline
    // by run_pending_maintenance, so the rank map read here is already current.
    // Also runs when the rank map is empty so a `__prioritized__` manual pin
    // (#queue-manual-priority-override) still floats to the top of the queue even
    // when no backlog item carries a `priority` attribute.
    if comp.attrs.contains_key("priority") || source_queue_priority {
        let rank =
            agent_doc_queue::backlog_sync::collect_backlog_priority_ranks(&components, &content);
        let mut operator_authored_identities: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        if let Ok(Some(snap_content)) = agent_doc_snapshot_io::load_document_baseline(file)
            && let Ok(snap_components) = agent_doc_element::element::parse(&snap_content)
            && let Ok(Some(snap_queue)) = agent_doc_queue::queue_set::component_for_id(
                &snap_content,
                &snap_components,
                &selected_queue_id,
            )
        {
            let snap_body = &snap_content[snap_queue.open_end..snap_queue.close_start];
            if let Ok(snap_entries) = agent_doc_queue::document_queue::parse(snap_body) {
                if let Some(pinned) =
                    agent_doc_queue::document_queue::annotate_operator_priority_reorders(
                        &snap_entries,
                        &entries,
                    )
                {
                    let new_body = agent_doc_queue::document_queue::render(&pinned);
                    current_content = {
                        let comps = agent_doc_element::element::parse(&current_content)?;
                        let q = agent_doc_queue::queue_set::component_for_id(
                            &current_content,
                            &comps,
                            &selected_queue_id,
                        )?
                        .context(
                            "queue maintenance: selected queue vanished during priority pin",
                        )?;
                        q.replace_content(&current_content, &new_body)
                    };
                    eprintln!(
                        "[preflight] queue: pinned manually reordered prompt(s) with operator priority"
                    );
                    entries = pinned;
                    mutated = true;
                }
                // #7r2s/#qauthorderpin: a brand-new queue line the operator just
                // typed (absent from the snapshot, not one the binary appended
                // from the backlog this cycle) carries no visible pin. Thread its
                // stable identity into the priority/DAG sort below so the authored
                // slot is held without injecting a `:pushpin:`.
                let synced_set: std::collections::HashSet<String> =
                    synced_queue_ids.iter().cloned().collect();
                operator_authored_identities =
                    agent_doc_queue::document_queue::operator_authored_prompt_identities(
                        &snap_entries,
                        &entries,
                        &synced_set,
                    );
                if !operator_authored_identities.is_empty() {
                    eprintln!(
                        "[preflight] queue: preserving {} manually-added prompt slot(s) by stable identity (#qauthorderpin)",
                        operator_authored_identities.len()
                    );
                }
            }
        }
        // `#backlog-queue-append-stable`: a `do [#id]` queue prompt whose id is an
        // active backlog id is backlog-sourced — the binary's `queue` attribute
        // appends it at the tail. The priority sort holds such prompts AFTER the
        // pre-existing unpinned manual / free-text prompts instead of floating them
        // up by backlog rank, so the default `queue` append stays appended even
        // under `priority` ("append, not prepend, even with non-annotated items in
        // the queue"). Operator/agent pins are exempt (a pin is an explicit position
        // signal — `#7r2s` already operator-pins genuinely operator-typed new lines).
        // Keyed off the active backlog id set (durable across cycles), not a
        // new-this-cycle diff, so a previously-synced item does not float up later.
        let backlog_sourced: std::collections::HashSet<String> = components
            .iter()
            .filter(|c| matches!(c.name.as_str(), "backlog" | "pending"))
            .flat_map(|c| {
                agent_doc_element_backlog::backlog::active_item_ids(
                    &content[c.open_end..c.close_start],
                )
            })
            .map(|id| id.to_ascii_lowercase())
            .collect();
        // Auto-dag (#queue-auto-dag-priority): order by `after=#id` dependency
        // graph first (a blocker outranks a pin); fall back to the plain
        // pin+priority sort when there are no dependency edges.
        let deps = agent_doc_queue::backlog_sync::collect_after_deps(&components, &content);
        let sorted = agent_doc_queue::document_queue::sort_prompts_by_dag_with_operator_authored(
            &entries,
            &rank,
            &deps,
            &backlog_sourced,
            &operator_authored_identities,
        )
        .map(|s| ("auto-dag dependency order (blockers + pins)", s))
        .or_else(|| {
            agent_doc_queue::document_queue::sort_prompts_by_priority_with_operator_authored(
                &entries,
                &rank,
                &backlog_sourced,
                &operator_authored_identities,
            )
            .map(|s| ("backlog priority (operator pins position-locked)", s))
        });
        if let Some((how, sorted)) = sorted {
            // `#pinoperatoronly`: do NOT annotate sort promotions with the agent
            // pin marker. Ordering is expressed by POSITION, which is already
            // deterministic and already visible; injecting `:round_pushpin:` on
            // every promoted head mutates operator-visible text to restate what
            // the position says, accumulates markers across passes, and makes an
            // agent-ordered head look pinned. A pin now means exactly one thing:
            // the operator pinned it. Matches `#qauthorder` — holding a slot must
            // not rewrite the line.
            let new_body = agent_doc_queue::document_queue::render(&sorted);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = agent_doc_queue::queue_set::component_for_id(
                    &current_content,
                    &comps,
                    &selected_queue_id,
                )?
                .context("queue maintenance: selected queue vanished during priority sort")?;
                q.replace_content(&current_content, &new_body)
            };
            eprintln!("[preflight] queue: sorted do-prompts by {how}");
            entries = sorted;
            mutated = true;
        }
    }

    // Read current state. A marker-side queue control (`start`/`go`/`stop`,
    // #queue-state-unify) is the marker spelling of the canonical `queue:`
    // frontmatter control: `start`/`go` are a fresh-activation gesture
    // equivalent to the legacy `auto` attribute (routed through the Auto trigger,
    // not the continuation-only Persisted path), and `stop` forces the queue
    // inactive this cycle. The control token is stripped from the tag below when
    // the queue drains, mirroring `auto`.
    let marker_control = agent_doc_queue::document_queue::marker_control(&comp.attrs);
    let marker_stop = matches!(
        marker_control,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Pause)
    );
    let has_immediate_activation = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff
        .map(agent_doc_diff::detect_queue_trigger)
        .unwrap_or(false);
    let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, frontmatter_queue_control(&fm));
    let persisted_activation =
        queue_control_activation(&comp.attrs, frontmatter_queue_control(&fm));

    let mut activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_immediate_activation,
        exchange_triggered,
        persisted_activation,
    );
    // A `stop` marker control forces the queue inactive this cycle regardless of
    // any other activation signal (#queue-state-unify), so the later
    // drain/clear path halts a running queue and strips the control token.
    if marker_stop && activation.active {
        activation = agent_doc_queue::document_queue::QueueActivation {
            entries_after: activation.entries_after,
            ..Default::default()
        };
    }
    let snapshot_was_active = snapshot_proves_queue_was_active(file);

    // Collapse duplicated queue nodes by durable AST node key, never by prompt
    // text. This keeps intentional repeated `do [#id]` prompts executable while
    // preserving a structural cleanup point for true duplicate node-key replay
    // residue from IPC/snapshot drift.
    // #queue-completed-items-escape-below-component: a post-commit CRDT/boundary
    // merge can displace struck queue items past `<!-- /agent:queue -->` into the
    // neighbouring parking-lot comment, where they render invisibly and
    // accumulate as orphaned residue. Drop any such displaced struck-queue line
    // (outside every agent component span) before the rest of queue maintenance.
    if let Some(repaired) =
        agent_doc_template::repair_queue_struck_items_escaped_below_marker(&current_content)
    {
        current_content = repaired;
        mutated = true;
        eprintln!(
            "[preflight] queue: removed displaced struck queue item(s) below the closing marker"
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_escape_repair file={} reason=struck_items_below_close_marker",
                file.display()
            ),
        );
    }

    if let Some((deduped_content, dropped)) =
        agent_doc_queue::queue_projection::dedup_queue_nodes_by_key(&current_content)?
    {
        current_content = deduped_content;
        let comps = agent_doc_element::element::parse(&current_content)?;
        if let Some(q) = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &comps,
            &selected_queue_id,
        )? {
            let body = &current_content[q.open_end..q.close_start];
            activation.entries_after = agent_doc_queue::document_queue::parse(body)
                .context("queue maintenance: failed to parse AST-deduped queue")?;
        }
        mutated = true;
        eprintln!("[preflight] queue: collapsed {dropped} duplicate queue node-key(s)");
    }

    // Unified, state-machine-driven queue convergence (#queuestatemachine2 /
    // #cgfx). This single pass replaces the former pile of independent dedup
    // normalizers (`dedup_bare_id_reference_heads` / #qdup-bare-id,
    // `dedup_pin_variant_do_heads` / #qdedupsync+#pushpinaccum,
    // `dedup_free_text_heads` / #qauthorder+#rt83qflood). It keys every
    // prompt-bearing entry by its durable head identity
    // (`agent_doc_element_queue::QueueItemIdentity`) and drives each identity's
    // per-item lifecycle SM to its lawful state: re-injecting an identity that
    // already has a lawful representative is a no-op transition, so a
    // stale-CRDT / supervisor re-emit cannot leave a visible duplicate —
    // duplication is structurally impossible rather than patched after the fact.
    // The historical passes survive only as transition guards inside
    // `converge_queue_via_lifecycle` (intentional-twin guard, pin-variant
    // collapse, snapshot-authored multiplicity) and as a thin migration shim
    // (the unit-tested individual functions remain `pub` so external callers and
    // their regression coverage do not break). Position-lock
    // (#queue-operator-pin-position-lock) is preserved: convergence never
    // reorders, and its only additive text projection is the stable
    // `⟦effect:N⟧` identity on lawful repeated free-text rows.
    let snapshot_text = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let snapshot_queue_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = snapshot_text
        .as_deref()
        .map(|snapshot| queue_entries_for_id(snapshot, &selected_queue_id))
        .unwrap_or_default();
    // `#unstrikelost`: the operator's own captured ops decide whether a live copy
    // of a snapshot-struck head is a re-arm (operator unstrike) or a stale
    // re-emit. Only the former may survive snapshot-struck dominance.
    let operator_rearmed = match snapshot_text.as_deref() {
        Some(snap) => operator_rearm_evidence_for_maintenance(
            file,
            snap,
            &snapshot_queue_entries,
            &activation.entries_after,
        )?,
        None => std::collections::HashSet::new(),
    };
    if let Some(converged_entries) =
        agent_doc_queue::document_queue::converge_queue_via_lifecycle_with_operator_rearmed(
            &activation.entries_after,
            &snapshot_queue_entries,
            &agent_doc_queue_io::queue_tombstone::current_tombstones(file),
            &operator_rearmed,
        )
    {
        let dropped = activation
            .entries_after
            .len()
            .saturating_sub(converged_entries.len());
        let new_body = agent_doc_queue::document_queue::render(&converged_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: queue component vanished before convergence")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = converged_entries;
        mutated = true;
        eprintln!(
            "[preflight] queue: converged lifecycle identities (dropped {dropped} replay head(s); annotated any repeated free-text effects) (#cgfx/#queueeffectidentity)"
        );
    }

    // `#queueoperatoraction`: the annotation is a projection of the same-id
    // operator todo, never durable authority by itself. An operator may check
    // the Markdown todo directly, so preflight also removes stale annotations
    // instead of relying only on the closeout command path.
    let open_todo_ids = agent_doc_element::todo::open_todo_ids(&current_content);
    if let Some(projected_entries) = agent_doc_queue::document_queue::set_prompt_operator_actions(
        &activation.entries_after,
        &[],
        &open_todo_ids,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&projected_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: selected queue vanished during operator-action sync")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = projected_entries;
        mutated = true;
        eprintln!("[preflight] queue: cleared resolved operator-action indicators");
    }

    // Consume start fence if needed
    if activation.consumed_start_fence {
        let new_body = agent_doc_queue::document_queue::render(&activation.entries_after);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: selected queue vanished after start fence")?;
            q.replace_content(&current_content, &new_body)
        };
        mutated = true;
        eprintln!("[preflight] queue: consumed start fence");
    }

    // Auto-strike queue head prompts whose `#id` is already in `agent:done`.
    //
    // Without this, the queue stays wedged on the first done item whenever
    // the cycle's diff does not literally match the queue head text — for
    // example after the user types new prompts into the exchange or after a
    // commit-mode finalize that reaped the backlog item via `--done` but
    // could not advance the queue because the prompt-text did not match
    // verbatim. The `should_consume_queue_prompt_for_diff_content` gate is
    // intentionally strict; this preflight-side maintenance pass is the
    // catch-up path that keeps the auto queue moving across already-resolved
    // items.
    //
    // Fixes the user-reported "queue gets stuck after 1 turn" symptom.
    // `project_root` / `done_ids` were computed once before the backlog→queue
    // sync (above) and reused here — `agent:done` is untouched by queue
    // maintenance, so the set is still current.
    let gated_ids = agent_doc_element_review::collect_gated_review_ids(&current_content);
    // `#qgateverdict`: a live queue head may be the operator action that resolves
    // a gated review item. In that shape the text after `[#id]:` is an explicit
    // verdict (for example "I lift the push hold"), so striking the head merely
    // because the referenced review row is still `[/]` destroys the only input
    // that can release the gate. Ordinary mirror heads remain non-drainable;
    // only the already-established operator-verdict syntax reactivates the id.
    let operator_answered_ids =
        agent_doc_queue::queue_continuation::operator_answered_head_ids(&current_content);
    let mut eligible_ids: std::collections::HashSet<String> = sync_done_ids.clone();
    for id in &gated_ids {
        if !operator_answered_ids.contains(&id.to_ascii_lowercase()) {
            eligible_ids.insert(id.clone());
        }
    }
    // GH #161: prompt preset names are reusable commands, not durable work-item
    // identities. An older `agent:done` row (including one in the done archive)
    // may legitimately have the same id, but must not complete a live preset
    // invocation. `head_id_is_registered_preset` preserves tracked-item
    // precedence when an id intentionally names both a preset and open work.
    eligible_ids.retain(|id| {
        !done_ids.contains(id)
            || !agent_doc_queue::queue_response::head_id_is_registered_preset_with_presets(
                &current_content,
                id,
                &effective_prompt_presets,
            )
    });
    // `#todosync`: a live `do [#id]` row whose id an operator-CHECKED Todo owns
    // is closed here, the way an operator-checked backlog item is reaped to
    // `agent:done` and its row struck by the `done` source above. Without it a
    // checked Todo kept a `📥 do [#id]` row live and dispatchable forever.
    // Conservative: the id must not be open or gated tracked work (a legacy
    // Todo that shares a backlog id is the operator prerequisite for that
    // work, not the work), must not be held open by another Todo row, must not
    // carry an operator verdict on the head (`do [#id]: ...`), and must not be
    // a registered preset command. The Todo itself is never touched.
    let todo_checked_ids: std::collections::HashSet<String> = {
        let active_tracked =
            agent_doc_queue::queue_continuation::active_tracked_ids(&current_content);
        agent_doc_element::todo::checked_todo_own_ids(&current_content)
            .into_iter()
            .filter(|id| {
                !active_tracked.contains(id)
                    && !gated_ids.iter().any(|gated| gated.eq_ignore_ascii_case(id))
                    && !operator_answered_ids.contains(id)
                    && agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(
                        &effective_prompt_presets,
                        id,
                    )
                    .is_none()
            })
            .filter(|id| !eligible_ids.contains(id))
            .collect()
    };
    eligible_ids.extend(todo_checked_ids.iter().cloned());
    // `activation.entries_after` already reflects start-fence consumption and
    // the duplicate-prompt collapse above, so it is the authoritative current
    // entry set for the strike pass in every branch.
    let entries_for_strike = activation.entries_after.clone();

    // `#fr79`: also strike DANGLING mirror-created heads — a head the backlog
    // mirror created whose id has since ceased to exist entirely (operator
    // deletion, rename, lost write). Such a head is undrainable forever:
    // nothing can resolve it, and it holds the drain position every cycle.
    //
    // Gated on provenance, which is the whole reason this is safe. Striking on
    // "no tracked item" alone previously deleted real work and failed 12
    // preflight tests, because a queue head is NOT required to have a backlog
    // item — operators author `do [#id]` heads directly. A head with no
    // recorded provenance is UNKNOWN, treated as operator-authored, and never
    // struck (`#qauthorder`).
    let mut drift_struck_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mirror_created_identities = agent_doc_project_root_io::project_root_containing(file)
        .and_then(|root| agent_doc_sqlite::state_store::open_state_db(&root).ok())
        .and_then(|conn| {
            agent_doc_sqlite::state_store::load_mirrored_queue_head_identities_from_db(
                &conn,
                &file.display().to_string(),
            )
            .ok()
        })
        .unwrap_or_default();
    if !mirror_created_identities.is_empty() {
        let active_tracked =
            agent_doc_queue::queue_continuation::active_tracked_ids(&current_content);
        for id in entries_for_strike
            .iter()
            .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        {
            if agent_doc_queue::queue_continuation::queue_head_is_strikable_drift(
                &id,
                &active_tracked,
                &done_ids,
                &gated_ids,
                &mirror_created_identities,
            ) {
                drift_struck_ids.insert(id.to_ascii_lowercase());
                eligible_ids.insert(id);
            }
        }
    }

    let mut eligible_id_list: Vec<String> = eligible_ids.iter().cloned().collect();
    eligible_id_list.sort();
    if !eligible_id_list.is_empty() {
        let (new_entries, struck) =
            agent_doc_queue::queue_consume::mark_entries_completed_by_done_ids(
                &entries_for_strike,
                &eligible_id_list,
            );
        if !struck.is_empty() {
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = agent_doc_queue::queue_set::component_for_id(
                    &current_content,
                    &comps,
                    &selected_queue_id,
                )?
                .context("queue maintenance: selected queue vanished before done-id strike")?;
                q.replace_content(&current_content, &new_body)
            };
            mutated = true;
            for prompt_text in &struck {
                let source =
                    match agent_doc_queue::queue_response::queue_prompt_done_id(prompt_text) {
                        Some(id) if done_ids.contains(&id) => "done",
                        Some(id) if gated_ids.contains(&id) => "review_gated",
                        Some(id) if todo_checked_ids.contains(&id) => "todo_checked",
                        // `#fr79`: mirror-created head whose backlog id vanished.
                        Some(id) if drift_struck_ids.contains(&id.to_ascii_lowercase()) => {
                            "orphaned_mirror_drift"
                        }
                        _ => "unknown",
                    };
                eprintln!(
                    "[preflight] queue: auto-struck already-resolved head prompt {:?} source={}",
                    prompt_text, source
                );
            }
            // Recompute activation against the rewritten entry list so subsequent
            // halt / step / dispatch maintenance phases see the post-strike head.
            activation.entries_after = new_entries;
            // If the strike consumed the entire live head set, the queue is now
            // drained residue — every queued `do [#id]` was resolved via
            // `agent:done` / review-gate. `resolve_activation` ran on the
            // pre-strike entries (live prompts present) so `active` is stale-true;
            // flip it false here so the drain-cleanup path below clears
            // `queue_active`, strips `auto`, and empties the body. Without this the
            // stale `active: true` either trips the `item_modified` halt (the
            // post-strike head is `None` vs a still-live snapshot head) or leaves
            // the queue reported active with an empty prompt set. (#drained-done-queue-clear)
            if agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty() {
                activation.active = false;
                activation.trigger = None;
            }
        }
    }

    // `#qheadresidue`: free-text catch-up strike. The per-cycle `#ftstrike`
    // (`strike_answered_free_text_queue_heads`) only matches THIS cycle's
    // `response_body`, so a free-text queue head answered by a PRIOR cycle — its
    // `> **Queue prompt:**` echo lives in committed `agent:exchange` but not in
    // the current response — was never struck. That left "completed residue"
    // that `check_free_text_queue_head_provenance` INTERRUPTs on every closeout
    // while backlog→queue convergence kept re-adding it, churning the go-queue
    // forever (live repro: the `🚧 JB Run Agent Doc` head).
    //
    // This strikes at the QUEUE-ENTRY level (`Prompt` → `Completed`), NOT via
    // `answered_free_text_head_node_keys`/`item_nodes`: the live churning heads
    // are BARE, multi-line operator-pasted blocks (no `- ` bullet, embedded
    // route-error code fences), which the markdown list-item parser does not
    // surface — only `agent_doc_queue::document_queue::parse` represents them (as a multiline
    // `Prompt`). The match reuses the SAME `free_text_head_answered_in_document`
    // predicate the session-check residue guard uses, so the preflight strike set
    // and the session-check INTERRUPT set agree — anything struck here is exactly
    // what would otherwise have INTERRUPTed closeout. A snapshot gate restricts
    // the strike to heads already present in the committed queue (mirroring
    // session-check's `committed_queue_contains_active_free_text_head`), so an
    // in-flight operator edit the convergence just added is never struck.
    let exchange_text =
        agent_doc_queue::queue_head_entry::exchange_component_text(&current_content);
    if !exchange_text.trim().is_empty() {
        // Normalized prose of every free-text head committed in the snapshot — the
        // in-flight-edit gate. `snapshot_queue_entries` was parsed above. The
        // identity is the one the `#qheadresiduescope` entry walk matches by.
        let gate_norm = agent_doc_queue::queue_head_entry::free_text_head_entry_identity;
        let committed_free_text: std::collections::HashSet<String> = snapshot_queue_entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(gate_norm(&p.text)),
                _ => None,
            })
            .collect();
        // `#claimstrike`: a worker claim does not exempt a head from this
        // catch-up. The strike set must equal the session-check residue set,
        // which ignores claims; exempting claimed heads left a completed,
        // claimed head that INTERRUPTed every closeout until the claim was
        // released by hand. An echo that only reported dispatch is a deferral
        // (`latest_free_text_head_echo_is_deferral`) and stays queued. The
        // struck heads' claims are released below.
        let claimed_heads = agent_doc_queue_io::queue_claim::claimed_live_head_texts_for_content(
            file,
            &current_content,
        );
        let claimed =
            agent_doc_queue::queue_claim::ClaimedQueueItems::none().with_heads(&claimed_heads);
        // `#subagentinlineveto`: a head the queue's `subagents` attribute owns
        // for dispatch, with no worker attempt behind it, is dispatch work even
        // when an earlier response answered it inline. Never catch it up here;
        // the cycle contract lists it under `queue_subagent_dispatch` again.
        let inline_vetoed = agent_doc_queue::queue_claim::ClaimedQueueItems::none().with_heads(
            &agent_doc_queue_io::subagent_dispatch::inline_answer_vetoed_heads_for_content(
                file,
                &current_content,
            ),
        );
        // `#ftstrikeclaimedmention`: a claimed head is residue only on an exact
        // `> **Queue prompt:**` echo, the same proof session-check and the
        // finalize strike require of it. `#deferstrike`: an echo followed by a
        // deferral keeps the head queued. `#nestvector`: a nested leaf is also
        // answered by its nest-vector echo, and that echo does not answer an
        // ancestor label.
        let answered_residue_in = |exchange: &str, head: &str| -> bool {
            (if claimed.claims(head) {
                agent_doc_queue::queue_response::free_text_head_has_explicit_answer_evidence(
                    exchange, head,
                ) || agent_doc_queue::queue_response::response_explicitly_echoes_nest_vector(
                    &current_content,
                    exchange,
                    head,
                )
            } else {
                agent_doc_queue::queue_response::free_text_head_answered_in_document(
                    &current_content,
                    exchange,
                    head,
                )
            }) && !agent_doc_queue::queue_response::latest_queue_head_echo_is_deferral(
                &current_content,
                exchange,
                head,
            )
        };
        let mut predating_echo_heads = 0usize;
        // GH #327 pass 4: a `loop` queue's head is a
        // standing program the pass restores, so the committed exchange always
        // holds earlier runs' echoes, and a sibling queue may hold the same
        // text. Only an echo written after the current pass opened, and not
        // about another queue's copy of the head, answers it. With no pass
        // open (or no readable schedule state) the head is held and nothing
        // in this pass can have answered it.
        // The `#syntheticstrike` continuation-head skip resolves the same
        // window and scope, so the two never disagree on a head.
        let echo_window = agent_doc_queue_io::queue_schedule::head_echo_window(
            file,
            &current_content,
            &selected_queue_id,
        );
        let mut loop_scope_skips: Vec<&'static str> = Vec::new();
        let mut struck_claimed_heads: Vec<String> = Vec::new();
        let mut struck_count = 0usize;
        // `#answeredrequeststrike`: the open-request hold matches the `🙋` projection.
        let open_todo_ids = agent_doc_element::todo::open_todo_ids(&current_content);
        let new_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = activation
            .entries_after
            .iter()
            .map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p)
                    // `#qimpstrike`: a recurring-imperative command head (`deploy`,
                    // `commit`, `push`, ...) is a standing executable directive that
                    // stays valid every cycle. A prior `> **Queue prompt:**` echo does
                    // NOT retire it, so it must be exempt here exactly as the
                    // session-check residue guard
                    // (`free_text_queue_head_is_completed_residue`) exempts it —
                    // otherwise the preflight strike set and the session-check
                    // INTERRUPT set disagree and a fresh `deploy` request gets struck
                    // with no action taken.
                            if !agent_doc_queue::queue_continuation::is_recurring_imperative_head(&p.text)
                                && !inline_vetoed.claims(&p.text)
                                // `#operatoractionresidue`: a reported worker can
                                // leave an exact queue echo while its linked
                                // operator request is still unanswered. That
                                // `🙋` gate outranks historical answer evidence.
                                && !agent_doc_document::queue_projection::operator_action_holds_row(&p.text, &open_todo_ids)
                                && queue_prompt_text_is_free_text(&current_content, &p.text)
                                && committed_free_text.contains(&gate_norm(&p.text))
                                && answered_residue_in(&exchange_text, &p.text)
                                // `#qheadresiduescope`: only an echo written
                                // after the head (re-)entered the queue
                                // answers it. An echo of an earlier, already
                                // answered copy of the same text is history.
                                // GH #327 pass 4: a `loop` head counts only
                                // its own pass's echo, and none while no pass
                                // is open.
                                && {
                                    let scope = agent_doc_git_io::revision::head_echo_scope(
                                        file,
                                        &selected_queue_id,
                                        &p.text,
                                        echo_window,
                                        &current_content,
                                    );
                                    let answered_in_scope = scope.answers(&exchange_text, |scoped| {
                                        answered_residue_in(scoped, &p.text)
                                    });
                                    if !answered_in_scope {
                                        if echo_window.is_loop() {
                                            loop_scope_skips.push(scope.label());
                                        } else {
                                            predating_echo_heads += 1;
                                        }
                                    }
                                    answered_in_scope
                                } =>
                {
                    struck_count += 1;
                    if claimed.claims(&p.text) {
                        struck_claimed_heads.push(p.text.clone());
                    }
                    // #qftstuck: a struck head is no longer in progress — drop the
                    // cosmetic `🚧` marker so it does not linger inside the
                    // strikethrough (`set_first_prompt_in_progress` re-applies it to
                    // the genuinely-active next head).
                    let cleaned = strip_in_progress_marker(&p.text);
                    if cleaned == p.text {
                        agent_doc_queue::document_queue::QueueEntry::Completed(p.clone())
                    } else {
                        agent_doc_queue::document_queue::QueueEntry::Completed(
                            agent_doc_queue::document_queue::QueuePrompt {
                                text: cleaned,
                                multiline: p.multiline,
                                indent: p.indent,
                                ordered_marker: p.ordered_marker.clone(),
                            },
                        )
                    }
                }
                other => other.clone(),
            })
            .collect();
        if predating_echo_heads > 0 {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_freetext_residue_strike_skipped_echo_predates_head_entry file={} heads={predating_echo_heads} (#qheadresiduescope)",
                    file.display(),
                ),
            );
        }
        if !loop_scope_skips.is_empty() {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_freetext_residue_strike_skipped_loop_echo_outside_pass file={} queue_id={} heads={} anchors={} (GH #327)",
                    file.display(),
                    selected_queue_id,
                    loop_scope_skips.len(),
                    loop_scope_skips.join(","),
                ),
            );
        }
        if struck_count > 0 {
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = agent_doc_queue::queue_set::component_for_id(
                    &current_content,
                    &comps,
                    &selected_queue_id,
                )?
                .context(
                    "queue maintenance: queue component vanished before free-text residue strike",
                )?;
                q.replace_content(&current_content, &new_body)
            };
            activation.entries_after = new_entries;
            mutated = true;
            eprintln!(
                "[preflight] queue: auto-struck {struck_count} answered free-text head(s) by committed exchange match (#qheadresidue)"
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_freetext_residue_strike file={} struck={struck_count}",
                    file.display(),
                ),
            );
            // `#claimstrike`: the answered head's claim ends with its strike.
            // Never fatal: a claim on a closed head is inert, and closeout
            // reconciliation prunes it.
            for head in &struck_claimed_heads {
                if let Err(err) = agent_doc_queue_io::queue_claim::release_closed_head(file, head) {
                    eprintln!(
                        "[preflight] WARNING: could not release the claim on a struck free-text head in {}: {err:#}",
                        file.display()
                    );
                }
            }
            // If the strike emptied the live head set, mirror the id-backed
            // done-strike drain-clear so the queue does not report active with an
            // empty prompt set (#drained-done-queue-clear).
            if agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty() {
                activation.active = false;
                activation.trigger = None;
            }
        }
    }

    // `#qftbklgstrike`: convergence auto-strike of a LIVE free-text queue head
    // when it is already complete (a matching `agent:done` item exists) OR a
    // backlog item already addresses it (a matching active `agent:backlog` item
    // exists). This complements the `#qheadresidue` strike above (which needs a
    // committed `agent:exchange` ANSWER): here the head may never have been
    // answered, but the deterministic semantic scorer
    // (`semantic_queue_strike_matches`, the strike sibling of the existing
    // `semantic_completion_match` warning) proves the work is captured elsewhere,
    // so the operator prompt is redundant and lingering only churns the queue.
    //
    // SAFETY: only `QueueEntry::Prompt` heads that are free-text (no `#id` — id
    // heads have their own done-strike) are eligible, the match must clear the
    // conservative `QUEUE_STRIKE_THRESHOLD`, a backlog match must be the head's
    // exact lineage (`#freetextqueue`: the item's own text IS the head's text —
    // a backlog item that merely quotes the head never counts), and a
    // committed-snapshot gate (mirroring the `#qheadresidue` gate)
    // restricts the strike to heads already present in the committed queue so an
    // in-flight operator edit convergence just added is never struck. The strike
    // is annotation-only: the head is converted to a `Completed` entry whose text
    // names the matched id + reason, never deleted — preserving the operator's
    // prompt verbatim inside the strikethrough for auditability.
    {
        let gate_norm = |text: &str| -> String {
            strip_priority_markers(text)
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let committed_free_text: std::collections::HashSet<String> = snapshot_queue_entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(gate_norm(&p.text)),
                _ => None,
            })
            .collect();
        match agent_doc_memory_io::session::semantic_queue_strike_matches(
            file,
            None,
            agent_doc_memory::QUEUE_STRIKE_THRESHOLD,
            16,
        ) {
            Ok(strike_matches) if !strike_matches.is_empty() => {
                // Match by normalized head text rather than parse index: earlier
                // maintenance phases may have mutated `entries_after` so its
                // indices need not align with the on-disk parse order
                // `semantic_queue_strike_matches` scored. Normalized text is the
                // stable free-text head identity (same key the `#qauthorder`
                // dedup uses).
                let mut by_norm: std::collections::HashMap<
                    String,
                    agent_doc_memory::QueueStrikeMatch,
                > = std::collections::HashMap::new();
                for m in strike_matches {
                    by_norm.entry(gate_norm(&m.candidate_text)).or_insert(m);
                }
                let mut struck: Vec<(agent_doc_memory::QueueStrikeMatch, String)> = Vec::new();
                let new_entries: Vec<agent_doc_queue::document_queue::QueueEntry> = activation
                    .entries_after
                    .iter()
                    .map(|entry| match entry {
                        agent_doc_queue::document_queue::QueueEntry::Prompt(p)
                            // `#qimpstrike`: a recurring-imperative command head is a
                            // standing directive — a semantic match against a tracked
                            // backlog/done item must never retire it, or a fresh
                            // `deploy` request gets struck with no action taken.
                            if !agent_doc_queue::queue_continuation::is_recurring_imperative_head(&p.text)
                                && queue_prompt_text_is_free_text(&current_content, &p.text)
                                && committed_free_text.contains(&gate_norm(&p.text)) =>
                        {
                            match by_norm.get(&gate_norm(&p.text)) {
                                Some(m) => {
                                    let id = m.matched_id.as_deref().unwrap_or("?");
                                    let reason = match m.matched_kind {
                                        agent_doc_memory::QueueStrikeMatchKind::Done => {
                                            format!("auto-struck: completed by #{id} (#qftbklgstrike)")
                                        }
                                        agent_doc_memory::QueueStrikeMatchKind::Backlog => {
                                            format!(
                                                "auto-struck: tracked by backlog #{id} (#qftbklgstrike)"
                                            )
                                        }
                                    };
                                    // Bake the reason INSIDE the strikethrough so the
                                    // rendered `- ~~<original> — <reason>~~` round-trips
                                    // through `parse_completed_inline` as a stable
                                    // `Completed` entry (a trailing suffix outside the
                                    // `~~` would re-parse as a live Prompt and churn).
                                    // #qftstuck: drop the cosmetic `🚧` in-progress
                                    // marker before baking — a struck head is no longer
                                    // in progress, so the marker must not linger inside
                                    // the strikethrough (and `set_first_prompt_in_progress`
                                    // re-applies it to the genuinely-active next head).
                                    let head_clean = strip_in_progress_marker(p.text.trim_end());
                                    let annotated = format!("{head_clean} — {reason}");
                                    struck.push((m.clone(), annotated.clone()));
                                    agent_doc_queue::document_queue::QueueEntry::Completed(agent_doc_queue::document_queue::QueuePrompt {
                                        text: annotated,
                                        multiline: p.multiline,
                                        indent: p.indent,
                                        ordered_marker: p.ordered_marker.clone(),
                                    })
                                }
                                None => entry.clone(),
                            }
                        }
                        _ => entry.clone(),
                    })
                    .collect();
                if !struck.is_empty() {
                    let new_body = agent_doc_queue::document_queue::render(&new_entries);
                    current_content = {
                        let comps = agent_doc_element::element::parse(&current_content)?;
                        let q = agent_doc_queue::queue_set::component_for_id(
                            &current_content,
                            &comps,
                            &selected_queue_id,
                        )?
                        .context(
                            "queue maintenance: queue component vanished before backlog/done strike",
                        )?;
                        q.replace_content(&current_content, &new_body)
                    };
                    activation.entries_after = new_entries;
                    mutated = true;
                    for (m, _annotated) in &struck {
                        let kind = match m.matched_kind {
                            agent_doc_memory::QueueStrikeMatchKind::Done => "done",
                            agent_doc_memory::QueueStrikeMatchKind::Backlog => "backlog",
                        };
                        let display: String = m.candidate_text.chars().take(120).collect();
                        eprintln!(
                            "[preflight] queue: auto-struck free-text head matched={kind} #{} ({:.3}) (#qftbklgstrike): {:?}",
                            m.matched_id.as_deref().unwrap_or("?"),
                            m.score,
                            display,
                        );
                    }
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "preflight_freetext_backlog_done_strike file={} struck={}",
                            file.display(),
                            struck.len(),
                        ),
                    );
                    if agent_doc_queue::document_queue::prompts(&activation.entries_after)
                        .is_empty()
                    {
                        activation.active = false;
                        activation.trigger = None;
                    }
                }
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!(
                    "[preflight] queue: backlog/done strike retrieval unavailable (#qftbklgstrike): {err}"
                );
            }
        }
    }

    // `#strikeretires`: every strike above (`#qdonestrike`, the `#qheadresidue`
    // catch-up, `#qftbklgstrike`) closes finished work. Retire the struck rows'
    // binary completion cells in this same maintenance successor, before a
    // drain clear can remove the struck rows that identify them; otherwise a
    // cell keyed only by work text outlives its row and projects `🔄` onto a
    // later row with the same text. The snapshot drops the same cells, and
    // the reported/terminal claim attempts and reported-release receipts end
    // once the successor persists (`retire_struck_or_warn`).
    let struck_retirement = agent_doc_queue::queue_consume::retire_struck_rows_completion_feedback(
        &authority_baseline,
        &current_content,
    );
    if struck_retirement.content != current_content {
        current_content = struck_retirement.content;
        mutated = true;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "preflight_struck_completion_cells_retired file={} heads={} cells={} (#strikeretires)",
                file.display(),
                struck_retirement.struck_heads.len(),
                struck_retirement.retired_keys.len(),
            ),
        );
    }
    let struck_heads_this_pass = struck_retirement.struck_heads;

    // Phase 3: halt detection — stop fences and item modification
    if activation.active {
        // Stop fence at head → halt the queue
        if agent_doc_queue::document_queue::has_stop_fence_at_head(&activation.entries_after) {
            eprintln!("[preflight] queue: halt — stop fence at head");
            // Consume the stop fence
            let after_stop: Vec<agent_doc_queue::document_queue::QueueEntry> =
                activation.entries_after[1..].to_vec();
            let new_body = agent_doc_queue::document_queue::render(&after_stop);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = agent_doc_queue::queue_set::component_for_id(
                    &current_content,
                    &comps,
                    &selected_queue_id,
                )?
                .context("queue maintenance: selected queue vanished during stop-fence handling")?;
                q.replace_content(&current_content, &new_body)
            };
            // Strip ephemeral activation controls and HOLD the queue: the
            // operator's fence asks the drain to wait here, and a cleared
            // control would just resume it (`#queuestopremove`).
            current_content = strip_queue_activation_tokens_in_content(&current_content)?;
            current_content = frontmatter::merge_queue_hold(&current_content)?;
            // Persist to file + snapshot (skip the raw disk write behind a live
            // editor; #fccqueue routes the queue shape through IPC convergence).
            current_content = persist_queue_maintenance_doc(
                file,
                &current_content,
                &authority_baseline,
                project_root.as_deref(),
                "queue_halt",
            )?;
            agent_doc_queue_io::claim_release_receipt::retire_struck_or_warn(
                file,
                &struck_heads_this_pass,
                &current_content,
                "queue_halt",
            );
            if let Ok(Some(snap)) = agent_doc_snapshot_io::load_document_baseline(file) {
                let mut new_snap =
                    agent_doc_queue::queue_consume::retire_consumed_completion_feedback(
                        &snap,
                        &struck_heads_this_pass,
                    )
                    .0;
                if let Ok(sc) = agent_doc_element::element::parse(&new_snap)
                    && let Ok(Some(sq)) = agent_doc_queue::queue_set::component_for_id(
                        &new_snap,
                        &sc,
                        &selected_queue_id,
                    )
                {
                    new_snap = sq.replace_content(&new_snap, &new_body);
                    new_snap = strip_queue_activation_tokens_in_content(&new_snap)?;
                    if let Ok(m) = frontmatter::merge_queue_hold(&new_snap) {
                        new_snap = m;
                    }
                    if new_snap != snap
                        && let Err(e) = agent_doc_snapshot_io::checkpoint_document_baseline(
                            file,
                            &new_snap,
                            agent_doc_ops_log_io::log_op,
                        )
                    {
                        eprintln!("[preflight] queue halt: snapshot sync warning: {}", e);
                    }
                }
            }
            record_queue_worklist_state(file, &current_content, &after_stop, false)?;
            if let Some(head) = agent_doc_queue::document_queue::first_prompt(&after_stop) {
                let head_text = strip_in_progress_marker(&head.text);
                record_deferred_queue_head_state(file, &current_content, &head_text, "stop_fence")?;
            }
            return Ok(QueueState {
                authoritative_content: Some(current_content),
                queue_prompts: vec![],
                selected_queue_prompts: vec![],
                queue_active: Some(false),
                queue_deferred: false,
                queue_start_at: None,
                queue_trigger: activation.trigger,
                queue_halted: Some("stop_fence".into()),
                queue_paused: false,
                queue_pause_reason: None,
                queue_drainable_head_count: 0,
                queue_continuation_required: false,
                queue_supervisor_drainable: false,
                queue_supervisor_continuation_head: None,
                // `#qmaintorphan`: read-only / pre-persist path — no plane split to commit.
                maintenance_mutated: false,
                claimed_queue_heads: vec![],
                synced_queue_ids,
                free_text_promotions: free_text_promotions.clone(),
                warnings: Vec::new(),
            });
        }

        // Time gate at head → defer if not yet time
        if let Some(dt) =
            agent_doc_queue::document_queue::time_gate_at_head(&activation.entries_after)
        {
            eprintln!("[preflight] queue: deferred — time gate at head: {}", dt);
            record_queue_worklist_state(file, &current_content, &activation.entries_after, false)?;
            if let Some(head) =
                agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
            {
                let head_text = strip_in_progress_marker(&head.text);
                let reason = format!("time_gate:{dt}");
                record_deferred_queue_head_state(file, &current_content, &head_text, &reason)?;
            }
            return Ok(QueueState {
                authoritative_content: Some(current_content),
                queue_prompts: vec![],
                selected_queue_prompts: vec![],
                queue_active: None,
                queue_deferred: true,
                queue_start_at: Some(dt.to_string()),
                queue_trigger: activation.trigger,
                queue_halted: None,
                queue_paused: false,
                queue_pause_reason: None,
                queue_drainable_head_count: 0,
                queue_continuation_required: false,
                queue_supervisor_drainable: false,
                queue_supervisor_continuation_head: None,
                // `#qmaintorphan`: read-only / pre-persist path — no plane split to commit.
                maintenance_mutated: false,
                claimed_queue_heads: vec![],
                synced_queue_ids,
                free_text_promotions: free_text_promotions.clone(),
                warnings: Vec::new(),
            });
        }

        // Change detection: compare head prompt between snapshot and file, but
        // only for a queue that was already active. A newly auto/start/request
        // activated queue is operator-authored input for this cycle, not an
        // in-flight queue item edit.
        if snapshot_was_active
            && let Ok(Some(snap_content)) = agent_doc_snapshot_io::load_document_baseline(file)
            && let Ok(snap_comps) = agent_doc_element::element::parse(&snap_content)
            && let Ok(Some(snap_q)) = agent_doc_queue::queue_set::component_for_id(
                &snap_content,
                &snap_comps,
                &selected_queue_id,
            )
        {
            let snap_body = &snap_content[snap_q.open_end..snap_q.close_start];
            if let Ok(snap_entries) = agent_doc_queue::document_queue::parse(snap_body)
                && {
                    // Apply the same done/gated strike to the snapshot's
                    // entries before comparing heads. A cycle that resolved a
                    // leading queue head via `--done` (so the strike pass above
                    // converted it to `Completed`) otherwise reads as a
                    // head-text change vs the still-live snapshot head and
                    // false-halts as `item_modified`, wedging the remaining
                    // live head behind drained residue. Striking both sides
                    // leaves only genuine operator head edits visible.
                    // (#drained-done-queue-clear)
                    let snap_entries_struck = if eligible_id_list.is_empty() {
                        snap_entries
                    } else {
                        let (entries, struck) =
                            agent_doc_queue::queue_consume::mark_entries_completed_by_done_ids(
                                &snap_entries,
                                &eligible_id_list,
                            );
                        if struck.is_empty() {
                            snap_entries
                        } else {
                            entries
                        }
                    };
                    agent_doc_queue::document_queue::detect_head_prompt_modified(
                        &snap_entries_struck,
                        &activation.entries_after,
                    )
                }
            {
                // Lazily current is the coherent editor cut. Adopt the edited
                // head directly; the eventual mutation is still protected by
                // expected-current CAS, so a later keystroke rebases instead of
                // resurrecting a stale queue snapshot.
                eprintln!(
                    "[preflight] queue: head prompt modified in Lazily current — adopting edited head and continuing loop (#queue-no-stall-on-head-edit)"
                );
                adopt_edited_queue_head_into_snapshot(file, &current_content);
            }
        }
    }

    // Handle queue drain: if the queue has no remaining prompts, clear
    // queue_active, strip auto, and remove completed/directive residue.
    let queue_has_prompts =
        !agent_doc_queue::document_queue::prompts(&activation.entries_after).is_empty();
    let drained_residue = queue_entries_are_drained_residue(&activation.entries_after);
    let need_sync_newly_activated_queue_snapshot = activation.active && !snapshot_was_active;
    let need_set_active = activation.active && !persisted_active;
    // `#f4d5`: never revoke an activation the operator set THIS cycle.
    //
    // Reported live from a second session on `brookebrodack-dev.md`: the
    // operator flips frontmatter to `queue: start`, preflight consumes that
    // start fence, then reads a queue body a PREVIOUS cycle failed to persist
    // (`queue_maintenance_not_persisted`), infers "drained" from that stale
    // emptiness, and strips the activation straight back to `queue: stop`. The
    // operator's explicit request is erased by state they never saw.
    //
    // Deactivating on known-stale data is the unsafe direction, and an
    // activation consumed this cycle is exactly when the body is least
    // trustworthy. Holding it costs one cycle: if the queue really is empty,
    // the next pass has no fresh fence and clears it normally.
    let need_clear_active = !activation.active
        && persisted_active
        && !activation.deferred
        && !activation.consumed_start_fence;
    // The marker `go`/`start` gestures enter the same immediate-activation path
    // as legacy `auto`. Name the combined predicate explicitly: on a drain it
    // must strip every activation spelling, not just the literal `auto` token
    // (GH #292).
    let need_strip_activation = has_immediate_activation && !queue_has_prompts;
    let need_clear_non_auto_residue =
        !has_immediate_activation && !activation.active && !activation.deferred && drained_residue;
    // `#queuelineclobber`: a drain clear blanks the ENTIRE queue body, and
    // neither predicate above can see operator text. `need_strip_activation`
    // asks only whether an actionable `do` prompt remains, and
    // `drained_residue` classifies by entry kind — a `Freeform` line (where the
    // parser puts an operator's `re [#id]: <question>` reference, or any prose
    // they typed into the queue)
    // satisfies "no prompts" while carrying words nothing else holds.
    //
    // Reported live on `src/haiven-dev/tasks/infra.md`: the operator wrote
    // `- re [#fpeidentitysecondclient]: Since we moved over to a Secret Bearer
    // Token…` into the queue and flipped `queue: go`; preflight found no
    // drainable prompt, called the queue drained, blanked the body, and set
    // `queue: stop` — their question was gone and the activation they had just
    // asked for was revoked. Reproduced deterministically from a seeded document.
    //
    // Gate the whole decision, not just the body write: if the clear would be
    // lossy the queue is not drained, so the auto-strip and the
    // `queue: stop` revocation it drives are wrong for the same reason. The body
    // then surfaces through the `inactive_queue_residue` warning below, which is
    // what an unanswered operator queue line should do.
    let drained_body_clearable = (need_strip_activation || need_clear_non_auto_residue)
        && !activation.deferred
        && queue_body_clear_is_lossless(&activation.entries_after);
    // GH #327: a `loop` queue keeps its struck rows when it drains. They are
    // the scheduler's evidence that the pass ran each item, and the loop
    // restore removes them when it re-appends the items. Blanking the body
    // here (in the same write as the residue strike) left the item neither
    // live nor struck, so the pass completed with `restore_items=0` and the
    // loop lost its program.
    let selected_queue_retains_struck = agent_doc_element::element::parse(&current_content)
        .ok()
        .and_then(|comps| {
            agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )
            .ok()
            .flatten()
            .map(|q| agent_doc_queue::queue_schedule::queue_retains_struck_heads(&q.attrs))
        })
        .unwrap_or(false);
    let need_clear_drained_body = drained_body_clearable && !selected_queue_retains_struck;
    // `#queuestopretire`: a DRAIN — no live prompt left, not a `stop`-marker
    // halt, nothing held back from the backlog — clears the control on both
    // surfaces instead of writing `queue: stop` (the marker token is stripped
    // below). A HALT with heads left keeps the `stop` hold so the default `go`
    // does not re-dispatch the head the turn just gave up on.
    let drain_clears_control =
        !queue_has_prompts && !marker_stop && backlog_ids_held_out_of_queue == 0;
    // Couple both surfaces to the same settled drain decision. The immediate
    // activation predicate normally covers marker `go`/`start`, but the drain
    // invariant must not depend on which activation path populated the cached
    // state: if frontmatter is cleared, marker control is stripped too.
    let strip_queue_marker_control =
        need_strip_activation || marker_stop || (need_clear_active && drain_clears_control);

    if need_clear_drained_body {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &comps,
            &selected_queue_id,
        )?
        .context("queue maintenance: selected queue vanished before drain clear")?;
        if !q.content(&current_content).trim().is_empty() {
            current_content = q.replace_content(&current_content, "");
            mutated = true;
            eprintln!("[preflight] queue: cleared drained queue body");
        }
    }

    if !activation.active
        && !activation.deferred
        && !activation.entries_after.is_empty()
        && !drained_body_clearable
    {
        // `inactive_queue_residue` is a per-*edit* signal, not a per-preflight
        // nag. It is useful when the operator just added/changed content in an
        // inactive queue (so a `do [#id]` they expected to run silently will
        // not). It is pure noise when the inactive queue is unchanged from the
        // committed snapshot — exactly the steady state an `item_modified` halt
        // leaves behind, where re-warning on every preflight with no user edit
        // drives the #adoc-queue-ipc-drift loop. Only warn when the inactive
        // queue body actually changed since the snapshot this cycle.
        let inactive_queue_changed = match agent_doc_snapshot_io::load_document_baseline(file) {
            Ok(Some(snapshot_content)) => {
                inactive_queue_changed_vs_snapshot(&snapshot_content, &activation.entries_after)
            }
            _ => true,
        };
        if inactive_queue_changed {
            queue_warnings.push(PreflightWarning {
                code: "inactive_queue_residue".to_string(),
                message: "agent:queue is inactive but still contains directive/item residue; only active queue state is executable priority context".to_string(),
                document_agent: None,
                active_harness: None,
            });
        } else {
            eprintln!(
                "[preflight] queue: inactive with retained entries unchanged from snapshot — stable, not re-flagged as residue"
            );
        }
    }

    // Strip auto attribute from opening tag when queue drains
    // Strip the activation token from the opening tag when the queue drains
    // (`auto`/`go`/`start`) or when a `stop` marker halts it (#queue-state-unify).
    // The token is the ephemeral activation gesture; once consumed it must not
    // re-trigger on the next cycle.
    if strip_queue_marker_control {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = agent_doc_queue::queue_set::component_for_id(
            &current_content,
            &comps,
            &selected_queue_id,
        )?
        .context("queue maintenance: selected queue vanished before control strip")?;
        let raw_tag = &current_content[q.open_start..q.open_end];
        let new_tag = agent_doc_queue::document_queue::strip_control_from_tag(
            &agent_doc_queue::document_queue::strip_auto_from_tag(raw_tag),
        );
        if new_tag != raw_tag {
            let mut rebuilt = String::with_capacity(current_content.len());
            rebuilt.push_str(&current_content[..q.open_start]);
            rebuilt.push_str(&new_tag);
            rebuilt.push_str(&current_content[q.open_end..]);
            current_content = rebuilt;
            mutated = true;
            eprintln!(
                "[preflight] queue: stripped activation token ({})",
                if marker_stop { "stop" } else { "drained" }
            );
        }
    }

    // Persist canonical queue activation state to frontmatter (#queue-state-unify
    // phase 4: emit `queue: start`/`queue: stop`, migrating off `queue_active:`).
    if need_set_active {
        current_content = frontmatter::merge_queue_state(&current_content, true)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: start");
    } else if need_clear_active && drain_clears_control {
        current_content = frontmatter::clear_queue_control(&current_content)?;
        mutated = true;
        eprintln!("[preflight] queue: drained — cleared queue control (#queuestopretire)");
    } else if need_clear_active && marker_stop {
        // `#queuestopremove`: a `pause` (or legacy `stop`) marker gesture is the
        // operator's hold; persist it as the one visible hold.
        current_content = frontmatter::merge_queue_hold(&current_content)?;
        mutated = true;
        eprintln!("[preflight] queue: held — set queue: pause");
    } else if need_clear_active {
        current_content = frontmatter::merge_queue_state(&current_content, false)?;
        mutated = true;
        eprintln!("[preflight] queue: halted — cleared queue control (#queuestopremove)");
    }

    // `#queueskip`: recompute the skipped-head set for this cycle. A head that was
    // dispatched last cycle and came back unconsumed (its `#id` never entered the
    // prior cycle's resolved/gated set) yet is still a live head is confirmed
    // stalled — skip it so the queue advances to a non-dependent drainable head
    // instead of re-dispatching a dead ref. Carried skips persist until their id
    // is consumed or no longer a live head. Computed from the prior cycle's
    // state-ledger projection before `start_preflight` advances this run.
    // `#claimskipmarker` / `#queueawaitop`: ids a worker holds a live claim on,
    // and ids whose agent work is finished and which wait on the operator. A
    // claimed head is in flight (`🚧`), an awaiting head was never dispatched;
    // neither may be blamed as "dispatched and came back unconsumed", so both
    // are exempt from the skip set. Before this, a claimed subagent head was the
    // `🚧` row `prior_dispatched_queue_id` blamed, so the next cycle stamped
    // `⏭️` over the claim's `🚧`, and the first `[operator-verify]` head was
    // blamed by the fallback (observed 2026-10-11 on haiven-dev `tasks/sdk.md`:
    // claimed `#cligenport` and deferred `#quirkbigint` both showed `⏭️`).
    let active_claim_generations: std::collections::HashMap<String, u64> = if activation.active {
        agent_doc_queue_io::queue_claim::active_claims_for_content(file, &current_content)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|claim| {
                agent_doc_queue::queue_continuation::extract_head_id(&claim.item_text)
                    .map(|id| (id.to_ascii_lowercase(), claim.generation))
            })
            .collect()
    } else {
        std::collections::HashMap::new()
    };
    let claimed_head_ids: std::collections::HashSet<String> = if activation.active {
        agent_doc_queue_io::queue_claim::claimed_live_head_texts_for_content(file, &current_content)
            .iter()
            .filter_map(|text| agent_doc_queue::queue_continuation::extract_head_id(text))
            .map(|id| id.to_ascii_lowercase())
            .collect()
    } else {
        std::collections::HashSet::new()
    };
    let awaiting_operator_all: std::collections::HashSet<String> = if activation.active {
        agent_doc_queue::queue_continuation::awaiting_operator_head_ids(&current_content)
    } else {
        std::collections::HashSet::new()
    };
    let awaiting_operator_ids: std::collections::HashSet<String> = awaiting_operator_all
        .difference(&claimed_head_ids)
        .cloned()
        .collect();
    // `#queueblocked`: a blocked head was never dispatched (no drain scope may
    // select it), so it can never have "come back unconsumed". Exempting it
    // keeps `⏭️` off it and lets the stall evidence fall on the head the
    // cycle could actually have run.
    let blocked_head_ids: std::collections::HashSet<String> = if activation.active {
        agent_doc_queue::queue_blocked::queue_blockers(&current_content).deferred_ids()
    } else {
        std::collections::HashSet::new()
    };
    let skip_exempt_ids: std::collections::HashSet<String> = claimed_head_ids
        .union(&awaiting_operator_ids)
        .cloned()
        .chain(blocked_head_ids.iter().cloned())
        .collect();
    let mut claims_held_after_report = awaiting_operator_all
        .intersection(&claimed_head_ids)
        .cloned()
        .collect::<Vec<_>>();
    claims_held_after_report.sort();
    if !claims_held_after_report.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "preflight_queue_claim_held_after_report file={} ids={} (#queueawaitop)",
                file.display(),
                claims_held_after_report.join(","),
            ),
        );
        queue_warnings.push(PreflightWarning {
            code: "queue_claim_held_after_report".to_string(),
            message: format!(
                "claimed queue head(s) {} now wait on the operator ([operator-verify] or a gated review row) but the worker claim was never released, so the queue still shows them in flight. Once the subagent has reported, release each claim: {}",
                claims_held_after_report
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                claims_held_after_report
                    .iter()
                    .map(|id| {
                        let generation = active_claim_generations
                            .get(id)
                            .filter(|generation| **generation > 0)
                            .map(|generation| format!(" --generation {generation}"))
                            .unwrap_or_default();
                        format!(
                            "`agent-doc queue release {} --item '#{id}'{generation}`",
                            file.display()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    let skipped_queue_head_ids: std::collections::HashSet<String> = if activation.active {
        // `#presetargdedup`: a `#gh-fix <url>` head names a prompt PRESET, not a
        // tracked item. Treating `gh-fix` as a dispatched-but-unresolved item id
        // skipped every sibling invocation after the first (`⏭️`), so the drain
        // jumped past operator-queued work. Only ids that are not preset-only
        // take part in the skip set.
        let preset_only = preset_only_identity_ids(&current_content, &effective_prompt_presets);
        let current_live_ids: std::collections::HashSet<String> = activation
            .entries_after
            .iter()
            .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
            .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
            .filter(|id| !preset_only.contains(id))
            .collect();
        let prior = agent_doc_cycle_state_io::load(file).ok().flatten();
        let carried: std::collections::HashSet<String> = prior
            .as_ref()
            .map(|s| s.skipped_queue_head_ids.iter().cloned().collect())
            .unwrap_or_default();
        let prior_resolved: std::collections::HashSet<String> = prior
            .as_ref()
            .map(|s| {
                s.pending_done_ids
                    .iter()
                    .chain(s.reaped_pending_ids.iter())
                    .chain(s.pending_gated_ids.iter())
                    .map(|id| id.trim().trim_start_matches('#').to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        let prior_dispatched = prior
            .as_ref()
            .filter(|s| prior_cycle_hosted_a_turn(s))
            .and_then(|s| {
                prior_dispatched_queue_id(
                    &s.active_queue_heads,
                    &carried,
                    &preset_only,
                    &skip_exempt_ids,
                )
            });
        let operator_unskipped =
            agent_doc_queue::queue_projection::operator_unskipped_queue_ids(diff);
        if !operator_unskipped.is_empty() {
            let mut ids = operator_unskipped.iter().cloned().collect::<Vec<_>>();
            ids.sort();
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_queue_operator_unskip file={} ids={} (#releaseskipcarried)",
                    file.display(),
                    ids.join(","),
                ),
            );
        }
        let mut fresh = advance_skipped_queue_head_ids(
            carried,
            prior_dispatched.as_deref(),
            &prior_resolved,
            &current_live_ids,
            &operator_unskipped,
        );
        fresh.retain(|id| !skip_exempt_ids.contains(id));
        fresh
    } else {
        std::collections::HashSet::new()
    };
    if let Err(err) = agent_doc_cycle_state_io::set_skipped_queue_head_ids(
        file,
        &skipped_queue_head_ids.iter().cloned().collect::<Vec<_>>(),
    ) {
        eprintln!("[preflight] queue: failed to persist skipped-head set ({err:#})");
    }

    let mut in_progress_markers_changed = false;
    // `#queueclaim`: a head whose worker is still claimed/dispatching or
    // running outside the in-session loop is in flight; this cycle must not
    // select or consume it. Reported attempts stay fenced from redispatch but
    // return to coordinator selection for integration/writeback
    // (`#reportedwritebackdebt`). `#closeout-steering`: likewise a new
    // subagent-intent line the cycle must dispatch (`queue_subagent_dispatch`).
    // Both leave the selection projection BEFORE it picks heads, so the next
    // coordinator-owned head runs inline instead of the cycle selecting
    // nothing.
    let in_flight_queue_items = if activation.active {
        agent_doc_queue_io::queue_claim::in_flight_claimed_items_for_content(file, &current_content)
    } else {
        agent_doc_queue::queue_claim::ClaimedQueueItems::none()
    };
    let subagent_dispatch = if activation.active {
        agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(
            file,
            &current_content,
        )
    } else {
        Vec::new()
    };
    let out_of_loop_queue_items = if activation.active {
        agent_doc_queue_io::subagent_dispatch::out_of_loop_items_for_content(file, &current_content)
    } else {
        agent_doc_queue::queue_claim::ClaimedQueueItems::none()
    };
    let projection_entries = entries_without_out_of_loop_heads(
        &activation.entries_after,
        &in_flight_queue_items,
        &subagent_dispatch,
    );
    let active_queue_projection = if activation.active {
        let current_components = agent_doc_element::element::parse(&current_content)?;
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &current_content,
            &projection_entries,
            &agent_doc_queue::backlog_sync::collect_after_deps(
                &current_components,
                &current_content,
            ),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &current_content,
                &activation.entries_after,
                &load_projected_in_progress_queue_heads(file),
            ),
            &skipped_queue_head_ids,
        )
    } else {
        agent_doc_document::queue_projection::ActiveQueuePromptProjection::default()
    };
    if !skipped_queue_head_ids.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "preflight_queue_skip file={} skipped={} (#queueskip)",
                file.display(),
                skipped_queue_head_ids.len(),
            ),
        );
    }
    if active_queue_projection.retargeted {
        eprintln!(
            "[preflight] queue: honored operator in-progress marker retarget to {} active head(s)",
            active_queue_projection.prompts.len()
        );
    }
    if !active_queue_projection.missing_dependency_ids.is_empty() {
        queue_warnings.push(PreflightWarning {
            code: "queue_retarget_missing_prerequisite".to_string(),
            message: format!(
                "operator-selected queue head has prerequisite id(s) not present as live queue prompts: {}",
                active_queue_projection
                    .missing_dependency_ids
                    .iter()
                    .map(|id| format!("#{id}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    let active_queue_prompt_texts: Vec<String> = active_queue_projection
        .prompts
        .into_iter()
        .filter(|prompt| !in_flight_queue_items.claims(prompt))
        .filter(|prompt| {
            !agent_doc_queue_io::subagent_dispatch::is_dispatch_item(&subagent_dispatch, prompt)
        })
        .collect();
    // `#queueclaimstate`: selection excludes blocking attempts, while the
    // visible queue projection derives every attempt state from the ledger.
    // Durable completion cells can restore a matching attempt to Reported
    // after restart; unmatched/older-generation evidence never creates state.
    if activation.active
        && let Err(err) =
            agent_doc_queue_io::queue_claim::reconcile_completion_evidence(file, &current_content)
    {
        eprintln!("[preflight] queue: completion evidence recovery failed ({err:#})");
    }
    let claim_ledger = activation
        .active
        .then(|| agent_doc_queue_io::queue_claim::load_ledger_following(file, &current_content));
    let receipt_ledger = activation
        .active
        .then(|| agent_doc_queue_io::claim_release_receipt::load(file));
    let lifecycle_state_degraded = claim_ledger.as_ref().is_some_and(Result::is_err)
        || receipt_ledger.as_ref().is_some_and(Result::is_err);
    if let Some(Err(err)) = &claim_ledger {
        eprintln!("[preflight] queue: failed to read typed claim states ({err:#})");
    }
    if let Some(Err(err)) = &receipt_ledger {
        eprintln!("[preflight] queue: failed to read reported outbox ({err:#})");
    }
    let now = agent_doc_queue_io::queue_claim::claim_clock_now_secs();
    let claim_ledger = claim_ledger.and_then(Result::ok).unwrap_or_default();
    let cancellations = claim_ledger.cancellations;
    let claim_states = claim_ledger
        .claims
        .into_iter()
        .map(|mut claim| {
            if claim.state != agent_doc_queue::queue_claim::QueueClaimState::Reported
                && claim.is_expired(now)
            {
                claim.state = agent_doc_queue::queue_claim::QueueClaimState::Lost;
            }
            claim
        })
        .collect::<Vec<_>>();
    let report_receipts = receipt_ledger
        .and_then(Result::ok)
        .unwrap_or_default()
        .receipts
        .into_iter()
        .filter(|receipt| {
            !cancellations
                .iter()
                .any(|cancelled| cancelled.cancels_attempt(&receipt.identity, receipt.generation))
        })
        .collect::<Vec<_>>();
    let mut in_progress_queue_prompt_texts = active_queue_prompt_texts.clone();
    in_progress_queue_prompt_texts.extend(
        claim_states
            .iter()
            .filter(|claim| claim.state.blocks_dispatch())
            .map(|claim| claim.item_text.clone()),
    );
    if activation.active
        && let Err(err) = agent_doc_cycle_state_io::set_projected_in_progress_queue_heads(
            file,
            &in_progress_queue_prompt_texts
                .iter()
                .map(|text| strip_priority_markers(text))
                .collect::<Vec<_>>(),
        )
    {
        eprintln!("[preflight] queue: failed to persist projected in-progress heads ({err:#})");
    }
    let marker_components = agent_doc_element::element::parse(&current_content)?;
    let marker_body = agent_doc_queue::queue_set::component_for_id(
        &current_content,
        &marker_components,
        &selected_queue_id,
    )?
    .context("queue maintenance: queue component vanished before in-progress marker")?
    .content(&current_content)
    .to_string();
    let active_identities = active_queue_prompt_texts
        .iter()
        .map(|text| agent_doc_queue::queue_claim::claim_identity(text))
        .collect::<std::collections::HashSet<_>>();
    let claim_by_identity = claim_states
        .iter()
        .map(|claim| (claim.identity.clone(), claim))
        .collect::<std::collections::HashMap<_, _>>();
    let receipt_by_identity = report_receipts
        .iter()
        .filter(|receipt| !claim_by_identity.contains_key(&receipt.identity))
        .map(|receipt| (receipt.identity.clone(), receipt))
        .collect::<std::collections::HashMap<_, _>>();
    let completion_items =
        agent_doc_document::subagent_completion::completion_evidence(&current_content)
            .into_iter()
            .filter(|evidence| {
                !cancellations
                    .iter()
                    .any(|cancelled| cancelled.cancels_completion_key(&evidence.key))
            })
            .map(|evidence| evidence.item)
            .collect::<Vec<_>>();
    let row_blockers = agent_doc_queue::queue_blocked::queue_blockers(&current_content);
    // `#strikeisterminal`, per queue: a struck row whose identity is also live in this queue
    // is an earlier attempt's record, never the claim's row.
    let live_rows = agent_doc_queue::queue_claim::live_row_identities(&activation.entries_after);
    let lifecycle_targets = activation
        .entries_after
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let prompt = match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                | agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => prompt,
                _ => return None,
            };
            let identity =
                agent_doc_queue::queue_claim::claim_identity_at(&activation.entries_after, index)
                    .unwrap_or_else(|| agent_doc_queue::queue_claim::claim_identity(&prompt.text));
            let claim = claim_by_identity.get(&identity).copied();
            let receipt = receipt_by_identity.get(&identity).copied();
            // `#strikeisterminal`: only a live attempt reopens a struck row;
            // a receipt or completion cell agrees with the strike, so the
            // residue strike earlier in this pass is never undone here.
            let durable_on_struck = matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Completed(_)
            ) && agent_doc_queue::queue_claim::struck_row_reopened_in_queue(
                claim, &identity, &live_rows,
            );
            if matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Completed(_)
            ) && !durable_on_struck
            {
                return None;
            }
            let marker = if lifecycle_state_degraded || durable_on_struck {
                agent_doc_document::queue_projection::QueueLifecycleMarker::Unknown
            } else if let Some(claim) = claim {
                claim.state.projection_marker()
            } else if receipt.is_some() {
                agent_doc_document::queue_projection::QueueLifecycleMarker::Reported
            } else if completion_items.iter().any(|item| {
                agent_doc_queue::queue_claim::completion_evidence_matches_row(item, &prompt.text)
            }) {
                agent_doc_document::queue_projection::QueueLifecycleMarker::Reconciling
            } else if active_identities.contains(&identity) {
                agent_doc_document::queue_projection::QueueLifecycleMarker::Running
            } else if row_blockers.blocks_row(&prompt.text) {
                // `#queueblocked`: held out of drain and dispatch; names the
                // open ids it waits on.
                agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
            } else {
                agent_doc_document::queue_projection::QueueLifecycleMarker::Pending
            };
            let owner = claim
                .map(
                    |claim| agent_doc_document::queue_projection::QueueOwnerProjection {
                        owner: claim.owner.clone(),
                        generation: claim.generation,
                    },
                )
                .or_else(|| {
                    receipt.map(|receipt| {
                        agent_doc_document::queue_projection::QueueOwnerProjection {
                            owner: receipt.owner.clone(),
                            generation: receipt.generation,
                        }
                    })
                });
            Some(agent_doc_queue::document_queue::QueueRowProjection {
                text: prompt.text.clone(),
                blockers: if marker
                    == agent_doc_document::queue_projection::QueueLifecycleMarker::Blocked
                {
                    row_blockers
                        .blockers_for_row(&prompt.text)
                        .map(<[String]>::to_vec)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                },
                marker,
                owner,
                reopen_completed: durable_on_struck,
            })
        })
        .collect::<Vec<_>>();
    let tracked_work_lifecycle = lifecycle_targets
        .iter()
        .filter(|target| {
            matches!(
                target.marker,
                agent_doc_document::queue_projection::QueueLifecycleMarker::Claimed
                    | agent_doc_document::queue_projection::QueueLifecycleMarker::Running
                    | agent_doc_document::queue_projection::QueueLifecycleMarker::Reported
                    | agent_doc_document::queue_projection::QueueLifecycleMarker::Lost
                    | agent_doc_document::queue_projection::QueueLifecycleMarker::Reconciling
                    | agent_doc_document::queue_projection::QueueLifecycleMarker::Unknown
            )
        })
        .filter_map(
            |target| match agent_doc_queue::queue_claim::claim_identity(&target.text) {
                agent_doc_element_queue::QueueItemIdentity::Id(id) => Some((id, target.marker)),
                agent_doc_element_queue::QueueItemIdentity::FreeText(_) => None,
            },
        )
        .collect::<std::collections::HashMap<_, _>>();
    if let Some((new_body, marked_entries)) =
        agent_doc_queue::document_queue::project_prompt_runtime_projections(
            &marker_body,
            &lifecycle_targets,
        )?
    {
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: queue component vanished before in-progress marker")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = marked_entries;
        mutated = true;
        in_progress_markers_changed = true;
    }
    // `#queueskip` / `#queueawaitop`: stamp `⏭️` on skipped heads and `⏳` on
    // heads waiting on the operator (and clear either from heads that no longer
    // qualify), AFTER the `🚧` pass so the selected head and every claimed
    // worker head keep `🚧`.
    if let Some(skip_marked) = agent_doc_queue::document_queue::set_prompts_lifecycle_markers(
        &activation.entries_after,
        &skipped_queue_head_ids,
        &awaiting_operator_ids,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&skip_marked);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: queue component vanished before skip marker")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = skip_marked;
        mutated = true;
        in_progress_markers_changed = true;
    }
    // `#queueactionablecontext`: leaf-only execution makes nested parents
    // context. Once every existing leaf is terminal, close a plain context row
    // or move unresolved lifecycle/operator evidence to one executable child.
    // Never leave a blocked/lost/operator-gated parent as an inert queue end.
    if let Some(settled_entries) = agent_doc_queue::document_queue::settle_inert_group_parents(
        &activation.entries_after,
        &open_todo_ids,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&settled_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &comps,
                &selected_queue_id,
            )?
            .context("queue maintenance: queue component vanished during context settlement")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = settled_entries;
        mutated = true;
        in_progress_markers_changed = true;
        eprintln!(
            "[preflight] queue: settled inert context parents into terminal history or actionable leaves (#queueactionablecontext)"
        );
    }
    // `#queuemirrormarkers`: claims, report receipts, and completion cells are
    // keyed by work identity, not queue id, so every queue that displays an
    // identity must show its durable attempt state. The passes above project
    // only the selected queue; repaint every sibling queue's live rows from the
    // same ledgers so a mirrored copy cannot keep an expired generation, a
    // released report, or a `🚧` left by an earlier pass that selected its
    // queue. A sibling's struck rows are never reopened.
    match agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection_scoped(
        file,
        &current_content,
        None,
        agent_doc_queue_io::queue_cmd::ClaimProjectionScope::SiblingsOf {
            projected_queue_id: &selected_queue_id,
            selected_heads: &active_queue_prompt_texts,
        },
    ) {
        Ok(projection) if projection.changed => {
            current_content = projection.target_content;
            mutated = true;
            in_progress_markers_changed = true;
            eprintln!(
                "[preflight] queue: projected durable attempt state onto sibling queue rows (#queuemirrormarkers)"
            );
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!("[preflight] queue: sibling queue marker projection skipped ({err:#})");
        }
    }
    let (marked_content, pending_markers_changed) =
        set_work_item_lifecycle_markers(&current_content, &tracked_work_lifecycle)?;
    if pending_markers_changed {
        current_content = marked_content;
        mutated = true;
        in_progress_markers_changed = true;
    }
    let need_sync_active_queue_future_state_snapshot = if activation.active && snapshot_was_active {
        match agent_doc_snapshot_io::load_document_baseline(file) {
            Ok(Some(snapshot_content)) => {
                selected_queue_head_unchanged_in_snapshot(
                    &snapshot_content,
                    &activation.entries_after,
                ) && queue_region_differs_from_snapshot(&snapshot_content, &current_content)
            }
            _ => false,
        }
    } else {
        false
    };

    // Persist file mutations.
    if mutated {
        current_content = persist_queue_maintenance_doc(
            file,
            &current_content,
            &authority_baseline,
            project_root.as_deref(),
            "queue_maintenance",
        )?;
        agent_doc_queue_io::claim_release_receipt::retire_struck_or_warn(
            file,
            &struck_heads_this_pass,
            &current_content,
            "queue_maintenance",
        );
    }

    // Persist snapshot mutations. For newly activated queues, sync the queue
    // component from the visible document into the snapshot so later closeout
    // consumption can prove the same head prompt in both places.
    if (mutated
        || need_sync_newly_activated_queue_snapshot
        || need_sync_active_queue_future_state_snapshot)
        && let Ok(Some(snap_content)) = agent_doc_snapshot_io::load_document_baseline(file)
    {
        let mut new_snap = agent_doc_queue::queue_consume::retire_consumed_completion_feedback(
            &snap_content,
            &struck_heads_this_pass,
        )
        .0;

        if in_progress_markers_changed {
            new_snap = sync_in_progress_marker_regions(&new_snap, &current_content);
        }

        if queue_tag_attrs_normalized
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Ok(Some(snap_q)) = agent_doc_queue::queue_set::component_for_id(
                &new_snap,
                &snap_comps,
                &selected_queue_id,
            )
        {
            let raw_tag = &new_snap[snap_q.open_start..snap_q.open_end];
            let normalized_tag =
                agent_doc_queue::document_queue::normalize_queue_tag_attrs(raw_tag);
            if normalized_tag != raw_tag {
                let mut rebuilt = String::with_capacity(new_snap.len());
                rebuilt.push_str(&new_snap[..snap_q.open_start]);
                rebuilt.push_str(&normalized_tag);
                rebuilt.push_str(&new_snap[snap_q.open_end..]);
                new_snap = rebuilt;
            }
        }

        if (need_sync_newly_activated_queue_snapshot
            || need_sync_active_queue_future_state_snapshot)
            && let Ok(current_comps) = agent_doc_element::element::parse(&current_content)
            && let Ok(Some(current_q)) = agent_doc_queue::queue_set::component_for_id(
                &current_content,
                &current_comps,
                &selected_queue_id,
            )
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Ok(Some(snap_q)) = agent_doc_queue::queue_set::component_for_id(
                &new_snap,
                &snap_comps,
                &selected_queue_id,
            )
        {
            let queue_region = &current_content[current_q.open_start..current_q.close_end];
            let mut rebuilt = String::with_capacity(new_snap.len() + queue_region.len());
            rebuilt.push_str(&new_snap[..snap_q.open_start]);
            rebuilt.push_str(queue_region);
            rebuilt.push_str(&new_snap[snap_q.close_end..]);
            new_snap = rebuilt;
        }

        // Apply queue body change to snapshot
        if !need_sync_newly_activated_queue_snapshot
            && (activation.consumed_start_fence || need_strip_activation || need_clear_drained_body)
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Ok(Some(snap_q)) = agent_doc_queue::queue_set::component_for_id(
                &new_snap,
                &snap_comps,
                &selected_queue_id,
            )
        {
            let new_body = if need_clear_drained_body {
                String::new()
            } else {
                agent_doc_queue::document_queue::render(&activation.entries_after)
            };
            new_snap = snap_q.replace_content(&new_snap, &new_body);

            if strip_queue_marker_control
                && let Ok(snap_comps2) = agent_doc_element::element::parse(&new_snap)
                && let Ok(Some(snap_q2)) = agent_doc_queue::queue_set::component_for_id(
                    &new_snap,
                    &snap_comps2,
                    &selected_queue_id,
                )
            {
                let raw_tag = &new_snap[snap_q2.open_start..snap_q2.open_end];
                let new_tag = agent_doc_queue::document_queue::strip_control_from_tag(
                    &agent_doc_queue::document_queue::strip_auto_from_tag(raw_tag),
                );
                if new_tag != raw_tag {
                    let mut rebuilt = String::with_capacity(new_snap.len());
                    rebuilt.push_str(&new_snap[..snap_q2.open_start]);
                    rebuilt.push_str(&new_tag);
                    rebuilt.push_str(&new_snap[snap_q2.open_end..]);
                    new_snap = rebuilt;
                }
            }
        }

        // Apply frontmatter change to snapshot
        if need_set_active && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true) {
            new_snap = merged;
        } else if need_sync_newly_activated_queue_snapshot
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, true)
        {
            new_snap = merged;
        } else if need_clear_active
            && let Ok(merged) = if marker_stop {
                frontmatter::merge_queue_hold(&new_snap)
            } else {
                frontmatter::clear_queue_control(&new_snap)
            }
        {
            new_snap = merged;
        }
        if need_clear_drained_body
            && let Ok(merged) = if need_clear_active && drain_clears_control {
                frontmatter::clear_queue_control(&new_snap)
            } else {
                frontmatter::merge_queue_state(&new_snap, false)
            }
        {
            new_snap = merged;
        }

        if new_snap != snap_content
            && let Err(e) = agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &new_snap,
                agent_doc_ops_log_io::log_op,
            )
        {
            eprintln!("[preflight] queue: snapshot sync warning: {}", e);
        }
    }

    // Retain the final editor-authoritative queue frontier independently of the
    // committed snapshot. A queue head may be added after the snapshot and then
    // deleted by the operator before another commit; without this frontier the
    // backlog mirror mistakes that deletion for "never mirrored" and resurrects
    // the head on the next pass.
    let observed_active_queue_ids: std::collections::HashSet<String> = activation
        .entries_after
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Prompt(_)
            )
        })
        .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        .collect();
    agent_doc_queue_io::queue_tombstone::record_observed_active_ids(
        file,
        &observed_active_queue_ids,
    );

    // Build output
    let queue_prompts: Vec<String> = if activation.active {
        agent_doc_queue::document_queue::prompts(&activation.entries_after)
            .iter()
            .map(|p| strip_in_progress_marker(&p.text))
            .collect()
    } else {
        vec![]
    };

    // `#cleardrainsignal`: count heads the agent can actually drain this session,
    // applying the same `#goqueuestall`/`#goqstall2` deferred/noise filtering the
    // supervisor idle-watch uses. When the queue is active but this is 0, the
    // remaining heads are all `[clean-session]` (under live IPC) / `[operator-verify]`
    // / inert noise — a no-op churn cycle. Surfacing it lets the agent and the
    // Claude Code auto-loop stop without re-deriving drainability from prose, even
    // when the route-owned supervisor predates the idle-watch filter (#qchurn).
    // `#qpausego`: an accepted controller `admin queue pause` suppresses the
    // *unattended* supervisor idle-watch auto-injection (the flood this fixes —
    // see `start/idle_watch.rs`) and is surfaced here as `queue_paused` for
    // visibility. It deliberately does NOT drop `queue_continuation_required` or
    // `queue_drainable_head_count`: the attended in-session `/loop` is the
    // legitimate single-owner drain of real queue work and must keep going. A
    // pause stalling the in-session loop strands genuine drainable backlog
    // (`#qdurcrash`, `#733r`, …) — the operator-rejected over-reach. Use
    // `queue: stop` frontmatter / `--- stop` fences to stop the in-session loop.
    let queue_pause_reason =
        agent_doc_queue_io::controller_pause::document_queue_controller_pause_reason(file);
    let queue_paused = queue_pause_reason.is_some();
    // Recompute owner claims on the post-maintenance document. The set
    // captured when the queue component was chosen is stale once this
    // function rewrites `current_content`.
    let owner_selection = agent_doc_queue_io::subagent_dispatch::owner_selection_claims_for_content(
        file,
        &current_content,
    );
    let queue_drainable_head_count = if activation.active
        && authority_queue_unresolved_prompts == Some(0)
        && current_content == authority_baseline
    {
        0
    } else if activation.active {
        // GH #381: project-inherited presets count as preset invocations.
        agent_doc_queue::queue_continuation::drainable_head_count_for_owner(
            &current_content,
            &owner_selection,
            &out_of_loop_queue_items,
            &effective_prompt_presets,
        )
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    // `#rt83`: supervisor-scope drainability (defers `[operator-verify]`/noise only).
    // Used to gate the preflight synthetic queue-head diff so an operator-verify-only
    // (or otherwise non-actionable) head stops perpetually reporting `no_changes:false`.
    //
    // `#explicitrunqueue`: claimed heads are excluded exactly as the read-only
    // probe path and `queue_drainable_head_count` exclude them. A head a live
    // subagent claimed is in flight elsewhere; counting it here made an explicit
    // Run Agent Doc synthesize `+do [#id]` for the claimed head on every press,
    // opening a no-op cycle that tripped `expect_done_or_gate_guard`.
    let queue_supervisor_continuation_head = if activation.active {
        agent_doc_queue::queue_continuation::live_drainable_head_prompt_text_for_owner(
            &current_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
            &owner_selection,
            &out_of_loop_queue_items,
            &effective_prompt_presets,
        )
    } else {
        None
    };
    let queue_supervisor_drainable = queue_supervisor_continuation_head.is_some();
    let claimed_queue_heads = if activation.active {
        claimed_queue_head_facts(file, &current_content)
    } else {
        Vec::new()
    };
    // Publish the final authority frontier before worklist/selection facts.
    // Lifecycle persistence remains a projection Effect, not a maintenance
    // companion write.
    agent_doc_queue_io::queue_consume::observe_authoritative_queue_state(file, &current_content)?;
    record_queue_worklist_state(
        file,
        &current_content,
        &activation.entries_after,
        activation.active,
    )?;
    if activation.active && !active_queue_prompt_texts.is_empty() {
        for head_text in &active_queue_prompt_texts {
            record_selected_queue_head_state(file, &current_content, head_text, true)?;
        }
    } else if activation.deferred
        && let Some(head) = agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
    {
        let head_text = strip_in_progress_marker(&head.text);
        let reason = activation
            .start_at
            .as_deref()
            .map(|start_at| format!("time_gate:{start_at}"))
            .unwrap_or_else(|| "deferred".to_string());
        record_deferred_queue_head_state(file, &current_content, &head_text, &reason)?;
    }

    Ok(QueueState {
        authoritative_content: Some(current_content),
        queue_prompts,
        selected_queue_prompts: active_queue_prompt_texts,
        queue_active: if activation.active {
            Some(true)
        } else if activation.deferred {
            None
        } else if persisted_active || explicit_stop {
            Some(false)
        } else {
            None
        },
        queue_deferred: activation.deferred,
        queue_start_at: activation.start_at,
        queue_trigger: activation.trigger,
        queue_halted: None,
        queue_paused,
        queue_pause_reason,
        queue_drainable_head_count,
        queue_continuation_required,
        queue_supervisor_drainable,
        queue_supervisor_continuation_head,
        // `#qmaintorphan`: only the persisted mutation counts. `mutated` is set
        // by the branches above and written to BOTH the visible document and the
        // snapshot by `persist_queue_maintenance_doc` + the snapshot sync block;
        // the earlier stop-fence / time-gate returns exit before that persist, so
        // they report `false` and leave no plane split behind.
        maintenance_mutated: mutated,
        claimed_queue_heads,
        synced_queue_ids,
        free_text_promotions,
        warnings: queue_warnings,
    })
}

/// The document image the same-cycle queue mirror decides against: the live
/// authority's current text when it answers `Current`, else the disk
/// projection (detached documents, no controller, or a not-ready head — the
/// CAS in `persist_queue_maintenance_doc` still fences the write itself).
fn same_cycle_queue_sync_base(file: &Path) -> Option<String> {
    if let Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) =
        current_text_via_preflight_authority(file, "actionable_backlog_sync_base")
    {
        return Some(text);
    }
    std::fs::read_to_string(file).ok()
}

/// Closeout-side repair for same-cycle backlog capture.
///
/// Preflight's normal backlog→queue sync runs before `finalize` / `write`
/// applies backlog mutations, so a go-mode document can commit a fresh or newly
/// ungated backlog item without a matching queue head. This helper runs after
/// closeout queue consumption, enqueuing only ids explicitly recorded as made
/// actionable in this cycle. It never applies a full queue recompute, so it
/// cannot resurrect unrelated operator-deleted heads.
///
/// `#queueatcreate`: placement is caller-chosen and defaults to the queue HEAD.
/// This used to hardcode `Append`, which buried every follow-up behind the whole
/// existing queue — observed live, where four follow-ups filed in one turn sat
/// invisible behind a 112-line queue and the operator re-added them by hand. A
/// follow-up filed by the turn that just ran is, by construction, the most
/// task-relevant work in the document, so the head is the right default; agents
/// that deliberately do not want to preempt the current drain pass `Append`.
pub fn sync_same_cycle_actionable_backlog_into_go_queue(
    file: &Path,
    placement: agent_doc_queue::backlog_sync::FollowUpQueuePlacement,
) -> Result<Vec<String>> {
    let actionable_this_cycle = agent_doc_cycle_state_io::pending_actionable_ids(file);
    if actionable_this_cycle.is_empty() {
        return Ok(Vec::new());
    }

    // `#respondcloseoutbacklog`: decide "already queued" against the live
    // authority, not the disk projection. A retained closeout's queue head can
    // sit in the editor/controller model while disk still lags; reading disk
    // made a second same-cycle sync (the foreground closeout and the
    // captured-finalize resume each run one) re-add a head the authority
    // already held, and the queue-maintenance CAS then rebased that insert over
    // the live head and kept BOTH copies.
    let content = match same_cycle_queue_sync_base(file) {
        Some(c) => c,
        None => return Ok(Vec::new()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    // GH #378: gate and diff against the component the mirror writes into (the
    // first non-repeating `agent:run`), never the first `queue` component: a
    // sibling `agent:loop` above the run carries no `go`, so reading it here
    // silently dropped every same-cycle `--backlog-add` mirror.
    let Some(queue_component) = agent_doc_queue::backlog_sync::mirror_queue_component(&components)
    else {
        return Ok(Vec::new());
    };
    let queue_body = &content[queue_component.open_end..queue_component.close_start];
    let entries = match agent_doc_queue::document_queue::parse(queue_body) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!(
                "[write] queue: same-cycle actionable backlog sync skipped — parse warning: {e}"
            );
            return Ok(Vec::new());
        }
    };

    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let queue_go_mode =
        explicit_queue_go_mode(&queue_component.attrs, frontmatter_queue_control(&fm));
    let queue_active = fm.queue_active.unwrap_or(false) || queue_go_mode;
    if !queue_active || !queue_go_mode {
        return Ok(Vec::new());
    }

    let backlog_has_queue_attr = components.iter().any(|comp| {
        comp.name == "backlog"
            && comp
                .attrs
                .get("queue")
                .and_then(|value| {
                    agent_doc_queue::document_queue::BacklogQueueSyncMode::parse(value)
                })
                .is_some()
    });
    if !backlog_has_queue_attr {
        return Ok(Vec::new());
    }

    let Some(sync_request) =
        agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content)
    else {
        return Ok(Vec::new());
    };
    let actionable_norm: std::collections::HashSet<String> = actionable_this_cycle
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| !id.is_empty())
        .collect();
    let mut backlog_ids: Vec<String> = sync_request
        .ids
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| actionable_norm.contains(id))
        .collect();
    if backlog_ids.is_empty() {
        return Ok(Vec::new());
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_project_root_io::project_root_containing(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let done_ids: std::collections::HashSet<String> = collect_agent_done_ids(file, &content)?
        .into_iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    if !done_ids.is_empty() {
        backlog_ids.retain(|id| !done_ids.contains(&id.to_ascii_lowercase()));
    }

    let exec_ctxs = agent_doc_queue::queue_continuation::collect_backlog_execution_contexts(
        &components,
        &content,
    );
    if exec_ctxs.values().any(|ctx| ctx.is_deferred()) {
        let (drainable, skipped) =
            agent_doc_queue::queue_continuation::partition_drainable_backlog_ids(
                &backlog_ids,
                &exec_ctxs,
            );
        for skip in skipped {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "closeout_queue_skip_same_cycle_pending_add id=#{} skip={}",
                    skip.id, skip.reason
                ),
            );
        }
        backlog_ids = drainable;
    }
    if backlog_ids.is_empty() {
        return Ok(Vec::new());
    }

    let pre_sync_ids = entries
        .iter()
        .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        .collect::<std::collections::HashSet<String>>();
    // `#backlogqueuepopulation`: use the insert-only backlog ingress helper for
    // the primary path too. Besides preserving every pre-existing queue byte,
    // this is the single place that orders a newly actionable block by hard
    // `after=#id` dependencies and then priority.
    // `#queuemirrororder`: ids filed with an explicit `--backlog-add-after` /
    // `--backlog-add-before` anchor follow that anchor into the mirror; everything
    // else keeps `placement` (the `#queueatcreate` head default).
    let anchored_ids: Vec<String> = agent_doc_cycle_state_io::pending_anchored_ids(file)
        .into_iter()
        .collect();
    let current_content = match agent_doc_queue::backlog_sync::enqueue_actionable_ids_reporting(
        &content,
        &backlog_ids,
        placement,
        &anchored_ids,
    )? {
        agent_doc_queue::backlog_sync::MirrorOutcome::Mirrored(next) => next,
        agent_doc_queue::backlog_sync::MirrorOutcome::Refused(refusal) => {
            // `#bkqattrcli`: a document that can NEVER mirror is a configuration
            // defect, not a quiet no-op. Silently returning here made the "+ queue"
            // half of a capture instruction land nothing while the cycle reported
            // success, so say it once, with the command that fixes it.
            if refusal.is_configuration_defect() {
                eprintln!(
                    "[closeout] {} backlog id(s) could not mirror into agent:queue: {}",
                    backlog_ids.len(),
                    refusal.remedy()
                );
                // The refusal renders its own ops-log line, so its reason token
                // stays owned by the flow enum instead of this hot path.
                agent_doc_ops_log_io::log_op(file, &refusal.closeout_log_line(backlog_ids.len()));
            }
            return Ok(Vec::new());
        }
    };
    let updated_components = agent_doc_element::element::parse(&current_content)?;
    let updated_queue = agent_doc_queue::backlog_sync::mirror_queue_component(&updated_components)
        .context("queue component disappeared during actionable backlog reconciliation")?;
    let synced = agent_doc_queue::document_queue::parse(
        &current_content[updated_queue.open_end..updated_queue.close_start],
    )?;
    let mut seen = std::collections::HashSet::new();
    let synced_ids: Vec<String> = synced
        .iter()
        .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
        .filter(|id| !pre_sync_ids.contains(id))
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if synced_ids.is_empty() {
        return Ok(Vec::new());
    }
    // `#queueatcreate` / `#5d9f`: two persistence paths with different
    // reachability is the bug surface. `persist_queue_maintenance_doc` requires a
    // ready editor model and discards its work otherwise, but the backlog add
    // that produced these very ids persisted through the tracked-work path
    // (`converge_or_disk_write`), which succeeds under the same conditions. So a
    // single write grew `agent:backlog` while the matching `agent:queue` head was
    // silently dropped — the operator-reported "backlog items were not prepended
    // onto the queue", reproduced here as
    // `editor authority stayed in editor_attached_model_missing`.
    //
    // Prefer the queue-maintenance path (it carries the snapshot/head handling),
    // but fall back to the same converging write the backlog half used rather
    // than discarding the enqueue. This is not a force-disk escape hatch: it is
    // the identical helper the accompanying backlog mutation already ran, so it
    // converges against a live buffer exactly as that write does.
    let persisted_content = match persist_queue_maintenance_doc(
        file,
        &current_content,
        &content,
        project_root.as_deref(),
        "actionable_backlog_sync",
    ) {
        Ok(persisted) => persisted,
        Err(primary_err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_actionable_backlog_sync_persist_fallback file={} reason={} (#queueatcreate)",
                    file.display(),
                    primary_err
                ),
            );
            // NOTE: this uses the tracked-work write path, which reads its IO
            // through a thread-local effects scope. Callers must run this function
            // inside `with_backlog_command_effects` (the write runtime does), or the
            // fallback fails with "backlog command write effects are not installed"
            // — which reads like an authority failure but is purely structural.
            agent_doc_element_backlog_io::backlog_cmd::apply_document_rewrite(
                file,
                "actionable_backlog_sync_fallback",
                |live| {
                    // Recompute against the live document: the primary attempt may
                    // have observed a different image, and the backlog write that
                    // created these ids already landed there.
                    agent_doc_queue::backlog_sync::enqueue_actionable_ids_in_content(
                        live,
                        &backlog_ids,
                        placement,
                    )
                },
            )
            .with_context(|| {
                format!(
                    "same-cycle actionable backlog enqueue failed on both the queue-maintenance path ({primary_err}) \
                     and the tracked-work fallback"
                )
            })?;
            match current_text_via_preflight_authority_retrying(
                file,
                "actionable_backlog_sync_fallback_observe",
            )? {
                Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
                Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
                    std::fs::read_to_string(file)?
                }
                Some(
                    agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
                    | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending,
                ) => current_content.clone(),
            }
        }
    };
    adopt_queue_region_into_snapshot(
        file,
        &persisted_content,
        agent_doc_queue::backlog_sync::mirror_queue_component,
    );
    eprintln!(
        "[write] queue: {} {} same-cycle actionable backlog id(s) into active go queue",
        placement.log_verb(),
        synced_ids.len()
    );
    Ok(synced_ids)
}

fn resolve_free_text_execution(
    file: &Path,
    content: &str,
    project_root: Option<&Path>,
    ids: &[String],
) -> Result<(ResolvedFreeTextExecution, Vec<PreflightWarning>)> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap_or_default();
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    let global_config = agent_doc_project_config_io::apply_project_defaults(
        agent_doc_config::load().unwrap_or_default(),
        &project_config,
    );
    let requested = fm
        .free_text_execution
        .or(project_config.agent_doc_free_text_execution)
        .or(global_config.agent_doc_free_text_execution)
        .unwrap_or_default();
    let harness = agent_doc_harness::HarnessConfig::from_context(&fm, &global_config);
    let goal_available = harness.supports_goal_command(
        agent_doc_harness::opencode_goal_extension_available(file, project_root),
    );
    let (execution, warning) = agent_doc_workflow::preflight_policy::resolve_free_text_execution(
        requested,
        goal_available,
        &file.display().to_string(),
        &harness.binary,
        ids,
    );
    let warnings = warning
        .into_iter()
        .map(|warning| PreflightWarning {
            code: warning.code,
            message: warning.message,
            document_agent: fm.agent.clone(),
            active_harness: Some(harness.binary.clone()),
        })
        .collect();
    Ok((execution, warnings))
}

/// Apply queue maintenance as one compare-and-swap against the Lazily head from
/// which the maintenance plan was derived. A concurrent operator edit makes the
/// transition stale and retryable; it is never overwritten from a disk/snapshot
/// projection, which makes editor queue deletions monotonic.
/// What queue maintenance should do with an observed Lazily head (`#qdonestrike-durable`).
///
/// Pure so the not-ready → ensure → re-observe transition is unit-provable without
/// a live relay, editor, or controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueueMaintenanceHeadAction {
    /// Head is live: compare-and-swap the maintenance plan onto it.
    CompareAndSwap,
    /// No editor owns the document: project straight to disk.
    DiskWrite,
    /// Replica missing / sync pending: drive the bounded model-ensure and re-observe.
    EnsureThenReobserve,
    /// Still not ready after the ensure: fail closed rather than clobber a live buffer.
    FailClosed,
}

/// Decide the queue-maintenance action for an observed head.
///
/// `already_ensured` records whether the bounded model-ensure has already run this
/// call, which is what keeps a persistently-not-ready head from looping forever.
pub(crate) fn queue_maintenance_head_action(
    current: &agent_doc_crdt_relay_io::CurrentText,
    already_ensured: bool,
) -> QueueMaintenanceHeadAction {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Current { .. } => {
            QueueMaintenanceHeadAction::CompareAndSwap
        }
        agent_doc_crdt_relay_io::CurrentText::Detached => QueueMaintenanceHeadAction::DiskWrite,
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            if already_ensured {
                QueueMaintenanceHeadAction::FailClosed
            } else {
                QueueMaintenanceHeadAction::EnsureThenReobserve
            }
        }
    }
}

/// Stable ops-log label for a Lazily head state observed by queue maintenance.
fn queue_maintenance_head_label(current: &agent_doc_crdt_relay_io::CurrentText) -> &'static str {
    match current {
        agent_doc_crdt_relay_io::CurrentText::Current { .. } => "current",
        agent_doc_crdt_relay_io::CurrentText::Detached => "detached",
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica => {
            "editor_attached_model_missing"
        }
        agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => "editor_sync_pending",
    }
}

/// Operator re-arm keys for [`rebase_queue_maintenance_target`]
/// (`#unstrikelost`, unreadable evidence).
///
/// Re-arm keys only ever name queue items struck in the merge base, so a base
/// with no struck queue item needs no evidence. Otherwise an unreadable epoch
/// aborts the rebase rather than joining an operator unstrike back to struck.
fn rebase_operator_rearmed_keys(
    file: &Path,
    source: &str,
    expected_current: &str,
) -> Result<std::collections::HashSet<String>> {
    let base_has_struck_item = queue_entries_in_content(expected_current)
        .iter()
        .any(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Completed(_)
            )
        });
    if !base_has_struck_item {
        return Ok(std::collections::HashSet::new());
    }
    agent_doc_op_capture_io::try_operator_rearmed_queue_item_keys(file, expected_current).map_err(
        |err| {
            QueueAuthorityUnavailable::new(format!(
                "{source}: operator re-arm evidence is unreadable; refusing to rebase queue \
                 maintenance over the live head without it: {err:#}"
            ))
            .into()
        },
    )
}

fn rebase_queue_maintenance_target(
    file: &Path,
    source: &str,
    expected_current: &str,
    content: &str,
    current: &str,
) -> Result<String> {
    if current == expected_current {
        return Ok(content.to_string());
    }
    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(expected_current).encode_state();
    let rebased = agent_doc_merge::crdt::merge_by_component_with_operator_rearmed(
        Some(&base_state),
        content,
        current,
        rebase_operator_rearmed_keys(file, source, expected_current)?,
    )
    .with_context(|| {
        format!("{source}: failed to rebase queue maintenance over the live Lazily head")
    })?;
    let rebased =
        reproject_marker_only_queue_over_live_head(expected_current, content, current, &rebased)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_rebased source={source} outcome=head_advanced_preserved \
             expected_hash={} current_hash={} rebased_hash={} expected_len={} current_len={} \
             rebased_len={} recovery=component_crdt",
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(current),
            agent_doc_hash::content_hash(&rebased),
            expected_current.len(),
            current.len(),
            rebased.len(),
        ),
    );
    Ok(rebased)
}

fn queue_component_body(content: &str) -> Result<Option<&str>> {
    let components = agent_doc_element::element::parse(content)?;
    Ok(components
        .iter()
        .find(|component| component.name == "queue")
        .map(|component| component.content(content)))
}

fn reproject_marker_only_queue_over_live_head(
    expected_current: &str,
    content: &str,
    current: &str,
    rebased: &str,
) -> Result<String> {
    let (Some(expected_body), Some(content_body), Some(current_body)) = (
        queue_component_body(expected_current)?,
        queue_component_body(content)?,
        queue_component_body(current)?,
    ) else {
        return Ok(rebased.to_string());
    };
    let Some(targets) = agent_doc_queue::document_queue::in_progress_marker_only_targets(
        expected_body,
        content_body,
    )?
    else {
        return Ok(rebased.to_string());
    };
    let projected_live_body =
        match agent_doc_queue::document_queue::project_prompts_in_progress(current_body, &targets)?
        {
            Some((body, _)) => body,
            None => current_body.to_string(),
        };
    let components = agent_doc_element::element::parse(rebased)?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")
        .context("marker-only queue rebase lost the queue component")?;
    Ok(queue.replace_content(rebased, &projected_live_body))
}

/// `#ensurereplicagen` — drive the model-ensure transition through whichever
/// process actually owns the relay hub.
///
/// This is the root of the long-running `editor_attached_model_missing` wedge.
/// `agent_doc_crdt_relay_io::ensure_document_model` observes
/// `current_text_for_file*`, which resolves against `hub_registry()` — a
/// PROCESS-LOCAL map that only the project controller ever populates
/// (`replica_register` is served by the controller RPC). Queue maintenance runs
/// in the short-lived preflight CLI process, so it was asking its own always-empty
/// registry. Once durable liveness reports the editor attached, that miss reads
/// as `EditorAttachedMissingReplica` **every single time, for the life of the
/// process** — no editor behaviour can ever clear it.
///
/// Proof from a live wedge: the JetBrains plugin registered successfully
/// (`transport.register ok=true`, forwarder cached) and the controller recorded
/// `controller_crdt_replica_handled method=replica_register data_kind=ok`, while
/// `crdt_current_text_unavailable ... reason=missing_replica process_pid=<CLI>`
/// was logged from a pid that was not the controller. The replica existed the
/// whole time; the reader was simply asking the wrong process.
///
/// `idle_watch.rs` already documents this hazard and routes around it, and
/// `document-realtime-io` has the same controller-routed shape. Queue
/// maintenance was the remaining direct caller.
fn ensure_document_model_via_authority(
    file: &Path,
    source: &str,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
        return agent_doc_crdt_relay_io::ensure_document_model(file, source);
    }
    match agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
        file, source,
    )? {
        Some(current) => Ok(current),
        None => Ok(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica),
    }
}

/// `#px82`/`#bn41` — retry `ensure_document_model` with an explicit replica
/// re-registration between attempts.
///
/// The recomputed queue is the expensive part of maintenance and it is thrown
/// away wholesale when this ensure fails, so the retry budget belongs here
/// rather than at the caller. Each attempt records its observed status, and the
/// ORIGINAL error is returned if the budget is exhausted so the operator-facing
/// message keeps its existing diagnostic wording.
fn ensure_document_model_with_replica_reregistration(
    file: &Path,
    source: &str,
    first_err: anyhow::Error,
) -> Result<agent_doc_crdt_relay_io::CurrentText> {
    let attempts = queue_authority_attempts();
    let mut last_err = first_err;
    for attempt in 1..=attempts {
        let reregister = match agent_doc_crdt_relay_io::signal_crdt_replica_event(
            file,
            agent_doc_crdt_relay_io::CrdtReplicaEventReason::EditorReplicaReregister,
            0,
        ) {
            Ok(()) => "requested".to_string(),
            Err(err) => format!("failed:{}", format!("{err:#}").replace('\n', "\\n")),
        };
        std::thread::sleep(QUEUE_AUTHORITY_RETRY_BACKOFF);
        let retried = ensure_document_model_via_authority(file, source);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_model_ensure_retry file={} source={} attempt={}/{} reregister={} outcome={} (#px82)",
                file.display(),
                source,
                attempt,
                attempts,
                reregister,
                match &retried {
                    Ok(current) => queue_maintenance_head_label(current).to_string(),
                    Err(err) => format!(
                        "failed:{}",
                        format!("{err:#}")
                            .replace('\n', " | ")
                            .chars()
                            .take(160)
                            .collect::<String>()
                    ),
                }
            ),
        );
        match retried {
            Ok(current) => return Ok(current),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

/// `#stalemarkerretarget`: the heads the binary last projected `🚧` onto, so a
/// marker it wrote itself is never read back as an operator retarget.
fn load_projected_in_progress_queue_heads(file: &Path) -> std::collections::HashSet<String> {
    agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .map(|state| {
            state
                .projected_in_progress_queue_heads
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

/// A queue-maintenance CAS refusal means the editor advanced after our last
/// observation. Re-observe the authoritative model, rebase the original
/// maintenance delta over that fresh cut, and retry within a small budget.
///
/// This is deliberately actorless and has no timer: every additional attempt is
/// earned by an explicit `retry_crdt_merge` refusal from the owning actor and is
/// preceded by a fresh authoritative observation.
const QUEUE_MAINTENANCE_CAS_ATTEMPTS: usize = 3;

fn queue_maintenance_cas_is_retryable(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("recovery=retry_crdt_merge")
}

fn persist_attached_queue_maintenance_with_retry(
    file: &Path,
    content: &str,
    expected_current: &str,
    source: &str,
    initial_current: String,
    mut apply: impl FnMut(&str, &str) -> Result<bool>,
    mut observe: impl FnMut() -> Result<String>,
) -> Result<String> {
    let mut current = initial_current;
    for attempt in 1..=QUEUE_MAINTENANCE_CAS_ATTEMPTS {
        let rebased =
            rebase_queue_maintenance_target(file, source, expected_current, content, &current)?;
        match apply(&current, &rebased) {
            Ok(true) => return Ok(rebased),
            Ok(false) => anyhow::bail!("{source}: attached Lazily write was not applied"),
            Err(err)
                if attempt < QUEUE_MAINTENANCE_CAS_ATTEMPTS
                    && queue_maintenance_cas_is_retryable(&err) =>
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_cas_retry file={} source={} attempt={}/{} expected_hash={} attempted_hash={} recovery=retry_crdt_merge",
                        file.display(),
                        source,
                        attempt,
                        QUEUE_MAINTENANCE_CAS_ATTEMPTS,
                        agent_doc_hash::content_hash(&current),
                        agent_doc_hash::content_hash(&rebased),
                    ),
                );
                current = observe()?;
            }
            Err(err) => return Err(err),
        }
    }
    unreachable!("the bounded queue-maintenance CAS loop always returns")
}

pub(crate) fn persist_queue_maintenance_doc(
    file: &Path,
    content: &str,
    expected_current: &str,
    project_root: Option<&Path>,
    source: &str,
) -> Result<String> {
    let _ = project_root;
    // `#qdonestrike-durable`: a not-ready editor head must not silently discard the
    // maintenance plan. Queue maintenance is idempotent and recomputed every
    // preflight, so a missing/pending replica used to mean the auto-strike of
    // already-`agent:done` heads was computed and thrown away on EVERY run — the
    // queue kept re-mirroring the same resolved items forever and never cleared.
    // Drive the bounded model-ensure transition first and re-observe; only bail if
    // the head is still not ready. `ensure_document_model` observes relay state
    // only and never elects a disk/sidecar projection while an editor is attached,
    // so this stays fail-closed against clobbering a live buffer.
    // `#ensurereplicagen`: observe through the hub-owning process, not this
    // CLI process's always-empty local registry.
    let observed = current_text_via_preflight_authority_retrying(file, source)?
        .unwrap_or(agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica);
    let current = match queue_maintenance_head_action(&observed, false) {
        QueueMaintenanceHeadAction::EnsureThenReobserve => {
            // `#ensurewindowsize`: log the outcome on BOTH paths. This log used to
            // sit after the `?`, so the failure case — the only case anyone needs
            // to debug — recorded no before/after label at all, and the ops.log
            // showed a bare `document_model_ensure_failed` with no queue-side
            // context.
            let ensured = ensure_document_model_via_authority(file, source);
            match &ensured {
                Ok(ensured) => agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_model_ensure source={source} before={} after={} outcome=ready",
                        queue_maintenance_head_label(&observed),
                        queue_maintenance_head_label(ensured),
                    ),
                ),
                Err(err) => agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_model_ensure source={source} before={} outcome=failed error={}",
                        queue_maintenance_head_label(&observed),
                        format!("{err:#}")
                            .replace('\n', " | ")
                            .chars()
                            .take(240)
                            .collect::<String>(),
                    ),
                ),
            }
            // `#px82`: a FAILED ensure used to `?` straight out of here, which
            // discarded the whole recomputed queue on the FIRST failure. The
            // failure is intermittent (observed alternating FAIL/OK across
            // back-to-back preflights), so one attempt is a coin flip that
            // silently disarms the drain. Re-register the replica (`#bn41`) and
            // re-observe within a bounded budget before giving up.
            match ensured {
                Ok(ensured) => ensured,
                Err(first_err) => {
                    ensure_document_model_with_replica_reregistration(file, source, first_err)?
                }
            }
        }
        _ => observed,
    };
    if queue_maintenance_head_action(&current, true) == QueueMaintenanceHeadAction::FailClosed {
        anyhow::bail!("{source}: Lazily editor head is not ready for queue maintenance")
    }
    match current {
        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => {
            // `#qtypeclobber`: queue maintenance is a structural successor, not
            // a whole-document replacement. The editor may publish even a few
            // bytes after preflight captured `expected_current`; rebase the
            // candidate over that exact live cut, then CAS the rebased cut.
            // `#ensurereplicagen`: the write is the symmetric half of the read
            // fix above and must land in the hub-owning process too.
            // `apply_cp_write_for_file` is process-local: with no hub in this
            // short-lived CLI it recovers one from the durable `.yrs` projection
            // and compare-and-swaps against THAT. The projection is the
            // last-known canonical, so once it lags the controller's live model
            // the CAS fails with a *stable* hash mismatch — identical
            // expected/current hashes on every retry, `recovery=retry_crdt_merge`
            // that no retry can ever satisfy. Route through the controller, whose
            // model is the real current.
            let embedded = agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file);
            let rebased = persist_attached_queue_maintenance_with_retry(
                file,
                content,
                expected_current,
                source,
                text,
                |expected, target| {
                    let write = if embedded {
                        agent_doc_crdt_relay_io::apply_cp_write_for_file(
                            file, expected, target, source,
                        )?
                    } else {
                        agent_doc_controller_io::project_controller::apply_cp_write_via_controller_model_for_doc(
                            file, expected, target, source,
                        )?
                    };
                    Ok(write.is_some())
                },
                || {
                    let observed = current_text_via_preflight_authority_retrying(file, source)?
                        .unwrap_or(
                            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
                        );
                    match observed {
                        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => Ok(text),
                        other => anyhow::bail!(
                            "{source}: Lazily head became {} during queue-maintenance CAS retry",
                            queue_maintenance_head_label(&other)
                        ),
                    }
                },
            )?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "write_authority action=compare_and_swap authority=lazily surface=queue_maintenance source={source} len={}",
                    rebased.len()
                ),
            );
            Ok(rebased)
        }
        agent_doc_crdt_relay_io::CurrentText::Detached => {
            // A detached relay observation is not permission to replace a file
            // behind an IDE whose durable process/file liveness is still
            // present. That stamp-only rewrite is the direct cause of
            // JetBrains File Cache Conflict dialogs.
            anyhow::ensure!(
                !agent_doc_controller_io::project_controller::reliable_sync_editor_live_for_file(
                    file
                ),
                "{source}: refusing detached queue-maintenance disk write while durable editor liveness remains"
            );
            let disk = std::fs::read_to_string(file)?;
            anyhow::ensure!(
                disk == expected_current || disk == content,
                "{source}: disk head advanced during detached queue maintenance"
            );
            if disk != content {
                std::fs::write(file, content)
                    .with_context(|| format!("{source}: failed to write {}", file.display()))?;
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "write_authority action=compare_and_swap authority=lazily surface=queue_maintenance source={source} len={}",
                    content.len()
                ),
            );
            Ok(content.to_string())
        }
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            anyhow::bail!("{source}: Lazily editor head is not ready for queue maintenance")
        }
    }
}

/// Update the recovery baseline after the Lazily compare-and-swap succeeds.
pub(crate) fn adopt_edited_queue_head_into_snapshot(file: &Path, current_content: &str) {
    adopt_queue_region_into_snapshot(file, current_content, |components| {
        components.iter().find(|c| c.name == "queue")
    });
}

/// Adopt one runnable component's region from `current_content` into the
/// recovery baseline. `select` picks the same component in both images.
///
/// GH #378: the same-cycle backlog mirror writes into
/// [`agent_doc_queue::backlog_sync::mirror_queue_component`] (the first
/// non-repeating `agent:run`). Adopting the first `queue` component instead
/// copied an untouched sibling `agent:loop` and left the baseline without the
/// mirrored row, so the closeout committed the backlog add and dropped the
/// queue half from HEAD.
fn adopt_queue_region_into_snapshot(
    file: &Path,
    current_content: &str,
    select: for<'a> fn(
        &'a [agent_doc_element::element::Component],
    ) -> Option<&'a agent_doc_element::element::Component>,
) {
    let snap_now = match agent_doc_snapshot_io::load_document_baseline(file) {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[preflight] queue: adopt-head snapshot load warning (non-fatal): {e}");
            return;
        }
    };
    let Ok(cur_comps) = agent_doc_element::element::parse(current_content) else {
        return;
    };
    let Some(cur_q) = select(&cur_comps) else {
        return;
    };
    let Ok(snap_comps) = agent_doc_element::element::parse(&snap_now) else {
        return;
    };
    let Some(snap_q) = select(&snap_comps) else {
        return;
    };
    let queue_region = &current_content[cur_q.open_start..cur_q.close_end];
    let mut rebuilt = String::with_capacity(snap_now.len() + queue_region.len());
    rebuilt.push_str(&snap_now[..snap_q.open_start]);
    rebuilt.push_str(queue_region);
    rebuilt.push_str(&snap_now[snap_q.close_end..]);
    if rebuilt != snap_now
        && let Err(e) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &rebuilt,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] queue: adopt-head snapshot sync warning (non-fatal): {e}");
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_document::queue_projection::IN_PROGRESS_MARKER;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    /// Preflight admits a document whose open `agent:todo` items carry their
    /// own `[#id]` (the `--todo-add` form) and whose queue cites two of them,
    /// and still refuses a genuinely orphaned backlog row.
    #[test]
    fn enforce_no_shadow_open_backlog_admits_agent_todo_ids() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("sdk.md");
        let mut content = String::from(concat!(
            "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents priority go -->\n",
            "- [#tdsdkdevrename]: rename to sdk-dev\n",
            "- [#tdue427go]: go ahead\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#tdsdkdevrename] Rename the repo\n",
            "- [ ] [#tdue427go] Approve the go-ahead\n",
        ));
        for n in 0..19 {
            content.push_str(&format!("- [ ] [#tditem{n}] Operator action {n}\n"));
        }
        content.push_str(concat!(
            "<!-- /agent:todo -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#live1] Real backlog work\n",
            "<!-- /agent:backlog -->\n",
        ));
        std::fs::write(&doc, &content).unwrap();
        enforce_no_shadow_open_backlog(&doc).expect("todo ids must be admitted");

        content.push_str("\n<!-- parked copy\n- [ ] [#lost1] Drifted out of backlog\n-->\n");
        std::fs::write(&doc, &content).unwrap();
        let err = enforce_no_shadow_open_backlog(&doc)
            .unwrap_err()
            .to_string();
        assert!(err.contains("#lost1"), "{err}");
        assert!(
            !err.contains("#td"),
            "todo ids must never be reported: {err}"
        );
    }

    /// `#qheadannotation`: the cycle contract carries operator annotations on
    /// selected id heads as a typed field (with guidance), and omits both when
    /// the selected heads are canonical.
    #[test]
    fn cycle_contract_surfaces_queue_head_annotations() {
        let selected = vec![
            format!(
                "{IN_PROGRESS_MARKER} do [#sdkrestemitnative]: can the *.h files be generated from the contract as well?"
            ),
            "do [#plain]".to_string(),
        ];
        let annotations = agent_doc_queue::queue_head_annotation::queue_head_annotations(&selected);
        let output = PreflightOutput {
            queue_head_annotation_guidance:
                agent_doc_queue::queue_head_annotation::queue_head_annotation_guidance(&annotations),
            queue_head_annotations: annotations,
            selected_queue_prompts: selected,
            ..Default::default()
        };
        let json = serde_json::to_value(&output).unwrap();
        let listed = json["queue_head_annotations"].as_array().unwrap();
        assert_eq!(listed.len(), 1, "{json}");
        assert_eq!(listed[0]["id"], "sdkrestemitnative");
        assert_eq!(
            listed[0]["annotation_verbatim"],
            "can the *.h files be generated from the contract as well?"
        );
        let guidance = json["queue_head_annotation_guidance"].as_str().unwrap();
        assert!(guidance.contains("> **Operator note:**"), "{guidance}");
        assert!(guidance.contains("THIS turn"), "{guidance}");

        let canonical = PreflightOutput {
            selected_queue_prompts: vec!["do [#plain]".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_value(&canonical).unwrap();
        assert!(json.get("queue_head_annotations").is_none(), "{json}");
        assert!(
            json.get("queue_head_annotation_guidance").is_none(),
            "{json}"
        );
    }

    #[test]
    fn exchange_subagent_dispatch_contract_has_no_queue_lifecycle() {
        let entries = exchange_subagent_dispatch_entries(&[
            "fix the parser".to_string(),
            "add a regression test".to_string(),
        ]);
        let output = PreflightOutput {
            exchange_subagent_dispatch: entries,
            exchange_subagent_dispatch_guidance: Some(
                EXCHANGE_SUBAGENT_DISPATCH_GUIDANCE.to_string(),
            ),
            ..Default::default()
        };
        let json = serde_json::to_value(output).unwrap();
        assert_eq!(
            json["exchange_subagent_dispatch"][0]["item"],
            "fix the parser"
        );
        assert_eq!(
            json["exchange_subagent_dispatch"][1]["item"],
            "add a regression test"
        );
        let guidance = json["exchange_subagent_dispatch_guidance"]
            .as_str()
            .unwrap();
        assert!(guidance.contains("parent retains this response cycle"));
        assert!(guidance.contains("No queue claim/release"));
        assert!(
            guidance.contains(agent_doc_queue::subagent_brief::SUBAGENT_SCRATCH_ISOLATION_RULE)
        );
        assert!(guidance.ends_with(agent_doc_queue::subagent_brief::SUBAGENT_TMUX_ISOLATION_RULE));
    }

    // `#qdonestrike-durable`: a not-ready Lazily head used to discard the whole
    // queue-maintenance plan, so the auto-strike of heads already in `agent:done`
    // was recomputed and thrown away on every preflight and resolved queue items
    // never cleared. Maintenance must now drive the bounded model-ensure and
    // re-observe before failing closed.
    #[test]
    fn queue_maintenance_missing_replica_ensures_before_failing_closed() {
        assert_eq!(
            queue_maintenance_head_action(
                &agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
                false,
            ),
            QueueMaintenanceHeadAction::EnsureThenReobserve,
        );
        assert_eq!(
            queue_maintenance_head_action(
                &agent_doc_crdt_relay_io::CurrentText::EditorSyncPending,
                false,
            ),
            QueueMaintenanceHeadAction::EnsureThenReobserve,
        );
    }

    // The ensure is bounded: a head that is STILL not ready afterwards fails
    // closed rather than looping or clobbering a live editor buffer.
    #[test]
    fn queue_maintenance_still_not_ready_after_ensure_fails_closed() {
        assert_eq!(
            queue_maintenance_head_action(
                &agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
                true,
            ),
            QueueMaintenanceHeadAction::FailClosed,
        );
        assert_eq!(
            queue_maintenance_head_action(
                &agent_doc_crdt_relay_io::CurrentText::EditorSyncPending,
                true,
            ),
            QueueMaintenanceHeadAction::FailClosed,
        );
    }

    // A head that becomes ready after the ensure persists through the normal
    // authority path — CAS against a live editor, disk only when detached.
    #[test]
    fn queue_maintenance_ready_head_persists_through_normal_authority() {
        for already_ensured in [false, true] {
            assert_eq!(
                queue_maintenance_head_action(
                    &agent_doc_crdt_relay_io::CurrentText::Current {
                        text: "doc".to_string(),
                        live_editors: 1,
                        delivery_converged: true,
                        delivery_version: 1,
                        semantics: None,
                        quarantined_replicas: 0,
                    },
                    already_ensured,
                ),
                QueueMaintenanceHeadAction::CompareAndSwap,
            );
            assert_eq!(
                queue_maintenance_head_action(
                    &agent_doc_crdt_relay_io::CurrentText::Detached,
                    already_ensured,
                ),
                QueueMaintenanceHeadAction::DiskWrite,
            );
        }
    }

    /// GH-162: a live editor can advance between queue maintenance's rebase and
    /// CAS. A retryable refusal must trigger a fresh observation and a fresh
    /// rebase so neither the maintenance marker nor the operator's new item is
    /// lost.
    #[test]
    fn queue_maintenance_cas_refusal_reobserves_rebases_and_retries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.md");
        let base = concat!(
            "<!-- agent:queue go -->\n",
            "- active head\n",
            "- existing tail\n",
            "<!-- /agent:queue -->\n",
        );
        let maintenance = concat!(
            "<!-- agent:queue go -->\n",
            "- 🚧 active head\n",
            "- existing tail\n",
            "<!-- /agent:queue -->\n",
        );
        let advanced = concat!(
            "<!-- agent:queue go -->\n",
            "- active head\n",
            "- existing tail\n",
            "- operator-added item\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&file, base).unwrap();

        let mut attempts = Vec::new();
        let mut observations = 0;
        let written = persist_attached_queue_maintenance_with_retry(
            &file,
            maintenance,
            base,
            "test",
            base.to_string(),
            |expected, target| {
                attempts.push((expected.to_string(), target.to_string()));
                if attempts.len() == 1 {
                    anyhow::bail!("CAS refused recovery=retry_crdt_merge")
                }
                Ok(true)
            },
            || {
                observations += 1;
                Ok(advanced.to_string())
            },
        )
        .unwrap();

        assert_eq!(observations, 1);
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].0, base);
        assert_eq!(attempts[1].0, advanced);
        assert!(written.contains("🚧 active head"), "{written}");
        assert!(written.contains("operator-added item"), "{written}");
        assert_eq!(attempts[1].1, written);
    }

    struct TestPreflightMaintenanceWriteEffects;

    static TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS: TestPreflightMaintenanceWriteEffects =
        TestPreflightMaintenanceWriteEffects;

    impl PreflightMaintenanceWriteEffects for TestPreflightMaintenanceWriteEffects {
        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected_current: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current_content: &str,
            target_content: &str,
            _source: &str,
        ) -> Result<()> {
            std::fs::write(file, target_content)?;
            Ok(())
        }
    }

    struct GuardFailingPreflightMaintenanceWriteEffects {
        authority_checks: std::cell::Cell<usize>,
        converge_calls: std::cell::Cell<usize>,
        visible_write_error: String,
        /// Raise the failure from `converge_or_disk_write` instead of the
        /// pre-write authority check.
        ///
        /// `#retaineddeferisnotafailure`: the two are NOT interchangeable. The
        /// pre-write check fails before anything is delivered; the retained
        /// refusal is raised by `guard_visible_delivery_convergence` *after*
        /// `try_editor_converge` already returned true, so the content is in the
        /// editor's CRDT authority and only the snapshot/commit boundary is
        /// waiting. Every existing defer test failed at the earlier site, which
        /// is why the production shape had no coverage at all.
        fail_at_converge: bool,
    }

    impl Default for GuardFailingPreflightMaintenanceWriteEffects {
        fn default() -> Self {
            Self {
                authority_checks: std::cell::Cell::new(0),
                converge_calls: std::cell::Cell::new(0),
                visible_write_error: "failed to resolve editor authority for test document"
                    .to_string(),
                fail_at_converge: false,
            }
        }
    }

    impl GuardFailingPreflightMaintenanceWriteEffects {
        fn with_error(message: &str) -> Self {
            Self {
                visible_write_error: message.to_string(),
                ..Self::default()
            }
        }

        /// Fail the way the live delivery guard does: after the editor converge
        /// has already been accepted.
        fn with_converge_error(message: &str) -> Self {
            Self {
                visible_write_error: message.to_string(),
                fail_at_converge: true,
                ..Self::default()
            }
        }
    }

    impl PreflightMaintenanceWriteEffects for GuardFailingPreflightMaintenanceWriteEffects {
        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected_current: &str,
        ) -> Result<()> {
            self.authority_checks.set(self.authority_checks.get() + 1);
            if self.fail_at_converge {
                return Ok(());
            }
            anyhow::bail!("{}", self.visible_write_error)
        }

        fn converge_or_disk_write(
            &self,
            _file: &Path,
            _current_content: &str,
            _target_content: &str,
            _source: &str,
        ) -> Result<()> {
            self.converge_calls.set(self.converge_calls.get() + 1);
            if self.fail_at_converge {
                anyhow::bail!("{}", self.visible_write_error)
            }
            Ok(())
        }
    }

    struct TestQueueConsumeWriteEffects;

    static TEST_QUEUE_CONSUME_WRITE_EFFECTS: TestQueueConsumeWriteEffects =
        TestQueueConsumeWriteEffects;

    impl agent_doc_queue_io::queue_consume::QueueConsumeWriteEffects for TestQueueConsumeWriteEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            std::fs::write(file, content)?;
            Ok(())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            std::fs::write(file, target_content)?;
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestPreflightCycleCompletionEffects {
        repair_calls: std::cell::Cell<usize>,
        repair_outcome: Option<agent_doc_turn::repair::RepairOutcome>,
        commit_calls: std::cell::Cell<usize>,
        recovery_calls: std::cell::Cell<usize>,
        gate_reads: std::cell::Cell<usize>,
        retained_document_write: std::cell::Cell<bool>,
        recovery_succeeds: bool,
        /// The `Satisfied` verdict. `#retainedclearreactive`: settling is a
        /// consequence of *reading* the derived verdict — the controller's
        /// per-document effect is subscribed to the same cell — so the stub
        /// clears on read rather than exposing a `settle_*` companion.
        settle_satisfies: bool,
        session_interruption: Option<String>,
        session_interruption_calls: std::cell::Cell<usize>,
    }

    impl PreflightCycleCompletionEffects for TestPreflightCycleCompletionEffects {
        fn repair(&self, _file: &Path) -> Result<agent_doc_turn::repair::RepairOutcome> {
            self.repair_calls.set(self.repair_calls.get() + 1);
            Ok(self
                .repair_outcome
                .unwrap_or(agent_doc_turn::repair::RepairOutcome::Noop))
        }

        fn commit(&self, _file: &Path) -> Result<bool> {
            self.commit_calls.set(self.commit_calls.get() + 1);
            Ok(false)
        }

        fn recover_retained_document_write(&self, _file: &Path) -> Result<bool> {
            self.recovery_calls.set(self.recovery_calls.get() + 1);
            if self.recovery_succeeds {
                self.retained_document_write.set(false);
            }
            Ok(self.recovery_succeeds)
        }

        fn retained_document_write(&self, _file: &Path) -> bool {
            self.gate_reads.set(self.gate_reads.get() + 1);
            let blocking = self.retained_document_write.get();
            if self.settle_satisfies {
                // The subscribed clear fires off this read.
                self.retained_document_write.set(false);
                return false;
            }
            blocking
        }

        fn session_interruption(&self, _file: &Path) -> Result<Option<String>> {
            self.session_interruption_calls
                .set(self.session_interruption_calls.get() + 1);
            Ok(self.session_interruption.clone())
        }

        fn detect_bypassed_response_write(&self, _file: &Path) -> Result<Option<String>> {
            Ok(None)
        }
    }

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@test.com"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .ok();

        dir
    }

    #[test]
    fn links_cache_dir_creates_directory() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        let cache = links_cache_dir(&doc);
        assert!(cache.is_some());
        let cache_path = cache.unwrap();
        assert!(cache_path.exists());
        assert!(cache_path.ends_with("links_cache"));
    }

    #[test]
    fn preflight_claims_read_and_truncated() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        let log_path = dir.path().join(".agent-doc/claims.log");
        std::fs::write(&log_path, "claim A\nclaim B\n").unwrap();

        let claims = read_and_truncate_claims(&doc);
        assert_eq!(claims, vec!["claim A", "claim B"]);

        let after = std::fs::read_to_string(&log_path).unwrap();
        assert!(after.is_empty(), "claims log should be empty after read");
    }

    #[test]
    fn preflight_no_claims_log_returns_empty() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        let claims = read_and_truncate_claims(&doc);
        assert!(claims.is_empty());
    }

    fn write_optverify_doc(dir: &TempDir, predicate_annotation: &str) -> PathBuf {
        let doc = dir.path().join("session.md");
        let file_content = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Review\n\n",
                "<!-- agent:review -->\n",
                "- [/] [#saev] early receipt live verify {}\n",
                "<!-- /agent:review -->\n"
            ),
            predicate_annotation
        );
        std::fs::write(&doc, &file_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &file_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        doc
    }

    fn write_ops_log(dir: &TempDir, body: &str) {
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), body).unwrap();
    }

    /// GH #325 fixture: a project with a session document at
    /// `tasks/a/target.md` whose `agent:done` marker carries `archive_attr`.
    fn gh325_project(archive_attr: &str) -> (TempDir, std::path::PathBuf, String) {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/a")).unwrap();
        let doc = dir.path().join("tasks/a/target.md");
        let content = format!(
            "<!-- agent:done {archive_attr} -->\n- 2026-10-01 [#inline1] Inline entry.\n<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        (dir, doc, content)
    }

    const GH325_ARCHIVE: &str = "# Agent Doc Completed Work\n\n- 2026-10-19 [#archived1] First archived item\n- 2026-10-20 [#archived2] Second\n";

    #[test]
    fn collect_agent_done_ids_reads_project_relative_archive_attr() {
        let (dir, doc, content) = gh325_project("archive=tasks/done-archive.done.md");
        std::fs::write(dir.path().join("tasks/done-archive.done.md"), GH325_ARCHIVE).unwrap();
        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        for id in ["inline1", "archived1", "archived2"] {
            assert!(ids.contains(id), "{id} missing: {ids:?}");
        }
    }

    /// GH #325: a bare `archive` (the documented default) is `<stem>.done.md`
    /// beside the document. The old reader joined the empty value onto the
    /// project root, read the root DIRECTORY, failed silently, and returned no
    /// archived ids or texts.
    #[test]
    fn collect_agent_done_ids_and_texts_read_bare_archive_beside_document() {
        let (dir, doc, content) = gh325_project("archive");
        std::fs::write(dir.path().join("tasks/a/target.done.md"), GH325_ARCHIVE).unwrap();
        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        assert!(ids.contains("archived1"), "{ids:?}");
        assert!(ids.contains("archived2"), "{ids:?}");
        assert!(ids.contains("inline1"), "{ids:?}");
        let texts = collect_agent_done_texts(&doc, &content).unwrap();
        assert_eq!(
            texts.get("archived1").map(String::as_str),
            Some("First archived item"),
            "{texts:?}"
        );
        assert_eq!(
            texts.get("inline1").map(String::as_str),
            Some("Inline entry."),
            "{texts:?}"
        );
    }

    /// GH #325: `document:` / `project:` anchors resolve like the writer's.
    /// The old reader looked for the literal `<root>/document:x.done.md`.
    #[test]
    fn collect_agent_done_ids_honours_document_and_project_anchors() {
        let (dir, doc, content) = gh325_project("archive=document:../history.done.md");
        std::fs::write(dir.path().join("tasks/history.done.md"), GH325_ARCHIVE).unwrap();
        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        assert!(ids.contains("archived1"), "document: anchor: {ids:?}");
        let texts = collect_agent_done_texts(&doc, &content).unwrap();
        assert!(
            texts.contains_key("archived2"),
            "document: anchor: {texts:?}"
        );

        let (dir, doc, content) = gh325_project("archive=project:tasks/history.done.md");
        std::fs::write(dir.path().join("tasks/history.done.md"), GH325_ARCHIVE).unwrap();
        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        assert!(ids.contains("archived1"), "project: anchor: {ids:?}");
    }

    /// The preflight readers take the same `<stem>.done.md` sibling union
    /// `archived_tracked_ids` reads, even with no `archive=` declared.
    #[test]
    fn collect_agent_done_ids_reads_sibling_archive_without_declared_target() {
        let (dir, doc, content) = gh325_project("");
        std::fs::write(dir.path().join("tasks/a/target.done.md"), GH325_ARCHIVE).unwrap();
        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        assert!(ids.contains("archived1"), "{ids:?}");
    }

    /// Spec: invalid archive targets fail closed instead of being ignored.
    #[test]
    fn collect_agent_done_ids_fails_closed_on_invalid_archive_target() {
        let (_dir, doc, content) = gh325_project("archive=tasks/not-an-archive.md");
        assert!(collect_agent_done_ids(&doc, &content).is_err());
        assert!(collect_agent_done_texts(&doc, &content).is_err());
        let (_dir, doc, content) = gh325_project("archive=project:../escape.done.md");
        assert!(collect_agent_done_ids(&doc, &content).is_err());
    }

    /// GH #325: 0.35.521–0.35.523 wrote `<doc dir>/<project-relative path>`.
    /// That file is never read as history, and preflight names it once.
    #[test]
    fn misplaced_legacy_done_archive_is_not_read_and_is_advised_once() {
        let (dir, doc, content) = gh325_project("archive=tasks/a/target.done.md");
        std::fs::write(dir.path().join("tasks/a/target.done.md"), GH325_ARCHIVE).unwrap();
        let legacy = dir.path().join("tasks/a/tasks/a/target.done.md");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "- 2026-10-19 [#legacyonly] Misplaced.\n").unwrap();

        let ids = collect_agent_done_ids(&doc, &content).unwrap();
        assert!(ids.contains("archived1"), "{ids:?}");
        assert!(!ids.contains("legacyonly"), "{ids:?}");

        let advisories = dir.path().join(".agent-doc/advisories");
        advise_misplaced_legacy_done_archive(&doc, &content);
        let markers: Vec<_> = std::fs::read_dir(&advisories)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(markers.len(), 1, "{markers:?}");
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert_eq!(
            ops.matches("legacy_done_archive_advisory").count(),
            1,
            "{ops}"
        );

        advise_misplaced_legacy_done_archive(&doc, &content);
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert_eq!(
            ops.matches("legacy_done_archive_advisory").count(),
            1,
            "the advisory is one-time: {ops}"
        );
        assert!(
            legacy.is_file(),
            "the advisory never moves or deletes the file"
        );
    }

    #[test]
    fn no_legacy_advisory_without_a_misplaced_archive() {
        let (dir, doc, content) = gh325_project("archive");
        std::fs::write(dir.path().join("tasks/a/target.done.md"), GH325_ARCHIVE).unwrap();
        advise_misplaced_legacy_done_archive(&doc, &content);
        assert!(!dir.path().join(".agent-doc/advisories").exists());
    }

    /// GH #161: a prompt preset is a reusable command. An archived done row
    /// with the same id must not strike the live invocation or make the claim
    /// command advertised by preflight fail.
    #[test]
    fn archived_done_id_does_not_complete_registered_preset_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "tasks/session.done.md";
        let archive = dir.path().join(archive_rel);
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(&archive, "- [x] [#upgrade] Older completed upgrade\n").unwrap();
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "queue_active: true\n",
                "prompt_presets:\n",
                "  '#upgrade': Upgrade the sample application.\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — test\n\nAnswered.\n",
                "<!-- agent:boundary:committed -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue preset=\"#subagents\" go -->\n",
                "- #upgrade\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:done archive=\"{}\" -->\n",
                "<!-- /agent:done -->\n"
            ),
            archive_rel
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_prompts, vec!["#upgrade".to_string()]);
        let authoritative = state.authoritative_content.as_deref().unwrap();
        assert!(authoritative.contains("- 📥 #upgrade"), "{authoritative}");
        assert_eq!(
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                &doc,
                authoritative,
            )
            .unwrap(),
            vec!["#upgrade".to_string()],
        );
        agent_doc_queue_io::queue_claim::claim(&doc, "#upgrade", "subagent:test", 600).unwrap();
    }

    #[test]
    fn archived_done_id_does_not_complete_project_preset_head() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#upgrade': Upgrade the sample application.\n---\n",
        )
        .unwrap();
        let archive_rel = "tasks/session.done.md";
        let archive = dir.path().join(archive_rel);
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::write(&archive, "- [x] [#upgrade] Older completed upgrade\n").unwrap();
        let doc = dir.path().join("session.md");
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
                "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n- #upgrade\n<!-- /agent:queue -->\n\n",
                "<!-- agent:done archive=\"{}\" -->\n<!-- /agent:done -->\n"
            ),
            archive_rel
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_prompts, vec!["#upgrade".to_string()]);
        let authoritative = state.authoritative_content.as_deref().unwrap();
        assert!(authoritative.contains("#upgrade"), "{authoritative}");
        assert!(!authoritative.contains("~~#upgrade~~"), "{authoritative}");
    }

    /// GH #381 repro: `.agent-doc/project.md` declares `'#review'`; the session
    /// document has an empty `agent:backlog` and a go queue whose only live head
    /// is `- #review`, and never carries the preset itself. Preflight reported
    /// `queue_drainable_head_count: 0` and a `tracked_work_missing_todo_backlink`
    /// signal naming `#review`. The head is a preset invocation: it drains and
    /// needs neither a backlog item nor a Todo backlink.
    #[test]
    fn project_preset_queue_head_drains_without_backlog_item_or_todo_backlink() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/project.md"),
            "---\npresets:\n  '#review': Review the change.\n---\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - test\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- #review\n<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let inspected = inspect_queue_state(&doc, None).unwrap();
        assert_eq!(inspected.queue_drainable_head_count, 1);
        assert!(inspected.queue_continuation_required);
        assert!(inspected.queue_supervisor_drainable);

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_prompts, vec!["#review".to_string()]);
        assert_eq!(state.queue_drainable_head_count, 1);
        assert!(state.queue_continuation_required);

        let current = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !current.contains("Review the change."),
            "inherited presets are never copied into the session document:\n{current}"
        );
        let signals = todo_signals_for_document(&doc, &current);
        assert!(
            signals
                .iter()
                .all(|signal| signal.todo != "review" && !signal.action.contains("#review")),
            "a project-preset head owes no Todo backlink: {signals:?}"
        );

        // Control: a head naming no preset and no tracked work is still flagged.
        let orphan = current.replace("#review", "#ghostid");
        let signals = todo_signals_for_document(&doc, &orphan);
        assert!(
            signals.iter().any(|signal| signal.todo == "ghostid"),
            "an orphan id head keeps its Todo-backlink signal: {signals:?}\n{orphan}"
        );
    }

    /// GH #161: downstream dispatch consumes maintenance's settled projection,
    /// so a genuinely completed head cannot remain advertised from the earlier
    /// diff cut after maintenance strikes it.
    #[test]
    fn queue_maintenance_returns_post_strike_authority_for_dispatch() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — test\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue preset=\"#subagents\" go -->\n",
            "- do [#finished]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#finished] Completed earlier\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert_eq!(
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                &doc, content,
            )
            .unwrap(),
            vec!["do [#finished]".to_string()],
        );

        let state = run_queue_maintenance(&doc, None).unwrap();
        let authoritative = state.authoritative_content.as_deref().unwrap();
        assert!(
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                &doc,
                authoritative,
            )
            .unwrap()
            .is_empty(),
            "settled queue projection must not advertise the struck head: {authoritative}",
        );
    }

    // `#px82` — the editor-authority failure is intermittent, so queue
    // maintenance must re-observe instead of discarding the recomputed queue on
    // the first `editor_attached_model_missing`.
    #[test]
    fn queue_authority_observation_retries_transient_editor_authority_status() {
        let file = Path::new("/tmp/agent-doc-px82-retry.md");
        let mut calls = 0usize;
        let observed = observe_current_text_with_bounded_retry(file, "test", 3, |_| {
            calls += 1;
            if calls < 3 {
                Ok(Some(
                    agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
                ))
            } else {
                Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
                    text: "recovered".to_string(),
                    live_editors: 1,
                    delivery_converged: true,
                    delivery_version: 1,
                    semantics: None,
                    quarantined_replicas: 0,
                }))
            }
        });
        assert_eq!(calls, 3, "expected the transient status to be re-observed");
        match observed {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => {
                assert_eq!(text, "recovered");
            }
            other => panic!("expected a recovered current observation, got {other:?}"),
        }
    }

    // A controller read timeout is another transient authority observation. The
    // live `haiven.md` failure reached attempt 1/3, but `Error` bypassed the typed
    // retry transition and discarded an already-computed two-row queue strike.
    #[test]
    fn queue_authority_observation_retries_controller_error_then_current() {
        let file = Path::new("/tmp/agent-doc-px82-controller-error.md");
        let mut calls = 0usize;
        let observed = observe_current_text_with_bounded_retry(file, "test", 3, |_| {
            calls += 1;
            if calls == 1 {
                anyhow::bail!("timed out after 5.0s waiting for project controller response")
            }
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
                text: "recovered".to_string(),
                live_editors: 1,
                delivery_converged: true,
                delivery_version: 2,
                semantics: None,
                quarantined_replicas: 0,
            }))
        });
        assert_eq!(calls, 2, "the controller error must consume a retry");
        match observed {
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => {
                assert_eq!(text, "recovered");
            }
            other => panic!("expected a recovered current observation, got {other:?}"),
        }
    }

    // A settled status must not pay the retry cost — only the transient
    // editor-authority statuses and observation errors are re-observed.
    #[test]
    fn queue_authority_observation_returns_immediately_on_settled_status() {
        let file = Path::new("/tmp/agent-doc-px82-settled.md");
        let mut calls = 0usize;
        let observed = observe_current_text_with_bounded_retry(file, "test", 3, |_| {
            calls += 1;
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached))
        });
        assert_eq!(calls, 1, "a settled status must not be retried");
        assert!(matches!(
            observed,
            Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached))
        ));
    }

    // The retry is bounded: a permanently missing replica still surfaces the
    // transient status after the attempt budget instead of looping forever.
    #[test]
    fn queue_authority_observation_gives_up_after_attempt_budget() {
        let file = Path::new("/tmp/agent-doc-px82-budget.md");
        let mut calls = 0usize;
        let observed = observe_current_text_with_bounded_retry(file, "test", 2, |_| {
            calls += 1;
            Ok(Some(
                agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
            ))
        });
        assert_eq!(calls, 2, "expected exactly the configured attempt budget");
        assert!(matches!(
            observed,
            Ok(Some(
                agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
            ))
        ));
    }

    #[test]
    fn queue_authority_status_token_names_each_observation() {
        assert_eq!(
            current_text_status_token(&Ok(Some(
                agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
            ))),
            "editor_attached_model_missing"
        );
        assert_eq!(
            current_text_status_token(&Ok(Some(
                agent_doc_crdt_relay_io::CurrentText::EditorSyncPending
            ))),
            "editor_sync_pending"
        );
        assert_eq!(current_text_status_token(&Ok(None)), "none");
        assert!(
            current_text_status_token(&Err(anyhow::anyhow!("boom"))).starts_with("error:"),
            "an observation error must still be instrumented"
        );
    }

    #[test]
    fn queue_authority_reobserves_native_save_until_editor_cut_converges() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- is the integration runnable?\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let saved = baseline
            .replace(
                "<!-- agent:queue go -->",
                "<!-- agent:queue go priority -->",
            )
            .replace("<!-- agent:backlog -->", "<!-- agent:backlog priority -->");
        std::fs::write(&doc, &saved).unwrap();

        let mut calls = 0usize;
        let observed = observe_queue_authority_after_native_save_with_bounded_retry_and_adopt(
            &doc,
            "test",
            3,
            |_| {
                calls += 1;
                let text = if calls < 3 { baseline } else { &saved };
                Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current {
                    text: text.to_string(),
                    live_editors: 1,
                    delivery_converged: true,
                    delivery_version: calls as u64,
                    semantics: None,
                    quarantined_replicas: 0,
                }))
            },
            |_, authority, disk, _| {
                assert_eq!(authority, baseline);
                assert_eq!(disk, saved);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(calls, 3, "the stale editor cut must be re-observed");
        match observed {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => {
                assert_eq!(
                    text, saved,
                    "the converged editor cut retains both priorities"
                );
            }
            other => panic!("expected converged editor authority, got {other:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            saved,
            "preflight reconciliation must never rewrite the native save"
        );
    }

    fn component_body(content: &str, name: &str) -> String {
        let comps = agent_doc_element::element::parse(content).unwrap();
        let comp = comps.iter().find(|c| c.name == name).unwrap();
        comp.content(content).to_string()
    }

    fn backlog_id_for_text(content: &str, text: &str) -> String {
        let body = component_body(content, "backlog");
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(&body);
        items
            .into_iter()
            .find(|item| strip_in_progress_marker(&item.text) == text)
            .map(|item| item.id)
            .unwrap()
    }

    #[test]
    fn inspect_queue_state_simulates_activation_without_persisting() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let snapshot_before = agent_doc_snapshot_io::load_document_baseline(&doc).unwrap();

        let state = inspect_queue_state(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_prompts, vec!["do [#alpha]".to_string()]);
        assert_eq!(state.queue_drainable_head_count, 1);
        assert!(state.queue_continuation_required);
        assert!(state.queue_supervisor_drainable);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc).unwrap(),
            snapshot_before
        );
    }

    /// `#presetargdedup` (live, agent-doc-bugs.md 2026-09-30): after the first
    /// `#gh-fix <url>` head was answered, its siblings were marked `⏭️` as if
    /// `gh-fix` were a dispatched-but-unresolved tracked item, and the drain
    /// jumped to `release + publish`. A preset-only id never joins the skip set.
    #[test]
    fn run_queue_maintenance_never_skips_sibling_preset_invocations() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#gh-fix': fix then close\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- ~~#gh-fix https://x/issues/68~~\n",
            "- #gh-fix https://x/issues/69\n",
            "- #gh-fix https://x/issues/70\n",
            "- release + publish\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_active_queue_heads(
            &doc,
            &["#gh-fix https://x/issues/68".to_string()],
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(content),
            Some(content),
            &agent_doc_hash::content_hash("### Re: 68\n\nfixed\n"),
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "committed", Some(content), Some(content))
            .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            state.selected_queue_prompts,
            vec!["#gh-fix https://x/issues/69".to_string()],
            "the next sibling invocation must be selected:\n{updated}"
        );
        assert!(
            !updated.contains("⏭️"),
            "no sibling may be skipped:\n{updated}"
        );
        let persisted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(
            persisted.skipped_queue_head_ids.is_empty(),
            "{:?}",
            persisted.skipped_queue_head_ids
        );
    }

    /// `#releaseskipblame` (live, agent-doc-bugs.md 2026-10-01): the prior turn
    /// ran a `#gh-fix <url>` preset head, but `active_queue_heads` also held the
    /// operator's later `:pin: #release`. Skipping preset-only ids inside the
    /// search walked past the head that ran and blamed `release`, which was then
    /// marked `⏭️` every cycle.
    #[test]
    fn prior_dispatched_queue_id_never_blames_a_head_that_did_not_run() {
        let preset_only: std::collections::HashSet<String> = ["gh-fix".to_string()].into();
        let none = std::collections::HashSet::new();
        let heads = vec![
            "#gh-fix https://x/issues/77".to_string(),
            "🚧 #gh-fix https://x/issues/77".to_string(),
            ":pin: #release".to_string(),
        ];
        assert_eq!(
            prior_dispatched_queue_id(&heads, &none, &preset_only, &none),
            None
        );
        // No `🚧` recorded: the first head ran, and it is a preset.
        assert_eq!(
            prior_dispatched_queue_id(&heads[..1], &none, &preset_only, &none),
            None
        );
        // A free-text head that ran dispatches no tracked id either.
        let free = vec![
            "look at the old issues".to_string(),
            ":pin: #release".to_string(),
        ];
        assert_eq!(
            prior_dispatched_queue_id(&free, &none, &preset_only, &none),
            None
        );
        // A `⏭️` head is not the running one.
        let skipped = vec!["⏭️ :pin: #release".to_string(), "🚧 do [#hmw9]".to_string()];
        assert_eq!(
            prior_dispatched_queue_id(&skipped, &none, &preset_only, &none),
            Some("hmw9".to_string())
        );
        // A tracked head that ran and came back is still reported.
        let tracked = vec!["do [#sy71]".to_string(), "do [#hmw9]".to_string()];
        assert_eq!(
            prior_dispatched_queue_id(&tracked, &none, &preset_only, &none),
            Some("sy71".to_string())
        );
        // A head already skipped is passed over, as selection passes over it.
        let carried: std::collections::HashSet<String> = ["sy71".to_string()].into();
        assert_eq!(
            prior_dispatched_queue_id(&tracked, &carried, &preset_only, &none),
            Some("hmw9".to_string())
        );
    }

    /// End to end: after a `#gh-fix` turn, the operator's pinned release head
    /// stays selectable and carries no `⏭️`.
    #[test]
    fn run_queue_maintenance_does_not_skip_a_pinned_head_after_a_preset_turn() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "prompt_presets:\n",
            "  '#gh-fix': fix then close\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- ~~#gh-fix https://x/issues/77~~\n",
            "- #gh-fix https://x/issues/78\n",
            "- :pin: #release\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_active_queue_heads(
            &doc,
            &[
                "🚧 #gh-fix https://x/issues/77".to_string(),
                "#gh-fix https://x/issues/78".to_string(),
                ":pin: #release".to_string(),
            ],
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(content),
            Some(content),
            &agent_doc_hash::content_hash("### Re: 77\n\nfixed\n"),
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "committed", Some(content), Some(content))
            .unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("⏭️"),
            "the release head must not be skipped:\n{updated}"
        );
        let persisted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(
            persisted.skipped_queue_head_ids.is_empty(),
            "{:?}",
            persisted.skipped_queue_head_ids
        );
    }

    /// `#claimskipmarker` / `#queueawaitop` fixture: the haiven-dev `tasks/sdk.md`
    /// shape of 2026-10-11 — a subagent-claimed head, a head whose subagent
    /// finished and tagged its item `[operator-verify]`, and a plain open head —
    /// after a prior cycle that hosted a turn with the claimed head's `🚧`
    /// recorded among its active heads.
    fn sdk_queue_lifecycle_fixture(claim_item: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — test\n\nDispatched subagents.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#verify]\n",
            "- do [#claimed]\n",
            "- do [#plain]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#verify] [operator-verify] subagent committed locally; operator: approve push\n",
            "- [ ] [#claimed] a subagent is still working this\n",
            "- [ ] [#plain] a real open task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, claim_item, "subagent:worker", 3600).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_active_queue_heads(
            &doc,
            // The coordinator's turn only dispatched: no inline head ran, and
            // `#plain` was queued after it.
            &["do [#verify]".to_string(), "🚧 do [#claimed]".to_string()],
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(content),
            Some(content),
            &agent_doc_hash::content_hash("### Re: dispatch\n\nsubagents running\n"),
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "committed", Some(content), Some(content))
            .unwrap();
        (dir, doc)
    }

    fn queue_line<'a>(doc: &'a str, id: &str) -> &'a str {
        let needle = format!("[#{id}]");
        doc.lines()
            .find(|line| line.starts_with("- ") && line.contains(&needle) && !line.contains("[ ]"))
            .unwrap_or_else(|| panic!("no queue line for #{id}:\n{doc}"))
    }

    /// `#claimskipmarker` + `#queueawaitop` (live, haiven-dev `tasks/sdk.md`
    /// 2026-10-11): the claimed head was the `🚧` row the prior cycle recorded, so
    /// it was blamed as "dispatched and came back unconsumed" and the next cycle
    /// stamped `⏭️` over the claim; the `[operator-verify]` head whose subagent
    /// had finished showed no marker at all and read as unstarted. Now the claim
    /// keeps its truthful `🟡` claimed/dispatching marker, the finished head
    /// shows `⏳`, and nothing is skipped — across
    /// repeated maintenance passes.
    #[test]
    fn run_queue_maintenance_projects_claim_and_awaiting_operator_lifecycle() {
        let (_dir, doc) = sdk_queue_lifecycle_fixture("#claimed");
        for pass in 0..2 {
            let state = run_queue_maintenance(&doc, None).unwrap();
            let updated = std::fs::read_to_string(&doc).unwrap();
            assert_eq!(
                queue_line(&updated, "claimed"),
                "- 🟡 [#subagent:worker#1] do [#claimed]",
                "pass {pass}: a live claim keeps its in-flight marker:\n{updated}"
            );
            assert_eq!(
                queue_line(&updated, "verify"),
                "- ⏳ do [#verify]",
                "pass {pass}: a head waiting on the operator is not unstarted:\n{updated}"
            );
            assert!(
                !updated.contains('\u{23ed}'),
                "pass {pass}: no head may be skipped:\n{updated}"
            );
            assert_eq!(
                state.selected_queue_prompts,
                vec!["do [#plain]".to_string()],
                "pass {pass}: selection runs the real open head inline:\n{updated}"
            );
            assert!(
                !state
                    .warnings
                    .iter()
                    .any(|w| w.code == "queue_claim_held_after_report"),
                "pass {pass}: {:?}",
                state.warnings
            );
            let persisted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
            assert!(
                persisted.skipped_queue_head_ids.is_empty(),
                "pass {pass}: {:?}",
                persisted.skipped_queue_head_ids
            );
        }

        // Releasing the claim (the subagent reported) drops `🚧` on the next pass;
        // the head returns to the open queue rather than vanishing.
        agent_doc_queue_io::queue_claim::release(&doc, "#claimed").unwrap();
        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !queue_line(&updated, "claimed").contains('\u{23f3}'),
            "an open (not operator-verify) item never shows awaiting:\n{updated}"
        );
        assert_eq!(queue_line(&updated, "verify"), "- ⏳ do [#verify]");
    }

    /// `#queueawaitop`: a claim held on a head whose item already waits on the
    /// operator keeps `🟡` (the claim is real, but no worker heartbeat/report
    /// receipt exists) and preflight names it, so the
    /// coordinator releases it instead of leaving the queue showing work in
    /// flight that finished.
    #[test]
    fn run_queue_maintenance_warns_when_a_reported_head_keeps_its_claim() {
        let (_dir, doc) = sdk_queue_lifecycle_fixture("#verify");
        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            queue_line(&updated, "verify"),
            "- 🟡 [#subagent:worker#1] do [#verify]",
            "{updated}"
        );
        let warning = state
            .warnings
            .iter()
            .find(|w| w.code == "queue_claim_held_after_report")
            .unwrap_or_else(|| panic!("missing warning: {:?}", state.warnings));
        assert!(warning.message.contains("#verify"), "{}", warning.message);
        assert!(
            warning.message.contains("agent-doc queue release"),
            "{}",
            warning.message
        );
    }

    /// `#claimskipmarker`: claimed and awaiting-operator heads are never the
    /// blamed dispatch, whether found by their `🚧` or by the first-head
    /// fallback.
    #[test]
    fn prior_dispatched_queue_id_exempts_claimed_and_awaiting_heads() {
        let none = std::collections::HashSet::new();
        let heads = vec![
            "do [#verify]".to_string(),
            "🚧 do [#claimed]".to_string(),
            "do [#plain]".to_string(),
        ];
        assert_eq!(
            prior_dispatched_queue_id(&heads, &none, &none, &none),
            Some("claimed".to_string()),
            "without exemptions the claim's marker is blamed (the 2026-10-11 bug)"
        );
        let exempt: std::collections::HashSet<String> =
            ["verify".to_string(), "claimed".to_string()].into();
        assert_eq!(
            prior_dispatched_queue_id(&heads, &none, &none, &exempt),
            Some("plain".to_string())
        );
        let inline = vec!["🚧 do [#claimed]".to_string(), "🚧 do [#plain]".to_string()];
        assert_eq!(
            prior_dispatched_queue_id(&inline, &none, &none, &exempt),
            Some("plain".to_string()),
            "the inline head's 🚧 is found past a claimed head's 🚧"
        );
    }

    #[test]
    fn run_queue_maintenance_skips_stalled_head_and_advances() {
        // #queueskip: a head dispatched last cycle that came back unconsumed is
        // skipped this cycle; selection advances to the next non-dependent
        // drainable head, the skipped head stays queued with a `⏭️` marker (never
        // struck/dropped), and its id is persisted in cycle-state.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#sy71]\n",
            "- do [#hmw9]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#hmw9] a real open task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Simulate the PRIOR cycle: it dispatched #sy71 (first head), CAPTURED a
        // response, and committed WITHOUT consuming it (no reap/done recorded).
        // The capture is load-bearing, not decoration: a committed cycle with no
        // capture is preflight's own step-2 bookkeeping commit, which
        // `prior_cycle_hosted_a_turn` must reject (`#adpreflightreentryskip`).
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(content),
            Some(content),
            &agent_doc_hash::content_hash("### Re: prior turn\n\nunconsumed\n"),
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "committed", Some(content), Some(content))
            .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        // Selection advanced past #sy71 to the drainable #hmw9.
        assert_eq!(
            state.selected_queue_prompts,
            vec!["do [#hmw9]".to_string()],
            "selection must advance past the stalled #sy71:\n{updated}"
        );

        // #sy71 is skipped (persisted) and carries the ⏭️ marker but stays a live
        // (unstruck) Prompt.
        let persisted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(persisted.skipped_queue_head_ids, vec!["sy71".to_string()]);
        let comps = agent_doc_element::element::parse(&updated).unwrap();
        let queue = comps.iter().find(|c| c.name == "queue").unwrap();
        let entries = agent_doc_queue::document_queue::parse(queue.content(&updated)).unwrap();
        let sy71 = entries
            .iter()
            .find(|e| {
                agent_doc_queue::queue_projection::queue_entry_do_id(e).as_deref() == Some("sy71")
            })
            .expect("sy71 present");
        assert!(
            matches!(sy71, agent_doc_queue::document_queue::QueueEntry::Prompt(_)),
            "#sy71 stays a live prompt (not struck/dropped): {sy71:?}"
        );
        assert!(
            queue.content(&updated).contains("⏭\u{fe0f}")
                && queue
                    .content(&updated)
                    .lines()
                    .any(|l| l.contains("⏭\u{fe0f}") && l.contains("[#sy71]")),
            "the skipped #sy71 head must carry the ⏭️ marker:\n{updated}"
        );
    }

    /// `#queueblocked` (haiven-dev `tasks/sdk/sdk-demo.md`, 2026-10-27): rows
    /// whose tracked items carry `(stays open: blocked behind #a, #b)` rendered
    /// `📥` like runnable work, and a carried skip painted `⏭️` ("dispatched and
    /// came back unconsumed") on one that was never dispatched. Now each
    /// blocked row shows `⛔` naming only the blockers still open here (not the
    /// PR number `#749` or the id living in another document), is never
    /// selected or skipped, and selection goes to the runnable row.
    #[test]
    fn run_queue_maintenance_marks_blocked_rows_and_never_skips_them() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — test\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- ⏭\u{fe0f} do [#stg]\n",
            "- 📥 do [#sbx]\n",
            "- do [#runnable]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#deploy] Pick how haiven-api #749 reaches SBX.\n",
            "<!-- /agent:todo -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#sbx] Provision SBX. (stays open: blocked behind #template-api-create and the dev deploy, #deploy)\n",
            "- [ ] [#stg] Repeat in STG. (stays open: blocked behind #sbx and a release containing #749)\n",
            "- [ ] [#runnable] a real open task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // The live document carried a skip for #stg from an earlier cycle.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::set_skipped_queue_head_ids(&doc, &["stg".to_string()]).unwrap();
        agent_doc_cycle_state_io::mark_committed(&doc, "committed", Some(content), Some(content))
            .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            queue_line(&updated, "stg"),
            "- ⛔ ⟦blocked by sbx⟧ do [#stg]",
            "{updated}"
        );
        assert_eq!(
            queue_line(&updated, "sbx"),
            "- ⛔ ⟦blocked by deploy⟧ do [#sbx]",
            "{updated}"
        );
        assert_eq!(
            state.selected_queue_prompts,
            vec!["do [#runnable]".to_string()],
            "{updated}"
        );
        let persisted = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert!(
            persisted.skipped_queue_head_ids.is_empty(),
            "a blocked head is never skipped: {:?}",
            persisted.skipped_queue_head_ids
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::drainable_head_count(&updated),
            1,
            "only the runnable row counts as drainable:\n{updated}"
        );
        // The operator's row text and the tracked items are untouched.
        assert!(
            updated.contains("(stays open: blocked behind #sbx and a release containing #749)")
        );

        // Closing the last open blocker unblocks the row on the next pass.
        let unblocked = updated.replace("- [ ] [#deploy]", "- [x] [#deploy]");
        std::fs::write(&doc, &unblocked).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &unblocked,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        // #sbx is the first runnable row again, so this pass selects it.
        assert_eq!(queue_line(&updated, "sbx"), "- 🚧 do [#sbx]", "{updated}");
        assert_eq!(
            state.selected_queue_prompts,
            vec!["do [#sbx]".to_string()],
            "{updated}"
        );
        assert_eq!(
            queue_line(&updated, "stg"),
            "- ⛔ ⟦blocked by sbx⟧ do [#stg]",
            "{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_never_mirrors_backlog_ids_into_a_selected_loop() {
        // `#loopnoinsert`: with `agent:run id=main` drained, queue selection
        // falls through to the `issues` loop. The backlog mirror used to write
        // `do [#fresh]` into that loop, where every later pass restored it as
        // a recurring row (2026-10-26, tasks/agent-doc/agent-doc-bugs.md).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let loop_block = concat!(
            "## Run: Issues\n",
            "<!-- agent:loop id=issues interval=1h priority -->\n",
            "- Watch https://github.com/btakita/agent-doc/issues for new issues.\n",
            "<!-- /agent:loop -->\n",
        );
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "<!-- /agent:exchange -->\n\n",
                "## Queue: Main\n",
                "<!-- agent:run id=\"main\" subagents priority go -->\n",
                "- ~~do [#older]~~\n",
                "<!-- /agent:run -->\n\n",
                "{}\n",
                "<!-- agent:backlog priority queue -->\n",
                "- [ ] [#fresh] one-off work filed by the loop's watch item\n",
                "<!-- /agent:backlog -->\n",
            ),
            loop_block
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let loop_start = updated.find("<!-- agent:loop").unwrap();
        let loop_body = updated[loop_start..]
            .split("<!-- /agent:loop -->")
            .next()
            .unwrap();
        assert!(
            !loop_body.contains("#fresh"),
            "a backlog id never becomes a loop row:\n{updated}"
        );
        // Only the operator's row (with its lifecycle projection) remains.
        assert_eq!(
            loop_body
                .lines()
                .filter(|line| line.starts_with("- "))
                .count(),
            1,
            "{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_keeps_a_claimed_mirror_row_whose_snapshot_was_bare() {
        // `#claimrowtombstone` (live 2026-10-26, tasks/agent-doc/agent-doc-bugs.md
        // 13:09:26Z): the snapshot held the backlog-mirrored head bare
        // (`📥 do [#claimrowmigrate]`); the claim then projected
        // `[#subagent:claimrowmigrate#1]` onto the live row. Tombstone
        // reconciliation read the live row's id as `subagent`, recorded
        // `claimrowmigrate` as operator-deleted, and lifecycle convergence gave
        // the tombstoned directive a multiplicity of zero, so queue
        // maintenance removed the reported row with no closeout.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — test\n\nDispatched.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 📥 do [#claimrowmigrate]\n",
            "- do [#plain]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#claimrowmigrate] keep a claim on its own row\n",
            "- [ ] [#plain] a real open task\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            "#claimrowmigrate",
            "subagent:claimrowmigrate",
            3600,
        )
        .unwrap();
        let live = snapshot.replace(
            "- 📥 do [#claimrowmigrate]\n",
            "- 🟡 [#subagent:claimrowmigrate#1] do [#claimrowmigrate]\n",
        );
        std::fs::write(&doc, &live).unwrap();

        for pass in 0..2 {
            run_queue_maintenance(&doc, None).unwrap();
            let updated = std::fs::read_to_string(&doc).unwrap();
            assert_eq!(
                queue_line(&updated, "claimrowmigrate"),
                "- 🟡 [#subagent:claimrowmigrate#1] do [#claimrowmigrate]",
                "pass {pass}: a claimed mirror row is live work, not an operator delete:\n{updated}"
            );
            assert!(
                !agent_doc_queue_io::queue_tombstone::current_tombstones(&doc)
                    .contains("claimrowmigrate"),
                "pass {pass}: the attempt annotation must not tombstone the row's id"
            );
        }
    }

    #[test]
    fn run_queue_maintenance_projects_claim_state_onto_sibling_queue_rows() {
        // `#queuemirrormarkers` (live 2026-10-26): `main` holds the live
        // generation-2 claim; the `issues` loop shows the expired generation 1
        // copy and a `🚧` free-text row with no claim and no in-session
        // selection. Maintenance runs on `main`, and its sibling pass repaints
        // the loop from the same ledgers.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — test\n\nDispatched.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue: Main\n",
            "<!-- agent:run id=\"main\" subagents go -->\n",
            "- do [#layout]\n",
            "- do [#plain]\n",
            "<!-- /agent:run -->\n\n",
            "## Run: Issues\n",
            "<!-- agent:loop id=issues interval=1h priority -->\n",
            "- 🚧 [#subagent:layout_liveverify#1] do [#layout]\n",
            "- 🚧 We need to work on making agent-doc easy to onboard.\n",
            "<!-- /agent:loop -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        {
            let _clock = agent_doc_queue_io::queue_claim::pin_claim_clock_for_current_thread(1_000);
            agent_doc_queue_io::queue_claim::claim(
                &doc,
                "#layout",
                "subagent:layout_liveverify",
                60,
            )
            .unwrap();
        }
        let (_, claim) = agent_doc_queue_io::queue_claim::claim(
            &doc,
            "#layout",
            "subagent:layout_liveverify",
            3600,
        )
        .unwrap();
        assert_eq!(claim.generation, 2);

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let issues = updated
            .split("<!-- agent:loop id=issues interval=1h priority -->\n")
            .nth(1)
            .and_then(|rest| rest.split("<!-- /agent:loop -->").next())
            .unwrap_or_else(|| panic!("issues loop missing:\n{updated}"));
        assert_eq!(
            issues,
            "- 🟡 [#subagent:layout_liveverify#2] do [#layout]\n\
             - 📥 We need to work on making agent-doc easy to onboard.\n",
            "{updated}"
        );
        assert!(
            updated.contains("- 🟡 [#subagent:layout_liveverify#2] do [#layout]\n- "),
            "main shows the same attempt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_syncs_backlog_into_empty_queue() {
        // #backlog-queue-sync-attr: a backlog carrying `queue=sync` regenerates
        // the (empty) queue with `do [#id]` for active items; gated/done excluded.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "- [/] [#gated] blocked\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "synced queue:\n{updated}"
        );
        assert!(updated.contains("- 📥 do [#beta]"));
        assert!(
            !updated.contains("- do [#gated]"),
            "gated item must not be queued:\n{updated}"
        );
        assert!(
            state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "empty-queue-before-sync must emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_preserves_nested_leaf_in_sibling_run() {
        // `#crossrunmirror`: api.md's selected `main` run had other work while
        // `#closed` already existed beneath PR 170 in `contracts-prs`. The
        // append mirror inspected only `main` and manufactured a duplicate.
        let dir = setup_project();
        let doc = dir.path().join("api.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:run id=\"main\" go -->\n",
            "- do [#other]\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:run id=\"contracts-prs\" -->\n",
            "- https://github.com/haiven-dev/haiven-contracts/pull/170\n",
            "  - do [#closed]\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#other] other work\n",
            "- [ ] [#closed] Why was this PR closed?\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = state.authoritative_content.as_deref().unwrap_or(content);
        let components = agent_doc_element::element::parse(updated).unwrap();
        let queues = agent_doc_queue::queue_set::QueueSet::parse(updated, &components).unwrap();
        let main = queues.block("main").unwrap();
        let contracts = queues.block("contracts-prs").unwrap();

        assert!(
            main.entries.iter().all(|entry| {
                !agent_doc_queue::document_queue::queue_entry_reference_ids(entry)
                    .iter()
                    .any(|id| id == "closed")
            }),
            "main received a duplicate #closed mirror:\n{updated}"
        );
        assert_eq!(
            contracts
                .entries
                .iter()
                .flat_map(agent_doc_queue::document_queue::queue_entry_reference_ids)
                .filter(|id| id == "closed")
                .count(),
            1,
            "contracts-prs must retain the original nested leaf:\n{updated}"
        );
    }

    #[test]
    fn review_first_sync_preserve_keeps_leaf_on_first_queue_maintenance() {
        // #review-first-sync-preserve: a session added a `#review` preset beneath
        // its PR URL. The first Run Agent Doc maintenance pass treated the bare
        // preset token as an id-backed mirror row and deleted it; re-adding it
        // after activation survived only because active-loop sync was held.
        let dir = setup_project();
        let doc = dir.path().join("sampleorders.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "prompt_presets:\n",
            "  '#review': Review the parent pull request.\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- https://github.com/example/sample-api/pull/762\n",
            "  - #review\n",
            "- do [#stale]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#active] active backlog work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = state
            .authoritative_content
            .as_deref()
            .unwrap_or_else(|| panic!("maintenance returned no authoritative content"));
        let components = agent_doc_element::element::parse(updated).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        let entries = agent_doc_queue::document_queue::parse(queue.content(updated)).unwrap();

        assert!(entries.iter().any(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                    if prompt.indent == 2
                        && agent_doc_document::queue_projection::strip_priority_markers(&prompt.text)
                            == "#review"
            )
        }), "nested #review leaf was deleted on first maintenance:\n{updated}");
        assert!(
            queue.content(updated).contains("do [#active]"),
            "active backlog mirror row was not added:\n{updated}"
        );
        assert!(
            !queue.content(updated).contains("[#stale]"),
            "stale canonical mirror row was not replaced:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_admits_exchange_free_text_as_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Build the importer\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Build the importer");
        let queue = component_body(&updated, "queue");

        // `#freetextdoid`: admitted free-text becomes a real `do [#id]` head, not
        // a `/goal` line — so it resolves as an id, can be an `after=` target, and
        // avoids the slash-command render shape that drops the `- ` marker.
        assert!(
            queue.contains(&format!("do [#{id}]")),
            "admitted free-text must queue a do-directive:\n{updated}"
        );
        assert!(
            !queue.contains("/goal Implement backlog item(s)"),
            "the /goal encoding must not be emitted by default:\n{updated}"
        );
        assert_eq!(state.selected_queue_prompts, vec![format!("do [#{id}]")]);
    }

    #[test]
    fn run_queue_maintenance_admits_new_active_queue_free_text_as_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#existing] existing work\n",
            "<!-- /agent:backlog -->\n",
        );
        let content = snapshot_content.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- Implement active queue addition\n- \n",
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement active queue addition");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("do [#{id}]")),
            "new active queue free text should become a do-directive:\n{updated}"
        );
        assert!(
            !queue.contains("Implement active queue addition"),
            "admitted free-text source line should be removed from the active queue:\n{updated}"
        );
        assert!(
            !queue.lines().any(|line| line.trim() == "-"),
            "the editor's trailing empty list placeholder must not be projected as a bare dash:\n{updated}"
        );
        assert!(
            !queue.contains("--- start"),
            "an already-active queue must not receive a redundant start fence:\n{updated}"
        );
        assert_eq!(
            state.selected_queue_prompts.first(),
            Some(&format!("do [#{id}]"))
        );
    }

    /// `#freetextqueue`: a CLAIMED free-text head promoted into a backlog id
    /// keeps its claim on the new `do [#id]` head (2026-10-10: the claimed
    /// "Run Agent Doc on frontend.md crashed …" head became
    /// `do [#runfrontendcrashed]` and re-surfaced as unclaimed dispatch work).
    #[test]
    fn run_queue_maintenance_carries_claim_onto_promoted_free_text_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#existing] existing work\n",
            "<!-- /agent:backlog -->\n",
        );
        let head = "Run Agent Doc on frontend.md crashed with a route panic";
        let content = snapshot_content.replace(
            "- do [#existing]\n",
            &format!("- do [#existing]\n- {head}\n"),
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, head, "subagent:frontendcrash", 3600).unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, head);
        let queue = component_body(&updated, "queue");
        assert!(
            queue.contains(&format!("do [#{id}]")) && !queue.contains(head),
            "the claimed free-text head must be promoted:\n{updated}"
        );

        let claimed = agent_doc_queue_io::queue_claim::claimed_items_for_content(&doc, &updated);
        assert!(
            claimed.claims(&format!("do [#{id}]")),
            "the promoted id head must inherit the free-text head's claim:\n{updated}"
        );
        let owners = agent_doc_queue_io::queue_claim::claim_owners_for_content(&doc, &updated);
        assert_eq!(
            owners.get(&agent_doc_queue::queue_claim::claim_identity(&format!(
                "do [#{id}]"
            ))),
            Some(&"subagent:frontendcrash".to_string())
        );
    }

    /// `#qstartinert`: canonical frontmatter `queue: start` must arm the queue on
    /// its own.
    ///
    /// The control-binding migration made `queue:` the canonical activation
    /// control and made `queue_active` legacy — `control_binding::queue_binding_state`
    /// deliberately ignores `queue_active` whenever `queue:` is present, and
    /// `converge_queue_control_binding_content` writes only `queue: <mode>`. But
    /// `persisted_activation` still required the legacy `queue_active: true` flag
    /// it had stopped writing, so a document whose only control was `queue: start`
    /// could never activate: the queue populated but stayed permanently UNARMED
    /// (`queue_drainable_head_count: 0`, `queue_continuation_required: false`) and
    /// the auto-loop never got a head. Observed live on an active session document
    /// with 11 queued heads, `queue: start`, and a bare queue marker.
    #[test]
    fn run_queue_maintenance_arms_queue_from_canonical_frontmatter_start_control() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: claude\n",
            // Canonical control only — no legacy `queue_active:` flag, exactly the
            // shape `converge_queue_control_binding_content` produces.
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            // Bare marker: the operator set the control in frontmatter.
            "<!-- agent:queue -->\n",
            "- do [#armme]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#armme] an ordinary agent-drainable item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_active,
            Some(true),
            "`queue: start` is the canonical activation control and must activate \
             the queue without a legacy `queue_active: true` flag"
        );
        // `start` is the supervisor-scope activation gesture (`go` is the
        // in-session drain control), so the head must be drainable in the scope
        // that owns it. Before the fix this was `false`: convergence rewrote the
        // operator's `start` to `stop` and nothing could ever pick the queue up.
        assert!(
            state.queue_supervisor_drainable,
            "an armed `queue: start` head must be drainable by the supervisor"
        );
    }

    /// `#qstartinert`: the same shape under the in-session drain control (`go`)
    /// must arm the in-session loop.
    #[test]
    fn run_queue_maintenance_arms_in_session_loop_from_canonical_frontmatter_go_control() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: claude\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#armme]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#armme] an ordinary agent-drainable item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.queue_drainable_head_count, 1,
            "an ordinary (non-operator-verify) head under `queue: go` must be \
             agent-drainable, not left unarmed"
        );
        assert!(
            state.queue_continuation_required,
            "an armed queue with a drainable head must request continuation"
        );
    }

    /// `#qstartinert` guard: `queue: stop` still dominates, and a legacy-only
    /// `queue_active: true` with no control token stays inactive exactly as before.
    #[test]
    fn run_queue_maintenance_frontmatter_stop_control_still_dominates() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: claude\n",
            "queue_active: true\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#nope]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#nope] must not run\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_drainable_head_count, 0);
        assert!(!state.queue_continuation_required);
    }

    #[test]
    fn run_queue_maintenance_ignores_response_tail_for_free_text_admission() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:head-boundary -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#ov1]\n",
            "- [route] target tmux session: 0\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ov1] [operator-verify] live drive needs a human editor\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");
        let backlog = component_body(&updated, "backlog");

        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            !backlog.contains("Done."),
            "assistant closeout text must not become backlog work:\n{updated}"
        );
        assert_eq!(state.queue_drainable_head_count, 0);
        assert!(!state.queue_continuation_required);
    }

    #[test]
    fn run_queue_maintenance_keeps_do_directive_exchange_tail_unadmitted() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:head -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(updated, content);
        assert!(state.queue_prompts.is_empty());
        assert_eq!(state.queue_active, None);
    }

    /// `#closeout-steering`: a `#subagents` queue line added since the
    /// previous cycle's steering seed is dispatched, not selected inline; a
    /// claimed one is neither listed nor selected.
    #[test]
    fn run_queue_maintenance_does_not_select_a_new_subagent_dispatch_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let head = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
        );
        let previous = format!("{head}- do something\n<!-- /agent:queue -->\n");
        let content = format!(
            "{head}- #subagents do [#preflightdeadline]\n- do something\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // The previous cycle's steering seed did not have the line.
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &agent_doc_queue_io::subagent_dispatch::midturn_steering_state_key("base", &doc),
            &serde_json::json!({
                "cycle_id": "cycle-previous",
                "baseline": previous,
                "acknowledged_queue": ["do something"],
            })
            .to_string(),
            1,
        )
        .unwrap();

        let dispatch = agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(
            &doc, &content,
        );
        assert_eq!(
            dispatch,
            vec!["#subagents do [#preflightdeadline]".to_string()]
        );
        let entries = queue_subagent_dispatch_entries(&doc, &content, &dispatch).unwrap();
        assert!(
            entries[0]
                .claim_command
                .contains("--item '#preflightdeadline'"),
            "{entries:?}"
        );
        assert!(
            entries[0].start_command.contains("queue start")
                && entries[0]
                    .start_command
                    .contains("--item '#preflightdeadline'")
                && entries[0]
                    .start_command
                    .contains("--generation <generation>")
                && entries[0].start_command.contains("--worker-id <worker-id>"),
            "{entries:?}"
        );
        // `#waypostauthorization`: every entry carries the operator's
        // authorization and the command that re-prints it.
        assert!(
            entries[0]
                .brief_command
                .starts_with("agent-doc queue brief ")
                && entries[0]
                    .brief_command
                    .ends_with("--item '#preflightdeadline'"),
            "{entries:?}"
        );
        let auth = &entries[0].authorization;
        assert_eq!(auth.item, "#subagents do [#preflightdeadline]");
        assert_eq!(
            auth.status,
            agent_doc_queue::subagent_brief::AuthorizationStatus::ItemTextOnly
        );
        assert!(
            auth.subagent_prompt_preamble
                .contains("> #subagents do [#preflightdeadline]"),
            "{auth:?}"
        );
        assert!(
            auth.subagent_prompt_preamble
                .contains(agent_doc_queue::subagent_brief::SUBAGENT_OPERATOR_REQUEST_MARKER),
            "{auth:?}"
        );
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("request, context, and every choice"));
        assert!(
            QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("also record its concrete operator action")
        );
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("--todo-add"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("--todo-done"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("Never treat a chat answer"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("run its `start_command`"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("🟡 dispatching to 🚧 running"));
        assert!(
            QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("--response '<complete worker response>'")
        );
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("placeholder-only reports are refused"));
        // `#subagentinlineveto`: never run a dispatch item inline, and say so
        // when the harness cannot spawn a subagent.
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("#subagentinlineveto"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("`**Deferred:** no subagent available`"));
        let json = serde_json::to_value(&entries[0]).unwrap();
        assert!(json["authorization"]["subagent_prompt_preamble"].is_string());
        assert!(
            json.get("model").is_none(),
            "unset launch policy must be omitted"
        );
        assert!(
            json.get("effort").is_none(),
            "unset launch policy must be omitted"
        );
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("`reasoning_effort`"));
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.selected_queue_prompts,
            vec!["do something".to_string()],
            "the next head still runs inline"
        );
        assert!(
            !state
                .selected_queue_prompts
                .iter()
                .any(|prompt| prompt.contains("preflightdeadline")),
            "a subagent dispatch item must not be selected inline: {:?}",
            state.selected_queue_prompts
        );
        let inspected = inspect_queue_state(&doc, None).unwrap();
        assert!(
            !inspected
                .selected_queue_prompts
                .iter()
                .any(|prompt| prompt.contains("preflightdeadline")),
            "{:?}",
            inspected.selected_queue_prompts
        );

        agent_doc_queue_io::queue_claim::claim(&doc, "#preflightdeadline", "subagent:pd", 600)
            .unwrap();
        let live = std::fs::read_to_string(&doc).unwrap();
        assert!(
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(&doc, &live)
                .is_empty(),
            "a claimed item is already dispatched"
        );
    }

    /// `#lostsubagentrestart`: an old row re-offered because its predecessor
    /// was abandoned carries the reason and exact prior attempt identity in
    /// the Run Agent Doc dispatch contract.
    #[test]
    fn replacement_subagent_dispatch_names_lost_reason_and_prior_worker() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let item = "re: [#grok-live-editor]: Also validate real-time steering parity";
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent: codex\n---\n\n\
             <!-- agent:run subagents go -->\n- do [#grok-live-editor]\n  - {item}\n<!-- /agent:run -->\n"
        );
        std::fs::write(&doc, &content).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            &conn,
            &agent_doc_queue_io::subagent_dispatch::midturn_steering_state_key("base", &doc),
            &serde_json::json!({
                "cycle_id": "cycle-previous",
                "baseline": content,
                "acknowledged_queue": ["do [#grok-live-editor]", item],
            })
            .to_string(),
            1,
        )
        .unwrap();
        let (_, prior) = agent_doc_queue_io::queue_claim::claim(
            &doc,
            item,
            "subagent:rtsteeringparity",
            600,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            item,
            "subagent:rtsteeringparity",
            "/root/rtsteeringparity",
            600,
            prior.generation,
        )
        .unwrap();
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            item,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Requeue,
            Some(prior.generation),
        )
        .unwrap();

        let live = std::fs::read_to_string(&doc).unwrap();
        let dispatch =
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                &doc, &live,
            )
            .unwrap();
        assert_eq!(dispatch, vec![item.to_string()]);
        let entries = queue_subagent_dispatch_entries(&doc, &live, &dispatch).unwrap();
        let recovery = entries[0].recovery.as_ref().unwrap();
        assert_eq!(recovery.reason, "attempt_abandoned_for_requeue");
        assert_eq!(recovery.prior_owner, "subagent:rtsteeringparity");
        assert_eq!(
            recovery.prior_worker_id.as_deref(),
            Some("/root/rtsteeringparity")
        );
        assert_eq!(recovery.prior_generation, prior.generation);
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("replacement"));
        assert!(QUEUE_SUBAGENT_DISPATCH_GUIDANCE.contains("prior worker/generation"));
        let json = serde_json::to_value(&entries[0]).unwrap();
        assert_eq!(
            json["recovery"]["reason"],
            "attempt_abandoned_for_requeue"
        );
    }

    /// `#presetattralias`: the live `tasks/docs.md` marker used plural
    /// `presets="#auth"`. The dispatch contract must carry that authorization
    /// to Codex instead of degrading it to `item_text_only`.
    #[test]
    fn queue_subagent_dispatch_resolves_plural_presets_alias() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent: codex\n",
            "prompt_presets:\n",
            "  '#auth': I authorize you to do this.\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents presets=\"#auth\" priority go -->\n",
            "- review and push the docs change\n",
            "<!-- /agent:queue -->\n",
        );
        let item = "review and push the docs change".to_string();
        let entries = queue_subagent_dispatch_entries(&doc, content, &[item]).unwrap();
        let auth = &entries[0].authorization;

        assert_eq!(
            auth.status,
            agent_doc_queue::subagent_brief::AuthorizationStatus::Resolved
        );
        assert_eq!(auth.presets.len(), 1, "{auth:?}");
        assert_eq!(auth.presets[0].name, "#auth");
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("I authorize you to do this.")
        );
        assert!(
            auth.subagent_prompt_preamble
                .contains("Operator authorization, queue preset `#auth`"),
            "{auth:?}"
        );
        assert!(
            !auth
                .subagent_prompt_preamble
                .contains("No queue preset applies"),
            "{auth:?}"
        );
    }

    /// `#idlessqueueidentity`: with `id=` deleted from the document's one
    /// `agent:run` and one `agent:loop`, preflight's dispatch listing offers
    /// the run's unclaimed free-text rows again (it offered nothing while the
    /// queue graph refused the document), and each entry names its origin
    /// queue `run` with a usable `queue follow-up --id run` command. The same
    /// document with its old ids keeps naming `main`.
    #[test]
    fn queue_subagent_dispatch_lists_idless_run_rows_with_their_kind_queue_id() {
        for (ids, run_id) in [(false, "run"), (true, "main")] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let (run_attr, loop_attr) = if ids {
                ("id=\"main\" ", "id=\"issues\" ")
            } else {
                ("", "")
            };
            let content = format!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nqueue: go\n\
prompt_presets:\n  \"#spec-test-build-install-commit-push\": \"update spec + tests. commit + push\"\n---\n\n\
## Exchange <!-- agent:exchange patch=append subagents -->\n<!-- /agent:exchange -->\n\n\
## Run <!-- agent:run {run_attr}subagents preset=\"#spec-test-build-install-commit-push\" priority go -->\n\
- Why have the ☑️ run items not completed?\n\
- I removed id from run + loop. Ensure these items are distinct and have semantic meaning.\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop {loop_attr}interval=\"1h\" priority -->\n\
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues.\n\
<!-- /agent:loop -->\n\n\
## Backlog <!-- agent:backlog priority queue -->\n<!-- /agent:backlog -->\n"
            );
            std::fs::write(&doc, &content).unwrap();
            let items =
                agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
                    &doc, &content,
                )
                .unwrap();
            assert_eq!(
                items,
                vec![
                    "Why have the ☑️ run items not completed?".to_string(),
                    "I removed id from run + loop. Ensure these items are distinct and have semantic meaning."
                        .to_string(),
                ],
                "ids={ids}"
            );
            let entries = queue_subagent_dispatch_entries(&doc, &content, &items).unwrap();
            for entry in &entries {
                assert_eq!(
                    entry.authorization.queue_id.as_deref(),
                    Some(run_id),
                    "ids={ids}: {entry:?}"
                );
                assert!(
                    entry
                        .authorization
                        .subagent_prompt_preamble
                        .contains(&format!("--id {run_id} --after <origin>")),
                    "{}",
                    entry.authorization.subagent_prompt_preamble
                );
            }
        }
    }

    #[test]
    fn queue_subagent_dispatch_emits_resolved_launch_policy() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:queue subagents model=opus effort=high go -->\n",
            "- review docs\n<!-- /agent:queue -->\n",
        );
        let entries =
            queue_subagent_dispatch_entries(&doc, content, &["review docs".to_string()]).unwrap();
        assert_eq!(entries[0].model.as_deref(), Some("opus"));
        assert_eq!(entries[0].effort.as_deref(), Some("high"));
        let json = serde_json::to_value(&entries[0]).unwrap();
        assert_eq!(json["model"], "opus");
        assert_eq!(json["effort"], "high");
    }

    /// Move every stored claim on `doc` to an already-lapsed TTL, writing the
    /// ledger row directly so expiry is deterministic (no sleeping).
    fn expire_queue_claims(root: &Path, doc: &Path) {
        let mut ledger = agent_doc_queue_io::queue_claim::load_ledger(doc).unwrap();
        assert!(!ledger.claims.is_empty());
        for claim in &mut ledger.claims {
            claim.claimed_at_secs = 1;
            claim.expires_at_secs = 2;
        }
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(doc).unwrap(),
                state_kind: "queue_claims".to_string(),
                canonical_path: doc.canonicalize().unwrap().to_string_lossy().into_owned(),
                payload_json: serde_json::to_string(&ledger).unwrap(),
                updated_at_secs: 3,
            },
        )
        .unwrap();
    }

    /// qsubattr3: under the queue `subagents` attribute, a claim whose TTL
    /// lapsed (its subagent died or stopped heartbeating) returns the head to
    /// `queue_subagent_dispatch`, and the head is never selected inline, so the
    /// in-session loop cannot duplicate work a subagent may still be doing.
    #[test]
    fn expired_claim_under_subagents_attribute_redispatches_and_never_drains_inline() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents=1 go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "- check the pane [#c] [inline]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let dispatch = |doc: &Path| {
            let live = std::fs::read_to_string(doc).unwrap();
            agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_or_warn(doc, &live)
        };
        let selected = |doc: &Path| {
            inspect_queue_state(doc, None)
                .unwrap()
                .selected_queue_prompts
        };

        assert_eq!(dispatch(&doc), vec!["do [#a]".to_string()]);
        agent_doc_queue_io::queue_claim::claim(&doc, "#a", "subagent:a", 600).unwrap();
        // The one slot is in flight: nothing to dispatch, `b` is held, and
        // only the opt-out head runs inline.
        assert!(dispatch(&doc).is_empty());
        assert_eq!(
            selected(&doc),
            vec!["check the pane [#c] [inline]".to_string()]
        );

        // The claim lapses without a release or a refresh.
        expire_queue_claims(dir.path(), &doc);
        assert!(
            agent_doc_queue_io::queue_claim::refresh(&doc, "#a", "subagent:a", 600).is_err(),
            "a lapsed claim cannot be refreshed back to life"
        );
        assert_eq!(
            dispatch(&doc),
            vec!["do [#a]".to_string()],
            "an expired claim makes the head eligible for dispatch again"
        );
        let inspected = selected(&doc);
        assert!(
            !inspected
                .iter()
                .any(|prompt| prompt.contains("[#a]") || prompt.contains("[#b]")),
            "an expired subagent head must never drain inline: {inspected:?}"
        );
        assert_eq!(inspected, vec!["check the pane [#c] [inline]".to_string()]);
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            !state
                .selected_queue_prompts
                .iter()
                .any(|prompt| prompt.contains("[#a]") || prompt.contains("[#b]")),
            "{:?}",
            state.selected_queue_prompts
        );
    }

    #[test]
    fn run_queue_maintenance_keeps_non_actionable_free_text_queue_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nAnswered.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do something\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");
        let backlog = component_body(&updated, "backlog");

        assert!(queue.contains("do something"), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            !backlog.contains("do something"),
            "non-actionable existing queue text should not be lifted into backlog:\n{updated}"
        );
        assert_eq!(state.selected_queue_prompts, vec!["do something"]);
    }

    #[test]
    fn run_queue_maintenance_frontmatter_can_force_free_text_queue_execution() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "agent_doc_free_text_execution: queue\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement checkout flow\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement checkout flow");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("do [#{id}]")),
            "queue mode must materialize a do head:\n{updated}"
        );
        assert!(
            !queue.contains("Implement checkout flow"),
            "free-text queue source should be replaced by the backlog-backed head:\n{updated}"
        );
        assert!(
            !queue.contains("/goal"),
            "frontmatter queue mode must not create a /goal command:\n{updated}"
        );
        assert!(state.synced_queue_ids.contains(&id));
    }

    #[test]
    fn run_queue_maintenance_queue_fallback_uses_auto_dag_priority_order() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            "agent_doc_free_text_execution: goal\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement checkout shipping after=#setup\n",
            "- Implement checkout setup\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ship] Implement checkout shipping after=#setup\n",
            "- [ ] [#setup] Implement checkout setup\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue = component_body(&updated, "queue");

        assert!(
            updated.contains("<!-- agent:queue priority -->"),
            "queue fallback must opt into the existing priority/auto-DAG path:\n{updated}"
        );
        assert!(
            // `#queueblocked`: the dependent waits on its open prerequisite.
            queue.contains("- 🚧 do [#setup]\n- ⛔ ⟦blocked by setup⟧ do [#ship]"),
            "auto-DAG fallback must run prerequisites before dependents:\n{updated}"
        );
        assert!(!queue.contains("/goal"), "{updated}");
        assert_eq!(state.queue_active, Some(true));
        assert!(
            state
                .warnings
                .iter()
                .any(|warning| warning.code == "free_text_goal_unavailable"),
            "OpenCode without /goal support must warn about queue fallback: {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_opencode_goal_extension_uses_native_goal() {
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".opencode/commands")).unwrap();
        std::fs::write(
            dir.path().join(".opencode/commands/goal.md"),
            "goal command",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            // `#freetextdoid`: `/goal` is now opt-in — `Auto` emits `do [#id]`.
            "agent_doc_free_text_execution: goal\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement OpenCode goal extension flow\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement OpenCode goal extension flow");
        let queue = component_body(&updated, "queue");

        assert!(
            queue.contains(&format!("/goal Implement backlog item(s): #{id}")),
            "OpenCode with a goal extension should use native /goal:\n{updated}"
        );
        assert!(!queue.contains("do [#"), "{updated}");
        assert!(
            state
                .warnings
                .iter()
                .all(|warning| warning.code != "free_text_goal_unavailable"),
            "goal-capable OpenCode should not warn about fallback: {:?}",
            state.warnings
        );
    }

    #[test]
    fn run_queue_maintenance_project_config_can_force_free_text_queue_execution() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "agent_doc_free_text_execution = \"queue\"\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: codex\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement import mapping\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement import mapping");
        let queue = component_body(&updated, "queue");

        assert!(queue.contains(&format!("do [#{id}]")), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_opencode_goal_config_falls_back_to_queue_without_extension() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "agent: opencode\n",
            "agent_doc_free_text_execution: goal\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement export retry\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        let id = backlog_id_for_text(&updated, "Implement export retry");
        let queue = component_body(&updated, "queue");

        assert!(queue.contains(&format!("do [#{id}]")), "{updated}");
        assert!(!queue.contains("/goal"), "{updated}");
        assert!(
            state
                .warnings
                .iter()
                .any(|warning| warning.code == "free_text_goal_unavailable"),
            "OpenCode without a /goal extension must surface the fallback warning: {:?}",
            state.warnings
        );
    }
    #[test]
    fn run_queue_maintenance_defers_while_foreign_queue_edit_lease_held() {
        // #sqedit-race Phase 2: a backlog `queue=sync` would normally regenerate
        // the empty queue. While a DIFFERENT live process holds a fresh queue-edit
        // lease (a direct `queue prune-noise` / `queue consume` in flight),
        // preflight maintenance must defer entirely — no mutation, no sync — so it
        // never round-trips a torn intermediate queue.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // pid 1 (init) is always live on Unix and is never this test process →
        // a genuine foreign in-flight queue edit.
        let doc_str = doc.to_string_lossy().to_string();
        agent_doc_queue_io::queue_edit_owner::refresh_queue_edit_owner_lease(&doc_str, 1).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            after, content,
            "queue must be untouched while a foreign edit is in flight"
        );
        assert!(
            state.synced_queue_ids.is_empty(),
            "no backlog→queue sync may run while deferred, got {:?}",
            state.synced_queue_ids
        );
        assert!(
            !after.contains("- do [#alpha]"),
            "deferred maintenance must not mint the queue head:\n{after}"
        );

        // Once the lease clears, the next pass syncs normally (the defer is a
        // yield, not a permanent skip).
        agent_doc_queue_io::queue_edit_owner::clear_queue_edit_owner_lease(&doc_str);
        let resumed = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(resumed.synced_queue_ids, vec!["alpha".to_string()]);
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("do [#alpha]")
        );
    }

    fn publish_test_live_buffer(
        doc: &Path,
        editor_id: &str,
        live_content: &str,
    ) -> (String, agent_doc_merge::crdt_sync::ReplicaState) {
        let canonical = doc.canonicalize().unwrap();
        agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&canonical).unwrap();
        let canonical_key = canonical.to_string_lossy().to_string();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: editor_id.to_string(),
            }]);
        let identity = format!("{editor_id}:{canonical_key}");
        let (client_id, bootstrap) =
            agent_doc_crdt_relay_io::register_replica_for_file(&canonical, &identity)
                .unwrap()
                .expect("test editor should register a CRDT relay replica");
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let replica_text = replica.text();
        replica.apply_local_edit(0, replica_text.len() as u32, live_content);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &canonical,
            &identity,
            &replica.encode_state(),
        )
        .unwrap()
        .expect("test editor should publish live buffer through CRDT relay");
        (identity, replica)
    }

    #[test]
    fn queue_authority_fast_forwards_proven_native_queue_save() {
        // A native editor save can reach disk after the relay lost that editor's
        // registration. The retained relay cut then still equals the baseline,
        // while disk contains the newly entered queue head. Treating the relay
        // cut as a real empty queue silently drops/skips the operator request.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let _live_replica = publish_test_live_buffer(&doc, "jetbrains-stale", baseline);

        let saved = baseline.replace(
            "<!-- agent:queue go -->\n",
            "<!-- agent:queue go -->\n- re [#modconfpush]: run the queued follow-up\n",
        );
        std::fs::write(&doc, &saved).unwrap();

        let observed = observe_queue_authority_after_native_save_with_bounded_retry(
            &doc,
            "test_native_save_fast_forward",
            3,
            |file| agent_doc_crdt_relay_io::current_text_for_file(file).map(Some),
        )
        .unwrap()
        .expect("live queue authority should remain available");
        let current = observed;
        assert!(
            matches!(
                current,
                agent_doc_crdt_relay_io::CurrentText::Current { ref text, .. }
                    if text.contains("run the queued follow-up")
            ),
            "the live CRDT authority must fast-forward to the durable queue save"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            saved,
            "the newer native-save queue item must remain byte-identical"
        );
    }

    /// `#admissionmergedup` (infra.md, 2026-09-30), reconstructed byte-exact from
    /// the logged lengths: baseline 38985 (= commit 5e6da24c), disk +18 carries
    /// the operator's reword ` if there is drift`, the authority +36 carries it
    /// twice (a controller recycle re-applied the editor's typing), and the
    /// adopted Semantic merge was authority +60 — disk's version of the line as a
    /// second item. Replayed on the real triple it is 39081 bytes exactly.
    fn admission_both_edited_docs() -> (String, String, String) {
        let doc = |head: &str, _exchange: &str| {
            format!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue: stop\n---\n\n\
                 ## Status\n\n<!-- agent:status patch=replace -->\nThe infra review is merged.\n<!-- /agent:status -->\n\n\
                 ## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nPrior answer.\n\
                 <!-- agent:boundary:bdb190e6 -->\n<!-- /agent:exchange -->\n\n\
                 ## Queue\n\n<!-- agent:queue priority -->\n- {head}\n<!-- /agent:queue -->\n\n\
                 ## Backlog\n\n<!-- agent:backlog priority -->\n<!-- /agent:backlog -->\n\n\
                 ## Review\n\n<!-- agent:review -->\n- [/] [#r1] item\n<!-- /agent:review -->\n\n\
                 ## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n\n\
                 ## Done\n\n<!-- agent:done archive=\"tasks/infra.done.md\"-->\n- [x] [#d1] done\n<!-- /agent:done -->\n"
            )
        };
        let old = "Create a PR to fix the SBX + STG drift.";
        let new = "Create a PR to fix the SBX + STG drift if there is drift.";
        let baseline = doc(old, "Prior answer.\n");
        let doubled = "Create a PR to fix the SBX + STG drift if there is drift if there is drift.";
        let disk = doc(new, "Prior answer.\n");
        let authority = doc(doubled, "Prior answer.\n");
        (baseline, authority, disk)
    }

    #[test]
    fn admission_split_merge_keeps_an_item_both_sides_edited_once() {
        let dir = setup_project();
        let doc = dir.path().join("infra.md");
        let (baseline, authority, disk) = admission_both_edited_docs();
        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .unwrap_or_else(|| authority.clone());
        let body = component_body(&merged, "queue");
        assert_eq!(
            body.matches("Create a PR to fix the SBX + STG drift")
                .count(),
            1,
            "the item both sides edited must stay one item:\n{merged}"
        );
        assert!(
            body.contains("drift if there is drift if there is drift."),
            "{merged}"
        );
    }

    /// `typingforkdup` (sdk.md, 2026-10-11): the operator was typing a NEW queue
    /// item. A native save put a 279-byte cut of it on disk (ending mid-sentence,
    /// with a trailing space); the live editor authority held the 344-byte
    /// revision; the baseline held neither. The shipped ladder answered
    /// `Cell=equals_authority Semantic=adopted` and the adopted revision carried
    /// the item twice -- the full line and its truncated prefix -- because the
    /// count guard counted disk's cut as an independent disk addition. The
    /// byte-exact replay on the reconstructed triple returned the logged 53079
    /// bytes; this fixture keeps the shape with neutral text.
    fn admission_typing_fork_docs(
        disk_queue: &str,
        authority_queue: &str,
    ) -> (String, String, String) {
        let doc = |queue: &str| {
            format!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
                 ## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nPrior answer.\n\
                 <!-- agent:boundary:bcb96c78 -->\n<!-- /agent:exchange -->\n\n\
                 # Queue\n\n<!-- agent:queue subagents priority go -->\n- do [#first]\n{queue}<!-- /agent:queue -->\n\n\
                 # Notes\n\n<!-- agent:notes -->\n<!-- /agent:notes -->\n"
            )
        };
        (doc(""), doc(authority_queue), doc(disk_queue))
    }

    const TYPING_FULL: &str = "- We also need documentation on all steps of the DX. The tooling \
         should be self-guiding. The generation DX needs to have developer friendly \
         documentation. The agentic layer needs documen";
    const TYPING_CUT: &str = "- We also need documentation on all steps of the DX. The tooling \
         should be self-guiding. The generation DX needs to have ";

    fn queue_lines_starting(merged: &str, prefix: &str) -> usize {
        component_body(merged, "queue")
            .lines()
            .filter(|line| line.starts_with(prefix))
            .count()
    }

    #[test]
    fn admission_split_merge_never_materializes_a_typing_cut_as_a_second_item() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        let (baseline, authority, disk) =
            admission_typing_fork_docs(&format!("{TYPING_CUT}\n"), &format!("{TYPING_FULL}\n"));
        std::fs::write(&doc, &disk).unwrap();
        // Precondition: the line rung on the raw triple duplicates -- this is the
        // incident, and it must stay reproducible or the test proves nothing.
        let raw =
            agent_doc_merge::document_cell_merge::document_cell_merge(&baseline, &disk, &authority)
                .merged_doc;
        assert_eq!(
            queue_lines_starting(&raw, "- We also need documentation"),
            2,
            "the unprojected line merge must reproduce the duplicate:\n{raw}"
        );

        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .unwrap_or_else(|| authority.clone());
        assert_eq!(
            queue_lines_starting(&merged, "- We also need documentation"),
            1,
            "the operator's in-progress item must stay one item:\n{merged}"
        );
        assert_eq!(merged, authority, "operator text is authoritative");
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        // `#queuecutexclusive`: both planes changed the queue, so disk's cut never
        // reaches a rung (the textual twin collapse is no longer what saves it).
        assert!(
            log.contains("CutExclusive=authority_kept(queue:0)"),
            "the cut-exclusivity projection must be visible in the admission log:\n{log}"
        );
    }

    /// `opcutahead` (sdk.md, 2026-10-15): the operator pressed Run while typing
    /// a new queue line. A native save put `...which is not f` on disk; the
    /// operator then backspaced the `f` and typed on. The authority's CRDT cut
    /// held `...which is not to be b`, and the op epoch -- read after the cut was
    /// observed -- was already a few keystrokes further. The backspaced cut is not
    /// a prefix of the operator's line, so the textual revision-twin projection
    /// cannot see it, and the op-lineage projection used to demand that the FINAL
    /// replay equal the cut, so it bailed too. The ladder logged
    /// `RevisionTwin=collapsed(1) Cell=equals_authority Semantic=adopted` and the
    /// adopted revision carried the line twice (completed line, then the cut).
    #[test]
    fn admission_split_merge_drops_a_backspaced_typing_cut_when_ops_run_ahead_of_the_cut() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        const FIRST: &str = "- If a merge gets changes requested, do I amend?\n";
        const STALE: &str = "- Can the host support two review modes? One which is not f\n";
        let (baseline, _, _) = admission_typing_fork_docs("", "");
        let (_, _, disk) = admission_typing_fork_docs(&format!("{FIRST}{STALE}"), "");
        let close = baseline.find("<!-- /agent:queue -->").unwrap();
        let f_at = close + FIRST.len() + STALE.len() - 2;
        let ops = vec![
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: close,
                text: FIRST.to_string(),
            },
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: close + FIRST.len(),
                text: STALE.to_string(),
            },
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: f_at,
                len: 1,
            },
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: f_at,
                text: "to be b".to_string(),
            },
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: f_at + "to be b".len(),
                text: "locked on a review".to_string(),
            },
        ];
        let authority = agent_doc_merge::crdt::replay_editor_ops(&baseline, &ops[..4]).unwrap();
        assert!(authority.contains("which is not to be b\n"), "{authority}");
        std::fs::write(&doc, &disk).unwrap();
        const LINE: &str = "- Can the host support two review modes?";

        // Precondition: the raw line rung on this triple reproduces the fork.
        let raw = agent_doc_merge::merge(agent_doc_merge::MergeRequest::semantic(
            &baseline, &disk, &authority,
        ))
        .merged_doc;
        assert_eq!(queue_lines_starting(&raw, LINE), 2, "{raw}");

        // `#queuecutexclusive`: WITHOUT op lineage the ladder no longer forks
        // either -- the op-ledger proof stopped being load-bearing for safety.
        let unproven =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .unwrap_or_else(|| authority.clone());
        assert_eq!(
            queue_lines_starting(&unproven, LINE),
            1,
            "the unproven ladder must keep one revision:\n{unproven}"
        );
        assert_eq!(unproven, authority, "operator text is authoritative");
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("rungs=[CutExclusive=authority_kept(queue:0) "),
            "the cut-exclusivity projection must be what kept one revision:\n{log}"
        );

        let merged = merge_admission_three_way_split(
            &doc,
            &baseline,
            &authority,
            &disk,
            &authority,
            Some(&ops),
        )
        .unwrap()
        .unwrap_or_else(|| authority.clone());
        assert_eq!(
            queue_lines_starting(&merged, LINE),
            1,
            "exactly one revision of the operator's line may survive:\n{merged}"
        );
        assert_eq!(merged, authority, "operator text is authoritative");
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("EditorOpLineage=suppressed_queue_revisions(1)"),
            "the op-lineage suppression still runs first when a proof exists:\n{log}"
        );
    }

    /// `#queuecutexclusive` reverses `AdmissionSplitMergeTwinReach` for the
    /// queue the operator is typing in: disk's `- disk branch` saved in the
    /// SAME component as a stale cut of the operator's line is not merged by
    /// admission. Text alone cannot tell it from the editor's own next
    /// keystrokes (which would then arrive twice); with a live editor, an
    /// independent disk write reaches the authority through the editor's own
    /// reload and op stream. Disk work in a component the authority did not
    /// change still lands (`admission_split_merge_still_merges_disk_work_in_an_untouched_component`).
    #[test]
    fn admission_split_merge_keeps_the_authority_queue_when_disk_adds_beside_a_typing_cut() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        let (baseline, authority, disk) = admission_typing_fork_docs(
            &format!("{TYPING_CUT}\n- disk branch\n"),
            &format!("{TYPING_FULL}\n"),
        );
        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap();
        assert_eq!(merged, None, "admission proceeds on the authority");
    }

    /// Reach for `#queuecutexclusive` (`AdmissionCutExclusivityReach`): disk
    /// work in a component the authority did NOT change still merges, beside
    /// the authority's version of the queue both planes changed.
    #[test]
    fn admission_split_merge_still_merges_disk_work_in_an_untouched_component() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        let (baseline, authority, disk_queue_only) =
            admission_typing_fork_docs(&format!("{TYPING_CUT}\n"), &format!("{TYPING_FULL}\n"));
        let disk = disk_queue_only.replace(
            "<!-- agent:notes -->\n",
            "<!-- agent:notes -->\n- disk-only note\n",
        );
        assert_ne!(
            disk, disk_queue_only,
            "fixture must carry a notes component"
        );
        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .expect("disk work in an untouched component must merge");
        assert_eq!(
            queue_lines_starting(&merged, "- We also need documentation"),
            1,
            "{merged}"
        );
        assert_eq!(
            component_body(&merged, "queue"),
            component_body(&authority, "queue"),
            "the queue both planes changed is the authority's"
        );
        assert!(
            component_body(&merged, "notes").contains("- disk-only note\n"),
            "{merged}"
        );
    }

    /// Record one attested editor burst the way the JetBrains reporter does:
    /// typed into `pre`, stamped with whatever merge baseline the editor's
    /// memo resolved. Returns the buffer after the burst.
    fn record_attested_burst(
        doc: &Path,
        stamped_merge_base: &str,
        pre: &str,
        ops: Vec<agent_doc_merge::crdt::EditorOp>,
    ) -> String {
        let post = agent_doc_merge::crdt::replay_editor_ops(pre, &ops).expect("burst applies");
        agent_doc_op_capture_io::record_editor_ops_retaining_with_lineage(
            doc,
            stamped_merge_base,
            ops,
            Some(agent_doc_state_backbone::EditorOpBurstLineage {
                pre_hash: agent_doc_hash::content_hash(pre),
                post_hash: agent_doc_hash::content_hash(&post),
            }),
        )
        .unwrap();
        post
    }

    /// `#keylogrebase` end to end: compaction -> ledger reopen -> admission
    /// proof -> the cut-exclusivity residual closed.
    ///
    /// Replays the 2026-10-15 shape. A compaction fences the op epoch while the
    /// merge baseline still names the pre-compaction text. The operator types a
    /// queue line into the compacted buffer (the authority's cut holds it),
    /// corrects it, and types a note; the native save holds all of it, the
    /// authority only the first burst. The queue is a component BOTH planes
    /// changed (cut exclusivity keeps the authority's). The notes component is
    /// one only DISK changed: before provenance it was merged as new CRDT items
    /// and the in-flight note ops then landed it a second time. The reopened
    /// ledger must be based on the compacted text, prove the authority's
    /// lineage, and keep the editor's in-flight note out of the merge.
    #[test]
    fn admission_keeps_in_flight_typing_in_a_disk_only_component_with_a_rebased_ledger() {
        let (compacted, _, _) = admission_typing_fork_docs("", "");
        let pre_compaction = compacted.replace(
            "Prior answer.\n",
            "Prior answer.\n\n### Re: older\n\nA long exchange the compaction archived.\n",
        );
        assert_ne!(pre_compaction, compacted);
        let queue_close = compacted.find("<!-- /agent:queue -->").unwrap();
        const DIX: &str = "- #gh-dixhttps://x/issues/268\n";
        const NOTE: &str = "- in-flight note the authority has not received\n";

        let replay = |rebased: bool| {
            let dir = setup_project();
            let doc = dir.path().join("agent-doc-bugs.md");
            std::fs::write(&doc, &compacted).unwrap();
            let stale = agent_doc_hash::content_hash(&pre_compaction);
            let committed = agent_doc_hash::content_hash(&compacted);
            let burst1 = vec![agent_doc_merge::crdt::EditorOp::Insert {
                offset: queue_close,
                text: DIX.to_string(),
            }];
            let fix_at = queue_close + "- #gh-".len();
            let burst2 = vec![
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset: fix_at,
                    len: 3,
                },
                agent_doc_merge::crdt::EditorOp::Insert {
                    offset: fix_at,
                    text: "fix ".to_string(),
                },
            ];
            // Compaction fenced the epoch the operator was typing in before it.
            agent_doc_op_capture_io::clear_op_capture_for_editor_projection(&doc).unwrap();
            let (authority, disk) = if rebased {
                let after1 = record_attested_burst(&doc, &stale, &compacted, burst1);
                let after2 = record_attested_burst(&doc, &committed, &after1, burst2);
                let notes_close = after2.find("<!-- /agent:notes -->").unwrap();
                let burst3 = vec![agent_doc_merge::crdt::EditorOp::Insert {
                    offset: notes_close,
                    text: NOTE.to_string(),
                }];
                let after3 = record_attested_burst(&doc, &committed, &after2, burst3);
                (after1, after3)
            } else {
                // The pre-fix ledger: every burst stamped with the merge
                // baseline the editor looked up, never with its own text.
                agent_doc_op_capture_io::record_editor_ops(&doc, &stale, burst1.clone()).unwrap();
                let after1 = agent_doc_merge::crdt::replay_editor_ops(&compacted, &burst1).unwrap();
                agent_doc_op_capture_io::record_editor_ops(&doc, &committed, burst2.clone())
                    .unwrap();
                let after2 = agent_doc_merge::crdt::replay_editor_ops(&after1, &burst2).unwrap();
                let notes_close = after2.find("<!-- /agent:notes -->").unwrap();
                let burst3 = vec![agent_doc_merge::crdt::EditorOp::Insert {
                    offset: notes_close,
                    text: NOTE.to_string(),
                }];
                agent_doc_op_capture_io::record_editor_ops(&doc, &committed, burst3.clone())
                    .unwrap();
                let after3 = agent_doc_merge::crdt::replay_editor_ops(&after2, &burst3).unwrap();
                (after1, after3)
            };
            std::fs::write(&doc, &disk).unwrap();
            let merged = merge_admission_three_way_split(
                &doc, &compacted, &authority, &disk, &authority, None,
            )
            .unwrap();
            let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap_or_default();
            (authority, merged, log)
        };

        // Without a correctly based ledger there is no provenance: the note
        // only disk changed is merged as new items -- the stated residual.
        let (authority, merged, log) = replay(false);
        let merged = merged.expect("the provenance-free ladder merges disk-only work");
        assert!(component_body(&merged, "notes").contains(NOTE), "{merged}");
        assert_eq!(
            component_body(&merged, "queue"),
            component_body(&authority, "queue")
        );
        assert!(!log.contains("EditorProvenance="), "{log}");

        // With the rebased ledger: verified provenance keeps the authority's
        // notes, so the editor's own in-flight ops land the note exactly once.
        let (_, merged, log) = replay(true);
        assert_eq!(merged, None, "admission proceeds on the authority:\n{log}");
        assert!(
            log.contains("EditorProvenance=authority_kept(notes:0)"),
            "the provenance projection must be what kept the note out:\n{log}"
        );
        assert!(
            log.contains("CutExclusive=authority_kept(queue:0)"),
            "the queue both planes changed is still cut exclusivity's:\n{log}"
        );
    }

    /// Reach for `#keylogrebase`: with a verified lineage, disk work that is NOT
    /// on it (an independent write) still merges.
    #[test]
    fn admission_with_provenance_still_merges_independent_disk_work() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        let (baseline, _, _) = admission_typing_fork_docs("", "");
        std::fs::write(&doc, &baseline).unwrap();
        let queue_close = baseline.find("<!-- /agent:queue -->").unwrap();
        let authority = record_attested_burst(
            &doc,
            &agent_doc_hash::content_hash(&baseline),
            &baseline,
            vec![agent_doc_merge::crdt::EditorOp::Insert {
                offset: queue_close,
                text: "- operator line\n".to_string(),
            }],
        );
        let disk = baseline.replace(
            "<!-- agent:notes -->\n",
            "<!-- agent:notes -->\n- independent note\n",
        );
        std::fs::write(&doc, &disk).unwrap();
        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .expect("independent disk work must still merge");
        assert!(component_body(&merged, "notes").contains("- independent note\n"));
        assert!(component_body(&merged, "queue").contains("- operator line\n"));
    }

    /// A disk revision NEWER than the authority's (a native save ahead of the
    /// replica) is not projected; the twin-pair guard refuses every candidate
    /// that would hold both, and admission proceeds on the authority
    /// (`AdmissionSplitMergeTwinPairWedge`).
    #[test]
    fn admission_split_merge_refuses_a_newer_disk_revision_beside_the_authority() {
        let dir = setup_project();
        let doc = dir.path().join("sdk.md");
        // The authority's cut carries no trailing space here: the line rung trims
        // trailing whitespace, and a trimmed authority line would be refused as
        // `would_drop_authority_content` before the twin-pair guard is reached.
        let (baseline, authority, disk) = admission_typing_fork_docs(
            &format!("{TYPING_FULL}\n"),
            &format!("{}\n", TYPING_CUT.trim_end()),
        );
        std::fs::write(&doc, &disk).unwrap();
        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .unwrap_or_else(|| authority.clone());
        assert_eq!(
            queue_lines_starting(&merged, "- We also need documentation"),
            1,
            "{merged}"
        );
        assert_eq!(merged, authority, "operator text is authoritative");
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        // `#queuecutexclusive`: the newer disk revision never reaches a rung, so
        // the twin-pair guard is no longer the edge that refuses it.
        assert!(
            log.contains("CutExclusive=authority_kept(queue:0)"),
            "the cut-exclusivity projection must be what kept the authority:\n{log}"
        );
    }

    /// `#admissionsplitmerge`: end-to-end through the real CRDT relay. Both branches
    /// durably advanced past the baseline, which used to be a typed fail-closed
    /// refusal. It must not stall, and disk is never written by admission.
    ///
    /// `#queuecutexclusive`: both planes changed the QUEUE, so the authority's
    /// queue is kept. This test used to require the union of the two queue
    /// additions; that union is exactly the body every live duplicate since
    /// 2026-09-30 adopted, where disk was the editor's own native save. Disk work
    /// in a component the authority left alone still merges (next test).
    #[test]
    fn queue_authority_keeps_the_editor_queue_when_both_branches_changed_it() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let (identity, replica) = publish_test_live_buffer(&doc, "jetbrains-diverged", baseline);
        let authority = baseline.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:queue -->\n- authority branch\n",
        );
        replica.apply_local_edit(0, baseline.len() as u32, &authority);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap();
        let disk = baseline.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:queue -->\n- disk branch\n",
        );
        std::fs::write(&doc, &disk).unwrap();

        let observed = observe_queue_authority_after_native_save_with_bounded_retry(
            &doc,
            "test_ambiguous_native_save",
            3,
            |file| agent_doc_crdt_relay_io::current_text_for_file(file).map(Some),
        )
        .expect("two advanced branches must not refuse admission");
        let text = match observed {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
            other => panic!("expected a live current authority, got {other:?}"),
        };
        assert!(
            text.contains("- authority branch"),
            "the authority must keep the editor's branch:\n{text}"
        );
        assert!(
            !text.contains("- disk branch"),
            "a second version of the queue both planes changed must not be merged in:\n{text}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            disk,
            "admission reconciles the live authority and never writes disk"
        );
    }

    /// `#queuecutexclusive` reach, end to end: the authority changed the queue
    /// and disk changed only the exchange. The disk branch merges into the live
    /// authority beside the editor's queue.
    #[test]
    fn queue_authority_merges_disk_work_in_a_component_the_editor_left_alone() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\nPrior answer.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let (identity, replica) = publish_test_live_buffer(&doc, "jetbrains-disjoint", baseline);
        let authority = baseline.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:queue -->\n- authority branch\n",
        );
        replica.apply_local_edit(0, baseline.len() as u32, &authority);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap();
        let disk = baseline.replace("Prior answer.\n", "Prior answer.\nDisk branch.\n");
        std::fs::write(&doc, &disk).unwrap();

        let observed = observe_queue_authority_after_native_save_with_bounded_retry(
            &doc,
            "test_disjoint_native_save",
            3,
            |file| agent_doc_crdt_relay_io::current_text_for_file(file).map(Some),
        )
        .expect("two advanced branches must merge, not refuse admission");
        let text = match observed {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
            other => panic!("expected a live current authority, got {other:?}"),
        };
        assert!(text.contains("- authority branch"), "{text}");
        assert!(text.contains("Disk branch."), "{text}");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), disk);
    }

    /// `#queuecutexclusive` fixture: the 2026-10-15 incident shape with neutral
    /// text. The baseline queue is empty. The operator added six issue lines,
    /// typed `#gh-dix` on all six with a multi-caret, then corrected them to
    /// `#gh-fix ` and pressed Run. The authority's cut holds the `#gh-dix`
    /// revision (the correction's ops are still in flight); the native save on
    /// disk holds the corrected lines. Neither is a prefix of the other, so the
    /// textual revision-twin guards cannot pair them, and both sides "added" six
    /// items, so the count bound admits twelve.
    fn admission_inflight_correction_docs() -> (String, String, String) {
        let doc = |queue: &str| {
            format!(
                "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
                 ## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior\n\nPrior answer.\n\
                 <!-- agent:boundary:06765b07 -->\n<!-- /agent:exchange -->\n\n\
                 ## Pending\n<!-- agent:queue subagents preset=\"#spec\" priority go -->\n{queue}<!-- /agent:queue -->\n\n\
                 ## Backlog\n\n<!-- agent:backlog priority queue -->\n<!-- /agent:backlog -->\n"
            )
        };
        let lines = |tag: &str| -> String {
            (268..=273)
                .map(|n| format!("- {tag}https://example.test/agent-doc/issues/{n}\n"))
                .collect()
        };
        (doc(""), doc(&lines("#gh-dix")), doc(&lines("#gh-fix ")))
    }

    fn issue_line_counts(merged: &str) -> Vec<usize> {
        let body = component_body(merged, "queue");
        (268..=273)
            .map(|n| {
                body.lines()
                    .filter(|line| line.ends_with(&format!("/agent-doc/issues/{n}")))
                    .count()
            })
            .collect()
    }

    /// `#queuecutexclusive`: the admission ladder never unions two cuts of one
    /// editor history. Pre-fix it logged `Cell=equals_authority
    /// Semantic=adopted(...)` and adopted every issue twice (both revisions).
    #[test]
    fn admission_split_merge_never_unions_an_inflight_mid_line_correction() {
        let dir = setup_project();
        let doc = dir.path().join("bugs.md");
        let (baseline, authority, disk) = admission_inflight_correction_docs();
        std::fs::write(&doc, &disk).unwrap();
        // Precondition: the raw line rung on the triple duplicates every issue --
        // the incident must stay reproducible or this test proves nothing.
        let raw = agent_doc_merge::merge(agent_doc_merge::MergeRequest::semantic(
            &baseline, &disk, &authority,
        ))
        .merged_doc;
        assert_eq!(issue_line_counts(&raw), vec![2; 6], "{raw}");

        let merged =
            merge_admission_three_way_split(&doc, &baseline, &authority, &disk, &authority, None)
                .unwrap()
                .unwrap_or_else(|| authority.clone());
        assert_eq!(issue_line_counts(&merged), vec![1; 6], "{merged}");
        assert_eq!(merged, authority, "the authority's cut is kept whole");
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            log.contains("rungs=[CutExclusive=authority_kept(queue:0) Cell=equals_authority Semantic=equals_authority]"),
            "the projection must be what kept the authority:\n{log}"
        );
        assert!(log.contains("outcome=proceed_on_authority"), "{log}");
    }

    /// `#componentownclose`: preflight admission over the incident shape. The
    /// operator's editor deleted loop rows (valid), then the loop close, then
    /// through the todo body, while disk carries the agent's loop repaint.
    /// Admission never adopts a merge that ends the loop at a sibling's close:
    /// for broken markers every rung refuses and the authority (operator text)
    /// is kept as-is; for valid markers the todo survives byte-for-byte.
    #[test]
    fn componentownclose_admission_never_absorbs_the_sibling_todo() {
        use agent_doc_element::sibling_span_fixture::{self as fx, LoopEdit};
        for style in fx::ALL_STYLES {
            for edit in [
                LoopEdit::DeleteRowsKeepClose,
                LoopEdit::DeleteRowsAndLoopClose,
                LoopEdit::DeleteThroughTodoBody,
            ] {
                let dir = setup_project();
                let doc = dir.path().join("bugs.md");
                let baseline = fx::document(style, LoopEdit::None);
                let disk =
                    baseline.replacen("- 🔄 release + publish\n", "- 📥 release + publish\n", 1);
                let authority = fx::document(style, edit);
                std::fs::write(&doc, &disk).unwrap();
                let merged = merge_admission_three_way_split(
                    &doc, &baseline, &authority, &disk, &authority, None,
                )
                .unwrap();
                let label = format!("{style:?}/{edit:?}");
                let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                    .unwrap_or_default();
                if fx::edit_is_structurally_valid(edit) {
                    let kept = merged.unwrap_or_else(|| authority.clone());
                    assert_eq!(
                        kept.matches(&fx::todo_region(style)).count(),
                        1,
                        "{label}:\n{kept}"
                    );
                    assert!(agent_doc_element::element::parse(&kept).is_ok(), "{label}");
                } else {
                    assert_eq!(
                        merged, None,
                        "{label}: nothing is adopted over broken markers"
                    );
                    assert!(
                        log.contains(
                            "Cell=component_structure_refused Semantic=component_structure_refused"
                        ),
                        "{label}:\n{log}"
                    );
                    assert!(log.contains("outcome=proceed_on_authority"), "{label}");
                }
            }
        }
    }

    /// `#queuecutexclusive` fail-closed guard, independent of the projection:
    /// a candidate whose concurrently changed queue is a body built from both
    /// cuts is refused, while either cut alone is admitted.
    #[test]
    fn the_cut_exclusivity_guard_refuses_the_logged_union_of_both_cuts() {
        let (baseline, authority, disk) = admission_inflight_correction_docs();
        let union = agent_doc_merge::merge(agent_doc_merge::MergeRequest::semantic(
            &baseline, &disk, &authority,
        ))
        .merged_doc;
        assert_eq!(
            agent_doc_document::component_exclusivity::merge_mixes_component_versions(
                &baseline, &authority, &disk, &union
            )
            .as_deref(),
            Some("queue:0")
        );
        for cut in [&authority, &disk] {
            assert_eq!(
                agent_doc_document::component_exclusivity::merge_mixes_component_versions(
                    &baseline, &authority, &disk, cut
                ),
                None
            );
        }
    }

    /// `#queuecutexclusive` end to end through the real CRDT relay, including
    /// the step that completed the live duplicate: the editor's in-flight
    /// correction ops arriving AFTER admission. Pre-fix, admission inserted the
    /// six `#gh-fix ` lines as new CRDT items and the late ops then rewrote the
    /// editor's own six `#gh-dix` items into the same text -- twelve `#gh-fix`
    /// lines. Now admission proceeds on the authority and the late ops roll it
    /// forward exactly once.
    #[test]
    fn queue_authority_never_duplicates_a_correction_whose_ops_arrive_after_admission() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let (baseline, authority, disk) = admission_inflight_correction_docs();
        std::fs::write(&doc, &baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let (identity, replica) = publish_test_live_buffer(&doc, "jetbrains-inflight", &baseline);
        // The operator's `#gh-dix` typing reached the authority.
        let close = replica.text().find("<!-- /agent:queue -->").unwrap();
        let dix_block = component_body(&authority, "queue");
        replica.apply_local_edit(close as u32, 0, &dix_block);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap();
        // The native save already holds the corrected lines.
        std::fs::write(&doc, &disk).unwrap();

        let observed = observe_queue_authority_after_native_save_with_bounded_retry(
            &doc,
            "test_inflight_correction",
            3,
            |file| agent_doc_crdt_relay_io::current_text_for_file(file).map(Some),
        )
        .expect("admission must not stall");
        let admitted = match observed {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
            other => panic!("expected a live current authority, got {other:?}"),
        };
        assert_eq!(issue_line_counts(&admitted), vec![1; 6], "{admitted}");

        // The correction's ops arrive late, on the editor's own items.
        loop {
            let text = replica.text();
            let Some(at) = text.find("#gh-dixhttps") else {
                break;
            };
            let d_at = at + "#gh-".len();
            replica.apply_local_edit(d_at as u32, 1, "f");
            replica.apply_local_edit((d_at + "fix".len()) as u32, 0, " ");
        }
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap();
        let settled = match agent_doc_crdt_relay_io::current_text_for_file(&doc).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("expected a live current authority, got {other:?}"),
        };
        assert_eq!(
            issue_line_counts(&settled),
            vec![1; 6],
            "every issue line must be held exactly once after the late ops:\n{settled}"
        );
        assert_eq!(
            component_body(&settled, "queue"),
            component_body(&disk, "queue"),
            "the editor's final text is the authority's, exactly once"
        );
    }

    /// GH #198: disk can be an intermediate save from the same editor-op
    /// lineage while also carrying another addition. Admission must never
    /// re-add the superseded intermediate row. Since `#queuecutexclusive` the
    /// queue both planes changed is the authority's, so the other same-queue
    /// addition is not merged by admission either.
    #[test]
    fn admission_merge_drops_an_op_proven_stale_queue_revision_only() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let insert_at = baseline.find("<!-- /agent:queue -->").unwrap();
        let stale_row = "- Add me to channen under team.\n";
        let suffix = " Also verify membership.";
        let typo_at = insert_at + stale_row.find("channen").unwrap() + "channe".len();
        let append_at = insert_at + stale_row.len() - 1;
        let ops = vec![
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: insert_at,
                text: stale_row.to_string(),
            },
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: append_at,
                text: suffix.to_string(),
            },
            agent_doc_merge::crdt::EditorOp::Delete {
                offset: typo_at,
                len: 1,
            },
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: typo_at,
                text: "l".to_string(),
            },
        ];
        let authority = agent_doc_merge::crdt::replay_editor_ops(baseline, &ops).unwrap();
        assert!(authority.contains("- Add me to channel under team. Also verify membership.\n"));
        let (identity, replica) = publish_test_live_buffer(&doc, "jetbrains-op-lineage", baseline);
        replica.apply_local_edit(0, baseline.len() as u32, &authority);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap();
        agent_doc_op_capture_io::record_editor_ops(
            &doc,
            &agent_doc_hash::content_hash(baseline),
            ops,
        )
        .unwrap();
        assert!(
            agent_doc_op_capture_io::editor_ops_for_base(&doc, baseline)
                .unwrap()
                .is_some(),
            "the exact baseline-keyed editor-op epoch must be available to admission"
        );

        let disk = baseline.replace(
            "<!-- agent:queue -->\n",
            concat!(
                "<!-- agent:queue -->\n",
                "- Add me to channen under team.\n",
                "- independent disk addition\n",
            ),
        );
        std::fs::write(&doc, &disk).unwrap();
        // `#queuecutexclusive`: both planes changed the queue, so admission
        // proceeds on the authority. The op-proven stale row is not re-added
        // (the original GH #198 failure); the same-component disk addition is
        // not merged by admission either (see
        // `admission_split_merge_keeps_the_authority_queue_when_disk_adds_beside_a_typing_cut`).
        let first_projection = disk_edit_newer_than_registered_authority(&doc, &authority).unwrap();
        assert_eq!(first_projection, None);

        let observed = observe_queue_authority_after_native_save_with_bounded_retry(
            &doc,
            "test_op_lineage_admission_merge",
            3,
            |file| agent_doc_crdt_relay_io::current_text_for_file(file).map(Some),
        )
        .unwrap()
        .expect("live authority remains available");
        let text = match observed {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("expected current live authority, got {other:?}"),
        };
        assert!(
            text.contains("- Add me to channel under team. Also verify membership.\n"),
            "the final operator revision must survive:\n{text}"
        );
        assert!(
            !text.contains("- independent disk addition\n"),
            "a second version of the queue both planes changed is not merged in:\n{text}"
        );
        assert!(
            !text.contains("- Add me to channen under team.\n"),
            "the op-proven intermediate revision must not be re-added:\n{text}"
        );
    }

    #[test]
    fn run_queue_maintenance_preserves_every_pre_run_live_buffer_queue_addition() {
        // #qeditrace: the operator replaced a stale queue and added several
        // heads before invoking Run Agent Doc. Maintenance must derive and CAS
        // from the complete live buffer, never retain only its first addition.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#stale-one]\n",
            "- do [#stale-two]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, snapshot_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let live_content = snapshot_content
            .replace("<!-- agent:queue -->", "<!-- agent:queue auto -->")
            .replace(
                "- do [#stale-one]\n- do [#stale-two]\n",
                "- do [#fresh-one]\n- do [#fresh-two]\n- do [#fresh-three]\n",
            );
        let _ = publish_test_live_buffer(&doc, "preflight-multi-add-test", &live_content);

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_prompts,
            vec![
                "do [#fresh-one]".to_string(),
                "do [#fresh-two]".to_string(),
                "do [#fresh-three]".to_string(),
            ],
        );
        let current =
            match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(&doc).unwrap() {
                agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
                other => panic!("expected embedded Lazily authority, got {other:?}"),
            };
        for id in ["fresh-one", "fresh-two", "fresh-three"] {
            assert!(
                current.contains(&format!("do [#{id}]")),
                "live Lazily head lost {id}:\n{current}"
            );
        }
        assert!(!current.contains("do [#stale-one]"));
        assert!(!current.contains("do [#stale-two]"));
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            snapshot_content,
            "preflight must not overwrite stale disk behind the editor authority"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        for id in ["fresh-one", "fresh-two", "fresh-three"] {
            assert!(
                snapshot.contains(&format!("do [#{id}]")),
                "recovery baseline lost {id}:\n{snapshot}"
            );
        }
        assert!(!snapshot.contains("do [#stale-one]"));
        assert!(!snapshot.contains("do [#stale-two]"));
    }

    #[test]
    fn queue_maintenance_coalesces_a_continued_draft_with_its_adjacent_snapshot_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior\nDone.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue go -->\n- 🚧 do [#existing]\n<!-- /agent:queue -->\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] 🚧 [#existing] Implement release and publish\n",
            "<!-- /agent:backlog -->\n",
        );
        let live_content = snapshot_content.replace(
            "- 🚧 do [#existing]\n",
            "- 🚧 do [#existing]\n- Implement release and publish and install\n",
        );
        std::fs::write(&doc, &live_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(updated.matches("do [#existing]").count(), 1, "{updated}");
        assert_eq!(updated.matches("[#existing]").count(), 2, "{updated}");
        assert!(
            updated.contains("[#existing] Implement release and publish and install"),
            "the live continuation must replace the partial backlog snapshot:\n{updated}"
        );
        assert!(
            !updated.contains("\n- Implement release and publish and install\n"),
            "the live continuation must not survive as a second queue item:\n{updated}"
        );
    }

    #[test]
    fn stale_queue_maintenance_rebases_over_live_multi_adds_and_three_typed_bytes() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let disk_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior\nDone.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#old]\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, disk_content).unwrap();
        let observed_first =
            disk_content.replace("- do [#old]\n", "- do [#old]\n- do [#fresh-one]\n");
        let (identity, replica) =
            publish_test_live_buffer(&doc, "preflight-stale-cas-test", &observed_first);
        let live_all = observed_first
            .replace(
                "- do [#fresh-one]\n",
                "- do [#fresh-one]\n- do [#fresh-two]\n- do [#fresh-three]\n",
            )
            .replace("Done.\n", "Donexyz.\n");
        let replica_text = replica.text();
        replica.apply_local_edit(0, replica_text.len() as u32, &live_all);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &doc,
            &identity,
            &replica.encode_state(),
        )
        .unwrap()
        .expect("test editor should publish the newer complete live buffer");

        let stale_target = observed_first.replace("queue_active: false", "queue_active: true");
        let applied = persist_queue_maintenance_doc(
            &doc,
            &stale_target,
            &observed_first,
            None,
            "qeditrace_test",
        )
        .expect("stale maintenance should rebase over the live head");
        let expected = live_all.replace("queue_active: false", "queue_active: true");
        assert_eq!(
            applied, expected,
            "the returned maintenance cut must be the exact rebased canonical"
        );

        let current =
            match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(&doc).unwrap() {
                agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
                other => panic!("expected embedded Lazily authority, got {other:?}"),
            };
        assert_eq!(
            current, expected,
            "maintenance must preserve all live queue additions and the exact three typed bytes"
        );
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("queue_maintenance_rebased")
                && ops_log.contains("outcome=head_advanced_preserved")
                && ops_log.contains("recovery=component_crdt"),
            "stale maintenance needs a durable rebase artifact:\n{ops_log}"
        );
    }

    /// `#unstrikelost` (unreadable evidence): the CAS rebase joins a base-struck item
    /// back to struck unless operator evidence re-arms it, so evidence it
    /// cannot read (here: no project state at all) must abort the rebase.
    #[test]
    fn rebase_over_a_struck_base_refuses_when_rearm_evidence_is_unreadable() {
        let expected = concat!(
            "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#stillopen]\n",
            "- ~~release + publish~~\n",
            "<!-- /agent:queue -->\n",
        );
        let target = expected.replace("- do [#stillopen]", "- 🚧 do [#stillopen]");
        let live = expected.replace("~~release + publish~~", "release + publish");

        let err = rebase_queue_maintenance_target(
            Path::new("missing-dir/session.md"),
            "test",
            expected,
            &target,
            &live,
        )
        .expect_err("unreadable evidence over a struck base must not rebase");

        assert!(
            err.downcast_ref::<QueueAuthorityUnavailable>().is_some(),
            "{err:#}"
        );
    }

    #[test]
    fn marker_only_rebase_projects_one_row_without_joining_its_tracked_neighbor() {
        let expected = concat!(
            "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue priority go -->\n",
            "- #upgrade\n",
            "- I'm seeing \n",
            "- do [#issuesneedreopened]\n",
            "<!-- /agent:queue -->\n",
        );
        let target = expected.replace("- I'm seeing \n", "- 🚧 I'm seeing \n");
        let live = expected.replace("old\n", "operator kept typing\n");

        let rebased = rebase_queue_maintenance_target(
            Path::new("session.md"),
            "test",
            expected,
            &target,
            &live,
        )
        .unwrap();

        assert!(rebased.contains("operator kept typing\n"));
        assert!(rebased.contains("- 🚧 I'm seeing \n- do [#issuesneedreopened]\n"));
        assert_eq!(rebased.matches("[#issuesneedreopened]").count(), 1);
        let body = queue_component_body(&rebased).unwrap().unwrap();
        assert_eq!(
            agent_doc_queue::document_queue::parse(body).unwrap().len(),
            3
        );
    }

    #[test]
    fn marker_only_rebase_preserves_a_concurrently_extended_free_text_row() {
        let expected = concat!(
            "<!-- agent:queue priority go -->\n",
            "- #upgrade\n",
            "- I'm seeing \n",
            "- do [#issuesneedreopened]\n",
            "<!-- /agent:queue -->\n",
        );
        let target = expected.replace("- I'm seeing \n", "- 🚧 I'm seeing \n");
        let live = expected.replace("- I'm seeing \n", "- I'm seeing a second symptom\n");

        let rebased = rebase_queue_maintenance_target(
            Path::new("session.md"),
            "test",
            expected,
            &target,
            &live,
        )
        .unwrap();

        assert!(rebased.contains("- I'm seeing a second symptom\n"));
        assert!(!rebased.contains("🚧 I'm seeing \n"));
        assert!(rebased.contains("\n- do [#issuesneedreopened]\n"));
        let body = queue_component_body(&rebased).unwrap().unwrap();
        assert_eq!(
            agent_doc_queue::document_queue::parse(body).unwrap().len(),
            3
        );
    }

    #[test]
    fn run_queue_maintenance_adopts_live_buffer_queue_duplicate_delete() {
        // #qeditdelete: the operator deletes one duplicate queue row in the live
        // editor while disk still has both copies. Queue maintenance must start
        // from that editor-authored deletion before it converges queue shape back
        // to the editor, or the stale disk copy reappears.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- :pushpin: do [#dup]\n",
            "- :pushpin: do [#dup]\n",
            "- :pushpin: do [#keep]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let live_content = content.replacen(
            "- :pushpin: do [#dup]\n- :pushpin: do [#dup]\n",
            "- :pushpin: do [#dup]\n",
            1,
        );
        let canonical = doc.canonicalize().unwrap();
        agent_doc_crdt_relay_io::register_embedded_relay_route_for_file(&canonical).unwrap();
        let canonical_key = canonical.to_string_lossy().to_string();
        let editor_id = "preflight-queue-maintenance-test";
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: editor_id.to_string(),
            }]);
        let identity = format!("{editor_id}:{canonical_key}");
        let (client_id, bootstrap) =
            agent_doc_crdt_relay_io::register_replica_for_file(&canonical, &identity)
                .unwrap()
                .expect("test editor should register a CRDT relay replica");
        let replica =
            agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap).unwrap();
        let replica_text = replica.text();
        replica.apply_local_edit(0, replica_text.len() as u32, &live_content);
        agent_doc_crdt_relay_io::relay_replica_update_for_file(
            &canonical,
            &identity,
            &replica.encode_state(),
        )
        .unwrap()
        .expect("test editor should publish live buffer through CRDT relay");

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated =
            match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(&doc).unwrap() {
                agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
                other => panic!("expected embedded Lazily authority, got {other:?}"),
            };
        assert_eq!(
            updated.matches("do [#dup]").count(),
            1,
            "the operator-deleted duplicate must not be re-pushed into Lazily:\n{updated}"
        );
        assert!(
            updated.contains("do [#keep]"),
            "unrelated queue rows must survive:\n{updated}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .matches("do [#dup]")
                .count(),
            2,
            "preflight must not overwrite disk behind the editor authority"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snap.matches("do [#dup]").count(),
            1,
            "snapshot must adopt the deleted duplicate too:\n{snap}"
        );
    }

    #[test]
    fn run_queue_maintenance_restrikes_snapshot_struck_live_reemit() {
        // #qeditdupguard: a stale editor/live-buffer flush can replay an
        // unstruck copy of a queue row that the committed snapshot already
        // struck. Queue maintenance must converge that row back to `Completed`
        // instead of making the retired work runnable again.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue preset=\"#spec-test-commit-push\" priority start -->\n",
            "- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n",
        );
        let live_stale_reemit = snapshot_content.replace(
            "- ~~:pushpin: do [#qeditdup] [#qftloss#qftloss]~~",
            "- :pushpin: do [#qeditdup] [#qftloss#qftloss]",
        );
        std::fs::write(&doc, live_stale_reemit).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let active: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                    Some(prompt.text.as_str())
                }
                _ => None,
            })
            .collect();
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => {
                    Some(prompt.text.as_str())
                }
                _ => None,
            })
            .collect();

        assert!(
            !active.iter().any(|text| text.contains("#qeditdup")),
            "stale live qeditdup head must not stay runnable: active={active:?}"
        );
        assert!(
            completed.iter().any(|text| text.contains("#qeditdup")),
            "snapshot-struck qeditdup head must remain struck: completed={completed:?}"
        );
        assert!(
            active.iter().any(|text| text.contains("#stillopen")),
            "unrelated live head must stay runnable: active={active:?}"
        );
    }

    /// `#unstrikelost` (2026-10-04, agent-doc-bugs.md 08fdaf4438): the operator
    /// un-struck `~~release + publish~~` in the editor. The next preflight's
    /// queue maintenance treated the live copy as a stale re-emit of the
    /// snapshot-struck head (`#qeditdupguard`) and re-struck it, and the
    /// following closeout — which never quoted or answered that head — committed
    /// the re-strike, silently removing it from the drain.
    ///
    /// The operator's own captured editor ops replay onto the snapshot to the
    /// live head, so this is a re-arm: it must stay live through maintenance and
    /// through a closeout whose response does not quote it.
    #[test]
    fn unstrike_then_a_closeout_with_no_quote_keeps_the_head_live() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — claude\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "Released.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#stillopen]\n",
            "- ~~release + publish~~\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, snapshot_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // The operator deletes the closing then the opening `~~` in the editor;
        // the editor records those keystrokes against the snapshot base.
        let struck_at = snapshot_content.find("~~release + publish~~").unwrap();
        let close_at = struck_at + "~~release + publish".len();
        agent_doc_op_capture_io::record_editor_ops(
            &doc,
            &agent_doc_hash::content_hash(snapshot_content),
            vec![
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset: close_at,
                    len: 2,
                },
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset: struck_at,
                    len: 2,
                },
            ],
        )
        .unwrap();
        let unstruck = snapshot_content.replace("~~release + publish~~", "release + publish");
        std::fs::write(&doc, &unstruck).unwrap();

        let live_release = |doc: &Path| {
            read_queue_entries(doc).iter().any(|entry| {
                matches!(
                    entry,
                    agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                        if prompt.text.contains("release + publish")
                )
            })
        };

        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            live_release(&doc),
            "queue maintenance re-struck the operator's unstrike:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );

        // A closeout whose response dispatched other work and never quoted the
        // re-armed head must not strike it (`#ftstrike` strikes answers only).
        let struck = agent_doc_queue_io::queue_consume::strike_answered_free_text_queue_heads(
            &doc,
            "> **Queue prompt:** do [#stillopen]\n\nDispatched to a subagent.\n",
            true,
            &TEST_QUEUE_CONSUME_WRITE_EFFECTS,
        )
        .unwrap();
        assert_eq!(struck, 0, "an unquoted re-armed head is not an answer");

        // And the next preflight's maintenance still leaves it in the drain.
        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            live_release(&doc),
            "the second maintenance pass re-struck the operator's unstrike:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
    }

    /// The `#unstrikelost` fixture: the snapshot holds `~~release + publish~~`,
    /// the operator's captured editor ops un-strike it, and disk carries the
    /// unstruck queue.
    fn operator_unstruck_release_fixture() -> (TempDir, PathBuf) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — claude\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "Released.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#stillopen]\n",
            "- ~~release + publish~~\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, snapshot_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let struck_at = snapshot_content.find("~~release + publish~~").unwrap();
        let close_at = struck_at + "~~release + publish".len();
        agent_doc_op_capture_io::record_editor_ops(
            &doc,
            &agent_doc_hash::content_hash(snapshot_content),
            vec![
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset: close_at,
                    len: 2,
                },
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset: struck_at,
                    len: 2,
                },
            ],
        )
        .unwrap();
        std::fs::write(
            &doc,
            snapshot_content.replace("~~release + publish~~", "release + publish"),
        )
        .unwrap();
        (dir, doc)
    }

    fn release_head_is_live(doc: &Path) -> bool {
        read_queue_entries(doc).iter().any(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                    if prompt.text.contains("release + publish")
            )
        })
    }

    /// `#unstrikelost`, unreadable evidence (the load flake of
    /// `unstrike_then_a_closeout_with_no_quote_keeps_the_head_live`, 2026-10-15):
    /// the controller this pass launched died before the `#unstrikelost`
    /// evidence read, the read failed, the failure was mapped to "no
    /// evidence", and `#qeditdupguard` re-struck and persisted the operator's
    /// unstrike. An unreadable epoch must refuse admission and leave the
    /// queue untouched instead.
    #[test]
    fn unreadable_rearm_evidence_refuses_instead_of_restriking_an_unstrike() {
        let (dir, doc) = operator_unstruck_release_fixture();
        let before = std::fs::read_to_string(&doc).unwrap();

        rearm_evidence_fault::arm(u32::MAX);
        let err = run_queue_maintenance(&doc, None)
            .expect_err("maintenance must not converge on unreadable re-arm evidence");
        let reads = rearm_evidence_fault::taken();
        rearm_evidence_fault::arm(0);

        assert!(
            err.downcast_ref::<QueueAuthorityUnavailable>().is_some(),
            "the refusal must be the admission-refusing error class: {err:#}"
        );
        assert_eq!(
            reads,
            queue_authority_attempts(),
            "bounded retries, then refuse"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            before,
            "a refused pass must not persist anything"
        );
        assert!(release_head_is_live(&doc));
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            ops.contains("queue_rearm_evidence_unavailable") && ops.contains("admission=refused"),
            "the refusal must be logged:\n{ops}"
        );
    }

    /// A transiently unreadable epoch (one failed read) recovers on retry and
    /// keeps the operator's unstrike live.
    #[test]
    fn transiently_unreadable_rearm_evidence_recovers_and_keeps_the_unstrike() {
        let (_dir, doc) = operator_unstruck_release_fixture();

        rearm_evidence_fault::arm(1);
        let result = run_queue_maintenance(&doc, None);
        let reads = rearm_evidence_fault::taken();
        rearm_evidence_fault::arm(0);

        result.expect("a retry reads the evidence");
        assert_eq!(reads, 2);
        assert!(
            release_head_is_live(&doc),
            "{}",
            std::fs::read_to_string(&doc).unwrap()
        );
    }

    /// With no live head the snapshot holds struck, the evidence cannot change
    /// the outcome, so it is not read and cannot block maintenance.
    #[test]
    fn rearm_evidence_is_not_read_when_no_live_head_is_snapshot_struck() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — claude\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#stillopen]\n",
            "- ~~release + publish~~\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        rearm_evidence_fault::arm(u32::MAX);
        let result = run_queue_maintenance(&doc, None);
        let reads = rearm_evidence_fault::taken();
        rearm_evidence_fault::arm(0);

        result.expect("uncontested maintenance never depends on re-arm evidence");
        assert_eq!(reads, 0);
    }

    /// `#retypestruck` (live, agent-doc-bugs.md 2026-10-11 06:20-06:21Z): the
    /// committed snapshot held the answered `~~release + publish~~ — auto-struck
    /// … (#ftstrike)` row. The operator first added an unrelated head (which
    /// advanced the editor-op epoch base past the snapshot), then deleted the
    /// struck row and typed a fresh `- release + publish` at the end of the
    /// queue, meaning "release once the claimed items land". The next preflight
    /// queue maintenance found no exact-base op evidence (epoch base !=
    /// snapshot), treated the new head as a stale re-emit of the snapshot-struck
    /// identity (`#qeditdupguard`) and rewrote it to a bare
    /// `~~release + publish~~`. It happened twice. No response quoted the head.
    ///
    /// The operator's own typed line is base-independent evidence: the head
    /// must stay live through maintenance, a closeout that does not quote it,
    /// and the following maintenance pass.
    #[test]
    fn retyped_head_of_an_answered_identity_stays_live_when_op_base_is_not_the_snapshot() {
        // The live shape: the operator deleted the struck row, then typed.
        assert_retyped_head_survives_maintenance(true);
    }

    /// `#retypestruck` variant: the operator leaves the struck row in place and
    /// appends the new head below it. Once the epoch closes, the snapshot still
    /// holds the struck copy, so the next pass must not drop the live repeat.
    #[test]
    fn retyped_head_below_its_kept_struck_row_stays_live() {
        assert_retyped_head_survives_maintenance(false);
    }

    fn assert_retyped_head_survives_maintenance(delete_struck_row: bool) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: v0.35.488 released — claude\n\n",
            "> **Queue prompt:** release + publish\n\n",
            "Released.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents preset=\"#spec-test-build-install-commit-push\" priority go -->\n",
            "- 🚧 do [#stillopen]\n",
            "- ~~release + publish~~ — auto-struck: answered this cycle (#ftstrike)\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, snapshot_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // An earlier operator edit (a new `#gh-fix` head) already landed, so the
        // active epoch's base is that text, not the snapshot.
        let epoch_base = snapshot_content.replace(
            "- 🚧 do [#stillopen]\n",
            "- 🚧 do [#stillopen]\n- #gh-fix: https://github.com/btakita/agent-doc/issues/238\n",
        );
        assert_ne!(epoch_base, snapshot_content);
        let struck_row = "- ~~release + publish~~ — auto-struck: answered this cycle (#ftstrike)\n";
        let struck_at = epoch_base.find(struck_row).unwrap();
        let (mut ops, mut at) = if delete_struck_row {
            (
                vec![agent_doc_merge::crdt::EditorOp::Delete {
                    offset: struck_at,
                    len: struck_row.len(),
                }],
                struck_at,
            )
        } else {
            (Vec::new(), struck_at + struck_row.len())
        };
        for batch in ["- r", "ele", "ase", " + ", "pub", "lis", "h\n"] {
            ops.push(agent_doc_merge::crdt::EditorOp::Insert {
                offset: at,
                text: batch.to_string(),
            });
            at += batch.len();
        }
        let typed = agent_doc_merge::crdt::replay_editor_ops(&epoch_base, &ops).unwrap();
        assert!(typed.contains("- release + publish\n<!-- /agent:queue -->"));
        // `#remainingwallclock`: record through the retaining path and wait for
        // the controller's acknowledgement, bounded only by a hang ceiling. The
        // strict `record_editor_ops` gives the actor one 250ms reply budget,
        // which a loaded `make check` (load average ~127) missed in setup.
        agent_doc_op_capture_io::record_editor_ops_retaining(
            &doc,
            &agent_doc_hash::content_hash(&epoch_base),
            ops,
        )
        .unwrap();
        assert!(
            agent_doc_op_capture_io::wait_for_retained_editor_ops(
                &doc,
                std::time::Duration::from_secs(60)
            ),
            "the operator's typing burst was never acknowledged"
        );
        std::fs::write(&doc, &typed).unwrap();

        let live_release = |doc: &Path| {
            read_queue_entries(doc).iter().any(|entry| {
                matches!(
                    entry,
                    agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                        if prompt.text.contains("release + publish")
                )
            })
        };

        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            live_release(&doc),
            "queue maintenance struck the operator's re-typed head:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .matches("~~release + publish~~")
                .count(),
            usize::from(!delete_struck_row),
            "the re-typed head must not be rewritten to the struck representative"
        );

        // A closeout that dispatched other work and never quoted it.
        let struck = agent_doc_queue_io::queue_consume::strike_answered_free_text_queue_heads(
            &doc,
            "> **Queue prompt:** do [#stillopen]\n\nDispatched to a subagent.\n",
            true,
            &TEST_QUEUE_CONSUME_WRITE_EFFECTS,
        )
        .unwrap();
        assert_eq!(struck, 0, "an unquoted re-typed head is not an answer");

        // In production the maintenance projection closes the operator's epoch
        // (`editor_op_epoch_closed cause=non_operator_projection`), so the next
        // pass runs with no active op evidence at all. `#remainingwallclock`:
        // close it the way finalize does, deciding from state with the
        // causal-fence budget, not from one 250ms clear reply.
        let consumed = agent_doc_op_capture_io::consume_replayed_editor_op_epoch(&doc);
        assert!(
            matches!(
                consumed,
                agent_doc_op_capture_io::ReplayedEpochConsumption::Consumed { .. }
            ),
            "the operator's epoch must close: {consumed:?}"
        );
        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            live_release(&doc),
            "the second maintenance pass struck the operator's re-typed head:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
    }

    #[test]
    fn run_queue_maintenance_preserves_completed_and_in_progress_heads() {
        // A live closeout committed both lifecycle transitions, then the next
        // maintenance pass rewrote the struck head and the in-progress head
        // back to plain runnable rows. Reproduce the marker shapes so the
        // generic stale-reemit fixture cannot hide a marker-specific regression.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: queue-lifecycle\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- ~~do [#completed-work]~~\n",
            "- 🚧 do [#in-progress-work]\n",
            "- do [#ready-work]\n",
            "- ⏭️ do [#blocked-work]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#in-progress-work] Finish the active task.\n",
            "- [ ] [#ready-work] Finish the ready task.\n",
            "- [ ] [#blocked-work] Finish the blocked task.\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let entries = read_queue_entries(&doc);
        assert!(
            matches!(
                &entries[0],
                agent_doc_queue::document_queue::QueueEntry::Completed(prompt)
                    if prompt.text.contains("#completed-work")
            ),
            "the completed head must remain struck: {entries:?}"
        );
        assert!(
            matches!(
                &entries[1],
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                    if prompt.text.starts_with("🚧 ")
                        && prompt.text.contains("#in-progress-work")
            ),
            "the in-progress head must remain in progress: {entries:?}"
        );

        let completed_node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| node.item.struck && node.item.text.contains("#completed-work"))
            .expect("completed queue head should retain a durable node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let late_selection = agent_doc_state_backbone::StateEvent::new(
            "late-selection-after-authoritative-completion",
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash: document_hash.clone(),
                node_key: completed_node_key.clone(),
                backlog_id: Some("completed-work".to_string()),
                prompt_text: Some("do [#completed-work]".to_string()),
                drainable: true,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event_for_test(
            dir.path(),
            &late_selection,
        )
        .unwrap();
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("completed queue state should project for document");
        let completed_head = projection
            .queue
            .heads
            .get(&completed_node_key)
            .expect("completed queue head should be present in terminal state");
        assert_eq!(
            completed_head.phase,
            agent_doc_state_backbone::QueueHeadPhase::Completed
        );
        assert_ne!(
            projection.queue.active_head.as_deref(),
            Some(completed_node_key.as_str()),
            "a late selection must not reactivate an authoritative completion"
        );
    }

    #[test]
    fn run_queue_maintenance_mirrors_operator_verify_into_queue_but_keeps_it_nondrainable() {
        // #mirrorall (operator directive 2026-06-18): operator-verify backlog items
        // are mirrored INTO the queue (complete worklist) instead of being skipped,
        // but they stay non-drainable — `drainable_head_count` counts only the
        // actionable head, so the in-session auto-drain loop is not re-armed by the
        // mirrored operator-verify head.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#act] actionable work\n",
            "- [ ] [#opv] [operator-verify] needs a human, do not auto-drain\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#act]"),
            "actionable item mirrored into queue:\n{updated}"
        );
        assert!(
            updated.contains("- ⏳ do [#opv]"),
            "operator-verify item must ALSO be mirrored into the queue (#mirrorall), \
             marked awaiting the operator (#queueawaitop):\n{updated}"
        );
        // ...but the operator-verify head must NOT count as drainable: the loop is
        // safe because only the actionable head is countable.
        let drainable = agent_doc_queue::queue_continuation::drainable_head_count(&updated);
        assert_eq!(
            drainable, 1,
            "operator-verify head must be deferred from drainability (#mirrorall keeps the loop \
             safe); only #act should count:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_operator_verify_only_queue_is_not_supervisor_drainable() {
        // `#rt83`: a queue whose only active heads are `[operator-verify]` has no
        // drainer (neither the in-session `/loop` nor the supervisor) — so
        // `queue_supervisor_drainable` must be false. The preflight synthetic
        // queue-head diff gates on this flag, so an operator-verify-only head no
        // longer synthesizes a phantom `+:pushpin: do [#id]` add every preflight
        // (the qchurn flood that kept `no_changes:false` forever).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#opv]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#opv] [operator-verify] needs a human, do not auto-drain\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            !state.queue_supervisor_drainable,
            "operator-verify-only queue must NOT be supervisor-drainable (#rt83): {state:?}"
        );
        assert_eq!(
            state.queue_drainable_head_count, 0,
            "operator-verify head is not in-session drainable either (#rt83): {state:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_focused_cycle_head_is_supervisor_drainable() {
        // `#rt83`: a `[focused-cycle]` head stays supervisor-drainable (the
        // supervisor force-`/clear`s + re-dispatches it), so the synthetic
        // queue-head diff must still fire — suppressing it only for non-drainable
        // heads must NOT strand legitimate supervisor-driven continuation.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#foc]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#foc] [focused-cycle] fix the merge core\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            state.queue_supervisor_drainable,
            "focused-cycle head must stay supervisor-drainable (#rt83): {state:?}"
        );
        assert_eq!(
            state.queue_drainable_head_count, 0,
            "focused-cycle head is deferred for the in-session loop (#qcontdrain): {state:?}"
        );
    }
    #[test]
    fn run_queue_maintenance_enqueue_marker_populates_queue_without_backlog_attr() {
        // #queue-enqueue-action: a single marked backlog item appends to the
        // queue without a component-level `queue` attr. Explicit markers bypass
        // the active-loop fresh-item hold because the user is directly enqueueing
        // that one id.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#running]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] :inbox_tray: queue this now\n",
            "- [ ] [#beta] leave this unqueued\n",
            "- [/] [#gated] :inbox_tray: blocked\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(state.synced_queue_ids, vec!["alpha".to_string()]);
        assert!(
            updated.contains("- 🚧 do [#running]"),
            "running head stays:\n{updated}"
        );
        assert!(
            updated.contains("- 📥 do [#alpha]"),
            "marked item should append:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#beta]"),
            "unmarked item must not append:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#gated]"),
            "gated marked item must not append:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_holds_fresh_backlog_item_out_of_active_queue() {
        // #backlog-queue-sync-pending-add-amplification (decision B/C): a backlog
        // item added while the auto-queue is already running (queue_active: true)
        // must NOT be promoted into the live queue this cycle — it waits for the
        // next activation. Prevents unbounded queue growth + pending_done_guard
        // churn when an agent captures follow-ups mid-loop.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "the already-running head stays:\n{updated}"
        );
        assert!(
            !updated.contains("- do [#beta]"),
            "a freshly-added backlog item must NOT be promoted into the active queue mid-loop:\n{updated}"
        );
        assert!(
            !state.synced_queue_ids.contains(&"beta".to_string()),
            "beta must not be a newly-synced queue id while the loop is active: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_go_mode_repopulates_drained_active_queue() {
        // #backlog-queue-empty-active-repopulate: with the `go` control
        // (`queue: go`, continuous-backlog-loop) and a fully drained live queue
        // (0 un-struck prompts), the amplification hold is skipped and the full
        // active backlog repopulates the queue so the loop keeps working it.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "go-mode must repopulate a drained active queue:\n{updated}"
        );
        assert!(
            updated.contains("- 📥 do [#beta]"),
            "go-mode must repopulate ALL open backlog ids:\n{updated}"
        );
        assert!(
            state.synced_queue_ids.contains(&"alpha".to_string())
                && state.synced_queue_ids.contains(&"beta".to_string()),
            "both ids must be newly synced under go-mode repopulation: {:?}",
            state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_queue_edit_after_drain_stop_runs_the_queue() {
        // #queueeditgo: a drain left `queue: stop`; the operator adds a queue item
        // and invokes Run Agent Doc. Preflight must arm `go` and hand the item to
        // the drain instead of leaving it inert.
        let doc_with = |control: &str, items: &str| {
            format!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n\
                 agent_doc_write: crdt\nqueue: {control}\n---\n\n\
                 <!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n\
                 <!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{items}<!-- /agent:queue -->\n"
            )
        };
        for (control, expect_active) in [("stop", true), ("pause", false)] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let baseline = doc_with(control, "");
            std::fs::write(&doc, &baseline).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                &baseline,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();
            std::fs::write(&doc, doc_with(control, "- fix the thing\n")).unwrap();

            let state = run_queue_maintenance(&doc, None).unwrap();
            let updated = std::fs::read_to_string(&doc).unwrap();

            if expect_active {
                assert_eq!(state.queue_active, Some(true), "{updated}");
                assert!(state.queue_continuation_required, "{updated}");
                assert!(updated.contains("queue: go\n"), "{updated}");
                assert!(updated.contains("<!-- agent:queue go"), "{updated}");
            } else {
                assert_ne!(
                    state.queue_active,
                    Some(true),
                    "pause must hold:\n{updated}"
                );
                assert!(updated.contains("queue: pause\n"), "{updated}");
            }
        }
    }

    #[test]
    fn run_queue_maintenance_controller_pause_surfaces_flag_without_stalling_continuation() {
        // `#qpausego`: an accepted controller `admin queue pause` surfaces
        // `queue_paused` (for visibility + the idle-watch auto-injection guard)
        // but must NOT drop `queue_continuation_required` / drainable head count:
        // the attended in-session `/loop` keeps draining real queue work. Stalling
        // the in-session loop on a pause strands genuine backlog (operator-rejected
        // over-reach). `resume` clears the flag; continuation is unaffected by both.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Baseline: an active go-mode queue with a live head requires continuation.
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            !state.queue_paused,
            "no controller pause means queue_paused is false"
        );
        assert!(
            state.queue_pause_reason.is_none(),
            "no controller pause means no pause reason is surfaced"
        );
        assert!(
            state.queue_continuation_required && state.queue_drainable_head_count > 0,
            "active go queue with a live head must require continuation before pause"
        );

        // Accepted controller pause must be surfaced without halting continuation.
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        let scope_id = doc.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state: "paused",
                reason: Some("operator pause"),
                operation_receipt_id: None,
            },
        )
        .unwrap();

        let paused = run_queue_maintenance(&doc, None).unwrap();
        assert!(paused.queue_paused, "accepted pause must set queue_paused");
        assert_eq!(
            paused.queue_pause_reason.as_deref(),
            Some("operator pause"),
            "accepted pause must surface its recorded reason"
        );
        assert!(
            paused.queue_continuation_required,
            "controller pause must NOT stall the in-session loop continuation"
        );
        assert!(
            paused.queue_drainable_head_count > 0,
            "controller pause must NOT zero the drainable head count for the in-session loop"
        );

        // Resume clears the flag; continuation is unaffected by either state.
        agent_doc_sqlite::state_store::upsert_queue_control_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueControlInsert {
                scope_kind: "document",
                scope_id: &scope_id,
                state: "resumed",
                reason: Some("operator resume"),
                operation_receipt_id: None,
            },
        )
        .unwrap();

        let resumed = run_queue_maintenance(&doc, None).unwrap();
        assert!(!resumed.queue_paused, "resume must clear queue_paused");
        assert!(
            resumed.queue_pause_reason.is_none(),
            "resume must clear the surfaced pause reason"
        );
        assert!(
            resumed.queue_continuation_required && resumed.queue_drainable_head_count > 0,
            "resume keeps continuation for the active go-mode queue"
        );
    }

    #[test]
    fn run_queue_maintenance_go_mode_appends_fresh_backlog_into_nondrained_queue() {
        // #backlog-queue-attr-populates-in-go-mode: with the `go` control and a
        // NON-drained live queue, a freshly-added backlog `queue`-attr item still
        // appends to the queue immediately (the operator opted into the
        // continuous-backlog-loop, so the `queue` attribute must populate it).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let updated_state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#alpha]"),
            "the running head stays:\n{updated}"
        );
        assert!(
            updated.contains("- 📥 do [#beta]"),
            "go-mode must append a fresh backlog `queue`-attr item even when the queue is not drained:\n{updated}"
        );
        assert!(
            updated_state.synced_queue_ids.contains(&"beta".to_string()),
            "beta must be a newly-synced queue id under go-mode: {:?}",
            updated_state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_queue_start_without_go_holds_fresh_backlog() {
        // `queue: start` is the durable active-state spelling for a normal queue
        // run, not a continuous-backlog-loop opt-in. Only explicit `go` should
        // append freshly-added backlog `queue` items into an already-running queue.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority start preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] already running\n",
            "- [ ] [#beta] freshly added mid-loop\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let updated_state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            !updated.contains("do [#beta]"),
            "queue:start without marker/frontmatter go must hold fresh backlog ids:\n{updated}"
        );
        assert!(
            updated_state.synced_queue_ids.is_empty(),
            "non-go queue must not report newly synced ids: {:?}",
            updated_state.synced_queue_ids
        );
    }
    #[test]
    fn run_queue_maintenance_holds_future_not_before_backlog_item_out_of_queue() {
        // #backlog-not-before: a backlog item with a future `not-before=` date
        // precondition is NOT synced into the queue (operator: "if items have
        // preconditions that are not met such as a date in the future, do not add
        // the backlog item into the queue"). A ready item still syncs.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#ready] do now\n",
            "- [ ] [#later] not-before=2999-12-31 scheduled for the future\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            updated.contains("- 🚧 do [#ready]"),
            "the ready item must sync into the queue:\n{updated}"
        );
        assert!(
            !updated.contains("do [#later]"),
            "a future not-before item must be held out of the queue:\n{updated}"
        );
        assert!(
            !state.synced_queue_ids.contains(&"later".to_string()),
            "future-dated id must not be a synced queue id: {:?}",
            state.synced_queue_ids
        );
    }
    /// `#queuemirrororder`: an id recorded as anchored follows its anchor into the
    /// mirror instead of taking the `#queueatcreate` head default.
    #[test]
    fn closeout_sync_places_anchored_same_cycle_add_after_its_anchor() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#registry]\n",
            "- do [#tail]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#registry] phase 1\n",
            "- [ ] [#reasonenum] phase 2, filed after phase 1 on purpose\n",
            "- [ ] [#tail] unrelated\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(&doc, &["reasonenum".to_string()])
            .unwrap();
        agent_doc_cycle_state_io::record_pending_anchored_ids(&doc, &["reasonenum".to_string()])
            .unwrap();

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::default(),
        )
        .unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(synced, vec!["reasonenum".to_string()]);
        let registry = updated.find("do [#registry]").unwrap();
        let reasonenum = updated.find("do [#reasonenum]").unwrap();
        let tail = updated.find("do [#tail]").unwrap();
        assert!(
            registry < reasonenum && reasonenum < tail,
            "an anchored follow-up must land after its anchor, not at the head:\n{updated}"
        );
    }

    #[test]
    fn closeout_sync_prepends_same_cycle_pending_add_in_go_mode() {
        // #pendingaddqueuesync: pending-add writes happen after preflight queue
        // maintenance, so closeout enqueues recorded same-cycle ids once the
        // current queue head has been consumed.
        //
        // `#queueatcreate`: placement defaults to the HEAD. This previously
        // appended, which buried a fresh follow-up behind the whole queue and
        // meant it was effectively never picked up.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(&doc, &["fresh".to_string()])
            .unwrap();

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::default(),
        )
        .unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(synced, vec!["fresh".to_string()]);
        let head = updated.find("do [#head]").unwrap();
        let fresh = updated.find("do [#fresh]").unwrap();
        assert!(
            fresh < head,
            "a same-cycle follow-up must land at the queue head by default:\n{updated}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("- do [#fresh]"),
            "snapshot queue region must include the appended closeout head:\n{snap}"
        );
    }

    /// `#respondcloseoutbacklog`: the same-cycle queue mirror runs once per
    /// closeout owner — the foreground closeout AND the captured-finalize
    /// resume. When the first run's head is already in the live authority but
    /// the disk projection still lags (a retained delivery), the second run
    /// must see the head and add nothing. It used to decide from disk, re-add
    /// the head, and the queue-maintenance CAS then rebased that insert over
    /// the live head and kept both copies.
    #[test]
    fn closeout_sync_decides_already_queued_from_the_live_authority_not_disk() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let disk = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, disk).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            disk,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(disk), Some(disk)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(&doc, &["fresh".to_string()])
            .unwrap();
        // The first owner's mirror already landed in the live authority; its
        // delivery to disk is still retained.
        let live = disk.replace("- do [#head]\n", "- do [#fresh]\n- do [#head]\n");
        let _live_replica = publish_test_live_buffer(&doc, "jetbrains-retained-mirror", &live);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), disk, "precondition");

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::default(),
        )
        .unwrap();

        assert!(
            synced.is_empty(),
            "the head is already queued in the authority: {synced:?}"
        );
        let current = match agent_doc_crdt_relay_io::current_text_for_file(&doc).unwrap() {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("live authority should stay current: {other:?}"),
        };
        assert_eq!(
            current.matches("do [#fresh]").count(),
            1,
            "the second mirror run must not add a second head:\n{current}"
        );
    }

    /// `#backlogqueuepopulation`: a triage closeout that ungates an ordered
    /// batch must populate the explicit queue in the same transaction. The
    /// dependency graph, not mutation argument order, owns the inserted block.
    #[test]
    fn closeout_sync_populates_same_cycle_ungates_in_dependency_order() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: triage — gpt-5\n\nUngated the implementation batch.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- ~do [#triage]~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#c] priority=1 after=#b final phase\n",
            "- [ ] [#b] priority=5 after=#a middle phase\n",
            "- [ ] [#a] priority=9 prerequisite phase\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(
            &doc,
            &["c".to_string(), "b".to_string(), "a".to_string()],
        )
        .unwrap();

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::Prepend,
        )
        .unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(synced, vec!["a", "b", "c"]);
        let a = updated.find("- do [#a]").unwrap();
        let b = updated.find("- do [#b]").unwrap();
        let c = updated.find("- do [#c]").unwrap();
        let triage = updated.find("- ~do [#triage]~").unwrap();
        assert!(
            a < b && b < c && c < triage,
            "ungated items must mirror ahead of the answered triage prompt in dependency order:\n{updated}"
        );
    }

    /// `#queueatcreate`: agents that deliberately do not want a follow-up to
    /// preempt the current drain pass `Append`.
    #[test]
    fn closeout_sync_appends_same_cycle_pending_add_when_placement_is_append() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(&doc, &["fresh".to_string()])
            .unwrap();

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::Append,
        )
        .unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(synced, vec!["fresh".to_string()]);
        let head = updated.find("do [#head]").unwrap();
        let fresh = updated.find("do [#fresh]").unwrap();
        assert!(
            head < fresh,
            "explicit append placement must keep the follow-up behind the head:\n{updated}"
        );
    }

    #[test]
    fn closeout_sync_holds_same_cycle_pending_add_without_go_mode() {
        // The old amplification guard still applies to a plain persisted-active
        // queue: same-cycle captures wait for a later activation unless the
        // operator opted into explicit `go` continuous backlog drain. `queue:
        // start` alone is just the durable active-state spelling.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:queue priority -->\n",
            "- do [#head]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#head] already running\n",
            "- [ ] [#fresh] same-cycle follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_actionable_ids(&doc, &["fresh".to_string()])
            .unwrap();

        let synced = sync_same_cycle_actionable_backlog_into_go_queue(
            &doc,
            agent_doc_queue::backlog_sync::FollowUpQueuePlacement::default(),
        )
        .unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(synced.is_empty());
        assert!(
            !updated.contains("do [#fresh]"),
            "non-go active queue must not append same-cycle pending add:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_no_go_keeps_drain_then_stop_on_empty_active_queue() {
        // #backlog-queue-empty-active-repopulate: WITHOUT the `go` control, a
        // drained persisted-active queue stays drained (drain-then-stop). The
        // amplification hold drops every backlog id because none are already
        // live queue heads, so nothing repopulates.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "- [ ] [#beta] second\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();

        assert!(
            !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
            "without `go`, a drained active queue must stay drained:\n{updated}"
        );
        assert!(
            state.synced_queue_ids.is_empty(),
            "no ids may be synced into a drained active queue without `go`: {:?}",
            state.synced_queue_ids
        );
        // #queuestopremove: even a drain that held backlog ids out clears the
        // control — there is no `stop` hold. The backlog opted into mirroring
        // (`queue=append`), so the next cycle mirrors and runs them.
        assert!(
            !updated.contains("queue:"),
            "a drain clears the control, never writes stop:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_legacy_queue_stop_with_head_runs_and_drops_stop() {
        // #queuestopremove: the lazily.md shape from 2026-10-03 — a binary-written
        // `queue: stop` left `do [#lzwiremodel]` waiting with no visible reason.
        // `stop` is retired: the legacy line is no control, the head runs in the
        // default `go`, and the next write removes the `stop` line.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue preset=\"#spec-test-commit-push\" priority -->\n",
            "- do [#lzwiremodel]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_active, Some(true), "the waiting head must run");
        assert_eq!(state.queue_prompts, vec!["do [#lzwiremodel]".to_string()]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("stop"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_drain_clears_control_and_added_head_runs() {
        // #queuestopretire: a DRAIN (no live heads, nothing held from the
        // backlog) clears the queue control instead of writing `queue: stop`, so
        // the stale-stop state behind GH #79 never exists. The cleared queue is
        // idle, and a head the operator adds later runs in the default `go`.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();
        let drained = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !drained.contains("queue: stop") && !drained.contains("queue: start"),
            "a drain must clear the control, not write stop:\n{drained}"
        );
        let (fm, _) = frontmatter::parse(&drained).unwrap();
        assert_eq!(fm.queue, None, "{drained}");
        assert_eq!(fm.queue_active, None, "{drained}");

        // Idle: a second pass over the empty, control-less queue writes nothing.
        let idle = run_queue_maintenance(&doc, None).unwrap();
        assert_ne!(
            idle.queue_active,
            Some(true),
            "an empty queue with no control is idle"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), drained);

        // The operator adds a head: it runs in the default `go`.
        let with_head = drained.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:queue -->\n- do the next thing\n",
        );
        std::fs::write(&doc, &with_head).unwrap();
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_active,
            Some(true),
            "an added head runs with no stale stop"
        );
        let rearmed = std::fs::read_to_string(&doc).unwrap();
        assert!(rearmed.contains("queue: go"), "{rearmed}");
    }

    #[test]
    fn run_queue_maintenance_drain_strips_marker_activation_token() {
        // GH #292: binding convergence copies a lone marker `go`/`start` into
        // frontmatter before settled maintenance. Draining must clear both
        // surfaces in the same pass or every replica update sees normalization
        // pending forever.
        for token in ["go", "start"] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let content = format!(
                concat!(
                    "---\nagent_doc_session: test\nagent_doc_format: template\n",
                    "agent_doc_write: crdt\n---\n\n",
                    "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue priority {} -->\n<!-- /agent:queue -->\n",
                ),
                token,
            );
            std::fs::write(&doc, &content).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                &content,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();

            run_queue_maintenance(&doc, None).unwrap();
            let drained = std::fs::read_to_string(&doc).unwrap();
            let (fm, _) = frontmatter::parse(&drained).unwrap();
            assert_eq!(fm.queue, None, "{drained}");
            assert_eq!(fm.queue_active, None, "{drained}");
            assert!(
                drained.contains("<!-- agent:queue priority -->"),
                "drain must strip marker `{token}` together with frontmatter:\n{drained}"
            );
            assert!(
                !drained.contains(&format!("agent:queue {token}")),
                "drain left marker `{token}` behind:\n{drained}"
            );

            let idle = run_queue_maintenance(&doc, None).unwrap();
            assert_ne!(idle.queue_active, Some(true), "{drained}");
            assert_eq!(std::fs::read_to_string(&doc).unwrap(), drained);
        }
    }

    #[test]
    fn run_queue_maintenance_stop_marker_halt_with_heads_holds_with_pause() {
        // #queuestopremove: a legacy `stop` MARKER gesture with heads left is
        // the operator's hold; it persists as the one visible hold, `pause`.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue stop -->\n",
            "- do the remaining thing\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        let halted = std::fs::read_to_string(&doc).unwrap();
        assert_ne!(state.queue_active, Some(true));
        assert!(halted.contains("queue: pause"), "{halted}");
        assert!(!halted.contains("stop"), "{halted}");
        assert!(halted.contains("- 📥 do the remaining thing"), "{halted}");
    }
    #[test]
    fn run_queue_maintenance_no_warning_when_queue_already_synced() {
        // When the queue already matches the backlog, no backlog_queue_sync_pending
        // warning should fire (sync_backlog_into_queue returns None → no warning path).
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "backlog_queue_sync_pending"),
            "already-synced queue must NOT emit backlog_queue_sync_pending warning, got {:?}",
            state.warnings
        );
    }
    #[test]
    fn run_queue_maintenance_marker_go_activates_like_auto() {
        // #queue-state-unify: a `go`/`start` marker control freshly activates the
        // queue through the Auto trigger, identical to the legacy `auto` attribute.
        for token in ["go", "start"] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let content = format!(
                concat!(
                    "---\nagent_doc_session: test\nagent_doc_format: template\n",
                    "agent_doc_write: crdt\n---\n\n",
                    "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue {} -->\n- please do the thing\n<!-- /agent:queue -->\n",
                ),
                token
            );
            std::fs::write(&doc, &content).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                &content,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();

            let state = run_queue_maintenance(&doc, None).unwrap();
            assert_eq!(
                state.queue_active,
                Some(true),
                "marker `{token}` must activate the queue"
            );
            assert_eq!(
                state.queue_trigger,
                Some(agent_doc_queue::document_queue::QueueTrigger::Auto)
            );
            let updated = std::fs::read_to_string(&doc).unwrap();
            let expected_queue = if token == "go" {
                "queue: go"
            } else {
                "queue: start"
            };
            assert!(
                updated.contains(expected_queue),
                "marker `{token}` must persist queue_active:\n{updated}"
            );
        }
    }
    #[test]
    fn run_queue_maintenance_marker_stop_halts_active_queue() {
        // #queue-state-unify: a `stop` marker control forces an otherwise-active
        // queue inactive and clears persisted queue_active.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue stop -->\n- please do the thing\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_active,
            Some(false),
            "marker `stop` must halt the active queue"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: pause"),
            "a marker `stop` gesture holds as `queue: pause` (#queuestopremove):\n{updated}"
        );
        assert!(
            !updated.contains("agent:queue stop"),
            "marker `stop` token must be stripped after halt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_removed_marker_go_clears_to_default_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content =
            snapshot_content.replace("<!-- agent:queue go -->", "<!-- agent:queue -->");
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        // #queuestopremove: removing the token clears the control; no control
        // is the default `go`, so the head keeps running. Hold with `pause`.
        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(!updated.contains("queue: stop"), "{updated}");
        assert!(!updated.contains("agent:queue go"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_frontmatter_go_adds_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: pause\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: pause", "queue: go");
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"), "{updated}");
        assert!(updated.contains("<!-- agent:queue go -->"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_frontmatter_pause_removes_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: go", "queue: pause");
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(false));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: pause"), "{updated}");
        assert!(updated.contains("<!-- agent:queue -->"), "{updated}");
        assert!(!updated.contains("agent:queue go"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_stop_fence_records_typed_deferred_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n--- stop\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_halted.as_deref(), Some("stop_fence"));
        assert_eq!(state.queue_active, Some(false));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("--- stop"),
            "stop fence should be consumed from halted queue:\n{updated}"
        );
        let node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("deferred queue head should retain a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("deferred queue state should project for document");
        assert_eq!(projection.queue.active_head, None);
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("deferred queue head should be present in projection");
        assert_eq!(
            head.phase,
            agent_doc_state_backbone::QueueHeadPhase::Deferred
        );
        assert_eq!(head.backlog_id.as_deref(), Some("alpha"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#alpha]"));
        assert_eq!(head.defer_reason.as_deref(), Some("stop_fence"));
        assert!(!head.drainable);

        record_selected_queue_head_state(&doc, &updated, "do [#alpha]", true).unwrap();
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("reselected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("reselected queue head should be present in projection");
        assert_eq!(
            head.phase,
            agent_doc_state_backbone::QueueHeadPhase::Selected
        );
        assert_eq!(head.defer_reason, None);
        assert!(head.drainable);
    }

    #[test]
    fn run_queue_maintenance_open_backlog_incarnation_overrides_done_history() {
        // #qrepeatid: `agent:done` is historical. A live `[ ]` backlog item is
        // the current incarnation even when it deliberately reuses an archived
        // id, so both it and an unrelated fresh id remain queueable.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#na3x] completed-but-lingering\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-06-01 [#na3x] completed-but-lingering\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("do [#na3x]"),
            "open repeated incarnation must be queued despite done history:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
        assert_eq!(
            state.synced_queue_ids,
            vec!["na3x".to_string(), "fresh".to_string()]
        );
    }

    #[test]
    fn run_queue_maintenance_preserves_repeated_completed_id_incarnation() {
        // #qrepeatid: the operator may deliberately repeat the same queue work.
        // Free-text admission converts the prompt to `do [#fixopenissues]`; an
        // older done entry with that id must not strike the newly-added
        // incarnation or suppress its backlog mirror. The live backlog item is
        // the durable incarnation boundary across maintenance retries.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-08-26 [#fixopenissues] Please fix all open issues on tsift\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let repeated = baseline.replace(
            "<!-- agent:queue priority go -->\n<!-- /agent:queue -->",
            concat!(
                "<!-- agent:queue priority go -->\n",
                "- Please fix all open issues on tsift\n",
                "<!-- /agent:queue -->",
            ),
        );
        let repeated = repeated.replace(
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->",
            concat!(
                "<!-- agent:backlog -->\n",
                "- [ ] [#fixopenissues] Please fix all open issues on tsift\n",
                "<!-- /agent:backlog -->",
            ),
        );
        std::fs::write(&doc, repeated).unwrap();

        let first = run_queue_maintenance(&doc, None).unwrap();
        assert!(
            first
                .synced_queue_ids
                .iter()
                .any(|id| id == "fixopenissues"),
            "fresh repeat must be admitted into the queue: {first:?}"
        );
        for _ in 0..2 {
            let entries = read_queue_entries(&doc);
            assert!(
                entries.iter().any(|entry| matches!(
                    entry,
                    agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
                        if prompt.text.contains("do [#fixopenissues]")
                )),
                "repeated id must remain live rather than auto-struck: {entries:?}"
            );
            assert!(
                entries.iter().all(|entry| !matches!(
                    entry,
                    agent_doc_queue::document_queue::QueueEntry::Completed(prompt)
                        if prompt.text.contains("fixopenissues")
                )),
                "historical done id must not complete the new incarnation: {entries:?}"
            );
            // A retry before closeout still compares against the old committed
            // baseline and must preserve the same fresh incarnation.
            let _ = run_queue_maintenance(&doc, None).unwrap();
        }
    }
    #[test]
    fn run_queue_maintenance_open_incarnation_overrides_external_done_history() {
        // #qrepeatid (external-archive variant): external `agent:done` history
        // has the same semantics as inline history. A live `[ ]` backlog item
        // makes the reused id a current incarnation, so its queue head remains
        // executable instead of being auto-struck.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "session.done.md";
        std::fs::write(
            dir.path().join(archive_rel),
            "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#extdone]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#extdone] lingering active dup of an externally-archived id\n",
            "- [ ] [#fresh] genuinely open\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("do [#extdone]") && !updated.contains("~~do [#extdone]~~"),
            "open incarnation must override externally archived done history:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_strikes_external_archive_done_queue_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(
            dir.path().join("session.done.md"),
            "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#extdone]\n",
            "- do [#fresh]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~~do [#extdone]~~"),
            "externally-archived live queue mirror must be struck:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]") && !updated.contains("~~do [#fresh]~~"),
            "fresh live queue prompt must remain:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_backlog_sync_is_idempotent() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=append -->\n",
            "- [ ] [#alpha] first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            state.synced_queue_ids.is_empty(),
            "idempotent sync should not report freshly-added ids"
        );
        assert_eq!(
            updated.matches("do [#alpha]").count(),
            1,
            "append must not duplicate an already-queued id:\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_records_only_newly_synced_ids() {
        // The existing queue head must stay outside the synced-id exclusion set
        // so pending_done_guard still requires the consumed `do [#worked]` item
        // to be done/gated.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#worked]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=prepend -->\n",
            "- [ ] [#worked] the real queue head\n",
            "- [ ] [#alpha] freshly synced\n",
            "- [ ] [#beta] freshly synced\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.synced_queue_ids,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        let open_backlog: std::collections::HashSet<String> = ["worked", "alpha", "beta"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let synced_queue_ids = state
            .synced_queue_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<String>>();
        let result = agent_doc_queue::queue_directive::filter_expect_done_or_gate_ids(
            &[
                "worked".to_string(),
                "alpha".to_string(),
                "beta".to_string(),
            ],
            &open_backlog,
            &synced_queue_ids,
        );
        assert_eq!(result, vec!["worked".to_string()]);
    }
    #[test]
    fn run_queue_maintenance_backlog_queue_priority_sorts_without_marking_promoted_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue auto -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync priority -->\n",
            "- [ ] [#slow] slower follow-up priority=9\n",
            "- [ ] [#fast] fast follow-up priority=1\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#fast]\n- 📥 do [#slow]"),
            // `#pinoperatoronly`: priority sorting reorders; it does not pin.
            // A marker in the queue now means the operator put it there.
            "backlog `queue priority` must sort synced queue prompts by position, \
             without injecting an agent pin marker:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_marks_current_queue_and_work_items_in_progress() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "- 🚧 do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] active work\n",
            "- [ ] 🚧 [#beta] stale work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] 🚧 [#cold] stale parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_prompts[0], "do [#alpha]");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#alpha]\n- 📥 do [#beta]"),
            "queue marker must move to active head:\n{updated}"
        );
        assert!(updated.contains("- [ ] 🚧 [#alpha] active work"));
        assert!(updated.contains("- [ ] [#beta] stale work"));
        assert!(updated.contains("- [ ] [#cold] stale parked work"));

        let node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("active queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&node_key)
            .expect("selected queue head should be present in projection");
        assert_eq!(
            head.phase,
            agent_doc_state_backbone::QueueHeadPhase::Selected
        );
        assert_eq!(head.backlog_id.as_deref(), Some("alpha"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#alpha]"));
        assert!(head.drainable);
        assert!(projection.queue.worklist_active);
        assert_eq!(projection.queue.worklist.len(), 2);
        assert_eq!(
            projection.queue.worklist[0].kind,
            agent_doc_state_backbone::QueueWorklistEntryKind::Prompt
        );
        assert_eq!(projection.queue.worklist[0].text, "do [#alpha]");
        assert_eq!(
            projection.queue.worklist[1].kind,
            agent_doc_state_backbone::QueueWorklistEntryKind::Prompt
        );
        assert_eq!(projection.queue.worklist[1].text, "do [#beta]");
        assert!(projection.queue.worklist_queue_hash.is_some());
    }

    /// `#claimmarker`: every live worker claim mirrors its exact lifecycle in
    /// both the queue and tracked-work row. Release clears the projection;
    /// expiry retains the typed lost state rather than stale `🚧`.
    #[test]
    fn run_queue_maintenance_syncs_claim_lifecycle_into_tracked_work() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#a]\n",
            "- do [#b]\n",
            "- do [#c]\n",
            "- do [#d]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] inline work\n",
            "- [ ] [#b] worker one\n",
            "- [ ] [#c] worker two\n",
            "- [ ] [#d] later work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "#b", "subagent:b", 3600).unwrap();
        let (_, c_claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "#c", "subagent:c", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "#c",
            "subagent:c",
            "worker:c",
            3600,
            c_claim.generation,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.selected_queue_prompts, vec!["do [#a]".to_string()]);
        assert_eq!(state.queue_drainable_head_count, 2);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#a]\n- 🟡 [#subagent:b#1] do [#b]\n- 🚧 [#subagent:c#1] do [#c]\n- 📥 do [#d]"),
            "selected, claimed, and running queue heads must carry typed state:\n{updated}"
        );
        assert!(updated.contains("- [ ] 🚧 [#a] inline work"));
        assert!(updated.contains("- [ ] 🟡 [#b] worker one"));
        assert!(updated.contains("- [ ] 🚧 [#c] worker two"));
        assert!(updated.contains("- [ ] [#d] later work"));

        agent_doc_queue_io::queue_claim::report_attempt(&doc, "#c", Some(c_claim.generation))
            .unwrap();
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.selected_queue_prompts, vec!["do [#a]".to_string()]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("- ☑️ [#subagent:c#1] do [#c]"));
        assert!(
            updated.contains("- [ ] ☑️ [#c] worker two"),
            "reported work must replace stale running state in backlog:\n{updated}"
        );
        assert!(!updated.contains("- [ ] 🚧 [#c] worker two"));

        agent_doc_queue_io::queue_claim::release(&doc, "#b").unwrap();
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.selected_queue_prompts, vec!["do [#a]".to_string()]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated
                .contains("- 🚧 do [#a]\n- 📥 do [#b]\n- ☑️ [#subagent:c#1] do [#c]\n- 📥 do [#d]")
        );
        assert!(updated.contains("- [ ] [#b] worker one"));
        assert!(updated.contains("- [ ] ☑️ [#c] worker two"));

        let (_, d_claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "#d", "subagent:d", 3600).unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "#d",
            "subagent:d",
            "worker:d",
            3600,
            d_claim.generation,
        )
        .unwrap();
        expire_queue_claims(dir.path(), &doc);
        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.selected_queue_prompts, vec!["do [#a]".to_string()]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains(
            "- 🚧 do [#a]\n- 📥 do [#b]\n- ☑️ [#subagent:c#1] do [#c]\n- ⚠️ [#subagent:d#1] do [#d]"
        ));
        assert!(updated.contains("- [ ] ☑️ [#c] worker two"));
        assert!(updated.contains("- [ ] ⚠️ [#d] later work"));
    }

    #[test]
    fn run_queue_maintenance_marks_first_in_session_drainable_head_in_progress() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#verify]\n",
            "- do [#focused]\n",
            "- do [#ready]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#verify] [operator-verify] needs a human\n",
            "- [ ] [#focused] [focused-cycle] needs a clean turn\n",
            "- [ ] [#ready] active work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_drainable_head_count, 1);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ⏳ do [#verify]\n- 📥 do [#focused]\n- 🚧 do [#ready]"),
            "in-progress marker must project the first in-session drainable head \
             (`#queueawaitop`: the operator-verify head shows ⏳, `[focused-cycle]` none):\n{updated}"
        );
        assert!(updated.contains("- [ ] [#verify] [operator-verify] needs a human"));
        assert!(updated.contains("- [ ] [#focused] [focused-cycle] needs a clean turn"));
        assert!(updated.contains("- [ ] 🚧 [#ready] active work"));

        let ready_node_key = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue")
            .unwrap()
            .into_iter()
            .find(|node| node.item.text.contains("#ready"))
            .expect("ready queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(ready_node_key.as_str())
        );
        let head = projection
            .queue
            .heads
            .get(&ready_node_key)
            .expect("ready queue head should be present in projection");
        assert_eq!(head.backlog_id.as_deref(), Some("ready"));
        assert_eq!(head.prompt_text.as_deref(), Some("do [#ready]"));
        assert!(head.drainable);
    }

    /// `#stalemarkerretarget`: maintenance projected `🚧` onto `release + publish`
    /// while it was the only live head. The operator then queued two `do` heads
    /// above it, and the baseline was rebuilt from a HEAD that predates the
    /// projection, so the preflight diff showed the marker as freshly added. The
    /// binary's own stale marker must not pin selection: the first head wins and
    /// the marker moves to it.
    #[test]
    fn run_queue_maintenance_moves_stale_binary_marker_to_new_first_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let header = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
        );
        let only_head = format!("{header}- release + publish\n<!-- /agent:queue -->\n");
        std::fs::write(&doc, &only_head).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &only_head,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&only_head), Some(&only_head))
            .unwrap();
        run_queue_maintenance(&doc, None).unwrap();
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("- 🚧 release + publish"),
            "the sole head is projected in progress first"
        );

        // Baseline rebuilt from HEAD (no marker); operator queued heads above.
        let edited = format!(
            "{header}- do [#a]\n- do [#b]\n- 🚧 release + publish\n<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, &edited).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &only_head,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let diff = "-- release + publish\n+- do [#a]\n+- do [#b]\n+- 🚧 release + publish\n";
        run_queue_maintenance(&doc, Some(diff)).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#a]\n- 📥 do [#b]\n- 📥 release + publish\n"),
            "a stale binary-projected marker must move to the operator's first head:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_honors_marker_retarget_with_auto_dag_prerequisites() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#ops]\n",
            "- 🚧 do [#ship]\n",
            "- do [#setup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#ops] priority=9 independent work\n",
            "- [ ] [#ship] priority=2 after=#setup selected dependent work\n",
            "- [ ] [#setup] priority=1 prerequisite work\n",
            "<!-- /agent:backlog -->\n",
        );
        let snapshot_content = content.replace(
            "- do [#ops]\n- 🚧 do [#ship]",
            "- 🚧 do [#ops]\n- do [#ship]",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let diff = concat!(
            "@@ queue @@\n",
            "-- 🚧 do [#ops]\n",
            "+- do [#ops]\n",
            "-- do [#ship]\n",
            "+- 🚧 do [#ship]\n",
        );
        let state = run_queue_maintenance(&doc, Some(diff)).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.selected_queue_prompts,
            vec!["do [#setup]".to_string(), "do [#ship]".to_string()]
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#setup]\n- 🚧 do [#ship]\n- 📥 do [#ops]"),
            "operator retarget must project the selected head and its auto-DAG prerequisite as active:\n{updated}"
        );
        assert!(updated.contains("- [ ] 🚧 [#setup] priority=1 prerequisite work"));
        assert!(
            updated.contains("- [ ] 🚧 [#ship] priority=2 after=#setup selected dependent work")
        );
        assert!(updated.contains("- [ ] [#ops] priority=9 independent work"));

        let node_keys = agent_doc_markdown_ast::mutations::item_nodes(&updated, "queue").unwrap();
        let setup_node_key = node_keys
            .iter()
            .find(|node| node.item.text.contains("#setup"))
            .expect("setup queue head should have a node key")
            .node_key
            .clone();
        let ship_node_key = node_keys
            .iter()
            .find(|node| node.item.text.contains("#ship"))
            .expect("ship queue head should have a node key")
            .node_key
            .clone();
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let ledger =
            agent_doc_controller_io::project_controller::load_state_event_ledger(dir.path())
                .unwrap();
        let projection = ledger
            .project_document(&document_hash)
            .expect("selected queue state should project for document");
        assert!(projection.queue.active_heads.contains(&setup_node_key));
        assert!(projection.queue.active_heads.contains(&ship_node_key));
        assert_eq!(
            projection.queue.active_head.as_deref(),
            Some(ship_node_key.as_str())
        );
    }

    #[test]
    fn run_queue_maintenance_removes_in_progress_from_completed_items() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#alpha]\n",
            "- ~~🚧 do [#done]~~\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] active work\n",
            "- [x] 🚧 [#done] finished work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [x] 🚧 [#reviewdone] finished review\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] 🚧 [#cold] finished parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_prompts[0], "do [#alpha]");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 do [#alpha]\n- ~~do [#done]~~"),
            "queue marker must move to the active head and clear struck items:\n{updated}"
        );
        assert!(
            updated.contains("- [ ] 🚧 [#alpha] active work"),
            "active backlog items must be marked:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#done] finished work"),
            "done backlog items must not keep in-progress markers:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#reviewdone] finished review"),
            "done review items must not keep in-progress markers:\n{updated}"
        );
        assert!(
            updated.contains("- [x] [#cold] finished parked work"),
            "done icebox items must not keep in-progress markers:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_pins_operator_moved_priority_queue_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority auto -->\n",
            "- do [#fast]\n",
            "- do [#medium]\n",
            "- do [#slow]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#fast] priority=1 first by rank\n",
            "- [ ] [#medium] priority=5 middle by rank\n",
            "- [ ] [#slow] priority=9 operator moved this up\n",
            "<!-- /agent:backlog -->\n",
        );
        let current_content = snapshot_content.replace(
            "- do [#fast]\n- do [#medium]\n- do [#slow]",
            "- do [#slow]\n- do [#fast]\n- do [#medium]",
        );
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- 🚧 📌 do [#slow]\n- 📥 do [#fast]\n- 📥 do [#medium]"),
            "operator-moved queue prompt should become sticky with an operator pin (📌):\n{updated}"
        );
    }
    #[test]
    fn run_queue_maintenance_auto_dag_intersperses_blocker_with_pinned_batch() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority auto -->\n",
            "- :pushpin: do [#ops]\n",
            "- :pushpin: do [#ship]\n",
            "- :pushpin: do [#notify]\n",
            "- :round_pushpin: do [#setup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#ops] priority=5 independent operator-pinned task\n",
            "- [ ] [#ship] priority=1 after=#setup depends on setup\n",
            "- [ ] [#notify] priority=2 after=#ship depends on ship\n",
            "- [ ] [#setup] priority=9 required setup work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(
                "- 🚧 :pushpin: do [#ops]\n\
                 - 📥 :round_pushpin: do [#setup]\n\
                 - ⛔ ⟦blocked by setup⟧ :pushpin: do [#ship]\n\
                 - ⛔ ⟦blocked by ship⟧ :pushpin: do [#notify]"
            ),
            "auto-dag must let dependency blockers intersperse a pinned batch:\n{updated}"
        );
    }
    #[test]
    fn preflight_new_auto_queue_from_inactive_snapshot_does_not_halt_on_changed_head() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: pause\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#oldhead]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#newhead] Run the newly queued head.\n",
            "- [ ] [#nexthead] Run the next queued item.\n",
            "<!-- /agent:backlog -->\n"
        );
        let current_content = snapshot_content
            .replace("<!-- agent:queue -->", "<!-- agent:queue auto -->")
            .replace("- do [#oldhead]", "- do [#newhead]\n- do [#nexthead]");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_halted, None);
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()]
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("<!-- agent:queue auto start -->"));
        assert!(updated.contains("- 🚧 do [#newhead]"));
        assert!(!updated.contains("- do [#oldhead]"));

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("queue: start")
                && snap.contains("<!-- agent:queue auto start -->")
                && snap.contains("do [#newhead]")
                && !snap.contains("- do [#oldhead]"),
            "newly activated queue must be snapshotted as the closeout baseline:\n{snap}"
        );

        let done_ids = vec!["newhead".to_string()];
        let outcome =
            agent_doc_queue_io::queue_consume::consume_queue_prompts_for_done_ids_force_disk_with_outcome(
                &doc,
                &done_ids,
                &TEST_QUEUE_CONSUME_WRITE_EFFECTS,
            )
        .unwrap()
        .expect("newly activated queue head should be consumable");
        assert_eq!(outcome.consumed_count, 1);
        assert_eq!(outcome.remaining, 1);

        let consumed = std::fs::read_to_string(&doc).unwrap();
        assert!(consumed.contains("- ~~do [#newhead]~~"));
        assert!(consumed.contains("- 📥 do [#nexthead]"));
    }
    #[test]
    fn queue_maintenance_drains_all_done_queue_without_item_modified_halt() {
        // #drained-done-queue-clear: a fully resolved auto-queue (every `do
        // [#id]` already in agent:done) plus a batch dispatch directive must
        // drain — not false-halt as `item_modified`. Before the fix the
        // strike pass converted every live head to Completed, leaving the
        // post-strike head `None` vs a still-live snapshot head, which
        // detect_head_prompt_modified read as an edit and halted before the
        // drain-cleanup path ran. The Corky live-repro shape: template doc,
        // dispatch preset, multiple bracketed `do [#id]` prompts, no diff.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "- [x] [#beta] Second done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "fully-resolved queue must drain, not halt as item_modified"
        );
        assert_eq!(state.queue_active, Some(false));
        assert!(state.queue_prompts.is_empty());

        let updated = std::fs::read_to_string(&doc).unwrap();
        // #queuestopretire: a drain clears the control instead of writing stop.
        assert!(
            !updated.contains("queue: stop") && !updated.contains("queue_active:"),
            "file: {updated}"
        );
        assert!(
            !updated.contains("agent:queue auto"),
            "auto must be stripped on drain: {updated}"
        );
        assert!(
            !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
            "drained queue body must be cleared: {updated}"
        );

        // Snapshot matches the drained file so the closeout commit boundary
        // does not strand the maintenance mutation.
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(!snap.contains("queue: stop") && !snap.contains("queue_active:"));
        assert!(!snap.contains("agent:queue auto"));
        assert!(!snap.contains("- do [#alpha]"));
    }

    #[test]
    fn queue_maintenance_keeps_a_drained_loop_queue_struck_rows_for_the_restore() {
        // GH #327 pass 4: one maintenance write struck the loop queue's last
        // live head and then blanked the drained body, so the scheduler saw the
        // item neither live nor struck and completed the pass with
        // `restore_items=0`. The same strike-then-drain shape as the sibling
        // all-done test above, on a `loop` queue: the struck rows must stay (the
        // loop restore removes them), in the document and the snapshot.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto loop -->\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "- [x] [#beta] Second done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        for (label, text) in [("document", &updated), ("snapshot", &snap)] {
            let queue_id = agent_doc_queue::queue_schedule::identified_queue_schedules(text)
                .unwrap()
                .into_iter()
                .next()
                .expect("the loop queue is still scheduled")
                .queue_id;
            assert!(
                agent_doc_queue::queue_schedule::live_heads_for_queue(text, &queue_id)
                    .unwrap()
                    .is_empty(),
                "{label}: both heads were struck: {text}"
            );
            assert_eq!(
                agent_doc_queue::queue_schedule::completed_heads_for_queue(text, &queue_id)
                    .unwrap(),
                vec!["do [#alpha]".to_string(), "do [#beta]".to_string()],
                "{label}: the drain kept the loop queue's struck rows: {text}"
            );
        }
    }

    #[test]
    fn queue_maintenance_never_clears_a_drained_body_over_operator_text() {
        // `#queuelineclobber`, the live `src/haiven-dev/tasks/infra.md` shape:
        // the operator types a `re [#id]: <question>` line straight into the
        // queue. It parses as `Freeform` — deliberately not an actionable prompt
        // — so the drain path saw "no prompts", called the queue drained, and
        // blanked the WHOLE body, deleting a question nothing else held.
        //
        // A genuinely drained body must still clear (the sibling test above), so
        // the assertion here is specifically that residue clears AROUND operator
        // text while the operator text stays.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let operator_line = "- re [#alpha]: Since we moved over to a Secret Bearer Token \
                             between the fpe + backend, is this still applicable?";
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "queue_active: true\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\n",
                "Done.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue auto -->\n",
                "- do [#alpha]\n",
                "{operator_line}\n",
                "<!-- /agent:queue -->\n\n",
                "## Done\n\n",
                "<!-- agent:done -->\n",
                "- [x] [#alpha] First done.\n",
                "<!-- /agent:done -->\n"
            ),
            operator_line = operator_line,
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(operator_line),
            "operator queue text must survive a drain clear: {updated}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains(operator_line),
            "the snapshot must keep it too, or the next preflight re-drops it: {snap}"
        );
    }

    #[test]
    fn queue_maintenance_clears_completed_item_with_indented_continuation() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- ~~Make the compact error human-readable:~~\n",
            "  command failed (exit 1): malformed directive\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let components = agent_doc_element::element::parse(&updated).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap();
        assert!(queue.content(&updated).trim().is_empty(), "{updated}");
        assert!(!queue.attrs.contains_key("auto"), "{updated}");
        assert!(!updated.contains("queue_active: true"), "{updated}");

        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, updated);
    }

    #[test]
    fn queue_maintenance_preserves_operator_verdict_for_gated_review_head() {
        // `#qgateverdict`: the queue item is the operator's answer to the gate.
        // It must be dispatched before the still-gated review row can change.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue priority go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#fpetrainpush] Push the restacked fpe train. Gated on: the push hold lifting.\n",
            "<!-- /agent:review -->\n",
        );
        let current = baseline.replace(
            "<!-- agent:queue priority go -->\n",
            concat!(
                "<!-- agent:queue priority go -->\n",
                "- complete [#fpetrainpush]: I lift the push hold for the restack: Do the restack + push\n",
            ),
        );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.queue_prompts,
            vec!["complete [#fpetrainpush]: I lift the push hold for the restack: Do the restack + push".to_string()],
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("complete [#fpetrainpush]: I lift the push hold"),
            "the gate-lifting verdict must remain executable:\n{updated}",
        );
        assert!(!updated.contains("~~complete [#fpetrainpush]"));
        assert_eq!(
            agent_doc_queue::queue_continuation::drainable_head_count(&updated),
            1,
            "the preserved gate-lifting verdict must be selected as executable work",
        );
    }
    #[test]
    fn queue_maintenance_partial_done_strike_advances_to_live_head_without_halt() {
        // #drained-done-queue-clear (partial case): a leading queue head that
        // is already done must be struck and the queue advanced to the next
        // live head — without false-halting as item_modified. The snapshot is
        // struck the same way before the head-modified comparison so only a
        // genuine operator head edit can halt.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#alpha] First done.\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "striking a done head must not halt while a live head remains"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_prompts, vec!["do [#beta]".to_string()]);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~~do [#alpha]~~"),
            "done head struck to completed: {updated}"
        );
        assert!(updated.contains("- 🚧 do [#beta]"));
        assert!(updated.contains("agent:queue auto"));
        assert!(updated.contains("queue: start"));
    }

    /// GH #325 end to end: a head whose id lives only in the bare-`archive`
    /// default archive is struck exactly like an inline `agent:done` entry, and
    /// an id that lives only in the 0.35.521–0.35.523 misplaced archive is not.
    #[test]
    fn queue_maintenance_strikes_head_archived_in_bare_default_archive() {
        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join("tasks/a/tasks/a")).unwrap();
        let doc = dir.path().join("tasks/a/target.md");
        std::fs::write(
            dir.path().join("tasks/a/target.done.md"),
            "# Agent Doc Completed Work\n\n- 2026-10-19 [#alpha] First done.\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tasks/a/tasks/a/target.done.md"),
            "# Agent Doc Completed Work\n\n- 2026-10-19 [#beta] Misplaced.\n",
        )
        .unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "- do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "## Done\n\n",
            "<!-- agent:done archive -->\n",
            "<!-- completed work archived in target.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_halted, None);
        assert_eq!(state.queue_prompts, vec!["do [#beta]".to_string()]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("- ~~do [#alpha]~~"),
            "a head archived in the default archive must be struck: {updated}"
        );
        assert!(
            updated.contains("- 🚧 do [#beta]"),
            "the misplaced legacy archive is not completed history: {updated}"
        );
    }
    #[test]
    fn preflight_rebases_active_queue_head_change_without_mid_edit_evidence() {
        // A snapshot/version mismatch alone is not evidence of active typing.
        // The current document is authoritative, so maintenance adopts the
        // edited head and keeps monotonic queue progress.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(state.queue_active, Some(true));
        assert_eq!(state.queue_halted, None);
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()]
        );

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: start"));
        assert!(updated.contains("do [#newhead]"));
    }
    #[test]
    fn preflight_adopts_edited_queue_head_when_buffer_settled() {
        // #queue-no-stall-on-head-edit (adopt case): when an already-active
        // auto-queue's head prompt changes between cycles and the buffer is
        // settled (no live typing indicator), the loop must adopt the edited
        // head as the new prompt and stay armed — NOT strip `auto` / force
        // queue_active:false. The snapshot must absorb the edited head so
        // closeout queue-consume proves the same prompt and the next cycle sees
        // no spurious item_modified edit.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#oldhead]\n",
            "- do [#nexthead]\n",
            "<!-- /agent:queue -->\n"
        );
        let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
        std::fs::write(&doc, &current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // No typing indicator written → buffer is settled.

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert_eq!(
            state.queue_halted, None,
            "settled head edit must adopt + continue, not halt"
        );
        assert_eq!(state.queue_active, Some(true));
        assert_eq!(
            state.queue_prompts,
            vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()],
            "loop continues with the edited head as the new prompt"
        );

        // File keeps the armed auto-queue with the edited head.
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("agent:queue auto"),
            "auto preserved: {updated}"
        );
        assert!(
            updated.contains("queue: start"),
            "active preserved: {updated}"
        );
        assert!(updated.contains("- 🚧 do [#newhead]"));

        // Snapshot absorbed the edited head so a follow-up pass is idempotent
        // (no spurious item_modified on the now-converged head).
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("do [#newhead]"),
            "snapshot must absorb the adopted head: {snap}"
        );
        assert!(
            !snap.contains("- do [#oldhead]"),
            "snapshot must drop the stale head: {snap}"
        );
        let state2 = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state2.queue_halted, None,
            "converged head must not re-halt on the next pass"
        );
        assert_eq!(state2.queue_active, Some(true));
    }
    #[test]
    fn preflight_preserves_intentional_duplicate_tracked_queue_prompt() {
        // #queue-dedup-destroys-intentional-duplicates / #md-ast-document-model:
        // duplicate `do [#id]` text can be intentional user queue intent. Preflight
        // must not collapse it by raw prompt/id matching; only duplicate AST node
        // keys are eligible for cleanup.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("do [#adoc-orch-shim-cleanup]").count(),
            2,
            "duplicate tracked prompts must remain executable queue intent:\n{updated}"
        );
        assert_eq!(
            state.queue_prompts,
            vec![
                "do [#adoc-orch-shim-cleanup]".to_string(),
                "do [#adoc-orch-shim-cleanup]".to_string()
            ],
            "duplicate tracked prompts should remain queued: {state:?}"
        );
        // Re-running maintenance on the converged doc is a no-op (stable).
        let before = std::fs::read_to_string(&doc).unwrap();
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            before, after,
            "queue maintenance must be idempotent after dedup"
        );
    }
    #[test]
    fn preflight_keeps_intentional_duplicate_free_text_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do deploy\n",
            "- do deploy\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_prompts,
            vec![
                "do deploy ⟦effect:1⟧".to_string(),
                "do deploy ⟦effect:2⟧".to_string()
            ],
            "intentional duplicate free-text prompts should remain separately queued: {state:?}"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("do deploy ⟦effect:1⟧\n")
                && updated.contains("do deploy ⟦effect:2⟧\n"),
            "maintenance should expose distinct duplicate effect identities:\n{updated}"
        );
        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            updated,
            "effect annotations must be idempotent across preflight"
        );
    }
    #[test]
    fn preflight_gives_a_repeated_nested_command_a_fresh_effect_identity() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior review — gpt-5\n\n",
            "Reviewed once.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- https://example.test/pull/795\n",
            "  - ~~#review~~\n",
            "  - #review\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(state.queue_prompts, vec!["#review ⟦effect:2⟧"]);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("  - ~~#review~~\n") && updated.contains("#review ⟦effect:2⟧\n"),
            "the completed review stays historical while the rerun gets a distinct identity:\n{updated}"
        );

        let _ = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            updated,
            "the rerun surrogate id must be stable across preflight"
        );
    }
    #[test]
    fn preflight_does_not_reflag_stable_inactive_queue_as_residue() {
        // #adoc-queue-ipc-drift root cause #1: after an `item_modified` halt the
        // queue goes inactive (queue_active: false, no `auto`) with a retained
        // live tail, and the halt synced that shape into the snapshot. On the
        // NEXT preflight the inactive queue is unchanged from the snapshot, so
        // re-emitting `inactive_queue_residue` every cycle (with no user edit)
        // is pure loop noise and must be suppressed.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        // Snapshot == file: a stable, already-committed inactive queue with a
        // retained tail (the post-halt steady state).
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: pause\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- ~do [#first-done]~\n",
            "- do [#second-live]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();

        assert!(
            !state
                .warnings
                .iter()
                .any(|w| w.code == "inactive_queue_residue"),
            "stable inactive queue (unchanged vs snapshot) must not re-warn residue: {:?}",
            state.warnings
        );
        // The retained tail is preserved, and maintenance is idempotent.
        let before = std::fs::read_to_string(&doc).unwrap();
        assert!(before.contains("- 📥 do [#second-live]"));
        let _ = run_queue_maintenance(&doc, None).unwrap();
        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(before, after, "stable inactive queue must not be mutated");
    }
    #[test]
    fn pending_maintenance_clears_stale_supervisor_marker_when_fresh() {
        // `#staleshow`: with no live route-owned supervisor in the test environment,
        // `stale_supervisor_warning_for_doc` reads NOT stale, so a pre-seeded
        // "🔴 (restart/recycle your supervisor)" marker must be removed from the status
        // component (file + snapshot) by the preflight maintenance pass.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "🔴 (restart/recycle your supervisor)\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !file_after
                .contains(agent_doc_document::status_projection::STALE_SUPERVISOR_STATUS_MARKER),
            "fresh supervisor must clear the stale marker from the file: {file_after}"
        );
        assert!(
            file_after.contains("Session ready."),
            "other status content must be preserved: {file_after}"
        );

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snapshot_after
                .contains(agent_doc_document::status_projection::STALE_SUPERVISOR_STATUS_MARKER),
            "fresh supervisor must clear the stale marker from the snapshot: {snapshot_after}"
        );
    }

    #[test]
    fn pending_maintenance_projects_reported_queue_handoff_into_status() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#reported]\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#reported] integrate the completed worker result\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "#reported", "subagent:worker", 3600)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "#reported",
            "subagent:worker",
            "worker:test",
            3600,
            claim.generation,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::report_attempt(&doc, "#reported", Some(claim.generation))
            .unwrap();
        agent_doc_queue_io::claim_release_receipt::release_attempt(
            &doc,
            "#reported",
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
            Some(claim.generation),
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let expected = "📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout.";
        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(file_after.contains(expected), "{file_after}");
        assert!(
            file_after.contains("Session ready."),
            "operator-authored status prose must be preserved: {file_after}"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(snapshot_after.contains(expected), "{snapshot_after}");
    }

    #[test]
    fn pending_maintenance_defers_stale_supervisor_marker_when_authority_unavailable() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "🔴 (restart/recycle your supervisor)\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let effects = GuardFailingPreflightMaintenanceWriteEffects::default();

        run_pending_maintenance(&doc, &effects).unwrap();

        assert_eq!(effects.authority_checks.get(), 1);
        assert_eq!(
            effects.converge_calls.get(),
            0,
            "guard failure must not fall through to an unproven write"
        );
        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            file_after, content,
            "optional stale-supervisor status clear must be deferred without touching the file"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot_after, content,
            "deferred status clear must not desync the snapshot from the file"
        );
    }

    // #realtime-maintenance-defer: when the visible maintenance write cannot land
    // because the realtime editor buffer is mid-reconcile (the operator is NOT
    // typing — the last committed response is still being reconciled back into the
    // live buffer), preflight must defer the idempotent maintenance write to a
    // later cycle instead of aborting the whole preflight. Mirrors the
    // stale-supervisor defer above but exercises the broader realtime-drift error
    // family and a mirror-reap mutation.
    #[test]
    fn pending_maintenance_defers_mirror_reap_when_realtime_buffer_drifted() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#reap1] Already-done mirror\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#reap1] Already-done mirror\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let effects = GuardFailingPreflightMaintenanceWriteEffects::with_error(
            "visible document write for session.md deferred: document changed after the response merge was computed; retry after typing stops",
        );

        // Must NOT return Err — the realtime model owns the buffer drift, so
        // preflight continues and the reap re-applies once the buffer is idle.
        run_pending_maintenance(&doc, &effects)
            .expect("realtime buffer drift must defer maintenance, not abort preflight");

        assert!(effects.authority_checks.get() >= 1);
        assert_eq!(
            effects.converge_calls.get(),
            0,
            "deferred guard failure must not fall through to an unproven write"
        );
        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            file_after, content,
            "deferred mirror reap must leave the working-tree file untouched"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot_after, content,
            "deferred mirror reap must not desync the snapshot from the file"
        );
    }

    /// `#retaineddeferisnotafailure` — the 2026-09-12 `/agent-doc` admission wedge.
    ///
    /// `guard_visible_delivery_convergence` refuses AFTER `try_editor_converge`
    /// accepted the write, so the maintenance content is already in the editor's
    /// CRDT authority and disk was deliberately untouched. Preflight's defer
    /// predicate matched two prose phrases and this refusal carried neither, so
    /// the `?` on `run_pending_maintenance` failed turn admission: the
    /// `UserPromptSubmit` hook emitted no cycle contract and the session could
    /// not start at all — a whole session lost to bookkeeping that is re-derived
    /// from scratch next cycle.
    ///
    /// Asserted against the emitting crate's exported token rather than a copy of
    /// the message, so the classifier cannot drift away from the constructor.
    #[test]
    fn a_retained_delivery_refusal_defers_maintenance_instead_of_failing_admission() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#reap1] Already-done mirror\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#reap1] Already-done mirror\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = GuardFailingPreflightMaintenanceWriteEffects::with_converge_error(&format!(
            "pending_maintenance: visible document write for session.md is retained by the lazy \
             delivery projection because the editor state projection has not converged; no \
             secondary snapshot/commit or forced disk write was attempted. [{}] [{}]",
            agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
            agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
        ));

        run_pending_maintenance(&doc, &effects).expect(
            "a retained delivery projection must defer maintenance, not fail turn admission",
        );

        assert_eq!(
            effects.converge_calls.get(),
            1,
            "the refusal must come from the converge step, the way the live guard raises it"
        );
        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            file_after, content,
            "a retained write must leave the working-tree file untouched"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot_after, content,
            "nothing may be committed behind a retained delivery projection"
        );
    }

    /// A deferred maintenance write must not then assert the reap it never made.
    ///
    /// `#retaineddeferisnotafailure`: the post-maintenance verification asserts
    /// that completed tracked items are GONE from the working tree. When the
    /// write was deferred, disk still holds them by construction, so the
    /// assertion fails and aborts preflight anyway — reinstating the exact wedge
    /// the defer exists to prevent. Covers both defer branches: this shape was
    /// already reachable through the older realtime-drift defer.
    #[test]
    fn a_deferred_maintenance_write_does_not_assert_the_reap_it_never_persisted() {
        for error in [
            format!(
                "pending_maintenance: visible document write for session.md is retained by the \
                 lazy delivery projection. [{}] [{}]",
                agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
                agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
            ),
            "pending_maintenance: document changed after the response merge was computed"
                .to_string(),
        ] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            // A completed item live in a tracked surface is what sets
            // `saw_completed_before`, arming the post-write reap verification.
            let content = concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "## Backlog\n\n",
                "<!-- agent:backlog -->\n",
                "- [x] [#reap1] Completed in place\n",
                "- [ ] [#keep1] Keep me\n",
                "<!-- /agent:backlog -->\n"
            );
            std::fs::write(&doc, content).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                content,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();

            let effects = GuardFailingPreflightMaintenanceWriteEffects::with_converge_error(&error);

            run_pending_maintenance(&doc, &effects).unwrap_or_else(|err| {
                panic!("deferred maintenance must not verify an unpersisted reap ({error}): {err}")
            });

            assert_eq!(
                std::fs::read_to_string(&doc).unwrap(),
                content,
                "a deferred reap must leave the working-tree file untouched"
            );
        }
    }

    /// The retained-write class is NOT uniformly safe to continue past.
    ///
    /// `#retaineddeferisnotafailure`: an attached editor with no registered
    /// replica means the editor authority is unreachable, not converging. A
    /// closeout that continued past it would write behind an active listener —
    /// the invariant
    /// `force_disk_closeout_pending_maintenance_bypasses_active_listener` owns.
    /// So the defer keys on the narrow projection-pending token, and the broad
    /// class token alone must still fail closed.
    #[test]
    fn an_unreachable_editor_replica_still_fails_maintenance_closed() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#reap1] Already-done mirror\n",
            "<!-- /agent:backlog -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#reap1] Already-done mirror\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = GuardFailingPreflightMaintenanceWriteEffects::with_converge_error(&format!(
            "pending_maintenance: visible document write for session.md is retained by the lazy \
             delivery projection; the attached editor replica is not registered, so no snapshot \
             or commit effect is eligible. [{}]",
            agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
        ));

        run_pending_maintenance(&doc, &effects).expect_err(
            "an unreachable editor replica must still fail closed, not defer like a converging one",
        );
    }

    #[test]
    fn pending_maintenance_reaps_completed_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog_after.contains("[#reap1]"));
        assert!(file_after.contains("[#keep1]"));
        assert!(file_after.contains("## Done"));
        assert!(file_after.contains("<!-- agent:done -->"));
        assert!(file_after.contains("[#reap1] Reap me"));

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let snapshot_backlog_after = agent_doc_element::element::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_backlog_after.contains("[#reap1]"));
        assert!(snapshot_after.contains("[#keep1]"));
        assert!(snapshot_after.contains("## Done"));
        assert!(snapshot_after.contains("<!-- agent:done -->"));
        assert!(snapshot_after.contains("[#reap1] Reap me"));
    }
    #[test]
    fn pending_maintenance_auto_reaps_ops_proof_done_items() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#doneci] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals stale busy state\n",
            "- [ ] [#partial] #agent-doc-bug PARTIAL SHIPPED 9df1244f: committed first slice. REMAINING: live proof gate\n",
            "- [ ] [#reopened] #agent-doc-bug REOPENED false closeout: previous closeout DONE 1234567 (CI 1 green)\n",
            "- [ ] [#noproof] DONE: lacks deterministic proof\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#reviewdone] SHIPPED abcdef1 (CI 2 passed): review-gated shipped marker\n",
            "- [/] [#reviewkeep] Needs release review\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(report.backlog_gated_count, 0);
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();
        let review_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_review_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        assert!(!backlog_after.contains("[#doneci]"));
        assert!(!review_after.contains("[#reviewdone]"));
        assert!(backlog_after.contains("[#partial]"));
        assert!(backlog_after.contains("[#reopened]"));
        assert!(backlog_after.contains("[#noproof]"));
        assert!(review_after.contains("[#reviewkeep]"));
        assert!(file_after.contains("## Done"));
        assert!(file_after.contains("[#doneci] #agent-doc-bug DONE 7b60fcdc"));
        assert!(file_after.contains("[#reviewdone] SHIPPED abcdef1"));

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(!snapshot_after.contains("- [ ] [#doneci]"));
        assert!(!snapshot_after.contains("- [/] [#reviewdone]"));
        assert!(snapshot_after.contains("[#partial]"));
        assert!(snapshot_after.contains("[#reopened]"));
        assert!(snapshot_after.contains("[#noproof]"));
        assert!(snapshot_after.contains("[#reviewkeep]"));

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("auto_complete_ops_proof"));
        assert!(log.contains("id=doneci"));
        assert!(log.contains("id=reviewdone"));
    }
    #[test]
    fn ops_proof_does_not_reap_same_cycle_added_gated_item() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#freshgate] operator live-verify the destructive path. Code SHIPPED 1edb20d2; this is the live gate only\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        // The snapshot already contains the item — this models the finalize path
        // where the same invocation's --review-add re-synced the snapshot, so the
        // snapshot-only guard cannot tell this is a brand-new add.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // cycle_state records #freshgate as added this cycle.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_pending_added_ids(&doc, &["freshgate".to_string()])
            .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        // The freshly added gated item survives — not reaped on its first cycle.
        assert!(
            file_after.contains("[#freshgate]"),
            "same-cycle-added gated item must not be ops-proof reaped: {file_after}"
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("auto_complete_ops_proof"),
            "no ops-proof auto-completion should fire for a same-cycle add"
        );
    }
    #[test]
    fn ops_proof_does_not_reap_cited_dependency_marker() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#citeddep] wire the predicate into dispatch. The predicate already shipped in 600797b3 and is unit-tested\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate into dispatch\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        // Cited-dependency marker stays open; leading-status marker is reaped.
        assert!(
            backlog_after.contains("[#citeddep]"),
            "cited-dependency item must not be reaped: {backlog_after}"
        );
        assert!(!backlog_after.contains("[#leadstatus]"));
        assert!(file_after.contains("[#leadstatus] DONE 7b60fcdc"));
    }
    #[test]
    fn ops_proof_does_not_reap_live_verify_gate_on_commit_hash() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#ktw8] [live-verify gate] destructive auto-/clear between queue turns. ",
            "Code SHIPPED 1edb20d2; a shipped commit is NOT proof, an operator drive is. ",
            "PASS = a genuine anchored ops.log line; current verdict UNDRIVEN.\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            file_after.contains("[#ktw8]"),
            "live-verify gate must not be ops-proof reaped on a cited commit hash: {file_after}"
        );
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !log.contains("auto_complete_ops_proof"),
            "no ops-proof auto-completion should fire for a live-verify gate"
        );
    }
    #[test]
    fn pending_maintenance_does_not_reap_same_cycle_add() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        // Snapshot baseline: an existing leading-status done item + a keeper.
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
            "- [ ] [#keep] keep this open item\n",
            "<!-- /agent:backlog -->\n"
        );
        // File adds a brand-new same-cycle item with a leading-status marker that
        // would normally reap — but it is absent from the snapshot.
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#freshdone] DONE abc1234: just landed this cycle\n",
            "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
            "- [ ] [#keep] keep this open item\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, file_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let backlog_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&file_after)
            .to_string();

        // Same-cycle add survives; pre-existing leading-status item is reaped.
        assert!(
            backlog_after.contains("[#freshdone]"),
            "same-cycle add must not be reaped: {backlog_after}"
        );
        assert!(backlog_after.contains("[#keep]"));
        assert!(!backlog_after.contains("[#leadstatus]"));
    }
    #[test]
    fn pending_maintenance_keeps_new_work_that_only_shares_a_done_id() {
        // `#donemirrorbytext`: 2026-09-29, a new `#fixapiissue` was reaped as an
        // "already-done mirror" of an unrelated 2026-09-23 entry with the same id.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#fixapiissue] Fix api.md issue: I typed in the following but agent-doc deleted it\n",
            "<!-- /agent:backlog -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#fixapiissue] Fix agent-doc api.md issue: the retained write never settled\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let components = agent_doc_element::element::parse(&file_after).unwrap();
        let backlog = components.iter().find(|c| c.name == "backlog").unwrap();
        assert!(
            backlog
                .content(&file_after)
                .contains("I typed in the following but agent-doc deleted it"),
            "new work sharing a done id must survive maintenance:\n{file_after}"
        );
    }

    #[test]
    fn pending_maintenance_reaps_inline_done_backlog_and_review_mirrors() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done1] already archived backlog\n",
            "- [ ] [#keep1] keep backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#done2] already archived review\n",
            "- [/] [#keep2] keep review\n",
            "<!-- /agent:review -->\n\n",
            "## Done\n\n",
            "<!-- agent:done -->\n",
            "- [x] [#done1] already archived backlog\n",
            "- [x] [#done2] already archived review\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(report.backlog_gated_count, 0);
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_components = agent_doc_element::element::parse(&file_after).unwrap();
        let file_backlog = file_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        let file_review = file_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog.contains("[#done1]"));
        assert!(file_backlog.contains("[#keep1] keep backlog"));
        assert!(!file_review.contains("[#done2]"));
        assert!(file_review.contains("[#keep2] keep review"));
        assert_eq!(file_after.matches("[#done1]").count(), 1);
        assert_eq!(file_after.matches("[#done2]").count(), 1);

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let snapshot_components = agent_doc_element::element::parse(&snapshot_after).unwrap();
        let snapshot_backlog = snapshot_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        let snapshot_review = snapshot_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_backlog.contains("[#done1]"));
        assert!(!snapshot_review.contains("[#done2]"));
        assert_eq!(snapshot_after.matches("[#done1]").count(), 1);
        assert_eq!(snapshot_after.matches("[#done2]").count(), 1);
    }
    #[test]
    fn pending_maintenance_reaps_external_done_archive_backlog_and_review_mirrors() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let archive_rel = "session.done.md";
        let archive_path = dir.path().join(archive_rel);
        let archive_content = concat!(
            "# Done\n\n",
            "- [x] [#extdone1] externally archived backlog\n",
            "- [x] [#extdone2] externally archived review\n",
        );
        std::fs::write(&archive_path, archive_content).unwrap();
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#extdone1] externally archived backlog\n",
            "- [ ] [#fresh1] fresh backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#extdone2] externally archived review\n",
            "- [/] [#fresh2] fresh review\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:done archive=session.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(report.review_count, 1);
        assert_eq!(report.review_gated_count, 1);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_components = agent_doc_element::element::parse(&file_after).unwrap();
        let file_backlog = file_components
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&file_after)
            .to_string();
        let file_review = file_components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_backlog.contains("[#extdone1]"));
        assert!(file_backlog.contains("[#fresh1] fresh backlog"));
        assert!(!file_review.contains("[#extdone2]"));
        assert!(file_review.contains("[#fresh2] fresh review"));
        assert_eq!(
            std::fs::read_to_string(&archive_path).unwrap(),
            archive_content
        );

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(!snapshot_after.contains("stale backlog mirror"));
        assert!(!snapshot_after.contains("stale review mirror"));
        assert!(snapshot_after.contains("[#fresh1] fresh backlog"));
        assert!(snapshot_after.contains("[#fresh2] fresh review"));
    }
    #[test]
    fn preflight_allows_user_marked_done_item_reaped_in_same_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [/] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "baseline", "--no-verify"])
            .output()
            .unwrap();

        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Waiting on manual validation\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(current)).unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);
        let head_content = agent_doc_git_io::revision::show_head(&doc).unwrap();
        enforce_no_dropped_backlog(&doc, head_content.as_deref(), current)
            .expect("same-cycle reap should count as intentional completion");
    }

    #[test]
    fn preflight_accepts_idle_operator_backlog_deletion_but_not_open_cycle_loss() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "- [ ] [#remove1] Operator may remove me\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "baseline", "--no-verify"])
            .output()
            .unwrap();

        let current = baseline.replace("- [ ] [#remove1] Operator may remove me\n", "");
        std::fs::write(&doc, &current).unwrap();
        let head_content = agent_doc_git_io::revision::show_head(&doc).unwrap();
        enforce_no_dropped_backlog(&doc, head_content.as_deref(), &current)
            .expect("an idle visible deletion is operator-authoritative");

        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(&current)).unwrap();
        let err = enforce_no_dropped_backlog(&doc, head_content.as_deref(), &current).expect_err(
            "the same absence without operator-op proof must fail during an open cycle",
        );
        assert!(err.to_string().contains("#remove1"), "{err:#}");
    }

    #[test]
    fn preflight_allows_open_backlog_item_moved_to_another_project_document() {
        let dir = setup_project();
        let source = dir.path().join("tasks/source.md");
        let destination = dir.path().join("tasks/destination.md");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let baseline = concat!(
            "---\nagent_doc_session: source\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#moved1] Continue this work elsewhere\n",
            "<!-- /agent:backlog -->\n"
        );
        let empty_destination = concat!(
            "---\nagent_doc_session: destination\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n"
        );
        std::fs::write(&source, baseline).unwrap();
        std::fs::write(&destination, empty_destination).unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "tasks/source.md", "tasks/destination.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "baseline", "--no-verify"])
            .output()
            .unwrap();

        let current_source = baseline.replace("- [ ] [#moved1] Continue this work elsewhere\n", "");
        let current_destination = empty_destination.replace(
            "<!-- agent:backlog -->\n",
            "<!-- agent:backlog -->\n- [ ] [#moved1] Continue this work elsewhere\n",
        );
        std::fs::write(&source, &current_source).unwrap();
        std::fs::write(&destination, current_destination).unwrap();

        let head_content = agent_doc_git_io::revision::show_head(&source).unwrap();
        enforce_no_dropped_backlog(&source, head_content.as_deref(), &current_source)
            .expect("a unique open destination should prove the transfer");
    }
    #[test]
    fn pending_maintenance_reaps_completed_icebox_items_from_file_and_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [x] [#ice01] Reap me from icebox\n",
            "- [ ] [#keep2] Keep me parked\n",
            "<!-- /agent:icebox -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert!(!report.reordered);
        assert_eq!(report.backlog_gated_count, 0);

        let file_after = std::fs::read_to_string(&doc).unwrap();
        let file_icebox_after = agent_doc_element::element::parse(&file_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&file_after)
            .to_string();
        assert!(!file_icebox_after.contains("[#ice01]"));
        assert!(file_after.contains("[#keep2]"));
        assert!(file_after.contains("## Done"));
        assert!(file_after.contains("[#ice01] Reap me from icebox"));

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let snapshot_icebox_after = agent_doc_element::element::parse(&snapshot_after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "icebox")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        assert!(!snapshot_icebox_after.contains("[#ice01]"));
        assert!(snapshot_after.contains("[#keep2]"));
        assert!(snapshot_after.contains("## Done"));
        assert!(snapshot_after.contains("[#ice01] Reap me from icebox"));
    }
    #[test]
    fn pending_maintenance_syncs_snapshot_for_write_phase_gate_without_reap() {
        // #pending-gate-snapshot-desync: the write phase moved #g1 from backlog
        // to review (a --pending-gate) on the FILE, but the content_ours snapshot
        // still shows #g1 in backlog and an empty review. Maintenance makes no
        // reap/backfill change, yet it must re-sync the snapshot's tracked
        // surfaces to the file so the upcoming commit stages the gate instead of
        // stranding it as post-commit drift.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#g1] Gated, awaiting review\n",
            "<!-- /agent:review -->\n"
        );
        // Snapshot lags the file: #g1 still in backlog, review empty (the
        // baseline+response content_ours saved before the gate mutation).
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "- [ ] [#g1] Gated, awaiting review\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "<!-- /agent:review -->\n"
        );
        std::fs::write(&doc, file_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let comps = agent_doc_element::element::parse(&snapshot_after).unwrap();
        let snap_backlog = comps
            .iter()
            .find(|c| c.name == "backlog")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        let snap_review = comps
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&snapshot_after)
            .to_string();
        // Snapshot now matches the file: #g1 gated into review, gone from backlog.
        assert!(
            !snap_backlog.contains("[#g1]"),
            "snapshot backlog must drop the gated item: {snap_backlog}"
        );
        assert!(
            snap_review.contains("[/] [#g1]"),
            "snapshot review must carry the gated item: {snap_review}"
        );
        assert!(snap_backlog.contains("[#keep1]"));
    }
    #[test]
    fn gate_verify_surfaces_provable_without_flipping_when_optin_off() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_receipt_accepted".to_string()),
                disproof: Some("false receipt-timeout".to_string()),
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(&dir, "[150] early_receipt_accepted emitted ok\n");

        let results =
            run_gate_verify(&doc, false, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "saev");
        assert_eq!(results[0].status, "provable");
        assert!(
            !results[0].auto_resolved,
            "opt-in off must not flip the gate"
        );

        // The document still shows the gated item — never silently flipped.
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_auto_resolves_provable_when_optin_on() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_receipt_accepted".to_string()),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(&dir, "[150] early_receipt_accepted emitted ok\n");

        let results =
            run_gate_verify_force_disk(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "provable");
        assert!(results[0].auto_resolved);

        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("[x] [#saev]"),
            "gate must be flipped: {after}"
        );
        // Snapshot kept in lockstep for the upcoming commit.
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("[x] [#saev]"),
            "snapshot must flip too: {snap}"
        );
    }
    #[test]
    fn gate_verify_failed_never_auto_resolves_even_with_optin() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_receipt_accepted".to_string()),
                disproof: Some("manual cleanup".to_string()),
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] early_receipt_accepted emitted\n[160] looks like a manual cleanup\n",
        );

        let results =
            run_gate_verify_force_disk(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(results[0].status, "failed", "disproof wins");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("- [/] [#saev]"),
            "failed gate must remain: {after}"
        );
    }
    #[test]
    fn gate_verify_empty_without_predicate() {
        let dir = setup_project();
        let doc = write_optverify_doc(&dir, "");
        write_ops_log(&dir, "[150] early_receipt_accepted emitted\n");
        let results =
            run_gate_verify_force_disk(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert!(results.is_empty(), "no predicate → no results");
    }
    #[test]
    fn gate_verify_ignores_marker_quoted_in_content_logging_lines() {
        // #gng8: queue_diff_active_prompt_differs embeds document prose via
        // {:?}; a gate must not auto-prove from its own backlog description.
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some("early_receipt_accepted".to_string()),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"expect early_receipt_accepted emitted before apply\"] queue_head=\"[#saev]\"\n",
        );

        let results =
            run_gate_verify(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "pending", "quoted prose must not prove");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_s760_builtin_ignores_queue_diff_prose_only() {
        // #ktw8: the destructive clear gate is proven only by an anchored
        // structured [s760] line, never by prose embedded in queue_diff logs.
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some(
                    agent_doc_element_backlog::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER
                        .to_string(),
                ),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"PASS requires [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\"] queue_head=\"[#ktw8]\"\n",
        );

        let results =
            run_gate_verify(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "pending", "quoted prose must not prove");
        assert!(!results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
    }
    #[test]
    fn gate_verify_s760_builtin_auto_resolves_on_anchored_clear_true() {
        let dir = setup_project();
        let pred = agent_doc_element_backlog::gate_verify::render_annotation(
            &agent_doc_element_backlog::gate_verify::GatePredicate {
                verify: Some(
                    agent_doc_element_backlog::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER
                        .to_string(),
                ),
                disproof: None,
                set_at: Some(100),
            },
        );
        let doc = write_optverify_doc(&dir, &pred);
        write_ops_log(
            &dir,
            "[150] [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\n",
        );

        let results =
            run_gate_verify_force_disk(&doc, true, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "provable");
        assert!(results[0].auto_resolved);
        let after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            after.contains("[x] [#saev]"),
            "gate must be flipped: {after}"
        );
    }
    #[test]
    fn pending_maintenance_continues_when_snapshot_backlog_cannot_be_synced() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let file_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [x] [#reap1] Reap me\n",
            "<!-- /agent:backlog -->\n"
        );
        let snapshot_content =
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\nNo backlog here.\n";
        std::fs::write(&doc, file_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS).unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !agent_doc_element::element::parse(&file_after)
                .unwrap()
                .into_iter()
                .find(|c| c.name == "backlog")
                .unwrap()
                .content(&file_after)
                .contains("[#reap1]"),
            "completed item must be reaped from the authoritative document:\n{file_after}"
        );
        assert!(
            file_after.contains("## Done") && file_after.contains("[#reap1] Reap me"),
            "completed item must be archived in the document:\n{file_after}"
        );
    }

    #[test]
    fn pending_maintenance_prunes_redundant_terminal_debris_through_authority_path() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "The earlier response records correlation 93980072-a559-45d3-8fe5-98e2d0180fc4.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live] Current authoritative item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "- [ ] [#live] Superseded copy\n",
            "93980072-a559-45d3-8fe5-98e2d0180fc4\n",
            "- [ ]\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // The non-force path can only change the file through the injected
        // convergence effect. This is the regression for a disk-only cleanup
        // being restored by the still-corrupt CRDT canonical.
        let report =
            run_pending_maintenance(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS).unwrap();

        assert_eq!(report.terminal_debris_lines_pruned, 3);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.ends_with("<!-- /agent:done -->\n"));
        assert!(!updated.contains("Superseded copy"));
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, updated);
    }

    #[test]
    fn run_pending_maintenance_sorts_backlog_by_priority() {
        // #backlog-priority-attribute: a backlog carrying `priority` stable-sorts
        // items by their per-item priority token each cycle.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:backlog priority -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
            .unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let high = updated.find("[#high]").unwrap();
        let low = updated.find("[#low]").unwrap();
        assert!(
            high < low,
            "priority=1 item must sort before priority=5:\n{updated}"
        );
    }
    fn review_body_after_maintenance(review_marker: &str) -> String {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
             <!-- agent:backlog -->\n- [ ] [#open1] backlog item\n<!-- /agent:backlog -->\n\n\
             {review_marker}\n\
             - [/] [#rlow] priority=5 later review\n\
             - [/] [#runset] no rank review\n\
             - [/release] [#rhigh] priority=1 first review\n\
             <!-- /agent:review -->\n",
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let report =
            run_pending_maintenance_force_disk(&doc, &TEST_PREFLIGHT_MAINTENANCE_WRITE_EFFECTS)
                .unwrap();
        assert_eq!(report.review_count, 3);
        assert_eq!(report.review_gated_count, 3);

        let updated = std::fs::read_to_string(&doc).unwrap();
        let components = agent_doc_element::element::parse(&updated).unwrap();
        components
            .iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&updated)
            .to_string()
    }

    #[test]
    fn run_pending_maintenance_sorts_review_by_priority() {
        // #reviewpriority: an `agent:review priority` marker stable-sorts gated
        // review items by their per-item priority token, exactly like
        // backlog/icebox; unranked items sink below every numbered item.
        let review = review_body_after_maintenance("<!-- agent:review priority -->");
        let high = review.find("[#rhigh]").unwrap();
        let low = review.find("[#rlow]").unwrap();
        let unset = review.find("[#runset]").unwrap();
        assert!(
            high < low && low < unset,
            "review must sort priority=1, priority=5, then unranked:\n{review}"
        );
        assert!(
            review.contains("- [/release] [#rhigh] priority=1 first review"),
            "the sort must keep each item's gate state and text:\n{review}"
        );
    }

    #[test]
    fn run_pending_maintenance_leaves_review_order_without_priority_attr() {
        // #reviewpriority regression: without the marker attribute, per-item
        // priority tokens on review items never reorder the component.
        let review = review_body_after_maintenance("<!-- agent:review -->");
        let low = review.find("[#rlow]").unwrap();
        let unset = review.find("[#runset]").unwrap();
        let high = review.find("[#rhigh]").unwrap();
        assert!(
            low < unset && unset < high,
            "review without `priority` must keep authored order:\n{review}"
        );
    }

    #[test]
    fn run_queue_maintenance_orders_synced_queue_by_priority() {
        // #backlog-priority-attribute + #backlog-queue-sync-attr: a priority queue
        // synced from a priority backlog comes out prioritized.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "<!-- agent:queue priority -->\n",
            "- do [#low]\n",
            "- do [#high]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#low] priority=5 later\n",
            "- [ ] [#high] priority=1 first\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let q = updated.find("<!-- agent:queue").unwrap();
        let qend = updated[q..].find("<!-- /agent:queue").unwrap() + q;
        let queue_region = &updated[q..qend];
        let high = queue_region.find("do [#high]").unwrap();
        let low = queue_region.find("do [#low]").unwrap();
        // `#pinoperatoronly`: promotion is expressed by ORDER, not by a marker.
        assert!(
            !queue_region.contains("📍 do [#high]"),
            "priority sorting must not inject an agent pin marker:\n{queue_region}"
        );
        assert!(
            high < low,
            "priority=1 must sort before priority=5 in queue:\n{queue_region}"
        );
    }
    #[test]
    fn resolve_pipeline_state_none_without_cycle_or_frontmatter() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "body\n").unwrap();
        assert!(resolve_pipeline_state(&doc).unwrap().is_none());
    }
    #[test]
    fn resolve_pipeline_state_falls_back_to_frontmatter_block() {
        // No cycle-state on disk → read the document `agent_doc_pipeline:` mirror.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: cycle-77\n  step: write_applied\n---\n\nbody\n",
        )
        .unwrap();
        let p = resolve_pipeline_state(&doc)
            .unwrap()
            .expect("frontmatter fallback");
        assert_eq!(p.run_id.as_deref(), Some("cycle-77"));
        assert_eq!(p.step.as_deref(), Some("write_applied"));
    }
    #[test]
    fn resolve_pipeline_state_cycle_state_wins_over_frontmatter() {
        // Cycle-state is authoritative; a stale frontmatter block must not override it.
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_pipeline:\n  run_id: stale-mirror\n  step: committed\n---\n\nbody\n",
        )
        .unwrap();
        let state = agent_doc_cycle_state_io::start_preflight_with_task(
            &doc,
            Some("snap"),
            Some("body"),
            Some("#fmrunid-wire"),
            Some("#fmrunid-wire"),
        )
        .unwrap();

        let p = resolve_pipeline_state(&doc)
            .unwrap()
            .expect("cycle-state present");
        assert_eq!(p.run_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(p.step.as_deref(), Some("preflight_started"));
        assert_eq!(p.turn_id.as_deref(), Some("#fmrunid-wire"));
        assert_ne!(p.run_id.as_deref(), Some("stale-mirror"));
    }

    #[test]
    fn enforce_cycle_completion_does_not_commit_an_abandoned_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let content = "<!-- agent:exchange -->\nNew question?\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            repair_outcome: Some(
                agent_doc_turn::repair::RepairOutcome::StalePreflightCycleAbandoned,
            ),
            ..Default::default()
        };
        assert_eq!(
            enforce_cycle_completion(&doc, &effects).unwrap(),
            (true, false)
        );
        assert_eq!(effects.repair_calls.get(), 1);
        assert_eq!(effects.commit_calls.get(), 0);
    }

    #[test]
    fn enforce_cycle_completion_uses_committed_ledger_projection() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        let content = "---\nagent_doc_session: test\n---\n\n## Exchange\n\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(content),
            Some(content),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Committed
        );

        let effects = TestPreflightCycleCompletionEffects::default();
        let result = enforce_cycle_completion(&doc, &effects).unwrap();
        assert_eq!(result, (false, false));
        assert_eq!(
            effects.repair_calls.get(),
            0,
            "the terminal projection must not force repair"
        );
        assert_eq!(
            effects.commit_calls.get(),
            0,
            "stale open JSON must not force commit when lazily says committed"
        );
    }

    #[test]
    fn interrupted_cycle_folds_ipc_diagnostic_before_repair_can_commit() {
        let source = include_str!("lib.rs");
        let body = source
            .split_once("pub fn enforce_cycle_completion(")
            .unwrap()
            .1
            .split_once("pub fn append_latest_ipc_dogfood_note(")
            .unwrap()
            .0;
        let append = body
            .find("append_latest_ipc_dogfood_note(file)")
            .expect("diagnostic append step");
        let repair = body
            .find("effects.repair(file)")
            .expect("interrupted-cycle repair step");
        assert!(
            append < repair,
            "binary diagnostic must join the open recovery image before repair can close its cycle"
        );
    }

    #[test]
    fn enforce_cycle_completion_blocks_new_preflight_on_retained_effect() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            retained_document_write: std::cell::Cell::new(true),
            session_interruption: Some(
                "[session-check] INTERRUPTED: binary-owned response delivery is retained"
                    .to_string(),
            ),
            ..Default::default()
        };

        let error = enforce_cycle_completion(&doc, &effects).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("one bounded recovery transition"),
        );
        assert_eq!(effects.repair_calls.get(), 0);
        assert_eq!(effects.commit_calls.get(), 0);
        assert_eq!(
            effects.session_interruption_calls.get(),
            0,
            "preflight must not nest a full session-check behind an unresolved retained write"
        );
    }

    #[test]
    fn enforce_cycle_completion_recovers_stranded_write_before_reading_gate() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            retained_document_write: std::cell::Cell::new(true),
            recovery_succeeds: true,
            // If the gate runs before recovery, this proves the regression by
            // taking the interruption branch.
            session_interruption: Some(
                "[session-check] INTERRUPTED: retained write was not replayed".to_string(),
            ),
            ..Default::default()
        };

        let (recovered, committed) =
            enforce_cycle_completion(&doc, &effects).expect("preflight should recover and proceed");
        assert!(recovered);
        assert!(!committed);
        assert_eq!(effects.recovery_calls.get(), 1);
        assert_eq!(effects.gate_reads.get(), 1);
        assert_eq!(effects.repair_calls.get(), 0);
        assert_eq!(effects.commit_calls.get(), 0);
    }

    #[test]
    fn enforce_cycle_completion_applies_the_shared_settlement_before_gating() {
        // #preflightsettleparity: `session-check` read the shared verdict AND
        // applied its projection; preflight only read it. A `Satisfied` intent
        // that no `session-check` had yet cleared then blocked every cycle while
        // `session-check` simultaneously reported ok — the deadlock observed
        // 2026-07-26 on tasks/agent-doc/agent-doc-bugs2.md, where the verdict was
        // `Satisfied` the whole time and nothing in preflight's path ever
        // applied the clear. Sharing the cell is not sharing the action.
        //
        // #retainedclearreactive: the fix is no longer "preflight also calls
        // settle" — the clear is subscribed to the verdict cell in the
        // controller, so reading the gate settles a `Satisfied` intent. The gate
        // must therefore read the derived verdict, not a raw
        // `pending_write.is_some()`, or the subscription never runs.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            retained_document_write: std::cell::Cell::new(true),
            settle_satisfies: true,
            // Present so a regression cannot pass by way of the interruption
            // branch: if the gate is reached at all, this is the error raised.
            session_interruption: Some(
                "[session-check] INTERRUPTED: binary-owned response delivery is retained"
                    .to_string(),
            ),
            ..Default::default()
        };

        enforce_cycle_completion(&doc, &effects)
            .expect("a satisfied retained intent must not block a new cycle");
        assert_eq!(
            effects.gate_reads.get(),
            1,
            "the gate must read the shared derived verdict exactly once — that read is what settles"
        );
    }

    #[test]
    fn enforce_cycle_completion_still_blocks_when_settling_cannot_satisfy() {
        // The other half of #preflightsettleparity: settling is idempotent and a
        // genuinely `Unsettled` intent must still fail closed. Making the read
        // settle must not turn the gate into a rubber stamp.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            retained_document_write: std::cell::Cell::new(true),
            settle_satisfies: false,
            ..Default::default()
        };

        let error = enforce_cycle_completion(&doc, &effects).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("reconciliation is still pending"),
            "unexpected error: {error}"
        );
        assert_eq!(
            effects.gate_reads.get(),
            1,
            "one derived gate read is the bounded preflight decision"
        );
        assert_eq!(effects.session_interruption_calls.get(), 0);
    }

    fn id_set<const N: usize>(ids: [&str; N]) -> std::collections::HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn prior_cycle_json(
        phase: &str,
        capture: Option<&str>,
    ) -> agent_doc_cycle_state_io::CycleState {
        let capture = capture
            .map(|c| format!(r#","capture_id":"{c}""#))
            .unwrap_or_default();
        serde_json::from_str(&format!(
            r#"{{"cycle_id":"cycle-1","file":"session.md","phase":"{phase}","last_event":"x","started_at":0,"updated_at":0{capture}}}"#
        ))
        .expect("minimal cycle state")
    }

    #[test]
    fn a_preflight_only_cycle_is_not_evidence_that_its_head_stalled() {
        // #adpreflightreentryskip: repair closes the first preflight's cycle at
        // step 1, so a second `agent-doc preflight` in the SAME turn sees a
        // seconds-old `preflight_started` cycle as "the prior cycle". Treating
        // that as a completed dispatch is what walked the queue forward one head
        // per call on 2026-07-26, stamping ⏭️ on work nobody ran.
        assert!(
            !prior_cycle_hosted_a_turn(&prior_cycle_json("preflight_started", None)),
            "a cycle that only opened proves nothing about its head"
        );
        // `#queueskip`'s real case still qualifies: the turn ran and came back.
        assert!(prior_cycle_hosted_a_turn(&prior_cycle_json(
            "preflight_started",
            Some("cap-1")
        )));
        assert!(prior_cycle_hosted_a_turn(&prior_cycle_json(
            "committed",
            Some("cap-1")
        )));
        // The reopened half: preflight's own step-2 commit promotes the EMPTY
        // cycle it just opened to `committed`, so a terminal phase alone is
        // satisfied by exactly the re-entrant preflight this guard exists to
        // reject. Reproduced 2026-07-26 with the guard in place —
        // `document_cycles` rows cycle-1785091429188 / cycle-1785091492165 were
        // both `committed` with no capture, and two operator heads were skipped.
        assert!(
            !prior_cycle_hosted_a_turn(&prior_cycle_json("committed", None)),
            "a committed cycle with no capture is preflight's own bookkeeping commit, not a turn"
        );
    }

    #[test]
    fn a_head_is_skipped_only_when_a_completed_cycle_left_it_unconsumed() {
        let live = id_set(["alpha", "beta"]);

        // No qualifying prior cycle (re-entrant preflight): nothing is skipped.
        let fresh = advance_skipped_queue_head_ids(
            std::collections::HashSet::new(),
            None,
            &std::collections::HashSet::new(),
            &live,
            &std::collections::HashSet::new(),
        );
        assert!(
            fresh.is_empty(),
            "a preflight re-entry must not advance the skip set: {fresh:?}"
        );

        // A completed cycle that left `alpha` unresolved: skip it.
        let fresh = advance_skipped_queue_head_ids(
            std::collections::HashSet::new(),
            Some("alpha"),
            &std::collections::HashSet::new(),
            &live,
            &std::collections::HashSet::new(),
        );
        assert_eq!(fresh, id_set(["alpha"]));

        // Resolved heads and heads that left the queue are cleared, so a skip
        // never outlives the condition that justified it.
        let fresh = advance_skipped_queue_head_ids(
            id_set(["alpha", "gone"]),
            Some("beta"),
            &id_set(["alpha"]),
            &live,
            &std::collections::HashSet::new(),
        );
        assert_eq!(fresh, id_set(["beta"]));
    }

    #[test]
    fn a_skip_never_outlives_the_queue_making_progress() {
        // A carried skip retained purely because its id is still live is a
        // permanent one: a skipped head is never selected, so it can never
        // resolve itself out of the set, and the operator's top queue item
        // silently stops being worked. Any resolved id means the queue moved, so
        // the stall claim behind the carried skips is stale.
        let live = id_set(["alpha", "beta", "gamma"]);

        // No progress: carried skips survive, as before.
        let fresh = advance_skipped_queue_head_ids(
            id_set(["alpha", "beta"]),
            None,
            &std::collections::HashSet::new(),
            &live,
            &std::collections::HashSet::new(),
        );
        assert_eq!(fresh, id_set(["alpha", "beta"]));

        // Progress on an unrelated head clears them and re-offers the work.
        let fresh = advance_skipped_queue_head_ids(
            id_set(["alpha", "beta"]),
            None,
            &id_set(["gamma"]),
            &live,
            &std::collections::HashSet::new(),
        );
        assert!(
            fresh.is_empty(),
            "a completed head must re-offer previously skipped work, not strand it: {fresh:?}"
        );

        // The head the progressing cycle itself left unconsumed is still skipped
        // — clearing the carried set must not lose the fresh evidence.
        let fresh = advance_skipped_queue_head_ids(
            id_set(["alpha"]),
            Some("beta"),
            &id_set(["gamma"]),
            &live,
            &std::collections::HashSet::new(),
        );
        assert_eq!(fresh, id_set(["beta"]));
    }

    #[test]
    fn an_operator_unskip_clears_a_carried_skip_and_blocks_its_restamp() {
        // `#releaseskipcarried`: a bare `#release` head is never consumed, so the
        // prior cycle's evidence re-stamps it every cycle. Removing its `⏭️` is
        // the operator overruling that claim, and must win over both.
        let live = id_set(["release", "beta"]);
        let fresh = advance_skipped_queue_head_ids(
            id_set(["release"]),
            Some("release"),
            &std::collections::HashSet::new(),
            &live,
            &id_set(["release"]),
        );
        assert!(fresh.is_empty(), "operator un-skip must stick: {fresh:?}");

        // Other carried skips are untouched by an unrelated un-skip.
        let fresh = advance_skipped_queue_head_ids(
            id_set(["release", "beta"]),
            None,
            &std::collections::HashSet::new(),
            &live,
            &id_set(["release"]),
        );
        assert_eq!(fresh, id_set(["beta"]));
    }

    #[test]
    fn a_blocked_gate_returns_bounded_reconciliation_without_nested_session_check() {
        // #adretainedcircle: preflight used to invoke full session-check here,
        // then either report its interruption or discover that session-check
        // said ok and prescribe another write. Besides creating a closed loop,
        // the nested controller/editor reads could exhaust the MCP deadline.
        // One recovery transition followed by one gate read is the bounded,
        // resumable decision.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let effects = TestPreflightCycleCompletionEffects {
            retained_document_write: std::cell::Cell::new(true),
            settle_satisfies: false,
            // A nested session-check would report ok; it must not be called.
            session_interruption: None,
            ..Default::default()
        };

        let error = enforce_cycle_completion(&doc, &effects)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("one bounded recovery transition")
                && error.contains("will not run session-check inline"),
            "the refusal must explain the bounded recovery boundary: {error}"
        );
        assert!(
            error.contains("Do not start a new response")
                && error.contains("run only `agent-doc session-check"),
            "the refusal must preserve the retained effect and prescribe status-only continuation: {error}"
        );
        assert_eq!(
            effects.session_interruption_calls.get(),
            0,
            "preflight must not nest session-check behind the unresolved gate"
        );
    }

    #[test]
    fn enforce_cycle_completion_self_heals_uncommittable_backlog_capture() {
        // #capturebacklogatomic: a ResponseCaptured cycle that requested a backlog
        // capture (`requires_backlog_capture`) but recorded none
        // (`!pending_added_this_cycle`), and whose response cannot commit, is
        // unrecoverable — it used to hard-error ("still response_captured after
        // recovery/commit") and demand a manual mv-aside. enforce_cycle_completion
        // must instead self-heal by abandoning the stuck cycle so the next preflight
        // proceeds; the operator prompt stays uncommitted for a fresh cycle.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "# Doc\n\n<!-- agent:exchange -->\nprompt\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(content),
            Some(content),
            "deadbeef",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::record_backlog_capture_requirement(&doc, true)
            .unwrap()
            .expect("cycle state present");
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::ResponseCaptured
        );

        // commit refuses (Ok(false)): the backlog gate can never be satisfied from the
        // capture, so this used to bail. Now it self-heals to Abandoned.
        let effects = TestPreflightCycleCompletionEffects::default();
        let result = enforce_cycle_completion(&doc, &effects)
            .expect("uncommittable backlog capture must self-heal, not hard-error");
        assert!(
            result.0,
            "self-heal must report the interrupted cycle as recovered"
        );
        assert!(!result.1, "commit did not succeed");
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Abandoned,
            "the unrecoverable backlog-capture cycle must be abandoned (terminal)"
        );
    }

    #[test]
    fn subagentinlineveto_queue_maintenance_never_catches_up_an_inline_answered_dispatch_row() {
        // `#subagentinlineveto` (live, haiven-dev sdk-demo.md 2026-10-28): the
        // coordinator answered a `subagents` dispatch row inline. The closeout
        // strike now keeps it queued; this catch-up must not strike it on the
        // next preflight either, or the inline run still closes the row.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: PR 74 links — opus\n\n",
            "> **Queue prompt:** https://github.com/example/docs/pull/74/changes\n\n",
            "Fixed and pushed: every link is inline now.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:run priority subagents go -->\n",
            "- 📥 https://github.com/example/docs/pull/74/changes\n",
            "<!-- /agent:run -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("~~https://github.com/example/docs/pull/74/changes~~"),
            "an inline-answered subagents dispatch row must stay queued:\n{updated}"
        );
        assert!(
            updated
                .contains("https://github.com/example/docs/pull/74/changes\n<!-- /agent:run -->"),
            "{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_strikes_prior_cycle_answered_free_text_head() {
        // #qheadresidue: a free-text queue head answered by a PRIOR cycle (its
        // `> **Queue prompt:**` echo is in committed `agent:exchange`, but the
        // current response does not re-quote it) was never struck by the
        // per-cycle #ftstrike, so it stayed active as "completed residue" that
        // session-check INTERRUPTs on every closeout while go-mode convergence
        // re-adds it — the live queue-churn root cause. The preflight catch-up
        // strike must remove it.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: agent switch isn't reactive — opus-4-8\n\n",
            "> **Queue prompt:** JB Run Agent Doc on sampleportal after switching from codex to opencode any agent change has this issue\n\n",
            "Diagnosed: a paused stale parent supervisor. Restart it.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "🚧 :pushpin: JB Run Agent Doc on sampleportal after switching from codex to opencode any agent change has this issue\n",
            "```\n",
            "[route] target tmux session: 0\n",
            "Error: authoritative actor record deferring to boundary agent restart\n",
            "```\n",
            "---\n",
            "- :pushpin: do [#beta]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#beta] second item still open\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        let entries = agent_doc_queue::document_queue::parse(&queue_body).unwrap();
        // The answered bare multi-line head is now a `Completed` (struck) entry,
        // not an active `Prompt` — session-check's residue guard keys off active
        // heads, so this is exactly what clears the churn.
        let active: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !active
                .iter()
                .any(|t| t.contains("JB Run Agent Doc on sampleportal")),
            "answered head must NOT remain an active Prompt:\nactive={active:?}"
        );
        assert!(
            completed
                .iter()
                .any(|t| t.contains("JB Run Agent Doc on sampleportal")),
            "answered head must be moved to a Completed (struck) entry:\ncompleted={completed:?}\n{updated}"
        );
        // The unanswered id-backed head survives the strike pass, still active.
        assert!(
            active.iter().any(|t| t.contains("do [#beta]")),
            "unanswered id-backed head must remain an active Prompt:\nactive={active:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_keeps_reported_head_with_unanswered_operator_action() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prepare sdk demo\n\n",
            "> **Queue prompt:** prepare sdk demo\n\n",
            "The worker finished, but the operator decision is still pending.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- 🙋☑️ prepare sdk demo [#subagent-operator-request-sdk-demo-approval]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#subagent-operator-request-sdk-demo-approval] Approve the sdk demo release\n",
            "<!-- /agent:todo -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let (active, completed) = residue_scope_queue_rows(&doc);
        assert_eq!(active.len(), 1, "blocked reported head must remain live");
        assert!(
            active[0].contains("prepare sdk demo"),
            "the original blocked work must remain queued: {active:?}"
        );
        assert!(
            !agent_doc_document::queue_projection::operator_action_ids(&active[0]).is_empty(),
            "the unanswered operator-action link must remain projected: {active:?}"
        );
        assert!(
            completed.is_empty(),
            "historical answer evidence must not strike an unanswered operator request: {completed:?}"
        );
    }

    /// `#qheadresiduescope` fixture: a session document whose exchange holds
    /// `exchange` and whose queue body is `queue`.
    fn residue_scope_doc(exchange: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nqueue: start\n---\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue priority go -->\n{queue}- :pushpin: do [#beta]\n<!-- /agent:queue -->\n\n<!-- agent:backlog priority queue -->\n- [ ] [#beta] second item still open\n<!-- /agent:backlog -->\n"
        )
    }

    /// Commit `content` as `session.md` in the fixture's git repository and make
    /// it the committed snapshot, as a closeout does.
    fn residue_scope_commit(dir: &Path, doc: &Path, content: &str, message: &str) {
        std::fs::write(doc, content).unwrap();
        for args in [
            vec!["add", "session.md"],
            vec!["-c", "commit.gpgsign=false", "commit", "-q", "-m", message],
        ] {
            let out = Command::new("git")
                .current_dir(dir)
                .args(&args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        agent_doc_snapshot_io::checkpoint_document_baseline(
            doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
    }

    fn residue_scope_queue_rows(doc: &Path) -> (Vec<String>, Vec<String>) {
        let updated = std::fs::read_to_string(doc).unwrap();
        let mut active = Vec::new();
        let mut completed = Vec::new();
        for entry in queue_entries_in_content(&updated) {
            match entry {
                agent_doc_queue::document_queue::QueueEntry::Prompt(p) => active.push(p.text),
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => completed.push(p.text),
                _ => {}
            }
        }
        (active, completed)
    }

    const RESIDUE_SCOPE_REPORT: &str =
        "the session list flickers when a second pane opens the same document";

    /// The cycle that answered the report the first time: its echo, then the
    /// head struck in the committed queue.
    fn residue_scope_first_answer() -> String {
        format!(
            "### Re: session list flicker\n\n> **Queue prompt:** {RESIDUE_SCOPE_REPORT}\n\nFixed the double registration.\n\n"
        )
    }

    /// `#qheadresiduescope` (backlog: the catch-up residue strike can still
    /// strike a re-typed free-text head). The operator reported a bug, a cycle
    /// answered it (echo + strike), the bug came back, and the operator typed
    /// the same report again. A later cycle answered something else and the
    /// closeout did not quote the report. The catch-up strike matched the head
    /// against EVERY earlier `Queue prompt` echo, so the old answer struck the
    /// new report without anyone looking at it. Only echoes written after the
    /// head re-entered the queue may answer it. Both re-entry shapes are
    /// covered: the old struck row deleted, and the old struck row kept with
    /// the new line typed below it.
    #[test]
    fn run_queue_maintenance_keeps_retyped_repeated_report_live() {
        for keep_struck_row in [false, true] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            let report_row = format!("- {RESIDUE_SCOPE_REPORT}\n");
            let struck_row = format!("- ~~{RESIDUE_SCOPE_REPORT}~~\n");
            residue_scope_commit(
                dir.path(),
                &doc,
                &residue_scope_doc("", &report_row),
                "operator reports the bug",
            );
            let answered = residue_scope_first_answer();
            residue_scope_commit(
                dir.path(),
                &doc,
                &residue_scope_doc(&answered, &struck_row),
                "cycle answers and strikes it",
            );
            let retyped_queue = if keep_struck_row {
                format!("{struck_row}{report_row}")
            } else {
                report_row.clone()
            };
            // The re-typed head lands in a commit together with an unrelated
            // answer: this closeout did not quote the report.
            let later = format!(
                "{answered}### Re: other\n\n> **Queue prompt:** rename the export button to download\n\nRenamed.\n\n"
            );
            residue_scope_commit(
                dir.path(),
                &doc,
                &residue_scope_doc(&later, &retyped_queue),
                "operator re-types the report; unrelated closeout",
            );

            for pass in 0..2 {
                let _ = run_queue_maintenance(&doc, None).unwrap();
                let (active, _) = residue_scope_queue_rows(&doc);
                assert!(
                    active.iter().any(|t| t.contains(RESIDUE_SCOPE_REPORT)),
                    "keep_struck_row={keep_struck_row} pass={pass}: a re-typed report \
                     answered only BEFORE it re-entered the queue must stay live:\n{}",
                    std::fs::read_to_string(&doc).unwrap()
                );
            }
            let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
                .unwrap_or_default();
            assert!(
                ops.contains("preflight_freetext_residue_strike_skipped_echo_predates_head_entry"),
                "the skipped strike must be logged:\n{ops}"
            );
        }
    }

    /// `#qheadresiduescope` counterpart: the re-typed report IS answered by a
    /// newer echo (written after its re-entry) that the cycle's own strike
    /// missed. The catch-up still strikes it.
    #[test]
    fn run_queue_maintenance_strikes_retyped_report_answered_after_reentry() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let report_row = format!("- {RESIDUE_SCOPE_REPORT}\n");
        let struck_row = format!("- ~~{RESIDUE_SCOPE_REPORT}~~\n");
        residue_scope_commit(
            dir.path(),
            &doc,
            &residue_scope_doc("", &report_row),
            "operator reports the bug",
        );
        let answered = residue_scope_first_answer();
        residue_scope_commit(
            dir.path(),
            &doc,
            &residue_scope_doc(&answered, &struck_row),
            "cycle answers and strikes it",
        );
        residue_scope_commit(
            dir.path(),
            &doc,
            &residue_scope_doc(&answered, &report_row),
            "operator re-types the report",
        );
        let answered_again = format!(
            "{answered}### Re: session list flicker, again\n\n> **Queue prompt:** {RESIDUE_SCOPE_REPORT}\n\nThe second registration path is fixed too.\n\n"
        );
        residue_scope_commit(
            dir.path(),
            &doc,
            &residue_scope_doc(&answered_again, &report_row),
            "cycle answers the re-typed report; its strike was missed",
        );

        let _ = run_queue_maintenance(&doc, None).unwrap();
        let (active, completed) = residue_scope_queue_rows(&doc);
        assert!(
            !active.iter().any(|t| t.contains(RESIDUE_SCOPE_REPORT)),
            "a head answered after its re-entry is completed residue:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
        assert!(
            completed.iter().any(|t| t.contains(RESIDUE_SCOPE_REPORT)),
            "the answered head must be struck, not dropped: {completed:?}"
        );
        assert!(
            active.iter().any(|t| t.contains("do [#beta]")),
            "the unanswered id-backed head stays live: {active:?}"
        );
    }

    /// GH #327 pass 4 fixture: a priority `go` queue and
    /// an hourly `loop` queue in one session document.
    fn loop_scope_doc(exchange: &str, main: &str, looped: &str) -> String {
        format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue subagents priority id=queue-1 go -->\n{main}<!-- /agent:queue -->\n\n<!-- agent:queue loop interval=\"1h\" id=queue-2 -->\n{looped}<!-- /agent:queue -->\n"
        )
    }

    const LOOP_SCOPE_HEAD: &str = "#upgrade";
    const LOOP_SCOPE_T0: u64 = 1_800_000_000;

    /// Commit `content` as `session.md` with committer and author time
    /// `committed_secs`, then make it the committed snapshot.
    fn loop_scope_commit(dir: &Path, doc: &Path, content: &str, committed_secs: u64) {
        std::fs::write(doc, content).unwrap();
        let date = format!("@{committed_secs} +0000");
        for args in [
            vec!["add", "session.md"],
            vec!["-c", "commit.gpgsign=false", "commit", "-q", "-m", "cycle"],
        ] {
            let out = Command::new("git")
                .current_dir(dir)
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .args(&args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        agent_doc_snapshot_io::checkpoint_document_baseline(
            doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
    }

    /// Persist queue-2's schedule row, as the controller's scheduler does.
    fn loop_scope_store_window(
        dir: &Path,
        doc: &Path,
        window: agent_doc_queue::queue_schedule::ScheduleWindow,
    ) {
        let state = agent_doc_queue::queue_schedule::QueueScheduleState {
            owner_queue_id: Some("queue-2".to_string()),
            cadence: "interval=3600".to_string(),
            anchor_secs: LOOP_SCOPE_T0 - 7200,
            last_fired_due_secs: None,
            last_fired_at_secs: None,
            passes_completed: 3,
            last_pass_completed_secs: None,
            window,
            restore_pending: None,
            coalesced_ticks: 0,
            failures: 0,
            backoff_until_secs: None,
        };
        let conn = agent_doc_sqlite::state_store::open_state_db(dir).unwrap();
        agent_doc_sqlite::state_store::upsert_queue_document_state_in_db(
            &conn,
            &agent_doc_sqlite::state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(doc).unwrap(),
                state_kind: format!(
                    "{}:queue-2",
                    agent_doc_queue::queue_schedule::QUEUE_SCHEDULE_STATE_KIND
                ),
                canonical_path: doc.canonicalize().unwrap().to_string_lossy().into_owned(),
                payload_json: serde_json::to_string(&state).unwrap(),
                updated_at_secs: LOOP_SCOPE_T0,
            },
        )
        .unwrap();
        let loaded =
            agent_doc_controller_io::queue_scheduler::load_schedule_state_for_queue(doc, "queue-2")
                .unwrap()
                .expect("the fixture's schedule row is readable");
        assert_eq!(loaded, state);
    }

    fn loop_scope_open(opened_secs: u64) -> agent_doc_queue::queue_schedule::ScheduleWindow {
        agent_doc_queue::queue_schedule::ScheduleWindow::Open {
            pass: 4,
            opened_secs,
            roster: vec![LOOP_SCOPE_HEAD.to_string()],
            ran: Vec::new(),
        }
    }

    /// Queue `queue_id`'s live and struck rows, with the cosmetic `🚧`
    /// in-progress marker (`set_first_prompt_in_progress`) stripped.
    fn loop_scope_rows(doc: &Path, queue_id: &str) -> (Vec<String>, Vec<String>) {
        let text = std::fs::read_to_string(doc).unwrap();
        let unmarked = |rows: Vec<String>| -> Vec<String> {
            rows.iter()
                .map(|row| strip_in_progress_marker(row))
                .collect()
        };
        (
            unmarked(
                agent_doc_queue::queue_schedule::live_heads_for_queue(&text, queue_id).unwrap(),
            ),
            unmarked(
                agent_doc_queue::queue_schedule::completed_heads_for_queue(&text, queue_id)
                    .unwrap(),
            ),
        )
    }

    fn loop_scope_echo() -> String {
        format!("### Re: upgrade\n\n> **Queue prompt:** {LOOP_SCOPE_HEAD}\n\nUpgraded.\n\n")
    }

    /// The committed history GH #327 pass 4 had: both queues held `#upgrade`;
    /// a queue-1 cycle answered queue-1's copy and finalize struck it there
    /// (`#ftstrike`), leaving queue-2's copy live.
    fn loop_scope_history(dir: &Path, doc: &Path, answered_secs: u64) {
        let live = format!("- {LOOP_SCOPE_HEAD}\n");
        loop_scope_commit(dir, doc, &loop_scope_doc("", &live, &live), LOOP_SCOPE_T0);
        loop_scope_commit(
            dir,
            doc,
            &loop_scope_doc(
                &loop_scope_echo(),
                &format!(
                    "- ~~{LOOP_SCOPE_HEAD}~~ — auto-struck: answered this cycle (#ftstrike)\n"
                ),
                &live,
            ),
            answered_secs,
        );
    }

    fn loop_scope_ops(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(".agent-doc/logs/ops.log")).unwrap_or_default()
    }

    /// A delegated queue remains live while its workers run, but it must not
    /// hide an independent scheduled queue after that queue's window opens.
    /// This is the live `agent-doc-bugs.md` shape behind the unreliable loop:
    /// each hourly tick advanced durable schedule state while preflight kept
    /// selecting queue-1, then filtered every one of its heads as out-of-loop.
    #[test]
    fn inspect_queue_state_reaches_open_loop_behind_delegated_queue() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = loop_scope_doc(
            "",
            "- delegated main work\n",
            &format!("- {LOOP_SCOPE_HEAD}\n"),
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        loop_scope_store_window(dir.path(), &doc, loop_scope_open(LOOP_SCOPE_T0));

        let state = inspect_queue_state(&doc, None).unwrap();

        assert_eq!(state.queue_prompts, vec![LOOP_SCOPE_HEAD.to_string()]);
        assert_eq!(
            state.queue_supervisor_continuation_head,
            Some(LOOP_SCOPE_HEAD.to_string())
        );
        assert!(state.queue_supervisor_drainable);

        let maintained = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(maintained.queue_prompts, vec![LOOP_SCOPE_HEAD.to_string()]);
        assert_eq!(
            maintained.queue_supervisor_continuation_head,
            Some(LOOP_SCOPE_HEAD.to_string())
        );
    }

    /// GH #327 pass 4 (measured): the echo was committed
    /// before pass 4 opened, and it answered queue-1's copy of `#upgrade`. The
    /// loop-dispatched cycle's preflight struck queue-2's live head with it, so
    /// pass 4 never ran the item. Only an echo written after the pass opened
    /// may answer a `loop` head. Two histories: the measured one (echo and
    /// queue-1 strike in one commit), and one where queue-1's strike landed in
    /// a later commit, so only the pass opening tells the echo is history.
    #[test]
    fn residue_strike_keeps_a_loop_head_whose_echo_predates_the_pass() {
        for strike_in_later_commit in [false, true] {
            let dir = setup_project();
            let doc = dir.path().join("session.md");
            if strike_in_later_commit {
                let live = format!("- {LOOP_SCOPE_HEAD}\n");
                loop_scope_commit(
                    dir.path(),
                    &doc,
                    &loop_scope_doc("", &live, &live),
                    LOOP_SCOPE_T0,
                );
                loop_scope_commit(
                    dir.path(),
                    &doc,
                    &loop_scope_doc(&loop_scope_echo(), &live, &live),
                    LOOP_SCOPE_T0 + 100,
                );
                loop_scope_commit(
                    dir.path(),
                    &doc,
                    &loop_scope_doc(
                        &loop_scope_echo(),
                        &format!("- ~~{LOOP_SCOPE_HEAD}~~\n"),
                        &live,
                    ),
                    LOOP_SCOPE_T0 + 150,
                );
            } else {
                loop_scope_history(dir.path(), &doc, LOOP_SCOPE_T0 + 100);
            }
            loop_scope_store_window(dir.path(), &doc, loop_scope_open(LOOP_SCOPE_T0 + 200));

            let _ = run_queue_maintenance(&doc, None).unwrap();

            let (live, completed) = loop_scope_rows(&doc, "queue-2");
            assert_eq!(
                live,
                vec![LOOP_SCOPE_HEAD.to_string()],
                "strike_in_later_commit={strike_in_later_commit}: an echo committed before \
                 the pass opened must not strike the loop head (completed={completed:?}):\n{}",
                std::fs::read_to_string(&doc).unwrap()
            );
            let ops = loop_scope_ops(dir.path());
            assert!(
                ops.contains("preflight_freetext_residue_strike_skipped_loop_echo_outside_pass")
                    && ops.contains("anchors=pass_opened"),
                "strike_in_later_commit={strike_in_later_commit}: the skipped strike must be \
                 logged with its anchor:\n{ops}"
            );
            assert!(
                !ops.contains("preflight_freetext_residue_strike file="),
                "strike_in_later_commit={strike_in_later_commit}: nothing may be struck:\n{ops}"
            );
        }
    }

    /// GH #327 pass 4: an echo written after the pass opened, by a write
    /// that struck the same head in another queue, answered that queue's head.
    /// It does not strike the loop queue's copy.
    #[test]
    fn residue_strike_keeps_a_loop_head_answered_for_another_queue() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        loop_scope_history(dir.path(), &doc, LOOP_SCOPE_T0 + 100);
        loop_scope_store_window(dir.path(), &doc, loop_scope_open(LOOP_SCOPE_T0 + 50));

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let (live, completed) = loop_scope_rows(&doc, "queue-2");
        assert_eq!(
            live,
            vec![LOOP_SCOPE_HEAD.to_string()],
            "an echo about queue-1's head must not strike queue-2's copy \
             (completed={completed:?}):\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
        let ops = loop_scope_ops(dir.path());
        assert!(
            ops.contains("anchors=other_queue_echo"),
            "the skipped strike must name the other-queue anchor:\n{ops}"
        );
    }

    /// GH #327 pass 4: with no pass open the scheduler holds the loop
    /// head, so no echo can have answered it in this pass. A strike here would
    /// lose the item for good: no roster exists to restore it.
    #[test]
    fn residue_strike_keeps_a_loop_head_while_no_pass_is_open() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let live = format!("- {LOOP_SCOPE_HEAD}\n");
        loop_scope_commit(
            dir.path(),
            &doc,
            &loop_scope_doc("", "", &live),
            LOOP_SCOPE_T0,
        );
        loop_scope_commit(
            dir.path(),
            &doc,
            &loop_scope_doc(&loop_scope_echo(), "", &live),
            LOOP_SCOPE_T0 + 100,
        );
        for stored in [false, true] {
            if stored {
                loop_scope_store_window(
                    dir.path(),
                    &doc,
                    agent_doc_queue::queue_schedule::ScheduleWindow::Closed,
                );
            }
            let _ = run_queue_maintenance(&doc, None).unwrap();
            let (live_rows, completed) = loop_scope_rows(&doc, "queue-2");
            assert_eq!(
                live_rows,
                vec![LOOP_SCOPE_HEAD.to_string()],
                "stored={stored}: a held loop head must not be struck \
                 (completed={completed:?}):\n{}",
                std::fs::read_to_string(&doc).unwrap()
            );
        }
        assert!(
            loop_scope_ops(dir.path()).contains("anchors=no_open_pass"),
            "{}",
            loop_scope_ops(dir.path())
        );
    }

    /// GH #327 pass 4 positive control: the loop pass's own cycle answered
    /// the loop head after the pass opened, and no other queue held it. The
    /// catch-up still strikes it, and the drained `loop` queue keeps the struck
    /// row for the restore (GH #327).
    #[test]
    fn residue_strike_strikes_a_loop_head_answered_in_its_own_pass() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let live = format!("- {LOOP_SCOPE_HEAD}\n");
        loop_scope_commit(
            dir.path(),
            &doc,
            &loop_scope_doc("", "", &live),
            LOOP_SCOPE_T0,
        );
        loop_scope_commit(
            dir.path(),
            &doc,
            &loop_scope_doc(&loop_scope_echo(), "", &live),
            LOOP_SCOPE_T0 + 100,
        );
        loop_scope_store_window(dir.path(), &doc, loop_scope_open(LOOP_SCOPE_T0 + 50));

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let (live_rows, completed) = loop_scope_rows(&doc, "queue-2");
        assert!(
            live_rows.is_empty(),
            "the pass's own echo answers the loop head:\n{}",
            std::fs::read_to_string(&doc).unwrap()
        );
        assert_eq!(completed, vec![LOOP_SCOPE_HEAD.to_string()]);
        assert!(
            loop_scope_ops(dir.path()).contains("preflight_freetext_residue_strike file="),
            "{}",
            loop_scope_ops(dir.path())
        );
    }

    /// `#claimstrike` (live, 2026-10-05): a committed `Queue prompt` echo with
    /// no deferral completes a claimed free-text head too. Exempting claimed
    /// heads from this catch-up disagreed with the session-check residue guard
    /// (which ignores claims), so the repair needed a hand `queue release`
    /// first. A head whose latest echo is a deferral stays queued, claimed, and
    /// visibly marked in progress (`#claimmarker`).
    #[test]
    fn run_queue_maintenance_strikes_claimed_answered_head_and_keeps_deferred_one() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let done = "Alt+Shift+Menu does not open the agent-doc action menu";
        let pending = "Ctrl+Shift+A palette entry is missing for agent-doc";
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "queue: start\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: menu — opus\n\n",
                "> **Queue prompt:** {done}\n\n",
                "Integrated the subagent's fix; the menu opens again.\n\n",
                "### Re: palette — opus\n\n",
                "> **Queue prompt:** {pending}\n\n",
                "**Deferred:** dispatched to subagent:palette; not landed yet.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n",
                "- {done}\n",
                "- {pending}\n",
                "<!-- /agent:queue -->\n",
            ),
            done = done,
            pending = pending,
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, done, "subagent:altshiftmenu", 3600).unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, pending, "subagent:palette", 3600).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains(&format!("- ~~{done}~~")), "{updated}");
        assert!(
            updated.contains(&format!("- 🟡 [#subagent:palette#1] {pending}\n")),
            "{updated}"
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        let owners: Vec<&str> = ledger.claims.iter().map(|c| c.owner.as_str()).collect();
        assert_eq!(
            owners,
            vec!["subagent:palette"],
            "the struck head's claim is released; the deferred head stays claimed"
        );
    }

    /// Claim, start, and report one worker attempt on `item`, returning its
    /// binary completion cell (the `### Re: Subagent completed` exchange cell).
    fn reported_attempt_with_cell(doc: &std::path::Path, item: &str, owner: &str) -> String {
        let (_, claimed) = agent_doc_queue_io::queue_claim::claim(doc, item, owner, 3600).unwrap();
        let running = agent_doc_queue_io::queue_claim::start_attempt(
            doc,
            item,
            owner,
            &format!("worker:{owner}"),
            3600,
            claimed.generation,
        )
        .unwrap();
        let cell = agent_doc_document::subagent_completion::cell_with_response(
            &running.owner,
            &running.item_text,
            &agent_doc_queue_io::queue_claim::completion_feedback_key(&running).unwrap(),
            "Implemented and pushed; tests pass.",
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::report_attempt(doc, item, Some(running.generation))
            .unwrap();
        cell
    }

    /// `#strikeisterminal` (live 2026-10-26 15:03-15:07Z,
    /// tasks/agent-doc/agent-doc-bugs.md, cycle-1793026977143): the `main`
    /// queue held reported free-text rows on both sides of a pasted fenced
    /// block whose body has a bare `<!-- agent:run priority subagents -->`
    /// line. The closeout quoted every row, and the preflight residue strike
    /// struck all of them (`struck=24`, claims `closed_by_strike`). The same
    /// pass's lifecycle projection then reopened every struck row that still
    /// had a completion cell or a reported-release receipt as `❔`, and the
    /// next claim projection painted those `🔄` (cell) or `☑️` (receipt), so
    /// a go-mode queue offered finished work for fresh dispatch. The fenced
    /// marker was not the cause: every component scanner keeps one queue
    /// here. A strike stays struck across repeated passes.
    #[test]
    fn run_queue_maintenance_keeps_struck_reported_rows_struck_around_a_fenced_run_marker() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let before = "tmux pane auto sync is not working. Sync Tmux Panes works.";
        let title = "Run Agent Doc on sdk-demo.md still wedged even after I corrected the issue:";
        let after = "The agent:loop is getting items sent to it, but the items should have been sent into `agent:run`.";
        let orphan = "Rename \"## Completed / Reaped\" to \"## Done\"";
        let fenced = concat!(
            "```\n",
            "In sdk-demo.md on disk, the Run section is:\n",
            "\n",
            "## Run\n",
            "<!-- agent:run priority subagents -->\n",
            "◇ markdown   [Copy]\n",
            "\n",
            "The marker is alone on its line, and <!-- /agent:run --> is alone on its line.\n",
            "\n",
            "## Run <!-- agent:run priority subagents -->\n",
            "```\n",
        );
        let shell = |exchange: &str, main: &str| {
            format!(
                concat!(
                    "---\n",
                    "agent_doc_session: test\n",
                    "agent_doc_format: template\n",
                    "agent_doc_write: crdt\n",
                    "queue: start\n",
                    "---\n\n",
                    "<!-- agent:exchange patch=append -->\n",
                    "{exchange}",
                    "<!-- /agent:exchange -->\n\n",
                    "## Queue: Main\n",
                    "<!-- agent:run id=\"main\" subagents go -->\n",
                    "{main}",
                    "<!-- /agent:run -->\n\n",
                    "## Run: Issues\n",
                    "<!-- agent:loop id=issues interval=1h priority -->\n",
                    "- 📥 We need to work on making agent-doc easy to onboard.\n",
                    "<!-- /agent:loop -->\n",
                ),
                exchange = exchange,
                main = main,
            )
        };
        let bare_main = format!(
            "- {before}\n- {orphan}\n---\n{title}\n{fenced}---\n- {after}\n- 📥 still pending work\n"
        );
        std::fs::write(&doc, shell("", &bare_main)).unwrap();

        // Worker cells for both rows; `before`'s claim stays Reported (cell
        // only) and `after`'s is released with a reported-release receipt,
        // the `lazily_block_reason` / `release_promotion_concurrency` shape.
        let before_cell = reported_attempt_with_cell(&doc, before, "subagent:tmux_auto_sync");
        let after_cell = reported_attempt_with_cell(&doc, after, "subagent:loop_routing");
        agent_doc_queue_io::claim_release_receipt::release(
            &doc,
            after,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
        )
        .unwrap();
        // `orphan` was struck by an earlier binary strike (closeout / `queue
        // consume --id`) and still carries a reported-release receipt.
        let _orphan_cell = reported_attempt_with_cell(&doc, orphan, "subagent:done_verify");
        agent_doc_queue_io::claim_release_receipt::release(
            &doc,
            orphan,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
        )
        .unwrap();

        let exchange = format!(
            concat!(
                "{before_cell}\n\n{after_cell}\n\n",
                "### Re: cleanup the agent:loop + agent:run items — opus\n\n",
                "> **Queue prompt:** {before}\n\n",
                "> **Queue prompt:** {after}\n\n",
                "> **Queue prompt:** {orphan}\n\n",
                "Integrated and verified every reported row.\n",
            ),
            before_cell = before_cell,
            after_cell = after_cell,
            before = before,
            after = after,
            orphan = orphan,
        );
        let main = format!(
            concat!(
                "- ☑️ [#subagent:tmux_auto_sync#1] {before}\n",
                "- ~~{orphan}~~\n",
                "---\n",
                "{title}\n",
                "{fenced}",
                "---\n",
                "- ☑️ [#subagent:loop_routing#1] {after}\n",
                "- 📥 still pending work\n",
            ),
            before = before,
            orphan = orphan,
            title = title,
            fenced = fenced,
            after = after,
        );
        let content = shell(&exchange, &main);
        // One queue scanner verdict: the fenced marker never opens a component.
        let components = agent_doc_element::element::parse(&content).unwrap();
        assert_eq!(
            components.iter().filter(|c| c.name == "queue").count(),
            2,
            "main + issues only; the fenced `<!-- agent:run -->` is content"
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        for pass in 0..3 {
            let _ = run_queue_maintenance(&doc, None).unwrap();
            let updated = std::fs::read_to_string(&doc).unwrap();
            let main_body = {
                let comps = agent_doc_element::element::parse(&updated).unwrap();
                let main = agent_doc_queue::queue_set::component_for_id(&updated, &comps, "main")
                    .unwrap()
                    .expect("main queue");
                main.content(&updated).to_string()
            };
            for head in [before, after, orphan] {
                assert!(
                    main_body.contains(&format!("- ~~{head}~~\n")),
                    "pass {pass}: a struck reported row must stay struck: {head:?}\n{main_body}"
                );
                for resurrected in ["❔", "🔄", "☑️"] {
                    assert!(
                        !main_body
                            .lines()
                            .any(|line| line.contains(head) && line.contains(resurrected)),
                        "pass {pass}: {head:?} came back as {resurrected}\n{main_body}"
                    );
                }
            }
            assert!(
                main_body.contains(fenced),
                "pass {pass}: the pasted fenced block is preserved byte-for-byte\n{main_body}"
            );
            assert!(
                main_body.contains("still pending work"),
                "pass {pass}: unanswered work stays queued\n{main_body}"
            );
        }
    }

    /// `#strikeretires` (live 2026-10-26: a 52h-old `release_publish_patch`
    /// cell painted `🔄 release + publish` onto a new release request). The
    /// preflight residue strike closes a reported row, but before only
    /// closeout `queue consume` retired the row's completion cell and only
    /// closeout pruned its reported-release receipt, so a later row with the
    /// same text inherited `☑️` (receipt) or `🔄` (cell). The strike's own
    /// maintenance successor now retires the cell in the document and the
    /// snapshot, the receipt ends once it persists, and the re-added row
    /// projects `📥`. Operator exchange text stays byte for byte.
    #[test]
    fn residue_strike_retires_cell_and_receipt_so_a_readded_row_is_pending() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let item = "release + publish";
        let pending = "still pending work";
        let shell = |exchange: &str, main: &str| {
            format!(
                concat!(
                    "---\n",
                    "agent_doc_session: test\n",
                    "agent_doc_format: template\n",
                    "agent_doc_write: crdt\n",
                    "queue: start\n",
                    "---\n\n",
                    "<!-- agent:exchange patch=append -->\n",
                    "{exchange}",
                    "<!-- /agent:exchange -->\n\n",
                    "## Queue: Main\n",
                    "<!-- agent:run id=\"main\" subagents go -->\n",
                    "{main}",
                    "<!-- /agent:run -->\n",
                ),
                exchange = exchange,
                main = main,
            )
        };
        std::fs::write(&doc, shell("", &format!("- {item}\n- {pending}\n"))).unwrap();
        let cell = reported_attempt_with_cell(&doc, item, "subagent:release_publish_patch");
        agent_doc_queue_io::claim_release_receipt::release(
            &doc,
            item,
            agent_doc_queue_io::claim_release_receipt::ReleaseKind::Reported,
        )
        .unwrap();
        let answer = format!(
            "### Re: release — opus\n\n> **Queue prompt:** {item}\n\nReleased and verified.\n"
        );
        let content = shell(
            &format!("{cell}\n\n{answer}"),
            &format!("- ☑️ [#subagent:release_publish_patch#1] {item}\n- 📥 {pending}\n"),
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let receipts = || {
            agent_doc_queue_io::claim_release_receipt::load(&doc)
                .unwrap()
                .receipts
                .len()
        };
        assert_eq!(receipts(), 1);

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(&format!("- ~~{item}~~\n")),
            "the residue strike closes the answered row:\n{updated}"
        );
        assert!(
            !updated.contains(&cell),
            "the struck row's completion cell retires with the strike:\n{updated}"
        );
        assert!(
            updated.contains(&answer),
            "the integrated answer stays:\n{updated}"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !snapshot.contains(&cell) && snapshot.contains(&answer),
            "the snapshot retires the same cell:\n{snapshot}"
        );
        assert_eq!(
            receipts(),
            0,
            "the strike retires the reported-release receipt"
        );

        // A later operator request with the same text is new work.
        let readded = updated.replace(
            &format!("- ~~{item}~~\n"),
            &format!("- ~~{item}~~\n- {item}\n"),
        );
        assert!(
            agent_doc_queue_io::queue_claim::completion_evidence_head_texts(&readded).is_empty()
        );
        let projection =
            agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection(&doc, &readded, None)
                .unwrap();
        assert!(
            projection
                .target_content
                .contains(&format!("- 📥 {item}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            !projection.target_content.contains("🔄")
                && !projection.target_content.contains("- ☑️"),
            "{}",
            projection.target_content
        );
    }

    /// The re-added row `item` in `readded` projects `📥`, never `☑️`/`🔄`.
    fn assert_readded_row_projects_pending(doc: &std::path::Path, readded: &str, item: &str) {
        let projection =
            agent_doc_queue_io::queue_cmd::derive_claim_runtime_projection(doc, readded, None)
                .unwrap();
        assert!(
            projection
                .target_content
                .contains(&format!("- 📥 {item}\n")),
            "{}",
            projection.target_content
        );
        assert!(
            !projection.target_content.contains("🔄")
                && !projection.target_content.contains("- ☑️"),
            "{}",
            projection.target_content
        );
    }

    fn claim_texts(doc: &std::path::Path) -> Vec<String> {
        agent_doc_queue_io::queue_claim::load_ledger(doc)
            .unwrap()
            .claims
            .into_iter()
            .map(|claim| claim.item_text)
            .collect()
    }

    /// `#strikeretires` (found by the completion-cell retirement fix,
    /// 2026-10-26): the done-id strike (`#qdonestrike`) closed a reported
    /// `do [#alpha]` row, but its `Reported` attempt stayed in the claim
    /// ledger until the next closeout prune, so a re-added identical row
    /// projected `☑️`. The strike's maintenance successor now ends it.
    #[test]
    fn done_id_strike_ends_a_reported_claim_so_a_readded_row_is_pending() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let item = "do [#alpha]";
        let owner = "subagent:alpha";
        let shell = |queue: &str| {
            format!(
                concat!(
                    "---\n",
                    "agent_doc_session: test\n",
                    "agent_doc_format: template\n",
                    "agent_doc_write: crdt\n",
                    "queue_active: true\n",
                    "---\n\n",
                    "<!-- agent:exchange patch=append -->\n",
                    "### Re: prior — gpt-5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue auto -->\n",
                    "{queue}",
                    "<!-- /agent:queue -->\n\n",
                    "<!-- agent:done -->\n",
                    "- [x] [#alpha] First done.\n",
                    "<!-- /agent:done -->\n",
                ),
                queue = queue,
            )
        };
        std::fs::write(&doc, shell(&format!("- {item}\n- do [#beta]\n"))).unwrap();
        let _cell = reported_attempt_with_cell(&doc, item, owner);
        let content = shell(&format!("- ☑️ [#{owner}#1] {item}\n- do [#beta]\n"));
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert_eq!(claim_texts(&doc), vec![item.to_string()]);

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(&format!("- ~~{item}~~")),
            "the done strike closes the reported row:\n{updated}"
        );
        assert!(
            claim_texts(&doc).is_empty(),
            "the strike ends the reported claim"
        );
        let readded = updated.replace("- 🚧 do [#beta]\n", &format!("- 🚧 do [#beta]\n- {item}\n"));
        assert_ne!(readded, updated, "{updated}");
        assert_readded_row_projects_pending(&doc, &readded, item);
    }

    /// `#strikeretires`: the backlog/done strike (`#qftbklgstrike`) bakes its
    /// note inside the strikethrough. Before, that note made the struck row's
    /// identity differ from the live row's, so the strike retired nothing and
    /// the row's `Reported` attempt outlived it. It now ends with the strike,
    /// and a re-added identical row projects `📥`. (The row is not yet
    /// lifecycle-projected: the semantic scorer reads a projected
    /// `[#owner#N]` annotation as an id and leaves the row to the id paths.)
    #[test]
    fn backlog_strike_ends_a_reported_claim_so_a_readded_row_is_pending() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let item = "Fix JB File Cache Conflict dialogs on every save";
        let owner = "subagent:jbcache";
        let shell = |queue: &str| {
            format!(
                concat!(
                    "---\n",
                    "agent_doc_session: test\n",
                    "agent_doc_format: template\n",
                    "agent_doc_write: crdt\n",
                    "queue: start\n",
                    "---\n\n",
                    "<!-- agent:exchange patch=append -->\n",
                    "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue priority go -->\n",
                    "{queue}",
                    "<!-- /agent:queue -->\n\n",
                    "<!-- agent:backlog priority queue -->\n",
                    "- [ ] [#stillopen] unrelated open work item\n",
                    "<!-- /agent:backlog -->\n\n",
                    "<!-- agent:done archive=tasks.done.md -->\n",
                    "<!-- /agent:done -->\n",
                ),
                queue = queue,
            )
        };
        std::fs::write(&doc, shell(&format!("- {item}\n- do [#stillopen]\n"))).unwrap();
        let _cell = reported_attempt_with_cell(&doc, item, owner);
        let content = shell(&format!("- {item}\n- do [#stillopen]\n"));
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert_eq!(claim_texts(&doc), vec![item.to_string()]);

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let struck =
            format!("- ~~{item} — auto-struck: completed by #jbcache (#qftbklgstrike)~~\n");
        assert!(
            updated.contains(&struck),
            "the backlog/done strike closes the reported row:\n{updated}"
        );
        assert!(
            claim_texts(&doc).is_empty(),
            "the strike ends the reported claim"
        );
        let readded = updated.replace(&struck, &format!("{struck}- {item}\n"));
        assert_readded_row_projects_pending(&doc, &readded, item);
    }

    /// `#qftbklgclaimed` (#decisionbacklogstrike): the backlog/done strike
    /// (`#qftbklgstrike`) leaves a free-text row that already shows a claim
    /// projection (`🟡`/`🚧`/`☑️ [#owner#N]`) alone, even when a done item
    /// restates it. That row's work closes through coordinator integration
    /// and closeout, not through a lexical match: striking a `☑️` row here
    /// would retire its completion cell (the worker's only in-document
    /// report) and its `Reported` claim before anyone integrated it, and a
    /// `Claimed`/`Running` row would reopen as `❔` (`#strikeisterminal`).
    /// Whether this strike should close claimed rows is an open operator
    /// decision; this pins today's behaviour so a change to it is deliberate.
    #[test]
    fn backlog_strike_leaves_claim_projected_free_text_rows_and_their_evidence() {
        for (marker, report) in [("🟡", None), ("🚧", Some(false)), ("☑️", Some(true))] {
            let dir = setup_project();
            std::fs::write(
                dir.path().join("tasks.done.md"),
                "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
            )
            .unwrap();
            let doc = dir.path().join("session.md");
            let item = "Fix JB File Cache Conflict dialogs on every save";
            let owner = "subagent:jbcache";
            let shell = |exchange_extra: &str, queue: &str| {
                format!(
                    concat!(
                        "---\n",
                        "agent_doc_session: test\n",
                        "agent_doc_format: template\n",
                        "agent_doc_write: crdt\n",
                        "queue: start\n",
                        "---\n\n",
                        "<!-- agent:exchange patch=append -->\n",
                        "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
                        "{exchange_extra}",
                        "<!-- /agent:exchange -->\n\n",
                        "<!-- agent:queue priority go -->\n",
                        "{queue}",
                        "<!-- /agent:queue -->\n\n",
                        "<!-- agent:backlog priority queue -->\n",
                        "- [ ] [#stillopen] unrelated open work item\n",
                        "<!-- /agent:backlog -->\n\n",
                        "<!-- agent:done archive=tasks.done.md -->\n",
                        "<!-- /agent:done -->\n",
                    ),
                    exchange_extra = exchange_extra,
                    queue = queue,
                )
            };
            std::fs::write(&doc, shell("", &format!("- {item}\n- do [#stillopen]\n"))).unwrap();
            let (_, claimed) =
                agent_doc_queue_io::queue_claim::claim(&doc, item, owner, 3600).unwrap();
            let mut cell = String::new();
            if let Some(reported) = report {
                let running = agent_doc_queue_io::queue_claim::start_attempt(
                    &doc,
                    item,
                    owner,
                    &format!("worker:{owner}"),
                    3600,
                    claimed.generation,
                )
                .unwrap();
                if reported {
                    cell = agent_doc_document::subagent_completion::cell_with_response(
                        &running.owner,
                        &running.item_text,
                        &agent_doc_queue_io::queue_claim::completion_feedback_key(&running)
                            .unwrap(),
                        "Implemented and pushed; tests pass.",
                    )
                    .unwrap();
                    agent_doc_queue_io::queue_claim::report_attempt(
                        &doc,
                        item,
                        Some(running.generation),
                    )
                    .unwrap();
                }
            }
            let exchange_extra = if cell.is_empty() {
                String::new()
            } else {
                format!("\n{cell}\n")
            };
            let content = shell(
                &exchange_extra,
                &format!("- {marker} [#{owner}#1] {item}\n- do [#stillopen]\n"),
            );
            std::fs::write(&doc, &content).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                &content,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();
            assert_eq!(claim_texts(&doc), vec![item.to_string()], "{marker}");

            let _ = run_queue_maintenance(&doc, None).unwrap();

            let updated = std::fs::read_to_string(&doc).unwrap();
            assert!(
                !updated.contains("#qftbklgstrike") && !updated.contains(&format!("~~{item}")),
                "{marker}: a claim-projected row is not struck by the backlog/done strike:\n{updated}"
            );
            assert!(
                updated.contains(&format!("[#{owner}#1] {item}\n")),
                "{marker}: the claimed row stays live with its owner:\n{updated}"
            );
            assert_eq!(
                claim_texts(&doc),
                vec![item.to_string()],
                "{marker}: the claim survives"
            );
            if !cell.is_empty() {
                assert!(
                    updated.contains("Implemented and pushed; tests pass."),
                    "{marker}: the unintegrated worker report survives:\n{updated}"
                );
            }
        }
    }

    #[test]
    fn run_queue_maintenance_keeps_reported_head_when_only_worker_echoes_it() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let head = "Fix queue rows regressing from reported to pending";
        let content = format!(
            concat!(
                "---\n",
                "agent_doc_session: test\n",
                "agent_doc_format: template\n",
                "agent_doc_write: crdt\n",
                "queue: start\n",
                "---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue go -->\n",
                "- {head}\n",
                "<!-- /agent:queue -->\n",
            ),
            head = head,
        );
        std::fs::write(&doc, &content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let (_, claimed) =
            agent_doc_queue_io::queue_claim::claim(&doc, head, "subagent:queue-state", 3600)
                .unwrap();
        let running = agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            head,
            "subagent:queue-state",
            "worker:queue-state",
            3600,
            claimed.generation,
        )
        .unwrap();
        let worker_response = format!(
            "> **Queue prompt:** {head}\n\nImplemented the worker-side fix and tests pass."
        );
        let completion = agent_doc_document::subagent_completion::cell_with_response(
            &running.owner,
            &running.item_text,
            &agent_doc_queue_io::queue_claim::completion_feedback_key(&running).unwrap(),
            &worker_response,
        )
        .unwrap();
        let with_completion = content.replace(
            "<!-- /agent:exchange -->",
            &format!("{completion}\n<!-- /agent:exchange -->"),
        );
        std::fs::write(&doc, &with_completion).unwrap();
        agent_doc_queue_io::queue_claim::report_attempt(&doc, head, Some(running.generation))
            .unwrap();

        let queue_state = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(&format!(
                "- ☑️ [#subagent:queue-state#{}] {head}",
                running.generation
            )),
            "worker completion feedback must remain a reported handoff:\n{updated}"
        );
        assert!(
            !updated.contains(&format!("~~{head}")),
            "the worker's nested queue echo must not strike the row:\n{updated}"
        );
        let ledger = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        assert_eq!(
            ledger.claims.len(),
            1,
            "reported claim must not be released"
        );
        assert_eq!(
            ledger.claims[0].state,
            agent_doc_queue::queue_claim::QueueClaimState::Reported
        );
        assert_eq!(
            queue_state.selected_queue_prompts,
            vec![head.to_string()],
            "a reported attempt is coordinator writeback debt, not in-flight worker work: {queue_state:#?}"
        );
        assert_eq!(queue_state.queue_drainable_head_count, 1);
        assert!(queue_state.queue_continuation_required);
    }

    #[test]
    fn run_queue_maintenance_strikes_prior_distinctive_identifier_heading() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: Why `_load_identity_age_consent_embedded_runtime` was deleted in FPE PR #12\n\n",
            "The later duplicate helper shadowed the canonical implementation.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Please help me understand why you deleted ",
            "_load_identity_age_consent_embedded_runtime in ",
            "https://github.com/haiven-dev/fpe/pull/12/changes#diff-example.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|component| component.name == "queue")
            .map(|queue| updated[queue.open_end..queue.close_start].to_string())
            .unwrap();
        assert!(
            queue_body.trim().is_empty()
                && !updated.contains("queue: start")
                && !updated.contains("queue: stop"),
            // #queuestopretire: the drain clears the control instead of writing stop.
            "the sole historical completed residue must strike and drain the queue:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_keeps_unanswered_free_text_head_active() {
        // #qheadresidue guard: the catch-up strike must NOT strike a free-text
        // head the exchange does not answer — only genuine completed residue is
        // removed, never a live operator report still awaiting a response.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: something unrelated — opus-4-8\n\n",
            "An answer about a completely different topic entirely.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- :pushpin: Still getting JB File Cache Conflict dialogs on every save\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        let entries = agent_doc_queue::document_queue::parse(&queue_body).unwrap();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active
                .iter()
                .any(|t| t.contains("Still getting JB File Cache Conflict dialogs")),
            "unanswered free-text head must stay active (not falsely struck):\n{active:?}"
        );
        assert!(
            !updated.contains("auto-struck"),
            "no head should be struck when the exchange answers none of them:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_exempts_recurring_imperative_deploy_head() {
        // #qimpstrike: a recurring-imperative command head (`deploy`) is a standing
        // executable directive. A PRIOR cycle answered a `deploy` request and left a
        // `> **Queue prompt:** deploy` echo in committed exchange; the operator then
        // re-adds `deploy` to run it again. The preflight #qheadresidue catch-up
        // strike must NOT retire that fresh directive — the head stays an active
        // Prompt so the next dispatch executes it. This keeps the preflight strike
        // set aligned with the session-check residue guard
        // (`free_text_queue_head_is_completed_residue`), which already exempts it.
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deploy — opus-4-8\n\n",
            "> **Queue prompt:** deploy\n\n",
            "Deployed successfully.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- deploy\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let entries = read_queue_entries(&doc);
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        // The head is now the active drain target, so it carries the `🚧`
        // in-progress marker — it stays an active Prompt, which is the point.
        assert!(
            active.iter().any(|t| t.contains("deploy")),
            "recurring-imperative `deploy` head must stay an active Prompt:\nactive={active:?}\n{updated}"
        );
        assert!(
            !updated.contains("auto-struck"),
            "recurring-imperative `deploy` head must not be struck as residue:\n{updated}"
        );
    }

    /// Test helper: read the queue entries from the on-disk document.
    fn read_queue_entries(doc: &Path) -> Vec<agent_doc_queue::document_queue::QueueEntry> {
        let updated = std::fs::read_to_string(doc).unwrap();
        let queue_body = agent_doc_element::element::parse(&updated)
            .unwrap()
            .iter()
            .find(|c| c.name == "queue")
            .map(|q| updated[q.open_end..q.close_start].to_string())
            .unwrap();
        agent_doc_queue::document_queue::parse(&queue_body).unwrap()
    }

    #[test]
    fn run_queue_maintenance_strikes_free_text_head_completed_by_done_item() {
        // #qftbklgstrike case (a): a LIVE free-text queue head (never answered in
        // the exchange) that restates a completed `agent:done` item is struck in
        // place, annotated "completed by #<id>".
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Fix JB File Cache Conflict dialogs on every save\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        // A second active id-backed head keeps the queue from fully draining, so
        // the struck residue stays in the body (the drain-clear that wipes a
        // fully-emptied queue is existing convergence behavior, not part of this
        // strike). The free-text head is now Completed + annotated.
        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            !active
                .iter()
                .any(|t| t.contains("Fix JB File Cache Conflict dialogs")),
            "struck head must no longer be an active Prompt:\nactive={active:?}"
        );
        assert!(
            completed.iter().any(|t| {
                t.contains("Fix JB File Cache Conflict dialogs")
                    && t.contains("auto-struck: completed by #jbcache (#qftbklgstrike)")
            }),
            "head must be struck + annotated 'completed by #jbcache':\ncompleted={completed:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_clears_in_progress_marker_when_free_text_head_struck() {
        // #qftstuck: an in-progress (🚧-marked) free-text queue head that is then
        // struck by the #qftbklgstrike backlog/done convergence must NOT keep the
        // 🚧 marker baked inside the strikethrough, and the marker must not be
        // stranded on an unrelated head either. The genuinely-active next head
        // (`do [#stillopen]`) DOES keep 🚧 (it is now the in-progress head).
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- 🚧 Fix JB File Cache Conflict dialogs on every save\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        // The struck head retains its prose + annotation but NOT the 🚧 marker.
        assert!(
            completed.iter().any(|t| {
                t.contains("Fix JB File Cache Conflict dialogs")
                    && t.contains("auto-struck: completed by #jbcache (#qftbklgstrike)")
                    && !t.contains(IN_PROGRESS_MARKER)
            }),
            "struck head must be annotated with NO 🚧 marker:\ncompleted={completed:?}"
        );
        // The 🚧 marker is not stranded on any struck/completed entry.
        assert!(
            !completed.iter().any(|t| t.contains(IN_PROGRESS_MARKER)),
            "no completed entry may carry 🚧:\ncompleted={completed:?}"
        );
        // The newly-promoted active head is the in-progress head now.
        assert!(
            active
                .iter()
                .filter(|t| t.contains(IN_PROGRESS_MARKER))
                .count()
                == 1,
            "exactly one active head should carry 🚧 (the new in-progress head):\nactive={active:?}"
        );
        assert!(
            active
                .iter()
                .any(|t| t.contains("[#stillopen]") && t.contains(IN_PROGRESS_MARKER)),
            "the genuinely-active next head should be the 🚧 in-progress head:\nactive={active:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_strikes_free_text_head_tracked_by_backlog_item() {
        // #qftbklgstrike case (b): a LIVE free-text queue head that restates an
        // active `agent:backlog` item is struck in place, annotated "tracked by
        // backlog #<id>".
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Opencode responses are reverse ordering numeric lists\n",
            "- do [#stillopen]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#revlist] Opencode responses are reverse ordering numeric lists\n",
            "- [ ] [#stillopen] unrelated open work item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let entries = read_queue_entries(&doc);
        let completed: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                agent_doc_queue::document_queue::QueueEntry::Completed(p) => Some(p.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            completed.iter().any(|t| {
                t.contains("Opencode responses are reverse ordering numeric lists")
                    && t.contains("auto-struck: tracked by backlog #revlist (#qftbklgstrike)")
            }),
            "head must be struck + annotated 'tracked by backlog #revlist':\ncompleted={completed:?}"
        );
    }

    #[test]
    fn run_queue_maintenance_does_not_strike_unrelated_operator_prompt() {
        // #qftbklgstrike false-strike safety: an unrelated operator prompt that is
        // NOT a restatement of any done/backlog item stays an active Prompt and is
        // never annotated/struck, even with done + backlog items present.
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer about a different topic.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- Please add a dark mode toggle to the settings panel\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#revlist] Opencode responses are reverse ordering numeric lists\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        let entries = read_queue_entries(&doc);
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active
                .iter()
                .any(|t| t.contains("Please add a dark mode toggle")),
            "unrelated operator prompt must stay active (NEVER silently buried):\n{active:?}"
        );
        assert!(
            !updated.contains("#qftbklgstrike"),
            "no #qftbklgstrike annotation should appear for an unrelated prompt:\n{updated}"
        );
    }

    #[test]
    fn active_go_maintenance_coins_midline_id_mention_and_plain_sibling() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#existing] Existing work\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let current = baseline.replace(
            "- do [#existing]\n",
            "- do [#existing]\n- verify [#existing] yourself\n- Why is the sibling missing?\n",
        );
        std::fs::write(&doc, current).unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("verify [#existing] yourself"), "{updated}");
        assert!(updated.contains("Why is the sibling missing?"), "{updated}");
        assert_eq!(updated.matches("[#existing] Existing work").count(), 1);
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(
            agent_doc_element::element::parse(&updated)
                .unwrap()
                .iter()
                .find(|component| component.name == "backlog")
                .unwrap()
                .content(&updated),
        );
        assert_eq!(items.len(), 3, "{items:?}");
        assert!(
            items
                .iter()
                .any(|item| item.text == "verify [#existing] yourself")
        );
        assert!(
            items
                .iter()
                .any(|item| item.text == "Why is the sibling missing?")
        );
        assert_eq!(items.iter().filter(|item| item.id == "existing").count(), 1);
    }

    #[test]
    fn run_queue_maintenance_qftbklgstrike_leaves_id_backed_heads_untouched() {
        // #qftbklgstrike: id-backed heads have their own done-strike path; the
        // free-text strike must not annotate them with the #qftbklgstrike marker.
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#jbcache] Fix JB File Cache Conflict dialogs on every save\n",
        )
        .unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "queue: start\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: unrelated — opus-4-8\n\nAn answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#open1] still-open work\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#open1] still-open work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _ = run_queue_maintenance(&doc, None).unwrap();

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !updated.contains("#qftbklgstrike"),
            "id-backed head must not be touched by the #qftbklgstrike free-text path:\n{updated}"
        );
        let entries = read_queue_entries(&doc);
        let active: Vec<String> = agent_doc_queue::document_queue::prompts(&entries)
            .iter()
            .map(|p| p.text.clone())
            .collect();
        assert!(
            active.iter().any(|t| t.contains("do [#open1]")),
            "unanswered id-backed head must remain active:\n{active:?}"
        );
    }

    /// The diagnostic pair the classifier owns. Asserting through it — rather than
    /// re-spelling the `reason` / `action` token text here — keeps them in one place.
    fn expected_fields(
        divergence: agent_doc_document::admission_divergence::AdmissionDivergence,
        adoption: agent_doc_document::admission_divergence::DiskAdoption,
    ) -> String {
        agent_doc_document::admission_divergence::AdmissionAssessment {
            divergence,
            adoption,
        }
        .diagnostic_fields()
    }

    /// Set up a document whose durable baseline, disk file, and (caller-supplied)
    /// live authority are all under test control, then run the admission gate.
    fn admission_gate(
        baseline: &str,
        disk: &str,
        authority: &str,
    ) -> (TempDir, Result<Option<String>>, String) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, disk).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, baseline, |_, _| {}).unwrap();
        let outcome = disk_edit_newer_than_registered_authority(&doc, authority);
        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
        (dir, outcome, log)
    }

    /// `#admissionmarkersplit`: the live wedge. The baseline is checkpointed in the
    /// durable (marker-stripped) domain while the editor authority still carries
    /// agent-doc's own boundary marker, so raw bytes make three revisions out of
    /// two. Admission must NOT refuse, and the newer durable save must still be
    /// fast-forwarded exactly as it would be with no marker present.
    #[test]
    fn a_transient_marker_only_authority_does_not_refuse_admission() {
        let (_dir, outcome, log) = admission_gate(
            "# Doc\n\nbody\n",
            "# Doc\n\nbody\ndurable save\n",
            "# Doc\n\n<!-- agent:boundary:fe1c0161:session -->\nbody\n",
        );
        let adopted = outcome.expect("marker-only divergence must not refuse admission");
        assert_eq!(adopted.as_deref(), Some("# Doc\n\nbody\ndurable save\n"));
        assert!(
            log.contains("admission_divergence ")
                && log.contains(&expected_fields(
                    agent_doc_document::admission_divergence::AdmissionDivergence::TransientMarkersOnly,
                    agent_doc_document::admission_divergence::DiskAdoption::FastForwardAuthorityToDisk,
                )),
            "the observation must name its class in the log:\n{log}"
        );
    }

    /// `#admissionancestorsplit`: the blocked-commit shape. The baseline advanced
    /// to the live snapshot while disk kept the older committed revision, then the
    /// operator kept typing. Three durably distinct revisions, but disk is an
    /// ancestor of the live buffer, so there is no writer to drop — proceed on the
    /// authority and adopt nothing.
    #[test]
    fn an_authority_that_already_contains_disk_does_not_refuse_admission() {
        let (_dir, outcome, log) = admission_gate(
            "# Doc\n\nintro\n",
            "# Doc\n\nintro\ncommitted response\n",
            "# Doc\n\nintro\ncommitted response\noperator steering\n",
        );
        let adopted = outcome.expect("an ancestor disk revision must not refuse admission");
        assert_eq!(
            adopted, None,
            "disk is an ancestor: adopting it would drop the operator's steering"
        );
        assert!(
            log.contains(&expected_fields(
                agent_doc_document::admission_divergence::AdmissionDivergence::AuthoritySubsumesDisk,
                agent_doc_document::admission_divergence::DiskAdoption::ProceedOnAuthority,
            )),
            "the observation must name its class in the log:\n{log}"
        );
    }

    /// A session document with the components a real one has. `queue` is the
    /// component both live `#admissionsplitmerge` incidents diverged in.
    fn queue_document(queue_lines: &str) -> String {
        format!(
            "# Doc\n\n<!-- agent:exchange -->\nintro\n<!-- /agent:exchange -->\n\
             <!-- agent:queue go -->\n{queue_lines}<!-- /agent:queue -->\n"
        )
    }

    /// `#admissionsplitmerge`: a durable three-plane split never refuses. This is
    /// the shape of the 2026-09-27 incident on `tasks/agent-doc/agent-doc-bugs.md`:
    /// the operator's queue edit reached disk while the live editor buffer held a
    /// different revision of the same queue.
    ///
    /// `#queuecutexclusive`: both revisions are the operator's, typed in the one
    /// live editor -- two cuts of one history, not two writers. This test used to
    /// require their union; that union is the body every admission duplicate
    /// since adopted. The queue both planes changed keeps the authority's cut and
    /// admission proceeds on it; the editor's own ops carry the rest exactly once.
    #[test]
    fn genuinely_divergent_durable_queue_keeps_the_authority_cut() {
        let (_dir, outcome, log) = admission_gate(
            &queue_document("- do [#one]\n- do [#two]\n"),
            &queue_document("- do [#one]\n- Fix Run Agent Doc\n- do [#two]\n"),
            &queue_document("- do [#one]\n- do [#two]: automate\n"),
        );
        let adopted = outcome.expect("durably divergent planes must never refuse");
        assert_eq!(
            adopted, None,
            "a queue both planes changed proceeds on the authority's cut"
        );
        assert!(
            log.contains(&expected_fields(
                agent_doc_document::admission_divergence::AdmissionDivergence::ThreeWaySplitMerged,
                agent_doc_document::admission_divergence::DiskAdoption::MergeDiskIntoAuthority,
            )),
            "the observation must name its class in the log:\n{log}"
        );
        assert!(
            log.contains("admission_three_way_merge ")
                && log.contains("outcome=proceed_on_authority")
                && log.contains("rungs=[CutExclusive=authority_kept(queue:0) "),
            "the merge must record why it kept the authority:\n{log}"
        );
    }

    /// The load-bearing safety gate: a rung whose merged revision would drop text
    /// the live editor buffer holds must never be adopted. `Semantic` rebuilds prose
    /// from the operator skeleton and empties a component body it cannot reconstruct,
    /// so without this gate the ladder would clobber a live buffer — the exact
    /// dropped-writer outcome the old refusal existed to prevent.
    #[test]
    fn a_merge_that_would_drop_authority_text_is_never_adopted() {
        let baseline = queue_document("- do [#one]\n");
        let disk = queue_document("- do [#one]\n- disk only\n");
        let authority = queue_document("- do [#one]\n- authority only\n");
        let (_dir, outcome, _log) = admission_gate(&baseline, &disk, &authority);
        let adopted = outcome.expect("a three-plane split must never refuse admission");
        if let Some(adopted) = adopted {
            assert!(
                adopted.contains("authority only"),
                "an adopted merge must preserve every durable authority line:\n{adopted}"
            );
            assert!(
                adopted.contains("intro"),
                "an adopted merge must preserve untouched component bodies:\n{adopted}"
            );
        }
    }

    /// A split with nothing for the merge to contribute must proceed on the
    /// authority, never refuse.
    #[test]
    fn an_unadoptable_merge_proceeds_on_the_authority_instead_of_refusing() {
        let (_dir, outcome, log) = admission_gate(
            "# Doc\n\nintro\n",
            "# Doc\n\nintro\nshared line\n",
            "# Doc\n\nintro\nshared line\noperator steering\n",
        );
        let adopted = outcome.expect("an unadoptable merge must not refuse admission");
        assert_eq!(
            adopted, None,
            "disk is an ancestor here: there is nothing to adopt"
        );
        assert!(
            !log.contains("refus"),
            "admission must never refuse on a three-plane observation:\n{log}"
        );
    }

    /// The ordinary observation — nothing diverged — must stay silent. Logging it
    /// would emit a line on every preflight and bury the three rare classes the
    /// diagnosis depends on.
    #[test]
    fn a_converged_observation_logs_no_admission_divergence_line() {
        let (_dir, outcome, log) = admission_gate(
            "# Doc\n\nbody\n",
            "# Doc\n\nbody\n",
            "# Doc\n\nbody\nlive edit\n",
        );
        assert_eq!(outcome.unwrap(), None);
        assert!(
            !log.contains("admission_divergence "),
            "verbatim-resolvable observations must not be logged:\n{log}"
        );
    }

    /// Run one queue-maintenance pass over a `priority` queue whose backlog
    /// mirrors into it, returning `(queue_prompts, rendered queue body)`.
    fn priority_queue_maintenance(snap_q: &str, cur_q: &str) -> (Vec<String>, String) {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let mk = |q: &str| {
            format!(
                concat!(
                    "---\n",
                    "agent_doc_session: test\n",
                    "agent_doc_format: template\n",
                    "agent_doc_write: crdt\n",
                    "agent: claude\n",
                    "prompt_presets:\n",
                    "  '#ship': update spec + tests. commit + push\n",
                    "queue: go\n",
                    "---\n\n",
                    "<!-- agent:exchange patch=append -->\n",
                    "### Re: prior — opus-5.5\n\nDone.\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue preset=\"#ship\" priority go -->\n",
                    "{}",
                    "<!-- /agent:queue -->\n\n",
                    "<!-- agent:backlog -->\n",
                    "- [ ] [#sr] stop replay\n",
                    "- [ ] [#sd] sup drain\n",
                    "- [ ] [#qsr] queue select reversed\n",
                    "- [ ] [#rsc] release skip carried\n",
                    "- [ ] [#g76] gh76 secondary\n",
                    "<!-- /agent:backlog -->\n",
                ),
                q
            )
        };
        std::fs::write(&doc, mk(cur_q)).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &mk(snap_q),
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let state = run_queue_maintenance(&doc, None).unwrap();
        let updated = std::fs::read_to_string(&doc).unwrap();
        (state.queue_prompts, component_body(&updated, "queue"))
    }

    /// `#queuerankless-anchor`: a bare `#release` preset reference has no
    /// backlog rank. It used to sort in the "pre-existing" append-stable group,
    /// ahead of every backlog-sourced head, so each maintenance pass lifted it
    /// from the tail to the first live slot with no operator edit at all
    /// (2026-09-30, `tasks/agent-doc/agent-doc-bugs.md`: "You keep on reordering
    /// the queue").
    #[test]
    fn priority_maintenance_keeps_a_rankless_reference_at_its_authored_slot() {
        let queue = concat!(
            "- ~~do [#sr]~~\n",
            "- do [#qsr]\n",
            "- do [#rsc]\n",
            "- do [#g76]\n",
            "- ~~do [#sd]~~\n",
            "- #release\n",
        );
        let (prompts, body) = priority_queue_maintenance(queue, queue);
        assert_eq!(
            prompts,
            vec!["do [#qsr]", "do [#rsc]", "do [#g76]", "#release"],
            "{body}"
        );
        assert!(body.trim_end().ends_with("- 📥 #release"), "{body}");
    }

    /// `#queuemovepinneighbors`: parking one line at the tail must not stamp an
    /// operator pin on the lines that merely shifted up beneath it.
    #[test]
    fn priority_maintenance_moving_a_line_down_pins_no_neighbor() {
        let snap = "- #release\n- do [#qsr]\n- do [#rsc]\n- do [#g76]\n";
        let cur = "- do [#qsr]\n- do [#rsc]\n- do [#g76]\n- #release\n";
        let (prompts, body) = priority_queue_maintenance(snap, cur);
        assert!(!body.contains('📌'), "no line was raised:\n{body}");
        assert_eq!(
            prompts,
            vec!["do [#qsr]", "do [#rsc]", "do [#g76]", "#release"],
            "{body}"
        );
    }

    /// `#rundocsstop`: the haiven-dev `tasks/docs.md` refusal (2026-10-15
    /// 21:39Z). The operator pressed Run Agent Doc (an explicit route) on a
    /// document holding two open `- [ ] [#id]` rows outside `agent:backlog`;
    /// preflight refused, and the operator's edit went unanswered. With operator
    /// intent at entry the same document is admitted with a repair directive;
    /// without it (no admission, or a bare re-run) the refusal stands.
    #[test]
    fn shadow_open_backlog_admits_operator_intent_with_a_repair_directive() {
        use agent_doc_turn::turn_admission::TurnAdmission;
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("docs.md");
        std::fs::write(
            &doc,
            concat!(
                "<!-- agent:exchange -->\n### Re: prior\n\nDone.\n\nWhy int32?\n<!-- /agent:exchange -->\n\n",
                "<!-- agent:backlog -->\n- [ ] [#live1] Real backlog work\n<!-- /agent:backlog -->\n\n",
                "## Parked\n\n",
                "- [ ] [#todopr49answers] Answer the ADR 0025 questions\n",
                "- [ ] [#todopr49automerge] Disarm squash auto-merge\n",
            ),
        )
        .unwrap();

        for admission in [
            TurnAdmission::ContinueWithExplicitRoute,
            TurnAdmission::ContinueWithSteering,
        ] {
            let warning = shadow_open_backlog_admission(&doc, Some(admission))
                .unwrap_or_else(|err| panic!("{} must admit: {err}", admission.as_str()))
                .expect("admitted shadow rows carry a repair directive");
            assert_eq!(warning.code, SHADOW_OPEN_BACKLOG_REPAIR_WARNING);
            assert!(
                warning.message.contains("#todopr49answers")
                    && warning.message.contains("#todopr49automerge")
                    && warning.message.contains("answer that edit")
                    && warning
                        .message
                        .contains("move each listed item back into agent:backlog"),
                "{}",
                warning.message
            );
        }

        for admission in [None, Some(TurnAdmission::RequireCleanCloseout)] {
            let err = shadow_open_backlog_admission(&doc, admission)
                .expect_err("no operator intent keeps the refusal")
                .to_string();
            assert!(
                err.contains("exist only outside live agent:backlog")
                    && err.contains("#todopr49answers"),
                "{err}"
            );
        }
        assert!(enforce_no_shadow_open_backlog(&doc).is_err());

        std::fs::write(
            &doc,
            "<!-- agent:backlog -->\n- [ ] [#live1] Real backlog work\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        assert_eq!(
            shadow_open_backlog_admission(&doc, Some(TurnAdmission::ContinueWithExplicitRoute))
                .unwrap(),
            None,
            "a clean document adds no directive"
        );
        enforce_no_shadow_open_backlog(&doc).unwrap();
    }
}

/// One requested prompt preset paired with its frontmatter body
/// (`#orchestratepresetexpand`). See [`PreflightOutput::prompt_preset_expansions`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PromptPresetExpansion {
    /// The canonical preset key, as it appears in `prompt_presets` frontmatter.
    pub name: String,
    /// The preset body, verbatim.
    pub body: String,
    /// Validated project-relative runbook path associated with the preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
    /// Imperative harness instruction emitted only when the preset is invoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook_load_instruction: Option<String>,
}
