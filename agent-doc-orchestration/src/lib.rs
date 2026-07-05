//! # agent-doc-orchestration
//!
//! The orchestration layer of agent-doc: routing, git, sessions, IPC,
//! process supervision, and tmux sync. Sits between the CLI shell
//! (`agent-doc`) and the focused domain crates.
//!
//! Extraction tracked under `#adoc-orchestration-crate` / `#bz6s`. See
//! `tasks/agent-doc/plan-agent-doc-orchestration-extraction.md` for the wave
//! plan.
//!
//! Wave 0 (scaffold) + Wave 1a: `ipc_socket` (the one dependency-free leaf).
//! Direction A, increment 2: `secret_redact`; environment expansion now lives
//! in `agent-doc-config`.
//! Direction A, increment 3: global config now lives in `agent-doc-config`.
//! Project config file I/O lives in `agent-doc-project-config-io`.
//! Direction A, increment 4: `ops_log` — best-effort operational logging.
//! Pulled project-root discovery and optional file reads into `agent-doc-fs`
//! so effectful adapters no longer reach through `snapshot`.
//! Direction A, increment 5: `input_diag` — structured tmux/supervisor input
//! diagnostic emission. Pure formatting/hash/gating policy now lives in
//! `agent-doc-tmux-commands`; orchestration keeps only stderr/ops-log adapters.
//! Direction A, increment 6 (big-bang): the entire entangled cluster +
//! sessions/supervisor + neighbors moved in one migration. Orchestration now
//! depends on focused crates directly for extracted document, merge, turn, and
//! realtime policy.
//!
//! The next boundary is to retire this crate as an authority holder. Pure
//! document projection lives in `agent-doc-document`, document authority
//! scheduling should move into `agent-doc-document-realtime`, turn lifecycle
//! state lives in `agent-doc-turn`, shared turn-executor vocabulary in
//! `agent-doc-turn-executor`, shared tmux facts/effects in the
//! `agent-doc-tmux` crate family, and tmux-to-turn readiness in
//! `agent-doc-turn-executor-tmux`. This crate remains a transitional adapter
//! for harness, git, editor, and remaining command ports while those ports are
//! split into narrower crates.

// The orchestration cluster + sessions/supervisor + neighbors (increment 6).
pub mod codex_hook;
pub mod preflight;
pub mod repair;
pub mod route;
pub mod start;
pub mod write;

pub use agent_doc_document_realtime_io::{
    RUNTIME_PIPELINE_FRONTMATTER_EFFECTS as PIPELINE_FRONTMATTER_EFFECTS,
    RuntimePipelineFrontmatterEffects as PipelineFrontmatterEffects,
};

pub struct OrchestrationRepairReplayWriteEffects;

pub static REPAIR_REPLAY_WRITE_EFFECTS: OrchestrationRepairReplayWriteEffects =
    OrchestrationRepairReplayWriteEffects;

pub(crate) fn repair_coordinator_effects() -> agent_doc_repair_io::RepairCoordinatorEffects<
    'static,
    agent_doc_closeout_runtime_io::RuntimeRepairIoEffects,
    OrchestrationRepairReplayWriteEffects,
> {
    agent_doc_repair_io::RepairCoordinatorEffects {
        repair_io_effects: &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
        replay_write_effects: &REPAIR_REPLAY_WRITE_EFFECTS,
        complete_required_closeout: repair_complete_required_closeout,
        inspect_session: repair_inspect_session,
        recover_missing_committed_head_response:
            agent_doc_repair_runtime_io::recover_missing_committed_head_response,
        recover_dedupe_only_drift: agent_doc_repair_runtime_io::recover_dedupe_only_drift,
    }
}

fn repair_complete_required_closeout(file: &std::path::Path) -> anyhow::Result<bool> {
    agent_doc_flow_io::closeout::complete_required_closeout(
        file,
        &agent_doc_closeout_runtime_io::closeout_effects(),
    )
}

fn repair_inspect_session(
    file: &std::path::Path,
) -> anyhow::Result<agent_doc_session_check_io::SessionCheckStatus> {
    agent_doc_session_check_io::inspect(
        file,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
}

impl agent_doc_repair_io::RepairReplayWriteEffects for OrchestrationRepairReplayWriteEffects {
    fn run_strict_write_replay(
        &self,
        file: &std::path::Path,
        response: &str,
        is_template: bool,
        is_stream: bool,
        force_disk: bool,
        queue_completion_ids: &[String],
    ) -> anyhow::Result<()> {
        let commit_mode = if agent_doc_git_io::status::is_in_git_repo(file) {
            agent_doc_write_command_io::CommitMode::Required
        } else {
            agent_doc_write_command_io::CommitMode::None
        };
        crate::write::run_command_with_response(
            agent_doc_write_command_io::CommandOptions::repair_replay(
                file,
                is_template,
                is_stream,
                force_disk,
                queue_completion_ids,
            ),
            commit_mode,
            response.to_string(),
        )
    }

    fn apply_template_from_string(
        &self,
        file: &std::path::Path,
        response: &str,
        force_disk: bool,
    ) -> anyhow::Result<()> {
        crate::write::apply_template_from_string_with_options(
            file,
            response,
            agent_doc_write_command_io::TemplateApplyOptions { force_disk },
        )
    }

    fn apply_append_from_string(
        &self,
        file: &std::path::Path,
        response: &str,
    ) -> anyhow::Result<()> {
        crate::write::apply_append_from_string(file, response)
    }

    fn strike_recovered_free_text_queue_head(&self, file: &std::path::Path) -> anyhow::Result<()> {
        match agent_doc_queue_io::queue_consume::consume_queue_prompt_force_disk(
            file,
            &agent_doc_document_realtime_io::RUNTIME_QUEUE_CONSUME_WRITEBACK_EFFECTS,
        ) {
            Ok(Some(outcome)) => {
                eprintln!(
                    "[repair] struck consumed free-text queue head (remaining: {})",
                    outcome.remaining
                );
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => Err(err),
        }
    }
}
