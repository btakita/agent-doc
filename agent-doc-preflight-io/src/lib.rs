//! Preflight maintenance I/O adapters.

pub mod debounce;
pub mod gc;
pub mod layout;
pub mod sweep;
pub mod warnings;

use agent_doc_document::queue_projection::{
    set_in_progress_work_item_markers, strip_in_progress_marker, strip_priority_markers,
    sync_in_progress_marker_regions,
};
use agent_doc_element::element::{
    is_backlog_component, is_review_component, is_tracked_work_component,
};
use agent_doc_element_backlog::backlog::{
    component_matches_tracked_surface, ensure_no_completed_tracked_items, format_dropped_refs,
    format_shadow_refs, maintenance_surface_label, review_counts, should_reap_already_done_mirrors,
    should_reap_ops_proof_completions, tracked_body_for_reorder,
};
use agent_doc_frontmatter::frontmatter;
use agent_doc_queue::{
    backlog_sync::AutoBacklogQueueSyncPolicy,
    control_binding::{
        converge_queue_control_binding_content, explicit_queue_go_mode, explicit_queue_start_mode,
        explicit_queue_stop_mode, strip_queue_activation_tokens_in_content,
    },
    free_text_admission::{
        FreeTextAdmissionExecution, FreeTextAdmissionScope, append_empty_agent_component,
        collect_actionable_free_text_prompts, prepare_free_text_admission,
        queue_currently_active_for_free_text_admission, queue_free_text_admission_scope,
    },
    queue_convergence::{
        inactive_queue_changed_vs_snapshot, queue_entries_are_drained_residue,
        queue_region_differs_from_snapshot, selected_queue_head_unchanged_in_snapshot,
    },
    queue_response::{free_text_head_answered_by_response, queue_prompt_text_is_free_text},
};
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
/// A `Current`/`Detached`/`None` observation returns immediately — only the
/// transient editor-authority statuses are retried.
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
                agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
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
    /// Lines from `.agent-doc/claims.log` (truncated after read).
    /// #per-cycle-protocol-output-overhead: omit when empty (the common case).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<String>,
    /// Unified diff text, or `null` if there are no changes.
    pub diff: Option<String>,
    /// True when the snapshot matches the document (no new user input).
    pub no_changes: bool,
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
    /// Canonical serialized list of user-authored changes that should preempt
    /// or guide the current response cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_intent_prompt_changes: Vec<agent_doc_diff::PromptBearingChange>,
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
    /// Bounded session-growth / churn advisory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_accretion: Option<agent_doc_session_accretion::SessionAccretionReport>,
    /// Live finalize-pipeline state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<agent_doc_frontmatter::frontmatter::AgentDocPipeline>,
    /// Semantic merge acknowledgements to surface once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_cell_merge_acks: Vec<agent_doc_cycle_state_io::PendingSemanticMergeAck>,
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

    fn retained_document_write(&self, file: &Path) -> bool;

    fn session_interruption(&self, file: &Path) -> Result<Option<String>>;

    fn detect_bypassed_response_write(&self, file: &Path) -> Result<Option<String>>;
}

pub fn enforce_cycle_completion(
    file: &Path,
    effects: &impl PreflightCycleCompletionEffects,
) -> Result<(bool, bool)> {
    // A retained document-write effect is an unfinished durable sink, even
    // when an older repair accidentally made its closeout cycle look terminal.
    // Give session-check one chance to settle the exact capture, then fail
    // closed before a new preflight can replace its live projection.
    if effects.retained_document_write(file) {
        if let Some(reason) = effects.session_interruption(file)? {
            anyhow::bail!("{}", reason.replace('\n', " "));
        }
        if effects.retained_document_write(file) {
            anyhow::bail!(
                "retained document-write effect remains unsettled for {}; retry `agent-doc session-check {}` before starting another cycle",
                file.display(),
                file.display(),
            );
        }
    }

    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
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
        return Ok((recovered || ipc_dogfood_note_appended, committed));
    }

    let Some(state) = state else {
        return Ok((false, false));
    };
    if !state.is_open() {
        return Ok((false, false));
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
            "interrupted_cycle_detected file={} cycle_id={} phase={:?} event={}",
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
        recovered || ipc_dogfood_note_appended || self_healed_abandoned,
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
    // #pending-gate-snapshot-desync: the snapshot may need re-syncing to the
    // file's tracked surfaces even when maintenance itself makes no change —
    // the write phase can apply --pending-gate / --pending-edit / --review-add
    // to the file without those reaching the content_ours snapshot. Tracked
    // separately from `mutated` so the snapshot is re-saved without an
    // unnecessary working-tree rewrite.
    let mut snapshot_mutated = false;
    let mut saw_completed_before = false;
    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_project_root_io::project_root_containing(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let already_done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

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
            let (after_mirror_reap, mirror_items) =
                agent_doc_element_backlog::backlog::op_take_active_items_by_ids(
                    &current_body,
                    &already_done_ids,
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

        // Priority sort (#backlog-priority-attribute): when the component marker
        // carries `priority`, stable-sort items by their per-item `priority=<1..9>`
        // token (1 = highest; absent = lowest) so a downstream `agent:queue` sync
        // inherits the prioritized order.
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
            } else if deferable_realtime_drift {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_maintenance_deferred_realtime_buffer_busy file={} source=pending_maintenance error={}",
                        file.display(),
                        err_message.replace('\n', " ")
                    ),
                );
                eprintln!(
                    "[preflight] pending: deferred maintenance write for {} (realtime buffer busy; not aborting preflight): {}",
                    file.display(),
                    err
                );
                // Revert to the pre-maintenance baseline so nothing is
                // half-persisted: the visible write failed before touching the
                // file, and skipping the snapshot save keeps the two in sync.
                current_content = content.clone();
                snapshot_content = snapshot_at_start.clone();
                mutated = false;
                snapshot_mutated = false;
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

    if saw_completed_before {
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

    Ok(PendingMaintenanceReport {
        reordered,
        backlog_gated_count,
        review_count,
        review_gated_count,
        legacy_gated_in_backlog_count: backlog_gated_count,
    })
}

fn collect_agent_done_ids_with_root(
    content: &str,
    project_root: Option<&Path>,
) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return ids;
    };
    for comp in &components {
        if !agent_doc_element::element::is_backlog_done_component(&comp.name) {
            continue;
        }
        for id in agent_doc_element_done::collect_done_component_own_ids(content, comp) {
            ids.insert(id);
        }
        if let Some(archive) = comp.attrs.get("archive")
            && let Some(root) = project_root
        {
            let archive_path = root.join(archive);
            if let Ok(archive_content) = std::fs::read_to_string(&archive_path) {
                for id in agent_doc_element_done::collect_done_item_own_ids(&archive_content) {
                    ids.insert(id);
                }
            }
        }
    }
    ids
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
        .and_then(|root| std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).ok())
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

pub fn enforce_no_shadow_open_backlog(file: &Path) -> Result<()> {
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
    if !report.shadow_only.is_empty() {
        anyhow::bail!(
            "open backlog item(s) exist only outside live agent:backlog: {}. Move them back into the live backlog or mark them complete before continuing",
            format_shadow_refs(&report.shadow_only)
        );
    }
    Ok(())
}

pub fn enforce_no_dropped_backlog(file: &Path, head_content: Option<&str>) -> Result<()> {
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

    let external_done_ids = agent_doc_element_backlog_io::done_archive::external_done_archive_ids(
        file,
        &current_content,
    )?;
    let report =
        agent_doc_element_backlog::backlog::detect_dropped_from_history_with_extra_current_ids(
            &current_content,
            head_content,
            &resolved_ids,
            &external_done_ids,
        )?;
    if !report.dropped.is_empty() {
        anyhow::bail!(
            "open backlog item(s) from recent committed history are completely absent from the document: {}. Restore them to the live backlog, move them to icebox, or mark them done before continuing",
            format_dropped_refs(&report.dropped)
        );
    }
    Ok(())
}

/// Queue component state extracted during maintenance.
///
/// Returned by `run_queue_maintenance` for later composition into `PreflightOutput`.
/// The `queue_prompts` are only populated when the queue is active.
#[derive(Debug, Default)]
pub struct QueueState {
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
    pub synced_queue_ids: Vec<String>,
    pub warnings: Vec<PreflightWarning>,
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
    let event = agent_doc_state_backbone::StateEvent::new(
        format!("queue-head-selected:{document_hash}:{node_key}:0:{content_hash}"),
        agent_doc_state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable,
            hosting_epoch: None,
        },
    );
    let inserted =
        agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)?;
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
    let selected_event = agent_doc_state_backbone::StateEvent::new(
        format!("queue-head-deferred-selected:{document_hash}:{node_key}:0:{content_hash}"),
        agent_doc_state_backbone::StateFact::QueueHeadSelected {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            backlog_id: agent_doc_queue::queue_response::queue_prompt_done_id(head_text),
            prompt_text: Some(head_text.to_string()),
            drainable: false,
            hosting_epoch: None,
        },
    );
    let selected_inserted = agent_doc_controller_io::project_controller::append_state_event(
        &project_root,
        &selected_event,
    )?;
    let reason_hash = agent_doc_hash::content_hash(reason);
    let deferred_event = agent_doc_state_backbone::StateEvent::new(
        format!("queue-head-deferred:{document_hash}:{node_key}:0:{reason_hash}:{content_hash}"),
        agent_doc_state_backbone::StateFact::QueueHeadDeferred {
            document_hash: document_hash.clone(),
            node_key: node_key.clone(),
            reason: reason.to_string(),
            hosting_epoch: None,
        },
    );
    let deferred_inserted = agent_doc_controller_io::project_controller::append_state_event(
        &project_root,
        &deferred_event,
    )?;
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
    let inserted =
        agent_doc_controller_io::project_controller::append_state_event(&project_root, &event)?;
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
    let components = match agent_doc_element::element::parse(&content) {
        Ok(components) => components,
        Err(_) => return Ok(QueueState::default()),
    };
    let comp = match components
        .iter()
        .find(|component| component.name == "queue")
    {
        Some(component) => component,
        None => return Ok(QueueState::default()),
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
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop)
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
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, fm.queue.as_deref());
    let persisted_activation = queue_control_activation(&comp.attrs, fm.queue.as_deref());

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
            synced_queue_ids: vec![],
            warnings: vec![],
        });
    }

    if activation.active
        && let Some(start_at) =
            agent_doc_queue::document_queue::time_gate_at_head(&activation.entries_after)
    {
        return Ok(QueueState {
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
            synced_queue_ids: vec![],
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
    let queue_drainable_head_count = if activation.active {
        agent_doc_queue::queue_continuation::drainable_head_count(&drainability_content)
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    let queue_supervisor_drainable = activation.active
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &drainability_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
    let skipped_queue_head_ids: std::collections::HashSet<String> =
        agent_doc_cycle_state_io::load(file)
            .ok()
            .flatten()
            .map(|state| state.skipped_queue_head_ids.into_iter().collect())
            .unwrap_or_default();
    let selected_queue_prompts = if activation.active {
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &drainability_content,
            &activation.entries_after,
            &agent_doc_queue::backlog_sync::collect_after_deps(&components, &content),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &drainability_content,
                &activation.entries_after,
            ),
            &skipped_queue_head_ids,
        )
        .prompts
    } else {
        Vec::new()
    };

    Ok(QueueState {
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
        synced_queue_ids: vec![],
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
    !explicit_queue_stop_mode(attrs, frontmatter_queue)
        && (explicit_queue_go_mode(attrs, frontmatter_queue)
            || explicit_queue_start_mode(attrs, frontmatter_queue))
}

pub fn run_queue_maintenance(file: &Path, diff: Option<&str>) -> Result<QueueState> {
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
    let mut content = match current_text_via_preflight_authority_retrying(
        file,
        "preflight_queue_maintenance",
    ) {
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. })) => text,
        Ok(Some(agent_doc_crdt_relay_io::CurrentText::Detached)) | Ok(None) => {
            match std::fs::read_to_string(file) {
                Ok(c) => c,
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
            return Ok(QueueState::default());
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
            return Ok(QueueState::default());
        }
    };
    // `content` already came from the CP/editor authority above. Do not reread
    // disk or run a second queue-only merge here: that used to compare the live
    // frontier with itself, making the supposed deletion-adoption branch dead
    // while obscuring which authority maintenance actually mutated.
    let authority_baseline = content.clone();
    let mut current_content = content.clone();
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
    let mut comp = match components.iter().find(|c| c.name == "queue").cloned() {
        Some(c) => c,
        None => return Ok(QueueState::default()),
    };

    let body = &current_content[comp.open_end..comp.close_start];
    let entries = match agent_doc_queue::document_queue::parse(body) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[preflight] queue parse warning: {}", e);
            return Ok(QueueState::default());
        }
    };

    let mut entries = entries;
    let mut queue_warnings = Vec::new();
    let mut synced_queue_ids = Vec::new();
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
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after tag normalization")?
            .clone();
        mutated = true;
        queue_tag_attrs_normalized = true;
        eprintln!("[preflight] queue: normalized malformed queue marker attributes");
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
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after control binding sync")?
            .clone();
        mutated = true;
        eprintln!("[preflight] queue: synchronized queue marker/frontmatter control binding");
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_project_root_io::project_root_containing(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let queue_active_for_free_text =
        queue_currently_active_for_free_text_admission(&current_content, &comp.attrs);
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten();
    let queue_free_text_scope = queue_free_text_admission_scope(
        &current_content,
        &comp.attrs,
        &entries,
        snapshot_content.as_deref(),
    );
    let exchange_prompt =
        agent_doc_turn::exchange_tail::unresolved_exchange_prompt_in_content(&current_content);
    let document_id = agent_doc_hash::document_id_for_path(file);
    if let Some(prepared_admission) = prepare_free_text_admission(
        &current_content,
        &entries,
        exchange_prompt.as_deref(),
        &queue_free_text_scope,
        !queue_active_for_free_text,
        &document_id,
    )? {
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
        let admission = prepared_admission.finish(execution)?;
        current_content = admission.content;
        content = current_content.clone();
        components = agent_doc_element::element::parse(&current_content)?;
        comp = components
            .iter()
            .find(|c| c.name == "queue")
            .context("queue maintenance: queue component vanished after free-text admission")?
            .clone();
        let body = &current_content[comp.open_end..comp.close_start];
        entries = agent_doc_queue::document_queue::parse(body)
            .context("queue maintenance: failed to parse queue after free-text admission")?;
        synced_queue_ids.extend(admission.queued_ids);
        queue_warnings.extend(warnings);
        mutated = true;
        eprintln!(
            "[preflight] queue: admitted {} free-text prompt(s) into backlog ({})",
            admission.admitted_count, admission.execution_label
        );
    }

    // `#ynra`: collect `agent:done` ids ONCE up front. The backlog→queue sync
    // below must never re-mint a `do [#id]` whose id is already completed
    // (archived in `agent:done`) — otherwise the strike pass removes it every
    // cycle, the sync re-injects it the next cycle, and the queue churns forever
    // on a completed ref. `agent:done` is not mutated by any queue maintenance
    // step, so this set is valid for both the sync filter and the later strike.
    let done_ids = collect_agent_done_ids_with_root(&content, project_root.as_deref());

    // Backlog→queue sync (#backlog-queue-sync-attr): when an `agent:backlog`
    // component carries a `queue` attribute, regenerate the queue `do [#id]`
    // prompts from its active items BEFORE activation so a freshly synced queue
    // can auto-activate on the same cycle. `agent:icebox` is intentionally not a
    // component-level sync source; parked work must be moved to backlog or
    // explicitly marked for enqueue. Per-item enqueue markers
    // (#queue-enqueue-action) append marked ids without requiring the component
    // attribute.
    if let Some(sync_request) =
        agent_doc_queue::backlog_sync::collect_backlog_queue_sync(&components, &content)
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
                        let q = comps.iter().find(|c| c.name == "queue")?;
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
                .and_then(|fm| fm.queue.as_deref()),
        );
        let queue_explicitly_stopped = explicit_queue_stop_mode(
            &comp.attrs,
            incoming_frontmatter
                .as_ref()
                .and_then(|fm| fm.queue.as_deref()),
        );
        let sync_plan = agent_doc_queue::backlog_sync::plan_auto_backlog_queue_sync_ids(
            agent_doc_queue::backlog_sync::AutoBacklogQueueSyncInput {
                requested_ids: &sync_request.ids,
                enqueue_ids: &sync_request.enqueue_ids,
                done_ids: &done_ids,
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
        if let Some(synced) =
            agent_doc_queue::document_queue::sync_backlog_into_queue(&entries, &backlog_ids, mode)
        {
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
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
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
            && let Some(snap_queue) = snap_components.iter().find(|c| c.name == "queue")
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
                        let q = comps.iter().find(|c| c.name == "queue").unwrap();
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
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
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
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop)
    );
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&comp.attrs)
        || matches!(
            marker_control,
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    let exchange_triggered = diff
        .map(agent_doc_diff::detect_queue_trigger)
        .unwrap_or(false);
    let (fm, _) = frontmatter::parse(&current_content).unwrap_or_default();
    let persisted_active = fm.queue_active.unwrap_or(false);
    let explicit_stop = explicit_queue_stop_mode(&comp.attrs, fm.queue.as_deref());
    let persisted_activation = queue_control_activation(&comp.attrs, fm.queue.as_deref());

    let mut activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        has_auto,
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
        if let Some(q) = comps.iter().find(|c| c.name == "queue") {
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
    // (#queue-operator-pin-position-lock) is preserved: convergence is purely
    // subtractive at each identity's earliest slot.
    let snapshot_queue_entries: Vec<agent_doc_queue::document_queue::QueueEntry> =
        match agent_doc_snapshot_io::load_document_baseline(file) {
            Ok(Some(snap)) => agent_doc_element::element::parse(&snap)
                .ok()
                .and_then(|comps| {
                    comps
                        .iter()
                        .find(|c| c.name == "queue")
                        .map(|q| snap[q.open_end..q.close_start].to_string())
                })
                .and_then(|body| agent_doc_queue::document_queue::parse(&body).ok())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
    if let Some(converged_entries) = agent_doc_queue::document_queue::converge_queue_via_lifecycle(
        &activation.entries_after,
        &snapshot_queue_entries,
    ) {
        let dropped = activation
            .entries_after
            .len()
            .saturating_sub(converged_entries.len());
        let new_body = agent_doc_queue::document_queue::render(&converged_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps
                .iter()
                .find(|c| c.name == "queue")
                .context("queue maintenance: queue component vanished before convergence")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = converged_entries;
        mutated = true;
        eprintln!(
            "[preflight] queue: converged {dropped} duplicate queue head(s) via per-item lifecycle SM (#cgfx)"
        );
    }

    // Consume start fence if needed
    if activation.consumed_start_fence {
        let new_body = agent_doc_queue::document_queue::render(&activation.entries_after);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps.iter().find(|c| c.name == "queue").unwrap();
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
    let mut eligible_ids: std::collections::HashSet<String> = done_ids.clone();
    for id in &gated_ids {
        eligible_ids.insert(id.clone());
    }
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
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            mutated = true;
            for prompt_text in &struck {
                let source =
                    match agent_doc_queue::queue_response::queue_prompt_done_id(prompt_text) {
                        Some(id) if done_ids.contains(&id) => "done",
                        Some(id) if gated_ids.contains(&id) => "review_gated",
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
    // `Prompt`). The match reuses the SAME `free_text_head_answered_by_response`
    // predicate the session-check residue guard uses, so the preflight strike set
    // and the session-check INTERRUPT set agree — anything struck here is exactly
    // what would otherwise have INTERRUPTed closeout. A snapshot gate restricts
    // the strike to heads already present in the committed queue (mirroring
    // session-check's `committed_queue_contains_active_free_text_head`), so an
    // in-flight operator edit the convergence just added is never struck.
    let exchange_text = agent_doc_element::element::parse(&current_content)
        .ok()
        .and_then(|comps| {
            comps
                .iter()
                .find(|c| c.name == "exchange")
                .map(|c| c.content(&current_content).to_string())
        })
        .unwrap_or_default();
    if !exchange_text.trim().is_empty() {
        // Normalized prose of every free-text head committed in the snapshot — the
        // in-flight-edit gate. `snapshot_queue_entries` was parsed above.
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
        let mut struck_count = 0usize;
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
                        && queue_prompt_text_is_free_text(&current_content, &p.text)
                        && free_text_head_answered_by_response(&exchange_text, &p.text)
                        && committed_free_text.contains(&gate_norm(&p.text)) =>
                {
                    struck_count += 1;
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
                                indent: 0,
                            },
                        )
                    }
                }
                other => other.clone(),
            })
            .collect();
        if struck_count > 0 {
            let new_body = agent_doc_queue::document_queue::render(&new_entries);
            current_content = {
                let comps = agent_doc_element::element::parse(&current_content)?;
                let q = comps.iter().find(|c| c.name == "queue").context(
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
    // conservative `QUEUE_STRIKE_THRESHOLD` (set above the `+1.0`
    // substring-contains bonus so an unrelated operator prompt can never reach
    // it), and a committed-snapshot gate (mirroring the `#qheadresidue` gate)
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
                                        indent: 0,
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
                        let q = comps.iter().find(|c| c.name == "queue").context(
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
                let q = comps.iter().find(|c| c.name == "queue").unwrap();
                q.replace_content(&current_content, &new_body)
            };
            // Strip ephemeral activation controls and clear queue state.
            current_content = strip_queue_activation_tokens_in_content(&current_content)?;
            if persisted_active {
                current_content = frontmatter::merge_queue_state(&current_content, false)?;
            }
            // Persist to file + snapshot (skip the raw disk write behind a live
            // editor; #fccqueue routes the queue shape through IPC convergence).
            persist_queue_maintenance_doc(
                file,
                &current_content,
                &authority_baseline,
                project_root.as_deref(),
                "queue_halt",
            )?;
            if let Ok(Some(snap)) = agent_doc_snapshot_io::load_document_baseline(file) {
                let mut new_snap = snap.clone();
                if let Ok(sc) = agent_doc_element::element::parse(&new_snap)
                    && let Some(sq) = sc.iter().find(|c| c.name == "queue")
                {
                    new_snap = sq.replace_content(&new_snap, &new_body);
                    new_snap = strip_queue_activation_tokens_in_content(&new_snap)?;
                    if persisted_active
                        && let Ok(m) = frontmatter::merge_queue_state(&new_snap, false)
                    {
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
                synced_queue_ids,
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
                synced_queue_ids,
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
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
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
    let need_strip_auto = has_auto && !queue_has_prompts;
    let need_clear_non_auto_residue =
        !has_auto && !activation.active && !activation.deferred && drained_residue;
    let need_clear_drained_body =
        (need_strip_auto || need_clear_non_auto_residue) && !activation.deferred;

    if need_clear_drained_body {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        if !q.content(&current_content).trim().is_empty() {
            current_content = q.replace_content(&current_content, "");
            mutated = true;
            eprintln!("[preflight] queue: cleared drained queue body");
        }
    }

    if !activation.active
        && !activation.deferred
        && !activation.entries_after.is_empty()
        && !need_clear_drained_body
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
    if need_strip_auto || marker_stop {
        let comps = agent_doc_element::element::parse(&current_content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
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
    } else if need_clear_active {
        current_content = frontmatter::merge_queue_state(&current_content, false)?;
        mutated = true;
        eprintln!("[preflight] queue: set queue: stop");
    }

    // `#queueskip`: recompute the skipped-head set for this cycle. A head that was
    // dispatched last cycle and came back unconsumed (its `#id` never entered the
    // prior cycle's resolved/gated set) yet is still a live head is confirmed
    // stalled — skip it so the queue advances to a non-dependent drainable head
    // instead of re-dispatching a dead ref. Carried skips persist until their id
    // is consumed or no longer a live head. Computed from the prior cycle's
    // state-ledger projection before `start_preflight` advances this run.
    let skipped_queue_head_ids: std::collections::HashSet<String> = if activation.active {
        let current_live_ids: std::collections::HashSet<String> = activation
            .entries_after
            .iter()
            .filter(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Prompt(_)))
            .filter_map(agent_doc_queue::queue_projection::queue_entry_do_id)
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
        // The head the prior cycle actually dispatched: its first live id-backed
        // head that was not already skipped.
        let prior_dispatched = prior.as_ref().and_then(|s| {
            s.active_queue_heads
                .iter()
                .filter_map(|h| agent_doc_queue::queue_response::queue_prompt_done_id(h))
                .find(|id| !carried.contains(id))
        });
        let mut fresh = carried;
        if let Some(id) = prior_dispatched
            && !prior_resolved.contains(&id)
            && current_live_ids.contains(&id)
        {
            fresh.insert(id);
        }
        // Clear ids that are no longer live heads or were just consumed.
        fresh.retain(|id| current_live_ids.contains(id) && !prior_resolved.contains(id));
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
    let active_queue_projection = if activation.active {
        let current_components = agent_doc_element::element::parse(&current_content)?;
        agent_doc_queue::queue_projection::active_queue_prompt_projection(
            &current_content,
            &activation.entries_after,
            &agent_doc_queue::backlog_sync::collect_after_deps(
                &current_components,
                &current_content,
            ),
            agent_doc_queue::queue_projection::in_progress_marker_retarget_requested(
                diff,
                &current_content,
                &activation.entries_after,
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
    let active_queue_prompt_texts = active_queue_projection.prompts;
    let current_head_ids = active_queue_prompt_texts
        .iter()
        .filter_map(|text| agent_doc_queue::queue_response::queue_prompt_done_id(text))
        .collect::<std::collections::HashSet<_>>();
    if let Some(marked_entries) = agent_doc_queue::document_queue::set_prompts_in_progress(
        &activation.entries_after,
        &active_queue_prompt_texts,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&marked_entries);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps
                .iter()
                .find(|c| c.name == "queue")
                .context("queue maintenance: queue component vanished before in-progress marker")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = marked_entries;
        mutated = true;
        in_progress_markers_changed = true;
    }
    // `#queueskip`: stamp `⏭️` on skipped heads (and clear it from heads no longer
    // skipped), AFTER the `🚧` pass so the selected head keeps `🚧` and skipped
    // heads carry the visible skip marker.
    if let Some(skip_marked) = agent_doc_queue::document_queue::set_prompts_skipped(
        &activation.entries_after,
        &skipped_queue_head_ids,
    ) {
        let new_body = agent_doc_queue::document_queue::render(&skip_marked);
        current_content = {
            let comps = agent_doc_element::element::parse(&current_content)?;
            let q = comps
                .iter()
                .find(|c| c.name == "queue")
                .context("queue maintenance: queue component vanished before skip marker")?;
            q.replace_content(&current_content, &new_body)
        };
        activation.entries_after = skip_marked;
        mutated = true;
        in_progress_markers_changed = true;
    }
    let (marked_content, pending_markers_changed) =
        set_in_progress_work_item_markers(&current_content, &current_head_ids)?;
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
        persist_queue_maintenance_doc(
            file,
            &current_content,
            &authority_baseline,
            project_root.as_deref(),
            "queue_maintenance",
        )?;
    }

    // Persist snapshot mutations. For newly activated queues, sync the queue
    // component from the visible document into the snapshot so later closeout
    // consumption can prove the same head prompt in both places.
    if (mutated
        || need_sync_newly_activated_queue_snapshot
        || need_sync_active_queue_future_state_snapshot)
        && let Ok(Some(snap_content)) = agent_doc_snapshot_io::load_document_baseline(file)
    {
        let mut new_snap = snap_content.clone();

        if in_progress_markers_changed {
            new_snap = sync_in_progress_marker_regions(&new_snap, &current_content);
        }

        if queue_tag_attrs_normalized
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
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
            && let Some(current_q) = current_comps
                .iter()
                .find(|component| component.name == "queue")
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps
                .iter()
                .find(|component| component.name == "queue")
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
            && (activation.consumed_start_fence || need_strip_auto || need_clear_drained_body)
            && let Ok(snap_comps) = agent_doc_element::element::parse(&new_snap)
            && let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue")
        {
            let new_body = if need_clear_drained_body {
                String::new()
            } else {
                agent_doc_queue::document_queue::render(&activation.entries_after)
            };
            new_snap = snap_q.replace_content(&new_snap, &new_body);

            if (need_strip_auto || marker_stop)
                && let Ok(snap_comps2) = agent_doc_element::element::parse(&new_snap)
                && let Some(snap_q2) = snap_comps2.iter().find(|c| c.name == "queue")
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
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
        {
            new_snap = merged;
        }
        if need_clear_drained_body
            && let Ok(merged) = frontmatter::merge_queue_state(&new_snap, false)
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
    let queue_drainable_head_count = if activation.active {
        agent_doc_queue::queue_continuation::drainable_head_count(&current_content)
    } else {
        0
    };
    let queue_continuation_required = activation.active && queue_drainable_head_count > 0;
    // `#rt83`: supervisor-scope drainability (defers `[operator-verify]`/noise only).
    // Used to gate the preflight synthetic queue-head diff so an operator-verify-only
    // (or otherwise non-actionable) head stops perpetually reporting `no_changes:false`.
    let queue_supervisor_drainable = activation.active
        && agent_doc_queue::queue_continuation::live_drainable_continuation_head(
            &current_content,
            agent_doc_queue::queue_continuation::DrainScope::Supervisor,
        )
        .is_some();
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
        synced_queue_ids,
        warnings: queue_warnings,
    })
}

/// Closeout-side repair for same-cycle backlog capture.
///
/// Preflight's normal backlog→queue sync runs before `finalize` / `write`
/// applies `--pending-add*` mutations, so a go-mode document can commit a fresh
/// backlog item without a matching queue head. This helper runs after closeout
/// queue consumption, enqueuing only ids that were explicitly recorded as
/// same-cycle pending additions. It never applies a full priority/sync recompute,
/// so it cannot move the head that the current response just consumed.
///
/// `#queueatcreate`: placement is caller-chosen and defaults to the queue HEAD.
/// This used to hardcode `Append`, which buried every follow-up behind the whole
/// existing queue — observed live, where four follow-ups filed in one turn sat
/// invisible behind a 112-line queue and the operator re-added them by hand. A
/// follow-up filed by the turn that just ran is, by construction, the most
/// task-relevant work in the document, so the head is the right default; agents
/// that deliberately do not want to preempt the current drain pass `Append`.
pub fn sync_same_cycle_pending_adds_into_go_queue(
    file: &Path,
    placement: agent_doc_queue::backlog_sync::FollowUpQueuePlacement,
) -> Result<Vec<String>> {
    let added_this_cycle = agent_doc_cycle_state_io::pending_added_ids(file);
    if added_this_cycle.is_empty() {
        return Ok(Vec::new());
    }

    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let components = match agent_doc_element::element::parse(&content) {
        Ok(cs) => cs,
        Err(_) => return Ok(Vec::new()),
    };
    let Some(queue_component) = components.iter().find(|c| c.name == "queue") else {
        return Ok(Vec::new());
    };
    let queue_body = &content[queue_component.open_end..queue_component.close_start];
    let entries = match agent_doc_queue::document_queue::parse(queue_body) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("[write] queue: same-cycle pending-add sync skipped — parse warning: {e}");
            return Ok(Vec::new());
        }
    };

    let (fm, _) = frontmatter::parse(&content).unwrap_or_default();
    let queue_go_mode = explicit_queue_go_mode(&queue_component.attrs, fm.queue.as_deref());
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
    let pending_norm: std::collections::HashSet<String> = added_this_cycle
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| !id.is_empty())
        .collect();
    let mut backlog_ids: Vec<String> = sync_request
        .ids
        .into_iter()
        .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
        .filter(|id| pending_norm.contains(id))
        .collect();
    if backlog_ids.is_empty() {
        return Ok(Vec::new());
    }

    let project_root = file.canonicalize().ok().and_then(|canonical| {
        agent_doc_project_root_io::project_root_containing(&canonical)
            .or_else(|| canonical.parent().map(std::path::Path::to_path_buf))
    });
    let done_ids: std::collections::HashSet<String> =
        collect_agent_done_ids_with_root(&content, project_root.as_deref())
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
    let Some(synced) = agent_doc_queue::document_queue::sync_backlog_into_queue(
        &entries,
        &backlog_ids,
        placement.sync_mode(),
    ) else {
        return Ok(Vec::new());
    };
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

    let new_body = agent_doc_queue::document_queue::render(&synced);
    let current_content = {
        let comps = agent_doc_element::element::parse(&content)?;
        let q = comps.iter().find(|c| c.name == "queue").unwrap();
        q.replace_content(&content, &new_body)
    };
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
    if let Err(primary_err) = persist_queue_maintenance_doc(
        file,
        &current_content,
        &content,
        project_root.as_deref(),
        "pending_add_sync",
    ) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_pending_add_sync_persist_fallback file={} reason={} (#queueatcreate)",
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
            "pending_add_sync_fallback",
            |live| {
                // Recompute against the live document: the primary attempt may
                // have observed a different image, and the backlog write that
                // created these ids already landed there.
                agent_doc_queue::backlog_sync::enqueue_created_ids_in_content(
                    live,
                    &backlog_ids,
                    placement,
                )
            },
        )
        .with_context(|| {
            format!(
                "same-cycle queue enqueue failed on both the queue-maintenance path ({primary_err}) \
                 and the tracked-work fallback"
            )
        })?;
    }
    adopt_edited_queue_head_into_snapshot(file, &current_content);
    eprintln!(
        "[write] queue: {} {} same-cycle pending-add id(s) into active go queue",
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
    let global_config = agent_doc_config::load().unwrap_or_default();
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

fn guard_queue_maintenance_expected_current(
    file: &Path,
    source: &str,
    expected_current: &str,
    current: &str,
) -> Result<()> {
    if current == expected_current {
        return Ok(());
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_compare_and_swap_blocked source={source} outcome=head_advanced \
             expected_hash={} current_hash={} expected_len={} current_len={} \
             recovery=retry_from_live_head",
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(current),
            expected_current.len(),
            current.len(),
        ),
    );
    anyhow::bail!(
        "{source}: Lazily head advanced during queue maintenance; retry from the live head"
    )
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
            agent_doc_crdt_relay_io::CrdtReplicaEventReason::AckRecoveryForceRefresh,
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

pub(crate) fn persist_queue_maintenance_doc(
    file: &Path,
    content: &str,
    expected_current: &str,
    project_root: Option<&Path>,
    source: &str,
) -> Result<()> {
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
            guard_queue_maintenance_expected_current(file, source, expected_current, &text)?;
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
            let write = if agent_doc_crdt_relay_io::embedded_relay_is_available_for_file(file) {
                agent_doc_crdt_relay_io::apply_cp_write_for_file(
                    file,
                    expected_current,
                    content,
                    source,
                )?
            } else {
                agent_doc_controller_io::project_controller::apply_cp_write_via_controller_model_for_doc(
                    file,
                    expected_current,
                    content,
                    source,
                )?
            };
            anyhow::ensure!(
                write.is_some(),
                "{source}: attached Lazily write was not applied"
            );
        }
        agent_doc_crdt_relay_io::CurrentText::Detached => {
            let disk = std::fs::read_to_string(file)?;
            anyhow::ensure!(
                disk == expected_current || disk == content,
                "{source}: disk head advanced during detached queue maintenance"
            );
            if disk != content {
                std::fs::write(file, content)
                    .with_context(|| format!("{source}: failed to write {}", file.display()))?;
            }
        }
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => {
            anyhow::bail!("{source}: Lazily editor head is not ready for queue maintenance")
        }
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "write_authority action=compare_and_swap authority=lazily surface=queue_maintenance source={source} len={}",
            content.len()
        ),
    );
    Ok(())
}

/// Update the recovery baseline after the Lazily compare-and-swap succeeds.
pub(crate) fn adopt_edited_queue_head_into_snapshot(file: &Path, current_content: &str) {
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
    let Some(cur_q) = cur_comps.iter().find(|c| c.name == "queue") else {
        return;
    };
    let Ok(snap_comps) = agent_doc_element::element::parse(&snap_now) else {
        return;
    };
    let Some(snap_q) = snap_comps.iter().find(|c| c.name == "queue") else {
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
    }

    impl Default for GuardFailingPreflightMaintenanceWriteEffects {
        fn default() -> Self {
            Self {
                authority_checks: std::cell::Cell::new(0),
                converge_calls: std::cell::Cell::new(0),
                visible_write_error: "failed to resolve editor authority for test document"
                    .to_string(),
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
        commit_calls: std::cell::Cell<usize>,
        retained_document_write: bool,
        session_interruption: Option<String>,
    }

    impl PreflightCycleCompletionEffects for TestPreflightCycleCompletionEffects {
        fn repair(&self, _file: &Path) -> Result<agent_doc_turn::repair::RepairOutcome> {
            self.repair_calls.set(self.repair_calls.get() + 1);
            Ok(agent_doc_turn::repair::RepairOutcome::Noop)
        }

        fn commit(&self, _file: &Path) -> Result<bool> {
            self.commit_calls.set(self.commit_calls.get() + 1);
            Ok(false)
        }

        fn retained_document_write(&self, _file: &Path) -> bool {
            self.retained_document_write
        }

        fn session_interruption(&self, _file: &Path) -> Result<Option<String>> {
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

    #[test]
    fn collect_agent_done_ids_reads_archive_attr_when_present() {
        let dir = TempDir::new().unwrap();
        let archive_rel = "tasks/done-archive.md";
        let archive_path = dir.path().join(archive_rel);
        std::fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
        std::fs::write(
            &archive_path,
            "- [x] [#archived1] First archived item\n- [x] [#archived2] Second\n",
        )
        .unwrap();
        let content = format!(
            "<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
            archive_rel
        );
        let ids = collect_agent_done_ids_with_root(&content, Some(dir.path()));
        assert!(
            ids.contains("archived1"),
            "expected ids to include archived1 from archive file: {:?}",
            ids
        );
        assert!(ids.contains("archived2"));
        let ids_no_root = collect_agent_done_ids_with_root(&content, None);
        assert!(ids_no_root.is_empty());
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

    // A settled status must not pay the retry cost — only the transient
    // editor-authority statuses are re-observed.
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

        // Simulate the PRIOR cycle: it dispatched #sy71 (first head) and committed
        // WITHOUT consuming it (no reap/done recorded).
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
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
        assert!(updated.contains("- do [#beta]"));
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
            "- do [#existing]\n- Implement active queue addition\n",
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
            !queue.contains("--- start"),
            "an already-active queue must not receive a redundant start fence:\n{updated}"
        );
        assert_eq!(
            state.selected_queue_prompts.first(),
            Some(&format!("do [#{id}]"))
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
    /// the auto-loop never got a head. Observed live on
    /// `tasks/brookebrodack-dev.md` (11 queued heads, `queue: start`, bare marker).
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
            queue.contains("- 🚧 do [#setup]\n- do [#ship]"),
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
    fn stale_queue_maintenance_cas_preserves_live_multi_adds_and_records_recovery() {
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
        let live_all = observed_first.replace(
            "- do [#fresh-one]\n",
            "- do [#fresh-one]\n- do [#fresh-two]\n- do [#fresh-three]\n",
        );
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
        let error = persist_queue_maintenance_doc(
            &doc,
            &stale_target,
            &observed_first,
            None,
            "qeditrace_test",
        )
        .expect_err("stale maintenance must fail closed");
        assert!(
            format!("{error:#}").contains("retry from the live head"),
            "failure must name the recovery: {error:#}"
        );

        let current =
            match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(&doc).unwrap() {
                agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
                other => panic!("expected embedded Lazily authority, got {other:?}"),
            };
        assert_eq!(current, live_all, "stale maintenance mutated the live head");
        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("queue_maintenance_compare_and_swap_blocked")
                && ops_log.contains("outcome=head_advanced")
                && ops_log.contains("recovery=retry_from_live_head"),
            "stale maintenance needs a durable recovery artifact:\n{ops_log}"
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
            "<!-- agent:queue priority go -->\n",
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
            updated.contains("- do [#opv]"),
            "operator-verify item must ALSO be mirrored into the queue (#mirrorall):\n{updated}"
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
            updated.contains("- do [#alpha]"),
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
            updated.contains("- do [#beta]"),
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
            updated.contains("- do [#beta]"),
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
        agent_doc_cycle_state_io::record_pending_added_ids(&doc, &["fresh".to_string()]).unwrap();

        let synced = sync_same_cycle_pending_adds_into_go_queue(
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
        agent_doc_cycle_state_io::record_pending_added_ids(&doc, &["fresh".to_string()]).unwrap();

        let synced = sync_same_cycle_pending_adds_into_go_queue(
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
        agent_doc_cycle_state_io::record_pending_added_ids(&doc, &["fresh".to_string()]).unwrap();

        let synced = sync_same_cycle_pending_adds_into_go_queue(
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
            updated.contains("queue: stop"),
            "marker `stop` must clear queue_active:\n{updated}"
        );
        assert!(
            !updated.contains("agent:queue stop"),
            "marker `stop` token must be stripped after halt:\n{updated}"
        );
    }

    #[test]
    fn run_queue_maintenance_removed_marker_go_stops_frontmatter_queue() {
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

        assert_eq!(state.queue_active, Some(false));
        assert!(!state.queue_continuation_required);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: stop"), "{updated}");
        assert!(updated.contains("<!-- agent:queue -->"), "{updated}");
        assert!(!updated.contains("agent:queue go"), "{updated}");
    }

    #[test]
    fn run_queue_maintenance_frontmatter_go_adds_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: stop\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: stop", "queue: go");
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
    fn run_queue_maintenance_frontmatter_stop_removes_marker_go() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n",
            "agent_doc_write: crdt\nqueue: go\n---\n\n",
            "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        );
        let current_content = snapshot_content.replace("queue: go", "queue: stop");
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
        assert!(updated.contains("queue: stop"), "{updated}");
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
    fn run_queue_maintenance_excludes_done_ids_from_backlog_sync() {
        // #ynra: a lingering active backlog `[ ]` bullet whose id is also archived
        // in `agent:done` must NOT be re-minted into the queue (it would be struck
        // every cycle and re-injected the next → forever churn). The fresh active
        // id is still minted.
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
            !updated.contains("[#na3x]") || !updated.contains("do [#na3x]"),
            "completed id must not be minted into the queue:\n{updated}"
        );
        assert!(
            !updated.contains("do [#na3x]"),
            "completed id must not appear as a queue do-prompt:\n{updated}"
        );
        assert!(
            updated.contains("do [#fresh]"),
            "fresh active id must still be queued:\n{updated}"
        );
        assert_eq!(state.synced_queue_ids, vec!["fresh".to_string()]);
    }
    #[test]
    fn run_queue_maintenance_excludes_external_archive_done_ids() {
        // #ynra (external-archive variant): a completed id reaped to the EXTERNAL
        // `agent:done archive=<file>` (not inline) must also be excluded from the
        // backlog→queue sync and struck from the queue. Done-id collection reads
        // the archive file, so the queue must not churn on an externally-archived
        // completed ref.
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
            !updated.contains("- do [#extdone]"),
            "externally-archived completed ref must be struck/excluded, not left live:\n{updated}"
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
            updated.contains("- do [#fresh]"),
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
            updated.matches("- do [#alpha]").count(),
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
            updated.contains("- 🚧 do [#fast]\n- do [#slow]"),
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
            updated.contains("- 🚧 do [#alpha]\n- do [#beta]"),
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
            updated.contains("- do [#verify]\n- do [#focused]\n- 🚧 do [#ready]"),
            "in-progress marker must project the first in-session drainable head:\n{updated}"
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
            updated.contains("- 🚧 do [#setup]\n- 🚧 do [#ship]\n- do [#ops]"),
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
            updated.contains("- 🚧 📌 do [#slow]\n- do [#fast]\n- do [#medium]"),
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
                 - :round_pushpin: do [#setup]\n\
                 - :pushpin: do [#ship]\n\
                 - :pushpin: do [#notify]"
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
            "queue_active: false\n",
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
        assert!(consumed.contains("- do [#nexthead]"));
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
            "## Completed / Reaped\n\n",
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
        assert!(updated.contains("queue: stop"), "file: {updated}");
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
        assert!(snap.contains("queue: stop"));
        assert!(!snap.contains("agent:queue auto"));
        assert!(!snap.contains("- do [#alpha]"));
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
            "## Completed / Reaped\n\n",
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
            vec!["do deploy".to_string(), "do deploy".to_string()],
            "intentional duplicate free-text prompts should remain queued: {state:?}"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("do deploy").count(),
            2,
            "maintenance should preserve intentional duplicate free-text prompts:\n{updated}"
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
            "queue_active: false\n",
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
        assert!(before.contains("- do [#second-live]"));
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
        assert!(file_after.contains("## Completed / Reaped"));
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
        assert!(snapshot_after.contains("## Completed / Reaped"));
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
        assert!(file_after.contains("## Completed / Reaped"));
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
    fn pending_maintenance_reaps_inline_done_backlog_and_review_mirrors() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#done1] stale backlog mirror\n",
            "- [ ] [#keep1] keep backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#done2] stale review mirror\n",
            "- [/] [#keep2] keep review\n",
            "<!-- /agent:review -->\n\n",
            "## Completed / Reaped\n\n",
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
            "- [ ] [#extdone1] stale backlog mirror\n",
            "- [ ] [#fresh1] fresh backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#extdone2] stale review mirror\n",
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
        enforce_no_dropped_backlog(&doc, head_content.as_deref())
            .expect("same-cycle reap should count as intentional completion");
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
        assert!(file_after.contains("## Completed / Reaped"));
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
        assert!(snapshot_after.contains("## Completed / Reaped"));
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
            file_after.contains("## Completed / Reaped") && file_after.contains("[#reap1] Reap me"),
            "completed item must be archived in the document:\n{file_after}"
        );
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
            retained_document_write: true,
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
                .contains("binary-owned response delivery is retained"),
        );
        assert_eq!(effects.repair_calls.get(), 0);
        assert_eq!(effects.commit_calls.get(), 0);
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
}
