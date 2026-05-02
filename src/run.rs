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
//! - Builds one of four prompt shapes: append/template × resume/fork. Template
//!   prompts require `patch:exchange` blocks; append prompts require plain
//!   markdown without `## Assistant`. Resumed prompts also restate ordered
//!   request blocks extracted from the diff so the agent does not anchor only
//!   on the newest question in a changed exchange tail.
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

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::Path;

use crate::{agent, config::Config, diff, frontmatter, git, merge, snapshot, template, write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunMode {
    Append,
    Template,
}

impl RunMode {
    fn from_frontmatter(fm: &frontmatter::Frontmatter) -> Self {
        if fm.resolve_mode().is_template() {
            Self::Template
        } else {
            Self::Append
        }
    }
}

pub fn run(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    config: &Config,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    eprintln!("[run] starting for {}", file.display());

    // Compute diff
    let the_diff = match diff::compute(file)? {
        Some(d) => {
            eprintln!("[run] diff computed ({} bytes)", d.len());
            d
        }
        None => {
            eprintln!(
                "[run] Nothing changed since last run for {}",
                file.display()
            );
            return Ok(());
        }
    };
    write::guard_no_exchange_compaction_request_for_diff(file, &the_diff)?;

    // Ensure the document has a session UUID (for tmux routing)
    let raw_content = std::fs::read_to_string(file)?;
    let (content_original, _session_id) = frontmatter::ensure_session(&raw_content)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    let (fm, _body) = frontmatter::parse(&content_original)?;
    let run_mode = RunMode::from_frontmatter(&fm);

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);

    // Expand frontmatter env vars (applied to the spawned agent child process).
    let expanded_env = if fm.env.is_empty() {
        Vec::new()
    } else {
        match crate::env::expand_values(&fm.env) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[run] env expansion failed: {} — continuing without env", e);
                Vec::new()
            }
        }
    };

    let backend = agent::resolve_for_file(agent_name, agent_config, expanded_env, file, &fm)?;

    let prompt = build_prompt(run_mode, &fm, &the_diff, &content_original);

    if dry_run {
        eprintln!("--- Diff ---");
        print!("{}", the_diff);
        eprintln!("--- Prompt would be {} bytes ---", prompt.len());
        return Ok(());
    }

    // Create branch if requested
    if branch && !no_git {
        git::create_branch(file)?;
    }

    // Pre-commit: commit user's changes before sending to agent
    // This lets the editor show agent additions as diff gutters
    if !no_git {
        git::commit(file)?;
    }
    start_run_cycle(file)?;

    eprintln!("Submitting to {}...", agent_name);

    // Send to agent — use `resume` for agent conversation tracking
    let fork = fm.resume.is_none();
    let harness = agent_doc::model_tier::detect_harness();
    let model = model.or(fm.resolve_harness_model(&harness));
    let response = backend.send(&prompt, fm.resume.as_deref(), fork, model)?;

    let response_text = match run_mode {
        RunMode::Append => write::strip_assistant_heading(&response.text),
        RunMode::Template => response.text.clone(),
    };
    write::enforce_imperative_response_contract_for_diff(file, &the_diff, &response_text)?;
    crate::repair::save_pending(file, &response_text)?;

    match run_mode {
        RunMode::Append => apply_append_response(file, &content_original, &response_text)?,
        RunMode::Template => apply_template_response(
            file,
            &content_original,
            &response_text,
            fm.resolve_mode().is_crdt(),
        )?,
    }
    mark_run_write_applied(file, "run_write_applied")?;

    if let Some(ref sid) = response.session_id {
        update_resume_id(file, sid)?;
        mark_run_write_applied(file, "run_write_applied_resume")?;
    }

    crate::repair::clear_pending(file)?;
    maybe_abort_after_write_applied_for_test()?;

    if !no_git {
        write::complete_required_closeout(file)?;
    }

    eprintln!("Response written to {}", file.display());
    Ok(())
}

fn mark_run_write_applied(file: &Path, event: &str) -> Result<()> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} after run write", file.display()))?;
    let snapshot_content = snapshot::load(file)?;
    crate::cycle_state::mark_write_applied(
        file,
        event,
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

fn start_run_cycle(file: &Path) -> Result<()> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} before run dispatch", file.display()))?;
    let snapshot_content = snapshot::load(file)?;
    crate::cycle_state::start_preflight(file, snapshot_content.as_deref(), Some(&file_content))?;
    Ok(())
}

fn maybe_abort_after_write_applied_for_test() -> Result<()> {
    if std::env::var_os("AGENT_DOC_TEST_ABORT_AFTER_RUN_WRITE_APPLIED").is_some() {
        anyhow::bail!("test abort after run write_applied");
    }
    Ok(())
}

fn build_prompt(
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        crate::prompt_contract::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    match (run_mode, fm.resume.is_some()) {
        (RunMode::Template, true) => format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            the_diff, prompt_bearing_changes, active_format_requirements, content
        ),
        (RunMode::Template, false) => format!(
            "The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            active_format_requirements, content
        ),
        (RunMode::Append, true) => format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user inserted prompt-bearing edits inline, classify them as prompt targets vs content edits before responding.",
            the_diff, prompt_bearing_changes, active_format_requirements, content
        ),
        (RunMode::Append, false) => format!(
            "The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user asked questions or prompt-bearing edits inline (e.g., in blockquotes or prior responses), address those too.",
            active_format_requirements, content
        ),
    }
}

fn apply_append_response(file: &Path, baseline: &str, response: &str) -> Result<()> {
    let doc_lock = acquire_doc_lock(file)?;
    snapshot::save_pre_response(file, baseline)?;

    let mut content_ours = baseline.to_string();
    if !content_ours.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("## Assistant\n\n");
    content_ours.push_str(response);
    if !response.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("\n## User\n\n");

    let content_current = std::fs::read_to_string(file)?;
    let final_content = if content_current == baseline {
        content_ours
    } else {
        eprintln!("File was modified during run. Merging changes...");
        merge::merge_contents(baseline, &content_ours, &content_current)?
    };

    snapshot::save(file, &final_content)?;
    atomic_write(file, &final_content)?;
    drop(doc_lock);
    Ok(())
}

fn apply_template_response(
    file: &Path,
    baseline: &str,
    response: &str,
    use_crdt: bool,
) -> Result<()> {
    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (mut patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;
    write::sanitize_patches(&mut patches);
    let normalized =
        write::normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;
    write::enforce_no_replace_pending(&patches, false)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    write::ensure_template_response_write_proof(&patches, &unmatched)?;

    let doc_lock = acquire_doc_lock(file)?;
    snapshot::save_pre_response(file, baseline)?;

    let content_ours = template::apply_patches(baseline, &patches, &unmatched, file)
        .context("failed to apply template patches")?;
    let content_ours = write::normalize_template_structure_or_fail(&content_ours, file)?;

    let content_current = std::fs::read_to_string(file)?;
    let (final_content, crdt_state) = if content_current == baseline {
        let state = if use_crdt {
            Some(crate::crdt::CrdtDoc::from_text(&content_ours).encode_state())
        } else {
            None
        };
        (content_ours, state)
    } else if use_crdt {
        eprintln!("File was modified during run. CRDT merging changes...");
        let base_state = crate::crdt::CrdtDoc::from_text(baseline).encode_state();
        let (merged, state) =
            merge::merge_contents_crdt(Some(&base_state), &content_ours, &content_current)?;
        (merged, Some(state))
    } else {
        eprintln!("File was modified during run. Merging changes...");
        (
            merge::merge_contents(baseline, &content_ours, &content_current)?,
            None,
        )
    };
    let final_content = write::normalize_template_structure_or_fail(&final_content, file)?;

    snapshot::save(file, &final_content)?;
    if let Some(state) = crdt_state {
        snapshot::save_crdt(file, &state)?;
    }
    atomic_write(file, &final_content)?;
    drop(doc_lock);
    Ok(())
}

fn update_resume_id(file: &Path, session_id: &str) -> Result<()> {
    let current = std::fs::read_to_string(file)?;
    let updated = frontmatter::set_resume_id(&current, session_id)?;
    atomic_write(file, &updated)?;
    snapshot::save(file, &updated)?;
    Ok(())
}

/// Acquire an advisory flock on a document file for agent-doc-vs-agent-doc
/// coordination. Lock file is `.agent-doc/locks/<hash>.lock`. Released on drop.
fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = crate::snapshot::lock_path_for(path)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open doc lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire doc lock on {}", lock_path.display()))?;
    Ok(file)
}

/// Write content to a file atomically via write-to-temp + rename.
///
/// This eliminates the partial-write window where another process (e.g., an
/// editor or the watch daemon) could read a half-written file. The rename is
/// atomic on POSIX filesystems when source and destination are on the same
/// filesystem (guaranteed here since the temp file is a sibling).
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::sync::{Arc, Barrier};
    use tempfile::TempDir;

    #[test]
    fn build_prompt_defaults_to_template_mode() {
        let fm = frontmatter::Frontmatter::default();
        let prompt = build_prompt(RunMode::from_frontmatter(&fm), &fm, "diff", "doc");
        assert!(prompt.contains("patch:exchange"));
        assert!(!prompt.contains("## Assistant heading"));
    }

    #[test]
    fn build_prompt_append_mode_uses_inline_contract() {
        let fm = frontmatter::Frontmatter {
            format: Some(frontmatter::AgentDocFormat::Append),
            ..Default::default()
        };
        let prompt = build_prompt(RunMode::from_frontmatter(&fm), &fm, "diff", "doc");
        assert!(prompt.contains("Do not include a ## Assistant heading"));
        assert!(!prompt.contains("patch:exchange"));
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
        let prompt = build_prompt(RunMode::Template, &fm, diff, "doc");
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

        let prompt = build_prompt(RunMode::Template, &fm, "diff", doc);
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

        apply_template_response(&doc, baseline, response, false)
            .expect("run path should normalize legacy backlog patches before enforcement");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] added item"));
        assert!(updated.contains("- [ ] [#keep1] existing item"));
    }

    #[test]
    fn apply_template_response_normalizes_monsterrodholders_style_backlog_patch() {
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
            "### Re: monsterrodholders backlog follow-up — gpt-5\n\n",
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

        apply_template_response(&doc, baseline, response, false)
            .expect("run path should normalize monsterrodholders-style backlog patches");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("### Re: monsterrodholders backlog follow-up — gpt-5"));
        assert!(updated.contains("- [ ] [#new1] Verify direct rerun completed cleanly"));
        assert!(updated.contains("- [ ] [#yckq] [#ss01] ShipStation fix"));
        assert!(updated.contains("- [ ] [#2gdt] [#wpmem] WP memory limits"));
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
        snapshot::save(&doc, baseline).unwrap();

        let err = run(&doc, false, None, None, true, true, &Config::default())
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
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("overwrite.md");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, "new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
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
                atomic_write(&p, &content).unwrap();
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
            atomic_write(&path_a, &format!("{}\n## Assistant\nResponse A", content)).unwrap();
        });

        let bar_b = Arc::clone(&barrier);
        let path_b = doc_b.clone();
        let hb = std::thread::spawn(move || {
            let _lock = acquire_doc_lock(&path_b).unwrap();
            bar_b.wait(); // both threads hold their own lock simultaneously
            let content = std::fs::read_to_string(&path_b).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
            atomic_write(&path_b, &format!("{}\n## Assistant\nResponse B", content)).unwrap();
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
                atomic_write(&path, &updated).unwrap();
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
            atomic_write(&path_w, "after").unwrap();
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
