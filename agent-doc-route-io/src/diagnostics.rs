//! Route diagnostics and route-dispatch bug filing I/O.

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::dispatch::{BusyRouteQueuedDiagnosticFacts, RouteDispatchBugReportFacts};
use agent_doc_controller::dispatch::{
    RouteBusyDiagnosticFacts, RouteBusyQueuedDiagnosticFacts, RouteDispatchBugReportItemFacts,
    RouteStartupMissDiagnosticFacts, route_busy_diagnostic_message,
    route_busy_queued_diagnostic_message, route_dispatch_bug_report_item,
    route_startup_miss_diagnostic_message,
};
use agent_doc_harness::HarnessConfig;
use tmux_router::Tmux;

pub type AddRouteDispatchBugBacklogItemsFn =
    fn(target_file: &Path, items: &[String], force_disk: bool) -> Result<Vec<String>>;

#[derive(Clone, Copy)]
pub struct RouteDispatchBugReportEffects {
    pub force_disk_pending_writes: fn() -> bool,
    pub add_backlog_items: AddRouteDispatchBugBacklogItemsFn,
}

pub fn route_dispatch_bug_force_disk_pending_writes() -> bool {
    crate::invocation::force_disk_route_writes()
}

pub fn add_route_dispatch_bug_backlog_items(
    target_file: &Path,
    items: &[String],
    force_disk: bool,
) -> Result<Vec<String>> {
    agent_doc_element_backlog_io::with_backlog_command_effects(
        &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
        || {
            agent_doc_element_backlog_io::backlog_cmd::with_force_disk_pending_writes(
                force_disk,
                || agent_doc_element_backlog_io::backlog_cmd::add_many(target_file, items, false),
            )
        },
    )
}

pub fn runtime_route_dispatch_bug_report_effects() -> RouteDispatchBugReportEffects {
    RouteDispatchBugReportEffects {
        force_disk_pending_writes: route_dispatch_bug_force_disk_pending_writes,
        add_backlog_items: add_route_dispatch_bug_backlog_items,
    }
}

pub fn file_route_dispatch_bug_report_with_runtime_effects(facts: RouteDispatchBugReportFacts<'_>) {
    file_route_dispatch_bug_report(facts, runtime_route_dispatch_bug_report_effects());
}

fn route_current_actor_generation(file: &Path) -> Option<u64> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    agent_doc_session_actor_io::load_record_in(&root, canonical.to_string_lossy().as_ref())
        .ok()
        .flatten()
        .map(|record| record.generation)
}

fn route_ops_log_path(file: &Path) -> Option<PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    Some(root.join(".agent-doc/logs/ops.log"))
}

pub fn file_route_dispatch_bug_report(
    facts: RouteDispatchBugReportFacts<'_>,
    effects: RouteDispatchBugReportEffects,
) {
    let document_display = facts.file.display().to_string();
    let document_id = agent_doc_hash::document_id_for_path(facts.file);
    let editor_attempt_id = crate::direct_pane_dispatch::editor_route_attempt_id();
    let dispatch_proof_state = facts.proof.map(|proof| proof.dispatch_stage_label());
    let diagnostic_path = facts.diagnostic_path.map(|path| path.display().to_string());
    let ops_log_path = route_ops_log_path(facts.file).map(|path| path.display().to_string());
    let item = match route_dispatch_bug_report_item(RouteDispatchBugReportItemFacts {
        document_display: &document_display,
        document_id: &document_id,
        pane: facts.pane,
        phase: facts.phase,
        issue: facts.issue,
        result: facts.result,
        elapsed_ms: facts.elapsed.as_millis(),
        actor_generation: route_current_actor_generation(facts.file),
        editor_attempt_id: editor_attempt_id.as_deref(),
        dispatch_proof_state,
        diagnostic_path: diagnostic_path.as_deref(),
        ops_log_path: ops_log_path.as_deref(),
    }) {
        Ok(item) => item,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_item_failed file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err).replace(char::is_whitespace, "_")
                ),
            );
            return;
        }
    };
    let target_file = match agent_doc_project_config_io::agent_doc_bug_target_document_for_doc(
        facts.file,
    ) {
        Ok(Some(target)) => target,
        Ok(None) => facts.file.to_path_buf(),
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_target_resolve_failed file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
            facts.file.to_path_buf()
        }
    };
    let items = [item];
    match (effects.add_backlog_items)(&target_file, &items, (effects.force_disk_pending_writes)()) {
        Ok(ids) => {
            let id = ids
                .first()
                .map(|id| id.as_str())
                .unwrap_or("deduped_existing");
            agent_doc_ops_log_io::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_filed file={} target_file={} pane={} harness={} phase={} issue={} id={} inserted={}",
                    facts.file.display(),
                    target_file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    id,
                    !ids.is_empty()
                ),
            );
        }
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                facts.file,
                &format!(
                    "route_dispatch_bug_backlog_file_failed file={} target_file={} pane={} harness={} phase={} issue={} error={}",
                    facts.file.display(),
                    target_file.display(),
                    facts.pane,
                    facts.harness.binary,
                    facts.phase,
                    facts.issue,
                    agent_doc_secret_redact::redact(&err.to_string())
                        .replace(char::is_whitespace, "_")
                ),
            );
        }
    }
}

pub fn emit_busy_route_queued_diagnostic_from_facts(facts: BusyRouteQueuedDiagnosticFacts<'_>) {
    emit_busy_route_queued_diagnostic(facts.tmux, facts.pane, facts.file, facts.harness);
}

const STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS: &str = "10000";
const BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS: &str = "10000";

pub fn emit_startup_miss_diagnostic(tmux: &Tmux, pane_id: &str, file: &Path, reason: &str) {
    let file_display = file.display().to_string();
    let msg = route_startup_miss_diagnostic_message(RouteStartupMissDiagnosticFacts {
        file_display: &file_display,
        reason,
    });
    if let Err(e) =
        agent_doc_tmux_io::show_message(tmux, pane_id, STARTUP_MISS_DIAGNOSTIC_DISPLAY_MS, &msg)
    {
        eprintln!(
            "[route] warning: failed to emit startup-miss diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

pub fn emit_busy_route_diagnostic(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    harness: &HarnessConfig,
) {
    let file_display = file.display().to_string();
    let msg = route_busy_diagnostic_message(RouteBusyDiagnosticFacts {
        file_display: &file_display,
        harness_binary: &harness.binary,
    });
    if let Err(e) =
        agent_doc_tmux_io::show_message(tmux, pane_id, BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS, &msg)
    {
        eprintln!(
            "[route] warning: failed to emit busy-route diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

pub fn emit_busy_route_queued_diagnostic(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    harness: &HarnessConfig,
) {
    let file_display = file.display().to_string();
    let user_outcome = agent_doc_flow::outcome::user_outcome_fields(
        agent_doc_flow::outcome::UserFacingOutcomeKind::QueuedBehindOwner,
    );
    let msg = route_busy_queued_diagnostic_message(RouteBusyQueuedDiagnosticFacts {
        file_display: &file_display,
        harness_binary: &harness.binary,
        user_outcome_fields: &user_outcome,
    });
    if let Err(e) =
        agent_doc_tmux_io::show_message(tmux, pane_id, BUSY_ROUTE_DIAGNOSTIC_DISPLAY_MS, &msg)
    {
        eprintln!(
            "[route] warning: failed to emit busy-route queued diagnostic to pane {}: {}",
            pane_id, e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_add(_target_file: &Path, _items: &[String], _force_disk: bool) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn no_force_disk() -> bool {
        false
    }

    #[test]
    fn route_bug_report_effects_are_copyable() {
        let effects = RouteDispatchBugReportEffects {
            force_disk_pending_writes: no_force_disk,
            add_backlog_items: noop_add,
        };
        let copied = effects;
        assert!(!(copied.force_disk_pending_writes)());
    }

    #[test]
    fn route_ops_log_path_uses_agent_doc_project_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "# Doc\n").unwrap();

        assert_eq!(
            route_ops_log_path(&doc),
            Some(dir.path().join(".agent-doc/logs/ops.log"))
        );
    }
}
