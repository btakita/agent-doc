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
//!   on the newest question in a changed exchange tail. When session accretion
//!   has already reached warn/block severity and the diff still contains live
//!   prompt targets, resumed prompts replace the full exchange tail with a
//!   bounded response-context pack containing prompt targets, session summary,
//!   backlog head, and available component names.
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunCycleOutcome {
    dispatched: bool,
    queue_synthetic_diff: bool,
    queue_consumption: Option<write::QueueConsumptionOutcome>,
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

    loop {
        let outcome = run_once(
            file,
            create_branch,
            agent_name,
            model,
            dry_run,
            no_git,
            config,
            run_context,
        )?;
        create_branch = false;

        if !outcome.dispatched {
            return Ok(());
        }
        if let Some(queue_consumption) = outcome.queue_consumption.as_ref() {
            completed_queue_items += queue_consumption.consumed_count.max(1);
        }
        if !should_continue_auto_queue(file, &outcome, completed_queue_items, no_git)? {
            return Ok(());
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

    let session_accretion = crate::session_accretion::inspect(file).ok();
    let prompt = build_prompt(
        file,
        run_mode,
        &fm,
        &the_diff,
        &content_original,
        session_accretion.as_ref(),
    );

    if dry_run {
        eprintln!("--- Diff ---");
        print!("{}", the_diff);
        eprintln!("--- Prompt would be {} bytes ---", prompt.len());
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
    let fork = fm.resume.is_none();
    let harness = agent_doc_core::model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = model
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc_core::model_tier::canonical_model_name(m, &harness, &config.model));
    let response_result = {
        let _heartbeat = RunHeartbeat::start(
            file,
            "child_agent_wait",
            agent_name,
            Some(crate::agent::run_agent_timeout()),
        );
        backend.send(
            &prompt,
            fm.resume.as_deref(),
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
) -> Result<bool> {
    if no_git || !outcome.queue_synthetic_diff {
        return Ok(false);
    }
    let Some(queue) = outcome.queue_consumption.as_ref() else {
        return Ok(false);
    };
    // `auto` is a start trigger only. Continuation is driven by the active queue
    // state: consumption only runs when `queue_active: true`, so an active
    // persisted queue (no `auto`) continues on the same evidence as `auto`
    // (`#active-queue-persisted-no-continue`). The `active_queue_prompt_state`
    // re-check below still halts on stop fence / time gate / head-modified /
    // inactive / empty.
    if queue.drained || queue.remaining == 0 {
        return Ok(false);
    }

    match active_queue_prompt_state(file)? {
        ActiveQueuePromptState::Ready { prompt } => {
            eprintln!(
                "[queue] queue continuation: completed {} item(s); launching next prompt: {:?}",
                completed_queue_items, prompt
            );
            Ok(true)
        }
        ActiveQueuePromptState::StopFence { next_prompt } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): stop_fence before next prompt {:?}",
                completed_queue_items, next_prompt
            );
            Ok(false)
        }
        ActiveQueuePromptState::TimeGate {
            start_at,
            next_prompt,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): time_gate {} before next prompt {:?}",
                completed_queue_items, start_at, next_prompt
            );
            Ok(false)
        }
        ActiveQueuePromptState::ItemModified {
            snapshot_head,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): item_modified snapshot_head={:?} document_head={:?}",
                completed_queue_items, snapshot_head, document_head
            );
            Ok(false)
        }
        ActiveQueuePromptState::Inactive => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): queue_inactive",
                completed_queue_items
            );
            Ok(false)
        }
        ActiveQueuePromptState::Empty => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): no_remaining_prompt",
                completed_queue_items
            );
            Ok(false)
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
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
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
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user inserted prompt-bearing edits inline, classify them as prompt targets vs content edits before responding.",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
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
    let final_content = normalize_direct_run_template_content(
        file,
        baseline,
        snapshot_doc.as_deref(),
        &final_content,
    )?;

    write::guard_visible_write_idle(file, "direct_run_template")?;
    snapshot::save(file, &final_content)?;
    if let Some(state) = crdt_state {
        snapshot::save_crdt(file, &state)?;
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
    fn owned_pane_queue_handoff_diagnostic_names_head_and_recovery() {
        // #codex-owned-pane-auto-queue-stuck: the fail-closed handoff diagnostic
        // must name the live head + id, the in-owner-turn recovery path, and warn
        // against re-running the same direct command.
        let continuation = crate::queue_continuation::QueueContinuation {
            head_prompt: "do [#codex-owned-pane-auto-queue-stuck]".to_string(),
            head_id: Some("codex-owned-pane-auto-queue-stuck".to_string()),
            reason: "active `agent:queue auto` still has a ready head prompt".to_string(),
        };
        let msg = owned_pane_queue_handoff_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
            &continuation,
        );
        assert!(msg.contains("active auto-queue head"));
        assert!(msg.contains("do [#codex-owned-pane-auto-queue-stuck]"));
        assert!(msg.contains("(id #codex-owned-pane-auto-queue-stuck)"));
        assert!(msg.contains("THIS owner pane"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("Do NOT re-run"));
        assert!(msg.contains("No pre-commit, snapshot, or queue mutation was made"));
    }

    #[test]
    fn owned_pane_queue_wedge_halt_diagnostic_names_halt_and_both_recoveries() {
        // #recguard-wedge-escape-live-verify (deterministic core): when the
        // owner-pane self-invocation guard trips WEDGE_THRESHOLD times in a row,
        // the escalated diagnostic must (a) state the auto-queue was HALTED
        // (queue: stop), (b) state the head stays live / no drift committed,
        // (c) name BOTH recovery actions (answer+finalize+queue:go, or
        // agent-doc start from OUTSIDE the pane), and (d) warn against re-running
        // the same direct command. The end-to-end verification on a real wedged
        // Codex pane stays a recommended live-verify (#recguard-wedge-escape-live-verify).
        let continuation = crate::queue_continuation::QueueContinuation {
            head_prompt: "do [#recguard-wedge-escape]".to_string(),
            head_id: Some("recguard-wedge-escape".to_string()),
            reason: "active `agent:queue auto` still has a ready head prompt".to_string(),
        };
        let msg = owned_pane_queue_wedge_halt_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
            &continuation,
            crate::recguard_wedge::WEDGE_THRESHOLD,
        );
        assert!(msg.contains("WEDGE"));
        assert!(msg.contains("HALTED (`queue: stop`)"));
        assert!(msg.contains("do [#recguard-wedge-escape]"));
        assert!(msg.contains("(id #recguard-wedge-escape)"));
        assert!(msg.contains("stays live"));
        assert!(msg.contains("agent-doc finalize tasks/x.md"));
        assert!(msg.contains("queue: go"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("OUTSIDE this pane"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn recursive_start_diagnostic_refuses_and_names_out_of_pane_recovery() {
        // #recursion-guard-wedge-escape (part 1): `agent-doc start <FILE>` inside
        // the Codex pane that already owns the doc must fail closed with a message
        // that (a) names the deadlock as a recursive self-owned-pane start, (b)
        // explains it would loop re-injecting `agent-doc <FILE>`, (c) points at an
        // out-of-pane recovery (session status reconcile, then interrupt-clear,
        // escalating to interrupt-clear --force), and (d) warns against re-running
        // `agent-doc start` from this pane.
        let msg = format_recursive_start_diagnostic(
            Path::new("tasks/x.md"),
            "current_pane=%9 session_id=sess actor_generation=3 actor_state=alive-busy actor_pane=%9",
        );
        assert!(msg.contains("recursive self-owned-pane start would deadlock"));
        assert!(msg.contains("agent-doc start tasks/x.md"));
        assert!(msg.contains("loop re-injecting `agent-doc tasks/x.md`"));
        assert!(msg.contains("DIFFERENT pane"));
        assert!(msg.contains("agent-doc session status tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md"));
        assert!(msg.contains("agent-doc session interrupt-clear tasks/x.md --force"));
        assert!(msg.contains("Do NOT re-run"));
    }

    #[test]
    fn no_change_after_recursive_block_reports_typed_diagnostic() {
        // #nochange-after-stall: a direct run that finds no diff but whose latest
        // cycle was abandoned by the recursive-owner guard must surface the prior
        // state + recovery instead of plain "Nothing changed".
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-1","file":"x.md","phase":"abandoned","last_event":"recursive_direct_invocation_blocked recursive direct invocation would deadlock","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        match classify_no_change_cycle_state(Some(&st)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("recursive direct invocation"));
                assert!(summary.contains("cycle-1"));
                assert!(recovery.contains("managed pane"));
                // #stale-busy-recursion-recovery-discoverability: a stale busy
                // idle pane must be recoverable without a pane kill via the
                // existing idle-reconcile path, so the diagnostic must surface
                // `session status` / `session clear` ahead of the heavy restart.
                assert!(recovery.contains("agent-doc session status"));
                assert!(recovery.contains("agent-doc session clear"));
                assert!(recovery.contains("without killing the pane"));
            }
            NoChangeVerdict::Clean => panic!("expected an abnormal no-change verdict"),
        }
    }

    #[test]
    fn no_change_after_generic_abandoned_cycle_reports_typed_diagnostic() {
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-2","file":"x.md","phase":"abandoned","last_event":"stale_preflight","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert!(matches!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Abnormal { .. }
        ));
    }

    #[test]
    fn no_change_with_committed_cycle_stays_clean() {
        // Normal healthy completed session: no-change behavior must be unchanged.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-3","file":"x.md","phase":"committed","last_event":"commit","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
        );
        assert_eq!(classify_no_change_cycle_state(None), NoChangeVerdict::Clean);
    }

    #[test]
    fn no_change_after_committed_bookkeeping_only_cycle_reports_abnormal() {
        // #jb-codex-nochange-after-repair: when a committed cycle has no
        // response body but carried bookkeeping-only mutations (repair/reap
        // following an abandoned recursive invocation), the "Nothing changed"
        // output must surface the prior abnormal state instead of Clean.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-repair-1","file":"tasks/monsterrodholders.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0,"had_pending_mutations":true,"reaped_pending_ids":["stale-item"]}"#,
        )
        .unwrap();
        match classify_no_change_cycle_state(Some(&st)) {
            NoChangeVerdict::Abnormal { summary, recovery } => {
                assert!(summary.contains("cycle-repair-1"));
                assert!(summary.contains("bookkeeping-only"));
                assert!(summary.contains("commit_success"));
                assert!(recovery.contains("tasks/monsterrodholders.md"));
                assert!(recovery.contains("non-owner pane"));
                assert!(recovery.contains("agent-doc start"));
            }
            NoChangeVerdict::Clean => {
                panic!("expected Abnormal for committed no-response bookkeeping cycle")
            }
        }
    }

    #[test]
    fn no_change_committed_no_response_no_bookkeeping_stays_clean() {
        // A committed no-response cycle with no bookkeeping is not suspicious.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-4","file":"x.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
        );
    }

    #[test]
    fn no_change_committed_with_response_stays_clean() {
        // A committed cycle WITH a response body is healthy regardless of bookkeeping.
        let st: crate::cycle_state::CycleState = serde_json::from_str(
            r#"{"cycle_id":"cycle-5","file":"x.md","phase":"committed","last_event":"commit_success","started_at":0,"updated_at":0,"capture_id":"cap-1","response_sha256":"abc123","had_pending_mutations":true}"#,
        )
        .unwrap();
        assert_eq!(
            classify_no_change_cycle_state(Some(&st)),
            NoChangeVerdict::Clean
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
        let report = crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
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
        snapshot::save(&doc, snapshot).unwrap();

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
        snapshot::save(&doc, snapshot).unwrap();

        let diff_text = crate::diff::unified_diff_from_contents(snapshot, baseline)
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
