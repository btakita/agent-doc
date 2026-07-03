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

use agent_doc_document::queue_projection::strip_in_progress_marker;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
use agent_doc_session_accretion::SessionAccretionReport;
use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use agent_doc_element::element;

use agent_doc_config::Config;
use agent_doc_diff as diff;
use agent_doc_frontmatter::frontmatter;
#[cfg(test)]
use agent_doc_prompt_cache::{PROMPT_CACHE_BOUNDARY, PROMPT_CACHE_CONTROL};
use agent_doc_prompt_cache::{
    PromptCacheBlocks, PromptCacheSessionCostSample, render_cache_miss_ranking,
};
use agent_doc_template as template;
use agent_doc_turn::no_change::{
    NoChangeCycleStateInput, NoChangeVerdict, classify_no_change_cycle_state,
};
use agent_doc_turn::owner_pane_recursion::{
    OwnerPaneQueueHead, owner_pane_wedge_threshold_reached, prompt_miss_message,
    queue_handoff_message, queue_wedge_halt_message, recursive_direct_invocation_message,
    recursive_start_invocation_message,
};
use agent_doc_workflow::owner_pane_self_invocation::{
    OwnedPaneSelfInvocation, OwnedPaneSelfInvocationInput, OwnedPaneSelfInvocationKind,
    OwnedPaneSelfInvocationOptions, build_owned_pane_self_invocation,
};

use agent_doc_agent_io::agent;

use crate::{git, write};

const AGENT_DOC_RUN_HEARTBEAT_SECS_ENV: &str = "AGENT_DOC_RUN_HEARTBEAT_SECS";
const DEFAULT_RUN_HEARTBEAT_SECS: u64 = 30;

#[cfg(unix)]
struct RunStderrRedirect {
    saved_stderr: Option<OwnedFd>,
}

#[cfg(unix)]
impl RunStderrRedirect {
    fn inactive() -> Self {
        Self { saved_stderr: None }
    }

    fn maybe_start(file: &Path) -> Self {
        if !run_stderr_redirect_needed() {
            return Self::inactive();
        }
        match Self::start(file) {
            Ok(guard) => guard,
            Err(err) => {
                eprintln!("[run] warning: could not redirect stderr for managed TUI: {err:#}");
                Self::inactive()
            }
        }
    }

    fn start(file: &Path) -> Result<Self> {
        let canonical = file
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", file.display()))?;
        let project_root = agent_doc_project_root_io::project_root_containing(&canonical)
            .with_context(|| format!("failed to resolve project root for {}", file.display()))?;
        let logs_dir = project_root.join(".agent-doc").join("logs");
        std::fs::create_dir_all(&logs_dir)
            .with_context(|| format!("failed to create {}", logs_dir.display()))?;
        let stderr_path = logs_dir.join("run-stderr.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .with_context(|| format!("failed to open {}", stderr_path.display()))?;
        let saved_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
        if saved_fd < 0 {
            anyhow::bail!("dup(stderr) failed: {}", std::io::Error::last_os_error());
        }
        let saved_stderr = unsafe { OwnedFd::from_raw_fd(saved_fd) };
        let redirected = unsafe { libc::dup2(log_file.as_raw_fd(), libc::STDERR_FILENO) };
        if redirected < 0 {
            anyhow::bail!("dup2(stderr) failed: {}", std::io::Error::last_os_error());
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "run_stderr_redirect harness={} tmux_pane={} target={}",
                agent_doc_model_tier::detect_harness(),
                std::env::var("TMUX_PANE").unwrap_or_else(|_| "<unset>".to_string()),
                stderr_path.display()
            ),
        );
        eprintln!(
            "[run] stderr redirected to {} for managed TUI",
            stderr_path.display()
        );
        Ok(Self {
            saved_stderr: Some(saved_stderr),
        })
    }
}

#[cfg(unix)]
impl Drop for RunStderrRedirect {
    fn drop(&mut self) {
        let Some(saved_stderr) = self.saved_stderr.take() else {
            return;
        };
        let restored = unsafe { libc::dup2(saved_stderr.as_raw_fd(), libc::STDERR_FILENO) };
        if restored < 0 {
            let msg = b"[run] warning: failed to restore stderr after managed TUI redirect\n";
            unsafe {
                libc::write(
                    saved_stderr.as_raw_fd(),
                    msg.as_ptr().cast::<libc::c_void>(),
                    msg.len(),
                );
            }
        }
    }
}

#[cfg(not(unix))]
struct RunStderrRedirect;

#[cfg(not(unix))]
impl RunStderrRedirect {
    fn inactive() -> Self {
        Self
    }

    fn maybe_start(_file: &Path) -> Self {
        Self
    }
}

fn run_stderr_redirect_needed() -> bool {
    if agent_doc_tmux_commands::input_diag::verbose_enabled()
        || std::env::var_os("TMUX_PANE").is_none()
    {
        return false;
    }
    if std::env::var_os("AGENT_DOC_FORCE_RUN_STDERR_REDIRECT").is_none()
        && !std::io::stderr().is_terminal()
    {
        return false;
    }
    run_stderr_redirect_harness(agent_doc_model_tier::detect_harness().as_str())
}

fn run_stderr_redirect_harness(harness: &str) -> bool {
    matches!(harness, "claude" | "codex" | "opencode")
}

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

    fn cache_label(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Template => "template",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunCycleOutcome {
    dispatched: bool,
    queue_synthetic_diff: bool,
    queue_consumption: Option<write::QueueConsumptionOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoQueueContinuation {
    Stop,
    Continue { force_fresh_agent_session: bool },
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &Config,
) -> Result<()> {
    run_with_context(
        file, branch, agent_name, model, dry_run, no_git, force_disk, config, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_context(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &Config,
    run_context: Option<&crate::graph::RunContext>,
) -> Result<()> {
    let _stderr_redirect = if !dry_run && file.exists() {
        RunStderrRedirect::maybe_start(file)
    } else {
        RunStderrRedirect::inactive()
    };
    // #jb-tsift-pane-sync diagnostic: log if this run is executing inside a tmux
    // pane that owns a *different* document (cross-document contamination vector
    // — e.g. a tsift.md-owned pane running agent-doc-bugs2.md's cycle). The
    // same-document recursion guard below only catches same-document re-entry.
    crate::sync::log_cross_document_execution_context(file, "run");

    let mut create_branch = branch;
    let mut completed_queue_items = 0usize;
    let mut force_fresh_agent_session = false;
    let mut last_context_clear_at = None;

    loop {
        let used_fresh_agent_session = force_fresh_agent_session;
        let outcome = run_once(
            file,
            create_branch,
            agent_name,
            model,
            dry_run,
            no_git,
            force_disk,
            config,
            run_context,
            force_fresh_agent_session,
        )?;
        create_branch = false;
        if used_fresh_agent_session {
            last_context_clear_at = Some(current_epoch_secs());
        }

        if !outcome.dispatched {
            return Ok(());
        }
        if let Some(queue_consumption) = outcome.queue_consumption.as_ref() {
            completed_queue_items += queue_consumption.consumed_count.max(1);
        }
        match should_continue_auto_queue(
            file,
            &outcome,
            completed_queue_items,
            no_git,
            last_context_clear_at,
        )? {
            AutoQueueContinuation::Stop => return Ok(()),
            AutoQueueContinuation::Continue {
                force_fresh_agent_session: fresh,
            } => force_fresh_agent_session = fresh,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_once(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &Config,
    run_context: Option<&crate::graph::RunContext>,
    force_fresh_agent_session: bool,
) -> Result<RunCycleOutcome> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    eprintln!("[run] starting for {}", file.display());

    // Compute diff
    let Some((the_diff, queue_synthetic_diff)) = compute_run_diff(file)? else {
        let cycle_state = agent_doc_cycle_state_io::load(file)?;
        let no_change_input = cycle_state.as_ref().map(|state| NoChangeCycleStateInput {
            cycle_id: &state.cycle_id,
            file: &state.file,
            phase: state.phase,
            last_event: &state.last_event,
            has_capture: state.capture_id.is_some(),
            has_response_hash: state.response_sha256.is_some(),
            had_pending_mutations: state.had_pending_mutations,
            has_pending_done_ids: !state.pending_done_ids.is_empty(),
            has_pending_kept_open_ids: !state.pending_kept_open_ids.is_empty(),
            has_reaped_pending_ids: !state.reaped_pending_ids.is_empty(),
            has_pending_gated_ids: !state.pending_gated_ids.is_empty(),
            pending_added_this_cycle: state.pending_added_this_cycle,
        });
        match classify_no_change_cycle_state(no_change_input) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                eprintln!(
                    "[run] no document changes since last run for {}, but {}. Recovery: {}",
                    file.display(),
                    summary,
                    recovery
                );
            }
            NoChangeVerdict::Clean => {
                eprintln!(
                    "[run] Nothing changed since last run for {}",
                    file.display()
                );
            }
        }
        return Ok(RunCycleOutcome {
            dispatched: false,
            queue_synthetic_diff: false,
            queue_consumption: None,
        });
    };
    write::guard_no_exchange_compaction_request_for_diff(file, &the_diff)?;

    // Ensure the document has a session UUID (for tmux routing)
    let raw_content = std::fs::read_to_string(file)?;
    // Opt-in gate: a plain `.md` must not be auto-converted into a session.
    agent_doc_frontmatter_io::session::require_agent_doc_document(&raw_content, file)?;
    let (mut content_original, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&raw_content, file)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    if !dry_run {
        let early_rc = crate::graph::RunContext::new(file.to_path_buf());
        let (early_fm, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
            &content_original,
            file,
            &early_rc.ssh_context(),
        )?;
        let early_agent_name = agent_name
            .or(early_fm.agent.as_deref())
            .or(config.default_agent.as_deref())
            .unwrap_or("claude");
        if let Some(detail) = owned_pane_self_invocation_detail(file, &session_id, early_agent_name)
            && let Some(continuation) = crate::queue_continuation::detect(file)?
            && !queue_synthetic_diff
            && owner_pane_queue_edit_should_defer_until_closeout(file, &the_diff, &content_original)
        {
            return Ok(owner_pane_queue_edit_deferred_outcome(
                file,
                queue_synthetic_diff,
                &detail,
                &continuation,
            ));
        }
    }
    content_original = normalize_direct_run_prompt_prefixes(file, &content_original, &the_diff)?;
    let queue_diff_completion_id =
        agent_doc_queue::queue_consume::queue_diff_completion_id_for_current_head(
            file,
            &content_original,
            &the_diff,
        )?;
    let owned_rc;
    let rc: &crate::graph::RunContext = if let Some(provided) = run_context {
        provided.set_file_path(file.to_path_buf());
        provided
    } else {
        owned_rc = crate::graph::RunContext::new(file.to_path_buf());
        &owned_rc
    };
    let (fm, _body) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content_original,
        file,
        &rc.ssh_context(),
    )?;
    let mut prompt_fm = fm.clone();
    if force_fresh_agent_session && prompt_fm.resume.is_some() {
        eprintln!(
            "[run] queue context reset: starting a fresh agent session for {}",
            file.display()
        );
        prompt_fm.resume = None;
    }
    let run_mode = RunMode::from_frontmatter(&prompt_fm);

    // Resolve agent
    let agent_name = agent_name
        .or(fm.agent.as_deref())
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let agent_config = config.agents.get(agent_name);
    let harness = agent_doc_model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = model
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc_model_tier::canonical_model_name(m, &harness, &config.model));
    let prompt_cache_routing_affinity =
        prompt_cache_routing_affinity(run_mode, agent_name, resolved_model.as_deref());

    // Expand frontmatter env vars (applied to the spawned agent child process).
    let expanded_env = if fm.env.is_empty() {
        Vec::new()
    } else {
        match agent_doc_config::env::expand_values(&fm.env) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[run] env expansion failed: {} — continuing without env", e);
                Vec::new()
            }
        }
    };

    let backend = agent::resolve_for_file(agent_name, agent_config, expanded_env, file, &fm)?;

    let session_accretion = agent_doc_session_accretion_io::inspect(file).ok();
    let prompt = build_prompt(
        file,
        run_mode,
        &prompt_fm,
        &the_diff,
        &content_original,
        session_accretion.as_ref(),
    );

    if dry_run {
        eprintln!("--- Diff ---");
        print!("{}", the_diff);
        eprintln!("--- Prompt would be {} bytes ---", prompt.len());
        if let Some(blocks) = PromptCacheBlocks::from_rendered(&prompt) {
            let replay_key = blocks.replay_key(&prompt_cache_routing_affinity);
            let adapter_state = if prompt_fm.resume.is_some() {
                "resumed"
            } else {
                "fresh"
            };
            let current_cost =
                PromptCacheSessionCostSample::from_replay_key(&replay_key, adapter_state);
            eprintln!(
                "--- Prompt cache stable_prefix_sha256={} provider_cache_key={} cache_control={} routing_affinity={} ---",
                replay_key.stable_prefix_sha256,
                replay_key.provider_cache_key,
                replay_key.cache_control,
                replay_key.routing_affinity
            );
            eprintln!(
                "--- Prompt cache session_cost {} ---",
                render_cache_miss_ranking(None, &current_cost)
            );
        }
        return Ok(RunCycleOutcome {
            dispatched: false,
            queue_synthetic_diff,
            queue_consumption: None,
        });
    }

    // #codex-owned-pane-prompt-miss: when a Codex-owned pane re-invokes
    // `agent-doc <FILE>` for the document it already owns AND an unresolved
    // exchange prompt is still pending, fail closed *before* pre-commit and
    // before `start_run_cycle` opens a cycle. The late recursive-deadlock guard
    // further down would also refuse to dispatch a nested child, but only after
    // pre-commit baselined the prompt into HEAD — silently losing it as an
    // executable diff. Bailing here keeps the prompt uncommitted/executable and
    // tells the operator to answer it in this owner pane. The detector is a
    // strict subset of the recursive-guard case, so non-recursive runs are
    // unaffected.
    if let Some(detail) = owned_pane_self_invocation_detail(file, &session_id, agent_name)
        && let Some(unresolved) = crate::session_check::unresolved_exchange_prompt(file)?
    {
        let document = file.display().to_string();
        let diagnostic = prompt_miss_message(&document, &detail, &unresolved);
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "run_owned_pane_prompt_miss file={} {}",
                file.display(),
                detail
            ),
        );
        anyhow::bail!("{}", diagnostic);
    }

    // #codex-owned-pane-auto-queue-stuck: when a Codex-owned pane re-invokes
    // `agent-doc <FILE>` for the document it already owns AND a ready active
    // auto-queue head remains (an unresolved exchange prompt takes precedence and
    // is handled by the guard above), fail closed *before* pre-commit and child
    // dispatch. The late recursive-deadlock guard further down would otherwise
    // let pre-commit baseline queue/boundary drift and leave the head unprocessed
    // with no owner-pane handoff, so the operator gets a retry loop. Bailing here
    // keeps the queue head live/executable and tells the operator to run the head
    // in THIS owner turn rather than re-running the same direct command. The
    // detector is a strict subset of the recursive-guard case, so non-owner and
    // non-Codex runs are unaffected.
    if let Some(detail) = owned_pane_self_invocation_detail(file, &session_id, agent_name)
        && let Some(continuation) = crate::queue_continuation::detect(file)?
    {
        if !queue_synthetic_diff
            && owner_pane_queue_edit_should_defer_until_closeout(file, &the_diff, &content_original)
        {
            return Ok(owner_pane_queue_edit_deferred_outcome(
                file,
                queue_synthetic_diff,
                &detail,
                &continuation,
            ));
        }
        // #recguard-wedge-escape: a busy owner pane re-invoking `agent-doc <FILE>`
        // mid-turn trips this guard (Option B `#codex-self-reinvoke-prevent` only
        // redirects the *Stop-hook* continuation, not a mid-turn re-run). One
        // transient self-invoke is normal, but the SAME head tripping this guard
        // enough times to hit the owner-pane wedge threshold is a self-driving
        // `agent:queue auto` dead-loop with no operator watching — it would re-fire forever. Break
        // it: halt the runaway auto-queue (`queue: stop`) so the loop stops
        // burning cycles, and hand the operator one clear recovery action.
        let wedge_count = agent_doc_owner_pane_io::record(file, &continuation.head_prompt)?;
        if owner_pane_wedge_threshold_reached(wedge_count) {
            if let Ok(content) = std::fs::read_to_string(file)
                && let Ok(stopped) = frontmatter::merge_queue_state(&content, false)
                && let Err(err) = std::fs::write(file, &stopped)
            {
                eprintln!(
                    "[recguard-wedge] WARNING: failed to halt wedged auto-queue for {}: {}",
                    file.display(),
                    err
                );
            }
            agent_doc_owner_pane_io::clear(file)?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "recursive_self_invocation_wedge_halt file={} head_id={} count={} {}",
                    file.display(),
                    continuation.head_id.as_deref().unwrap_or("<none>"),
                    wedge_count,
                    detail
                ),
            );
            anyhow::bail!(
                "{}",
                queue_wedge_halt_message(
                    &file.display().to_string(),
                    &detail,
                    OwnerPaneQueueHead {
                        prompt: &continuation.head_prompt,
                        id: continuation.head_id.as_deref(),
                    },
                    wedge_count
                )
            );
        }
        let document = file.display().to_string();
        let diagnostic = queue_handoff_message(
            &document,
            &detail,
            OwnerPaneQueueHead {
                prompt: &continuation.head_prompt,
                id: continuation.head_id.as_deref(),
            },
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "run_owned_pane_queue_handoff file={} head_id={} self_invocation_count={} {}",
                file.display(),
                continuation.head_id.as_deref().unwrap_or("<none>"),
                wedge_count,
                detail
            ),
        );
        anyhow::bail!("{}", diagnostic);
    }

    // Create branch if requested
    if branch && !no_git {
        agent_doc_git_io::branch::create_session_branch(file)?;
    }

    // Pre-commit: commit user's changes before sending to agent
    // This lets the editor show agent additions as diff gutters
    if !no_git {
        let did_commit = git::commit(file)?;
        if !did_commit
            && !queue_synthetic_diff
            && agent_doc_diff_io::compute(
                &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
                file,
            )?
            .is_none()
        {
            anyhow::bail!(
                "no child-agent dispatch: the pre-commit repair closed {} as already committed and no new assistant response body was supplied. If you need to recover a missed response patchback, pipe the response through `agent-doc write --commit {}`.",
                file.display(),
                file.display()
            );
        }
    }
    start_run_cycle(file)?;

    eprintln!("Submitting to {}...", agent_name);
    if let Some(diagnostic) =
        recursive_codex_direct_invocation_diagnostic(file, &session_id, agent_name)
    {
        // The recursive same-pane guard fires before any response capture, so the
        // empty preflight cycle just opened by `start_run_cycle` must be marked
        // terminal (`Abandoned`) rather than left `preflight_started`. Otherwise
        // `session-check` reports an interruption and the owner session stays
        // wedged until a manual `agent-doc cancel`. (#recguard-abandon)
        abandon_run_recursive_cycle(file, "recursive_direct_invocation_blocked", &diagnostic)?;
        anyhow::bail!("{}", diagnostic);
    }

    // Send to agent — use `resume` for agent conversation tracking
    let fork = prompt_fm.resume.is_none();
    let response_result = {
        let _heartbeat = RunHeartbeat::start(
            file,
            "child_agent_wait",
            agent_name,
            Some(agent_doc_agent_io::agent::run_agent_timeout()),
        );
        backend.send(
            &prompt,
            prompt_fm.resume.as_deref(),
            fork,
            resolved_model.as_deref(),
        )
    };
    let response = match response_result {
        Ok(response) => response,
        Err(err) if agent_doc_harness::timeout::error_chain_is_timeout(&err) => {
            let diagnostic = run_dispatch_timeout_diagnostic(file, agent_name);
            record_run_preflight_timeout(file, "direct_invocation_timeout", &diagnostic)?;
            anyhow::bail!("{}\n\nsource: {}", diagnostic, err);
        }
        Err(err) => return Err(err),
    };

    let response_text = match run_mode {
        RunMode::Append => agent_doc_turn::response_text::strip_assistant_heading(&response.text),
        RunMode::Template => response.text.clone(),
    };
    write::enforce_imperative_response_contract_for_diff(file, &the_diff, &response_text)?;
    record_run_progress(file, "response_capture", agent_name, None);
    crate::repair::save_pending(file, &response_text)?;

    record_run_progress(file, "response_write", agent_name, None);
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

    let mut queue_consumption = None;
    if !no_git {
        let _heartbeat = RunHeartbeat::start(file, "commit_closeout", agent_name, None);
        if queue_synthetic_diff
            || write::should_consume_queue_prompt_for_diff(file, Some(&the_diff))?
        {
            let queue_completion_ids = queue_diff_completion_id
                .clone()
                .into_iter()
                .collect::<Vec<_>>();
            queue_consumption = if force_disk {
                write::consume_queue_prompts_with_outcome(file, &queue_completion_ids, true)?
            } else {
                write::consume_queue_prompts_for_done_ids_with_outcome(file, &queue_completion_ids)?
            };
        } else {
            eprintln!("{}", write::queue_skip_diagnostic_for_file(file)?);
        }
        write::complete_required_closeout(file)?;
    }

    eprintln!("Response written to {}", file.display());
    Ok(RunCycleOutcome {
        dispatched: true,
        queue_synthetic_diff,
        queue_consumption,
    })
}

fn compute_run_diff(file: &Path) -> Result<Option<(String, bool)>> {
    if let Some(d) = agent_doc_diff_io::compute(
        &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
        file,
    )? {
        eprintln!("[run] diff computed ({} bytes)", d.len());
        return Ok(Some((d, false)));
    }

    if let Some(d) = active_queue_prompt_diff(file)? {
        eprintln!("[run] active queue head synthesized as prompt diff");
        return Ok(Some((d, true)));
    }

    Ok(None)
}

fn active_queue_prompt_diff(file: &Path) -> Result<Option<String>> {
    let ActiveQueuePromptState::Ready { prompt } = active_queue_prompt_state(file)? else {
        return Ok(None);
    };
    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(&prompt) {
        eprintln!(
            "[run] active queue head is slash command {command:?}; leaving it for the managed supervisor to submit after the owner pane is idle"
        );
        return Ok(None);
    }
    Ok(Some(diff::synthetic_added_lines_diff(&prompt, "queue")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveQueuePromptState {
    Ready {
        prompt: String,
    },
    Inactive,
    StopFence {
        next_prompt: Option<String>,
    },
    TimeGate {
        start_at: String,
        next_prompt: Option<String>,
    },
    ItemModified {
        snapshot_head: Option<String>,
        document_head: Option<String>,
    },
    Unproven {
        reason: String,
        document_head: Option<String>,
    },
    Empty,
}

fn active_queue_prompt_state(file: &Path) -> Result<ActiveQueuePromptState> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content,
        file,
        &rc.ssh_context(),
    )?;
    if fm.queue_active != Some(true) {
        return Ok(ActiveQueuePromptState::Inactive);
    }

    let components = element::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(ActiveQueuePromptState::Inactive);
    };
    if !agent_doc_queue::control_binding::explicit_queue_go_mode(
        &queue_component.attrs,
        fm.queue.as_deref(),
    ) {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("run queue resume: failed to parse document queue")?;
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    let activation =
        agent_doc_queue::document_queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let document_head = agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
        .map(|prompt| strip_in_progress_marker(&prompt.text));
    if document_head.is_none() {
        return Ok(ActiveQueuePromptState::Empty);
    };

    if let Some(state) = typed_queue_prompt_state(file, &content) {
        return Ok(state);
    }

    eprintln!(
        "[run] active queue has no current typed selected/deferred head; refusing markdown fallback"
    );
    Ok(ActiveQueuePromptState::Unproven {
        reason: "missing_or_stale_typed_queue_head".to_string(),
        document_head,
    })
}

fn typed_queue_prompt_state(file: &Path, content: &str) -> Option<ActiveQueuePromptState> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let ledger = crate::project_controller::load_state_event_ledger(&project_root).ok()?;
    let projection = ledger.project_document(&document_hash)?;
    let current_nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").ok()?;
    let current_head = current_nodes.iter().find(|node| !node.item.struck)?;
    let head = projection.queue.heads.get(&current_head.node_key)?;
    match head.phase {
        agent_doc_state_backbone::QueueHeadPhase::Selected => {
            if projection.queue.active_head.as_deref() != Some(current_head.node_key.as_str()) {
                return None;
            }
            let prompt = head.prompt_text.clone()?;
            Some(ActiveQueuePromptState::Ready { prompt })
        }
        agent_doc_state_backbone::QueueHeadPhase::Deferred => {
            let reason = head.defer_reason.as_deref()?;
            if reason == "stop_fence" {
                eprintln!("[run] active queue halted by typed stop-fence state");
                return Some(ActiveQueuePromptState::StopFence {
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if let Some(start_at) = reason.strip_prefix("time_gate:") {
                eprintln!("[run] active queue deferred by typed time-gate state: {start_at}");
                return Some(ActiveQueuePromptState::TimeGate {
                    start_at: start_at.to_string(),
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if reason == "item_modified" {
                eprintln!("[run] active queue halted by typed item-modified state");
                return Some(ActiveQueuePromptState::ItemModified {
                    snapshot_head: None,
                    document_head: head.prompt_text.clone(),
                });
            }
            None
        }
        _ => None,
    }
}

fn should_continue_auto_queue(
    file: &Path,
    outcome: &RunCycleOutcome,
    completed_queue_items: usize,
    no_git: bool,
    last_context_clear_at: Option<u64>,
) -> Result<AutoQueueContinuation> {
    if no_git || !outcome.queue_synthetic_diff {
        return Ok(AutoQueueContinuation::Stop);
    }
    let Some(queue) = outcome.queue_consumption.as_ref() else {
        return Ok(AutoQueueContinuation::Stop);
    };
    // `auto` and `start` are start triggers only. Continuation is driven by
    // typed active queue state plus explicit `go` mode; a persisted
    // `queue_active: true` plain queue stays inert. The
    // `active_queue_prompt_state` re-check below still halts on typed stop fence
    // / time gate / head-modified / inactive / empty, and refuses markdown-only
    // fallback.
    if queue.drained || queue.remaining == 0 {
        return Ok(AutoQueueContinuation::Stop);
    }

    match active_queue_prompt_state(file)? {
        ActiveQueuePromptState::Ready { prompt } => {
            let force_fresh_agent_session =
                match agent_doc_session_accretion_io::queue_context_reset_reason_if_opted_in(
                    file,
                    last_context_clear_at,
                ) {
                    Ok(Some(reason)) => {
                        eprintln!(
                            "[queue] queue continuation will start a fresh agent session before next prompt: {}",
                            reason
                        );
                        true
                    }
                    Ok(None) => false,
                    Err(err) => {
                        eprintln!(
                            "[queue] warning: failed to inspect queue context reset policy for {}: {}",
                            file.display(),
                            err
                        );
                        false
                    }
                };
            eprintln!(
                "[queue] queue continuation: completed {} item(s); launching next prompt: {:?}",
                completed_queue_items, prompt
            );
            Ok(AutoQueueContinuation::Continue {
                force_fresh_agent_session,
            })
        }
        ActiveQueuePromptState::StopFence { next_prompt } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): stop_fence before next prompt {:?}",
                completed_queue_items, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::TimeGate {
            start_at,
            next_prompt,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): time_gate {} before next prompt {:?}",
                completed_queue_items, start_at, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::ItemModified {
            snapshot_head,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): item_modified snapshot_head={:?} document_head={:?}",
                completed_queue_items, snapshot_head, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Unproven {
            reason,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): unproven_typed_queue_state reason={} document_head={:?}",
                completed_queue_items, reason, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Inactive => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): queue_inactive",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Empty => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): no_remaining_prompt",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
    }
}

struct RunHeartbeat {
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl RunHeartbeat {
    fn start(
        file: &Path,
        phase: &'static str,
        agent_name: &str,
        timeout: Option<Duration>,
    ) -> Self {
        let file = file.to_path_buf();
        let agent_name = agent_name.to_string();
        let (stop, stop_rx) = mpsc::channel();
        let interval = run_heartbeat_interval();
        let handle = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                if stop_rx.recv_timeout(interval).is_ok() {
                    break;
                }
                let elapsed = started.elapsed();
                let timeout_detail = timeout
                    .map(|timeout| format!(" timeout_s={}", timeout.as_secs()))
                    .unwrap_or_default();
                let event = format!(
                    "run_heartbeat phase={} agent={} elapsed_s={}{}",
                    phase,
                    agent_name,
                    elapsed.as_secs(),
                    timeout_detail
                );
                let state = agent_doc_cycle_state_io::record_open_cycle_progress(&file, &event)
                    .ok()
                    .flatten();
                let (cycle_id, cycle_phase, last_event_age) = state
                    .as_ref()
                    .map(|state| {
                        (
                            state.cycle_id.as_str(),
                            state.phase.as_str(),
                            state.updated_at.saturating_sub(state.started_at),
                        )
                    })
                    .unwrap_or(("<unknown>", "<unknown>", 0));
                eprintln!(
                    "[run] heartbeat phase={} elapsed_s={}{} cycle_id={} cycle_phase={} cycle_age_s={}",
                    phase,
                    elapsed.as_secs(),
                    timeout_detail,
                    cycle_id,
                    cycle_phase,
                    last_event_age
                );
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for RunHeartbeat {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_heartbeat_interval() -> Duration {
    let secs = std::env::var(AGENT_DOC_RUN_HEARTBEAT_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RUN_HEARTBEAT_SECS);
    Duration::from_secs(secs.max(1))
}

fn record_run_progress(file: &Path, phase: &str, agent_name: &str, timeout: Option<Duration>) {
    let timeout_detail = timeout
        .map(|timeout| format!(" timeout_s={}", timeout.as_secs()))
        .unwrap_or_default();
    let event = format!("run_progress phase={phase} agent={agent_name}{timeout_detail}");
    let _ = agent_doc_cycle_state_io::record_open_cycle_progress(file, &event);
    eprintln!("[run] progress phase={phase}{timeout_detail}");
}

fn mark_run_write_applied(file: &Path, event: &str) -> Result<()> {
    let file_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {} after run write", file.display()))?;
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    agent_doc_cycle_state_io::mark_write_applied(
        file,
        event,
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

fn start_run_cycle(file: &Path) -> Result<()> {
    agent_doc_cycle_state_io::admit_with_current_resolver(
        file,
        |file, disk| crate::realtime_model::resolve_current_doc(file, disk).content,
        agent_doc_snapshot_io::load,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

fn record_run_preflight_timeout(file: &Path, event: &str, diagnostic: &str) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    agent_doc_cycle_state_io::mark_recoverable_preflight_timeout(file, &event)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_preflight_timeout file={} event={} diagnostic={}",
            file.display(),
            event.split_whitespace().next().unwrap_or(event.as_str()),
            compact
        ),
    );
    Ok(())
}

/// Mark the empty preflight cycle opened by `start_run_cycle` as terminal
/// (`Abandoned`) when a fail-fast guard refuses to dispatch before any response
/// capture (e.g. recursive same-pane direct invocation). Unlike
/// [`record_run_preflight_timeout`], which leaves the cycle `preflight_started`
/// (recoverable/open) for genuine hangs that may still complete, this records a
/// terminal state `session-check` accepts immediately — no manual
/// `agent-doc cancel` required. (#recguard-abandon)
fn abandon_run_recursive_cycle(file: &Path, event: &str, diagnostic: &str) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    let file_content = std::fs::read_to_string(file).ok();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
        &crate::PIPELINE_FRONTMATTER_EFFECTS,
        file,
        &event,
        snapshot_content.as_deref(),
        file_content.as_deref(),
    )?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_recursive_direct_invocation_abandoned file={} diagnostic={}",
            file.display(),
            compact
        ),
    );
    Ok(())
}

/// Shared owner-pane self-invocation detector. Returns the
/// `current_pane=… session_id=… actor_*=…` detail string when a Codex
/// `agent-doc <FILE>` direct invocation is running inside the pane that already
/// owns the document, else `None`. Both the early `#codex-owned-pane-prompt-miss`
/// fail-closed guard and the late `#recguard-abandon` deadlock guard key off
/// this single detector so they cannot disagree about ownership.
pub(crate) fn owned_pane_self_invocation_detail(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    if agent_name != "codex" || agent_doc_model_tier::detect_harness() != "codex" {
        return None;
    }
    let tmux = tmux_router::Tmux::default_server();
    let current_pane = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux)?;
    let registry_match = agent_doc_session_registry_io::lookup_entry(session_id)
        .ok()
        .flatten()
        .filter(|entry| entry.pane == current_pane);
    let actor = actor_record_for_file(file).ok().flatten();
    let actor_match = actor
        .as_ref()
        .is_some_and(|record| record.pane_id == current_pane);
    if registry_match.is_none() && !actor_match {
        return None;
    }
    let actor_detail = actor
        .as_ref()
        .map(|record| {
            format!(
                "actor_generation={} actor_state={} actor_pane={}",
                record.generation,
                record.state.as_str(),
                record.pane_id
            )
        })
        .unwrap_or_else(|| {
            "actor_generation=<unknown> actor_state=<unknown> actor_pane=<unknown>".to_string()
        });
    Some(format!(
        "current_pane={} session_id={} {}",
        current_pane, session_id, actor_detail
    ))
}

/// Detect a structured owner-pane self-invocation contract. An unresolved
/// exchange prompt takes precedence over an active auto-queue head
/// (`#prompt-preempts-auto-queue`). Returns `None` when this is not a Codex
/// owner-pane self-invocation, or when there is no unresolved exchange work.
///
/// `unresolved_prompt` is supplied by the caller because the boundary-keyed
/// [`crate::session_check::unresolved_exchange_prompt`] detector only sees a
/// prompt *before* the cycle's commit inserts a trailing boundary. The run path
/// passes that pre-commit detector's result; preflight (which runs after commit)
/// passes the diff-derived unresolved prompt so the contract survives the
/// boundary insertion.
pub fn detect_owned_pane_self_invocation(
    file: &Path,
    session_id: &str,
    agent_name: &str,
    unresolved_prompt: Option<String>,
) -> Result<Option<OwnedPaneSelfInvocation>> {
    detect_owned_pane_self_invocation_with_options(
        file,
        session_id,
        agent_name,
        unresolved_prompt,
        OwnedPaneSelfInvocationOptions::default(),
    )
}

pub(crate) fn detect_owned_pane_self_invocation_with_options(
    file: &Path,
    session_id: &str,
    agent_name: &str,
    unresolved_prompt: Option<String>,
    options: OwnedPaneSelfInvocationOptions,
) -> Result<Option<OwnedPaneSelfInvocation>> {
    if owned_pane_self_invocation_detail(file, session_id, agent_name).is_none() {
        return Ok(None);
    }
    let tmux = tmux_router::Tmux::default_server();
    let current_pane =
        agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux).unwrap_or_default();
    let actor = actor_record_for_file(file).ok().flatten();
    let actor_generation = actor.as_ref().map(|record| record.generation);
    let actor_state = actor
        .as_ref()
        .map(|record| record.state.as_str().to_string());
    let file_display = file.display().to_string();
    if let Some(unresolved) = unresolved_prompt.filter(|p| !p.trim().is_empty()) {
        return Ok(Some(build_owned_pane_self_invocation(
            OwnedPaneSelfInvocationInput {
                file: &file_display,
                current_pane: &current_pane,
                session_id,
                actor_generation,
                actor_state: actor_state.as_deref(),
                kind: OwnedPaneSelfInvocationKind::UnresolvedPrompt,
                work: &unresolved,
                head_id: None,
            },
        )));
    }
    if !options.suppress_active_queue_head
        && let Some(continuation) = crate::queue_continuation::detect(file)?
    {
        return Ok(Some(build_owned_pane_self_invocation(
            OwnedPaneSelfInvocationInput {
                file: &file_display,
                current_pane: &current_pane,
                session_id,
                actor_generation,
                actor_state: actor_state.as_deref(),
                kind: OwnedPaneSelfInvocationKind::ActiveQueueHead,
                work: &continuation.head_prompt,
                head_id: continuation.head_id.as_deref(),
            },
        )));
    }
    Ok(None)
}

fn owner_pane_queue_edit_should_defer_until_closeout(
    file: &Path,
    diff_text: &str,
    current_content: &str,
) -> bool {
    let open_cycle = agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .is_some_and(|state| state.is_open());
    if !open_cycle {
        return false;
    }
    let Some(scope) = agent_doc_turn_scope_io::load(file) else {
        return false;
    };
    let prompt_bearing_changes = diff::classify_prompt_bearing_changes(diff_text);
    if prompt_bearing_changes.is_empty() {
        return false;
    }
    let Some(previous) = agent_doc_snapshot_io::load(file).ok().flatten() else {
        return false;
    };
    let Some(summary) = agent_doc_diff::semantic::semantic_diff_summary(
        &previous,
        current_content,
        &prompt_bearing_changes,
    ) else {
        return false;
    };
    let document_path = file.to_string_lossy().to_string();
    let ops =
        agent_doc_turn::op_log::build_ops_from_semantic_diff(&document_path, None, "", &summary);
    let affectedness = agent_doc_turn::turn_scope::classify_cycle(&ops, &scope);
    !affectedness.turn_affected
}

fn owner_pane_queue_edit_deferred_outcome(
    file: &Path,
    queue_synthetic_diff: bool,
    detail: &str,
    continuation: &agent_doc_queue::queue_continuation::QueueContinuation,
) -> RunCycleOutcome {
    eprintln!(
        "[run] owner-pane queue edit deferred until current closeout for {} (head_id={} {})",
        file.display(),
        continuation.head_id.as_deref().unwrap_or("<none>"),
        detail
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "run_owned_pane_queue_edit_deferred file={} head_id={} {}",
            file.display(),
            continuation.head_id.as_deref().unwrap_or("<none>"),
            detail
        ),
    );
    RunCycleOutcome {
        dispatched: false,
        queue_synthetic_diff,
        queue_consumption: None,
    }
}

fn recursive_codex_direct_invocation_diagnostic(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    let detail = owned_pane_self_invocation_detail(file, session_id, agent_name)?;
    Some(recursive_direct_invocation_message(
        &file.display().to_string(),
        &detail,
    ))
}

/// `#recursion-guard-wedge-escape` (part 1): fail-closed diagnostic for
/// `agent-doc start <FILE>` (or the bare `agent-doc <FILE>` start entry) invoked
/// inside the Codex pane that already owns the document. Unlike the `run` guard
/// above, the `start` path would otherwise *spawn a replacement owner in this
/// same pane*, which loops re-injecting `agent-doc <FILE>` into the owner pane —
/// the exact self-owned-pane recursion wedge with no clean operator escape.
/// Returns `None` when this is not a Codex owner-pane self-invocation.
pub fn recursive_codex_start_invocation_diagnostic(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    let detail = owned_pane_self_invocation_detail(file, session_id, agent_name)?;
    Some(recursive_start_invocation_message(
        &file.display().to_string(),
        &detail,
    ))
}

fn run_dispatch_timeout_diagnostic(file: &Path, agent_name: &str) -> String {
    let state = agent_doc_cycle_state_io::load(file).ok().flatten();
    let actor = actor_record_for_file(file).ok().flatten();
    let tmux = tmux_router::Tmux::default_server();
    let current_pane = agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux);
    let (cycle_id, phase, last_event) = state
        .as_ref()
        .map(|state| {
            (
                state.cycle_id.as_str(),
                state.phase.as_str(),
                state.last_event.as_str(),
            )
        })
        .unwrap_or(("<unknown>", "<unknown>", "<unknown>"));
    let actor_detail = actor
        .as_ref()
        .map(|record| {
            format!(
                "actor_generation={} actor_state={} actor_pane={}",
                record.generation,
                record.state.as_str(),
                record.pane_id
            )
        })
        .unwrap_or_else(|| {
            "actor_generation=<unknown> actor_state=<unknown> actor_pane=<unknown>".to_string()
        });
    format!(
        "direct `agent-doc {}` invocation timed out after waiting {}s for {} to return after preflight. cycle_id={} phase={} last_event={} current_pane={} {}. The cycle is recoverable; inspect with `agent-doc session-check {}` or restart the managed owner with `agent-doc start {}`.",
        file.display(),
        agent_doc_agent_io::agent::run_agent_timeout().as_secs(),
        agent_name,
        cycle_id,
        phase,
        last_event,
        current_pane.as_deref().unwrap_or("<unknown>"),
        actor_detail,
        file.display(),
        file.display()
    )
}

fn actor_record_for_file(
    file: &Path,
) -> Result<Option<agent_doc_sqlite::state_store::ActorRecord>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) else {
        return Ok(None);
    };
    let file_arg = canonical.to_string_lossy();
    agent_doc_session_actor_io::load_record_in(&project_root, &file_arg)
}

fn maybe_abort_after_write_applied_for_test() -> Result<()> {
    if std::env::var_os("AGENT_DOC_TEST_ABORT_AFTER_RUN_WRITE_APPLIED").is_some() {
        anyhow::bail!("test abort after run write_applied");
    }
    Ok(())
}

fn build_prompt(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let stable_prefix = build_prompt_stable_prefix(run_mode);
    let volatile_suffix =
        build_prompt_volatile_suffix(file, run_mode, fm, the_diff, content, session_accretion);
    PromptCacheBlocks::new(stable_prefix, volatile_suffix).render()
}

fn prompt_cache_routing_affinity(
    run_mode: RunMode,
    agent_name: &str,
    resolved_model: Option<&str>,
) -> String {
    format!(
        "agent_doc_run:v1;agent={agent_name};model={};mode={}",
        resolved_model.unwrap_or("<default>"),
        run_mode.cache_label()
    )
}

fn build_prompt_stable_prefix(run_mode: RunMode) -> String {
    let response_format = match run_mode {
        RunMode::Template => {
            "Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->"
        }
        RunMode::Append => {
            "Write your response in markdown.\n\
             Do not include a ## Assistant heading - it will be added automatically.\n\
             If the volatile payload contains inline prompt-bearing edits, classify them as prompt targets vs content edits before responding."
        }
    };
    format!(
        "<agent_doc_prompt_stable_prefix>\n\
         You are responding inside an agent-doc markdown session.\n\n\
         <response_contract>\n{}\n\
         </response_contract>\n\n\
         <turn_payload_contract>\n\
         Read the volatile turn payload after the cache boundary before acting. Queue heads, status advisories, compaction/accretion diagnostics, diffs, and document excerpts in that payload are current for this turn.\n\
         </turn_payload_contract>\n\
         </agent_doc_prompt_stable_prefix>",
        response_format
    )
}

fn build_prompt_volatile_suffix(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        agent_doc_prompt_context::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let ssh_context = rc.ssh_context();
    let document_section = agent_doc_prompt_context_io::build_document_section_with_ssh_context(
        file,
        the_diff,
        content,
        session_accretion,
        &ssh_context,
    );
    match (run_mode, fm.resume.is_some()) {
        (RunMode::Template, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Template, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
        (RunMode::Append, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Append, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. If the user asked questions or prompt-bearing edits inline (e.g., in blockquotes or prior responses), address those too.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
    }
}

fn apply_append_response(file: &Path, baseline: &str, response: &str) -> Result<()> {
    let doc_lock = acquire_doc_lock(file)?;
    agent_doc_snapshot_io::save_pre_response(file, baseline)?;

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
        agent_doc_merge_io::merge_contents(baseline, &content_ours, &content_current)?
    };

    write::guard_visible_write_idle(file, "direct_run_append")?;
    agent_doc_snapshot_io::save(file, &final_content, agent_doc_ops_log_io::log_op)?;
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
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let current_content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (mut patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;
    agent_doc_template::sanitize::sanitize_patches(&mut patches);
    let normalized =
        write::normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;
    write::enforce_no_replace_pending(&patches, false)?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }
    agent_doc_template::response_materialization::ensure_template_response_write_proof(
        &patches, &unmatched,
    )?;

    let doc_lock = acquire_doc_lock(file)?;
    agent_doc_snapshot_io::save_pre_response(file, baseline)?;

    let content_ours = agent_doc_template_io::apply_patches_with_project_config(
        baseline,
        &patches,
        &unmatched,
        file,
        Some(rc.project_config()),
    )
    .context("failed to apply template patches")?;
    let snapshot_doc = agent_doc_snapshot_io::load(file).ok().flatten();
    let content_ours = normalize_direct_run_template_content(
        file,
        baseline,
        snapshot_doc.as_deref(),
        &content_ours,
    )?;

    let content_current = std::fs::read_to_string(file)?;
    let (final_content, crdt_state) = if content_current == baseline {
        let state = if use_crdt {
            Some(agent_doc_merge::crdt::CrdtDoc::from_text(&content_ours).encode_state())
        } else {
            None
        };
        (content_ours, state)
    } else if use_crdt {
        eprintln!("File was modified during run. CRDT merging changes...");
        let base_state = agent_doc_snapshot_io::crdt_merge_base_state_with(
            file,
            baseline,
            agent_doc_op_capture_io::has_pending_editor_ops,
            agent_doc_ops_log_io::log_op,
        )?
        .state;
        // `#crdtauth4` — disk demotion (plan phase 6). Under `MultiReplica` (a live
        // editor is attached) the in-memory canonical replica is authoritative and
        // the disk `.yrs` is a write-through recovery projection only: reconcile the
        // just-loaded base state INTO the live replica so in-memory wins (a stale
        // disk projection can only add ops the live replica genuinely lost, never
        // regress live text). Under `GitAuthoritative` (headless) this returns
        // `None` and the existing baseline-wins load above runs unchanged.
        if let Err(e) =
            crate::crdt_relay_host::reconcile_disk_projection_for_file(file, &base_state)
        {
            eprintln!("[crdt] disk-demotion reconcile failed (non-fatal): {e}");
        }
        let (merged, state) = agent_doc_merge::merge_contents_crdt(
            Some(&base_state),
            &content_ours,
            &content_current,
        )?;
        (merged, Some(state))
    } else {
        eprintln!("File was modified during run. Merging changes...");
        (
            agent_doc_merge_io::merge_contents(baseline, &content_ours, &content_current)?,
            None,
        )
    };
    let final_content = normalize_direct_run_template_content(
        file,
        baseline,
        snapshot_doc.as_deref(),
        &final_content,
    )?;

    write::guard_visible_write_idle(file, "direct_run_template")?;
    agent_doc_snapshot_io::save(file, &final_content, agent_doc_ops_log_io::log_op)?;
    if let Some(state) = crdt_state {
        agent_doc_merge_io::save_document_crdt(file, &state, &final_content)?;
    }
    atomic_write(file, &final_content)?;
    drop(doc_lock);
    Ok(())
}

fn normalize_direct_run_prompt_prefixes(
    file: &Path,
    content: &str,
    diff_text: &str,
) -> Result<String> {
    let has_prompt_bearing_user_drift = diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .any(|change| {
            matches!(
                change.kind,
                diff::PromptBearingChangeKind::PromptTarget
                    | diff::PromptBearingChangeKind::ContentEdit
            )
        });
    if !has_prompt_bearing_user_drift {
        return Ok(content.to_string());
    }

    let Some(snapshot_doc) = agent_doc_snapshot_io::load(file).ok().flatten() else {
        return Ok(content.to_string());
    };
    let boundary_normalized = template::reposition_boundary_to_end_clean(content);
    let normalized = write::normalize_user_prompts_in_exchange_safe(
        &boundary_normalized,
        &boundary_normalized,
        &snapshot_doc,
        file,
    );
    if normalized != content {
        write::guard_visible_write_idle(file, "direct_run_prefix_normalize")?;
        atomic_write(file, &normalized)?;
        eprintln!("[run] normalized direct-run user prompt prefixes");
    }
    Ok(normalized)
}

fn normalize_direct_run_template_content(
    file: &Path,
    baseline: &str,
    snapshot: Option<&str>,
    content: &str,
) -> Result<String> {
    let normalized = if let Some(snapshot_doc) = snapshot {
        write::normalize_user_prompts_in_exchange_safe(content, baseline, snapshot_doc, file)
    } else {
        content.to_string()
    };
    write::normalize_template_structure_or_fail(&normalized, file)
}

fn update_resume_id(file: &Path, session_id: &str) -> Result<()> {
    let current = std::fs::read_to_string(file)?;
    let updated = frontmatter::set_resume_id(&current, session_id)?;
    write::guard_visible_write_idle(file, "direct_run_update_resume_id")?;
    atomic_write(file, &updated)?;
    agent_doc_snapshot_io::save(file, &updated, agent_doc_ops_log_io::log_op)?;
    Ok(())
}

/// Acquire an advisory flock on a document file for agent-doc-vs-agent-doc
/// coordination. Lock file is `.agent-doc/locks/<hash>.lock`. Released on drop.
fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = agent_doc_fs::state_lock_path_for(path)?;
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

/// Write content to a file atomically, routed through the `#pcpc5cut` 08b
/// document write-authority gate ladder.
///
/// Delegates to [`crate::write::atomic_write_pub`] — the single gated chokepoint
/// shared with the IPC/finalize `write.rs::atomic_write` path — rather than being
/// a parallel direct-disk writer. The underlying raw write still uses
/// write-to-temp + rename (atomic on POSIX when source and destination share a
/// filesystem, guaranteed here since the temp file is a sibling).
///
/// `#pcpc5d` (08b removal rung): previously this wrote the visible `.md` straight
/// to disk and only recorded provenance, so under `dual-write`/`authority`/
/// `removed` a direct-run write still bypassed the session actor's ordered write
/// queue — a *surviving direct-disk writer* the cutover must delete. After this
/// change every same-process document writer flows through one gate:
/// - `off` (default): bare atomic write + provenance — byte-identical to the
///   prior behavior, so no response cycle persisting through this path changes;
/// - `shadow`: raw write + would-route observation to `ops.log`;
/// - `dual-write`/`authority`/`removed`: serialized through the ordered write
///   queue, removing the in-process direct-run vs finalize interleave at the root.
///
/// Provenance (`#ipc-drift-writeprovenance`) is still recorded inside the shared
/// raw writer (`write.rs::atomic_write_raw`) on both the raw and queued paths.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    crate::write::atomic_write_pub(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_config::Config;
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
        crate::project_controller::append_state_event(root, &event).unwrap();
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
        crate::project_controller::append_state_event(dir.path(), &event).unwrap();

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
            queue_consumption: Some(crate::write::QueueConsumptionOutcome {
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
        crate::project_controller::append_state_event(dir.path(), &event).unwrap();

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
        crate::project_controller::append_state_event(dir.path(), &selected).unwrap();
        let deferred = agent_doc_state_backbone::StateEvent::new(
            "typed-deferred-stop-head",
            agent_doc_state_backbone::StateFact::QueueHeadDeferred {
                document_hash,
                node_key,
                reason: "stop_fence".to_string(),
                hosting_epoch: None,
            },
        );
        crate::project_controller::append_state_event(dir.path(), &deferred).unwrap();

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

        apply_template_response(&doc, baseline, response, false)
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

        apply_template_response(&doc, baseline, response, false)
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

        apply_template_response(&doc, baseline, response, false)
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
        let normalized = normalize_direct_run_prompt_prefixes(&doc, baseline, &diff_text)
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

        let err = run(
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

    /// #ipc-drift-writeprovenance: the direct-run document-write path records the
    /// same write-provenance as the IPC/finalize `write.rs::atomic_write`, so a
    /// foreign-looking disk change from a direct-run write is positively
    /// attributed to agent-doc instead of inferred from the mtime heuristic.
    #[test]
    fn direct_run_atomic_write_records_provenance() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let path = dir.path().join("prov-direct-run.md");
        atomic_write(&path, "direct run body").unwrap();
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
        atomic_write(&path, "routed direct-run body").unwrap();
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
