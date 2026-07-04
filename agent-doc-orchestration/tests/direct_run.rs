//! # Module: run
//!
//! ## Spec
//! - `run(file, branch, agent_name, model, dry_run, no_git, config)`: executes
//!   a single agent request-response cycle for a session document.
//! - Bails immediately if the file does not exist.
//! - Computes a diff via `diff::compute`; returns `Ok(())` early (no-op) when
//!   the snapshot matches the document (nothing changed since the last run).
//! - Ensures the document has a session UUID in frontmatter, writing it if
//!   absent.
//! - Resolves the agent backend from: `agent_name` arg > frontmatter `agent`
//!   field > `config.default_agent` > fallback `"claude"`.
//! - Resolves the model from: `model` arg > frontmatter `model` field.
//! - Resolves the response write mode from frontmatter via
//!   `Frontmatter::resolve_mode()`, defaulting to template mode when no
//!   explicit format is present.
//! - Builds one of four prompt shapes: append/template × resume/fork. A stable
//!   cached prefix carries the durable response contract before the prompt-cache
//!   boundary; volatile diffs, queue/status/accretion context, and document
//!   excerpts follow the boundary. Template prompts require `patch:exchange`
//!   blocks; append prompts require plain markdown without `## Assistant`.
//!   Resumed prompts also restate ordered request blocks extracted from the diff
//!   so the agent does not anchor only on the newest question in a changed
//!   exchange tail. When session accretion has already reached warn/block
//!   severity and the diff still contains live prompt targets, resumed prompts
//!   replace the full exchange tail with a bounded response-context pack
//!   containing prompt targets, session summary, backlog head, and available
//!   component names.
//! - In `--dry-run` mode: prints the diff and prompt size to stderr and returns
//!   without calling the agent, writing files, or touching git.
//! - Optionally creates a git branch via `git::create_branch` before committing
//!   (only when `branch=true` and `no_git=false`).
//! - Pre-commits user's changes via `git::commit` before sending to the agent
//!   so the editor shows agent additions as diff-gutter entries.
//! - Opens a fresh `preflight_started` cycle after that pre-commit boundary so
//!   the response closeout for the current run is not attached to the earlier
//!   user-only commit state.
//! - Writes the agent response back through the mode-appropriate append or
//!   template path, preserving concurrent user edits against the original
//!   baseline captured before the agent call.
//! - When the user diff contains an imperative document directive (`do #id`,
//!   `run tests`, `build + install`, `commit + push`, or approval words like
//!   `go`), rejects status-only/meta agent replies unless they include either
//!   concrete execution evidence or a concrete blocker.
//! - Updates the `resume` ID in frontmatter from the agent's returned session
//!   ID after the response write succeeds.
//! - Captures the final parsed response in the durable response ledger before
//!   any file mutation so interrupted cycles can be replayed deterministically.
//! - Marks the final post-write document state as `write_applied` before the
//!   post-write commit so interrupted runs can be resumed from the exact
//!   already-written response instead of looking like generic `response_captured`
//!   drift.
//! - Acquires an advisory `flock` on a per-document lock file before writing so
//!   concurrent `agent-doc run` / watch-daemon invocations are serialized.
//! - Re-reads the file under lock; if the user edited concurrently, performs a
//!   3-way merge for append/merge docs or a CRDT merge for template+CRDT docs.
//! - Tries IPC write to the IDE plugin first; on IPC miss, falls back to
//!   `atomic_write` (temp file + POSIX rename) and saves a snapshot.
//! - In git-backed runs, refuses success unless the post-write commit closes
//!   the cycle in `committed`.
//! - `acquire_doc_lock(path)`: opens/creates `.agent-doc/locks/<hash>.lock` and
//!   acquires an exclusive `flock`; returned `File` releases the lock on drop.
//! - `atomic_write(path, content)`: writes to a sibling temp file and renames
//!   atomically, eliminating partial-write windows.
//!
//! ## Agentic Contracts
//! - Callers must not assume the file is modified when `run` returns `Ok(())`
//!   early (no-op case): the document and snapshot are untouched.
//! - The snapshot saved after a successful run reflects the final post-merge
//!   document state, including any `resume` update that landed with the
//!   response write.
//! - Git operations (branch creation, pre-commit) are skipped entirely when
//!   `no_git=true`; the agent call and write still proceed normally.
//! - The advisory flock serializes only agent-doc processes; editors bypass it.
//!   Readers of the document file must not rely on the lock for read safety.
//! - `atomic_write` is safe for concurrent callers on the same path; one write
//!   wins and the file is never in a partially-written state.
//! - Append-mode responses strip any echoed `## Assistant` heading before
//!   insertion; template-mode responses keep their patch-block content intact.
//!
//! ## Evals
//! - `run_file_not_found`: call `run` with a missing path → `Err` containing
//!   "file not found".
//! - `run_no_changes`: snapshot matches document → returns `Ok(())` without
//!   calling the agent or modifying anything.
//! - `run_dry_run`: `dry_run=true` → diff and prompt size printed to stderr;
//!   file unchanged, no agent call, no git operations.
//! - `run_marks_write_applied_before_post_write_commit`: once the final
//!   response is written (and any `resume` update lands), the cycle state is
//!   advanced to `write_applied` before the post-write commit attempt.
//! - `acquire_doc_lock_succeeds`: lock file created and exclusive lock acquired
//!   on a fresh document path → `Ok(File)`.
//! - `doc_lock_released_on_drop`: after dropping the lock handle, a second
//!   `acquire_doc_lock` on the same path succeeds immediately.
//! - `atomic_write_correct_content`: written content is exactly the input string.
//! - `atomic_write_overwrites_existing`: writing to an existing file replaces
//!   content atomically.
//! - `concurrent_atomic_writes_no_corruption`: 20 concurrent writers → final
//!   file is exactly one valid write; no partial or interleaved content.
//! - `parallel_different_files_no_interference`: two concurrent cycles on
//!   different files complete without lock contention or cross-contamination.
//! - `same_file_serialized_by_flock`: two concurrent cycles on the same file
//!   are serialized; both writes land with no corruption.
//! - `flock_prevents_partial_read_during_write`: a reader blocked on the same
//!   lock sees the completed write, not a partial state.
//! - `merge_clean_no_conflicts`: agent response appended as "ours" + user
//!   unchanged as "theirs" → clean 3-way merge containing the response.
//! - `build_prompt_resume_lists_required_response_targets`: resumed prompt with
//!   two user request blocks → prompt includes the ordered turn-completeness section

#[cfg(test)]
use agent_doc_frontmatter::frontmatter;
#[cfg(test)]
use agent_doc_run_io::{
    ActiveQueuePromptState, AutoQueueContinuation, RunCycleOutcome, RunMode, acquire_doc_lock,
    active_queue_prompt_diff, active_queue_prompt_state, apply_template_response, build_prompt,
    direct_run_atomic_write, normalize_direct_run_prompt_prefixes, prompt_cache_routing_affinity,
    run_stderr_redirect_harness, should_continue_auto_queue, start_run_cycle,
};
#[cfg(test)]
use agent_doc_session_accretion::{SessionAccretionLevel, SessionAccretionReport};
use std::path::Path;

#[cfg(test)]
use agent_doc_prompt_cache::PromptCacheBlocks;
#[cfg(test)]
use agent_doc_prompt_cache::{PROMPT_CACHE_BOUNDARY, PROMPT_CACHE_CONTROL};

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_config::Config;
    use agent_doc_queue_io::queue_consume;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    fn first_queue_node_key(content: &str) -> String {
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("queue head should have a node key")
            .node_key
    }

    fn append_typed_selected_queue_head(
        root: &Path,
        doc: &Path,
        node_key: &str,
        prompt_text: &str,
        drainable: bool,
    ) {
        let document_hash =
            agent_doc_fs::document_state_hash(&doc.canonicalize().unwrap()).unwrap();
        let prompt_hash = agent_doc_hash::content_hash(prompt_text);
        let event = agent_doc_state_backbone::StateEvent::new(
            format!("test-typed-selected-head:{node_key}:{prompt_hash}"),
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash,
                node_key: node_key.to_string(),
                backlog_id: None,
                prompt_text: Some(prompt_text.to_string()),
                drainable,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(root, &event).unwrap();
    }

    #[test]
    fn run_stderr_redirect_harnesses_include_claude_codex_and_opencode() {
        assert!(run_stderr_redirect_harness("claude"));
        assert!(run_stderr_redirect_harness("codex"));
        assert!(run_stderr_redirect_harness("opencode"));
        assert!(!run_stderr_redirect_harness("unknown"));
    }

    #[test]
    fn start_run_cycle_routes_through_realtime_admit() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let doc = root.join("session.md");
        let content = "---\nagent_doc_session: run-admit\n---\n\n# Session\n\nRun this.\n";
        std::fs::write(&doc, content).unwrap();

        start_run_cycle(&doc).unwrap();

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::PreflightStarted);
        assert_eq!(state.last_event, "preflight_started");

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("realtime_admit"), "ops log:\n{log}");
        assert!(log.contains("source=admit"), "ops log:\n{log}");
        assert!(
            !log.contains("preflight_diff_start"),
            "run cycle admission must not call preflight start:\n{log}"
        );
    }

    #[test]
    fn active_queue_prompt_diff_ignores_slash_command_head() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "-   /clear  \n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let node_key = first_queue_node_key(content);
        append_typed_selected_queue_head(dir.path(), &doc, &node_key, "  /clear  ", true);

        assert_eq!(
            active_queue_prompt_diff(&doc).unwrap(),
            None,
            "slash-only active queue heads are command handoffs, not child-agent prompts"
        );
    }

    #[test]
    fn active_queue_prompt_state_refuses_markdown_prompt_without_typed_head() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#markdownhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();

        assert_eq!(
            active_queue_prompt_state(&doc).unwrap(),
            ActiveQueuePromptState::Unproven {
                reason: "missing_or_stale_typed_queue_head".to_string(),
                document_head: Some("do [#markdownhead]".to_string())
            }
        );
        assert_eq!(
            active_queue_prompt_diff(&doc).unwrap(),
            None,
            "markdown-only queue heads must not synthesize child-agent prompt diffs"
        );
    }

    #[test]
    fn active_queue_prompt_state_ignores_persisted_plain_queue_without_go() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#plainhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let node_key = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let event = agent_doc_state_backbone::StateEvent::new(
            "typed-selected-plain-no-go",
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash,
                node_key,
                backlog_id: Some("plainhead".to_string()),
                prompt_text: Some("do [#plainhead]".to_string()),
                drainable: true,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(dir.path(), &event)
            .unwrap();

        assert_eq!(
            active_queue_prompt_state(&doc).unwrap(),
            ActiveQueuePromptState::Inactive
        );
        assert_eq!(active_queue_prompt_diff(&doc).unwrap(), None);
    }

    #[test]
    fn should_continue_auto_queue_stops_without_typed_head_projection() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#markdownhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let outcome = RunCycleOutcome {
            dispatched: true,
            queue_synthetic_diff: true,
            queue_consumption: Some(queue_consume::QueueConsumptionOutcome {
                consumed_text: "do [#prior]".to_string(),
                consumed_count: 1,
                node_ops: Vec::new(),
                remaining: 1,
                drained: false,
                auto: true,
            }),
        };

        assert_eq!(
            should_continue_auto_queue(&doc, &outcome, 1, false, None).unwrap(),
            AutoQueueContinuation::Stop,
            "auto continuation must stop instead of falling back to markdown queue text"
        );
    }

    #[test]
    fn active_queue_prompt_state_refuses_stale_typed_head_for_different_node() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#markdownhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        append_typed_selected_queue_head(
            dir.path(),
            &doc,
            "queue:stale-node",
            "do [#typedhead]",
            true,
        );

        assert_eq!(
            active_queue_prompt_state(&doc).unwrap(),
            ActiveQueuePromptState::Unproven {
                reason: "missing_or_stale_typed_queue_head".to_string(),
                document_head: Some("do [#markdownhead]".to_string())
            }
        );
        assert_eq!(
            active_queue_prompt_diff(&doc).unwrap(),
            None,
            "stale typed queue heads must not fall back to markdown prompt text"
        );
    }

    #[test]
    fn active_queue_prompt_state_prefers_typed_selected_prompt_when_node_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#markdownhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let node_key = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let event = agent_doc_state_backbone::StateEvent::new(
            "typed-selected-head",
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash,
                node_key,
                backlog_id: Some("typedhead".to_string()),
                prompt_text: Some("do [#typedhead]".to_string()),
                drainable: true,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(dir.path(), &event)
            .unwrap();

        assert_eq!(
            active_queue_prompt_state(&doc).unwrap(),
            ActiveQueuePromptState::Ready {
                prompt: "do [#typedhead]".to_string()
            }
        );
    }

    #[test]
    fn active_queue_prompt_state_prefers_typed_deferred_stop_guard_when_node_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#markdownhead]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::save(&doc, content, agent_doc_ops_log_io::log_op).unwrap();
        let node_key = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")
            .unwrap()
            .into_iter()
            .find(|node| !node.item.struck)
            .expect("queue head should have a node key")
            .node_key;
        let document_hash = agent_doc_hash::document_id_for_path(&doc);
        let selected = agent_doc_state_backbone::StateEvent::new(
            "typed-selected-before-deferred",
            agent_doc_state_backbone::StateFact::QueueHeadSelected {
                document_hash: document_hash.clone(),
                node_key: node_key.clone(),
                backlog_id: Some("typedhead".to_string()),
                prompt_text: Some("do [#typedhead]".to_string()),
                drainable: false,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(dir.path(), &selected)
            .unwrap();
        let deferred = agent_doc_state_backbone::StateEvent::new(
            "typed-deferred-stop-head",
            agent_doc_state_backbone::StateFact::QueueHeadDeferred {
                document_hash,
                node_key,
                reason: "stop_fence".to_string(),
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(dir.path(), &deferred)
            .unwrap();

        assert_eq!(
            active_queue_prompt_state(&doc).unwrap(),
            ActiveQueuePromptState::StopFence {
                next_prompt: Some("do [#typedhead]".to_string())
            }
        );
    }

    #[test]
    fn build_prompt_defaults_to_template_mode() {
        let fm = frontmatter::Frontmatter::default();
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::from_frontmatter(&fm),
            &fm,
            "diff",
            "doc",
            None,
        );
        assert!(prompt.contains("patch:exchange"));
        assert!(!prompt.contains("## Assistant heading"));
    }

    #[test]
    fn build_prompt_append_mode_uses_inline_contract() {
        let fm = frontmatter::Frontmatter {
            format: Some(frontmatter::AgentDocFormat::Append),
            ..Default::default()
        };
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::from_frontmatter(&fm),
            &fm,
            "diff",
            "doc",
            None,
        );
        assert!(prompt.contains("Do not include a ## Assistant heading"));
        assert!(!prompt.contains("patch:exchange"));
    }

    #[test]
    fn build_prompt_places_turn_churn_after_cache_boundary() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
          Done.\n\
          +do [#pcache-boundary]. keep volatile queue churn below the boundary\n\
          <!-- /agent:exchange -->\n";
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic - gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#pcache-boundary] Prompt-cache boundary work\n",
            "<!-- /agent:backlog -->\n",
        );
        let report = SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            reasons: vec!["document hit 4 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        let boundary = prompt
            .find(PROMPT_CACHE_BOUNDARY)
            .expect("direct-run prompt should expose cache boundary");
        for volatile in [
            "<diff>",
            "do [#pcache-boundary]. keep volatile queue churn below the boundary",
            "User-authored prompt-bearing changes (oldest first):",
            "Accretion reason: document hit 4 no-op closeouts in the last 30 minutes.",
            "<response_context level=\"warn\">",
        ] {
            let pos = prompt
                .find(volatile)
                .unwrap_or_else(|| panic!("missing volatile fragment {volatile:?}:\n{prompt}"));
            assert!(
                pos > boundary,
                "volatile fragment {volatile:?} must stay after cache boundary:\n{prompt}"
            );
        }
        assert!(
            prompt.starts_with("<agent_doc_prompt_stable_prefix>"),
            "stable prefix should be the first prompt block:\n{prompt}"
        );
    }

    #[test]
    fn prompt_cache_boundary_contract_separates_durable_and_volatile_blocks() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,4 +1,6 @@\n\
old status\n\
+new status\n\
+do [#pcache-boundary-contract]\n";
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "new status\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older topic - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-boundary-contract]\n",
            "<!-- /agent:queue -->\n",
        );
        let report = SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            reasons: vec!["document closed 7 cycles in the last 30 minutes".to_string()],
            recent_noop_closeouts: 5,
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        let boundary = PROMPT_CACHE_BOUNDARY;
        let (stable, volatile) = prompt
            .split_once(boundary)
            .expect("direct-run prompt should expose cache boundary");

        for durable in [
            "<agent_doc_prompt_stable_prefix>",
            "<response_contract>",
            "<turn_payload_contract>",
            "Format your response as patch blocks",
            "Read the volatile turn payload after the cache boundary",
        ] {
            assert!(
                stable.contains(durable),
                "stable prefix must keep durable fragment {durable:?}:\n{prompt}"
            );
        }

        for volatile_fragment in [
            "<diff>",
            "new status",
            "do [#pcache-boundary-contract]",
            "<response_context level=\"warn\">",
            "Accretion reason: document closed 7 cycles in the last 30 minutes.",
        ] {
            assert!(
                !stable.contains(volatile_fragment),
                "volatile fragment {volatile_fragment:?} must not enter stable prefix:\n{prompt}"
            );
            assert!(
                volatile.contains(volatile_fragment),
                "volatile suffix must contain {volatile_fragment:?}:\n{prompt}"
            );
        }
    }

    #[test]
    fn prompt_cache_replay_key_survives_session_churn_and_invalidates_on_durable_contract() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let report = SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            reasons: vec!["document closed 3 no-op cycles in the last hour".to_string()],
            recent_noop_closeouts: 3,
            ..Default::default()
        };
        let base_diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
           complete\n\
           +do [#pcache-replaygate]\n\
           +queue_active: true\n";
        let churn_diff = "--- snapshot\n+++ document\n@@ -1,5 +1,7 @@\n\
           -complete\n\
           +working\n\
           +do [#pcache-replaygate]\n\
           +<!-- agent:boundary:churn -->\n";
        let base_doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "complete\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior topic - gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-replaygate]\n",
            "- do [#pcache-missrank]\n",
            "<!-- /agent:queue -->\n"
        );
        let churn_doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "working\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior topic - gpt-5\n\nDone.\n",
            "<!-- agent:boundary:churn -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#pcache-replaygate]\n",
            "- do [#pcache-missrank]\n",
            "- do [#pcache-ci-history]\n",
            "<!-- /agent:queue -->\n"
        );

        let base_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            base_diff,
            base_doc,
            Some(&report),
        );
        let churn_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            churn_diff,
            churn_doc,
            Some(&report),
        );
        assert_ne!(
            base_prompt, churn_prompt,
            "precondition: volatile session churn should change the full prompt"
        );

        let routing_affinity =
            prompt_cache_routing_affinity(RunMode::Template, "codex", Some("gpt-5"));
        let base_key = PromptCacheBlocks::from_rendered(&base_prompt)
            .expect("template prompt should expose prompt-cache blocks")
            .replay_key(&routing_affinity);
        let churn_key = PromptCacheBlocks::from_rendered(&churn_prompt)
            .expect("churn prompt should expose prompt-cache blocks")
            .replay_key(&routing_affinity);

        assert_eq!(base_key, churn_key);
        assert_eq!(base_key.cache_control, PROMPT_CACHE_CONTROL);
        assert_eq!(
            base_key.routing_affinity,
            "agent_doc_run:v1;agent=codex;model=gpt-5;mode=template"
        );

        let churn_boundary = churn_prompt
            .find(PROMPT_CACHE_BOUNDARY)
            .expect("prompt-cache boundary should be present");
        for volatile in [
            "working",
            "do [#pcache-replaygate]",
            "agent:boundary:churn",
            "Accretion reason: document closed 3 no-op cycles in the last hour.",
        ] {
            let pos = churn_prompt
                .find(volatile)
                .unwrap_or_else(|| panic!("missing volatile fragment {volatile:?}"));
            assert!(
                pos > churn_boundary,
                "volatile fragment {volatile:?} must remain after cache boundary:\n{churn_prompt}"
            );
        }

        let append_prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Append,
            &fm,
            base_diff,
            base_doc,
            Some(&report),
        );
        let append_same_route_key = PromptCacheBlocks::from_rendered(&append_prompt)
            .expect("append prompt should expose prompt-cache blocks")
            .replay_key(&base_key.routing_affinity);
        assert_eq!(
            append_same_route_key.routing_affinity,
            base_key.routing_affinity
        );
        assert_ne!(
            append_same_route_key.stable_prefix_sha256, base_key.stable_prefix_sha256,
            "changing the durable response contract should invalidate the stable-prefix fingerprint"
        );
        assert_ne!(
            append_same_route_key.provider_cache_key, base_key.provider_cache_key,
            "provider cache key must change when durable instructions change"
        );
    }

    #[test]
    fn build_prompt_resume_lists_required_response_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n\
           ctx\n\
           +❯ First unresolved question?\n\
           +\n\
           +❯ Second unresolved question?\n";
        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            "doc",
            None,
        );
        assert!(prompt.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(prompt.contains("Do not stop at the newest question"));
        assert!(prompt.contains("kind=\"prompt_target\""));
        assert!(prompt.contains("❯ First unresolved question?"));
        assert!(prompt.contains("❯ Second unresolved question?"));
    }

    #[test]
    fn build_prompt_carries_forward_active_format_requirements() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "Done.\n",
        );

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            "diff",
            doc,
            None,
        );
        assert!(
            prompt.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(prompt.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
    }

    #[test]
    fn build_prompt_uses_bounded_context_pack_for_warn_level_prompt_targets() {
        let fm = frontmatter::Frontmatter {
            resume: Some("sess-123".to_string()),
            ..Default::default()
        };
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
           Done.\n\
           +do [#ctxpack]. spec-test-build-install-commit-push\n\
           <!-- /agent:exchange -->\n";
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic — gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ctxpack] Add bounded context pack\n",
            "<!-- /agent:backlog -->\n",
        );
        let report = SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        };

        let prompt = build_prompt(
            Path::new("session.md"),
            RunMode::Template,
            &fm,
            diff,
            doc,
            Some(&report),
        );
        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(prompt.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(!prompt.contains("<document>\n## Exchange"));
    }

    #[test]
    fn apply_template_response_normalizes_legacy_backlog_patch_before_enforcement() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] existing item\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: backlog follow-up — gpt-5\n\n",
            "Captured the requested backlog update.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "- [ ] [#new1] added item\n",
            "- [ ] [#keep1] existing item\n",
            "<!-- /patch:backlog -->\n",
        );

        apply_template_response(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &doc,
            baseline,
            response,
            false,
        )
        .expect("run path should normalize legacy backlog patches before enforcement");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] added item"));
        assert!(updated.contains("- [ ] [#keep1] existing item"));
    }

    #[test]
    fn apply_template_response_normalizes_sampleorders_style_backlog_patch() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "### 2. Revenue / Fulfillment / Store Operations\n",
            "- [ ] [#2xcx] Verify ShipStation polling resumes after Cloudflare fix\n",
            "- [ ] [#yckq] [#ss01] ShipStation fix\n",
            "\n",
            "### 4. Internal Tooling / Documentation Carry-Forward\n",
            "- [ ] [#2gdt] [#wpmem] WP memory limits\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: sampleorders backlog follow-up — gpt-5\n\n",
            "Captured the requested backlog update.\n",
            "<!-- /patch:exchange -->\n\n",
            "<!-- patch:backlog -->\n",
            "### 2. Revenue / Fulfillment / Store Operations\n",
            "- [ ] [#new1] Verify direct rerun completed cleanly\n",
            "- [ ] [#2xcx] Verify ShipStation polling resumes after Cloudflare fix\n",
            "- [ ] [#yckq] [#ss01] ShipStation fix\n",
            "\n",
            "### 4. Internal Tooling / Documentation Carry-Forward\n",
            "- [ ] [#2gdt] [#wpmem] WP memory limits\n",
            "<!-- /patch:backlog -->\n",
        );

        apply_template_response(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &doc,
            baseline,
            response,
            false,
        )
        .expect("run path should normalize sampleorders-style backlog patches");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: sampleorders backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] Verify direct rerun completed cleanly"));
        assert!(updated.contains("- [ ] [#yckq] [#ss01] ShipStation fix"));
        assert!(updated.contains("- [ ] [#2gdt] [#wpmem] WP memory limits"));
    }

    #[test]
    fn apply_template_response_prefixes_direct_run_prompt_with_image_line() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "Read the image.\n",
            "![img_7.png](img_7.png)\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();

        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: image read — gpt-5\n\n",
            "The image line was handled.\n",
            "<!-- /patch:exchange -->\n",
        );

        apply_template_response(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &doc,
            baseline,
            response,
            false,
        )
        .expect("direct-run template write should normalize prompt lines");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("❯ Read the image.\n❯ ![img_7.png](img_7.png)\n"),
            "raw direct-run prompt block must be prefixed:\n{updated}"
        );
        assert!(
            updated.contains("### Re: image read — gpt-5 (HEAD)\n\nThe image line was handled."),
            "assistant response should be preserved:\n{updated}"
        );
        assert!(
            !updated.contains("❯ ### Re: image read"),
            "assistant response heading must not receive prompt prefix:\n{updated}"
        );
    }

    #[test]
    fn normalize_direct_run_prompt_prefixes_updates_baseline_before_precommit() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old -->\n",
            "Read the image.\n",
            "![img_7.png](img_7.png)\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot, agent_doc_ops_log_io::log_op).unwrap();

        let diff_text = agent_doc_diff::unified_diff_from_contents(snapshot, baseline)
            .expect("snapshot and baseline differ");
        let normalized = normalize_direct_run_prompt_prefixes(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &doc,
            baseline,
            &diff_text,
        )
        .expect("direct-run baseline prompt normalization should succeed");
        let on_disk = std::fs::read_to_string(&doc).unwrap();

        assert_eq!(normalized, on_disk);
        assert!(
            on_disk.contains("❯ Read the image.\n❯ ![img_7.png](img_7.png)\n"),
            "precommit baseline should be written with prompt prefixes:\n{on_disk}"
        );
    }

    #[test]
    fn run_rejects_bare_compact_exchange_directive() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        let baseline = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n\n",
            "compact exchange\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();

        let err = agent_doc_run_io::run(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &doc,
            false,
            None,
            None,
            true,
            true,
            false,
            &Config::default(),
        )
        .expect_err("run should fail closed on unresolved compaction directive");
        let msg = err.to_string();
        assert!(msg.contains("compact exchange"));
        assert!(msg.contains("agent-doc compact"));
    }

    #[test]
    fn acquire_doc_lock_succeeds() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        let lock = acquire_doc_lock(&doc);
        assert!(lock.is_ok());
    }

    #[test]
    fn doc_lock_released_on_drop() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        std::fs::write(&doc, "content").unwrap();
        {
            let _lock = acquire_doc_lock(&doc).unwrap();
        }
        // After drop, second acquire should succeed
        let lock2 = acquire_doc_lock(&doc);
        assert!(lock2.is_ok());
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        direct_run_atomic_write(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &path,
            "hello world",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("overwrite.md");
        std::fs::write(&path, "old content").unwrap();
        direct_run_atomic_write(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &path,
            "new content",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    /// #ipc-drift-writeprovenance: the direct-run document-write path records the
    /// same write-provenance as the IPC/finalize `write.rs::atomic_write`, so a
    /// foreign-looking disk change from a direct-run write is positively
    /// attributed to agent-doc instead of inferred from the mtime heuristic.
    #[test]
    fn direct_run_atomic_write_records_provenance() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join("prov-direct-run.md");
        direct_run_atomic_write(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &path,
            "direct run body",
        )
        .unwrap();
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.clone())
            .to_string_lossy()
            .to_string();
        let prov = agent_doc_debounce::write_provenance(&key)
            .expect("direct-run document write should record provenance");
        assert_eq!(prov.len, "direct run body".len());
        assert_eq!(prov.hash, agent_doc_hash::content_hash("direct run body"));
        assert_eq!(prov.actor, "agent");
        assert!(!prov.write_id.is_empty());
    }

    /// 08b end state (removal rung complete): the direct-run document-write path
    /// is no longer a parallel direct-disk writer — it routes through the session
    /// actor's ordered write queue, the SAME chokepoint as the IPC/finalize path
    /// (no flag). The routed write re-enters `atomic_write` on the owner thread;
    /// the owner-scope re-entrancy guard keeps that inner write on the raw path,
    /// so this must not deadlock, the content must land, and the routed decision
    /// must be reported to `ops.log` (proving no surviving direct-disk writer
    /// bypasses the queue).
    #[test]
    fn direct_run_atomic_write_routes_through_queue() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc").join("logs")).unwrap();
        let path = dir.path().join("routed-direct-run.md");
        direct_run_atomic_write(
            &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
            &path,
            "routed direct-run body",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "routed direct-run body"
        );
        let ops =
            std::fs::read_to_string(dir.path().join(".agent-doc").join("logs").join("ops.log"))
                .unwrap_or_default();
        assert!(
            ops.contains("write_authority action=routed"),
            "direct-run write must route through the queue and \
             report it to ops.log: {ops:?}"
        );
    }

    #[test]
    fn concurrent_atomic_writes_no_corruption() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        std::fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                direct_run_atomic_write(&agent_doc_orchestration::DIRECT_RUN_EFFECTS, &p, &content)
                    .unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Final content should be exactly one of the valid writes
        let final_content = std::fs::read_to_string(&path).unwrap();
        assert!(final_content.starts_with("writer-"));
        assert!(final_content.ends_with("-content"));
    }

    // -----------------------------------------------------------------------
    // Lazy parallelization: functional tests
    // -----------------------------------------------------------------------

    /// Simulate two document cycles on different files running in parallel.
    /// Both should complete without interference — no shared lock contention.
    #[test]
    fn parallel_different_files_no_interference() {
        let dir = TempDir::new().unwrap();
        let doc_a = dir.path().join("a.md");
        let doc_b = dir.path().join("b.md");
        std::fs::write(&doc_a, "initial-a").unwrap();
        std::fs::write(&doc_b, "initial-b").unwrap();

        let barrier = Arc::new(Barrier::new(2));

        let bar_a = Arc::clone(&barrier);
        let path_a = doc_a.clone();
        let ha = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_a).unwrap();
            bar_a.wait(); // both threads hold their own lock simultaneously
            // Simulate read-modify-write cycle
            let content = std::fs::read_to_string(&path_a).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            direct_run_atomic_write(
                &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
                &path_a,
                &format!("{}\n## Assistant\nResponse A", content),
            )
            .unwrap();
        });

        let bar_b = Arc::clone(&barrier);
        let path_b = doc_b.clone();
        let hb = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_b).unwrap();
            bar_b.wait(); // both threads hold their own lock simultaneously
            let content = std::fs::read_to_string(&path_b).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            direct_run_atomic_write(
                &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
                &path_b,
                &format!("{}\n## Assistant\nResponse B", content),
            )
            .unwrap();
        });

        ha.join().unwrap();
        hb.join().unwrap();

        let a = std::fs::read_to_string(&doc_a).unwrap();
        let b = std::fs::read_to_string(&doc_b).unwrap();
        assert!(a.contains("Response A"), "Doc A missing response: {}", a);
        assert!(b.contains("Response B"), "Doc B missing response: {}", b);
        assert!(!a.contains("Response B"), "Doc A has B's response");
        assert!(!b.contains("Response A"), "Doc B has A's response");
    }

    /// Simulate two document cycles on the SAME file running concurrently.
    /// flock serializes them — both writes land, no corruption.
    #[test]
    fn same_file_serialized_by_flock() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("shared.md");
        std::fs::write(&doc, "# Shared Doc\n").unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();

        for i in 0..2 {
            let path = doc.clone();
            let bar = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                bar.wait(); // both start at the same time
                let lock = acquire_doc_lock(&path).unwrap();
                // Critical section: read, modify, write
                let content = std::fs::read_to_string(&path).unwrap();
                let updated = format!("{}writer-{}\n", content, i);
                std::thread::sleep(std::time::Duration::from_millis(5));
                direct_run_atomic_write(
                    &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
                    &path,
                    &updated,
                )
                .unwrap();
                drop(lock);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = std::fs::read_to_string(&doc).unwrap();
        // Both writers should have appended (serialized by flock)
        assert!(
            final_content.contains("writer-0") && final_content.contains("writer-1"),
            "Both writes should land (flock serializes): {}",
            final_content
        );
    }

    /// Verify that a locked document cycle prevents concurrent reads of
    /// partial state — the second reader waits for the lock to be released.
    #[test]
    fn flock_prevents_partial_read_during_write() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("partial.md");
        std::fs::write(&doc, "before").unwrap();

        let path_w = doc.clone();
        let path_r = doc.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();

        // Writer: acquire lock, pause, then write
        let writer = std::thread::spawn(move || {
            let lock = acquire_doc_lock(&path_w).unwrap();
            locked_tx.send(()).unwrap();
            // Hold lock while "processing"
            std::thread::sleep(std::time::Duration::from_millis(50));
            direct_run_atomic_write(
                &agent_doc_orchestration::DIRECT_RUN_EFFECTS,
                &path_w,
                "after",
            )
            .unwrap();
            drop(lock);
        });

        // Reader: wait until writer definitely holds the lock, then block until release.
        locked_rx.recv().unwrap();
        let reader = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_r).unwrap();
            // By the time we get the lock, writer has finished
            std::fs::read_to_string(&path_r).unwrap()
        });

        writer.join().unwrap();
        let read_content = reader.join().unwrap();
        assert_eq!(
            read_content, "after",
            "Reader should see completed write, not partial state"
        );
    }

    #[test]
    fn merge_clean_no_conflicts() {
        // merge_contents spawns `git merge-file` which inherits CWD.
        // Other tests may invalidate CWD via TempDir drops, so we
        // perform the merge manually using temp files + Command with
        // an explicit current_dir to avoid CWD pollution.
        let dir = TempDir::new().unwrap();
        let base_path = dir.path().join("base");
        let ours_path = dir.path().join("ours");
        let theirs_path = dir.path().join("theirs");

        let base = "line 1\nline 2\nline 3\n";
        let ours = "line 1\nline 2\nline 3\n\n## Assistant\n\nResponse here.\n";
        let theirs = "line 1\nline 2\nline 3\n";

        std::fs::write(&base_path, base).unwrap();
        std::fs::write(&ours_path, ours).unwrap();
        std::fs::write(&theirs_path, theirs).unwrap();

        let output = std::process::Command::new("git")
            .current_dir(dir.path())
            .args([
                "merge-file",
                "-p",
                "--diff3",
                "-L",
                "agent-response",
                "-L",
                "original",
                "-L",
                "your-edits",
            ])
            .arg(&ours_path)
            .arg(&base_path)
            .arg(&theirs_path)
            .output()
            .unwrap();

        let merged = String::from_utf8(output.stdout).unwrap();
        assert!(output.status.success(), "merge should be clean");
        assert!(merged.contains("Response here."));
    }
}
