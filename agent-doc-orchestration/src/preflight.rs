//! # Module: preflight
//!
//! ## Spec
//! - `run(file)`: executes the full pre-agent preparation sequence for a
//!   session document and emits a single JSON object to stdout.
//! - Bails immediately if the file does not exist.
//! - Step 0 — layout check: calls `check_layout()` to detect tmux structural
//!   problems (window index, session drift); issues are
//!   included in output but do not abort the run.
//! - Step 0-pre — interrupted-cycle guard: inspects persisted cycle state.
//!   For any open prior cycle, preflight auto-attempts `agent_doc_repair_io::run` +
//!   `git::commit(file)` before diffing again. For an open `preflight_started`
//!   cycle with no recoverable response and unresolved prompt-bearing drift,
//!   preflight fails closed before the no-op commit path can mark an empty
//!   cycle committed. Non-prompt drift may still use the narrow no-op closeout
//!   that stages the snapshot only and leaves later live working-tree edits
//!   uncommitted; if that closeout still cannot prove the prior cycle is durable,
//!   preflight fails closed instead of diffing again.
//! - Step 1 — repair: calls `agent_doc_repair_io::run` to detect and apply any
//!   orphaned pending agent responses from a previous interrupted cycle.
//! - Step 2 — commit: calls `git::commit(file)` to record the previous
//!   exchange cycle; failure is downgraded to a warning, not a hard error.
//! - Step 3 — claims: reads `.agent-doc/claims.log` line-by-line via
//!   `read_and_truncate_claims`, then truncates the log to empty; claims are
//!   returned to the caller in the JSON output.
//! - Step 3b — debounce: waits up to 3 seconds (polling every 100 ms) for
//!   both the file mtime to be at least 500 ms old and the cross-process
//!   typing indicator to be inactive before proceeding to the diff step.
//! - Step 3c — linked docs: calls `check_linked_docs(file)` to inspect
//!   `links` from frontmatter. For local file links, compares git commit
//!   times against the snapshot mtime. For URL links (`http://`/`https://`),
//!   fetches content via `ureq`, converts HTML to markdown via `htmd`
//!   (stripping script/style/nav/footer/noscript/svg), caches in
//!   `.agent-doc/links_cache/<sha256(url)>.txt`, and reports changes by
//!   comparing against the cached content.
//! - Step 4 - diff: calls `agent_doc_diff_io::compute(...)` to compare the current
//!   document against the last snapshot; `no_changes=true` when they match.
//! - Also emits a bounded `session_accretion` advisory when local exchange/log
//!   heuristics detect churn-heavy growth or restart-heavy reopen patterns.
//! - Serializes `PreflightOutput` as pretty JSON to stdout; all diagnostic
//!   messages go to stderr.
//! - `check_layout()`: inspects the current tmux session for structural issues:
//!   missing window index 0 (base-index compliance) and session drift. Stash
//!   windows may have non-idle panes (backgrounded sessions). Read-only; no mutations.
//!   Returns an empty vec when not inside tmux (silent).
//! - `read_and_truncate_claims(file)`: locates `.agent-doc/claims.log` relative
//!   to the project root, collects non-empty lines, truncates the file to empty,
//!   and returns the lines. Returns empty vec if the log is absent or unreadable.
//!
//! ## Agentic Contracts
//! - All output intended for the SKILL workflow is on stdout as valid JSON;
//!   callers must not parse stderr.
//! - `no_changes=true` in the output means the SKILL workflow should skip
//!   sending to the agent; `diff` will be `null` in this case.
//! - `layout_issues` reports structural tmux issues that remain after any
//!   immediate pre-diff layout repair has run.
//! - The claims log is consumed (truncated) exactly once per `preflight` call;
//!   a second call in the same cycle will return empty claims.
//! - Recovery (`recovered=true`) means the document was modified before the
//!   diff step; the `diff` and `document` fields reflect post-recovery state.
//! - Debounce waits for user typing to settle before computing the diff;
//!   if the 3-second timeout expires, `run` proceeds and logs a warning to
//!   stderr — it never blocks indefinitely.
//! - `check_layout` is always safe to call outside tmux; it returns `[]`.
//!
//! ## Evals
//! - `preflight_produces_valid_json`: document with matching snapshot →
//!   `run` returns `Ok(())` and emits parseable JSON with `no_changes=true`.
//! - `preflight_file_not_found`: missing path → `Err` containing "file not found".
//! - `preflight_detects_diff`: snapshot saved at original content, document
//!   updated with new content → `diff::compute` returns `Some(_)` (non-null diff).
//! - `preflight_claims_read_and_truncated`: claims.log with two entries →
//!   `read_and_truncate_claims` returns both lines and the log is empty afterwards.
//! - `preflight_no_claims_log_returns_empty`: no claims.log present →
//!   `read_and_truncate_claims` returns an empty vec without error.
//! - `preflight_output_serializes_correctly`: `PreflightOutput` with known
//!   values serializes to JSON with correct field names and types.
//! - `preflight_output_null_diff_when_no_changes`: `diff=None` + `no_changes=true`
//!   → JSON has `"diff": null` and `"no_changes": true`.
//! - `check_layout_returns_empty_outside_tmux`: `TMUX` env var unset →
//!   `check_layout()` returns empty vec without invoking tmux.
//! - `check_layout_detects_session_drift`: two alive registered panes in
//!   different sessions → `layout_issues` contains a "session drift" entry.
//! - `preflight_output_includes_layout_issues`: `PreflightOutput` with one
//!   layout issue → JSON `layout_issues` array has length 1 with correct text.
//! - `preflight_output_slash_commands_from_diff`: diff containing `+/clear` →
//!   `builtin_commands` array has one entry `"/clear"` (built-in, not in `slash_commands`).
//! - `is_url_detects_http`: `http://` and `https://` prefixes → true;
//!   relative paths and empty strings → false.
//! - `is_html_content_detects_html`: `text/html` and `application/xhtml` → true;
//!   `application/json` and `text/plain` → false.
//! - `html_to_markdown_converts_basic_html`: `<h1>` and `<strong>` → markdown
//!   heading and bold syntax.
//! - `html_to_markdown_strips_script_and_style`: script/style content removed
//!   from output, visible content preserved.
//! - `html_to_markdown_strips_nav_and_footer`: nav/footer content removed,
//!   main content preserved.
//! - `url_cache_path_is_deterministic`: same URL → same path; different URL →
//!   different path; extension is `.txt`.
//! - `links_cache_dir_creates_directory`: creates `.agent-doc/links_cache/` and
//!   returns `Some(path)` when `.agent-doc/` exists.

use anyhow::Result;
use std::path::Path;

#[cfg(test)]
use agent_doc_preflight_io::layout::detect_duplicate_claims;
use agent_doc_preflight_io::{
    PreflightOutput, PreflightWarning,
    layout::{check_layout, maybe_auto_repair_base_index, maybe_auto_resync_on_drift},
};
use agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionReport;

/// Run the preflight sequence for a session document.
///
/// Steps (in order):
/// 0. Check tmux layout health (`check_layout`)
/// 1. Repair orphaned pending response (`repair::run`)
/// 2. Commit previous cycle (`git::commit`)
/// 3. Check claims log (read + truncate `.agent-doc/claims.log`)
/// 4. Compute diff (`diff::compute`)
/// 5. Read document HEAD from disk
///
/// Outputs JSON to stdout. Progress/diagnostic messages go to stderr.
fn enforce_cycle_completion(file: &Path) -> Result<(bool, bool)> {
    agent_doc_preflight_io::enforce_cycle_completion(
        file,
        &agent_doc_preflight_runtime_io::preflight_cycle_completion_effects(
            agent_doc_repair_runtime_io::repair_coordinator_effects(
                &crate::repair::REPAIR_REPLAY_WRITE_EFFECTS,
            ),
            agent_doc_closeout_runtime_io::session_check_effects(),
        ),
    )
}

mod run;
pub use run::*;

#[cfg(test)]
mod th {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;
    // The source-repo locator accepts the document's git root when it is the
    // `agent-doc` crate, the `src/agent-doc` dogfood submodule layout, and
    // returns `None` (silent no-op) when no `agent-doc` Cargo.toml is present.
    // #per-cycle-protocol-output-overhead: empty Vec fields must not spend
    // per-cycle context bytes. A healthy/default PreflightOutput omits the empty
    // `claims` and `layout_issues` arrays from its JSON, and still round-trips
    // back to empty Vecs (serde default) so consumers reading the struct are safe.
    pub(crate) struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl EnvGuard {
        pub(crate) fn set(key: &'static str, value: &str) -> Self {
            let lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK
                .lock()
                .unwrap();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }
    /// Set up a minimal project directory with .agent-doc/ structure and a git repo.
    pub(crate) fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

        // Initialize a bare git repo so `git commit` doesn't fail fatally.
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
    pub(crate) fn commit_all(root: &Path, message: &str, commit_date: Option<&str>) {
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        let mut commit = Command::new("git");
        commit
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"]);
        if let Some(date) = commit_date {
            commit
                .env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_DATE", date);
        }
        let output = commit.output().unwrap();
        assert!(
            output.status.success(),
            "git commit {message:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    pub(crate) fn initialize_git_head(root: &Path) {
        let readme = root.join("README.md");
        std::fs::write(&readme, "# project\n").unwrap();
        commit_all(root, "initial", None);
    }
    pub(crate) fn write_committed_doc(
        root: &Path,
        rel: &str,
        content: &str,
        message: &str,
        commit_date: Option<&str>,
    ) -> PathBuf {
        let doc = root.join(rel);
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        commit_all(root, message, commit_date);
        doc
    }
    pub(crate) fn write_sessions_json(root: &Path, entries: &[(&str, &str, &Path, &str, &str)]) {
        let mut sessions = serde_json::Map::new();
        for (session_id, pane, file, window, started) in entries {
            sessions.insert(
                (*session_id).to_string(),
                serde_json::json!({
                    "pane": pane,
                    "pid": 9999,
                    "cwd": root.to_string_lossy(),
                    "started": started,
                    "file": file.strip_prefix(root).unwrap().to_string_lossy(),
                    "window": window
                }),
            );
        }
        std::fs::write(
            root.join(".agent-doc/sessions.json"),
            serde_json::to_string_pretty(&serde_json::Value::Object(sessions)).unwrap(),
        )
        .unwrap();
    }
    pub(crate) fn age_cycle_state(file: &Path, age_secs: u64) {
        let canonical = file.canonicalize().unwrap();
        let root = agent_doc_project_root_io::project_root_containing(&canonical).unwrap();
        let hash = agent_doc_fs::document_state_hash(&canonical).unwrap();
        let path = root
            .join(".agent-doc/state/cycles")
            .join(format!("{hash}.json"));
        let mut state: agent_doc_cycle_state_io::CycleState =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        state.started_at = state.started_at.saturating_sub(age_secs);
        state.updated_at = state.updated_at.saturating_sub(age_secs);
        std::fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
    }
    pub(crate) fn write_cycles_log(doc: &Path, entries: &[agent_doc_ops_log_io::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }
    // #opsproof-samecycle-add: a gated review/backlog item added THIS cycle (its
    // text legitimately cites a shipped dependency commit) must NOT be ops-proof
    // auto-completed on the same cycle it first appears — even though the
    // write/finalize path already re-synced the on-disk snapshot to include it,
    // which defeats the snapshot-only same-cycle guard. cycle_state records the
    // added id; the reap must honor it.
    // #opsproof-falsepos: an open actionable backlog item whose completion
    // marker only describes already-landed *dependency* work (a cited commit
    // hash in mid-sentence prose) must NOT be auto-reaped. Only a marker that is
    // the item's own leading status verb proves the item itself is done.
    // #opsproofgate: a live-verify / operator-drive gate that cites a shipped
    // commit hash (e.g. "Code SHIPPED 1edb20d2") in its text must NOT be
    // auto-completed on evidence=commit — even when it has existed for several
    // cycles (not a same-cycle add). Only an anchored structured ops.log marker
    // driven live by the operator may close it.
    // #opsproof-falsepos: never auto-archive an item on the same cycle it is
    // added. A brand-new add is absent from the cycle-start snapshot, so even a
    // leading-status completion marker must not reap it this cycle.
    pub(crate) fn write_ops_log(dir: &TempDir, body: &str) {
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), body).unwrap();
    }
    // --- Fix 5: cross-document sweep ---
    // --- #cce5: resolve_agent_model / short_model_name tests ---
}
#[cfg(test)]
pub(crate) use th::*;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn preflight_output_omits_empty_claims_and_layout_issues() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        assert!(
            !json.contains("\"claims\""),
            "empty claims must be omitted from per-cycle output: {json}"
        );
        assert!(
            !json.contains("\"layout_issues\""),
            "empty layout_issues must be omitted from per-cycle output: {json}"
        );
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert!(round_trip.claims.is_empty());
        assert!(round_trip.layout_issues.is_empty());

        // Non-empty values are still emitted and round-trip intact.
        let populated = PreflightOutput {
            claims: vec!["claimed pane %1".to_string()],
            layout_issues: vec!["stash overflow".to_string()],
            ..PreflightOutput::default()
        };
        let json = serde_json::to_string(&populated).unwrap();
        assert!(json.contains("\"claims\""), "{json}");
        assert!(json.contains("\"layout_issues\""), "{json}");
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.claims, vec!["claimed pane %1".to_string()]);
        assert_eq!(round_trip.layout_issues, vec!["stash overflow".to_string()]);
    }
    #[test]
    fn preflight_detects_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();

        // Save snapshot of original, then add new content.
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n## User\n\nHello\n\nNew question here.\n",
        )
        .unwrap();

        // diff::compute should detect changes → no_changes = false.
        let diff_result = agent_doc_diff_io::compute(
            &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
            &doc,
        )
        .unwrap();
        assert!(diff_result.is_some(), "diff should detect new content");
    }
    #[test]
    fn append_latest_ipc_dogfood_note_reads_matching_ops_log_entry() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let diagnostic = format!(
            "ipc_proof_insufficient file={} source=socket_visible_write patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required",
            canonical.display()
        );
        write_ops_log(
            &dir,
            &format!(
                "older irrelevant line\n[2026-06-23T00:00:00Z] {}\n",
                diagnostic
            ),
        );

        // Gated entry point must NOT append into a non-dogfood document (a user
        // doc that merely sits in a superproject): the diagnostic stays in ops.log.
        assert!(!agent_doc_preflight_io::append_latest_ipc_dogfood_note(&doc).unwrap());
        assert!(
            !std::fs::read_to_string(&doc)
                .unwrap()
                .contains("IPC proof issue dogfood log")
        );

        // The underlying appender (reached only for genuine agent-doc dogfood
        // docs) still folds the diagnostic into the exchange and dedups a repeat.
        assert!(
            agent_doc_preflight_io::append_ipc_dogfood_note_for_diagnostic(&doc, &diagnostic)
                .unwrap()
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("IPC proof issue dogfood log"));
        assert!(updated.contains(&diagnostic));

        assert!(
            !agent_doc_preflight_io::append_ipc_dogfood_note_for_diagnostic(&doc, &diagnostic)
                .unwrap()
        );
    }

    /// #drained-done-queue-clear: a standalone no-diff preflight that drains a
    /// fully-resolved auto-queue writes the drained shape to disk + snapshot
    /// but leaves HEAD on the active-queue commit. The next preflight commit
    /// step must self-heal that pure queue-maintenance drift via the route
    /// queue commit-boundary recovery instead of stranding it for manual
    /// `agent-doc commit`. The drained snapshot has no active prompts, so this
    /// shape recovers only because HEAD proves the prior active auto-queue and
    /// nothing but queue state differs.
    #[test]
    fn route_queue_commit_boundary_recovers_drained_queue_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        agent_doc_snapshot_io::save(&doc, active, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Standalone maintenance drained the queue: queue_active cleared, auto
        // stripped, body emptied — on disk and in the snapshot — but HEAD still
        // carries the active auto-queue.
        let drained = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained).unwrap();
        agent_doc_snapshot_io::save(&doc, drained, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(active),
            Some(active),
        )
        .unwrap();

        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        let rc = agent_doc_run_context_io::RunContext::new(doc.clone());
        assert!(
            agent_doc_preflight_runtime_io::detect_route_queue_snapshot_commit_boundary_recoverable(
                &doc, &rc
            )
            .unwrap(),
            "drained-queue maintenance drift must be recoverable"
        );

        assert!(
            agent_doc_preflight_runtime_io::recover_route_queue_snapshot_commit_boundary(&doc, &rc)
                .unwrap()
        );
        assert!(
            matches!(
                agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
                agent_doc_snapshot_io::SnapshotCommitStatus::Committed
            ),
            "drained queue must be committed after recovery"
        );
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&show.stdout);
        assert!(head.contains("queue_active: false"), "HEAD: {head}");
        assert!(!head.contains("agent:queue auto"), "HEAD: {head}");
    }
    /// #drained-done-queue-clear guard: the route queue commit-boundary
    /// recovery must NOT fire when a real user edit rides alongside the queue
    /// drain. Only pure queue-state churn is auto-committable.
    #[test]
    fn route_queue_commit_boundary_skips_drained_queue_with_user_edit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        agent_doc_snapshot_io::save(&doc, active, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Drained queue PLUS an unrelated exchange edit — must not auto-commit.
        let drained_plus_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n\nAn extra user line.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained_plus_edit).unwrap();
        agent_doc_snapshot_io::save(&doc, drained_plus_edit, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(active),
            Some(active),
        )
        .unwrap();

        let rc = agent_doc_run_context_io::RunContext::new(doc.clone());
        assert!(
            !agent_doc_preflight_runtime_io::detect_route_queue_snapshot_commit_boundary_recoverable(
                &doc, &rc
            )
            .unwrap(),
            "a user edit alongside the drain must block auto-commit"
        );
    }
    #[test]
    fn preflight_resumes_commit_when_write_landed_without_open_cycle_state() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::save(&doc, original, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let patched =
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nRecovered answer\n";
        std::fs::write(&doc, patched).unwrap();
        agent_doc_snapshot_io::save(&doc, patched, agent_doc_ops_log_io::log_op).unwrap();
        let ops = root.join(".agent-doc/logs/ops.log");
        std::fs::write(
            &ops,
            format!(
                "[100] snapshot_saved_file_ipc file={} snap_len={}\n",
                doc.display(),
                patched.len()
            ),
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
        assert!(
            !recovered,
            "no replay should be needed when file already has the response"
        );
        assert!(
            committed,
            "commit boundary should resume and create a commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed_doc = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed_doc.contains("Recovered answer"),
            "HEAD should include the resumed response closeout:\n{committed_doc}"
        );

        let log = std::fs::read_to_string(ops).unwrap();
        assert!(
            log.contains("resume_commit_success file="),
            "resume commit success should be logged:\n{log}"
        );
    }
    #[test]
    fn archive_pending_done_inserts_canonical_done_component() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                in_progress: false,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:done -->"));
        assert!(archived.contains("<!-- /agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(!archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }
    #[test]
    fn archive_pending_done_ignores_removed_pending_done_alias() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                in_progress: false,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("<!-- agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }
    #[test]
    fn archive_pending_done_appends_to_external_done_archive() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, content).unwrap();

        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                in_progress: false,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        let external = std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert!(external.contains("[#done1] completed externally"));
        assert!(!archived.contains("[#done1]"));
        assert!(archived.contains("completed work archived in tasks/session.done.md"));

        agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            &archived,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                in_progress: false,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();
        let external_after =
            std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert_eq!(external_after.matches("[#done1]").count(), 1);
    }
    #[test]
    fn archive_pending_done_rejects_invalid_external_archive_paths() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let item = agent_doc_element_backlog::backlog::PendingItem {
            marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: agent_doc_element_backlog::backlog::PendingState::Done,
            gate_type: None,
            in_progress: false,
            text: "completed item".to_string(),
            continuation: String::new(),
        };
        for archive_path in [
            "/tmp/session.done.md",
            "../session.done.md",
            "tasks/session.md",
        ] {
            let content = format!(
                "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
                archive_path
            );
            std::fs::write(&file, &content).unwrap();
            let err = agent_doc_element_backlog_io::done_archive::archive_pending_done(
                &file,
                &content,
                std::slice::from_ref(&item),
            )
            .unwrap_err()
            .to_string();
            assert!(
                err.contains("agent:done archive="),
                "unexpected error for {archive_path}: {err}"
            );
        }
    }
    #[test]
    fn external_done_archive_ids_satisfy_dropped_history_guard() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            dir.path().join("tasks/session.done.md"),
            "# Agent Doc Completed Work\n\n- 2026-05-13 [#item1] Was open\n",
        )
        .unwrap();
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, current).unwrap();

        let external_ids =
            agent_doc_element_backlog_io::done_archive::external_done_archive_ids(&file, current)
                .unwrap();
        let report =
            agent_doc_element_backlog::backlog::detect_dropped_from_history_with_extra_current_ids(
                current,
                baseline,
                &std::collections::HashSet::new(),
                &external_ids,
            )
            .unwrap();

        assert!(report.dropped.is_empty());
    }
    #[test]
    fn preflight_closes_response_captured_cycle_when_snapshot_already_matches_head() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let committed = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::save(&doc, committed, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let visible_snapshot = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::save(&doc, visible_snapshot, agent_doc_ops_log_io::log_op).unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        std::fs::write(&doc, &with_user_edit).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(visible_snapshot),
            Some(&with_user_edit),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
        assert!(
            recovered,
            "the missing commit boundary should be recovered from already-committed HEAD"
        );
        assert!(
            !committed,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "preflight should record the no-op closeout instead of failing:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "preflight should not log a false commit_failed for HEAD-current closeout:\n{log}"
        );
    }
    #[test]
    fn preflight_warns_on_prompt_preset_text_inside_post_exchange_html_comment() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "Scratch note while testing.\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .expect("known prompt preset in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(
            warning
                .message
                .contains("#spec-test-build-install-commit-push")
        );
        assert!(warning.message.contains("non-executable user note"));
    }
    #[test]
    fn preflight_comment_prompt_preset_warning_ignores_agent_components() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:done -->\n",
            "<!-- archived #spec-test-build-install-commit-push -->\n",
            "<!-- /agent:done -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();

        assert!(
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .is_none(),
            "agent-owned queue directives remain executable state, not ordinary scratch comments"
        );
    }
    #[test]
    fn auto_on_backlog_does_not_activate_queue() {
        // #backlog-auto-marker-misfire regression: the auto-loop reads `auto`
        // only from the queue component, never from the backlog.
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog auto -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = agent_doc_element::element::parse(content).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        assert!(
            !agent_doc_queue::document_queue::has_auto_attr(&queue.attrs),
            "queue has no auto attribute"
        );
        let backlog = components.iter().find(|c| c.name == "backlog").unwrap();
        assert!(
            agent_doc_queue::document_queue::has_auto_attr(&backlog.attrs),
            "backlog carries the misplaced auto attribute"
        );
        let body = &content[queue.open_end..queue.close_start];
        let entries = agent_doc_queue::document_queue::parse(body).unwrap();
        // Activation is driven solely by the queue component's auto flag.
        let activation =
            agent_doc_queue::document_queue::resolve_activation(&entries, false, false, false);
        assert!(
            !activation.active,
            "backlog `auto` must never activate the auto-loop"
        );
    }
    #[test]
    fn preflight_warns_on_dispatch_text_inside_post_exchange_html_comment_without_presets() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "dispatch #manual-review\n",
            "/clear\n",
            "-->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .expect("dispatch-looking text in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(warning.message.contains("dispatch #manual-review"));
        assert!(warning.message.contains("/clear"));
    }
    #[test]
    fn preflight_preserves_duplicate_prompt_comment_from_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let rc = agent_doc_run_context_io::RunContext::new(doc.clone());
        let changed = agent_doc_preflight_runtime_io::remove_post_exchange_duplicate_prompt_comments_for_preflight(
            &doc, &rc,
        )
        .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !changed,
            "preflight cleanup should not rewrite baseline-owned scratch comments"
        );
        assert!(
            file_after.contains(&format!("<!--\n{prompt}\n-->")),
            "preflight must not scrub post-exchange scratch text that already existed in HEAD:\n{file_after}"
        );
    }
    #[test]
    fn preflight_output_serializes_correctly() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec!["foo".to_string()],
            diff: Some("+new line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["recovered"], false);
        assert_eq!(parsed["committed"], true);
        assert_eq!(parsed["claims"][0], "foo");
        assert_eq!(parsed["no_changes"], false);
        assert!(parsed["diff"].as_str().is_some());
        assert!(
            parsed.get("document").is_none(),
            "document field must be absent"
        );
    }
    #[test]
    fn preflight_output_includes_orchestration_request() {
        let output = PreflightOutput {
            no_changes: false,
            orchestration_request: Some(agent_doc_diff::OrchestrationRequest {
                mode: agent_doc_diff::OrchestrationRequestMode::Sequential,
                trigger_text: "Synchronous orcestra.".to_string(),
                task_count: 5,
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["orchestration_request"]["mode"], "sequential");
        assert_eq!(parsed["orchestration_request"]["task_count"], 5);
        assert_eq!(
            parsed["orchestration_request"]["trigger_text"],
            "Synchronous orcestra."
        );
    }
    #[test]
    fn preflight_output_omits_orchestration_request_when_absent() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("orchestration_request").is_none(),
            "orchestration_request should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_includes_prompt_presets_requested() {
        let output = PreflightOutput {
            no_changes: false,
            prompt_presets_requested: vec!["#1".to_string(), "release-check".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["prompt_presets_requested"][0], "#1");
        assert_eq!(parsed["prompt_presets_requested"][1], "release-check");
    }
    #[test]
    fn preflight_output_omits_prompt_presets_requested_when_empty() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("prompt_presets_requested").is_none(),
            "prompt_presets_requested should be omitted when empty"
        );
    }
    #[test]
    fn codex_network_access_warning_for_non_codex_harness() {
        let content = "---\nagent_doc_session: test\nagent: opencode\ncodex_network_access: enabled\n---\n\ntest\n";
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        assert!(
            fm.codex_network_access.is_some(),
            "frontmatter should have codex_network_access"
        );
        let active = "opencode";
        assert_ne!(
            agent_doc_model_tier::canonical_harness_name(active).as_deref(),
            Some("codex"),
            "opencode should not be canonical codex"
        );
        assert!(
            agent_doc_model_tier::canonical_harness_name(active).is_some(),
            "opencode is a known harness"
        );
        let has_guard = agent_doc_model_tier::canonical_harness_name("codex").as_deref()
            == Some("codex")
            && agent_doc_model_tier::canonical_harness_name(active).as_deref() != Some("codex")
            && fm.codex_network_access.is_some();
        assert!(
            has_guard,
            "guard condition should fire for opencode + codex_network_access: enabled"
        );
    }
    #[test]
    fn preflight_output_includes_warnings() {
        let output = PreflightOutput {
            warnings: vec![PreflightWarning {
                code: "harness_mismatch".to_string(),
                message: "Document declares agent: codex but active harness is claude-code."
                    .to_string(),
                document_agent: Some("codex".to_string()),
                active_harness: Some("claude-code".to_string()),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["warnings"][0]["code"], "harness_mismatch");
        assert_eq!(parsed["warnings"][0]["document_agent"], "codex");
        assert_eq!(parsed["warnings"][0]["active_harness"], "claude-code");
    }
    #[test]
    fn preflight_output_omits_warnings_when_empty() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("warnings").is_none(),
            "warnings should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_null_diff_when_no_changes() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["diff"].is_null());
        assert_eq!(parsed["no_changes"], true);
    }
    #[test]
    fn check_layout_returns_empty_outside_tmux() {
        // When TMUX env var is not set (typical in CI / test), check_layout
        // should return an empty vec silently.
        let _env_guard = agent_doc_test_support::env_lock();
        let saved = std::env::var("TMUX").ok();
        // SAFETY: test is single-threaded; we restore the value immediately after.
        unsafe { std::env::remove_var("TMUX") };
        let issues = check_layout();
        // Restore if it was set.
        if let Some(val) = saved {
            unsafe { std::env::set_var("TMUX", val) };
        }
        assert!(
            issues.is_empty(),
            "expected no issues outside tmux, got: {:?}",
            issues
        );
    }
    #[test]
    fn preflight_output_includes_layout_issues() {
        let output = PreflightOutput {
            layout_issues: vec!["window index 0 missing".to_string()],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["layout_issues"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["layout_issues"][0], "window index 0 missing");
    }
    #[test]
    fn maybe_auto_repair_base_index_noop_without_issue() {
        let dir = tempfile::tempdir().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\n---\n").unwrap();
        let issues: Vec<String> = vec![];
        maybe_auto_repair_base_index(&file, &issues);
        let counter_path = agent_doc_dir.join("state/base-index-repair.count");
        assert!(
            !counter_path.exists(),
            "no counter file should be created when no base-index issue"
        );
    }
    #[test]
    fn detect_duplicate_claims_empty_registry() {
        let registry = tmux_router::Registry::new();
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn detect_duplicate_claims_no_duplicates() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/foo.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/bar.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn detect_duplicate_claims_two_sessions_same_file() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        let issues = detect_duplicate_claims(&registry);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("duplicate claims"));
        assert!(issues[0].contains("tasks/shared.md"));
        assert!(issues[0].contains("session-a"));
        assert!(issues[0].contains("session-b"));
    }
    #[test]
    fn detect_duplicate_claims_skips_empty_file_entries() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: String::new(), // legacy entry — no file
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: String::new(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn is_url_detects_http() {
        assert!(agent_doc_workflow::preflight_policy::is_url(
            "http://example.com"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_url(
            "https://example.com/path"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(
            "../relative/path.md"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(
            "tasks/software/agent-doc.md"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(""));
    }
    #[test]
    fn is_html_content_detects_html() {
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "text/html; charset=utf-8"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "text/html"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "application/xhtml+xml"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_html_content(
            "application/json"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_html_content(
            "text/plain"
        ));
    }
    #[test]
    fn html_to_markdown_converts_basic_html() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong>.</p>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Title"), "should contain heading text");
        assert!(md.contains("**world**"), "should convert bold");
    }
    #[test]
    fn html_to_markdown_strips_script_and_style() {
        let html =
            "<p>Visible</p><script>alert('xss')</script><style>.foo{}</style><p>Also visible</p>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Visible"));
        assert!(md.contains("Also visible"));
        assert!(!md.contains("alert"), "script content should be stripped");
        assert!(!md.contains(".foo"), "style content should be stripped");
    }
    #[test]
    fn html_to_markdown_strips_nav_and_footer() {
        let html =
            "<nav><a href='/'>Home</a></nav><main><p>Content</p></main><footer>Copyright</footer>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Content"));
        assert!(!md.contains("Home"), "nav content should be stripped");
        assert!(
            !md.contains("Copyright"),
            "footer content should be stripped"
        );
    }
    #[test]
    fn url_cache_path_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let p1 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://example.com");
        let p2 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://example.com");
        assert_eq!(p1, p2, "same URL should produce same cache path");

        let p3 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://other.com");
        assert_ne!(
            p1, p3,
            "different URLs should produce different cache paths"
        );
        assert!(p1.extension().unwrap() == "txt");
    }
    #[test]
    fn preflight_output_includes_baseline_file() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: Some("/tmp/baseline.md".to_string()),
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["baseline_file"], "/tmp/baseline.md");
    }
    #[test]
    fn preflight_output_omits_baseline_file_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("baseline_file").is_none(),
            "baseline_file should be omitted when None"
        );
    }
    #[test]
    fn preflight_output_includes_diff_type_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+go\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: Some("approval".to_string()),
            diff_type_reason: Some("single approval word: \"go\"".to_string()),
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["diff_type"], "approval");
        assert!(parsed["diff_type_reason"].as_str().unwrap().contains("go"));
    }
    #[test]
    fn preflight_output_omits_diff_type_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("diff_type").is_none(),
            "diff_type should be omitted when None"
        );
        assert!(
            parsed.get("diff_type_reason").is_none(),
            "diff_type_reason should be omitted when None"
        );
    }
    #[test]
    fn preflight_output_includes_annotated_diff_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: Some("[user+] line".to_string()),
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["annotated_diff"], "[user+] line");
    }
    #[test]
    fn preflight_output_omits_annotated_diff_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("annotated_diff").is_none(),
            "annotated_diff should be omitted when None"
        );
    }
    #[test]
    fn preflight_output_includes_semantic_diff_when_set() {
        let output = PreflightOutput {
            semantic_diff: Some(agent_doc_diff::semantic::SemanticDiffSummary {
                schema_version: 1,
                changed_components: vec!["queue".to_string()],
                node_events: vec![agent_doc_diff::semantic::SemanticNodeEvent {
                    component: "queue".to_string(),
                    node_key: "queue:0:task:0".to_string(),
                    op: "insert".to_string(),
                    item_id: "task".to_string(),
                    before_index: None,
                    after_index: Some(0),
                    previous_node_key: None,
                    next_node_key: None,
                    before_preview: None,
                    after_preview: Some("- do [#task]".to_string()),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["semantic_diff"]["schema_version"], 1);
        assert_eq!(parsed["semantic_diff"]["changed_components"][0], "queue");
        assert_eq!(parsed["semantic_diff"]["node_events"][0]["op"], "insert");
    }
    #[test]
    fn preflight_output_omits_semantic_diff_when_none() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("semantic_diff").is_none(),
            "semantic_diff should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_semantic_merge_acks_roundtrip() {
        // #semmerge-ack-turn (Phase 4): carried acks serialize for skill
        // consumption and are omitted when empty.
        let empty = PreflightOutput::default();
        let empty_json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&empty).unwrap()).unwrap();
        assert!(
            empty_json.get("semantic_merge_acks").is_none(),
            "semantic_merge_acks omitted when empty"
        );

        let output = PreflightOutput {
            semantic_merge_acks: vec![agent_doc_cycle_state_io::PendingSemanticMergeAck {
                component: "exchange".to_string(),
                id: "p3kj".to_string(),
                reason: "operator_deleted_agent_edited_node".to_string(),
                detail: "operator deleted the node the agent edited".to_string(),
                recorded_cycle_id: Some("cycle-1".to_string()),
                surfaced: true,
            }],
            ..Default::default()
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        assert_eq!(parsed["semantic_merge_acks"][0]["component"], "exchange");
        assert_eq!(parsed["semantic_merge_acks"][0]["id"], "p3kj");
        assert_eq!(
            parsed["semantic_merge_acks"][0]["reason"],
            "operator_deleted_agent_edited_node"
        );
    }
    #[test]
    fn preflight_output_includes_inline_annotations() {
        let output = PreflightOutput {
            inline_annotations: vec![
                "This is wrong, fix it".to_string(),
                "Broaden the gate".to_string(),
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let anns = parsed["inline_annotations"].as_array().unwrap();
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0], "This is wrong, fix it");
        assert_eq!(anns[1], "Broaden the gate");
    }
    #[test]
    fn preflight_output_omits_inline_annotations_when_empty() {
        let output = PreflightOutput {
            inline_annotations: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("inline_annotations").is_none(),
            "inline_annotations should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_includes_user_intent_prompt_changes() {
        let output = PreflightOutput {
            user_intent_prompt_changes: vec![
                agent_doc_diff::PromptBearingChange {
                    kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                    text: "❯ Why was this missed?".to_string(),
                },
                agent_doc_diff::PromptBearingChange {
                    kind: agent_doc_diff::PromptBearingChangeKind::ContentEdit,
                    text: "This line should say 503, not 401.".to_string(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let changes = parsed["user_intent_prompt_changes"].as_array().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["kind"], "prompt_target");
        assert_eq!(changes[0]["text"], "❯ Why was this missed?");
        assert_eq!(changes[1]["kind"], "content_edit");
    }
    #[test]
    fn preflight_output_omits_user_intent_prompt_changes_when_empty() {
        let output = PreflightOutput {
            user_intent_prompt_changes: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("user_intent_prompt_changes").is_none(),
            "user_intent_prompt_changes should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_includes_session_accretion_when_present() {
        let output = PreflightOutput {
            session_accretion: Some(SessionAccretionReport {
                level: SessionAccretionLevel::Warn,
                exchange_lines: 220,
                response_sections: 9,
                recent_committed_cycles: 7,
                recent_noop_closeouts: 2,
                recent_restart_count: 0,
                recent_session_loss_count: 0,
                startup_miss_active: false,
                clear_threshold: 50,
                reasons: vec!["exchange has grown".to_string()],
                guidance: vec!["Run `agent-doc compact session.md --commit`.".to_string()],
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["session_accretion"]["level"], "warn");
        assert_eq!(parsed["session_accretion"]["exchange_lines"], 220);
    }
    #[test]
    fn preflight_output_omits_session_accretion_when_absent() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("session_accretion").is_none(),
            "session_accretion should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_slash_commands_from_diff() {
        // /clear is a built-in command — goes to builtin_commands, not slash_commands
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/clear\n";
        let parsed_cmds = agent_doc_diff::parse_slash_commands_classified(diff);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(diff.to_string()),
            no_changes: false,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: parsed_cmds.skill_commands,
            builtin_commands: parsed_cmds.builtin_commands,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // /clear is a built-in — appears in builtin_commands, not slash_commands
        assert_eq!(parsed["builtin_commands"][0], "/clear");
        assert!(
            parsed["slash_commands"].is_null()
                || parsed["slash_commands"]
                    .as_array()
                    .is_none_or(|a| a.is_empty())
        );
    }
    #[test]
    fn preflight_output_no_document_field() {
        // The `document` field was removed — it must not appear in serialized JSON.
        // Having it would send full document content to the agent every cycle,
        // wasting tokens on every invocation.
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            baseline_file: None,
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("document").is_none(),
            "document key must be absent from preflight JSON — it would waste tokens on every cycle"
        );
    }
    #[test]
    fn preflight_output_no_large_content() {
        // Regression: preflight JSON must not embed document content.
        // Any field containing the full file body would be sent to the agent
        // on every cycle, burning tokens proportional to document size.
        let large_content = "x".repeat(10_000);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(format!("+{large_content}")), // diff can include content
            no_changes: false,
            linked_changes: vec![],
            baseline_file: Some("/tmp/baseline.md".to_string()),
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Only `diff` may contain the large content (it's the actual user change).
        // No OTHER field should contain it.
        let diff_str = parsed["diff"].as_str().unwrap_or("");
        for (key, val) in parsed.as_object().unwrap() {
            if key == "diff" {
                continue;
            }
            let val_str = val.to_string();
            assert!(
                !val_str.contains(&large_content),
                "field `{key}` contains large content — this would waste tokens on every preflight cycle"
            );
            assert!(
                val_str.len() < 1_000 || key == "annotated_diff",
                "field `{key}` is suspiciously large ({} bytes) — preflight should not embed document content",
                val_str.len()
            );
        }
        // Diff itself is allowed to contain the content
        assert!(diff_str.contains(&large_content));
    }
}
