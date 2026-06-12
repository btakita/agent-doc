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

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{
    agent, component, config::Config, diff, frontmatter, git, merge, snapshot, template, write,
};

const AGENT_DOC_RUN_HEARTBEAT_SECS_ENV: &str = "AGENT_DOC_RUN_HEARTBEAT_SECS";
const DEFAULT_RUN_HEARTBEAT_SECS: u64 = 30;

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

pub fn run(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    config: &Config,
) -> Result<()> {
    run_with_context(
        file, branch, agent_name, model, dry_run, no_git, config, None,
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
    config: &Config,
    run_context: Option<&crate::graph::RunContext>,
) -> Result<()> {
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

/// Verdict for a direct run that found no document changes since the snapshot.
///
/// `#nochange-after-stall`: a plain "Nothing changed" verdict hides the fact
/// that the previous run ended abnormally — for example a recursive owner-pane
/// invocation that abandoned its cycle, leaving the operator with no durable
/// closeout. Classify the latest cycle state so the no-change path can surface
/// the prior terminal state and the recovery action instead of a snapshot-only
/// result. Healthy committed cycles stay `Clean`, so normal no-change behavior
/// is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NoChangeVerdict {
    Clean,
    Abnormal { summary: String, recovery: String },
}

fn classify_no_change_cycle_state(
    state: Option<&crate::cycle_state::CycleState>,
) -> NoChangeVerdict {
    use crate::cycle_state::CyclePhase;
    let Some(state) = state else {
        return NoChangeVerdict::Clean;
    };
    if state.phase == CyclePhase::Abandoned {
        if state
            .last_event
            .starts_with("recursive_direct_invocation_blocked")
        {
            return NoChangeVerdict::Abnormal {
                summary: format!(
                    "the previous run was blocked as a recursive direct invocation and its cycle ({}) was abandoned, so no normal dispatch/response completed",
                    state.cycle_id
                ),
                recovery:
                    "if the owning pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status <FILE>` (or `agent-doc session clear <FILE>`) — idle pane evidence repairs a stale busy actor back to ready. Otherwise dispatch from the document's managed pane (editor Run Agent Doc) or restart the owner with `agent-doc start <FILE>` instead of a nested direct `agent-doc <FILE>`"
                        .to_string(),
            };
        }
        return NoChangeVerdict::Abnormal {
            summary: format!(
                "the previous cycle ({}) was abandoned (last_event={}) and never reached a committed response",
                state.cycle_id, state.last_event
            ),
            recovery:
                "re-run `agent-doc <FILE>` to start a fresh cycle, or inspect `.agent-doc/logs/` for that cycle id"
                    .to_string(),
        };
    }
    // #jb-codex-nochange-after-repair: a committed no-response bookkeeping-only
    // cycle (repair/reap following an abandoned or failed run) means the prior
    // run did not produce an assistant response. When the document matches the
    // snapshot, the operator sees plain "Nothing changed" which hides the fact
    // that the last turn made no durable progress. Surface the prior state so
    // the operator can take recovery action. Broader active-queue-head checks
    // live in session-check (#nochange-after-stall-breadth).
    if state.phase == CyclePhase::Committed
        && state.capture_id.is_none()
        && state.response_sha256.is_none()
    {
        let bookkeeping = state.had_pending_mutations
            || !state.pending_done_ids.is_empty()
            || !state.pending_kept_open_ids.is_empty()
            || !state.reaped_pending_ids.is_empty()
            || !state.pending_gated_ids.is_empty()
            || state.pending_added_this_cycle;
        if bookkeeping {
            return NoChangeVerdict::Abnormal {
                summary: format!(
                    "the latest cycle ({}) committed without an assistant response body (bookkeeping-only closeout: last_event={}); the prior run was likely abandoned or repaired without producing a response",
                    state.cycle_id, state.last_event
                ),
                recovery: format!(
                    "re-run `agent-doc {}` from a non-owner pane or use `agent-doc start {}` to provision a fresh pane, then inspect `.agent-doc/logs/` for cycle {} history",
                    state.file, state.file, state.cycle_id
                ),
            };
        }
    }
    NoChangeVerdict::Clean
}

#[allow(clippy::too_many_arguments)]
fn run_once(
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
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
        match classify_no_change_cycle_state(crate::cycle_state::load(file)?.as_ref()) {
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
    frontmatter::require_agent_doc_document(&raw_content, file)?;
    let (mut content_original, session_id) =
        frontmatter::ensure_session_for_file(&raw_content, file)?;
    if content_original != raw_content {
        std::fs::write(file, &content_original)?;
    }
    content_original = normalize_direct_run_prompt_prefixes(file, &content_original, &the_diff)?;
    let owned_rc;
    let rc: &crate::graph::RunContext = if let Some(provided) = run_context {
        provided.set_file_path(file.to_path_buf());
        provided
    } else {
        owned_rc = crate::graph::RunContext::new(file.to_path_buf());
        &owned_rc
    };
    let (fm, _body) = frontmatter::parse_for_file_with_context(&content_original, file, rc)?;
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
    let harness = agent_doc_core::model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = model
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc_core::model_tier::canonical_model_name(m, &harness, &config.model));
    let prompt_cache_routing_affinity =
        prompt_cache_routing_affinity(run_mode, agent_name, resolved_model.as_deref());

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

    let session_accretion = crate::session_accretion::inspect(file).ok();
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
        if let Some(blocks) = crate::prompt_cache::PromptCacheBlocks::from_rendered(&prompt) {
            let replay_key = blocks.replay_key(&prompt_cache_routing_affinity);
            let adapter_state = if prompt_fm.resume.is_some() {
                "resumed"
            } else {
                "fresh"
            };
            let current_cost = crate::prompt_cache::PromptCacheSessionCostSample::from_replay_key(
                &replay_key,
                adapter_state,
            );
            eprintln!(
                "--- Prompt cache stable_prefix_sha256={} provider_cache_key={} cache_control={} routing_affinity={} ---",
                replay_key.stable_prefix_sha256,
                replay_key.provider_cache_key,
                replay_key.cache_control,
                replay_key.routing_affinity
            );
            eprintln!(
                "--- Prompt cache session_cost {} ---",
                crate::prompt_cache::render_cache_miss_ranking(None, &current_cost)
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
        let diagnostic = owned_pane_prompt_miss_diagnostic(file, &detail, &unresolved);
        crate::ops_log::log_op(
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
        // #recguard-wedge-escape: a busy owner pane re-invoking `agent-doc <FILE>`
        // mid-turn trips this guard (Option B `#codex-self-reinvoke-prevent` only
        // redirects the *Stop-hook* continuation, not a mid-turn re-run). One
        // transient self-invoke is normal, but the SAME head tripping this guard
        // `WEDGE_THRESHOLD` times in a row is a self-driving `agent:queue auto`
        // dead-loop with no operator watching — it would re-fire forever. Break
        // it: halt the runaway auto-queue (`queue: stop`) so the loop stops
        // burning cycles, and hand the operator one clear recovery action.
        let wedge_count = crate::recguard_wedge::record(file, &continuation.head_prompt)?;
        if crate::recguard_wedge::is_wedged(wedge_count) {
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
            crate::recguard_wedge::clear(file)?;
            crate::ops_log::log_op(
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
                owned_pane_queue_wedge_halt_diagnostic(file, &detail, &continuation, wedge_count)
            );
        }
        let diagnostic = owned_pane_queue_handoff_diagnostic(file, &detail, &continuation);
        crate::ops_log::log_op(
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
        git::create_branch(file)?;
    }

    // Pre-commit: commit user's changes before sending to agent
    // This lets the editor show agent additions as diff gutters
    if !no_git {
        let did_commit = git::commit(file)?;
        if !did_commit && !queue_synthetic_diff && diff::compute(file)?.is_none() {
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
            Some(crate::agent::run_agent_timeout()),
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
        Err(err) if is_timeout_error(&err) => {
            let diagnostic = run_dispatch_timeout_diagnostic(file, agent_name);
            record_run_preflight_timeout(file, "direct_invocation_timeout", &diagnostic)?;
            anyhow::bail!("{}\n\nsource: {}", diagnostic, err);
        }
        Err(err) => return Err(err),
    };

    let response_text = match run_mode {
        RunMode::Append => write::strip_assistant_heading(&response.text),
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
            queue_consumption = write::consume_queue_prompt_with_outcome(file)?;
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
    if let Some(d) = diff::compute(file)? {
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
    if let Some(command) = crate::queue_command::slash_command_text(&prompt) {
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
    Empty,
}

fn active_queue_prompt_state(file: &Path) -> Result<ActiveQueuePromptState> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let (fm, _) = frontmatter::parse_for_file_with_context(&content, file, &rc)?;
    if fm.queue_active != Some(true) {
        return Ok(ActiveQueuePromptState::Inactive);
    }

    let components = component::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(ActiveQueuePromptState::Inactive);
    };
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries =
        crate::queue::parse(body).context("run queue resume: failed to parse document queue")?;
    let has_auto = crate::queue::has_auto_attr(&queue_component.attrs);
    let activation = crate::queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    if crate::queue::has_stop_fence_at_head(&activation.entries_after) {
        eprintln!("[run] active queue halted by stop fence at head");
        return Ok(ActiveQueuePromptState::StopFence {
            next_prompt: crate::queue::first_prompt(&activation.entries_after)
                .map(|prompt| prompt.text.clone()),
        });
    }
    if let Some(start_at) = crate::queue::time_gate_at_head(&activation.entries_after) {
        eprintln!("[run] active queue deferred by time gate at head: {start_at}");
        return Ok(ActiveQueuePromptState::TimeGate {
            start_at: start_at.to_string(),
            next_prompt: crate::queue::first_prompt(&activation.entries_after)
                .map(|prompt| prompt.text.clone()),
        });
    }

    if let Some(snapshot_content) = snapshot::load(file)?
        && let Ok(snapshot_components) = component::parse(&snapshot_content)
        && let Some(snapshot_queue) = snapshot_components
            .iter()
            .find(|component| component.name == "queue")
    {
        let snapshot_body = &snapshot_content[snapshot_queue.open_end..snapshot_queue.close_start];
        if let Ok(snapshot_entries) = crate::queue::parse(snapshot_body) {
            let snapshot_has_auto = crate::queue::has_auto_attr(&snapshot_queue.attrs);
            let snapshot_activation =
                crate::queue::resolve_activation(&snapshot_entries, snapshot_has_auto, false, true);
            if crate::queue::detect_head_prompt_modified(
                &snapshot_activation.entries_after,
                &activation.entries_after,
            ) {
                let snapshot_head = crate::queue::first_prompt(&snapshot_activation.entries_after)
                    .map(|prompt| prompt.text.clone());
                let document_head = crate::queue::first_prompt(&activation.entries_after)
                    .map(|prompt| prompt.text.clone());
                eprintln!(
                    "[run] active queue halted because the head prompt changed since the snapshot"
                );
                return Ok(ActiveQueuePromptState::ItemModified {
                    snapshot_head,
                    document_head,
                });
            }
        }
    }

    let prompts = crate::queue::prompts(&activation.entries_after);
    let Some(prompt) = prompts.first() else {
        return Ok(ActiveQueuePromptState::Empty);
    };
    Ok(ActiveQueuePromptState::Ready {
        prompt: prompt.text.clone(),
    })
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
    // `auto` is a start trigger only. Continuation is driven by the active queue
    // state: consumption only runs when `queue_active: true`, so an active
    // persisted queue (no `auto`) continues on the same evidence as `auto`
    // (`#active-queue-persisted-no-continue`). The `active_queue_prompt_state`
    // re-check below still halts on stop fence / time gate / head-modified /
    // inactive / empty.
    if queue.drained || queue.remaining == 0 {
        return Ok(AutoQueueContinuation::Stop);
    }

    match active_queue_prompt_state(file)? {
        ActiveQueuePromptState::Ready { prompt } => {
            let force_fresh_agent_session =
                match crate::session_accretion::queue_context_reset_reason_if_opted_in(
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
                let state = crate::cycle_state::record_open_cycle_progress(&file, &event)
                    .ok()
                    .flatten();
                let (cycle_id, cycle_phase, last_event_age) = state
                    .as_ref()
                    .map(|state| {
                        (
                            state.cycle_id.as_str(),
                            cycle_phase_label(state.phase),
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
    let _ = crate::cycle_state::record_open_cycle_progress(file, &event);
    eprintln!("[run] progress phase={phase}{timeout_detail}");
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

fn is_timeout_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
            || cause.to_string().contains("timed out")
    })
}

fn record_run_preflight_timeout(file: &Path, event: &str, diagnostic: &str) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    crate::cycle_state::mark_recoverable_preflight_timeout(file, &event)?;
    crate::ops_log::log_op(
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
    let snapshot_content = snapshot::load(file)?;
    let file_content = std::fs::read_to_string(file).ok();
    crate::cycle_state::mark_abandoned(
        file,
        &event,
        snapshot_content.as_deref(),
        file_content.as_deref(),
    )?;
    crate::ops_log::log_op(
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
    if agent_name != "codex" || agent_doc_core::model_tier::detect_harness() != "codex" {
        return None;
    }
    let current_pane = crate::sessions::current_pane().ok()?;
    let registry_match = crate::sessions::lookup_entry(session_id)
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

/// Structured owner-pane self-invocation contract
/// (`#codex-owned-pane-prompt-miss-followups`, plan item 3 → preflight result).
///
/// Emitted by preflight when a Codex owner-pane re-invocation has unresolved
/// exchange work — an unanswered exchange prompt or a ready active auto-queue
/// head — that must be answered in THIS owner turn rather than dispatched to a
/// nested child. Codex guidance reads this to drive an in-pane response cycle
/// instead of only reading the run-time bail diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OwnedPaneSelfInvocation {
    pub file: String,
    pub current_pane: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_state: Option<String>,
    /// `"unresolved_prompt"` or `"active_queue_head"`.
    pub kind: String,
    /// First non-empty line of the unresolved work, truncated.
    pub work_excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_id: Option<String>,
    /// The exact persistence command to run after composing the in-pane response.
    pub persistence_command: String,
}

fn first_nonempty_excerpt(text: &str, max: usize) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .chars()
        .take(max)
        .collect()
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
    if owned_pane_self_invocation_detail(file, session_id, agent_name).is_none() {
        return Ok(None);
    }
    let current_pane = crate::sessions::current_pane().unwrap_or_default();
    let actor = actor_record_for_file(file).ok().flatten();
    let actor_generation = actor.as_ref().map(|record| record.generation);
    let actor_state = actor
        .as_ref()
        .map(|record| record.state.as_str().to_string());
    let persistence_command = format!(
        "agent-doc finalize {} (or agent-doc write --commit {})",
        file.display(),
        file.display()
    );
    if let Some(unresolved) = unresolved_prompt.filter(|p| !p.trim().is_empty()) {
        return Ok(Some(OwnedPaneSelfInvocation {
            file: file.display().to_string(),
            current_pane,
            session_id: session_id.to_string(),
            actor_generation,
            actor_state,
            kind: "unresolved_prompt".to_string(),
            work_excerpt: first_nonempty_excerpt(&unresolved, 200),
            head_id: None,
            persistence_command,
        }));
    }
    if let Some(continuation) = crate::queue_continuation::detect(file)? {
        return Ok(Some(OwnedPaneSelfInvocation {
            file: file.display().to_string(),
            current_pane,
            session_id: session_id.to_string(),
            actor_generation,
            actor_state,
            kind: "active_queue_head".to_string(),
            work_excerpt: first_nonempty_excerpt(&continuation.head_prompt, 200),
            head_id: continuation.head_id,
            persistence_command,
        }));
    }
    Ok(None)
}

fn recursive_codex_direct_invocation_diagnostic(
    file: &Path,
    session_id: &str,
    agent_name: &str,
) -> Option<String> {
    let detail = owned_pane_self_invocation_detail(file, session_id, agent_name)?;
    Some(format!(
        "recursive direct invocation would deadlock: `agent-doc {}` is running inside the Codex pane that already owns this document ({}). The empty preflight cycle has been abandoned (terminal — `session-check` accepts it, no manual `agent-doc cancel` needed). If the pane is now idle but the document still reports busy, reconcile it without killing the pane via `agent-doc session status {}` (or `agent-doc session clear {}`) — idle pane evidence repairs a stale busy actor back to ready; otherwise retry from outside the managed pane or restart the owner with `agent-doc start {}`.",
        file.display(),
        detail,
        file.display(),
        file.display(),
        file.display()
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
    Some(format_recursive_start_diagnostic(file, &detail))
}

/// Pure message builder for [`recursive_codex_start_invocation_diagnostic`], kept
/// separate so the operator-facing wording is unit-testable without live tmux,
/// registry, or harness-env state.
fn format_recursive_start_diagnostic(file: &Path, detail: &str) -> String {
    format!(
        "recursive self-owned-pane start would deadlock: `agent-doc start {}` was run inside the Codex pane that already owns this document ({}). Spawning a replacement owner here would loop re-injecting `agent-doc {}` into this same pane. Recover from a DIFFERENT pane: first reconcile a possibly stale-busy actor without killing the pane via `agent-doc session status {}`, then if the pane really is wedged run `agent-doc session interrupt-clear {}` to interrupt the owner and clear the session; if that cannot settle, run `agent-doc session interrupt-clear {} --force` to kill the owner pane/supervisor and clear the registry in one command. Do NOT re-run `agent-doc start {}` from this pane — it only re-trips this guard.",
        file.display(),
        detail,
        file.display(),
        file.display(),
        file.display(),
        file.display(),
        file.display()
    )
}

/// `#codex-owned-pane-prompt-miss`: structured fail-closed diagnostic for the
/// case where the owner pane re-invokes `agent-doc <FILE>` while an unresolved
/// exchange prompt is still pending. Names the prompt and the in-pane recovery
/// path, and explicitly tells the operator not to retry the same direct command
/// from the same pane (which would only re-trigger the guard).
fn owned_pane_prompt_miss_diagnostic(file: &Path, detail: &str, unresolved: &str) -> String {
    let excerpt: String = unresolved
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(unresolved)
        .trim()
        .chars()
        .take(200)
        .collect();
    format!(
        "owned-pane self-invocation with unresolved exchange prompt: `agent-doc {}` was run inside the Codex pane that already owns this document ({}), but a user prompt is still unanswered: \"{}\". The recursive same-pane guard refuses to dispatch a nested child here, so this request would be a no-op for that prompt. No pre-commit, snapshot, or queue mutation was made — the prompt stays executable. Recovery: answer the prompt in THIS owner pane's current turn, then persist with `agent-doc finalize {}` (or `agent-doc write --commit {}`). Do NOT re-run `agent-doc {}` from this same pane; that only re-triggers this guard.",
        file.display(),
        detail,
        excerpt,
        file.display(),
        file.display(),
        file.display()
    )
}

/// `#codex-owned-pane-auto-queue-stuck`: structured fail-closed diagnostic for
/// the owner pane re-invoking `agent-doc <FILE>` while a ready active auto-queue
/// head remains. Names the head (and id when known) plus the in-pane recovery
/// path, and tells the operator to run the head in THIS owner turn instead of
/// re-running the same direct command — which would only baseline queue/boundary
/// drift and re-trigger the recursive guard.
fn owned_pane_queue_handoff_diagnostic(
    file: &Path,
    detail: &str,
    continuation: &crate::queue_continuation::QueueContinuation,
) -> String {
    if let Some(command) = crate::queue_command::slash_command_text(&continuation.head_prompt) {
        return format!(
            "owned-pane self-invocation with active auto-queue slash command: `agent-doc {}` was run inside the Codex pane that already owns this document ({}), and the ready queue head is the literal slash command {:?}. The recursive same-pane guard refuses to answer slash commands as agent-doc work. No pre-commit, snapshot, exchange, or queue mutation was made — the command stays live. Recovery: let the current turn stop; the managed owner-pane supervisor will submit {:?} at the next idle prompt and consume the queue head. Do NOT answer this queue head in the exchange, and do NOT re-run `agent-doc {}` from this same pane.",
            file.display(),
            detail,
            command,
            command,
            file.display()
        );
    }
    let head_excerpt: String = continuation
        .head_prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(&continuation.head_prompt)
        .trim()
        .chars()
        .take(200)
        .collect();
    let id_note = continuation
        .head_id
        .as_deref()
        .map(|id| format!(" (id #{id})"))
        .unwrap_or_default();
    format!(
        "owned-pane self-invocation with active auto-queue head: `agent-doc {}` was run inside the Codex pane that already owns this document ({}), and a ready queue head is still live: \"{}\"{}. The recursive same-pane guard refuses to dispatch a nested child here, so this request would baseline queue/boundary drift and leave the head unprocessed. No pre-commit, snapshot, or queue mutation was made — the head stays live. Recovery: run the queue head in THIS owner pane's current turn, then persist with `agent-doc finalize {}` (or `agent-doc write --commit {}`) so the head is consumed and the next queue prompt is exposed. Do NOT re-run `agent-doc {}` from this same pane; that only re-triggers the recursive guard.",
        file.display(),
        detail,
        head_excerpt,
        id_note,
        file.display(),
        file.display(),
        file.display()
    )
}

/// `#recguard-wedge-escape`: escalated diagnostic when the SAME auto-queue head
/// has tripped the owner-pane self-invocation guard `WEDGE_THRESHOLD` times in a
/// row — a proven self-driving dead-loop. The runaway auto-queue has already been
/// halted (`queue: stop`) by the caller so it stops re-firing; this names the
/// wedge and the one recovery action that actually advances the head (it cannot
/// be dispatched from the owner pane that re-entered itself).
fn owned_pane_queue_wedge_halt_diagnostic(
    file: &Path,
    detail: &str,
    continuation: &crate::queue_continuation::QueueContinuation,
    count: u32,
) -> String {
    let head_excerpt: String = continuation
        .head_prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(&continuation.head_prompt)
        .trim()
        .chars()
        .take(200)
        .collect();
    let id_note = continuation
        .head_id
        .as_deref()
        .map(|id| format!(" (id #{id})"))
        .unwrap_or_default();
    format!(
        "owned-pane self-invocation WEDGE: `agent-doc {}` has re-entered the Codex pane that already owns this document ({}) {} times in a row for the same live queue head \"{}\"{} without it advancing — a self-driving `agent:queue auto` dead-loop. The auto-queue has been HALTED (`queue: stop`) so it stops re-firing. The head was NOT lost (it stays live) and no snapshot/queue drift was committed. Recovery: either (a) answer this head in the current owner turn and persist with `agent-doc finalize {}`, then re-enable with `queue: go`; or (b) re-establish a clean owner with `agent-doc start {}` and trigger the queue from OUTSIDE this pane. Do NOT re-run `agent-doc {}` from this same pane — that is exactly the re-entry that wedged the loop.",
        file.display(),
        detail,
        count,
        head_excerpt,
        id_note,
        file.display(),
        file.display(),
        file.display()
    )
}

fn run_dispatch_timeout_diagnostic(file: &Path, agent_name: &str) -> String {
    let state = crate::cycle_state::load(file).ok().flatten();
    let actor = actor_record_for_file(file).ok().flatten();
    let current_pane = crate::sessions::current_pane().ok();
    let (cycle_id, phase, last_event) = state
        .as_ref()
        .map(|state| {
            (
                state.cycle_id.as_str(),
                cycle_phase_label(state.phase),
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
        crate::agent::run_agent_timeout().as_secs(),
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

fn actor_record_for_file(file: &Path) -> Result<Option<crate::session_actor::ActorRecord>> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(project_root) = snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let file_arg = canonical.to_string_lossy();
    crate::session_actor::load_record_in(&project_root, &file_arg)
}

fn cycle_phase_label(phase: crate::cycle_state::CyclePhase) -> &'static str {
    match phase {
        crate::cycle_state::CyclePhase::PreflightStarted => "preflight_started",
        crate::cycle_state::CyclePhase::ResponseCaptured => "response_captured",
        crate::cycle_state::CyclePhase::WriteApplied => "write_applied",
        crate::cycle_state::CyclePhase::Committed => "committed",
        crate::cycle_state::CyclePhase::Abandoned => "abandoned",
    }
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
    session_accretion: Option<&crate::session_accretion::SessionAccretionReport>,
) -> String {
    let stable_prefix = build_prompt_stable_prefix(run_mode);
    let volatile_suffix =
        build_prompt_volatile_suffix(file, run_mode, fm, the_diff, content, session_accretion);
    crate::prompt_cache::PromptCacheBlocks::new(stable_prefix, volatile_suffix).render()
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
    session_accretion: Option<&crate::session_accretion::SessionAccretionReport>,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        crate::prompt_contract::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let document_section =
        crate::prompt_context::build_document_section(file, the_diff, content, session_accretion);
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

    write::guard_visible_write_idle(file, "direct_run_append")?;
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
    let rc = crate::graph::RunContext::new(file.to_path_buf());
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

    let content_ours =
        template::apply_patches_with_context(baseline, &patches, &unmatched, file, Some(&rc))
            .context("failed to apply template patches")?;
    let snapshot_doc = snapshot::load(file).ok().flatten();
    let content_ours = normalize_direct_run_template_content(
        file,
        baseline,
        snapshot_doc.as_deref(),
        &content_ours,
    )?;

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
        let base_state = snapshot::crdt_merge_base_state(file, baseline)?.state;
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
    let final_content = normalize_direct_run_template_content(
        file,
        baseline,
        snapshot_doc.as_deref(),
        &final_content,
    )?;

    write::guard_visible_write_idle(file, "direct_run_template")?;
    snapshot::save(file, &final_content)?;
    if let Some(state) = crdt_state {
        snapshot::save_document_crdt(file, &state, &final_content)?;
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

    let Some(snapshot_doc) = snapshot::load(file).ok().flatten() else {
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
mod tests;
