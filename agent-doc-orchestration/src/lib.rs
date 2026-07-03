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
pub use agent_doc_crdt_relay_io as crdt_relay_host;
pub mod flow;
pub mod git;
pub use agent_doc_run_context_io as graph;
pub mod preflight;
pub use agent_doc_queue_io::queue_continuation;
pub mod repair;
pub mod route;
pub mod run;
pub mod session_check;
pub mod start;
pub mod write;

pub(crate) struct BacklogCommandEffects;

pub(crate) static BACKLOG_COMMAND_EFFECTS: BacklogCommandEffects = BacklogCommandEffects;

impl agent_doc_element_backlog_io::BacklogCommandEffects for BacklogCommandEffects {
    fn converge_or_disk_write(
        &self,
        file: &std::path::Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        crate::write::converge_or_disk_write(file, current_content, target_content, reason)
    }

    fn record_document_write_provenance(&self, file: &std::path::Path, content: &str) {
        crate::write::record_document_write_provenance(file, content);
    }
}

pub(crate) struct PipelineFrontmatterEffects;

pub(crate) const PIPELINE_FRONTMATTER_EFFECTS: PipelineFrontmatterEffects =
    PipelineFrontmatterEffects;

impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
    for PipelineFrontmatterEffects
{
    fn converge_or_disk_write(
        &self,
        file: &std::path::Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> anyhow::Result<()> {
        crate::write::converge_or_disk_write(file, current_content, target_content, reason)
    }

    fn log_op(&self, file: &std::path::Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

pub(crate) struct SessionActorWriteQueueSubmitter;

pub(crate) static SESSION_ACTOR_WRITE_QUEUE: SessionActorWriteQueueSubmitter =
    SessionActorWriteQueueSubmitter;

impl agent_doc_queue_io::write_queue::DocumentWriteQueueSubmitter
    for SessionActorWriteQueueSubmitter
{
    fn submit<R, F>(
        &self,
        base_dir: &std::path::Path,
        file: &str,
        kind: agent_doc_document_realtime::session_ops::SessionOpKind,
        job: F,
    ) -> anyhow::Result<R>
    where
        R: Send + 'static,
        F: FnOnce() -> R + Send + 'static,
    {
        let actor = agent_doc_session_actor_io::document_actor_in(base_dir, file);
        actor.submit(kind, move |_ctx| job())
    }
}

fn load_active_capture_for_hooks(
    file: &std::path::Path,
) -> Result<Option<agent_doc_hooks_io::PostResponseCapture>, String> {
    agent_doc_capture_io::load_active(file)
        .map(|capture| {
            capture.map(|capture| agent_doc_hooks_io::PostResponseCapture {
                capture_id: capture.capture_id,
                response_sha256: capture.response_sha256,
                response_body: capture.response_body,
            })
        })
        .map_err(|err| err.to_string())
}

fn capture_tsift_memory_closeout_for_hooks(file: &std::path::Path, response_body: &str) {
    let _ = agent_doc_memory_io::closeout::capture_tsift_memory_closeout(file, response_body);
}

fn reap_local_model_leases_for_hooks(file: &std::path::Path) {
    let _ = agent_doc_lease_io::local_model::reap_local_model_leases(file);
}

fn reap_stale_editor_consumers_for_hooks(
    file: &std::path::Path,
) -> agent_doc_hooks_io::StaleConsumerReapCounts {
    let counts = agent_doc_plugin_owner_io::stale_cleanup::reap_stale_jetbrains_for_file(file);
    agent_doc_hooks_io::StaleConsumerReapCounts {
        consumer_patches: counts.consumer_patches,
        live_buffers: counts.live_buffers,
    }
}

pub(crate) fn post_response_hook_effects() -> impl agent_doc_hooks_io::PostResponseHookEffects {
    agent_doc_hooks_io::post_response_hook_effects(
        load_active_capture_for_hooks,
        capture_tsift_memory_closeout_for_hooks,
        reap_local_model_leases_for_hooks,
        reap_stale_editor_consumers_for_hooks,
    )
}

#[cfg(test)]
mod test_support;
