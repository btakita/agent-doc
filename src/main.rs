//! # Module: main (agent-doc CLI)
//!
//! ## Spec
//! - Entry point for the `agent-doc` binary; parses the command line with `clap` derive.
//! - Top-level struct `Cli` holds a single `Commands` subcommand enum (40+ variants).
//! - `AgentDocMode` enum (`Append`, `Template`, `Stream`) is a `ValueEnum` used by `Convert`
//!   and `Mode` subcommands; `Append` maps to inline format, `Template`/`Stream` to CRDT.
//! - On startup, calls `upgrade::warn_if_outdated()` for all subcommands except `Upgrade`.
//! - Loads global config via `agent_doc_config::load()` before dispatching; config is threaded into
//!   subcommands that accept an agent backend (`Run`, `Stream`, `Watch`, `Init`).
//! - Each subcommand delegates immediately to its owning module or focused crate (`agent_doc_run_io::run`, `agent_doc_diff_io::run`, etc.);
//!   `main` contains no business logic beyond argument destructuring and dispatch.
//! - `Route` no longer runs a follow-up sync; editor/plugin sync remains the
//!   authoritative layout path.
//! - `Write`/`Finalize` build `agent_doc_write_command_io::CommandOptions` and enter through
//!   the repair-owned empty-response bridge, which calls the extracted write runtime.
//! - `Prompt --all` runs `agent_doc_prompt_io::run_all()`; otherwise `FILE` is required.
//! - `History --restore <commit>` calls `history::restore`; bare `History` calls `history::list`.
//! - `Watch` dispatches to `agent_doc_watch_io::stop`, `agent_doc_watch_io::status`, or the CLI watch effects adapter based on flags.
//! - `Skill install --reload` prints `SKILL_RELOAD=compact` or `SKILL_RELOAD=restart` when the
//!   skill was updated, enabling the caller to take the appropriate reload action.
//! - `LibPath` prints the platform-appropriate shared library path (`libagent_doc.so/dylib/dll`)
//!   next to the binary, exiting with code 1 if not found.
//! - `ListCommands` emits a JSON array of all available subcommand names for plugin autocomplete.
//!
//! ## Agentic Contracts
//! - `try_main` returns `anyhow::Result<()>`; `main` renders any error with
//!   CRLF terminal newlines before exiting non-zero.
//! - Subcommand modules are the single source of truth for their behavior; `main` only routes.
//! - Config is loaded once and passed by reference; subcommands must not reload config.
//! - `Upgrade` bypasses the version check that all other subcommands run on startup.
//!
//! ## Evals
//! - dispatch_run: `agent-doc run <file>` → `agent_doc_run_io::run` called with correct args
//! - dispatch_write_crdt_autodetect: CRDT frontmatter + no flags → stream write mode selected
//! - dispatch_write_inline_autodetect: inline frontmatter + no flags → inline write mode selected
//! - dispatch_prompt_all: `--all` → `agent_doc_prompt_io::run_all`, no FILE required
//! - dispatch_history_restore: `--restore <sha>` → `history::restore` called
//! - dispatch_watch_stop: `--stop` flag → `agent_doc_watch_io::stop` called
//! - dispatch_skill_install_reload: skill updated + `--reload compact` → prints `SKILL_RELOAD=compact`
//! - dispatch_lib_path_missing: library absent → exits with code 1

mod annotate;
mod audit_docs;
mod auto_dag;
mod autoclaim;
mod clean;
mod cleanup_cmd;
mod commands;
mod convert;
mod crash_resilience;
mod dashboard_cmd;
mod dedupe_cmd;
mod describe_image;
mod exchange;
mod extract;
mod focus_effects;
mod history;
mod hook_cmd;
mod init;
mod install;
mod jobs;
mod layout;
mod lib_gc;
mod lib_install;
mod mcp;
mod migrate;
mod mode;
mod notify;
mod op_capture_verify;
mod ops_report;
mod orchestrate;
mod outline_cmd;
mod parallel;
mod patch;
mod plan;
mod plugin;
mod queue_dispatch;
mod queue_recovery;
mod read;
mod rename;
mod reset;
mod self_install;
mod serve;
mod session_actor_cmd;
mod session_cmd;
#[cfg(test)]
mod sim_world;
mod skill;
mod terminal;
#[cfg(test)]
mod test_support;
mod tsift_graph;
mod undo;
mod upgrade;
mod worktree;

use agent_doc_claim_io::ClaimRuntimeEffects;
use agent_doc_frontmatter::frontmatter;
use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_template_io as template_io;
use anyhow::Context;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

struct CliClaimRuntimeEffects;

impl ClaimRuntimeEffects for CliClaimRuntimeEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn commit(&self, file: &Path) -> anyhow::Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn provision_pane(
        &self,
        tmux: &tmux_router::Tmux,
        file: &Path,
        session_id: &str,
        file_path: &str,
        context_session: Option<&str>,
        col_args: &[String],
    ) -> anyhow::Result<String> {
        agent_doc_route_io::startup::provision_pane(
            tmux,
            file,
            session_id,
            file_path,
            context_session,
            col_args,
            agent_doc_route_io::runtime_effects::route_startup_effects(),
        )
    }
}

fn route_repair_closeout(file: &Path) -> anyhow::Result<String> {
    agent_doc_repair_command_io::repair(file).map(|outcome| format!("{outcome:?}"))
}

struct CliProjectControllerRuntimeEffects;

impl agent_doc_controller_io::project_controller::ProjectControllerRuntimeEffects
    for CliProjectControllerRuntimeEffects
{
    fn consume_queue_prompt_force_disk(
        &self,
        file: &Path,
    ) -> anyhow::Result<
        Option<agent_doc_controller_io::project_controller::ControllerQueueConsumptionOutcome>,
    > {
        Ok(
            agent_doc_queue_io::queue_consume::consume_queue_prompt_force_disk(
                file,
                &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
            )?
            .map(|outcome| {
                agent_doc_controller_io::project_controller::ControllerQueueConsumptionOutcome {
                    consumed_text: outcome.consumed_text,
                    remaining: outcome.remaining,
                    drained: outcome.drained,
                }
            }),
        )
    }

    fn route_auto_start(
        &self,
        tmux: &tmux_router::Tmux,
        file: &Path,
        session_id: &str,
        file_arg: &str,
        window: Option<&str>,
    ) -> anyhow::Result<String> {
        agent_doc_route_io::startup::auto_start(
            tmux,
            file,
            session_id,
            file_arg,
            window,
            agent_doc_route_io::runtime_effects::route_startup_effects(),
        )
    }

    fn run_editor_route(
        &self,
        invocation: agent_doc_controller_io::project_controller::ControllerEditorRouteInvocation,
    ) -> anyhow::Result<
        agent_doc_controller_io::project_controller::ControllerEditorRouteRuntimeResult,
    > {
        let mode = if invocation.dispatch_only {
            agent_doc_route_io::command::RouteMode::DispatchOnly
        } else {
            agent_doc_route_io::command::RouteMode::Managed
        };
        let wait_for_ready = invocation
            .wait_for_ready_secs
            .map(|secs| Duration::from_secs(secs.min(600)));
        match agent_doc_route_io::invocation::run_with_force_disk(
            &invocation.file,
            None,
            500,
            &invocation.layout_args,
            mode,
            invocation.plain_trigger,
            wait_for_ready,
            invocation.force_disk,
            agent_doc_route_io::runtime_effects::route_command_effects(route_repair_closeout),
        ) {
            Ok(()) => Ok(
                agent_doc_controller_io::project_controller::ControllerEditorRouteRuntimeResult {
                    exit_code: 0,
                    output: format!(
                        "[route] dispatched via controller editor_route for {}",
                        invocation.relative_path
                    ),
                },
            ),
            Err(err) => Ok(
                agent_doc_controller_io::project_controller::ControllerEditorRouteRuntimeResult {
                    exit_code: 1,
                    output: format!("{err:#}"),
                },
            ),
        }
    }

    fn sync_tmux_layout(
        &self,
        project_root: &Path,
        invocation: agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncInvocation,
    ) -> anyhow::Result<agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncReceipt>
    {
        let sync_result = if invocation.no_autostart {
            if invocation.exact_visible {
                agent_doc_sync_io::sync::run_layout_only_exact_visible_in_project_root(
                    project_root,
                    &invocation.columns,
                    invocation.window.as_deref(),
                    invocation.focus.as_deref(),
                )
            } else {
                agent_doc_sync_io::sync::run_layout_only_in_project_root(
                    project_root,
                    &invocation.columns,
                    invocation.window.as_deref(),
                    invocation.focus.as_deref(),
                )
            }
        } else {
            agent_doc_sync_io::sync::run_in_project_root(
                project_root,
                &invocation.columns,
                invocation.window.as_deref(),
                invocation.focus.as_deref(),
            )
        };
        sync_result?;
        let sync_report = agent_doc_sync_io::sync::last_sync_run_report();
        Ok(
            agent_doc_controller_io::project_controller::ControllerTmuxLayoutSyncReceipt {
                applied: sync_report.applied,
                reason: sync_report.reason,
                columns: invocation.columns,
                window: invocation.window,
                focus: invocation.focus,
                no_autostart: invocation.no_autostart,
                exact_visible: invocation.exact_visible,
            },
        )
    }

    fn commit_document(
        &self,
        file: &Path,
        authoritative_compaction: bool,
    ) -> anyhow::Result<agent_doc_controller_io::project_controller::ControllerCommitDocumentOutcome>
    {
        // Runs INSIDE the controller process (invoked by `handle_commit_document_rpc`).
        // `commit_document_in_controller` marks the commit as controller-owned so it
        // does not re-delegate over the socket and treats the relay barrier the
        // handler already flushed as pre-converged (`#cpc-commit`).
        let outcome =
            agent_doc_commit_io::commit_document_in_controller(file, authoritative_compaction)?;
        Ok(
            agent_doc_controller_io::project_controller::ControllerCommitDocumentOutcome {
                did_commit: outcome.did_commit,
                vcs_refresh_signaled: outcome.vcs_refresh_signaled,
            },
        )
    }
}

static PROJECT_CONTROLLER_RUNTIME_EFFECTS: CliProjectControllerRuntimeEffects =
    CliProjectControllerRuntimeEffects;

struct CliSyncRuntimeEffects;

impl agent_doc_sync_io::SyncRuntimeEffects for CliSyncRuntimeEffects {
    fn resolve_current_document(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn write_current_document(&self, file: &Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn commit(&self, file: &Path) -> anyhow::Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn detect_jb_cache_conflict_cancel_recoverable(&self, file: &Path) -> anyhow::Result<bool> {
        agent_doc_session_check_io::detect_jb_cache_conflict_cancel_recoverable(file)
    }

    fn detect_uncommitted_closeout_drift(&self, file: &Path) -> anyhow::Result<Option<String>> {
        agent_doc_session_check_io::detect_uncommitted_closeout_drift(
            file,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
    }

    fn repair(&self, file: &Path) -> anyhow::Result<agent_doc_turn::repair::RepairOutcome> {
        agent_doc_repair_command_io::repair(file)
    }

    fn repair_stale_preflight_started_cycle(
        &self,
        file: &Path,
    ) -> anyhow::Result<agent_doc_turn::repair::RepairOutcome> {
        agent_doc_repair_io::repair_stale_preflight_started_cycle(
            &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
            file,
        )
    }

    fn save_pending(&self, file: &Path, response: &str) -> anyhow::Result<()> {
        let current_content = self.resolve_current_document(file, "sync_save_pending_capture")?;
        agent_doc_repair_io::pending::save_pending_with_current_content(
            file,
            response,
            &current_content,
        )
    }

    fn session_check_inspect(
        &self,
        file: &Path,
    ) -> anyhow::Result<agent_doc_sync_io::SyncSessionCheckStatus> {
        match agent_doc_session_check_io::inspect(
            file,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )? {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                Ok(agent_doc_sync_io::SyncSessionCheckStatus::Ok(message))
            }
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => Ok(
                agent_doc_sync_io::SyncSessionCheckStatus::Interrupted(message),
            ),
        }
    }

    fn provision_pane(
        &self,
        tmux: &tmux_router::Tmux,
        file: &Path,
        session_id: &str,
        file_path: &str,
        context_session: Option<&str>,
        col_args: &[String],
    ) -> anyhow::Result<String> {
        agent_doc_route_io::startup::provision_pane(
            tmux,
            file,
            session_id,
            file_path,
            context_session,
            col_args,
            agent_doc_route_io::runtime_effects::route_startup_effects(),
        )
    }
}

static SYNC_RUNTIME_EFFECTS: CliSyncRuntimeEffects = CliSyncRuntimeEffects;

struct CliCompactRuntimeEffects;

impl agent_doc_compact_io::CompactRuntimeEffects for CliCompactRuntimeEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn commit_with_outcome(
        &self,
        file: &Path,
    ) -> anyhow::Result<agent_doc_compact_io::CompactCommitOutcome> {
        // `#jb-compact-commit-historical-patchback-guard`: this is the PRODUCTION
        // compaction closeout. Route through the compaction-aware commit entry —
        // otherwise the committed-historical response-patchback guard refuses the
        // compacted document because HEAD still carries the `### Re:` turns the
        // compaction just archived, and Compact Exchange fails closed with
        // "refusing to auto-adopt committed historical response patchback"
        // (observed live on agent-doc-bugs2.md). dd9ca291 fixed only the test
        // double (`TestCompactRuntimeEffects`); this CLI impl was still on the
        // plain `commit_with_outcome`, so the authoritative-compaction stand-down
        // never engaged in the real binary. Correctness stays enforced by
        // `verify_compact_head_landed` afterward. Keep this in lockstep with the
        // test double.
        let outcome = agent_doc_commit_io::commit_with_authoritative_compaction(file)?;
        Ok(agent_doc_compact_io::CompactCommitOutcome {
            did_commit: outcome.did_commit,
            vcs_refresh_signaled: outcome.vcs_refresh_signaled,
        })
    }

    fn atomic_write(&self, file: &Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn try_editor_converge(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> anyhow::Result<bool> {
        agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }

    fn guard_no_stale_snapshot_reset_drift(
        &self,
        file: &Path,
        projected: Option<&str>,
        visible: &str,
        stage: &str,
    ) -> anyhow::Result<bool> {
        let _ = (file, projected, visible, stage);
        Ok(false)
    }
}

static COMPACT_RUNTIME_EFFECTS: CliCompactRuntimeEffects = CliCompactRuntimeEffects;

/// Document mode for agent-doc sessions.
#[derive(Clone, Debug, ValueEnum)]
pub enum AgentDocMode {
    /// Append-mode: alternating ## User / ## Assistant blocks
    Append,
    /// Template-mode: in-place component patching
    Template,
    /// Stream-mode: real-time CRDT write-back (superset of template)
    Stream,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum PatchMode {
    Replace,
    Append,
    Prepend,
}

struct CliWorkflowDoctorEffects;

impl agent_doc_workflow_io::doctor::WorkflowDoctorEffects for CliWorkflowDoctorEffects {
    fn inspect_session_check(
        &mut self,
        file: &Path,
    ) -> anyhow::Result<Option<agent_doc_workflow_io::doctor::LiveSessionCheckFacts>> {
        let report = agent_doc_session_check_io::inspect_with_warnings(
            file,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )?;
        let facts = match report.status {
            agent_doc_session_check_io::SessionCheckStatus::Ok(message) => {
                agent_doc_workflow_io::doctor::LiveSessionCheckFacts {
                    ok: Some(true),
                    status: Some("ok".to_string()),
                    message: Some(message),
                    warnings: report.warnings,
                }
            }
            agent_doc_session_check_io::SessionCheckStatus::Interrupted(message) => {
                agent_doc_workflow_io::doctor::LiveSessionCheckFacts {
                    ok: Some(false),
                    status: Some("interrupted".to_string()),
                    message: Some(message),
                    warnings: report.warnings,
                }
            }
        };
        Ok(Some(facts))
    }

    fn inspect_actor(
        &mut self,
        project_root: &Path,
        file: &Path,
    ) -> anyhow::Result<agent_doc_workflow::doctor::ActorDoctorFacts> {
        let inspection = agent_doc_controller_io::project_controller::inspect_actor(
            project_root,
            Some(file),
            None,
            None,
        )?;
        let mut facts = agent_doc_workflow::doctor::ActorDoctorFacts {
            inspection_available: true,
            state: None,
            generation: None,
            pane: None,
            supervisor_pid: None,
            controller_fresh: None,
            supervisor_fresh: None,
            guidance: None,
        };
        if let Some(record) = inspection.record {
            facts.state = Some(record.state.as_str().to_string());
            facts.generation = Some(record.generation);
            facts.pane = Some(record.pane_id);
        }
        if let Some(lease) = inspection.supervisor_lease {
            facts.supervisor_pid = lease.supervisor_pid;
        }
        if let Some(freshness) = inspection.freshness {
            facts.controller_fresh = freshness.controller.matches_installed;
            facts.supervisor_fresh = freshness
                .route_owned_supervisor
                .as_ref()
                .and_then(|process| process.matches_installed);
            facts.guidance = Some(freshness.guidance);
        }
        Ok(facts)
    }

    fn live_buffer_diverges(
        &mut self,
        file: &Path,
        disk_content: &str,
        _project_root: &Path,
    ) -> anyhow::Result<Option<bool>> {
        Ok(Some(
            agent_doc_document_realtime_io::durable_buffer_state(file, disk_content).is_some(),
        ))
    }
}

struct CliGcControllerEffects;

impl agent_doc_gc_io::GcControllerEffects for CliGcControllerEffects {
    fn close_stale_starting_actors(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_starting_actors(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn close_stale_dead_pane_actors(
        &mut self,
        project_root: &Path,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
            project_root,
            dry_run,
            caller,
            reason,
        )
    }

    fn prune_dead_actors(
        &mut self,
        project_root: &Path,
        prune_after: Duration,
        dry_run: bool,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::prune_dead_actors(
            project_root,
            prune_after,
            dry_run,
        )
    }

    fn terminate_stale_preparing_controllers(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::terminate_stale_preparing_controllers(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn reap_orphaned_preparing_controllers(
        &mut self,
        project_root: &Path,
        stale_after: Duration,
        dry_run: bool,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_orphaned_preparing_controllers(
            project_root,
            stale_after,
            dry_run,
        )
    }

    fn reap_orphaned_preparing_controllers_all_projects(
        &mut self,
        stale_after: Duration,
        dry_run: bool,
        caller: &str,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_orphaned_preparing_controllers_all_projects(
            stale_after,
            dry_run,
            caller,
        )
    }

    fn reap_removed_project_root_controllers_all_projects(
        &mut self,
        stale_after: Duration,
        dry_run: bool,
        caller: &str,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::reap_removed_project_root_controllers_all_projects(
            stale_after,
            dry_run,
            caller,
        )
    }
}

struct CliAdminControllerEffects {
    tmux: tmux_router::Tmux,
}

impl Default for CliAdminControllerEffects {
    fn default() -> Self {
        Self {
            tmux: tmux_router::Tmux::default_server(),
        }
    }
}

fn admin_receipt_view(
    receipt: agent_doc_controller_io::project_controller::ControllerAdminReceipt,
) -> agent_doc_admin_io::ControllerAdminReceiptView {
    agent_doc_admin_io::ControllerAdminReceiptView {
        receipt_id: receipt.receipt_id,
        operation_kind: receipt.operation_kind,
        document_id: receipt.document_id,
        status: receipt.status,
        diagnostic_payload: receipt.diagnostic_payload,
        failed_stage: receipt.failed_stage,
        unblock_hint: receipt.unblock_hint,
        observed_generation: receipt.observed_generation,
        current_generation: receipt.current_generation,
    }
}

fn actor_inspection_view(
    inspection: agent_doc_controller_io::project_controller::ControllerActorInspection,
) -> agent_doc_admin_io::ControllerActorInspectionView {
    agent_doc_admin_io::ControllerActorInspectionView {
        target: inspection.target,
        document_id: inspection.document_id,
        record: inspection.record,
        supervisor_lease: inspection.supervisor_lease,
        freshness: inspection.freshness,
        queue_head: inspection.queue_head,
        queue_control: inspection.queue_control,
        queue_backpressure: inspection.queue_backpressure,
        projection_lag: inspection.projection_lag,
        dispatch_attempts: inspection.dispatch_attempts,
        admin_operations: inspection.admin_operations,
        projection_diagnostics: inspection.projection_diagnostics,
    }
}

impl agent_doc_admin_io::AdminControllerEffects for CliAdminControllerEffects {
    fn load_actor_list(
        &self,
        root: &Path,
    ) -> anyhow::Result<Vec<agent_doc_controller::fleet::ActorListRecord>> {
        let actors = agent_doc_controller_io::project_controller::load_actor_store(root)?;
        Ok(actors
            .values()
            .map(|record| agent_doc_controller::fleet::ActorListRecord {
                document_id: record.document_id.clone(),
                session_id: record.session_id.clone(),
                pane: record.pane_id.clone(),
                window: record.window_id.clone(),
                harness: record.harness.clone(),
                generation: record.generation,
                state: record.state.as_str().to_string(),
            })
            .collect())
    }

    fn load_registry_bindings(
        &self,
        root: &Path,
    ) -> anyhow::Result<Vec<agent_doc_controller::fleet::ActorListRegistryBinding>> {
        let registry = agent_doc_session_registry_io::load_in(root)?;
        Ok(registry
            .values()
            .map(
                |entry| agent_doc_controller::fleet::ActorListRegistryBinding {
                    session_id: entry.session_id.clone(),
                    supervisor_pid: entry.pid,
                    cwd: entry.cwd.clone(),
                },
            )
            .collect())
    }

    fn pane_alive(&self, pane: &str) -> bool {
        self.tmux.pane_alive(pane)
    }

    fn inspect_actor(
        &self,
        root: &Path,
        document: Option<&Path>,
        session: Option<&str>,
        pane: Option<&str>,
    ) -> anyhow::Result<agent_doc_admin_io::ControllerActorInspectionView> {
        agent_doc_controller_io::project_controller::inspect_actor(root, document, session, pane)
            .map(actor_inspection_view)
    }

    fn control_queue(
        &self,
        root: &Path,
        document: Option<&Path>,
        action: &str,
        observed_generation: Option<u64>,
        reason: Option<&str>,
        item_id: Option<&str>,
    ) -> anyhow::Result<agent_doc_admin_io::ControllerAdminReceiptView> {
        agent_doc_controller_io::project_controller::control_queue(
            root,
            document,
            action,
            observed_generation,
            reason,
            item_id,
        )
        .map(admin_receipt_view)
    }

    fn admin_reap(
        &self,
        root: &Path,
        document: Option<&Path>,
        session: Option<&str>,
        pane: Option<&str>,
        observed_generation: u64,
        reason: &str,
    ) -> anyhow::Result<agent_doc_admin_io::ControllerAdminReceiptView> {
        agent_doc_controller_io::project_controller::admin_reap(
            root,
            document,
            session,
            pane,
            observed_generation,
            reason,
        )
        .map(admin_receipt_view)
    }

    fn close_stale_dead_pane_actors_with_liveness(
        &self,
        root: &Path,
        pane_alive: &mut dyn FnMut(&str) -> bool,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_for_caller(
            root, pane_alive, dry_run, caller, reason,
        )
    }

    fn close_stale_dead_pane_actors_with_tmux(
        &self,
        root: &Path,
        dry_run: bool,
        caller: &str,
        reason: &str,
    ) -> anyhow::Result<(usize, usize)> {
        agent_doc_controller_io::project_controller::close_stale_dead_pane_actors_with_tmux_for_caller(
            root, dry_run, caller, reason,
        )
    }

    fn admin_handoff(
        &self,
        root: &Path,
        document: &Path,
        to_pane: &str,
        observed_generation: u64,
        reason: &str,
    ) -> anyhow::Result<agent_doc_admin_io::ControllerAdminReceiptView> {
        agent_doc_controller_io::project_controller::admin_handoff(
            root,
            document,
            to_pane,
            observed_generation,
            reason,
        )
        .map(admin_receipt_view)
    }

    fn repair_projection(
        &self,
        root: &Path,
        document: Option<&Path>,
        projection: &str,
        observed_generation: Option<u64>,
        reason: Option<&str>,
    ) -> anyhow::Result<agent_doc_admin_io::ControllerAdminReceiptView> {
        agent_doc_controller_io::project_controller::repair_projection(
            root,
            document,
            projection,
            observed_generation,
            reason,
        )
        .map(admin_receipt_view)
    }
}

struct CliQueueCommandEffects;

pub(crate) struct CliStreamRuntimeEffects;

pub(crate) static CLI_STREAM_RUNTIME_EFFECTS: CliStreamRuntimeEffects = CliStreamRuntimeEffects;

pub(crate) struct CliQueueConsumeWriteEffects;

pub(crate) static CLI_QUEUE_CONSUME_WRITE_EFFECTS: CliQueueConsumeWriteEffects =
    CliQueueConsumeWriteEffects;

impl agent_doc_queue_io::queue_consume::QueueConsumeWriteEffects for CliQueueConsumeWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }
}

fn cli_stream_effects() -> Arc<dyn agent_doc_stream_io::StreamRuntimeEffects> {
    Arc::new(CliStreamRuntimeEffects)
}

impl agent_doc_stream_io::StreamRuntimeEffects for CliStreamRuntimeEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn commit(&self, file: &Path) -> anyhow::Result<bool> {
        agent_doc_commit_io::commit(file)
    }

    fn save_pending(&self, file: &Path, response: &str) -> anyhow::Result<()> {
        let current_content = self.current_document_content(file, "stream_save_pending_capture")?;
        agent_doc_repair_io::pending::save_pending_with_current_content(
            file,
            response,
            &current_content,
        )
    }

    fn clear_pending(&self, file: &Path) -> anyhow::Result<()> {
        agent_doc_repair_io::pending::clear_pending(file)
    }

    fn atomic_write(&self, file: &Path, content: &str) -> anyhow::Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn try_ipc_stream_flush(
        &self,
        file: &Path,
        patches: &[agent_doc_template::PatchBlock],
        unmatched: &str,
    ) -> anyhow::Result<bool> {
        agent_doc_write_ipc_io::try_ipc_with_effects(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            patches,
            unmatched,
            None,
            None,
            None,
            None,
            None,
        )
        .map(|result| result.success)
    }

    fn fire_post_write(&self, file: &Path, session_id: &str) {
        let hook_effects = agent_doc_hooks_io::default_post_response_hook_effects();
        agent_doc_hooks_io::fire_post_write_with_effects(&hook_effects, file, session_id, 1);
        agent_doc_hooks_io::fire_doc_event(file, "post_write");
    }
}

fn queue_command_consume_outcome(
    outcome: agent_doc_queue_io::queue_consume::QueueConsumptionOutcome,
) -> agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome {
    agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome {
        consumed_text: outcome.consumed_text,
        remaining: outcome.remaining,
        drained: outcome.drained,
    }
}

impl agent_doc_queue_io::queue_cmd::QueueCommandEffects for CliQueueCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> anyhow::Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }

    fn consume_queue_prompt_force_disk(
        &self,
        file: &Path,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome>> {
        agent_doc_queue_io::queue_consume::consume_queue_prompt_force_disk(
            file,
            &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
        .map(|outcome| outcome.map(queue_command_consume_outcome))
    }

    fn consume_queue_prompt_with_outcome(
        &self,
        file: &Path,
    ) -> anyhow::Result<Option<agent_doc_queue_io::queue_cmd::QueueCommandConsumeOutcome>> {
        agent_doc_queue_io::queue_consume::consume_queue_prompt_with_outcome(
            file,
            &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
        .map(|outcome| outcome.map(queue_command_consume_outcome))
    }

    fn strike_orphan_id_backed_queue_head(&self, file: &Path, id: &str) -> anyhow::Result<bool> {
        agent_doc_queue_io::queue_consume::strike_orphan_id_backed_queue_head(
            file,
            id,
            &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
    }

    fn acknowledge_open_id_backed_queue_head(&self, file: &Path, id: &str) -> anyhow::Result<bool> {
        agent_doc_queue_io::queue_consume::acknowledge_open_id_backed_queue_head(
            file,
            id,
            &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
    }

    fn prune_noise_queue_heads(&self, file: &Path) -> anyhow::Result<usize> {
        agent_doc_queue_io::queue_consume::prune_noise_queue_heads(
            file,
            &CLI_QUEUE_CONSUME_WRITE_EFFECTS,
        )
    }
}

#[derive(Default)]
struct CliWatchDaemonEffects {
    actor_contexts: HashMap<PathBuf, agent_doc_run_context_io::ActorContext>,
}

impl agent_doc_watch_io::WatchDaemonEffects for CliWatchDaemonEffects {
    fn flush_stream_to_document(
        &mut self,
        file: &Path,
        text: &str,
        target: &str,
        baseline: &str,
    ) -> anyhow::Result<()> {
        agent_doc_stream_io::flush_to_document(
            file,
            text,
            target,
            baseline,
            &CLI_STREAM_RUNTIME_EFFECTS,
        )
    }

    fn route_file_change(
        &mut self,
        base_dir: &Path,
        doc_id: &str,
        file: &str,
        raw: &agent_doc_document_realtime::watch_authority::RawWatchEvent,
        current_content: &str,
    ) -> anyhow::Result<agent_doc_document_realtime::watch_authority::WatchDelivery> {
        let observation =
            agent_doc_watch_io::observe_document_event(doc_id, file, raw, current_content);
        if let Some(event) = observation.state_event {
            let actor = agent_doc_session_actor_io::document_actor_in(base_dir, file);
            let base_dir = base_dir.to_path_buf();
            actor.submit(
                agent_doc_document_realtime::session_ops::SessionOpKind::FileWatch,
                move |_ctx| -> anyhow::Result<()> {
                    agent_doc_controller_io::project_controller::append_state_event(
                        &base_dir, &event,
                    )?;
                    Ok(())
                },
            )??;
        }
        // C1b (`plan-crdt-scramble-and-disk-propagation.md`): for an editor-attached
        // document, ask CPC to drop a disk-change-reconcile marker so the controller
        // reconciles this out-of-band disk change into the canonical replica.
        // Best-effort + authority-gated inside the helper (headless docs get no
        // marker); a failure here must never wedge the watch loop.
        if let Err(e) = agent_doc_controller_io::project_controller::route_disk_change_signal_via_controller_model_for_doc(
            Path::new(file),
            &observation.delivery,
        ) {
            eprintln!("[watch] disk-change reconcile signal failed for {file}: {e}");
        }
        Ok(observation.delivery)
    }

    fn on_file_change(&mut self, path: &Path) -> anyhow::Result<()> {
        let ac = self
            .actor_contexts
            .entry(path.to_path_buf())
            .or_insert_with(|| agent_doc_run_context_io::actor_context(path.to_path_buf()));
        ac.on_file_change(path.to_path_buf());
        Ok(())
    }

    fn on_config_change(&mut self) -> anyhow::Result<usize> {
        for ac in self.actor_contexts.values() {
            ac.on_config_change();
        }
        Ok(self.actor_contexts.len())
    }

    fn on_stream_dead(&mut self, path: &Path) -> anyhow::Result<()> {
        self.actor_contexts.remove(path);
        Ok(())
    }
}

/// Structural node operations for `agent-doc exchange` (Phase 4 of the
/// exchange-tree plan). Each op mutates the exchange as a tree of distinct
/// response/prompt nodes, so it cannot bleed one node's content into another.
#[derive(Subcommand)]
enum ExchangeAction {
    /// List exchange nodes (id, kind, label) as JSON
    List {
        /// Path to the session document
        file: PathBuf,
    },
    /// Remove one exchange node by its stable id
    Remove {
        /// Path to the session document
        file: PathBuf,
        /// Node id (from `exchange list`)
        #[arg(long)]
        id: String,
    },
    /// Append an agent response turn; the body is read from stdin
    AddResponse {
        /// Path to the session document
        file: PathBuf,
        /// Response heading text (rendered as `### Re: <header>`)
        #[arg(long)]
        header: String,
    },
    /// Append a user prompt turn; the text is read from stdin
    AddPrompt {
        /// Path to the session document
        file: PathBuf,
    },
    /// Move a node immediately before/after an anchor node
    Move {
        /// Path to the session document
        file: PathBuf,
        /// Node id to move
        #[arg(long)]
        id: String,
        /// Anchor node id
        #[arg(long)]
        anchor: String,
        /// Insert before the anchor (default: after)
        #[arg(long)]
        before: bool,
    },
}

/// Turn lifecycle actions for `agent-doc turn-status`
/// (#claude-busy-status-during-active-turn).
#[derive(Subcommand)]
pub enum TurnStatusAction {
    /// A turn just started (UserPromptSubmit hook): show "turn in progress".
    Active,
    /// The turn just ended (Stop hook): clear the status.
    Idle,
    /// Install the monitor's hooks for Claude, Codex, and OpenCode.
    Install {
        /// Install root for all harnesses (default: cwd). Mainly for testing.
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Install into each harness's user config dir instead of the project.
        #[arg(long)]
        user: bool,
        /// Also enable tmux `pane-border-status` on the running server so the
        /// "turn in progress" border title is visible immediately.
        #[arg(long)]
        tmux: bool,
    },
}

#[derive(Parser)]
#[command(
    name = "agent-doc",
    version,
    about = "Interactive document sessions with AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn looks_like_document_path(arg: &str) -> bool {
    let path = Path::new(arg);
    path.exists() || path.components().count() > 1 || path.extension().is_some()
}

fn is_known_subcommand(arg: &str) -> bool {
    Cli::command()
        .get_subcommands()
        .any(|sub| sub.get_name() == arg || sub.get_all_aliases().any(|alias| alias == arg))
}

/// `#recycleforce` — pure decision: should a forced single-project `admin recycle`
/// escalate a dead supervisor to a kill+cold-start (`session restart-supervisor`)?
///
/// Only when ALL hold:
/// - `force` was passed (default behavior — no escalation — is unchanged),
/// - the controller recycle was a no-op (`recycled == false`, i.e. no live
///   controller answered, which on a forced recycle is the dead-supervisor signal),
/// - the operator passed a positional `target` that is a *document file* we can
///   cold-start (`session restart-supervisor` needs a file, not a project root).
///
/// Side-effect free so the escalation gate is unit-testable without driving a real
/// supervisor. The reused `session_actor_cmd::restart` carries its own
/// self-ancestor guard, so this decision deliberately does NOT re-implement one.
/// Whether a no-op `admin recycle` should escalate to a kill+cold-start
/// (`session restart-supervisor`). `#recycleforce` / `#recycle-no-boundaries`:
/// a recycle that found no live controller is almost always an operator trying
/// to bring a document's session back — either a DEAD supervisor (stale socket)
/// or a live supervisor whose project controller went away. `admin recycle` only
/// re-execs a *live* controller onto the fresh binary; it cannot revive those on
/// its own. Rather than dead-end with "nothing to recycle" and make the operator
/// pick a different command, escalate automatically whenever a session-document
/// path was given.
///
/// This is intentionally NOT gated on `--force` — recycle should "just work".
/// `--force` still flows through to `session_actor_cmd::restart`, where it
/// controls only whether a *busy* live pane is interrupted; the default keeps the
/// busy-pane guard. The reused `session_actor_cmd::restart` carries the same
/// self-ancestor guard `admin kill-supervisor` uses, so an escalation never tears
/// down the caller's own ancestor supervisor, and it routes a live supervisor to
/// a continue-restart and a dead one to a cold-start — so this single escalation
/// path covers every degraded state.
fn recycle_should_escalate_dead_supervisor(recycled: bool, target: Option<&Path>) -> bool {
    if recycled {
        return false;
    }
    match target {
        // A document path the operator typed (`agent-doc admin recycle <FILE>`).
        // Require an extension or an existing non-directory file so a bare
        // project-root directory argument does not get fed to
        // `session restart-supervisor`, which expects a session document.
        Some(path) => {
            if path.is_dir() {
                return false;
            }
            path.is_file() || path.extension().is_some()
        }
        None => false,
    }
}

/// Bounded wait for a `session *` ensure-or-cold-start to settle before retrying.
const SESSION_ENSURE_SUPERVISOR_WAIT: Duration = Duration::from_secs(8);

/// #supresilience Part C — classify a `session *` failure as a dead/unreachable-
/// supervisor fail-closed refusal that an ensure-or-cold-start should escalate.
///
/// These are the exact refusals `session_actor_cmd::clear` raises when it cannot
/// deliver a clear because there is no usable supervisor AND no live pane to fall
/// back on (verified against `src/session_actor_cmd.rs`):
/// - `ensure_supervisor_socket`: "no live supervisor socket for {} ..." (no socket).
/// - `send_command` context: "failed to contact supervisor for {}" (a STALE socket
///   left by a crashed supervisor — the Part B crash case).
/// - legacy IPC + no pane: "supervisor does not support clear IPC and no live pane
///   is available for direct `/clear` submission for {} ...".
///
/// It deliberately does NOT match:
/// - the controller-connect timeout ("timed out waiting for project controller ..."),
///   because when the whole control plane is down the escalation (`restart` →
///   `request_supervisor_replacement`) hits the same timeout and cannot help; and
/// - the turn-scoped no-op reports ("No active turn to cancel for ...", stop-agent's
///   supervisor-provided errors), so `stop-agent` / `cancel-turn` stay fail-closed and
///   never fabricate a turn (they are also not wrapped).
fn session_error_is_missing_supervisor(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let msg = cause.to_string();
        msg.contains("no live supervisor socket for")
            || msg.contains("failed to contact supervisor for")
            || msg.contains("supervisor does not support clear IPC and no live pane is available")
    })
}

/// #supresilience Part C — poll the supervisor socket until it becomes live or the
/// timeout elapses, so an ensure-or-cold-start retry has a fair chance to succeed
/// immediately after the cold-start dispatch. Best-effort: any resolution failure
/// (no session id / no project root) returns without waiting.
fn wait_for_live_supervisor_socket(file: &Path, timeout: Duration) {
    let canonical = file
        .canonicalize()
        .unwrap_or_else(|_| agent_doc_git_io::dirs::resolve_absolute_file_path(file));
    let Some(session_id) = agent_doc_frontmatter_io::session::read_session_id(&canonical) else {
        return;
    };
    let Some(base_dir) = agent_doc_fs::find_project_root(&canonical) else {
        return;
    };
    let socket = agent_doc_supervisor_io::ipc::socket_path(&base_dir, &session_id);
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if matches!(
            agent_doc_supervisor_io::ipc::probe_socket(&socket),
            agent_doc_supervisor_io::ipc::SocketLiveness::Live
        ) {
            return;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// #supresilience Part C — run a supervisor-requiring `session *` command and, if it
/// fails closed because no live supervisor exists, escalate through the SAME
/// dead→cold-start path `admin recycle <FILE>` uses
/// (`recycle_should_escalate_dead_supervisor` gate + `session_actor_cmd::restart`
/// continue-mode), wait briefly for the supervisor to come up, then retry ONCE.
///
/// Only `session clear` is wrapped: it is the supervisor-requiring command that
/// genuinely fails closed (it must inject `/clear` through the supervisor or a live
/// pane, and errors when neither is available). `session status` / `session doctor`
/// are NOT wrapped — they REPORT a dead supervisor as state/issues and return `Ok`, so
/// there is no fail-closed refusal to escalate (a read/report must not cold-start a
/// whole control plane as a side effect). Turn-scoped commands (`stop-agent`,
/// `cancel-turn`) are NOT wrapped either: they may legitimately no-op / fail closed
/// when there is no live agent turn, and this must never fabricate a turn.
fn run_session_command_ensuring_supervisor<F>(file: &Path, run: F) -> anyhow::Result<()>
where
    F: Fn() -> anyhow::Result<()>,
{
    match run() {
        Ok(()) => Ok(()),
        Err(err) => {
            // Only escalate the specific "no live supervisor" refusal, only for a real
            // session document (one that can be cold-started), and only through the
            // shared `admin recycle` escalation gate.
            if !session_error_is_missing_supervisor(&err)
                || agent_doc_frontmatter_io::session::read_session_id(file).is_none()
                || !recycle_should_escalate_dead_supervisor(false, Some(file))
            {
                return Err(err);
            }
            eprintln!(
                "[session] supervisor for {} is dead or unreachable — escalating to a cold-start via `session restart-supervisor {}` before retrying once",
                file.display(),
                file.display()
            );
            session_actor_cmd::restart(file, session_actor_cmd::RestartMode::Continue, false)
                .with_context(|| {
                    format!(
                        "failed to cold-start a supervisor for {} while ensuring one exists",
                        file.display()
                    )
                })?;
            wait_for_live_supervisor_socket(file, SESSION_ENSURE_SUPERVISOR_WAIT);
            run()
        }
    }
}

/// `#recycle-supervisor-fanout` — an explicit `admin recycle --all-projects` schedules a
/// recycle of every valid-state route-owned supervisor in addition to the controllers, so
/// an operator recycle fans out across the whole fleet (route-owned supervisors host the
/// agent turns, not just the project controllers). Pure JSON shape so the reported contract
/// is unit-tested independently of the `/proc` enumeration.
fn all_projects_recycle_json(
    controllers_recycled: usize,
    controllers_skipped: usize,
    supervisors_marked: usize,
    supervisors_skipped: usize,
    force: bool,
) -> serde_json::Value {
    serde_json::json!({
        "scope": "all_projects",
        "recycled": controllers_recycled,
        "skipped": controllers_skipped,
        "supervisors_marked": supervisors_marked,
        "supervisors_skipped": supervisors_skipped,
        "forced": force,
    })
}

fn is_bare_document_invocation(args: &[OsString]) -> bool {
    let Some(first) = args.get(1).and_then(|arg| arg.to_str()) else {
        return false;
    };
    if first.starts_with('-') {
        return false;
    }

    !is_known_subcommand(first) && looks_like_document_path(first)
}

fn reject_plain_shell_bare_file_invocation(args: &[OsString]) -> anyhow::Result<()> {
    if !is_bare_document_invocation(args) {
        return Ok(());
    }

    if agent_doc_model_tier::detect_harness() == "default" {
        anyhow::bail!(
            "bare `agent-doc <FILE>` must be run from a supported harness (Codex, Claude Code, or OpenCode). From a normal shell, use an explicit subcommand such as `agent-doc run <FILE>`, `agent-doc route <FILE>`, or `agent-doc start <FILE>`."
        );
    }

    Ok(())
}

fn rewrite_bare_file_invocation(mut args: Vec<OsString>) -> Vec<OsString> {
    if is_bare_document_invocation(&args) {
        args.insert(1, OsString::from("run"));
    }
    args
}

fn deprecated_pending_alias_used(args: &[OsString]) -> bool {
    matches!(args.get(1).and_then(|arg| arg.to_str()), Some("pending"))
}

#[derive(Args, Clone)]
struct WriteArgs {
    /// Path to the session document
    file: PathBuf,
    /// Baseline content for 3-way merge (reads from file if omitted)
    #[arg(long)]
    baseline_file: Option<PathBuf>,
    /// Template mode: parse <!-- patch:name --> blocks and apply to components
    #[arg(long)]
    template: bool,
    /// Stream mode: template patches with CRDT merge (conflict-free)
    #[arg(long)]
    stream: bool,
    /// IPC mode: write patch JSON to .agent-doc/patches/ for IDE plugin consumption
    #[arg(long)]
    ipc: bool,
    /// Force direct disk write, skip IPC even when plugin is installed
    #[arg(long)]
    force_disk: bool,
    /// Write origin identifier for tracing (e.g., "skill", "watch", "stream")
    #[arg(long)]
    origin: Option<String>,
    /// Add a new backlog item at the beginning of the list (repeatable).
    /// Multiple flags in one invocation land in flag order, top-down: the first
    /// `--backlog-add` is topmost (what you read is what you get). For a specific
    /// interleave with existing items, use `--backlog-add-after`/`--backlog-add-before`.
    /// Prefix with canonical `id=<custom> ` to preserve a custom id instead of generating one.
    /// Leading `[#custom] ` is also accepted as compatibility input.
    #[arg(long = "backlog-add", alias = "pending-add")]
    pending_add: Vec<String>,
    /// Add a new backlog item to another document's backlog (repeatable pairs: FILE TEXT).
    /// Prefix TEXT with canonical `id=<custom> ` to preserve a custom id.
    #[arg(
        long = "backlog-add-to",
        alias = "pending-add-to",
        num_args = 2,
        value_names = ["FILE", "TEXT"]
    )]
    pending_add_to: Vec<String>,
    /// Add a new gated backlog item at the beginning of the list (repeatable).
    /// Like `--backlog-add`, multiple flags land in flag order, top-down (first flag topmost).
    /// Prefix with canonical `id=<custom> ` to preserve a custom id instead of generating one.
    /// Leading `[#custom] ` is also accepted as compatibility input.
    #[arg(long = "backlog-add-gated", alias = "pending-add-gated")]
    pending_add_gated: Vec<String>,
    /// Add a new backlog item immediately AFTER an existing item (repeatable pairs: ID TEXT).
    /// Chains build a deterministic order: `--backlog-add-after A "B" --backlog-add-after B "C"` -> A->B->C.
    #[arg(
        long = "backlog-add-after",
        alias = "pending-add-after",
        num_args = 2,
        value_names = ["ID", "TEXT"]
    )]
    pending_add_after: Vec<String>,
    /// Add a new backlog item immediately BEFORE an existing item (repeatable pairs: ID TEXT).
    #[arg(
        long = "backlog-add-before",
        alias = "pending-add-before",
        num_args = 2,
        value_names = ["ID", "TEXT"]
    )]
    pending_add_before: Vec<String>,
    /// Add a new backlog item at the END of the active list (repeatable). Alias `--backlog-append`.
    #[arg(
        long = "backlog-add-back",
        alias = "pending-add-back",
        alias = "backlog-append",
        alias = "pending-append"
    )]
    pending_add_back: Vec<String>,
    /// Add a new icebox item at the beginning of the list (repeatable).
    #[arg(long = "icebox-add")]
    icebox_add: Vec<String>,
    /// Add a new icebox item immediately AFTER an existing item (repeatable pairs: ID TEXT).
    #[arg(long = "icebox-add-after", num_args = 2, value_names = ["ID", "TEXT"])]
    icebox_add_after: Vec<String>,
    /// Add a new icebox item immediately BEFORE an existing item (repeatable pairs: ID TEXT).
    #[arg(long = "icebox-add-before", num_args = 2, value_names = ["ID", "TEXT"])]
    icebox_add_before: Vec<String>,
    /// Add a new icebox item at the END of the list (repeatable). Alias `--icebox-append`.
    #[arg(long = "icebox-add-back", alias = "icebox-append")]
    icebox_add_back: Vec<String>,
    /// Edit an icebox item: `id=new text` (repeatable).
    #[arg(long = "icebox-edit")]
    icebox_edit: Vec<String>,
    /// Clear all icebox items.
    #[arg(long = "icebox-clear")]
    icebox_clear: bool,
    /// Reorder icebox items by comma-separated hash ids.
    #[arg(long = "icebox-reorder")]
    icebox_reorder: Option<String>,
    /// Mark a backlog or icebox item `[x]` by hash id (repeatable).
    #[arg(long = "done")]
    pending_done: Vec<String>,
    /// Edit a backlog item: `id=new text` (repeatable).
    #[arg(long = "backlog-edit", alias = "pending-edit")]
    pending_edit: Vec<String>,
    /// Clear all backlog items.
    #[arg(long = "backlog-clear", alias = "pending-clear")]
    pending_clear: bool,
    /// Reorder backlog items by comma-separated hash ids.
    #[arg(long = "backlog-reorder", alias = "pending-reorder")]
    pending_reorder: Option<String>,
    /// Transition a backlog item to `[/]` (gated) by hash id (repeatable).
    /// Idempotent on already-gated items; errors on `[x]` items.
    #[arg(long = "backlog-gate", alias = "pending-gate")]
    pending_gate: Vec<String>,
    /// Transition a backlog item from `[/]` back to `[ ]` by hash id (repeatable).
    /// Errors on `[ ]` or `[x]` items — the source must be gated.
    #[arg(long = "backlog-ungate", alias = "pending-ungate")]
    pending_ungate: Vec<String>,
    /// Resolve all items matching a typed gate (e.g., [/release] → [x]).
    #[arg(long = "backlog-resolve-gate", alias = "pending-resolve-gate")]
    pending_resolve_gate: Vec<String>,
    /// Set a typed gate on a gated item: `id=gate_type` (e.g., `gqep=release`).
    #[arg(long = "backlog-set-gate-type", alias = "pending-set-gate-type")]
    pending_set_gate_type: Vec<String>,
    /// Set a typed proof/disproof verify predicate on a gated item so the gate
    /// auto-resolves from ops.log markers (`#optverify`):
    /// `id=verify=ops_log:<marker>;disproof=ops_log:<text>` (repeatable).
    /// The gate-set timestamp is stamped automatically.
    #[arg(long = "backlog-set-verify", alias = "pending-set-verify")]
    pending_set_verify: Vec<String>,
    /// Add a new gated item directly to the review list (repeatable).
    #[arg(long = "review-add")]
    review_add: Vec<String>,
    /// Edit a review item: `id=new text` (repeatable).
    #[arg(long = "review-edit")]
    review_edit: Vec<String>,
    /// Remove a review item by id, deleting every entry that shares the id
    /// (clears stale or duplicate review entries; repeatable).
    #[arg(long = "review-remove")]
    review_remove: Vec<String>,
    /// Resolve a review item by id: remove from `agent:review` and archive to
    /// `agent:done` (the completion path; repeatable).
    #[arg(long = "review-resolve")]
    review_resolve: Vec<String>,
    /// Allow `replace:pending` blocks in stdin (escape hatch, hidden).
    #[arg(long = "allow-replace-pending", hide = true)]
    allow_replace_pending: bool,
    /// Only mutate tracked-work components — skip stdin reading and exchange synthesis.
    /// Requires at least one backlog/icebox/review mutation flag; incompatible with --template/--stream/--ipc.
    #[arg(long = "backlog-only", alias = "pending-only")]
    pending_only: bool,
    /// Replace the status component content (repeatable for multi-line).
    #[arg(long = "status")]
    status: Option<String>,
    /// Override the tagpath agent-doc lint dialect mode for this run.
    /// Values: `off | warn | strict`. Precedence: CLI > frontmatter
    /// `agent_doc_lint_dialect` > workspace `.agent-doc/config.toml`
    /// `[lint] dialect` > default (`warn`).
    #[arg(long = "lint", value_name = "MODE")]
    lint: Option<String>,
    /// Cross-repo sibling commit driven by this cycle (repeatable).
    /// After the session-document commit lands, stage and commit the named
    /// sibling working tree with a `Session-Doc:` git trailer auto-injected
    /// (URL points at the just-committed session-document blob). Stage the
    /// sibling's files before invoking `finalize` / `write --commit`. Pair
    /// each `--commit-sibling` with one `--commit-sibling-message` in the
    /// same order.
    #[arg(long = "commit-sibling", value_name = "REPO")]
    commit_sibling: Vec<PathBuf>,
    /// Commit message for each `--commit-sibling` (repeatable, positional pairing).
    #[arg(long = "commit-sibling-message", value_name = "MSG")]
    commit_sibling_message: Vec<String>,
}

#[derive(Subcommand)]
enum McpAction {
    /// Run a stdio Model Context Protocol server for agent-doc tools
    Serve {
        /// Set the working directory before serving MCP requests
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Run a session: diff, send to agent, write response by document mode
    Run {
        /// Path to the session document
        file: PathBuf,
        /// Auto-create a branch for session commits
        #[arg(short = 'b')]
        branch: bool,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Preview what would be sent without submitting
        #[arg(long)]
        dry_run: bool,
        /// Skip git commit after submit
        #[arg(long)]
        no_git: bool,
        /// Allow run closeout recovery writes to bypass editor IPC when no listener is attached
        #[arg(long)]
        force_disk: bool,
    },
    /// List or restore exchange component versions from git history
    History {
        /// Path to the session document
        file: PathBuf,
        /// Restore exchange content from a specific commit (prepend to current)
        #[arg(long)]
        restore: Option<String>,
    },
    /// Annotated git log for a session document (shows pre-compact tags)
    Log {
        /// Path to the session document
        file: PathBuf,
    },
    /// Inspect operational logs
    Ops {
        #[command(subcommand)]
        action: OpsAction,
    },
    /// Verify editor op-capture producer and merge-consumer evidence in ops.log
    VerifyOpCapture {
        /// Path to the session document
        file: PathBuf,
        /// Require the canonical café 日本 😀 byte-offset evidence
        #[arg(long)]
        expect_cafe_demo: bool,
    },
    /// Show document content at a specific point in git history
    Show {
        /// Path to the session document
        file: PathBuf,
        /// Show the file N commits back from HEAD (e.g. --back 1 → HEAD~1)
        #[arg(long)]
        back: Option<usize>,
        /// Show the Nth commit in git log order (0 = newest, 1 = next oldest, …)
        #[arg(long)]
        at: Option<usize>,
        /// Show the commit pointed to by this tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Scaffold a new session document (omit file to initialize project)
    Init {
        /// Path for the new session document (omit to initialize project)
        file: Option<PathBuf>,
        /// Session title
        title: Option<String>,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Document mode: append (default) or template
        #[arg(long)]
        mode: Option<String>,
    },
    /// System-level setup: check prerequisites, install editor plugins
    Install {
        /// Editor to install plugin for (jetbrains or vscode; auto-detected if omitted)
        #[arg(long)]
        editor: Option<String>,
        /// Skip prerequisite checks
        #[arg(long)]
        skip_prereqs: bool,
        /// Skip plugin installation
        #[arg(long)]
        skip_plugins: bool,
    },
    /// Preview the diff that would be sent, or diff between two git refs
    Diff {
        /// Path to the session document
        file: PathBuf,
        /// Wait for stable content (truncation detection) before computing diff
        #[arg(long)]
        wait: bool,
        /// Starting git ref for historical diff (e.g. commit hash, tag, HEAD~2)
        #[arg(long)]
        from: Option<String>,
        /// Ending git ref for historical diff (default: HEAD)
        #[arg(long)]
        to: Option<String>,
    },
    /// Clear session ID and delete or rebuild snapshot state
    /// Structural operations on the agent:exchange component (node-safe: no
    /// cross-response merge bleed; re-baselines snapshot + CRDT)
    Exchange {
        #[command(subcommand)]
        action: ExchangeAction,
    },
    Reset {
        /// Path to the session document
        file: PathBuf,
        /// Rebuild snapshot and CRDT state from the current visible markdown
        #[arg(long)]
        from_current: bool,
        /// With --from-current, preserve resume/session metadata and capture history
        #[arg(long)]
        preserve_session: bool,
        /// Bypass editor convergence and write reset-owned document mutations
        /// directly to disk. Intended for explicit recovery/headless invocations.
        #[arg(long)]
        force_disk: bool,
    },
    /// Squash session git history into one commit
    Clean {
        /// Path to the session document
        file: PathBuf,
        /// Create an archive tag before squashing (preserves full history)
        #[arg(long)]
        archive: bool,
    },
    /// Audit instruction files against the codebase
    AuditDocs {
        /// Project root directory (auto-detected if omitted)
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Garbage-collect orphaned files in .agent-doc/
    Gc {
        /// Project root directory (auto-detected if omitted)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Show what would be deleted without deleting
        #[arg(long)]
        dry_run: bool,
    },
    /// List or restore a document's pre-mutation recovery checkpoints
    /// (pre-auto-run / pre-compact tags)
    Checkpoint {
        /// Path to the session document
        file: PathBuf,
        /// Restore only this document from the named checkpoint tag (other files
        /// untouched); review and commit afterward
        #[arg(long, value_name = "TAG")]
        restore: Option<String>,
        /// Show `git diff <TAG> -- <FILE>` for the named checkpoint tag
        #[arg(long, value_name = "TAG", conflicts_with = "restore")]
        diff: Option<String>,
    },
    /// Start Claude in a tmux pane and register the session
    Start {
        /// Path to the session document
        file: PathBuf,
        /// Force binding the session to the current tmux pane, even if a live
        /// owner already exists in another pane
        #[arg(long)]
        force: bool,
        /// Internal route-owned pane mode: exit and reap after the first
        /// binary-owned document cycle commits when the document has no
        /// continued-interaction signals.
        #[arg(long = "route-owned", hide = true)]
        route_owned: bool,
        /// Internal route-owned pane reap policy.
        #[arg(
            long = "route-owned-reap-policy",
            hide = true,
            default_value_t = agent_doc_supervisor::route_owned::RouteOwnedReapPolicy::Auto
        )]
        route_owned_reap_policy: agent_doc_supervisor::route_owned::RouteOwnedReapPolicy,
    },
    /// Route agent-doc command to the correct tmux pane
    Route {
        /// Path to the session document
        file: PathBuf,
        /// Resolve the owning pane and send the bare reopen without route-owned
        /// busy-session recovery, startup-miss gating, or cycle-ack waiting.
        #[arg(long)]
        dispatch_only: bool,
        /// Send the plain `agent-doc <FILE>` reopen even for harnesses whose
        /// normal startup trigger is slash-command based.
        #[arg(long)]
        plain_trigger: bool,
        /// Tmux pane ID for lazy claiming (auto-claims if existing claim is stale)
        #[arg(long)]
        pane: Option<String>,
        /// Editor layout columns (comma-separated files per column, repeatable)
        #[arg(long = "col")]
        cols: Vec<String>,
        /// Focused file in the editor (for tmux pane focus)
        #[arg(long)]
        focus: Option<String>,
        /// Wait for typing to settle before routing (milliseconds, 0 = no debounce)
        #[arg(long, default_value_t = 500)]
        debounce: u64,
        /// Override the bounded wait for the authoritative actor to become
        /// dispatch-ready (seconds). When the actor is still in `starting`
        /// state, route normally fails closed after a harness-specific
        /// timeout (e.g. 10s for claude). User-initiated dispatches —
        /// especially the JB plugin's `Run Agent Doc` — can pass a longer
        /// wait (e.g. 60) so the user does not have to manually rerun while
        /// the supervisor is still booting. Capped at 600s.
        #[arg(long)]
        wait_for_ready: Option<u64>,
        /// Bypass editor convergence and write route-owned document mutations
        /// directly to disk. Intended for headless/no-listener invocations.
        #[arg(long)]
        force_disk: bool,
    },
    /// Detect permission prompts from a Claude Code or OpenCode session
    Prompt {
        /// Path to the session document (omit with --all)
        file: Option<PathBuf>,
        /// Answer a prompt by selecting option N (1-based)
        #[arg(long)]
        answer: Option<usize>,
        /// Poll all active sessions instead of a single file
        #[arg(long)]
        all: bool,
    },
    /// Commit a session document (git add + commit with timestamp)
    Commit {
        /// Path to the session document
        file: PathBuf,
    },
    /// Remove consecutive duplicate response blocks
    Dedupe {
        /// Path to the session document
        file: PathBuf,
    },
    /// Reclaim an orphaned cycle after an explicit run cancel: abandon an empty
    /// `preflight_started` cycle (no response capture) so the next dispatch can
    /// start fresh immediately instead of waiting for the staleness window.
    Cancel {
        /// Path to the session document
        file: PathBuf,
    },
    /// Claim a document for the current tmux pane
    Claim {
        /// Path to the session document
        file: PathBuf,
        /// Positional hint to select pane by position (left, right, top, bottom)
        #[arg(long)]
        position: Option<String>,
        /// Explicit tmux pane ID (e.g. %42) — overrides position detection
        #[arg(long)]
        pane: Option<String>,
        /// Scope pane resolution to this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
        /// Force overwrite tmux_session even if already set to a different session
        #[arg(long)]
        force: bool,
        /// Spawn a fresh Claude Code session in a new tmux window scoped to the
        /// document's nearest git repo root (loads CLAUDE.md, memory, skills for
        /// that repo rather than the superproject)
        #[arg(long)]
        isolate: bool,
    },
    /// Claim (or release) the drain-owner lease for a self-driving harness loop
    /// (#kp5z / #qflood). The Claude Code `/loop` auto-loop refreshes this lease
    /// just before re-invoking `/loop` so the supervisor idle-queue watch defers
    /// instead of double-injecting `agent-doc <FILE>` into the live input queue.
    #[command(name = "drain-claim")]
    DrainClaim {
        /// Path to the session document
        file: PathBuf,
        /// Owner tag for the lease (default: claude_loop)
        #[arg(long, default_value = agent_doc_queue::drain_owner::DRAIN_OWNER_CLAUDE_LOOP)]
        owner: String,
        /// Release the lease instead of claiming/refreshing it
        #[arg(long)]
        release: bool,
    },
    /// Focus the tmux pane for a session document
    Focus {
        /// Path to the session document
        file: PathBuf,
        /// Explicit tmux pane ID — overrides session lookup
        #[arg(long)]
        pane: Option<String>,
        /// Run the legacy synchronous focus path, including best-effort stash
        /// promotion before selecting the pane.
        #[arg(long, alias = "synchronous")]
        blocking: bool,
    },
    /// Arrange tmux panes to mirror editor split layout
    Layout {
        /// Session documents to arrange
        files: Vec<PathBuf>,
        /// Split direction: h (horizontal/side-by-side) or v (vertical/stacked)
        #[arg(long, short, default_value = "h")]
        split: String,
        /// Explicit tmux pane ID — scopes pane selection to this pane's session
        #[arg(long)]
        pane: Option<String>,
        /// Only operate on panes within this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
    },
    /// Sync tmux panes to a 2D columnar layout matching the editor
    Sync {
        /// Columns of comma-separated file paths (left-to-right). Repeat for each column.
        /// When omitted, sync falls back to the recorded `.agent-doc/last_layout.json`
        /// for the current sync scope.
        #[arg(long = "col")]
        columns: Vec<String>,
        /// Only operate on panes within this tmux window (e.g. @1)
        #[arg(long)]
        window: Option<String>,
        /// Focus this file's pane after arranging (defaults to first file)
        #[arg(long)]
        focus: Option<String>,
        /// Signal that this sync was triggered by a file rename. Creates a debounce marker
        /// that suppresses auto-start for the focused file across subsequent syncs (5s TTL).
        #[arg(long)]
        rename: bool,
        /// Arrange/reconcile existing panes without auto-starting replacement sessions.
        #[arg(long)]
        no_autostart: bool,
        /// Treat provided --col values as the exact editor-visible projection.
        /// This disables focus-only expansion from remembered column memory.
        #[arg(long)]
        exact_visible: bool,
    },
    /// Replace content in a named component
    Patch {
        /// Path to the document
        file: PathBuf,
        /// Component name (e.g. "status", "log")
        component: String,
        /// Patch mode override. Defaults to replace.
        #[arg(long, value_enum, default_value = "replace")]
        mode: PatchMode,
        /// Replacement content (reads from stdin if omitted)
        content: Option<String>,
    },
    /// Watch session files for changes and auto-submit
    Watch {
        /// Stop the running watch daemon
        #[arg(long)]
        stop: bool,
        /// Show watch daemon status
        #[arg(long)]
        status: bool,
        /// Debounce delay in milliseconds
        #[arg(long, default_value = "500")]
        debounce: u64,
        /// Maximum agent-triggered cycles per file
        #[arg(long, default_value = "3")]
        max_cycles: u32,
    },
    /// Display markdown outline with section structure and token counts
    Outline {
        /// Path to the markdown document
        file: PathBuf,
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
    /// Plan the completion work-graph (auto-dag) for a document's backlog +
    /// review items: classify each (implementable / live-verify / IPC-capture /
    /// blocked / done) and emit a Mermaid diagram + nested list
    /// (`#auto-dag-first-class`, the agent-doc analogue of `/goal`)
    AutoDag {
        /// Path to the markdown document
        file: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate the durable session registry against live tmux panes, remove stale entries
    Resync {
        /// Limit checks/fixes to a single session document
        file: Option<PathBuf>,
        /// Actually kill wrong-session panes and deregister stale entries (without this flag, dry-run only)
        #[arg(long)]
        fix: bool,
        /// Relocate WrongSession panes to this tmux session via join-pane instead of killing them.
        /// Requires --fix. Example: --session 10
        #[arg(long)]
        session: Option<String>,
    },
    /// Fix stale routing/session issues globally or for one session document (`resync --fix` alias)
    Fix {
        /// Limit fixes to a single session document
        file: Option<PathBuf>,
        /// Relocate WrongSession panes to this tmux session via join-pane instead of killing them.
        /// Example: --session 10
        #[arg(long)]
        session: Option<String>,
    },
    /// Manage the Claude Code skill definition
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage editor plugins
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Append an assistant response to a session document (reads from stdin)
    Write {
        #[command(flatten)]
        args: WriteArgs,
        /// Commit the document to git after a successful write (skipped silently if not in a git repo)
        #[arg(long)]
        commit: bool,
    },
    /// Append an assistant response and require the cycle to reach a committed state
    Finalize {
        #[command(flatten)]
        args: WriteArgs,
    },
    /// Stream agent output to document in real-time (CRDT merge)
    Stream {
        /// Path to the session document
        file: PathBuf,
        /// Write-back interval in milliseconds
        #[arg(long, default_value = "200")]
        interval: u64,
        /// Agent backend to use
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Skip git commit after stream completes
        #[arg(long)]
        no_git: bool,
        /// Override the tagpath agent-doc lint dialect mode for this stream run.
        /// Values: `off | warn | strict`. Precedence: CLI > frontmatter
        /// `agent_doc_lint_dialect` > workspace `.agent-doc/config.toml`
        /// `[lint] dialect` > default (`warn`).
        #[arg(long = "lint", value_name = "MODE")]
        lint: Option<String>,
    },
    /// Show template structure of a document (components, modes, content)
    TemplateInfo {
        /// Path to the document
        file: PathBuf,
    },
    /// Repair an orphaned response or stale document cycle (`recover` alias kept)
    #[command(name = "repair", visible_alias = "recover")]
    Repair {
        /// Path to the session document
        file: PathBuf,
        /// Apply the unambiguously-safe closeout recovery for the classified
        /// drift state in one step (abandon empty preflight cycle / commit
        /// boundary-artifact drift), instead of orphaned-response repair.
        #[arg(long)]
        apply_recovery: bool,
    },
    /// Admit a live agent request by opening a lightweight response-cycle checkpoint
    Admit {
        /// Path to the session document
        file: PathBuf,
    },
    /// Run all pre-agent steps (repair, commit, claims, diff, document HEAD) and output JSON
    Preflight {
        /// Path to the session document
        file: PathBuf,
        /// Pure inspection probe: emit the same JSON but do NOT open a
        /// `preflight_started` cycle, so a diagnostic preflight never leaves an
        /// open cycle that later wedges `session-check`
        /// (`#preflight-probe-side-effect-free`).
        #[arg(long)]
        probe: bool,
    },
    /// Diagnose workflow invariant status for a session document
    #[command(visible_alias = "diagnose")]
    Doctor {
        /// Path to the session document
        file: PathBuf,
        /// Optional JSON captured from `agent-doc preflight <FILE> --probe`
        #[arg(long)]
        preflight_json: Option<PathBuf>,
        /// Optional JSON captured from an external session-check wrapper
        #[arg(long)]
        session_check_json: Option<PathBuf>,
        /// Number of recent ops.log lines to scan
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Plan and optionally apply catalog-safe workflow invariant repairs
    Autofix {
        /// Path to the session document
        file: PathBuf,
        /// Optional JSON captured from `agent-doc preflight <FILE> --probe`
        #[arg(long)]
        preflight_json: Option<PathBuf>,
        /// Optional JSON captured from an external session-check wrapper
        #[arg(long)]
        session_check_json: Option<PathBuf>,
        /// Number of recent ops.log lines to scan
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// Execute the whitelisted safe repair commands after planning
        #[arg(long)]
        apply: bool,
        /// Do not write workflow_autofix proof markers to the proof ledger
        #[arg(long)]
        dry_run: bool,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Check end-of-cycle write invariant — nonzero exit if the cycle is open or a likely direct response patchback bypassed agent-doc
    SessionCheck {
        /// Path to the session document
        file: PathBuf,
        /// Strict Codex final gate: exit nonzero when a clean document still owes an active `agent:queue auto` continuation
        #[arg(long)]
        codex_final_gate: bool,
    },
    /// Run agent-doc's stdio MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Serve a localhost HTTP markdown editor for one document or a project session list
    Serve {
        /// Path to a session document or project directory. Defaults to the nearest project root from CWD.
        file: Option<PathBuf>,
        /// Bind host (default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,
        /// Bind port (default: 7333)
        #[arg(long)]
        port: Option<u16>,
        /// Edit bearer token. Defaults to a generated token when auth is required.
        #[arg(long, value_name = "TOKEN")]
        auth_token: Option<String>,
        /// Read-only bearer token for viewer access.
        #[arg(long, value_name = "TOKEN")]
        read_only_token: Option<String>,
        /// TLS certificate PEM path. Requires --tls-key.
        #[arg(long, value_name = "PEM")]
        tls_cert: Option<PathBuf>,
        /// TLS private key PEM path. Requires --tls-cert.
        #[arg(long, value_name = "PEM")]
        tls_key: Option<PathBuf>,
    },
    /// Describe an image file using a vision-capable AI model
    DescribeImage {
        /// Path to the image file
        image: PathBuf,
        /// Vision provider (openai, anthropic)
        #[arg(long)]
        provider: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// API key (defaults to AGENT_DOC_VISION_API_KEY or provider-specific env var)
        #[arg(long)]
        api_key: Option<String>,
        /// Custom prompt for the vision model
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Print document content to stdout (full file or a single named component).
    Read {
        /// Path to the session document
        file: PathBuf,
        /// Name of a specific component to extract (e.g. "exchange", "backlog").
        /// If omitted, the full file is printed.
        #[arg(long)]
        component: Option<String>,
    },
    /// List live and archived response sections for targeted retrieval
    ResponseToc {
        /// Path to the session document
        file: PathBuf,
        /// Exact backlog / prompt id to match (with or without leading #)
        #[arg(long = "id")]
        backlog_id: Option<String>,
        /// Free-text query over response headings and bodies
        #[arg(long)]
        query: Option<String>,
        /// Max archive entries to include
        #[arg(long, default_value_t = 6)]
        limit: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Load an exact live or archived response section, optionally with neighbors
    ResponseFetch {
        /// Path to the session document
        file: PathBuf,
        /// Locator from `agent-doc response-toc`
        #[arg(long)]
        locator: String,
        /// Include this many earlier adjacent sections
        #[arg(long, default_value_t = 0)]
        before: usize,
        /// Include this many later adjacent sections
        #[arg(long, default_value_t = 0)]
        after: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Build or refresh the sqlite archive index for compacted turns
    ArchiveIndex {
        /// Path to a session document in the target project
        file: PathBuf,
        /// Drop and rebuild the derived index from archive markdown
        #[arg(long)]
        rebuild: bool,
    },
    /// Search the sqlite archive index for compacted turns
    ArchiveSearch {
        /// Path to a session document in the target project
        file: PathBuf,
        /// Free-text query over indexed archive chunks
        #[arg(long)]
        query: Option<String>,
        /// Exact backlog / prompt id to match (with or without leading #)
        #[arg(long = "id")]
        backlog_id: Option<String>,
        /// Restrict to a specific archived session id
        #[arg(long)]
        session: Option<String>,
        /// Max results to print
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Rebuild the derived index before searching
        #[arg(long)]
        rebuild: bool,
    },
    /// Index/search agent-doc session memory through the shared tsift-memory crate
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Archive old exchanges / compact component content
    Compact {
        /// Path to the session document
        file: PathBuf,
        /// Number of recent exchanges/topics to keep.
        /// Append mode default: 2. Template mode: omit to archive all (full compact),
        /// or pass N to keep last N `### Re:` topic sections (partial compact).
        #[arg(long)]
        keep: Option<usize>,
        /// Component to compact (template/stream mode, default: exchange)
        #[arg(long)]
        component: Option<String>,
        /// Summary message to replace content with
        #[arg(long)]
        message: Option<String>,
        /// Git tag name for pre-compact checkpoint (default: auto-generated
        /// agent-doc/<doc-name>/pre-compact-N). Use "skip" to disable tagging.
        #[arg(long)]
        tag: Option<String>,
        /// Close out compaction via the agent-doc commit path and verify VCS refresh when available
        #[arg(long)]
        commit: bool,
        /// Allow compact to bypass editor IPC when no listener is attached
        #[arg(long)]
        force_disk: bool,
    },
    /// Convert a document between append and template modes
    Convert {
        /// Path to the session document
        file: PathBuf,
        /// Target mode (deprecated positional — use --agent-doc-format / --agent-doc-write instead)
        #[arg(value_enum)]
        mode: Option<AgentDocMode>,
        /// Set document format (append | template)
        #[arg(long, value_enum)]
        agent_doc_format: Option<frontmatter::AgentDocFormat>,
        /// Set write strategy (merge | crdt)
        #[arg(long, value_enum)]
        agent_doc_write: Option<frontmatter::AgentDocWrite>,
    },
    /// Get or set the document mode (format + write strategy)
    Mode {
        /// Path to the session document
        file: PathBuf,
        /// Set mode: append or template (deprecated — use --format / --write)
        #[arg(long)]
        set: Option<String>,
    },
    /// Print and clear the claims log (.agent-doc/claims.log)
    Claims,
    /// Fan-out: decompose task into parallel worktree-isolated subagents
    Parallel {
        /// Path to the session document
        file: PathBuf,
        /// Explicit subtask descriptions (repeatable)
        #[arg(long = "task")]
        tasks_explicit: Vec<String>,
        /// Model override for subtask agents
        #[arg(long)]
        model: Option<String>,
        /// Skip git commits in worktrees
        #[arg(long)]
        no_git: bool,
        /// Run without worktrees (read-only tasks, shared CWD)
        #[arg(long)]
        no_worktree: bool,
        /// Per-task timeout in seconds
        #[arg(long, default_value = "600")]
        timeout: u64,
        /// Show plan without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Orchestrate sequential, parallel, or dependency-aware task batches against one document
    Orchestrate {
        /// Path to the session document
        file: PathBuf,
        /// Orchestration mode
        #[arg(long, value_enum, default_value_t = orchestrate::OrchestrateMode::Sequential)]
        mode: orchestrate::OrchestrateMode,
        /// Explicit task descriptions (repeatable)
        #[arg(long = "task")]
        tasks_explicit: Vec<String>,
        /// Read task descriptions from a markdown/text file
        #[arg(long = "from-file")]
        from_file: Option<PathBuf>,
        /// Extract the latest task list/code block from the document exchange
        #[arg(long = "from-exchange")]
        from_exchange: bool,
        /// Extract all active prompts and preset directives from agent:queue
        #[arg(long = "from-queue")]
        from_queue: bool,
        /// Resume a persisted auto-DAG schedule id from .agent-doc/schedules
        #[arg(long = "resume-schedule")]
        resume_schedule: Option<String>,
        /// Agent backend override for sequential or DAG execution
        #[arg(long)]
        agent: Option<String>,
        /// Model override
        #[arg(long)]
        model: Option<String>,
        /// Skip git commits in worktrees (parallel mode only; sequential/DAG require finalize)
        #[arg(long)]
        no_git: bool,
        /// Run without worktrees (parallel mode only)
        #[arg(long)]
        no_worktree: bool,
        /// Per-task timeout in seconds (parallel mode only)
        #[arg(long, default_value = "600")]
        timeout: u64,
        /// Show the resolved plan without executing
        #[arg(long)]
        dry_run: bool,
        /// Show each task's fully expanded prompt (with presets applied) without executing
        #[arg(long)]
        plan: bool,
    },
    /// Re-establish claims after context compaction (SessionStart hook)
    Autoclaim,
    /// Surface turn-in-progress status on the agent's own tmux pane border.
    /// Hook-driven: `UserPromptSubmit` runs `active`, `Stop` runs `idle`
    /// (#claude-busy-status-during-active-turn). Best-effort; no-op outside tmux.
    #[command(name = "turn-status")]
    TurnStatus {
        #[command(subcommand)]
        action: TurnStatusAction,
    },
    /// Derive a structured post-preflight planning/dispatch record for a document
    Plan {
        /// Path to the session document
        file: PathBuf,
    },
    /// Manage lower-agent job packets for a session document
    Jobs {
        #[command(subcommand)]
        action: JobsAction,
    },
    /// Check for updates and upgrade to the latest version.
    Upgrade,
    /// Generate content-source annotation sidecar for a document
    Annotate {
        /// Path to the session document
        file: PathBuf,
        /// Force regeneration even if cache is valid
        #[arg(long)]
        force: bool,
        /// Use git blame for full history attribution
        #[arg(long)]
        history: bool,
    },
    /// Undo the last agent response (restore pre-response state)
    Undo {
        /// Path to the session document
        file: PathBuf,
    },
    /// Extract the last exchange entry from source to target document
    Extract {
        /// Source document
        source: PathBuf,
        /// Target document
        target: PathBuf,
        /// Component name to extract from (default: exchange)
        #[arg(long)]
        component: Option<String>,
    },
    /// Transfer entire component content from source to target document
    Transfer {
        /// Source document
        source: PathBuf,
        /// Target document
        target: PathBuf,
        /// Component name to transfer
        component: String,
        /// Bypass pane ownership check on target (for cross-session transfers)
        #[arg(long)]
        bypass_claim: bool,
        /// Transfer only specific backlog/pending or icebox items by ID (comma-separated, e.g., "#id1,#id2")
        #[arg(long)]
        items: Option<String>,
        /// Insert a referral pointer instead of moving content (target reads source on demand)
        #[arg(long)]
        referral: bool,
    },
    /// Migrate session state after a document file rename/move
    Rename {
        /// Original document path (may no longer exist on disk)
        old_path: PathBuf,
        /// New document path (must exist)
        new_path: PathBuf,
    },
    /// Migrate documents: rename deprecated components and strip deprecated attributes
    Migrate {
        /// Session documents to migrate
        files: Vec<PathBuf>,
        /// Scan project root for all documents with deprecated markers
        #[arg(long)]
        all: bool,
        /// Preview changes without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Open an external terminal with tmux attached to the session
    Terminal {
        /// Path to the session document
        file: PathBuf,
        /// Tmux session name (overrides frontmatter tmux_session)
        #[arg(long)]
        session: Option<String>,
    },
    /// Insert a boundary marker at the end of a component for response ordering
    Boundary {
        /// Path to the session document
        file: PathBuf,
        /// Component name (default: exchange)
        #[arg(long)]
        component: Option<String>,
    },
    /// Append a blockquote notification to a document's exchange component
    Notify {
        /// Path to the document
        file: PathBuf,
        /// Notification message (optional when --backlog-add is used)
        message: Option<String>,
        /// Source document or session
        #[arg(long)]
        source: Option<String>,
        /// Sections affected (for re-evaluation directive)
        #[arg(long)]
        affects: Option<String>,
        /// Skip git commit after notification
        #[arg(long)]
        no_commit: bool,
        /// Add a backlog item to the target document (repeatable). Auto-creates agent:backlog if absent.
        #[arg(long = "backlog-add", alias = "pending-add")]
        pending_add: Vec<String>,
        /// Add a gated backlog item (repeatable). Like --backlog-add but assigns [/] instead of [ ].
        #[arg(long = "backlog-add-gated", alias = "pending-add-gated")]
        pending_add_gated: Vec<String>,
        /// Do not auto-create agent:backlog component if absent
        #[arg(long = "no-create-backlog", alias = "no-create-pending")]
        no_create_pending: bool,
    },
    /// Print the path to the shared library (libagent_doc.so/dylib/dll)
    LibPath,
    /// Remove stale versioned shared libraries not in use
    GcLibs {
        /// Target directory (default: directory containing agent-doc binary)
        #[arg(long)]
        target_dir: Option<String>,
    },
    /// Install versioned shared library with atomic symlink swap
    LibInstall {
        /// Source .so path (default: target/<profile>/libagent_doc.so)
        #[arg(long)]
        source: Option<String>,
        /// Cargo profile to read when --source is omitted
        #[arg(long, default_value = "release")]
        profile: String,
        /// Target directory (default: directory containing agent-doc binary)
        #[arg(long)]
        target_dir: Option<String>,
    },
    /// Show the reliable-sync shadow liveness plane vs the sidecar open-set (dual-run
    /// `[operator-verify]` parity read — sidecar-retirement Phase 3C)
    #[command(name = "reliable-sync-status")]
    ReliableSyncStatus {
        /// Emit the raw JSON response instead of the human parity table
        #[arg(long)]
        json: bool,
        /// Project root (default: discovered from the current directory)
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Install this committed checkout from an isolated sibling git worktree
    #[command(name = "self-install")]
    SelfInstall {
        /// Source repo to install from (default: current git root)
        #[arg(long)]
        source_root: Option<PathBuf>,
        /// Target directory for the shared library (default: binary directory)
        #[arg(long)]
        target_dir: Option<PathBuf>,
        /// Cargo profile to build in the isolated worktree
        #[arg(long, default_value = "release")]
        profile: String,
        /// Leave the temporary worktree on disk for debugging
        #[arg(long)]
        keep_worktree: bool,
    },
    /// List all available commands as JSON (for editor plugin autocomplete)
    #[command(name = "commands")]
    #[allow(clippy::enum_variant_names)]
    ListCommands,
    /// Hook system for cross-session coordination
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
    /// Clean up document: compact, prune pending, apply callback results
    Cleanup {
        /// Path to the session document
        file: PathBuf,
        /// Timeout waiting for Claude session response (seconds)
        #[arg(long, default_value_t = 120)]
        timeout: u64,
        /// Polling interval for callback response (milliseconds)
        #[arg(long, default_value_t = 1000)]
        poll_interval: u64,
        /// Model for fallback agent (default: sonnet)
        #[arg(long, default_value = "sonnet")]
        fallback_model: String,
    },
    /// Manage the agent:backlog component (`pending` is a deprecated alias)
    #[command(name = "backlog", alias = "pending")]
    Backlog {
        /// Path to the session document
        file: PathBuf,
        /// Bypass editor convergence for explicit recovery/headless writes.
        #[arg(long)]
        force_disk: bool,
        #[command(subcommand)]
        action: PendingAction,
    },
    /// Manage the agent:icebox component
    Icebox {
        /// Path to the session document
        file: PathBuf,
        /// Bypass editor convergence for explicit recovery/headless writes.
        #[arg(long)]
        force_disk: bool,
        #[command(subcommand)]
        action: PendingAction,
    },
    /// Operate on the review component of a session document
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Manage the agent:queue component
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Resolve typed gates across tracked documents.
    /// Scans documents under the project root for [/<type>] items and flips to [x].
    /// Designed for hook integration: `agent-doc resolve-gate release`
    #[command(name = "resolve-gate")]
    ResolveGateCmd {
        /// Gate type to resolve (e.g., "release", "deploy")
        gate_type: String,
        /// Restrict scan to documents under this directory (defaults to project root)
        #[arg(long)]
        scope: Option<PathBuf>,
    },
    /// Manage bidirectional IPC callbacks
    Callback {
        #[command(subcommand)]
        action: CallbackAction,
    },
    /// Show or change the configured tmux session
    Session {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },
    /// Manage the project-local controller shell
    Controller {
        #[command(subcommand)]
        action: ControllerAction,
    },
    /// Fleet-wide actor admin control plane (`#ipc-admin-api`)
    Admin {
        #[command(subcommand)]
        action: AdminAction,
    },
}

#[derive(Subcommand)]
enum AdminAction {
    /// Inspect one actor and its controller receipts
    Inspect {
        /// Document to inspect
        document: Option<PathBuf>,
        /// Inspect by session id instead of document
        #[arg(long)]
        session: Option<String>,
        /// Inspect by tmux pane id instead of document
        #[arg(long)]
        pane: Option<String>,
        /// Project root to inspect (defaults to document root or nearest project)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Enumerate every actor in the project fleet (one row per document)
    List {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit JSON instead of a human-readable table
        #[arg(long)]
        json: bool,
    },
    /// Derived diagnostics: cross-document pane contention + orphaned bindings
    Detect {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Live read-only fleet dashboard over `admin list` / `admin detect`
    Dashboard {
        /// Project root to inspect (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Emit a single model snapshot as JSON and exit
        #[arg(long)]
        json: bool,
        /// Render one frame and exit instead of polling
        #[arg(long)]
        once: bool,
        /// Poll interval in milliseconds (default ~1s)
        #[arg(long, default_value_t = dashboard_cmd::DEFAULT_INTERVAL_MS)]
        interval: u64,
    },
    /// Pause, resume, or drain queue work through the controller
    Queue {
        #[command(subcommand)]
        action: AdminQueueAction,
    },
    /// Reap a stale actor after checking its observed generation
    Reap {
        /// Document to reap
        document: Option<PathBuf>,
        /// Reap every non-closed actor whose pane is no longer alive
        #[arg(long)]
        all_stale: bool,
        /// Reap by session id instead of document
        #[arg(long)]
        session: Option<String>,
        /// Reap by tmux pane id instead of document
        #[arg(long)]
        pane: Option<String>,
        /// Project root to inspect (defaults to document root or nearest project)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Generation observed by the operator before mutating actor state
        #[arg(long)]
        observed_generation: Option<u64>,
        /// Operator reason recorded in the durable receipt
        #[arg(long, default_value = "manual reap")]
        reason: String,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Terminate any controller wedged in `Preparing`/`Promoted` past the
    /// stuck-handoff threshold (#kqr6 / #sjwm / #stuckhandoff). Replaces the
    /// manual `pkill -f 'controller serve ... --handoff-state preparing'`.
    #[command(name = "reap-stale-controllers")]
    ReapStaleControllers {
        /// Project root to sweep (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Report what would be terminated without killing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Recycle running controllers onto the freshly-installed binary at their next
    /// idle boundary (no dispatch in flight). Run after `cargo install` so a
    /// long-running controller stops serving the prior binary (`#ctlrecycle`).
    Recycle {
        /// Optional document path or project root to recycle (defaults to the nearest project from CWD)
        #[arg(value_name = "FILE_OR_PROJECT_ROOT")]
        target: Option<PathBuf>,
        /// Project root to recycle (defaults to the nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Recycle the controller in every project root with a running controller
        #[arg(long)]
        all_projects: bool,
        /// Force the recycle to take effect promptly: override the cycle-open /
        /// in-flight-dispatch deferral (may interrupt an in-flight turn — that is
        /// the point of `--force`), and, when no live controller answers, escalate
        /// to a kill+cold-start (`session restart-supervisor`) instead of a no-op.
        /// Composes with `--all-projects`.
        #[arg(long)]
        force: bool,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Announce a freshly-installed `libagent_doc` cdylib to editor plugins by
    /// writing the global reload-broadcast file (`#cdylib-reload-broadcast`). This
    /// is the "recycle via API" counterpart to `admin recycle`: `lib-install`
    /// broadcasts automatically, and this command lets an operator re-announce the
    /// current cdylib on demand. JetBrains and VS Code plugins watch the broadcast
    /// and force their existing native-reload path immediately instead of waiting
    /// for the next lazy FFI call.
    #[command(name = "reload-lib")]
    ReloadLib {
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Kill the `start --route-owned` supervisor for a document: request a graceful
    /// idle-gated self-kill, then force-kill the verified pid after a grace window
    /// if it stays alive (`#supkill`). Refuses to kill the caller's own ancestor —
    /// run it from a different pane (or let the project controller drive it) when the
    /// target is the supervisor of the current session.
    #[command(name = "kill-supervisor")]
    KillSupervisor {
        /// Document whose route-owned supervisor should be killed
        document: PathBuf,
        /// Seconds to wait for the graceful self-kill before force-killing the pid
        /// (default: AGENT_DOC_SUPERVISOR_SELFKILL_GRACE_SECS or 10)
        #[arg(long)]
        grace_secs: Option<u64>,
        /// Report the target supervisor pid without signalling anything
        #[arg(long)]
        dry_run: bool,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Handoff a document actor to another pane after generation verification
    Handoff {
        /// Document to hand off
        document: PathBuf,
        /// Destination tmux pane id
        #[arg(long)]
        to_pane: String,
        /// Project root to inspect (defaults to document root or nearest project)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Generation observed by the operator before mutating actor state
        #[arg(long)]
        observed_generation: u64,
        /// Operator reason recorded in the durable receipt
        #[arg(long)]
        reason: String,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Rebuild compatibility projections from controller SQLite state
    RepairProjection {
        /// Optional document scope for sessions projection repair
        document: Option<PathBuf>,
        /// Project root to inspect (defaults to document root or nearest project)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Projection to repair: all, actors, sessions, or layout
        #[arg(long, default_value = "all")]
        projection: String,
        /// Optional generation guard when repairing a document-scoped projection
        #[arg(long)]
        observed_generation: Option<u64>,
        /// Operator reason recorded in the durable receipt
        #[arg(long)]
        reason: Option<String>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AdminQueueAction {
    /// Pause queue dispatch for a document or project
    Pause {
        /// Document queue to pause. Omit with --project-root for project scope.
        document: Option<PathBuf>,
        /// Project root for project-scoped control, or root override for document scope
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Generation observed by the operator before document-scoped mutation
        #[arg(long)]
        observed_generation: Option<u64>,
        /// Operator reason recorded in the durable receipt
        #[arg(long)]
        reason: String,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Resume queue dispatch for a document or project
    Resume {
        /// Document queue to resume. Omit with --project-root for project scope.
        document: Option<PathBuf>,
        /// Project root for project-scoped control, or root override for document scope
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Generation observed by the operator before document-scoped mutation
        #[arg(long)]
        observed_generation: Option<u64>,
        /// Operator reason recorded in the durable receipt
        #[arg(long)]
        reason: Option<String>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Mark a queue as draining; busy actors block new dispatch until ready
    Drain {
        /// Document queue to drain
        document: PathBuf,
        /// Project root override
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Generation observed by the operator before document-scoped mutation
        #[arg(long)]
        observed_generation: Option<u64>,
        /// Optional queue item id to drain through
        #[arg(long)]
        until_id: Option<String>,
        /// Operator reason recorded in the durable receipt
        #[arg(long)]
        reason: Option<String>,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum OpsAction {
    /// Summarize high-signal ops.log events by document/session
    Summary {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Number of trailing ops.log lines to scan; 0 scans the full file
        #[arg(long, default_value_t = ops_report::default_summary_limit())]
        limit: usize,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
    /// Gather cycle/patch diagnostics from agent-doc logs and sidecars
    Diagnose {
        /// Project root to inspect (defaults to --file root or nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Session document path used for project-root and file correlation
        #[arg(long)]
        file: Option<PathBuf>,
        /// Cycle id to correlate, e.g. cycle-1779845677327
        #[arg(long)]
        cycle_id: Option<String>,
        /// Editor IPC patch id to correlate
        #[arg(long)]
        patch_id: Option<String>,
        /// agent_doc_session / harness session id to correlate
        #[arg(long)]
        session_id: Option<String>,
        /// Number of trailing lines to scan in text logs; 0 scans full files
        #[arg(long, default_value_t = ops_report::default_summary_limit())]
        limit: usize,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Index backlog/review/done/icebox/exchange surfaces into .tsift/memory.db
    Index {
        /// Path to the session document
        file: PathBuf,
        /// Memory DB path (default: <project>/.tsift/memory.db)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Emit JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
    },
    /// Search indexed session memory plus current document tracked work
    Search {
        /// Path to the session document
        file: PathBuf,
        /// Free-text query
        #[arg(long)]
        query: String,
        /// Memory DB path (default: <project>/.tsift/memory.db)
        #[arg(long)]
        db: Option<PathBuf>,
        /// Max results to print
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit JSON instead of a human-readable summary
        #[arg(long)]
        json: bool,
        /// Index the current document before searching
        #[arg(long)]
        rebuild: bool,
    },
}

#[derive(Subcommand)]
enum JobsAction {
    /// Generate job packets from the current planning record
    Create {
        /// Path to the session document
        file: PathBuf,
        /// Also create an operation document for retained audit/review
        #[arg(long)]
        operation_doc: bool,
        /// Preserve generated packets after success; records intent in index metadata
        #[arg(long)]
        audit: bool,
        /// Token/byte budget used for generated tsift context sidecars
        #[arg(long, default_value_t = 6000)]
        budget: usize,
    },
    /// List generated job packets for a document
    List {
        /// Path to the session document
        file: PathBuf,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
    /// Show job packet completion status
    Status {
        /// Path to the session document
        file: PathBuf,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
    /// Collect worker result sidecars or embedded Worker Result JSON blocks
    Collect {
        /// Path to the session document
        file: PathBuf,
        /// Specific cycle id to collect; defaults to latest
        #[arg(long)]
        cycle: Option<String>,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ControllerAction {
    /// Show project controller status as JSON
    Status {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Lazily launch the controller before reading status
        #[arg(long)]
        ensure: bool,
    },
    /// Run the controller server loop
    #[command(hide = true)]
    Serve {
        /// Project root to serve (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Bootstrap launch mode to persist in controller state
        #[arg(long, default_value = "managed")]
        launch_mode: String,
        /// Private socket used while promoting a replacement controller
        #[arg(long, hide = true)]
        listen_socket: Option<PathBuf>,
        /// Controller generation to persist for a replacement controller
        #[arg(long, hide = true)]
        controller_generation: Option<u64>,
        /// Previous authoritative controller PID during handoff
        #[arg(long, hide = true)]
        previous_controller_pid: Option<u32>,
        /// Handoff state to persist at startup
        #[arg(long, hide = true, default_value = "stable")]
        handoff_state: String,
    },
    /// Stop the project controller if it is running
    Shutdown {
        /// Project root to inspect (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
    },
    /// Restart/recycle the project controller (out-of-band; works on a
    /// spin-wedged controller that no longer services RPCs). Checkpoints
    /// route-owned state first; the lazy launcher relaunches a fresh controller.
    Restart {
        /// Project root to restart (defaults to nearest project from CWD)
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Skip the graceful `shutdown` RPC and reap the controller PID(s)
        /// directly — use when the controller is spin-wedged / RPC-unreachable.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Fire a hook event
    Fire {
        /// Event name (e.g., post_write, post_commit, claim)
        event: String,
        /// Document file path
        file: String,
        /// Session ID (auto-read from frontmatter if omitted)
        #[arg(long)]
        session_id: Option<String>,
        /// JSON data to attach to the event
        #[arg(long)]
        data: Option<String>,
    },
    /// Poll for hook events
    Poll {
        /// Event name to poll
        event: String,
        /// Only return events newer than this timestamp (unix seconds)
        #[arg(long, default_value_t = 0)]
        since: u64,
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Start hook socket listener
    Listen {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Clean up expired events
    Gc {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Check for pending callback requests (called by PostToolUse hooks)
    CheckCallbacks {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
    /// Track the active `agent-doc` document for a Codex session (stdin JSON hook payload)
    CodexUserPromptSubmit,
    /// Enforce the Codex end-of-turn `session-check` guard (stdin JSON hook payload)
    CodexStop,
}

#[derive(Subcommand)]
enum PendingAction {
    /// Add an item to the selected tracked-work component (front of list; assigns stable hash id + `[ ]`)
    Add {
        /// The item description. Prefix with canonical `id=<custom> ` to preserve a custom id.
        /// Leading `[#custom] ` is also accepted as compatibility input.
        item: String,
    },
    /// Add a gated item to the selected tracked-work component (front of list; assigns stable hash id + `[/]`)
    AddGated {
        /// The item description. Prefix with canonical `id=<custom> ` to preserve a custom id.
        /// Leading `[#custom] ` is also accepted as compatibility input.
        item: String,
    },
    /// Remove an item from the selected tracked-work component
    Remove {
        /// Content to match
        target: String,
        /// Treat target as a substring match
        #[arg(long, short)]
        contains: bool,
    },
    /// Reap `[x]` items and print removed ids
    Reap,
    /// Run lazy backfill — assign missing hash ids and checkboxes
    Backfill,
    /// Mark an item done by id
    Done {
        /// Hash id (without the `#` prefix)
        id: String,
    },
    /// Rewrite an item's text, preserving its hash id
    Edit {
        /// Hash id (without the `#` prefix)
        id: String,
        /// New item text
        text: String,
    },
    /// Clear all items
    Clear,
    /// Reorder items by hash id (comma-separated)
    Reorder {
        /// Comma-separated list of hash ids
        ids: String,
    },
    /// List current items
    List,
    /// Resolve all items matching a typed gate (e.g., [/release] → [x])
    ResolveGate {
        /// Gate type to resolve (e.g., "release", "deploy")
        gate_type: String,
    },
    /// Set a typed gate on a gated item ([/] → [/release])
    SetGateType {
        /// Hash id (without the `#` prefix)
        id: String,
        /// Gate type (e.g., "release", "deploy")
        gate_type: String,
    },
    /// Set a typed proof/disproof verify predicate on a gated item so the gate
    /// auto-resolves from ops.log markers (`#optverify`).
    SetVerify {
        /// Hash id (without the `#` prefix)
        id: String,
        /// Predicate spec, e.g.
        /// `verify=ops_log:<marker>;disproof=ops_log:<text>`
        spec: String,
    },
}

#[derive(Subcommand)]
enum QueueAction {
    /// Reconstruct historical queue heads from snapshots, sidecars, and git history
    #[command(name = "recover-lost")]
    RecoverLost {
        /// Path to the session document
        file: PathBuf,
        /// Emit JSON instead of a human-readable report
        #[arg(long)]
        json: bool,
        /// Maximum git versions to scan for historical queue heads
        #[arg(long, default_value_t = 50)]
        max_git_versions: usize,
        /// Write an operator-reviewed restoration patch (JSON) for git-history-only
        /// candidates to this path. Restorable prompts are separated from
        /// non-restorable foreign-owned ones; the session document is never mutated.
        #[arg(long, value_name = "RESTORE_PATCH_PATH")]
        restore_patch: Option<PathBuf>,
    },
    /// One-shot sync from backlog items with `queue` attribute into agent:queue
    Sync {
        /// Path to the session document
        file: PathBuf,
    },
    /// Explicitly strike the leading free-text queue head(s) already answered by
    /// this turn's response(s). Use when a single cycle answered multiple
    /// free-text heads (the strike heuristic only consumes one head per
    /// finalize), to drain the answered stragglers instead of re-serving them.
    /// Scoped to free-text heads; id-backed heads must be reaped via `--done`
    /// / `--backlog-gate`, unless `--ack-id` is explicitly acknowledging a
    /// correction head while leaving its open backlog item in place.
    Consume {
        /// Path to the session document
        file: PathBuf,
        /// Number of leading free-text heads to strike (default 1)
        #[arg(long, default_value_t = 1)]
        count: usize,
        /// Bypass editor convergence and write directly to disk. Only valid for
        /// free-text consume; id-backed heads still use their guarded commands.
        #[arg(long, conflicts_with = "id", conflicts_with = "ack_id")]
        force_disk: bool,
        /// Escape hatch (#orphanqhead): strike the orphaned id-backed head
        /// `[#id]` whose backing backlog item was already reaped (`--done`
        /// reports "already resolved") or is gone, leaving the phantom head
        /// undrainable by the normal `--done` / free-text consume paths. Refuses
        /// when the id still names an OPEN backlog item (use `--done` instead).
        #[arg(long, conflicts_with = "count")]
        id: Option<String>,
        /// Explicit acknowledgement (#freshqueueauth): strike an exact id-backed
        /// correction head while preserving the still-open backlog item. Do not
        /// use for runnable work; leave live `do [#id]` heads queued until done,
        /// gated, or intentionally acknowledged as a correction.
        #[arg(long, conflicts_with = "count", conflicts_with = "id")]
        ack_id: Option<String>,
    },
    /// Strike every non-drainable NOISE queue head at any position (#goqstall2):
    /// pasted console output, agent-response fragments, and bare observations that
    /// carry no `#id`, question mark, or directive verb, so they can never drain
    /// and only churn the go-mode loop (surfaced as `queue_stale_noise_lines=N` by
    /// session-check for compatibility). Unlike `consume`, clears predicate-proven
    /// noise interleaved behind id-backed `do [#id]` heads; id-backed directives
    /// and genuinely drainable free-text heads are preserved.
    #[command(name = "prune-noise")]
    PruneNoise {
        /// Path to the session document
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    /// Add a backlog follow-up task for each gated review item so it is driven
    /// back out of review (ungate → done). Idempotent: review ids already
    /// covered by an existing ungate task are skipped.
    #[command(name = "ungate-tasks")]
    UngateTasks {
        /// Path to the session document
        file: PathBuf,
    },
    /// List gated review items in a token-efficient form (id, gate-type, tags,
    /// NEXT-step annotation) so a long review list can be triaged at a glance.
    List {
        /// Path to the session document
        file: PathBuf,
        /// Only items with this typed gate (e.g. `release`)
        #[arg(long)]
        gate_type: Option<String>,
        /// Only items carrying this hashtag (with or without leading `#`)
        #[arg(long)]
        tag: Option<String>,
        /// Only items that have a `NEXT:` annotation (actionable)
        #[arg(long, conflicts_with = "no_next")]
        has_next: bool,
        /// Only items WITHOUT a `NEXT:` annotation (the stale set to triage)
        #[arg(long, conflicts_with = "has_next")]
        no_next: bool,
        /// Emit JSON instead of the compact text form
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CallbackAction {
    /// Create a callback request for a document
    Request {
        /// Path to the session document
        file: PathBuf,
        /// Operations requested (comma-separated: compact,prune-pending,summary)
        operations: String,
        /// Optional additional context
        #[arg(long)]
        context: Option<String>,
        /// TTL in seconds before the request expires
        #[arg(long, default_value_t = 300)]
        ttl: u64,
    },
    /// Read the pending callback request for a document
    Read {
        /// Path to the session document
        file: PathBuf,
    },
    /// Write a callback response for a document
    Respond {
        /// Path to the session document
        file: PathBuf,
        /// The request_id to respond to (must match the pending request)
        #[arg(long)]
        request_id: String,
        /// Response status: "success" or "error"
        #[arg(long, default_value = "success")]
        status: String,
        /// Summary text
        #[arg(long)]
        summary: String,
    },
    /// Clean up expired callback requests
    Gc {
        /// Project root directory
        #[arg(long)]
        root: Option<String>,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Set the configured tmux session and migrate panes
    Set {
        /// Target tmux session name (e.g., "5")
        name: String,
    },
    /// Show the authoritative actor/session status for a document
    Status {
        /// Path to the session document
        file: PathBuf,
    },
    /// Show the actor/session transition history for a document
    History {
        /// Path to the session document
        file: PathBuf,
    },
    /// Explicitly attach a document session to a tmux pane, creating a new generation
    Attach {
        /// Path to the session document
        file: PathBuf,
        /// Explicit tmux pane ID (defaults to the current pane when inside tmux)
        #[arg(long)]
        pane: Option<String>,
    },
    /// Restart the live session supervisor for a document
    #[command(name = "restart-supervisor", visible_alias = "restart")]
    Restart {
        /// Path to the session document
        file: PathBuf,
        /// Request a fresh restart instead of the default continue-mode restart
        #[arg(long)]
        fresh: bool,
        /// Bypass stale busy-state refusal and request supervisor-mediated restart
        #[arg(long)]
        force: bool,
    },
    /// Stop the harness agent child while keeping the supervisor alive at its
    /// restart-or-quit keepalive prompt (the operator can then restart the agent)
    #[command(name = "stop-agent")]
    StopAgent {
        /// Path to the session document
        file: PathBuf,
        /// Optional human-readable reason recorded for observability
        #[arg(long)]
        reason: Option<String>,
    },
    /// Clear the configured tmux session when no file is provided, or clear the bound harness session when FILE is provided
    Clear {
        /// Optional path to the session document
        file: Option<PathBuf>,
    },
    /// Intentionally interrupt a live bound harness session, then clear it
    #[command(name = "interrupt-clear")]
    InterruptClear {
        /// Path to the session document
        file: PathBuf,
        /// Kill the bound pane/supervisor and clear registry state when normal interrupt-clear cannot settle
        #[arg(long)]
        force: bool,
    },
    /// Cancel the currently-running turn (interrupt only, context preserved).
    /// Safe no-op when the harness is idle, so repeated calls never close the agent.
    #[command(name = "cancel-turn")]
    CancelTurn {
        /// Path to the session document
        file: PathBuf,
    },
    /// Diagnose actor/registry/supervisor drift for a document
    Doctor {
        /// Path to the session document
        file: PathBuf,
        /// Escalate into the explicit repair path before re-checking status
        #[arg(long)]
        repair: bool,
    },
    /// Dump the state of ALL actors in a project (actor record + cycle phase +
    /// closeout recovery classification) for investigating state drift
    Debug {
        /// Optional path to a document whose project to inspect (defaults to the
        /// current working directory's project root)
        file: Option<PathBuf>,
        /// Emit a human-readable summary instead of JSON
        #[arg(long)]
        human: bool,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Download and install an editor plugin
    Install {
        /// Editor: jetbrains, vscode
        editor: String,
        /// Install from local build instead of GitHub Releases
        #[clap(long)]
        local: bool,
    },
    /// Update an installed plugin to the latest version
    Update {
        /// Editor: jetbrains, vscode
        editor: String,
    },
    /// List installed editor plugins
    List,
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Install the skill definition for the detected (or specified) agent harness
    Install {
        /// After install, output reload instructions: compact (default) or restart
        #[arg(long)]
        reload: Option<String>,
        /// Target harness: claude, opencode, codex, cursor, generic (auto-detected if omitted)
        #[arg(long)]
        harness: Option<String>,
        /// Install for all supported harnesses
        #[arg(long)]
        all: bool,
    },
    /// Check if the installed skill matches the binary version
    Check,
}

/// Initialize structured logging. When `AGENT_DOC_LOG` is set (e.g., "debug"),
/// logs are written to `.agent-doc/logs/debug.log`. When unset, this is a no-op.
fn init_tracing() {
    let filter = match std::env::var("AGENT_DOC_LOG") {
        Ok(val) => val,
        Err(_) => return, // No logging configured — zero overhead
    };

    // Find .agent-doc/logs/ directory (walk up from CWD)
    let log_dir = {
        let mut dir = std::env::current_dir().unwrap_or_default();
        loop {
            let candidate = dir.join(".agent-doc/logs");
            if candidate.is_dir() {
                break Some(candidate);
            }
            if !dir.pop() {
                break None;
            }
        }
    };

    let Some(log_dir) = log_dir else {
        eprintln!("[tracing] AGENT_DOC_LOG set but no .agent-doc/logs/ found — logging disabled");
        return;
    };

    let file_appender = tracing_appender::rolling::daily(&log_dir, "debug.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard so it lives for the program lifetime
    std::mem::forget(_guard);

    use tracing_subscriber::EnvFilter;
    let env_filter = EnvFilter::try_new(&filter).unwrap_or_else(|_| EnvFilter::new("debug"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .init();

    tracing::debug!("agent-doc tracing initialized (filter: {})", filter);
}

fn main() -> ExitCode {
    match try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            print_terminal_error_report(&err);
            ExitCode::FAILURE
        }
    }
}

fn print_terminal_error_report(err: &anyhow::Error) {
    let report = format!("Error: {err:?}");
    let mut stderr = std::io::stderr().lock();
    let _ = write_terminal_error_report(&mut stderr, &report);
}

fn write_terminal_error_report(writer: &mut impl Write, report: &str) -> std::io::Result<()> {
    let report = report.trim_end_matches(&['\r', '\n'][..]);
    for line in report.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\r\n")?;
    }
    writer.flush()
}

fn try_main() -> anyhow::Result<()> {
    // `#supresilience` — crash resilience before any output (SIGPIPE reset + panic hook).
    crash_resilience::install();
    // Initialize structured logging via AGENT_DOC_LOG env var.
    // Examples: AGENT_DOC_LOG=debug, AGENT_DOC_LOG=agent_doc::preflight=debug
    // When set, logs to .agent-doc/logs/debug.log (auto-rotated).
    // When unset, no file logging (zero overhead).
    init_tracing();

    // `#orchver` — stamp the real top-level binary version into controller/supervisor
    // identities so the stale-binary warning reports the installed executable version.
    agent_doc_controller_io::project_controller::set_binary_version(env!("CARGO_PKG_VERSION"));
    agent_doc_controller_io::project_controller::install_runtime_effects(
        &PROJECT_CONTROLLER_RUNTIME_EFFECTS,
    );
    agent_doc_sync_io::install_runtime_effects(&SYNC_RUNTIME_EFFECTS);
    agent_doc_compact_io::install_runtime_effects(&COMPACT_RUNTIME_EFFECTS);

    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let pending_alias_used = deprecated_pending_alias_used(&raw_args);
    reject_plain_shell_bare_file_invocation(&raw_args)?;
    let cli = Cli::parse_from(rewrite_bare_file_invocation(raw_args));

    // Warn about newer versions on startup, but skip if running the upgrade command itself.
    if !matches!(cli.command, Commands::Upgrade) {
        upgrade::warn_if_outdated();
    }

    if pending_alias_used && matches!(cli.command, Commands::Backlog { .. }) {
        eprintln!(
            "[deprecation] `agent-doc pending` is deprecated — use `agent-doc backlog` instead"
        );
    }

    let config = agent_doc_config::load()?;

    match cli.command {
        Commands::Run {
            file,
            branch,
            agent,
            model,
            dry_run,
            no_git,
            force_disk,
        } => agent_doc_run_io::run(
            &agent_doc_run_runtime_io::DIRECT_RUN_EFFECTS,
            &file,
            branch,
            agent.as_deref(),
            model.as_deref(),
            dry_run,
            no_git,
            force_disk,
            &config,
        ),
        Commands::History { file, restore } => match restore {
            Some(commit) => history::restore(&file, &commit),
            None => history::list(&file),
        },
        Commands::Log { file } => history::log(&file),
        Commands::Ops { action } => match action {
            OpsAction::Summary {
                project_root,
                limit,
                json,
            } => ops_report::run_summary(project_root.as_deref(), limit, json),
            OpsAction::Diagnose {
                project_root,
                file,
                cycle_id,
                patch_id,
                session_id,
                limit,
                json,
            } => ops_report::run_diagnose(
                project_root.as_deref(),
                file.as_deref(),
                cycle_id.as_deref(),
                patch_id.as_deref(),
                session_id.as_deref(),
                limit,
                json,
            ),
        },
        Commands::VerifyOpCapture {
            file,
            expect_cafe_demo,
        } => op_capture_verify::run(&file, expect_cafe_demo),
        Commands::Show {
            file,
            back,
            at,
            tag,
        } => history::show(&file, back, at, tag.as_deref()),
        Commands::Init {
            file,
            title,
            agent,
            mode,
        } => init::run(
            file.as_deref(),
            title.as_deref(),
            agent.as_deref(),
            mode.as_deref(),
            &config,
        ),
        Commands::Install {
            editor,
            skip_prereqs,
            skip_plugins,
        } => install::run(editor.as_deref(), skip_prereqs, skip_plugins),
        Commands::Diff {
            file,
            wait,
            from,
            to,
        } => {
            if let Some(from_ref) = from {
                let to_ref = to.as_deref().unwrap_or("HEAD");
                history::git_diff(&file, &from_ref, to_ref)
            } else {
                agent_doc_diff_io::run(
                    &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
                    &file,
                    wait,
                )
            }
        }
        Commands::Exchange { action } => match action {
            ExchangeAction::List { file } => exchange::list(&file),
            ExchangeAction::Remove { file, id } => exchange::remove(&file, &id),
            ExchangeAction::AddResponse { file, header } => exchange::add_response(&file, &header),
            ExchangeAction::AddPrompt { file } => exchange::add_prompt(&file),
            ExchangeAction::Move {
                file,
                id,
                anchor,
                before,
            } => exchange::move_node(&file, &id, &anchor, before),
        },
        Commands::Reset {
            file,
            from_current,
            preserve_session,
            force_disk,
        } => reset::run(&file, from_current, preserve_session, force_disk),
        Commands::Clean { file, archive } => clean::run(&file, archive),
        Commands::AuditDocs { root } => audit_docs::run(root.as_deref()),
        Commands::Checkpoint {
            file,
            restore,
            diff,
        } => agent_doc_git_io::checkpoint::run(&file, restore.as_deref(), diff.as_deref()),
        Commands::Gc { root, dry_run } => {
            let mut effects = CliGcControllerEffects;
            let result = agent_doc_gc_io::run_with_controller_effects(
                root.as_deref(),
                dry_run,
                &mut effects,
                agent_doc_gc_io::GcControllerConfig {
                    stale_starting_after: Duration::from_secs(3600),
                    dead_actor_prune_after:
                        agent_doc_controller_io::project_controller::DEAD_ACTOR_PRUNE_AFTER,
                    stale_preparing_controller_after: agent_doc_controller_io::project_controller::stale_preparing_controller_threshold(),
                },
            )?;
            if dry_run {
                eprintln!(
                    "[gc] Dry run: {} files would be deleted, {} kept",
                    result.deleted, result.skipped
                );
            }
            Ok(())
        }
        Commands::Start {
            file,
            force,
            route_owned,
            route_owned_reap_policy,
        } => match route_owned_reap_policy {
            agent_doc_supervisor::route_owned::RouteOwnedReapPolicy::Auto => {
                agent_doc_start_runtime_io::run(&file, force, route_owned)
            }
            policy => {
                agent_doc_start_runtime_io::run_with_reap_policy(&file, force, route_owned, policy)
            }
        },
        Commands::Route {
            file,
            dispatch_only,
            plain_trigger,
            pane,
            cols,
            focus: _focus,
            debounce,
            wait_for_ready,
            force_disk,
        } => {
            // NOTE: agent_doc_sync_io::sync::run_layout_only was previously called here after route when
            // --col args were provided. Removed because the JB plugin calls `agent-doc sync`
            // separately with the correct --window arg. Running sync from both route AND
            // the plugin created a double-sync glitch (panes bouncing between stash and
            // agent-doc window). The plugin's sync is authoritative for layout.
            let mode = if dispatch_only {
                agent_doc_route_io::command::RouteMode::DispatchOnly
            } else {
                agent_doc_route_io::command::RouteMode::Managed
            };
            let wait_for_ready =
                wait_for_ready.map(|secs| std::time::Duration::from_secs(secs.min(600)));
            agent_doc_route_io::invocation::run_with_force_disk(
                &file,
                pane.as_deref(),
                debounce,
                &cols,
                mode,
                plain_trigger,
                wait_for_ready,
                force_disk,
                agent_doc_route_io::runtime_effects::route_command_effects(route_repair_closeout),
            )
        }
        Commands::Prompt { file, answer, all } => {
            if all {
                return agent_doc_prompt_io::run_all();
            }
            let file = file.context("FILE required when not using --all")?;
            match answer {
                Some(option) => agent_doc_prompt_io::answer(&file, option),
                None => agent_doc_prompt_io::run(&file),
            }
        }
        Commands::Commit { file } => agent_doc_commit_io::commit(&file).map(|_| ()),
        Commands::Dedupe { file } => dedupe_cmd::run(&file),
        Commands::Cancel { file } => {
            match agent_doc_repair_io::cancel_preflight_cycle(
                &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
                &file,
            )? {
                agent_doc_turn::repair::CancelOutcome::Abandoned => {
                    println!(
                        "[cancel] abandoned orphaned preflight_started cycle; next dispatch starts fresh"
                    );
                }
                agent_doc_turn::repair::CancelOutcome::NoOpenCycle => {
                    println!("[cancel] no open cycle to reclaim");
                }
                agent_doc_turn::repair::CancelOutcome::Protected => {
                    println!(
                        "[cancel] open cycle owns real work (advanced past preflight or has a response capture); left intact"
                    );
                }
            }
            Ok(())
        }
        Commands::Claim {
            file,
            position,
            pane,
            window,
            force,
            isolate,
        } => agent_doc_claim_io::run(
            &file,
            position.as_deref(),
            pane.as_deref(),
            window.as_deref(),
            force,
            isolate,
            &CliClaimRuntimeEffects,
        ),
        Commands::DrainClaim {
            file,
            owner,
            release,
        } => {
            let file_str = file.to_string_lossy();
            if release {
                agent_doc_queue::drain_owner::clear_drain_owner_lease(&file_str);
                println!("released drain-owner lease for {}", file.display());
            } else {
                agent_doc_queue::drain_owner::refresh_drain_owner_lease(&file_str, &owner)?;
                println!(
                    "claimed drain-owner lease owner={owner} for {}",
                    file.display()
                );
            }
            Ok(())
        }
        Commands::Focus {
            file,
            pane,
            blocking,
            ..
        } => {
            if blocking {
                agent_doc_focus_io::run_blocking(
                    &focus_effects::FOCUS_EFFECTS,
                    &file,
                    pane.as_deref(),
                )
            } else {
                agent_doc_focus_io::run(&focus_effects::FOCUS_EFFECTS, &file, pane.as_deref())
            }
        }
        Commands::Layout {
            files,
            split,
            pane,
            window,
        } => {
            let split = match split.as_str() {
                "v" | "vertical" => layout::Split::Vertical,
                _ => layout::Split::Horizontal,
            };
            let paths: Vec<&Path> = files.iter().map(|f| f.as_path()).collect();
            layout::run(&paths, split, pane.as_deref(), window.as_deref())
        }
        Commands::Sync {
            columns,
            window,
            focus,
            rename,
            no_autostart,
            exact_visible,
        } => {
            if rename && let Some(ref f) = focus {
                agent_doc_sync_io::sync::write_rename_debounce(f);
            }
            if no_autostart {
                if exact_visible {
                    agent_doc_sync_io::sync::run_layout_only_exact_visible(
                        &columns,
                        window.as_deref(),
                        focus.as_deref(),
                    )
                } else {
                    agent_doc_sync_io::sync::run_layout_only(
                        &columns,
                        window.as_deref(),
                        focus.as_deref(),
                    )
                }
            } else {
                agent_doc_sync_io::sync::run(&columns, window.as_deref(), focus.as_deref())
            }
        }
        Commands::Patch {
            file,
            component,
            mode,
            content,
        } => patch::run(&file, &component, mode, content.as_deref()),
        Commands::Watch {
            stop,
            status,
            debounce,
            max_cycles,
        } => {
            if stop {
                agent_doc_watch_io::stop()
            } else if status {
                agent_doc_watch_io::status()
            } else {
                let mut effects = CliWatchDaemonEffects::default();
                agent_doc_watch_io::start(
                    &config,
                    agent_doc_watch_io::WatchConfig {
                        debounce_ms: debounce,
                        max_cycles,
                    },
                    &mut effects,
                )
            }
        }
        Commands::Outline { file, json } => outline_cmd::run_outline(&file, json),
        Commands::AutoDag { file, json } => auto_dag::run_command(&file, json),
        Commands::Resync { file, fix, session } => {
            if fix {
                agent_doc_sync_io::resync::run_fix(file.as_deref(), session.as_deref())
            } else {
                agent_doc_sync_io::resync::run(false, session.as_deref(), file.as_deref())
            }
        }
        Commands::Fix { file, session } => {
            agent_doc_sync_io::resync::run_fix(file.as_deref(), session.as_deref())
        }
        Commands::Skill { command } => {
            match command {
                SkillCommands::Install {
                    reload,
                    harness,
                    all,
                } => {
                    if all {
                        skill::install_all()?;
                    } else if let Some(ref h) = harness {
                        let env = agent_kit::detect::Environment::from_name(h)
                        .ok_or_else(|| anyhow::anyhow!(
                            "unknown harness '{}'. Valid: claude, opencode, codex, cursor, generic", h
                        ))?;
                        skill::install_for(env)?;
                    } else {
                        let updated = skill::install_and_check_updated()?;
                        if updated && let Some(ref mode) = reload {
                            match mode.as_str() {
                                "restart" => {
                                    println!("SKILL_RELOAD=restart");
                                    println!(
                                        "Skill updated. Please restart this session with --resume to reload the skill."
                                    );
                                }
                                _ => {
                                    println!("SKILL_RELOAD=compact");
                                    println!(
                                        "Skill updated. Please run /compact to reload the updated skill instructions."
                                    );
                                }
                            }
                        }
                    }
                    Ok(())
                }
                SkillCommands::Check => skill::check(),
            }
        }
        Commands::Plugin { action } => match action {
            PluginAction::Install { editor, local } => {
                if local {
                    plugin::install_local(&editor)
                } else {
                    plugin::install(&editor)
                }
            }
            PluginAction::Update { editor } => plugin::update(&editor),
            PluginAction::List => plugin::list(),
        },
        Commands::Write { args, commit } => {
            let lint_override = match args.lint.as_deref() {
                None => None,
                Some(s) => Some(
                    agent_doc_frontmatter::lint::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
            };
            agent_doc_repair_command_io::run_write_command_with_empty_response_recovery(
                agent_doc_write_command_io::CommandOptions {
                    file: args.file,
                    baseline_file: args.baseline_file,
                    is_template: args.template,
                    is_stream: args.stream,
                    is_ipc: args.ipc,
                    force_disk: args.force_disk,
                    origin: args.origin,
                    pending_add: args.pending_add,
                    pending_add_to: args.pending_add_to,
                    pending_add_gated: args.pending_add_gated,
                    pending_add_after: args.pending_add_after,
                    pending_add_before: args.pending_add_before,
                    pending_add_back: args.pending_add_back,
                    icebox_add: args.icebox_add,
                    icebox_add_after: args.icebox_add_after,
                    icebox_add_before: args.icebox_add_before,
                    icebox_add_back: args.icebox_add_back,
                    icebox_edit: args.icebox_edit,
                    icebox_clear: args.icebox_clear,
                    icebox_reorder: args.icebox_reorder,
                    pending_done: args.pending_done,
                    pending_edit: args.pending_edit,
                    pending_clear: args.pending_clear,
                    pending_reorder: args.pending_reorder,
                    pending_gate: args.pending_gate,
                    pending_ungate: args.pending_ungate,
                    pending_resolve_gate: args.pending_resolve_gate,
                    pending_set_gate_type: args.pending_set_gate_type,
                    pending_set_verify: args.pending_set_verify,
                    review_add: args.review_add,
                    review_edit: args.review_edit,
                    review_remove: args.review_remove,
                    review_resolve: args.review_resolve,
                    queue_completion_ids: Vec::new(),
                    allow_replace_pending: args.allow_replace_pending,
                    pending_only: args.pending_only,
                    status: args.status,
                    lint_override,
                    commit_sibling: args.commit_sibling,
                    commit_sibling_message: args.commit_sibling_message,
                },
                if commit {
                    agent_doc_write_command_io::CommitMode::BestEffort
                } else {
                    agent_doc_write_command_io::CommitMode::None
                },
            )
        }
        Commands::Finalize { args } => {
            let lint_override = match args.lint.as_deref() {
                None => None,
                Some(s) => Some(
                    agent_doc_frontmatter::lint::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
            };
            agent_doc_repair_command_io::run_write_command_with_empty_response_recovery(
                agent_doc_write_command_io::CommandOptions {
                    file: args.file,
                    baseline_file: args.baseline_file,
                    is_template: args.template,
                    is_stream: args.stream,
                    is_ipc: args.ipc,
                    force_disk: args.force_disk,
                    origin: args.origin,
                    pending_add: args.pending_add,
                    pending_add_to: args.pending_add_to,
                    pending_add_gated: args.pending_add_gated,
                    pending_add_after: args.pending_add_after,
                    pending_add_before: args.pending_add_before,
                    pending_add_back: args.pending_add_back,
                    icebox_add: args.icebox_add,
                    icebox_add_after: args.icebox_add_after,
                    icebox_add_before: args.icebox_add_before,
                    icebox_add_back: args.icebox_add_back,
                    icebox_edit: args.icebox_edit,
                    icebox_clear: args.icebox_clear,
                    icebox_reorder: args.icebox_reorder,
                    pending_done: args.pending_done,
                    pending_edit: args.pending_edit,
                    pending_clear: args.pending_clear,
                    pending_reorder: args.pending_reorder,
                    pending_gate: args.pending_gate,
                    pending_ungate: args.pending_ungate,
                    pending_resolve_gate: args.pending_resolve_gate,
                    pending_set_gate_type: args.pending_set_gate_type,
                    pending_set_verify: args.pending_set_verify,
                    review_add: args.review_add,
                    review_edit: args.review_edit,
                    review_remove: args.review_remove,
                    review_resolve: args.review_resolve,
                    queue_completion_ids: Vec::new(),
                    allow_replace_pending: args.allow_replace_pending,
                    pending_only: args.pending_only,
                    status: args.status,
                    lint_override,
                    commit_sibling: args.commit_sibling,
                    commit_sibling_message: args.commit_sibling_message,
                },
                agent_doc_write_command_io::CommitMode::Required,
            )
        }
        Commands::Stream {
            file,
            interval,
            agent,
            model,
            no_git,
            lint,
        } => {
            let lint_override = match lint.as_deref() {
                Some(s) => Some(
                    agent_doc_frontmatter::lint::LintCliMode::parse(s)
                        .map_err(|e| anyhow::anyhow!(e))?,
                ),
                None => None,
            };
            agent_doc_stream_io::run(
                agent_doc_stream_io::StreamRunOptions {
                    file: &file,
                    interval_ms: interval,
                    agent_name: agent.as_deref(),
                    model: model.as_deref(),
                    no_git,
                    config: &config,
                    lint_override,
                },
                cli_stream_effects(),
            )
        }
        Commands::TemplateInfo { file } => {
            let info = template_io::template_info(&file)?;
            println!("{}", serde_json::to_string_pretty(&info)?);
            Ok(())
        }
        Commands::Repair {
            file,
            apply_recovery,
        } => {
            if apply_recovery {
                use agent_doc_flow_io::closeout::RecoveryApplication;
                match agent_doc_flow_io::closeout::apply_closeout_recovery(
                    &file,
                    &agent_doc_closeout_runtime_io::closeout_effects(),
                )? {
                    RecoveryApplication::NothingToDo => {
                        eprintln!("[repair] {} is clean — no recovery needed", file.display());
                    }
                    RecoveryApplication::Applied { state, action } => {
                        eprintln!(
                            "[repair] applied recovery [{}]: {} for {}",
                            state.as_str(),
                            action,
                            file.display()
                        );
                    }
                    RecoveryApplication::NotApplied {
                        state,
                        reason,
                        recommended,
                    } => {
                        eprintln!(
                            "[repair] recovery [{}] not auto-applied: {}\n[repair] run: {}",
                            state.as_str(),
                            reason,
                            recommended
                        );
                    }
                }
                return Ok(());
            }
            let outcome = agent_doc_repair_command_io::repair(&file)?;
            if !outcome.repaired() {
                eprintln!("[repair] No pending response found for {}", file.display());
            }
            Ok(())
        }
        Commands::Preflight { file, probe } => agent_doc_preflight_command_io::run_with_options(
            &file,
            agent_doc_preflight_command_io::PreflightOptions { probe },
        ),
        Commands::Admit { file } => {
            let output = agent_doc_cycle_state_io::admit_with_current_resolver(
                &file,
                |file| {
                    Ok(
                        agent_doc_document_realtime_io::try_resolve_current_doc_from_file(file)?
                            .content,
                    )
                },
                agent_doc_snapshot_io::load,
                agent_doc_ops_log_io::log_op,
            )?;
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(())
        }
        Commands::Doctor {
            file,
            preflight_json,
            session_check_json,
            limit,
            json,
        } => {
            let mut effects = CliWorkflowDoctorEffects;
            agent_doc_workflow_io::doctor::run(
                &file,
                agent_doc_workflow_io::doctor::WorkflowDoctorOptions {
                    preflight_json,
                    session_check_json,
                    ops_limit: limit,
                    json,
                },
                &mut effects,
            )
        }
        Commands::Autofix {
            file,
            preflight_json,
            session_check_json,
            limit,
            apply,
            dry_run,
            json,
        } => {
            let mut effects = CliWorkflowDoctorEffects;
            agent_doc_workflow_io::autofix::run(
                &file,
                agent_doc_workflow_io::autofix::WorkflowAutofixOptions {
                    preflight_json,
                    session_check_json,
                    ops_limit: limit,
                    apply,
                    dry_run,
                    json,
                },
                &mut effects,
            )
        }
        Commands::Plan { file } => plan::run(&file),
        Commands::Jobs { action } => match action {
            JobsAction::Create {
                file,
                operation_doc,
                audit,
                budget,
            } => jobs::create(
                &file,
                jobs::CreateOptions {
                    operation_doc,
                    audit,
                    budget,
                },
            ),
            JobsAction::List { file, json } => jobs::list(&file, json),
            JobsAction::Status { file, json } => jobs::status(&file, json),
            JobsAction::Collect { file, cycle, json } => {
                jobs::collect(&file, cycle.as_deref(), json)
            }
        },
        Commands::SessionCheck {
            file,
            codex_final_gate,
        } => agent_doc_session_check_io::run_with_options(
            &file,
            codex_final_gate,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        ),
        Commands::Mcp { action } => match action {
            McpAction::Serve { project_root } => mcp::serve(project_root.as_deref()),
        },
        Commands::Serve {
            file,
            host,
            port,
            auth_token,
            read_only_token,
            tls_cert,
            tls_key,
        } => serve::run(serve::ServeOptions::new(
            file,
            host,
            port,
            auth_token,
            read_only_token,
            tls_cert,
            tls_key,
        )),
        Commands::Read { file, component } => read::run(&file, component.as_deref()),
        Commands::DescribeImage {
            image,
            provider,
            model,
            api_key,
            prompt,
        } => describe_image::run(
            &image,
            provider.as_deref(),
            model.as_deref(),
            api_key.as_deref(),
            prompt.as_deref(),
        ),
        Commands::ResponseToc {
            file,
            backlog_id,
            query,
            limit,
            json,
        } => agent_doc_response_toc_io::run_toc(
            &file,
            backlog_id.as_deref(),
            query.as_deref(),
            limit,
            json,
        ),
        Commands::ResponseFetch {
            file,
            locator,
            before,
            after,
            json,
        } => agent_doc_response_toc_io::run_fetch(&file, &locator, before, after, json),
        Commands::ArchiveIndex { file, rebuild } => {
            agent_doc_sqlite::archive_index::run_index(&file, rebuild)
        }
        Commands::ArchiveSearch {
            file,
            query,
            backlog_id,
            session,
            limit,
            json,
            rebuild,
        } => agent_doc_sqlite::archive_index::run_search(
            &file,
            query.as_deref(),
            backlog_id.as_deref(),
            session.as_deref(),
            limit,
            json,
            rebuild,
        ),
        Commands::Memory { action } => match action {
            MemoryAction::Index { file, db, json } => {
                agent_doc_memory_io::session::run_index(&file, db.as_deref(), json)
            }
            MemoryAction::Search {
                file,
                query,
                db,
                limit,
                json,
                rebuild,
            } => agent_doc_memory_io::session::run_search(
                &file,
                &query,
                db.as_deref(),
                limit,
                json,
                rebuild,
            ),
        },
        Commands::Compact {
            file,
            keep,
            component,
            message,
            tag,
            commit,
            force_disk,
        } => agent_doc_compact_io::run(
            &file,
            keep,
            component.as_deref(),
            message.as_deref(),
            tag.as_deref(),
            commit,
            force_disk,
        ),
        Commands::Convert {
            file,
            mode,
            agent_doc_format,
            agent_doc_write,
        } => convert::run(&file, mode.as_ref(), agent_doc_format, agent_doc_write),
        Commands::Mode { file, set } => mode::run(&file, set.as_deref()),
        Commands::Annotate {
            file,
            force,
            history,
        } => annotate::run(&file, force, history),
        Commands::Undo { file } => undo::run(&file),
        Commands::Extract {
            source,
            target,
            component,
        } => extract::run(&source, &target, component.as_deref()),
        Commands::Transfer {
            source,
            target,
            component,
            bypass_claim,
            items,
            referral,
        } => {
            let item_ids: Option<Vec<String>> = items.map(|s| {
                s.split(',')
                    .map(|id| id.trim().trim_start_matches('#').to_string())
                    .collect()
            });
            extract::transfer(
                &source,
                &target,
                &component,
                bypass_claim,
                item_ids.as_deref(),
                referral,
            )
        }
        Commands::Rename { old_path, new_path } => rename::run(&old_path, &new_path),
        Commands::Migrate {
            files,
            all,
            dry_run,
        } => migrate::run(&files, all, dry_run),
        Commands::Claims => {
            let cwd = std::env::current_dir()?;
            if let Some(root) = agent_doc_fs::find_project_root(&cwd) {
                let log_path = root.join(".agent-doc/claims.log");
                if let Ok(contents) = std::fs::read_to_string(&log_path)
                    && !contents.is_empty()
                {
                    print!("{}", contents);
                    std::fs::write(&log_path, "")?;
                }
            }
            Ok(())
        }
        Commands::Parallel {
            file,
            tasks_explicit,
            model,
            no_git,
            no_worktree,
            timeout,
            dry_run,
        } => orchestrate::run_parallel_compat(
            &file,
            parallel::ParallelConfig {
                tasks: tasks_explicit
                    .into_iter()
                    .map(|task| parallel::ParallelTask {
                        description: task.clone(),
                        prompt: task,
                    })
                    .collect(),
                model,
                no_git,
                no_worktree,
                timeout_secs: timeout,
                dry_run,
            },
            &config,
        ),
        Commands::Orchestrate {
            file,
            mode,
            tasks_explicit,
            from_file,
            from_exchange,
            from_queue,
            resume_schedule,
            agent,
            model,
            no_git,
            no_worktree,
            timeout,
            dry_run,
            plan,
        } => orchestrate::run(
            &file,
            orchestrate::OrchestrateConfig {
                mode,
                tasks_explicit,
                from_file,
                from_exchange,
                from_queue,
                resume_schedule,
                agent,
                model,
                no_git,
                no_worktree,
                timeout_secs: timeout,
                dry_run,
                plan,
            },
            &config,
        ),
        Commands::Notify {
            file,
            message,
            source,
            affects,
            no_commit,
            pending_add,
            pending_add_gated,
            no_create_pending,
        } => notify::run(
            &file,
            message.as_deref(),
            source.as_deref(),
            affects.as_deref(),
            !no_commit,
            &pending_add,
            &pending_add_gated,
            no_create_pending,
        ),
        Commands::Boundary { file, component } => {
            agent_doc_boundary_io::run(&file, component.as_deref())
        }
        Commands::Terminal { file, session } => terminal::run(&file, session.as_deref()),
        Commands::Autoclaim => autoclaim::run(),
        Commands::TurnStatus { action } => match action {
            TurnStatusAction::Active => agent_doc_turn_status_io::run(true),
            TurnStatusAction::Idle => agent_doc_turn_status_io::run(false),
            TurnStatusAction::Install { dir, user, tmux } => {
                skill::install_turn_status_hooks(dir.as_deref(), user, tmux)
            }
        },
        Commands::Upgrade => upgrade::run(),
        Commands::LibPath => {
            // Print the path to the shared library built alongside this binary.
            // The cdylib is in the same target directory as the binary.
            let exe = std::env::current_exe()?;
            let dir = exe.parent().unwrap();
            #[cfg(target_os = "linux")]
            let lib_name = "libagent_doc.so";
            #[cfg(target_os = "macos")]
            let lib_name = "libagent_doc.dylib";
            #[cfg(target_os = "windows")]
            let lib_name = "agent_doc.dll";
            let lib_path = dir.join(lib_name);
            if lib_path.exists() {
                println!("{}", lib_path.display());
            } else {
                eprintln!("[lib-path] library not found at {}", lib_path.display());
                eprintln!("[lib-path] build with: cargo build --release");
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::GcLibs { target_dir } => lib_gc::run(target_dir.as_deref()),
        Commands::LibInstall {
            source,
            profile,
            target_dir,
        } => lib_install::run(source.as_deref(), target_dir.as_deref(), &profile),
        Commands::ReliableSyncStatus { json, project_root } => {
            let root = match project_root {
                Some(root) => root,
                None => {
                    let cwd = std::env::current_dir()?;
                    agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd)
                }
            };
            let status = agent_doc_controller_io::project_controller::reliable_sync_status(&root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                let dark = if status.dual_run {
                    ""
                } else {
                    " (plane dark — AGENT_DOC_RELIABLE_SYNC_DUAL_RUN=0)"
                };
                println!("reliable-sync dual-run: {}{}", status.dual_run, dark);
                println!(
                    "parity (plane open-set == sidecar open-set): {}",
                    if status.parity { "MATCH" } else { "MISMATCH" }
                );
                println!("plane open docs ({}):", status.plane_open_docs.len());
                for doc in &status.plane_open_docs {
                    let pids = status
                        .per_doc_pids
                        .iter()
                        .find(|(d, _)| d == doc)
                        .map(|(_, p)| p.clone())
                        .unwrap_or_default();
                    let live = if status.plane_live_docs.contains(doc) {
                        "live"
                    } else {
                        "not-live"
                    };
                    let path = status
                        .plane_open_paths
                        .iter()
                        .find(|(h, _)| h == doc)
                        .and_then(|(_, p)| p.clone())
                        .unwrap_or_else(|| "<no sidecar to resolve path>".to_string());
                    println!("  {live:8}  pids={pids:?}  {path}");
                }
                println!(
                    "sidecar open docs — durable live-buffer scan, strictly-live editors only ({}):",
                    status.sidecar_open_docs.len()
                );
                for doc in &status.sidecar_open_docs {
                    println!("  {doc}");
                }
                println!(
                    "in-memory registry open docs — secondary, empty right after a recycle ({}):",
                    status.registry_open_docs.len()
                );
                for doc in &status.registry_open_docs {
                    println!("  {doc}");
                }
            }
            Ok(())
        }
        Commands::SelfInstall {
            source_root,
            target_dir,
            profile,
            keep_worktree,
        } => self_install::run(
            source_root.as_deref(),
            target_dir.as_deref(),
            keep_worktree,
            &profile,
        ),
        Commands::ListCommands => commands::run(),
        Commands::Session { action } => match action {
            Some(SessionAction::Set { name }) => session_cmd::set(&name),
            Some(SessionAction::Status { file }) => session_actor_cmd::status(&file),
            Some(SessionAction::History { file }) => session_actor_cmd::history(&file),
            Some(SessionAction::Attach { file, pane }) => {
                session_actor_cmd::attach(&file, pane.as_deref())
            }
            Some(SessionAction::Restart { file, fresh, force }) => session_actor_cmd::restart(
                &file,
                if fresh {
                    session_actor_cmd::RestartMode::Fresh
                } else {
                    session_actor_cmd::RestartMode::Continue
                },
                force,
            ),
            Some(SessionAction::StopAgent { file, reason }) => {
                session_actor_cmd::stop_agent(&file, reason)
            }
            Some(SessionAction::Clear { file: Some(file) }) => {
                run_session_command_ensuring_supervisor(&file, || session_actor_cmd::clear(&file))
            }
            Some(SessionAction::Clear { file: None }) => session_cmd::clear(),
            Some(SessionAction::InterruptClear { file, force }) => {
                session_actor_cmd::interrupt_clear(&file, force)
            }
            Some(SessionAction::CancelTurn { file }) => session_actor_cmd::cancel_turn(&file),
            Some(SessionAction::Doctor { file, repair }) => {
                session_actor_cmd::doctor(&file, repair)
            }
            Some(SessionAction::Debug { file, human }) => {
                session_actor_cmd::debug(file.as_deref(), !human)
            }
            None => session_cmd::show(),
        },
        Commands::Controller { action } => match action {
            ControllerAction::Status {
                project_root,
                ensure,
            } => agent_doc_controller_io::project_controller::run_status(
                project_root.as_deref(),
                ensure,
            ),
            ControllerAction::Serve {
                project_root,
                launch_mode,
                listen_socket,
                controller_generation,
                previous_controller_pid,
                handoff_state,
            } => agent_doc_controller_io::project_controller::run_serve(
                project_root.as_deref(),
                &launch_mode,
                listen_socket.as_deref(),
                controller_generation,
                previous_controller_pid,
                &handoff_state,
            ),
            ControllerAction::Shutdown { project_root } => {
                agent_doc_controller_io::project_controller::run_shutdown(project_root.as_deref())
            }
            ControllerAction::Restart {
                project_root,
                force,
            } => agent_doc_controller_io::project_controller::run_restart(
                project_root.as_deref(),
                force,
            ),
        },
        Commands::Admin { action } => {
            let admin_effects = CliAdminControllerEffects::default();
            match action {
                AdminAction::Inspect {
                    document,
                    session,
                    pane,
                    project_root,
                    json,
                } => agent_doc_admin_io::inspect(
                    &admin_effects,
                    project_root.as_deref(),
                    document.as_deref(),
                    session.as_deref(),
                    pane.as_deref(),
                    json,
                ),
                AdminAction::List { project_root, json } => {
                    agent_doc_admin_io::list(&admin_effects, project_root.as_deref(), json)
                }
                AdminAction::Detect { project_root, json } => {
                    agent_doc_admin_io::detect(&admin_effects, project_root.as_deref(), json)
                }
                AdminAction::Dashboard {
                    project_root,
                    json,
                    once,
                    interval,
                } => dashboard_cmd::dashboard(project_root.as_deref(), json, once, interval),
                AdminAction::ReapStaleControllers {
                    project_root,
                    dry_run,
                } => {
                    let root = match project_root {
                        Some(r) => r,
                        None => {
                            let cwd = std::env::current_dir()?;
                            agent_doc_fs::find_project_root(&cwd).ok_or_else(|| {
                                anyhow::anyhow!(
                                    ".agent-doc/ project root not found from {}",
                                    cwd.display()
                                )
                            })?
                        }
                    };
                    let threshold =
                    agent_doc_controller_io::project_controller::stale_preparing_controller_threshold();
                    let (record_reaped, record_kept) =
                        agent_doc_controller_io::project_controller::terminate_stale_preparing_controllers_for_caller(
                            &root, threshold, dry_run, "admin",
                        )?;
                    let (orphan_reaped, orphan_kept) =
                        agent_doc_controller_io::project_controller::reap_orphaned_preparing_controllers_for_caller(
                            &root, threshold, dry_run, "admin",
                        )?;
                    let reaped = record_reaped + orphan_reaped;
                    let kept = record_kept + orphan_kept;
                    if dry_run {
                        println!(
                            "[admin] reap-stale-controllers (dry-run): {reaped} would be terminated, {kept} kept"
                        );
                    } else {
                        println!(
                            "[admin] reap-stale-controllers: {reaped} terminated, {kept} kept"
                        );
                    }
                    Ok(())
                }
                AdminAction::Recycle {
                    target,
                    project_root,
                    all_projects,
                    force,
                    json,
                } => {
                    if all_projects {
                        if target.is_some() || project_root.is_some() {
                            anyhow::bail!(
                                "admin recycle --all-projects cannot be combined with FILE_OR_PROJECT_ROOT or --project-root"
                            );
                        }
                        let (recycled, skipped) =
                        agent_doc_controller_io::project_controller::recycle_controllers_all_projects_force(force)?;
                        // #recycle-supervisor-fanout: an explicit fleet recycle also schedules a
                        // recycle of every valid-state route-owned supervisor (they host the agent
                        // turns), honored at each supervisor's next idle boundary. Fail-open — a
                        // supervisor enumeration hiccup must not abort the controller recycle.
                        let (supervisors_marked, supervisors_skipped) =
                        agent_doc_controller_io::project_controller::recycle_supervisors_all_projects_force(force)
                            .unwrap_or_else(|err| {
                                eprintln!(
                                    "[agent-doc] warning: supervisor recycle fan-out failed: {err:#}"
                                );
                                (0, 0)
                            });
                        if json {
                            println!(
                                "{}",
                                all_projects_recycle_json(
                                    recycled,
                                    skipped,
                                    supervisors_marked,
                                    supervisors_skipped,
                                    force
                                )
                            );
                        } else {
                            let boundary = if force {
                                "now (forced, overriding the in-flight-dispatch deferral)"
                            } else {
                                "at next idle boundary"
                            };
                            println!(
                                "[admin] recycle (all projects{}): {recycled} controller(s) marked to recycle {boundary}, {skipped} skipped; {supervisors_marked} route-owned supervisor(s) scheduled to recycle at next idle boundary, {supervisors_skipped} skipped",
                                if force { ", forced" } else { "" }
                            );
                        }
                    } else {
                        if target.is_some() && project_root.is_some() {
                            anyhow::bail!(
                                "admin recycle accepts either FILE_OR_PROJECT_ROOT or --project-root, not both"
                            );
                        }
                        let root_arg = project_root.as_deref().or(target.as_deref());
                        let root = agent_doc_project_root_io::project_root_from_arg(root_arg)?;
                        let recycled =
                            agent_doc_controller_io::project_controller::recycle_controller_force(
                                &root, force,
                            )?;
                        // `#recycle-no-boundaries`: when NO live controller answered, this
                        // is almost always an operator trying to bring a document's session
                        // back (a dead supervisor, or a live supervisor whose controller went
                        // away). Escalate to the kill+cold-start path
                        // (`session restart-supervisor`) automatically instead of a dead-end
                        // no-op, whenever a session-document path was given — no `--force`
                        // required. `--force` still flows through to the restart, where it
                        // only governs interrupting a *busy* pane.
                        //
                        // Only escalate for an actual session document (one carrying an
                        // `agent_doc_session` frontmatter id). A path that is not a startable
                        // session — a stray file, a scratch doc — degrades to the informative
                        // message instead of hard-failing the `restart` cold-start, so
                        // `admin recycle <anything>` never errors out where the old no-op
                        // silently succeeded.
                        let escalate =
                            recycle_should_escalate_dead_supervisor(recycled, target.as_deref())
                                && target
                                    .as_deref()
                                    .map(|p| {
                                        agent_doc_frontmatter_io::session::read_session_id(p)
                                            .is_some()
                                    })
                                    .unwrap_or(false);
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({ "scope": "project", "project_root": root.display().to_string(), "recycled": recycled, "forced": force, "escalated_cold_start": escalate })
                            );
                        } else if recycled {
                            let boundary = if force {
                                "now (forced, overriding the in-flight-dispatch deferral)"
                            } else {
                                "at next idle boundary"
                            };
                            println!(
                                "[admin] recycle{}: controller for {} marked to recycle {boundary}",
                                if force { " (forced)" } else { "" },
                                root.display()
                            );
                        } else if !escalate {
                            // No session-document path to cold-start (a bare project-root /
                            // `--project-root` form). `admin recycle` can only re-exec a
                            // *live* controller onto the fresh binary; with none running and
                            // no document to revive, point at the one-step remedy.
                            println!(
                                "[admin] recycle: no running controller for {} (nothing to recycle). Pass a session-document path (`agent-doc admin recycle <FILE>`) to cold-start its supervisor automatically.",
                                root.display()
                            );
                        }
                        if escalate {
                            let file = target.as_deref().expect(
                                "recycle_should_escalate_dead_supervisor guarantees a target",
                            );
                            eprintln!(
                                "[admin] recycle: no live controller for {} — escalating to a kill+cold-start via `session restart-supervisor {}`",
                                root.display(),
                                file.display()
                            );
                            session_actor_cmd::restart(
                                file,
                                session_actor_cmd::RestartMode::Continue,
                                force,
                            )?;
                        }
                    }
                    Ok(())
                }
                AdminAction::ReloadLib { json } => {
                    // `#cdylib-reload-broadcast`: write the global reload-broadcast file
                    // for the currently-installed cdylib and report how many editor
                    // projects it could also signal. Deterministic logic lives in
                    // `lib_install::reload_lib_now`; main.rs only renders the report.
                    let report = lib_install::reload_lib_now()?;
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "broadcast_path": report.broadcast_path.display().to_string(),
                                "lib_version": report.lib_version,
                                "editor_projects": report.editor_projects,
                            })
                        );
                    } else {
                        println!(
                            "[admin] reload-lib: cdylib v{} reload announced to editor plugins ({}); {} editor project(s) could also be signaled directly",
                            report.lib_version,
                            report.broadcast_path.display(),
                            report.editor_projects,
                        );
                    }
                    Ok(())
                }
                AdminAction::KillSupervisor {
                    document,
                    grace_secs,
                    dry_run,
                    json,
                } => {
                    #[cfg(unix)]
                    {
                        use agent_doc_supervisor_io::selfkill::{
                            SupervisorKillOutcome, drive_supervisor_kill, selfkill_grace,
                        };
                        let grace = grace_secs
                            .map(std::time::Duration::from_secs)
                            .unwrap_or_else(selfkill_grace);
                        let outcome = drive_supervisor_kill(&document, grace, dry_run)?;
                        let (status, pid, message) = match outcome {
                            SupervisorKillOutcome::NoSupervisor => (
                                "no_supervisor",
                                None,
                                format!(
                                    "no running route-owned supervisor for {}",
                                    document.display()
                                ),
                            ),
                            SupervisorKillOutcome::RefusedSelfAncestor(pid) => (
                                "refused_self_ancestor",
                                Some(pid),
                                format!(
                                    "supervisor pid {pid} is this session's own ancestor — refused; run `agent-doc admin kill-supervisor` from a different pane, or let the project controller drive it"
                                ),
                            ),
                            SupervisorKillOutcome::WouldKill(pid) => (
                                "would_kill",
                                Some(pid),
                                format!("dry-run: would kill supervisor pid {pid}"),
                            ),
                            SupervisorKillOutcome::Graceful(pid) => (
                                "graceful",
                                Some(pid),
                                format!("supervisor pid {pid} self-killed within the grace window"),
                            ),
                            SupervisorKillOutcome::Forced(pid) => (
                                "forced",
                                Some(pid),
                                format!(
                                    "supervisor pid {pid} force-killed after the {}s grace window",
                                    grace.as_secs()
                                ),
                            ),
                        };
                        if json {
                            println!(
                                "{}",
                                serde_json::json!({
                                    "status": status,
                                    "pid": pid,
                                    "document": document.display().to_string(),
                                    "grace_secs": grace.as_secs(),
                                })
                            );
                        } else {
                            println!("[admin] kill-supervisor: {message}");
                        }
                        // A refused self-ancestor is an operator error worth a non-zero exit
                        // so scripts can branch; everything else (incl. no_supervisor) is Ok.
                        if status == "refused_self_ancestor" {
                            anyhow::bail!("{message}");
                        }
                        Ok(())
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (document, grace_secs, dry_run, json);
                        anyhow::bail!("admin kill-supervisor is only supported on Unix");
                    }
                }
                AdminAction::Queue { action } => match action {
                    AdminQueueAction::Pause {
                        document,
                        project_root,
                        observed_generation,
                        reason,
                        json,
                    } => agent_doc_admin_io::queue_control(
                        &admin_effects,
                        agent_doc_admin_io::QueueControlOptions {
                            project_root: project_root.as_deref(),
                            document: document.as_deref(),
                            action: "pause",
                            observed_generation,
                            reason: Some(&reason),
                            item_id: None,
                            json,
                        },
                    ),
                    AdminQueueAction::Resume {
                        document,
                        project_root,
                        observed_generation,
                        reason,
                        json,
                    } => agent_doc_admin_io::queue_control(
                        &admin_effects,
                        agent_doc_admin_io::QueueControlOptions {
                            project_root: project_root.as_deref(),
                            document: document.as_deref(),
                            action: "resume",
                            observed_generation,
                            reason: reason.as_deref(),
                            item_id: None,
                            json,
                        },
                    ),
                    AdminQueueAction::Drain {
                        document,
                        project_root,
                        observed_generation,
                        until_id,
                        reason,
                        json,
                    } => agent_doc_admin_io::queue_control(
                        &admin_effects,
                        agent_doc_admin_io::QueueControlOptions {
                            project_root: project_root.as_deref(),
                            document: Some(document.as_path()),
                            action: "drain",
                            observed_generation,
                            reason: reason.as_deref(),
                            item_id: until_id.as_deref(),
                            json,
                        },
                    ),
                },
                AdminAction::Reap {
                    document,
                    all_stale,
                    session,
                    pane,
                    project_root,
                    observed_generation,
                    reason,
                    json,
                } => {
                    if all_stale {
                        if document.is_some()
                            || session.is_some()
                            || pane.is_some()
                            || observed_generation.is_some()
                        {
                            anyhow::bail!(
                                "admin reap --all-stale cannot be combined with a document, --session, --pane, or --observed-generation"
                            );
                        }
                        agent_doc_admin_io::reap_all_stale(
                            &admin_effects,
                            project_root.as_deref(),
                            &reason,
                            json,
                        )
                    } else {
                        let observed_generation = observed_generation.ok_or_else(|| {
                        anyhow::anyhow!(
                            "admin reap requires --observed-generation unless --all-stale is used"
                        )
                    })?;
                        agent_doc_admin_io::reap(
                            &admin_effects,
                            agent_doc_admin_io::ReapOptions {
                                project_root: project_root.as_deref(),
                                document: document.as_deref(),
                                session: session.as_deref(),
                                pane: pane.as_deref(),
                                observed_generation,
                                reason: &reason,
                                json,
                            },
                        )
                    }
                }
                AdminAction::Handoff {
                    document,
                    to_pane,
                    project_root,
                    observed_generation,
                    reason,
                    json,
                } => agent_doc_admin_io::handoff(
                    &admin_effects,
                    project_root.as_deref(),
                    &document,
                    &to_pane,
                    observed_generation,
                    &reason,
                    json,
                ),
                AdminAction::RepairProjection {
                    document,
                    project_root,
                    projection,
                    observed_generation,
                    reason,
                    json,
                } => agent_doc_admin_io::repair_projection(
                    &admin_effects,
                    project_root.as_deref(),
                    document.as_deref(),
                    &projection,
                    observed_generation,
                    reason.as_deref(),
                    json,
                ),
            }
        }
        Commands::Hook { action } => match action {
            HookAction::Fire {
                event,
                file,
                session_id,
                data,
            } => hook_cmd::fire(&event, &file, session_id.as_deref(), data.as_deref()),
            HookAction::Poll { event, since, root } => {
                hook_cmd::poll(&event, since, root.as_deref())
            }
            HookAction::Listen { root } => hook_cmd::listen(root.as_deref()),
            HookAction::Gc { root } => hook_cmd::gc(root.as_deref()),
            HookAction::CheckCallbacks { root } => {
                let pending = agent_doc_callback_io::scan_pending_callbacks(root.as_deref())?;
                let json = serde_json::to_string_pretty(
                    &serde_json::json!({"pending_callbacks": pending}),
                )?;
                println!("{}", json);
                Ok(())
            }
            HookAction::CodexUserPromptSubmit => {
                agent_doc_codex_hook_io::handle_user_prompt_submit()
            }
            HookAction::CodexStop => agent_doc_codex_stop_io::handle_stop(),
        },
        Commands::Cleanup {
            file,
            timeout,
            poll_interval,
            fallback_model,
        } => cleanup_cmd::run(&file, timeout, poll_interval, &fallback_model),
        Commands::Backlog {
            file,
            force_disk,
            action,
        } => agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || {
                agent_doc_element_backlog_io::backlog_cmd::with_force_disk_pending_writes(
                    force_disk,
                    || match action {
                        PendingAction::Add { item } => {
                            agent_doc_element_backlog_io::backlog_cmd::add(&file, &item, false)
                        }
                        PendingAction::AddGated { item } => {
                            agent_doc_element_backlog_io::backlog_cmd::add(&file, &item, true)
                        }
                        PendingAction::Remove { target, contains } => {
                            agent_doc_element_backlog_io::backlog_cmd::remove(
                                &file, &target, contains,
                            )
                        }
                        PendingAction::Reap => {
                            agent_doc_element_backlog_io::backlog_cmd::reap(&file)
                        }
                        PendingAction::Backfill => {
                            agent_doc_element_backlog_io::backlog_cmd::backfill(&file)
                        }
                        PendingAction::Done { id } => {
                            agent_doc_element_backlog_io::backlog_cmd::done(&file, &id)
                        }
                        PendingAction::Edit { id, text } => {
                            agent_doc_element_backlog_io::backlog_cmd::edit(&file, &id, &text)
                        }
                        PendingAction::Clear => {
                            agent_doc_element_backlog_io::backlog_cmd::clear(&file)
                        }
                        PendingAction::Reorder { ids } => {
                            let ids: Vec<String> = ids
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            agent_doc_element_backlog_io::backlog_cmd::reorder(&file, &ids)
                        }
                        PendingAction::List => {
                            agent_doc_element_backlog_io::backlog_cmd::list(&file)
                        }
                        PendingAction::ResolveGate { gate_type } => {
                            agent_doc_element_backlog_io::backlog_cmd::resolve_gate(
                                &file, &gate_type,
                            )
                        }
                        PendingAction::SetGateType { id, gate_type } => {
                            agent_doc_element_backlog_io::backlog_cmd::set_gate_type(
                                &file, &id, &gate_type,
                            )
                        }
                        PendingAction::SetVerify { id, spec } => {
                            agent_doc_element_backlog_io::backlog_cmd::set_gate_verify(
                                &file, &id, &spec,
                            )
                        }
                    },
                )
            },
        ),
        Commands::Icebox {
            file,
            force_disk,
            action,
        } => agent_doc_element_backlog_io::with_backlog_command_effects(
            &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
            || {
                agent_doc_element_backlog_io::backlog_cmd::with_force_disk_pending_writes(
                    force_disk,
                    || match action {
                        PendingAction::Add { item } => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_add(&file, &item)
                        }
                        PendingAction::AddGated { item: _ } => {
                            anyhow::bail!(
                                "agent-doc icebox add-gated is not supported; use `agent-doc review add` for gated review work"
                            )
                        }
                        PendingAction::Remove { target, contains } => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_remove(
                                &file, &target, contains,
                            )
                        }
                        PendingAction::Reap => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_reap(&file)
                        }
                        PendingAction::Backfill => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_backfill(&file)
                        }
                        PendingAction::Done { id } => {
                            agent_doc_element_backlog_io::backlog_cmd::done(&file, &id)
                        }
                        PendingAction::Edit { id, text } => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_edit(
                                &file, &id, &text,
                            )
                        }
                        PendingAction::Clear => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_clear(&file)
                        }
                        PendingAction::Reorder { ids } => {
                            let ids: Vec<String> = ids
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            agent_doc_element_backlog_io::backlog_cmd::icebox_reorder(&file, &ids)
                        }
                        PendingAction::List => {
                            agent_doc_element_backlog_io::backlog_cmd::icebox_list(&file)
                        }
                        PendingAction::ResolveGate { gate_type } => {
                            anyhow::bail!(
                                "agent-doc icebox resolve-gate is not supported for parked work (requested gate type `{gate_type}`)"
                            )
                        }
                        PendingAction::SetGateType { id, gate_type } => {
                            anyhow::bail!(
                                "agent-doc icebox set-gate-type is not supported for parked work (requested #{id} -> {gate_type})"
                            )
                        }
                        PendingAction::SetVerify { id, spec: _ } => {
                            anyhow::bail!(
                                "agent-doc icebox set-verify is not supported for parked work (requested #{id})"
                            )
                        }
                    },
                )
            },
        ),
        Commands::Review { action } => match action {
            ReviewAction::UngateTasks { file } => {
                let report = agent_doc_element_backlog_io::with_backlog_command_effects(
                    &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
                    || {
                        agent_doc_element_backlog_io::backlog_cmd::add_ungate_tasks_for_review(
                            &file,
                        )
                    },
                )?;
                println!("  scanned review: {} gated item(s)", report.scanned);
                println!("  added {} backlog ungate task(s)", report.added.len());
                println!("  (skipped {} already-tracked)", report.skipped.len());
                Ok(())
            }
            ReviewAction::List {
                file,
                gate_type,
                tag,
                has_next,
                no_next,
                json,
            } => {
                let filter = agent_doc_element_review::ReviewListFilter {
                    gate_type,
                    tag,
                    has_next: if has_next {
                        Some(true)
                    } else if no_next {
                        Some(false)
                    } else {
                        None
                    },
                };
                let items = agent_doc_element_backlog_io::with_backlog_command_effects(
                    &agent_doc_element_backlog_runtime_io::RUNTIME_BACKLOG_COMMAND_EFFECTS,
                    || agent_doc_element_backlog_io::backlog_cmd::list_review_items(&file, &filter),
                )?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    println!("(no gated review items match)");
                } else {
                    for v in &items {
                        let gate = v
                            .gate_type
                            .as_deref()
                            .map(|g| format!(" [{g}]"))
                            .unwrap_or_default();
                        let tags = if v.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" {}", v.tags.join(" "))
                        };
                        println!("#{}{}  {}{}", v.id, gate, v.summary, tags);
                        if let Some(next) = &v.next {
                            println!("    → NEXT: {next}");
                        }
                    }
                    println!("\n{} gated review item(s)", items.len());
                }
                Ok(())
            }
        },
        Commands::Queue { action } => match action {
            QueueAction::RecoverLost {
                file,
                json,
                max_git_versions,
                restore_patch,
            } => queue_recovery::run(&file, json, max_git_versions, restore_patch.as_deref()),
            QueueAction::Sync { file } => {
                let queue_effects = CliQueueCommandEffects;
                agent_doc_queue_io::queue_cmd::sync_with_effects(&queue_effects, &file)
            }
            QueueAction::Consume {
                file,
                count,
                force_disk,
                id,
                ack_id,
            } => {
                let queue_effects = CliQueueCommandEffects;
                if let Some(id) = id {
                    agent_doc_queue_io::queue_cmd::consume_orphan_id(&queue_effects, &file, &id)
                } else if let Some(id) = ack_id {
                    agent_doc_queue_io::queue_cmd::acknowledge_open_id(&queue_effects, &file, &id)
                } else {
                    agent_doc_queue_io::queue_cmd::consume_with_options(
                        &queue_effects,
                        &file,
                        count,
                        agent_doc_queue_io::queue_cmd::ConsumeOptions { force_disk },
                    )
                }
            }
            QueueAction::PruneNoise { file } => {
                let queue_effects = CliQueueCommandEffects;
                agent_doc_queue_io::queue_cmd::prune_noise(&queue_effects, &file)
            }
        },
        Commands::ResolveGateCmd { gate_type, scope } => {
            // Determine scan root: explicit --scope, or cwd, or project root
            let scan_root = if let Some(s) = scope {
                s
            } else {
                let cwd = std::env::current_dir()?;
                agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd)
            };
            let total = agent_doc_element_backlog_io::backlog_cmd::resolve_gate_scan(
                &gate_type, &scan_root,
            )?;
            if total == 0 {
                eprintln!(
                    "[resolve-gate] no [/{}] items found under {}",
                    gate_type,
                    scan_root.display()
                );
            } else {
                eprintln!(
                    "[resolve-gate] resolved {} total [/{}] item(s)",
                    total, gate_type
                );
            }
            Ok(())
        }
        Commands::Callback { action } => match action {
            CallbackAction::Request {
                file,
                operations,
                context,
                ttl,
            } => {
                let ops: Vec<&str> = operations.split(',').map(|s| s.trim()).collect();
                let request =
                    agent_doc_callback_io::create_request(&file, &ops, context.as_deref(), ttl)?;
                println!("{}", serde_json::to_string_pretty(&request)?);
                Ok(())
            }
            CallbackAction::Read { file } => {
                match agent_doc_callback_io::read_request(&file)? {
                    Some(request) => {
                        println!("{}", serde_json::to_string_pretty(&request)?);
                    }
                    None => {
                        println!("{{}}");
                        eprintln!("[callback] no pending request for {}", file.display());
                    }
                }
                Ok(())
            }
            CallbackAction::Respond {
                file,
                request_id,
                status,
                summary,
            } => {
                agent_doc_callback_io::write_response(&file, &request_id, &status, &summary, None)?;
                eprintln!("[callback] response written for request {}", request_id);
                Ok(())
            }
            CallbackAction::Gc { root } => {
                let cwd = std::env::current_dir()?;
                let root_path = root
                    .map(PathBuf::from)
                    .or_else(|| agent_doc_fs::find_project_root(&cwd))
                    .context("could not find project root")?;
                agent_doc_callback_io::cleanup_expired(&root_path, 300)
            }
        },
    }
}

#[cfg(test)]
mod terminal_error_report_tests {
    use super::*;

    fn bare_lf_positions(s: &str) -> Vec<usize> {
        s.as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(idx, byte)| {
                if *byte == b'\n' && (idx == 0 || s.as_bytes()[idx - 1] != b'\r') {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn top_level_error_report_uses_crlf_for_anyhow_cause_chain() {
        let err = Err::<(), _>(anyhow::anyhow!("Connection reset by peer (os error 104)"))
            .context("failed to read project controller response")
            .unwrap_err();
        let mut rendered = Vec::new();

        write_terminal_error_report(&mut rendered, &format!("Error: {err:?}")).unwrap();

        let rendered = String::from_utf8(rendered).unwrap();
        assert!(rendered.starts_with("Error: failed to read project controller response"));
        assert!(rendered.contains("\r\n\r\nCaused by:\r\n"));
        assert!(
            bare_lf_positions(&rendered).is_empty(),
            "terminal report must not contain bare LF newlines: {rendered:?}"
        );
        assert!(rendered.ends_with("\r\n"));
    }

    #[test]
    fn top_level_error_report_trims_terminal_reset_carriage_return() {
        let mut rendered = Vec::new();

        write_terminal_error_report(&mut rendered, "Error: first\r\nsecond\r").unwrap();

        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "Error: first\r\nsecond\r\n"
        );
    }
}

#[cfg(test)]
mod recycle_force_tests {
    use super::*;

    /// `#recycleforce` — `--force` is a real flag (no `-- ` separator) and composes
    /// with the single-project form and `--all-projects`.
    ///
    /// The full `Cli` clap tree is large enough that the derive-generated parser
    /// overflows a test thread's default 2 MiB stack in a debug build (it parses
    /// fine on `main`'s 8 MiB stack). Parse on an explicit large-stack thread so the
    /// CLI-surface assertions are reliable.
    fn parse(args: &[&str]) -> Commands {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || Cli::try_parse_from(&owned).expect("parse").command)
            .expect("spawn parse thread")
            .join()
            .expect("parse thread")
    }

    #[test]
    fn force_flag_parses_without_separator() {
        // `agent-doc admin recycle --force` — directly, no `-- --force`.
        let cmd = parse(&["agent-doc", "admin", "recycle", "--force"]);
        match cmd {
            Commands::Admin {
                action:
                    AdminAction::Recycle {
                        force,
                        all_projects,
                        target,
                        ..
                    },
            } => {
                assert!(force, "--force must be honored directly");
                assert!(!all_projects);
                assert!(target.is_none());
            }
            _ => panic!("expected admin recycle subcommand"),
        }
    }

    #[test]
    fn all_projects_recycle_json_reports_supervisor_fanout() {
        // #recycle-supervisor-fanout: an explicit `admin recycle --all-projects` must
        // schedule a recycle of every valid-state route-owned supervisor in addition to
        // the controllers, and report both fan-out counts.
        let v = all_projects_recycle_json(2, 1, 3, 0, true);
        assert_eq!(v["recycled"], 2);
        assert_eq!(v["skipped"], 1);
        assert_eq!(v["supervisors_marked"], 3);
        assert_eq!(v["supervisors_skipped"], 0);
        assert_eq!(v["forced"], true);
    }

    #[test]
    fn force_composes_with_all_projects() {
        let cmd = parse(&["agent-doc", "admin", "recycle", "--all-projects", "--force"]);
        match cmd {
            Commands::Admin {
                action:
                    AdminAction::Recycle {
                        force,
                        all_projects,
                        ..
                    },
            } => {
                assert!(force);
                assert!(all_projects);
            }
            _ => panic!("expected admin recycle subcommand"),
        }
    }

    #[test]
    fn force_composes_with_target() {
        let cmd = parse(&["agent-doc", "admin", "recycle", "plan.md", "--force"]);
        match cmd {
            Commands::Admin {
                action:
                    AdminAction::Recycle {
                        force,
                        target,
                        all_projects,
                        ..
                    },
            } => {
                assert!(force);
                assert!(!all_projects);
                assert_eq!(target.as_deref(), Some(Path::new("plan.md")));
            }
            _ => panic!("expected admin recycle subcommand"),
        }
    }

    #[test]
    fn default_recycle_has_no_force() {
        // The `--force` *flag* still defaults to false; only the busy-pane interrupt
        // depends on it. Escalation to cold-start no longer requires it
        // (`#recycle-no-boundaries`).
        let cmd = parse(&["agent-doc", "admin", "recycle"]);
        match cmd {
            Commands::Admin {
                action: AdminAction::Recycle { force, .. },
            } => assert!(!force, "the --force flag default must remain false"),
            _ => panic!("expected admin recycle subcommand"),
        }
    }

    #[test]
    fn escalation_triggers_on_no_live_controller_with_a_document_path() {
        let doc = Path::new("plan.md");
        // `#recycle-no-boundaries`: no live controller + a session-document path →
        // escalate to a cold-start automatically, with or without `--force`.
        assert!(recycle_should_escalate_dead_supervisor(false, Some(doc)));
        // A live controller answered (recycled==true) → no escalation; the normal
        // recycle path handled it.
        assert!(!recycle_should_escalate_dead_supervisor(true, Some(doc)));
        // No positional target (e.g. `--project-root` form) → nothing to cold-start.
        assert!(!recycle_should_escalate_dead_supervisor(false, None));
    }

    #[test]
    fn escalation_rejects_a_directory_target() {
        // A bare project-root directory is not a session document; do not feed it to
        // `session restart-supervisor`.
        let dir = std::env::temp_dir();
        assert!(dir.is_dir());
        assert!(!recycle_should_escalate_dead_supervisor(
            false,
            Some(dir.as_path())
        ));
    }

    #[test]
    fn session_missing_supervisor_classifier_matches_real_clear_refusals() {
        // #supresilience Part C — the strings below are copied verbatim from the real
        // `session_actor_cmd::clear` / `ensure_supervisor_socket` refusal paths in
        // `src/session_actor_cmd.rs`. The classifier must fire on each one so the
        // ensure-or-cold-start escalation actually triggers in production.

        // ensure_supervisor_socket (no socket at all).
        let no_socket = anyhow::anyhow!(
            "no live supervisor socket for /repo/plan.md (expected /repo/.agent-doc/supervisor-abc.sock)"
        );
        assert!(session_error_is_missing_supervisor(&no_socket));

        // send_command context — a STALE socket left by a crashed supervisor (the
        // Part B crash case), surfaced through the anyhow context chain.
        let stale_socket = Err::<(), _>(anyhow::anyhow!("Connection refused (os error 111)"))
            .context("failed to contact supervisor for /repo/plan.md")
            .unwrap_err();
        assert!(session_error_is_missing_supervisor(&stale_socket));

        // legacy clear IPC + no live pane fallback.
        let legacy_no_pane = anyhow::anyhow!(
            "supervisor does not support clear IPC and no live pane is available for direct `/clear` submission for /repo/plan.md: legacy clear unsupported"
        );
        assert!(session_error_is_missing_supervisor(&legacy_no_pane));

        // NEGATIVE — the controller-connect timeout must NOT escalate (the escalation
        // path hits the same timeout and cannot help).
        let controller_timeout = anyhow::anyhow!(
            "timed out waiting for project controller at /repo/.agent-doc/controller.sock"
        );
        assert!(!session_error_is_missing_supervisor(&controller_timeout));

        // NEGATIVE — the turn-scoped no-op report (real `cancel_turn` wording) must
        // stay fail-closed; never cold-start a supervisor to cancel a nonexistent turn.
        let no_turn = anyhow::anyhow!(
            "No active turn to cancel for /repo/plan.md (harness is idle; not sending an interrupt)."
        );
        assert!(!session_error_is_missing_supervisor(&no_turn));

        // NEGATIVE — an unrelated error must not escalate.
        let unrelated = anyhow::anyhow!("failed to read /repo/plan.md");
        assert!(!session_error_is_missing_supervisor(&unrelated));
    }
}
