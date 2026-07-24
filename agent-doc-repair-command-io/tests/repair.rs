//! # Module: repair
//!
//! ## Spec
//! - Guards against response loss caused by context compaction interrupting the write-back phase (between agent respond and `agent-doc write`).
//! - Pending responses and streamed checkpoints are retained as typed facts in `state.db`.
//! - `run(file)` — canonicalizes the path, checks for a recoverable durable capture,
//!   and applies it if found. Terminal captures (`committed`, `discarded`) are ignored for
//!   replay so later preflights do not repeatedly enter the dedup path after a successful closeout.
//!   Before applying, reads the current document and checks if the response is already present
//!   (dedup guard). If already present, template docs still run binary-owned transcript/tail
//!   normalization before intent cleanup, then `run(file)` returns `RepairOutcome::AlreadyApplied`.
//!   When replaying from a durable capture, requires the current document and snapshot hashes to
//!   still match the captured baseline; otherwise fails closed.
//!   Template/CRDT patchback responses replay through the normal strict stream
//!   write path so recovery reuses the same response capture, materialization,
//!   queue-consumption, snapshot, and commit closeout as `finalize`.
//!   Other template documents replay through the template repair write path
//!   (`write::apply_template_from_string`) even when the captured response is raw text
//!   without `<!-- patch:... -->` fences (for example `compact exchange` closeouts).
//!   Template replay first passes through `replay_guard`; blocked transcript/full-document
//!   payloads are captured under `.agent-doc/repair-blocked`, and sanitized replayable
//!   payloads such as patch bodies extracted from leading commentary are what get written.
//!   Non-template documents use plain append (`write::apply_append_from_string`).
//!   Clears the retained response intent on successful write.
//! - `repair(file)` — runs the same recovery logic as `run(file)` and, when recovery work happened
//!   inside a git repo, immediately attempts `git::commit(file)` so the repaired response crosses
//!   the normal commit boundary instead of waiting for a later `preflight`.
//! - When there is no pending response/capture to replay and a stale open
//!   `preflight_started` cycle contains unresolved prompt-bearing drift, `run(file)` abandons
//!   that empty cycle without committing a placeholder response so the next preflight can start
//!   a fresh cycle for the still-visible prompt. Recent empty cycles still fail closed so a
//!   concurrent live preflight is not stolen.
//! - When there is no pending response/capture to replay, `run(file)` also reaps stale completed
//!   backlog items (`- [x] ...`) that should already have been removed, synchronizing the reap
//!   into the snapshot and `agent:done` archive when present.
//! - When there is no pending response/capture to replay, `run(file)` also normalizes safe
//!   template drift such as a stale `agent:boundary` marker left before an already-answered
//!   exchange turn; the repair repositions the boundary to the true end of the completed turn
//!   and advances the snapshot through the same binary-owned path.
//! - Retained response intent is owned by the state backbone and consumed by
//!   `agent-doc-repair-io` when replaying or cleaning up interrupted writes.
//! - Response replay/application matching is owned by `agent-doc-turn::response_replay`; this module supplies file-backed repair adapters.
//!
//! ## Agentic Contracts
//! - `run(file)` — returns a `RepairOutcome` describing whether nothing happened, the response was replayed, the response was already present, manual tail cleanup was respected, or a stale `preflight_started` lock was repaired. Returns `Err` on I/O failure or if the write-back itself fails.
//! - `repair(file)` — preserves `run(file)` behavior and additionally attempts `git::commit(file)` when the document lives in git and the outcome was not `Noop`.
//! - Retained intent is cleared only after a fully successful write (or dedup detection);
//!   a failed write leaves it available for retry.
//! - Callers (e.g., `preflight`) invoke `run` at session start to surface any orphaned responses before proceeding.
//!
//! ## Evals
//! - no_pending_returns_false: document with no retained capture → run returns Ok(false)
//! - save_and_clear_pending: save then clear → retained intent advances monotonically
//! - recover_append_response: retained plain text response → applied as Assistant section and committed
//! - recover_skips_duplicate_apply: retained response already present → no duplicate write
//! - recover_already_applied_template_canonicalizes_prompt_prefixes: template dedup still restores missing `❯ ` transcript prefixes before cleanup
//! - repair_repositions_stale_boundary_after_answered_turn: no pending response, stale boundary left before an answered turn → boundary moved to tail, snapshot advanced
//! - recover_replays_capture: durable capture → run returns Ok(true)
//! - recover_fails_closed_on_capture_hash_mismatch: durable capture baseline mismatch → run returns Err

use agent_doc_turn::repair::RepairOutcome;
use anyhow::Result;
use std::path::Path;

#[cfg(test)]
fn run(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::run(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::repair(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::repair::{
        AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR, CancelOutcome,
        EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR, STALE_EMPTY_PREFLIGHT_TTL_SECS,
    };
    use std::path::Path;
    use std::process::Command as ProcessCommand;
    use std::time::Duration;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        dir
    }

    fn init_git_repo(root: &Path, tracked: &Path) {
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["init"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", tracked.file_name().unwrap().to_str().unwrap()])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .status()
            .unwrap();
    }

    fn age_cycle_state(file: &Path, age_secs: u64) {
        agent_doc_cycle_state_io::age_current_cycle_for_tests(file, age_secs).unwrap();
    }

    #[test]
    fn no_pending_returns_false() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\n## User\n\nHello\n").unwrap();
        assert_eq!(run(&doc).unwrap(), RepairOutcome::Noop);
    }

    #[test]
    fn repair_materialization_requires_captured_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: build and install status — gpt-5\n\n",
            "- Reinstalled the CLI from this checkout.\n",
            "<!-- /patch:exchange -->\n",
        );
        let malformed = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Verification:\n",
            "- Reinstalled the CLI from this checkout.\n",
            "<!-- /agent:exchange -->\n",
        );

        let err = agent_doc_repair_io::ensure_repair_materialized_response(
            std::path::Path::new("session.md"),
            malformed,
            response,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("orphaned response replay did not materialize captured response"),
            "malformed body-only materialization must fail closed: {err}"
        );
    }

    #[test]
    fn cancel_preflight_cycle_abandons_empty_preflight_immediately() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        // Fresh empty preflight_started cycle (no capture), age irrelevant.
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        assert_eq!(
            agent_doc_repair_io::cancel_preflight_cycle(
                &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
                &doc
            )
            .unwrap(),
            CancelOutcome::Abandoned
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Abandoned);
    }

    #[test]
    fn cancel_preflight_cycle_protects_cycle_with_capture() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        // A response capture exists for this cycle → cancel must not discard it.
        agent_doc_capture_io::capture_response(&doc, "### Re: do — opus-4-8\n\nDone.\n").unwrap();

        assert_eq!(
            agent_doc_repair_io::cancel_preflight_cycle(
                &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
                &doc
            )
            .unwrap(),
            CancelOutcome::Protected
        );
        assert!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open(),
            "a cycle that owns a capture must stay open after cancel"
        );
    }

    #[test]
    fn cancel_preflight_cycle_protects_advanced_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "# Doc\n\n## User\n\nDo the thing\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_applied",
            Some(content),
            Some(content),
        )
        .unwrap();

        assert_eq!(
            agent_doc_repair_io::cancel_preflight_cycle(
                &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
                &doc
            )
            .unwrap(),
            CancelOutcome::Protected
        );
    }

    #[test]
    fn cancel_preflight_cycle_noop_without_open_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "# Doc\n\nNothing\n").unwrap();
        assert_eq!(
            agent_doc_repair_io::cancel_preflight_cycle(
                &agent_doc_closeout_runtime_io::REPAIR_IO_EFFECTS,
                &doc
            )
            .unwrap(),
            CancelOutcome::NoOpenCycle
        );
    }

    #[test]
    fn repair_repositions_stale_boundary_after_answered_turn() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "Can we run specific rubrics for fine tuning?\n",
            "### Re: specific rubrics — gpt-5\n\n",
            "Yes.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("Can we run specific rubrics for fine tuning?"));
        assert!(repaired.contains("### Re: specific rubrics — gpt-5"));
        assert!(
            repaired
                .contains("Yes.\n<!-- agent:boundary:keep-this-id -->\n<!-- /agent:exchange -->"),
            "boundary should move to the true end of the answered turn:\n{repaired}"
        );

        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(repaired_snapshot, repaired);
    }

    #[test]
    fn repair_normalizes_fragmented_inline_boundary_exchange() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: design review — model\n\nOriginal response.\n",
            "I don't see the diagrams.\n",
            "<!-- agent:boundary:turn -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: design review — model\n\nOriginal response.\n",
            "I don't see the diagrams.\n\n",
            "How would CAS work?<!-- agent:boundary:turn --><!-- agent:boundary:turn -->\n",
            "- duplicated partial response line\n\n",
            "### Re: diagrams — model\n\nUse an explicit dark theme.\n",
            "How would CAS wo?\n",
            "### Re: CAS — model\n\nUse a lock around compare-and-delete.\n",
            "<!-- agent:boundary:turn -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "{repaired}"
        );
        let diagrams_response = repaired.find("### Re: diagrams — model").unwrap();
        let complete_cas_prompt = repaired.find("How would CAS work?").unwrap();
        let cas_response = repaired.find("### Re: CAS — model").unwrap();
        assert!(
            diagrams_response < complete_cas_prompt && complete_cas_prompt < cas_response,
            "the complete prompt must follow the response that was in flight and precede its own response:\n{repaired}"
        );
        assert!(!repaired.contains("How would CAS wo?"), "{repaired}");
        assert!(
            !repaired.contains("duplicated partial response line"),
            "{repaired}"
        );
        assert!(
        repaired.contains(
            "Use a lock around compare-and-delete.\n<!-- agent:boundary:turn -->\n<!-- /agent:exchange -->"
        ),
        "{repaired}"
    );
    }

    #[test]
    fn repair_fragmented_exchange_projects_retained_crdt_target_with_zero_editor_replicas() {
        let dir = setup_project();
        let doc = dir.path().join("session-live-authority.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test-live\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: design review — model\n\nOriginal response.\n",
            "I don't see the diagrams.\n",
            "<!-- agent:boundary:turn -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let live_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test-live\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: design review — model\n\nOriginal response.\n",
            "I don't see the diagrams.\n\n",
            "How would CAS work?<!-- agent:boundary:turn --><!-- agent:boundary:turn -->\n",
            "- duplicated partial response line\n\n",
            "### Re: diagrams — model\n\nUse an explicit dark theme.\n",
            "How would CAS wo?\n",
            "### Re: CAS — model\n\nUse a lock around compare-and-delete.\n",
            "<!-- agent:boundary:turn -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let stale_disk_projection =
            live_content.replacen("How would CAS wo?", "How would CAS w?", 1);
        std::fs::write(&doc, stale_disk_projection).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let editor_identity = "intellij:repair-live-authority";
        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            editor_identity,
            live_content,
        );
        let relay_identity = format!(
            "{editor_identity}:{}",
            doc.canonicalize().unwrap().display()
        );
        assert!(
            agent_doc_crdt_relay_io::deregister_replica_for_file(&doc, &relay_identity).unwrap(),
            "the test editor replica must be removed while editor authority remains"
        );

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let authoritative = agent_doc_document_realtime_io::try_resolve_current_document_content(
            &doc,
            "repair_live_authority_test",
        )
        .unwrap();
        assert_eq!(authoritative, repaired);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            repaired
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "{repaired}"
        );
        assert!(!repaired.contains("How would CAS wo?"), "{repaired}");
        assert!(!repaired.contains("How would CAS w?"), "{repaired}");
        assert!(
            !repaired.contains("duplicated partial response line"),
            "{repaired}"
        );
        assert!(
        repaired.contains(
            "Use a lock around compare-and-delete.\n<!-- agent:boundary:turn -->\n<!-- /agent:exchange -->"
        ),
        "{repaired}"
    );
    }

    #[test]
    fn repair_reorders_response_before_prompt_tail_when_pending_response_is_visible() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let response = "### Re: timeout fallback — gpt-5\n\nDone.\n";
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please handle the timeout fallback.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please handle the timeout fallback.\n",
            "### Re: timeout fallback — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "Can you preserve the second paragraph too?\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let prompt_tail = repaired
            .find("Can you preserve the second paragraph too?")
            .unwrap();
        let response_heading = repaired.find("### Re: timeout fallback").unwrap();
        let boundary = repaired.find("<!-- agent:boundary:").unwrap();
        let close = repaired.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            prompt_tail < response_heading,
            "repair should move prompt tail before response:\n{repaired}"
        );
        assert!(
            response_heading < boundary && boundary < close,
            "boundary should close the repaired response turn:\n{repaired}"
        );
    }

    #[test]
    fn repair_does_not_move_boundary_past_unanswered_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:keep-this-id -->\n",
            "❯ Can we run specific rubrics for fine tuning?\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::Noop);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn save_and_clear_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, "response text").unwrap();
        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_some()
        );

        agent_doc_repair_io::pending::clear_pending(&doc).unwrap();
        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn repair_reaps_completed_backlog_without_pending_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));
        assert!(repaired.contains("[#bbbb] drop"));

        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(repaired_snapshot.contains("- [ ] [#aaaa] keep"));
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));
        assert!(repaired_snapshot.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_backfills_legacy_done_ids_before_reaping_completed_backlog() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "- [x] legacy drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let pending_body = repaired
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("pending component");
        assert!(
            repaired.contains("- [ ] [#"),
            "open legacy item should be backfilled: {repaired}"
        );
        assert!(repaired.contains("keep"));
        assert!(!pending_body.contains("legacy drop"));
        assert!(repaired.contains("legacy drop"));

        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let snapshot_pending_body = repaired_snapshot
            .split("<!-- agent:pending -->\n")
            .nth(1)
            .and_then(|rest| rest.split("\n<!-- /agent:pending -->").next())
            .expect("snapshot pending component");
        assert!(repaired_snapshot.contains("- [ ] [#"));
        assert!(!snapshot_pending_body.contains("legacy drop"));
        assert!(repaired_snapshot.contains("legacy drop"));
        assert!(repaired_snapshot.contains("agent:done"));
    }

    #[test]
    fn repair_commits_reaped_completed_backlog_in_git_repo() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] keep\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        match agent_doc_session_check_io::inspect(
            &doc,
            &agent_doc_closeout_runtime_io::session_check_effects(),
        )
        .unwrap()
        {
            agent_doc_session_check_io::SessionCheckStatus::Ok(_) => {}
            other => panic!("expected clean closeout after repair, got {other:?}"),
        }

        let head = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["show", "HEAD:test.md"])
            .output()
            .unwrap();
        let head_text = String::from_utf8_lossy(&head.stdout);
        assert!(head_text.contains("- [ ] [#aaaa] keep"));
        assert!(!head_text.contains("- [x] [#bbbb] drop"));
        assert!(head_text.contains("[#bbbb] drop"));
    }

    #[test]
    fn repair_completed_backlog_reap_preserves_live_prompt_outside_snapshot() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        let live_content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "do #statusws. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [x] [#bbbb] drop\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, live_content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot_content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::CompletedBacklogReaped);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("do #statusws. spec-test-build-install-commit-push"));
        assert!(!repaired.contains("- [x] [#bbbb] drop"));

        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            !repaired_snapshot.contains("do #statusws. spec-test-build-install-commit-push"),
            "snapshot must not absorb the live prompt"
        );
        assert!(!repaired_snapshot.contains("- [x] [#bbbb] drop"));

        let diff = agent_doc_diff_io::compute(
            &agent_doc_snapshot_io::DiffBaselineStore::new(agent_doc_ops_log_io::log_op),
            &doc,
        )
        .unwrap()
        .unwrap();
        assert!(diff.contains("do #statusws. spec-test-build-install-commit-push"));
    }

    #[test]
    fn recover_append_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();

        // Save a pending response
        agent_doc_repair_io::pending::save_pending(&doc, "This is the recovered response.")
            .unwrap();

        // Recover it
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        // Verify the response was written
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("This is the recovered response."));
        assert!(result.contains("## Assistant"));

        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn repair_strikes_consumed_free_text_queue_head() {
        // #repair-strike-consumed-head: a recovered free-text-head response must
        // strike its queue head (finalize does; repair historically left it live,
        // so preflight re-presented the answered head).
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- improve the docs please\n",
            "- a second queued item\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(
            &doc,
            "<!-- patch:exchange -->\n### Re: improve the docs — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        // The answered head is struck in place; the next item stays live.
        assert!(
            result.contains("~improve the docs please~"),
            "free-text queue head must be struck after repair replay:\n{result}"
        );
        assert!(
            result.contains("- a second queued item"),
            "the next queue item must remain live:\n{result}"
        );
    }

    #[test]
    fn repair_replay_preserves_response_leading_code_fence_after_prompt_fence() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(
            &doc,
            "<!-- patch:exchange -->\n```\nresponse body\n```\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();
        assert_eq!(
            exchange.matches("```").count(),
            4,
            "repair replay must preserve prompt and response fences:\n{exchange}"
        );
        assert!(
            exchange.contains("```\n```\nresponse body\n```"),
            "repair replay stripped the response opening fence:\n{exchange}"
        );
    }

    #[test]
    fn repair_leaves_do_id_queue_head_for_reap_path() {
        // do[#id] heads are struck by preflight's reap path once their backlog
        // item resolves; the repair strike must NOT touch them, or the head
        // desyncs from its still-open backlog id.
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#widget]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(
            &doc,
            "<!-- patch:exchange -->\n### Re: do [#widget] — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        run(&doc).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- do [#widget]") && !result.contains("~do [#widget]~"),
            "do[#id] head must remain for the reap path:\n{result}"
        );
    }

    #[test]
    fn recover_plain_response_uses_template_path_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(
            &doc,
            "Exchange compacted. No new work was run in this turn.",
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let summary = result
            .find("Exchange compacted. No new work was run in this turn.")
            .unwrap();
        assert!(
            summary < exchange_close,
            "plain recovery for template docs should stay inside exchange:\n{result}"
        );
        assert!(
            !result[exchange_close..].contains("## Assistant"),
            "template recovery must not append inline assistant blocks after exchange:\n{result}"
        );
    }

    #[test]
    fn recover_plain_response_uses_strict_template_patch_in_git_repo() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ recover the captured response\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let response = "### Re: captured response — gpt-5\n\nRecovered through strict write.\n";
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();
        assert_eq!(run(&doc).unwrap(), RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = result.find("<!-- /agent:exchange -->").unwrap();
        let response_at = result.find("### Re: captured response — gpt-5").unwrap();
        assert!(
            response_at < exchange_close,
            "response escaped exchange:\n{result}"
        );
        assert_eq!(
            result.matches("### Re: captured response — gpt-5").count(),
            1
        );
        let head = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["show", "HEAD:test.md"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&head.stdout).contains("Recovered through strict write."));
    }

    #[test]
    fn recover_historical_capture_uses_partial_proof_then_commits_full_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ fix partial response recovery\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();

        let response = concat!(
            "### Re: partial response recovery — gpt-5\n\n",
            "The complete response is durable.\n\n",
            "- editor-buffer save uses the live target;\n",
            "- snapshot staging uses the committed target;\n",
            "- relay convergence stays live.\n",
        );
        agent_doc_capture_io::capture_response_with_current_content(&doc, response, baseline)
            .unwrap();
        agent_doc_capture_io::mark_committed_with_current_content(&doc, baseline).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "incorrect_legacy_commit_boundary",
            Some(baseline),
            Some(baseline),
        )
        .unwrap();
        let partial = baseline
            .replace(
                "❯ fix partial response recovery",
                "❯ a newer unrelated prompt remains operator-owned",
            )
            .replace(
                "<!-- agent:boundary:abc123 -->",
                concat!(
                    "- relay convergence stays live.\n",
                    "- snapshot staging uses the committed target;\n",
                    "- editor-buffer save uses the live target;\n",
                    "<!-- agent:boundary:abc123 -->",
                ),
            );
        std::fs::write(&doc, &partial).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &partial,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(run(&doc).unwrap(), RepairOutcome::ReplayedResponse);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("The complete response is durable."));
        assert!(result.contains("a newer unrelated prompt remains operator-owned"));
        for line in [
            "- editor-buffer save uses the live target;",
            "- snapshot staging uses the committed target;",
            "- relay convergence stays live.",
        ] {
            assert_eq!(
                result.matches(line).count(),
                1,
                "duplicate line in:\n{result}"
            );
        }
        let head = ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["show", "HEAD:test.md"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&head.stdout).contains("The complete response is durable.")
        );
    }

    #[test]
    fn recover_normalizes_captured_replace_pending_patch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] [#aaaa] existing\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n\n",
            "Recovered.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- replace:pending -->\n",
            "- [x] [#aaaa] existing\n",
            "- [ ] [#bbbb] add regression coverage\n",
            "<!-- /replace:pending -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: topic — gpt-5"));
        assert!(result.contains("- [x] [#aaaa] existing"));
        assert!(result.contains("- [ ] [#bbbb] add regression coverage"));
        assert!(!result.contains("replace:pending"));
    }

    #[test]
    fn empty_pending_cleaned_up() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, "").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::Noop);

        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn recover_skips_duplicate_apply() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Document already contains the response content (as if IPC applied it)
        let response = "This is the response that was already applied.\nSecond line.\nThird line.";
        let content = format!(
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\n{}\n\n## User\n\n",
            response
        );
        std::fs::write(&doc, &content).unwrap();

        // Pending file still exists (clear_pending was never called after IPC write)
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        // run should detect the content is already present and skip
        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        // Document should be unchanged
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);

        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn recover_replays_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        agent_doc_repair_io::pending::save_pending(&doc, "Recovered from capture.").unwrap();
        agent_doc_repair_io::pending::clear_pending(&doc).unwrap();
        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none()
        );
        // Re-arm capture as if the write never happened.
        agent_doc_capture_io::capture_response(&doc, "Recovered from capture.").unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("Recovered from capture."));
    }

    #[test]
    fn binary_owned_resume_commits_one_durable_capture_exactly_once() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_capture_io::capture_response(&doc, "Binary-owned closeout.").unwrap();

        let key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .expect("captured response should expose a durable resume key");
        let outcome = agent_doc_repair_command_io::resume_captured_finalize(&doc, &key);
        assert!(
            matches!(
                outcome,
                agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Committed { .. }
            ),
            "{outcome:?}"
        );
        assert!(matches!(
            agent_doc_repair_command_io::resume_captured_finalize(&doc, &key),
            agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Superseded
        ));

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result.matches("Binary-owned closeout.").count(), 1);
        assert!(matches!(
            agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
                .unwrap()
                .expect("cycle state")
                .phase,
            agent_doc_turn::CyclePhase::Committed
        ));
    }

    #[test]
    fn binary_owned_resume_replays_captured_backlog_edit_plan() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\n",
            "session: test\n",
            "agent_doc_format: append\n",
            "agent_doc_write: merge\n",
            "---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#fix1] original next action\n",
            "<!-- /agent:backlog -->\n\n",
            "## User\n\n",
            "Hello\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let plan = agent_doc_write_command_io::CapturedCloseoutMutationPlan {
            pending_edit: vec!["fix1=narrowed recovery action".to_string()],
            ..Default::default()
        };
        let plan_json = serde_json::to_string(&plan).unwrap();
        agent_doc_capture_io::capture_response_with_current_content_and_intent_and_plan(
            &doc,
            "Recovered response with mutation intent.",
            content,
            Some("Recovered response with mutation intent."),
            Some(&plan_json),
        )
        .unwrap();

        let key = agent_doc_repair_command_io::captured_finalize_resume_key(&doc)
            .unwrap()
            .expect("captured response should expose a durable resume key");
        let outcome = agent_doc_repair_command_io::resume_captured_finalize(&doc, &key);
        assert!(
            matches!(
                outcome,
                agent_doc_repair_command_io::CapturedFinalizeResumeOutcome::Committed { .. }
            ),
            "{outcome:?}"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            result
                .matches("Recovered response with mutation intent.")
                .count(),
            1
        );
        assert!(result.contains("[#fix1] narrowed recovery action"));
        assert!(!result.contains("[#fix1] original next action"));
    }

    #[test]
    fn recover_already_applied_template_canonicalizes_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let response =
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->";
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prior question?\n",
            "Why was this missed?\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("❯ Why was this missed?"),
            "repair should restore the missing prompt prefix:\n{repaired}"
        );
        assert!(
            !repaired.contains("\nWhy was this missed?\n"),
            "bare prompt target should not remain after repair:\n{repaired}"
        );

        let saved_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved_snapshot, repaired,
            "snapshot should advance to the canonicalized repaired document"
        );
    }

    #[test]
    fn recover_already_applied_template_keeps_response_body_unprefixed() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: repair — gpt-5\n\n",
            "First response paragraph.\n\n",
            "Second response paragraph.\n",
            "- Proof line.\n",
            "<!-- /patch:exchange -->"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "First response paragraph.\n\n",
            "Second response paragraph.\n",
            "- Proof line.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::AlreadyApplied);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("\nFirst response paragraph.\n"));
        assert!(repaired.contains("\nSecond response paragraph.\n- Proof line.\n"));
        assert!(
            !repaired.contains("❯ First response paragraph.")
                && !repaired.contains("❯ Second response paragraph.")
                && !repaired.contains("❯ - Proof line."),
            "already-applied response body lines must not be prompt-prefixed:\n{repaired}"
        );

        let saved_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(saved_snapshot, repaired);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn repair_without_pending_strips_response_body_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Response intro.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Response intro.\n\n",
            "Verification passed:\n",
            "❯ - `make check`\n",
            "❯ - `agent-doc write --commit`\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(repaired.contains("\nVerification passed:\n"));
        assert!(repaired.contains("\n- `make check`\n- `agent-doc write --commit`\n"));
        assert!(
            !repaired.contains("❯ - `make check`")
                && !repaired.contains("❯ - `agent-doc write --commit`"),
            "no-pending response tails must not remain prompt-prefixed:\n{repaired}"
        );

        let saved_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(saved_snapshot, repaired);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn repair_without_pending_canonicalizes_bare_prompt_before_existing_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "Why was this missed?\n",
            "### Re: topic — gpt-5\n\n",
            "Body\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::TemplateNormalized);

        let repaired = std::fs::read_to_string(&doc).unwrap();
        assert!(
            repaired.contains("❯ Why was this missed?"),
            "repair should restore the missing prompt prefix even without pending replay:\n{repaired}"
        );
        assert!(
            !repaired.contains("\nWhy was this missed?\n"),
            "bare prompt target should not remain after repair:\n{repaired}"
        );

        let saved_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved_snapshot, repaired,
            "snapshot should advance to the canonicalized repaired document"
        );
    }

    #[test]
    fn recover_fails_closed_on_capture_hash_mismatch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, "Recovered from capture.").unwrap();
        std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "unexpected error: {err}"
        );
    }

    // A wedged WRITE-APPLIED capture whose response vanished while the operator
    // added later steering must adopt that authoritative monotonic cut and replay
    // exactly once. Retiring it would preserve only a forensic copy while losing
    // the response from the commit surface.
    #[test]
    fn rebases_and_replays_write_applied_capture_over_monotonic_steering() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let v1 = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Prior answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do something new\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, v1).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, v1, agent_doc_ops_log_io::log_op)
            .unwrap();
        init_git_repo(dir.path(), &doc);

        // A response that was captured + write-applied but never landed
        // contiguously in the document (the CRDT-intermix / concurrent-edit
        // class). It is absent from both v1 and the drifted v2.
        let lost_response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: do something new — opus-4-8\n\n",
            "The new answer that got lost.\n",
            "<!-- /patch:exchange -->",
        );
        let captured = agent_doc_capture_io::capture_response(&doc, lost_response).unwrap();
        agent_doc_capture_io::mark_write_applied(&doc).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&doc, "write_applied", Some(v1), Some(v1))
            .unwrap();

        // Concurrent user edit drifts the live file off the captured baseline.
        let v2 = v1.replace(
            "- do something new\n",
            "- do something new\n- another unrelated edit\n",
        );
        std::fs::write(&doc, &v2).unwrap();

        let recovered = repair(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::ReplayedResponse,
            "monotonic authoritative drift must replay the retained response"
        );

        // The operator's later steering and the retained response both survive.
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- another unrelated edit"),
            "authoritative steering must survive replay:\n{result}"
        );
        assert!(
            result.contains("The new answer that got lost"),
            "retained response must be replayed:\n{result}"
        );
        assert_eq!(result.matches("The new answer that got lost").count(), 1);

        // State projections follow the replayed cut and the capture remains durable.
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, result, "snapshot must follow the replayed document");
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let capture = agent_doc_capture_io::load_by_id(&doc, &captured.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Committed
        );
        assert_eq!(
            capture.response_body, lost_response,
            "captured body must be preserved for forensics"
        );

        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("capture_replay_baseline_rebased_authoritative_current"),
            "replay must record the authoritative baseline adoption:\n{log}"
        );
        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .expect("repair commits the replayed cut");
        assert_eq!(head, result);
    }

    // A `Captured`-only orphan (write never attempted) must STAY on the
    // conservative fail-closed path even when the baseline drifts unless a
    // superseding visible exchange turn proves the captured response is stale.
    #[test]
    fn captured_only_orphan_on_drift_still_fails_closed() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let v1 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v1).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, v1, agent_doc_ops_log_io::log_op)
            .unwrap();

        let lost =
            "<!-- patch:exchange -->\n### Re: new — opus-4-8\n\nLost.\n<!-- /patch:exchange -->";
        agent_doc_capture_io::capture_response(&doc, lost).unwrap();
        // NOTE: no mark_write_applied — capture stays `Captured`.

        let v2 = v1.replace("Prior.", "Prior, edited.");
        std::fs::write(&doc, &v2).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string().contains("baseline no longer matches"),
            "captured-only orphan must keep failing closed without superseding evidence: {err}"
        );
    }

    // `#stale-capture-captured-only-drift`: supersession proof, not baseline
    // drift, is the safety condition for a `Captured`-only orphan. This covers
    // the already-answered live-exchange wedge where replay would duplicate a
    // stale response even though the capture baseline still matches.
    #[test]
    fn retires_superseded_captured_only_orphan_without_baseline_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let live = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n### Re: new — opus-4-8\n\nThe real landed answer.\n<!-- agent:boundary:def -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, live).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            live,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let stale_duplicate = "<!-- patch:exchange -->\n### Re: new — opus-4-8\n\nLost duplicate.\n<!-- /patch:exchange -->";
        let capture = agent_doc_capture_io::capture_response(&doc, stale_duplicate).unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Captured
        );
        assert!(
            !agent_doc_capture_io::replay_baseline_drifted_with_current_content(
                &doc, &capture, live
            )
            .unwrap(),
            "fixture should prove supersession without relying on baseline drift"
        );

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::StaleCaptureRetired);
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, live, "current document must be preserved verbatim");
        assert!(
            !result.contains("Lost duplicate."),
            "stale captured body must not be replayed:\n{result}"
        );
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let capture = agent_doc_capture_io::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Discarded
        );
        let state = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
            .unwrap()
            .expect("cycle state should remain as terminal recovery proof");
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Abandoned);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("retire_superseded_captured_only_orphan"),
            "superseded captured orphan retirement must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    // `#stale-capture-captured-only-drift`: a `Captured`-only orphan whose
    // baseline drifted IS retired (non-destructively) once there is positive
    // superseding-turn evidence — the captured response's `### Re:` heading is
    // already answered in the live exchange, so the never-written body is a stale
    // duplicate, not the only answer.
    #[test]
    fn retires_superseded_captured_only_orphan_on_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // State when the orphan was captured: the prompt is NOT yet answered.
        let v1 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v1).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, v1, agent_doc_ops_log_io::log_op)
            .unwrap();

        // Capture a response (never written) answering `### Re: new`.
        let lost = "<!-- patch:exchange -->\n### Re: new — opus-4-8\n\nLost duplicate.\n<!-- /patch:exchange -->";
        let capture = agent_doc_capture_io::capture_response(&doc, lost).unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Captured
        );

        // A superseding turn answered the SAME prompt with a DIFFERENT body and
        // drifted the live document off the capture's recorded baseline.
        let v2 = "---\nagent_doc_format: template\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: prior — opus-4-8\n\nPrior.\n### Re: new — opus-4-8\n\nThe real landed answer.\n<!-- agent:boundary:def -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, v2).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::StaleCaptureRetired,
            "a Captured-only orphan whose heading is already answered + drifted must be retired"
        );
        // Current document preserved verbatim; the stale body is not replayed.
        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, v2, "current document must be preserved verbatim");
        assert!(
            !result.contains("Lost duplicate."),
            "stale captured body must not be replayed:\n{result}"
        );
        // Orphan retired (Discarded); body preserved in the state ledger for forensics.
        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let capture = agent_doc_capture_io::load_by_id(&doc, &capture.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Discarded
        );
        assert_eq!(
            capture.response_body, lost,
            "captured body must be preserved for forensics"
        );
        let state = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
            .unwrap()
            .expect("cycle state should remain as terminal recovery proof");
        assert_eq!(
            state.phase,
            agent_doc_turn::CyclePhase::Abandoned,
            "retiring a stale captured-only orphan must terminalize the old cycle"
        );
        assert!(
            state
                .last_event
                .contains("repair_retire_superseded_captured_only_orphan"),
            "abandoned cycle should explain why replay was dropped: {}",
            state.last_event
        );
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("closeout_recovery_mutation")
                && log.contains("retire_superseded_captured_only_orphan"),
            "superseded captured orphan retirement must go through the shared recovery mutation primitive:\n{log}"
        );
    }

    #[test]
    fn recover_respects_manual_removal_of_escaped_exchange_tail() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let malformed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n"
        );
        std::fs::write(&doc, malformed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            malformed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, "Escaped answer.").unwrap();
        let captured = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();

        let repaired = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        std::fs::write(&doc, repaired).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::ManualTailRemovalRespected,
            "manual deletion of the escaped tail should be treated as a repair"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, repaired);
        assert!(
            !result.contains("## Assistant"),
            "stale assistant tail must not be re-added:\n{result}"
        );

        assert!(
            agent_doc_repair_io::load_active_pending_response(&doc)
                .unwrap()
                .is_none(),
            "pending response intent should be cleared"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(
            state.last_event,
            "repair_respect_manual_exchange_tail_removal"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snap, repaired, "snapshot should follow the user repair");

        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let capture = agent_doc_capture_io::load_by_id(&doc, &captured.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Discarded
        );
    }

    #[test]
    fn recover_dedup_with_blank_lines_and_boundary() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        // Response has template patch with content lines
        let response = "<!-- patch:exchange -->\n### Re: topic — opus-4-6\n\n**Details:**\n- Item one\n<!-- /patch:exchange -->";
        // Document has the content with blank lines and (HEAD) boundary suffix
        let content = "---\nsession: test\n---\n\n<!-- agent:exchange -->\n### Re: topic — opus-4-6 (HEAD)\n\n**Details:**\n- Item one\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "should detect content as already applied despite (HEAD) suffix and blank lines"
        );

        let result = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn recover_repairs_stale_preflight_started_cycle_when_hashes_match() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(
            repaired,
            RepairOutcome::StalePreflightLockRepaired,
            "stale preflight lock should be repaired"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_stale_lock");
    }

    #[test]
    fn recover_stale_preflight_cycle_strips_response_body_prompt_prefixes() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Do the repair.\n",
            "### Re: repair — gpt-5\n\n",
            "Verification passed:\n",
            "❯ - `make check`\n",
            "❯ - `agent-doc write --commit`\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::TemplateNormalized);

        let doc_after = std::fs::read_to_string(&doc).unwrap();
        assert!(doc_after.contains("\n- `make check`\n- `agent-doc write --commit`\n"));
        assert!(
            !doc_after.contains("❯ - `make check`")
                && !doc_after.contains("❯ - `agent-doc write --commit`"),
            "stale-preflight repair must canonicalize response-owned proof lines:\n{doc_after}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        let log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("repair_preflight_stale_lock"));
        assert!(log.contains("repair_response_body_prompt_prefix_stripped"));
    }

    #[test]
    fn recover_repairs_stale_empty_preflight_started_cycle_with_frontmatter_only_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "agent_doc_session: test",
            "agent_doc_session: test\nagent: codex",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::StalePreflightLockRepaired);

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "repair_preflight_stale_empty_cycle");
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(base)
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live);
    }

    #[test]
    fn recover_abandons_stale_empty_preflight_started_cycle_with_prompt_drift() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#root-empty-preflight]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::StalePreflightCycleAbandoned);

        let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(after.cycle_id, state.cycle_id);
        assert_eq!(
            after.last_event,
            "repair_preflight_stale_prompt_cycle_abandoned"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(base)
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), live);

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "abandon event should be logged for diagnostics:\n{log}"
        );
    }

    #[test]
    fn stale_preflight_abandonment_stops_original_partial_checkpoint_writer() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let mut writer =
            agent_doc_capture_io::PartialCheckpointWriter::with_interval(&doc, Duration::ZERO);
        assert!(writer.maybe_checkpoint("first partial").unwrap().is_some());

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#staleckpt]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();
        age_cycle_state(&doc, STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::StalePreflightCycleAbandoned);

        let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(after.cycle_id, state.cycle_id);
        assert!(writer.maybe_checkpoint("second partial").unwrap().is_none());

        let loaded = agent_doc_capture_io::latest_partial_checkpoint(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.response_body, "first partial");
        assert_eq!(loaded.checkpoint_count, 1);

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
            "repair abandonment should be logged:\n{log}"
        );
        assert!(
            log.contains("partial_response_checkpoint_stopped"),
            "stale checkpoint writer should stop after repair abandonment:\n{log}"
        );
        assert!(
            log.contains("reason=cycle_closed"),
            "abandoned same-cycle checkpoint stop should be classified as a closed cycle:\n{log}"
        );
    }

    #[test]
    fn recover_fails_closed_on_recent_empty_preflight_started_cycle_with_prompt_drift() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#staleflt]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
        std::fs::write(&doc, &live).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(EMPTY_PREFLIGHT_STARTED_NO_CAPTURE_ERROR));
        assert!(message.contains("prompt_target: do [#staleflt]"));
        assert!(message.contains("no response exists to replay"));

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_eq!(state.last_event, "preflight_started");
    }

    #[test]
    fn recover_does_not_treat_orchestration_handoff_marker_as_missing_response() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let live = base.replace(
            "❯ Please reply\n",
            "❯ Please reply\n\nSynchronous orchestra:\n",
        );
        std::fs::write(&doc, &live).unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::Noop);

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
    }

    #[test]
    fn recover_repairs_preflight_started_cycle_when_committed_patchback_is_visible() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::StalePreflightLockRepaired);

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "capture_committed");
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(updated)
        );
    }

    #[test]
    fn recover_repairs_stale_preflight_cycle_despite_queue_only_churn() {
        // #adoc-queue-ipc-buffer-divergence root cause #4: a committed cycle
        // whose only working-tree drift since preflight start is queue-component
        // churn (auto strip + queue_active toggle from queue maintenance) must
        // still recover via the normalized replay-hash match instead of staying
        // wedged in PreflightStarted (the recurring stuck_captured_cycle symptom).
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test\n- do [#qchurn]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        let state =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        // Only the queue churned (halt: auto stripped + queue_active cleared +
        // body drained). The exchange/response is byte-identical. Commit it so
        // HEAD matches the working tree (the committed steady state).
        let churned = concat!(
            "---\nagent_doc_format: template\nagent_doc_session: test\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "done\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, churned).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            churned,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "queue churn", "--no-verify"])
            .status()
            .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(
            repaired,
            RepairOutcome::StalePreflightLockRepaired,
            "queue-only churn must not block stale-lock recovery"
        );

        let after = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(after.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(after.cycle_id, state.cycle_id);
    }

    #[test]
    fn recover_closes_write_applied_cycle_when_head_already_has_exchange_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(updated)).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(content),
            Some(updated),
        )
        .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::CommitBoundaryRecovered);

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "capture_committed");
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(updated)
        );
    }

    #[test]
    fn repair_already_present_response_closes_as_committed_when_head_matches() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->",
        );
        agent_doc_capture_io::capture_response(&doc, response).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let repaired = run(&doc).unwrap();
        assert_eq!(repaired, RepairOutcome::AlreadyApplied);

        let state = agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
            .unwrap()
            .expect("repair should keep terminal cycle proof");
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .as_deref(),
            Some(updated)
        );
    }

    #[test]
    fn recover_fails_closed_on_ambiguous_preflight_started_patchback_without_artifact() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let updated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();

        let err = run(&doc).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR));
        assert!(message.contains("### Re: topic — gpt-5"));
    }

    #[test]
    fn recover_ignores_committed_capture_without_pending() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\nbody\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_capture_io::capture_response(&doc, "Recovered answer.").unwrap();
        agent_doc_capture_io::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::Noop,
            "committed captures should not trigger replay/dedup on later preflights"
        );
    }

    #[test]
    fn recover_restores_committed_head_when_authority_replays_capture_baseline() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:baseline -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: reply — gpt-5\n\n",
            "Committed response.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_cycle_state_io::start_preflight(&doc, Some(baseline), Some(baseline)).unwrap();
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        assert_eq!(capture.baseline_content.as_deref(), Some(baseline));
        agent_doc_capture_io::mark_committed(&doc).unwrap();
        let committed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: reply — gpt-5\n\n",
            "Committed response.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n"
        );
        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(response, committed)
        );
        std::fs::write(&doc, committed).unwrap();
        ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["add", "test.md"])
            .output()
            .unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "test_committed_response",
            Some(committed),
            Some(committed),
        )
        .unwrap();
        ProcessCommand::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "committed response"])
            .output()
            .unwrap();
        let persisted_capture = agent_doc_capture_io::latest_committed(&doc)
            .unwrap()
            .expect("committed capture should remain available");
        assert_eq!(
            persisted_capture.baseline_content.as_deref(),
            Some(baseline)
        );
        assert!(
            agent_doc_turn::response_replay::response_materialized_in_content(
                &persisted_capture.response_body,
                &agent_doc_git_io::revision::show_head(&doc)
                    .unwrap()
                    .unwrap(),
            )
        );

        // Model the late IDE/CRDT baseline projection that previously made
        // preflight repair delete the committed response and commit the rollback.
        std::fs::write(&doc, baseline).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::Noop);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
        assert_eq!(
            agent_doc_git_io::revision::show_head(&doc)
                .unwrap()
                .unwrap(),
            committed
        );
        assert_eq!(
            committed.matches("<!-- agent:boundary:").count(),
            1,
            "the committed response projection must retain one terminal boundary"
        );
    }

    #[test]
    fn recover_replays_latest_committed_capture_when_matching_prompt_was_left_orphaned() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: code review — gpt-5\n\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_capture_io::mark_committed(&doc).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("❯ #code-review"));
        assert!(result.contains("### Re: code review — gpt-5"));
        assert!(result.contains("Recovered body."));
    }

    #[test]
    fn recover_replays_projected_committed_capture_when_capture_sidecar_missing() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: projected code review - gpt-5\n\n",
            "Recovered from projection.\n",
            "<!-- /patch:exchange -->\n"
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        agent_doc_capture_io::mark_committed(&doc).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &doc,
            "commit_success",
            Some(content),
            Some(content),
        )
        .unwrap();
        assert!(!capture.capture_id.is_empty());

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("❯ #code-review"));
        assert!(result.contains("### Re: projected code review - gpt-5"));
        assert!(result.contains("Recovered from projection."));
    }

    #[test]
    fn recover_repairs_escaped_exchange_tail_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_repair_io::pending::save_pending(&doc, "Recovered answer.").unwrap();
        let recovered = run(&doc).unwrap();
        assert_eq!(
            recovered,
            RepairOutcome::AlreadyApplied,
            "dedup path should skip replay"
        );

        let repaired = std::fs::read_to_string(&doc).unwrap();
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "escaped assistant block should move back inside exchange:\n{repaired}"
        );
    }

    #[test]
    fn recover_fails_closed_on_transcript_shaped_template_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let transcript_dump = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "### Re: topic — gpt-5\n",
            "Body\n",
            "<!-- agent:boundary:def456 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, transcript_dump).unwrap();

        let err = run(&doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("refused to replay pending response"),
            "unexpected error: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&doc).unwrap(),
            content,
            "blocked replay must not mutate the document"
        );

        let blocked_dir = dir.path().join(".agent-doc/repair-blocked");
        let captures: Vec<_> = std::fs::read_dir(&blocked_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .collect();
        assert_eq!(captures.len(), 1, "expected one blocked repair capture");
        let blocked_payload = std::fs::read_to_string(captures[0].path()).unwrap();
        assert!(blocked_payload.contains("agent component markers"));
        assert!(blocked_payload.contains("response_body"));
    }

    #[test]
    fn recover_replays_guard_prefixed_template_patch() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Hello\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let response = concat!(
            "<!-- no-pending-capture -->\n",
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let recovered = run(&doc).unwrap();
        assert_eq!(recovered, RepairOutcome::ReplayedResponse);

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("### Re: topic — gpt-5"));
        assert!(result.contains("Recovered body."));
        assert!(
            !dir.path().join(".agent-doc/repair-blocked").exists(),
            "guard-prefixed patch payload should not be parked as blocked"
        );
    }

    #[test]
    fn repair_crosses_commit_boundary_for_git_backed_replay() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\nagent_doc_format: append\nagent_doc_write: merge\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        agent_doc_repair_io::pending::save_pending(&doc, "This is the recovered response.")
            .unwrap();
        let captured = agent_doc_capture_io::load_active(&doc).unwrap().unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::ReplayedResponse);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("This is the recovered response."),
            "HEAD should contain the recovered response:\n{head}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);

        assert!(agent_doc_capture_io::load_active(&doc).unwrap().is_none());
        let capture = agent_doc_capture_io::load_by_id(&doc, &captured.capture_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.state,
            agent_doc_workflow::capture::CaptureState::Committed
        );
        assert!(
            capture.replayed_at.is_some(),
            "recovered patchback should retain replay provenance"
        );
        assert!(
            capture.committed_at.is_some(),
            "recovered patchback should record the later commit boundary"
        );
    }

    #[test]
    fn repair_crosses_commit_boundary_when_response_already_present() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Recovered answer.\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        agent_doc_repair_io::pending::save_pending(&doc, "Recovered answer.").unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Recovered answer."),
            "HEAD should contain the deduped recovered response:\n{head}"
        );
        let exchange_close = head.find("<!-- /agent:exchange -->").unwrap();
        let assistant = head.find("## Assistant").unwrap();
        assert!(
            assistant < exchange_close,
            "HEAD should keep the repaired assistant content inside exchange:\n{head}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("Recovered answer."),
            "snapshot should be advanced to the recovered response:\n{snap}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_adopts_visible_response_without_pending_when_cycle_never_started() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        let current = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ do [#8zjh]. spec-test-build-install-commit-push\n",
            "### Re: #8zjh — gpt-5\n\n",
            "Recovered from the visible exchange tail.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, current).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Recovered from the visible exchange tail."),
            "HEAD should contain the adopted response:\n{head}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("Recovered from the visible exchange tail."),
            "snapshot should advance to the visible response:\n{snap}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_adopts_visible_response_for_open_agent_doc_write_cycle() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please recover the partial patchback\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();

        let current = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please recover the partial patchback\n",
            "### Re: partial patchback — gpt-5\n\n",
            "Recovered from an agent-doc-owned visible response.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_cycle_state_io::mark_write_applied(
            &doc,
            "write_template",
            Some(base),
            Some(current),
        )
        .unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Recovered from an agent-doc-owned visible response."),
            "HEAD should contain the adopted partial patchback:\n{head}"
        );
        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("Recovered from an agent-doc-owned visible response."),
            "snapshot should advance to the adopted response:\n{snap}"
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_commits_already_present_response_when_snapshot_lags_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(dir.path(), &doc);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        let direct_patch = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did the patchback miss?\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "<!-- /agent:pending -->\n"
        );
        std::fs::write(&doc, direct_patch).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through direct patch.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let outcome = repair(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::AlreadyApplied);

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Recovered through direct patch."),
            "HEAD should own the already-present response after repair:\n{head}"
        );

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            snap.contains("Recovered through direct patch."),
            "snapshot should advance to the already-present response:\n{snap}"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
    }

    #[test]
    fn repair_run_does_not_rewind_committed_cycle_when_replaying_after_commit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nsession: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);

        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: replay after commit — gpt-5\n\n",
            "Recovered answer.\n",
            "<!-- /patch:exchange -->\n"
        );
        agent_doc_repair_io::pending::save_pending(&doc, response).unwrap();

        let outcome = run(&doc).unwrap();
        assert_eq!(outcome, RepairOutcome::ReplayedResponse);

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        let doc_after = std::fs::read_to_string(&doc).unwrap();
        assert!(doc_after.contains("### Re: replay after commit — gpt-5"));
    }

    #[test]
    fn repair_fails_closed_when_only_later_prompt_drift_remains_after_committed_patchback() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("test.md");
        let base = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did repair miss the pending response?\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        init_git_repo(root, &doc);

        let committed_patchback = concat!(
            "---\nagent_doc_session: sid\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Why did repair miss the pending response?\n",
            "### Re: missed patchback — gpt-5\n\n",
            "Recovered through a direct patchback.\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, committed_patchback).unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["add", "test.md"])
            .status()
            .unwrap();
        ProcessCommand::new("git")
            .current_dir(root)
            .args(["commit", "-m", "manual patchback", "--no-verify"])
            .status()
            .unwrap();

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            base,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(base),
            Some(base),
        )
        .unwrap();

        let current = committed_patchback.replace(
            "<!-- /agent:exchange -->\n",
            "do [#followup]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, &current).unwrap();

        let err = repair(&doc).expect_err(
            "repair should fail closed when only later prompt drift remains after adopting the committed patchback",
        );
        let message = err.to_string();
        assert!(message.contains("unresolved prompt-bearing user changes"));
        assert!(message.contains("do [#followup]. spec-test-build-install-commit-push"));

        let repaired_snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert!(
            repaired_snapshot.contains("Recovered through a direct patchback."),
            "snapshot should advance to the committed patchback:\n{repaired_snapshot}"
        );
        assert!(
            !repaired_snapshot.contains("do [#followup]. spec-test-build-install-commit-push"),
            "snapshot must not absorb the later prompt drift:\n{repaired_snapshot}"
        );

        let head = agent_doc_git_io::revision::show_head(&doc)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Recovered through a direct patchback."),
            "HEAD should keep the committed patchback:\n{head}"
        );
        assert!(
            !head.contains("do [#followup]. spec-test-build-install-commit-push"),
            "repair must not commit the later prompt drift:\n{head}"
        );
    }
}
