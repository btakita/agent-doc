//! Direct-run prompt, diff, and typed auto-queue IO graph.

use agent_doc_diff as diff;
use agent_doc_document::queue_projection::strip_in_progress_marker;
use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_cache::{
    PromptCacheBlocks, PromptCacheSessionCostSample, render_cache_miss_ranking,
};
use agent_doc_queue_io::queue_consume;
use agent_doc_session_accretion::SessionAccretionReport;
use agent_doc_template as template;
use agent_doc_template_io::{
    enforce_imperative_response_contract_for_diff, enforce_no_replace_pending,
    normalize_backlog_patch_response, normalize_user_prompts_in_exchange_safe,
};
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
use agent_doc_workflow::session_cycle::compact_command_hint;
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

const AGENT_DOC_RUN_HEARTBEAT_SECS_ENV: &str = "AGENT_DOC_RUN_HEARTBEAT_SECS";
const DEFAULT_RUN_HEARTBEAT_SECS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Append,
    Template,
}

impl RunMode {
    pub fn from_frontmatter(fm: &frontmatter::Frontmatter) -> Self {
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
pub struct RunCycleOutcome {
    pub dispatched: bool,
    pub queue_synthetic_diff: bool,
    pub queue_consumption: Option<queue_consume::QueueConsumptionOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoQueueContinuation {
    Stop,
    Continue { force_fresh_agent_session: bool },
}

pub trait DirectRunEffects {
    fn guard_no_exchange_compaction_request_for_diff(
        &self,
        file: &Path,
        diff_text: &str,
    ) -> Result<()>;

    fn commit(&self, file: &Path) -> Result<bool>;

    fn normalize_template_structure_or_fail(&self, content: &str, file: &Path) -> Result<String>;

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;

    fn consume_queue_prompts_for_done_ids_with_outcome(
        &self,
        file: &Path,
        done_ids: &[String],
        force_disk: bool,
    ) -> Result<Option<queue_consume::QueueConsumptionOutcome>>;

    fn complete_required_closeout(&self, file: &Path) -> Result<()>;

    fn abandon_recursive_cycle(&self, file: &Path, event: &str, diagnostic: &str) -> Result<()>;
}

pub fn guard_no_exchange_compaction_request_for_diff(file: &Path, diff_text: &str) -> Result<()> {
    if diff::detect_exchange_compaction_request(diff_text) {
        anyhow::bail!(
            "bare `compact exchange` directive detected in the current diff; close this turn \
             through the binary compaction path instead: `{}` \
             (optionally add `--message ...` for a custom checkpoint summary)",
            compact_command_hint(file)
        );
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveQueuePromptState {
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

/// Basic-repair a document's malformed frontmatter ON DISK before startup so a
/// recoverable formatting slip does not prevent the supervisor from opening.
pub fn repair_document_frontmatter_on_disk(file: &Path) -> Result<bool> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };
    if frontmatter::parse(&content).is_ok() {
        return Ok(false);
    }
    let Some((bad, good)) = frontmatter::raw_frontmatter_yaml(&content)
        .map(str::to_string)
        .and_then(|bad| frontmatter::repair_frontmatter_yaml(&bad).map(|good| (bad, good)))
    else {
        return Ok(false);
    };
    let repaired = content.replacen(&bad, &good, 1);
    std::fs::write(file, &repaired)
        .with_context(|| format!("failed to persist repaired frontmatter {}", file.display()))?;
    eprintln!(
        "[agent-doc] repaired malformed frontmatter in {} (tabs/stray fence) before startup",
        file.display()
    );
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    effects: &impl DirectRunEffects,
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &agent_doc_config::Config,
) -> Result<()> {
    run_with_context(
        effects, file, branch, agent_name, model, dry_run, no_git, force_disk, config, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_context(
    effects: &impl DirectRunEffects,
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &agent_doc_config::Config,
    run_context: Option<&agent_doc_run_context_io::RunContext>,
) -> Result<()> {
    let _stderr_redirect = if !dry_run && file.exists() {
        RunStderrRedirect::maybe_start(file)
    } else {
        RunStderrRedirect::inactive()
    };
    if !dry_run {
        let _ = repair_document_frontmatter_on_disk(file);
    }
    agent_doc_sync_io::sync::log_cross_document_execution_context(file, "run");

    let mut create_branch = branch;
    let mut completed_queue_items = 0usize;
    let mut force_fresh_agent_session = false;
    let mut last_context_clear_at = None;

    loop {
        let used_fresh_agent_session = force_fresh_agent_session;
        let outcome = run_once(
            effects,
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
pub fn run_once(
    effects: &impl DirectRunEffects,
    file: &Path,
    branch: bool,
    agent_name: Option<&str>,
    model: Option<&str>,
    dry_run: bool,
    no_git: bool,
    force_disk: bool,
    config: &agent_doc_config::Config,
    run_context: Option<&agent_doc_run_context_io::RunContext>,
    force_fresh_agent_session: bool,
) -> Result<RunCycleOutcome> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    eprintln!("[run] starting for {}", file.display());

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
    effects.guard_no_exchange_compaction_request_for_diff(file, &the_diff)?;

    let raw_content =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, "direct_run")?;
    agent_doc_frontmatter_io::session::require_agent_doc_document(&raw_content, file)?;
    let (mut content_original, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&raw_content, file)?;
    if content_original != raw_content {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, &content_original)?;
    }
    if !dry_run {
        let early_rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
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
            && let Some(continuation) = agent_doc_queue_io::queue_continuation::detect(file)?
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
    content_original =
        normalize_direct_run_prompt_prefixes(effects, file, &content_original, &the_diff)?;
    let queue_diff_completion_id =
        agent_doc_queue::queue_consume::queue_diff_completion_id_for_current_head(
            file,
            &content_original,
            &the_diff,
        )?;
    let owned_rc;
    let rc: &agent_doc_run_context_io::RunContext = if let Some(provided) = run_context {
        provided.set_file_path(file.to_path_buf());
        provided
    } else {
        owned_rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
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

    let backend = agent_doc_agent_io::agent::resolve_for_file(
        agent_name,
        agent_config,
        expanded_env,
        file,
        &fm,
    )?;

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

    if let Some(detail) = owned_pane_self_invocation_detail(file, &session_id, agent_name)
        && let Some(unresolved) = agent_doc_session_check_io::unresolved_exchange_prompt(file)?
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

    if let Some(detail) = owned_pane_self_invocation_detail(file, &session_id, agent_name)
        && let Some(continuation) = agent_doc_queue_io::queue_continuation::detect(file)?
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
        let wedge_count = agent_doc_owner_pane_io::record(file, &continuation.head_prompt)?;
        if owner_pane_wedge_threshold_reached(wedge_count) {
            if let Ok(content) =
                agent_doc_document_realtime_io::try_resolve_current_document_content(
                    file,
                    "owner_pane_wedge_stop_queue",
                )
                && let Ok(stopped) = frontmatter::merge_queue_state(&content, false)
                && let Err(err) =
                    agent_doc_document_realtime_io::atomic_write_through_authority(file, &stopped)
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

    if branch && !no_git {
        agent_doc_git_io::branch::create_session_branch(file)?;
    }

    if !no_git {
        let did_commit = effects.commit(file)?;
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
        effects.abandon_recursive_cycle(
            file,
            "recursive_direct_invocation_blocked",
            &diagnostic,
        )?;
        anyhow::bail!("{}", diagnostic);
    }

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
    enforce_imperative_response_contract_for_diff(file, &the_diff, &response_text)?;
    record_run_progress(file, "response_capture", agent_name, None);
    agent_doc_repair_io::pending::save_pending(file, &response_text)?;

    record_run_progress(file, "response_write", agent_name, None);
    match run_mode {
        RunMode::Append => apply_append_response(effects, file, &content_original, &response_text)?,
        RunMode::Template => apply_template_response(
            effects,
            file,
            &content_original,
            &response_text,
            fm.resolve_mode().is_crdt(),
        )?,
    }
    mark_run_write_applied(file, "run_write_applied")?;

    if let Some(ref sid) = response.session_id {
        update_resume_id(effects, file, sid)?;
        mark_run_write_applied(file, "run_write_applied_resume")?;
    }

    agent_doc_repair_io::pending::clear_pending(file)?;
    maybe_abort_after_write_applied_for_test()?;

    let mut queue_consumption = None;
    if !no_git {
        let _heartbeat = RunHeartbeat::start(file, "commit_closeout", agent_name, None);
        if queue_synthetic_diff
            || queue_consume::should_consume_queue_prompt_for_diff(file, Some(&the_diff))?
        {
            let queue_completion_ids = queue_diff_completion_id
                .clone()
                .into_iter()
                .collect::<Vec<_>>();
            queue_consumption = effects.consume_queue_prompts_for_done_ids_with_outcome(
                file,
                &queue_completion_ids,
                force_disk,
            )?;
        } else {
            eprintln!("{}", queue_consume::queue_skip_diagnostic_for_file(file)?);
        }
        effects.complete_required_closeout(file)?;
    }

    eprintln!("Response written to {}", file.display());
    Ok(RunCycleOutcome {
        dispatched: true,
        queue_synthetic_diff,
        queue_consumption,
    })
}

pub fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn maybe_abort_after_write_applied_for_test() -> Result<()> {
    if std::env::var_os("AGENT_DOC_TEST_ABORT_AFTER_RUN_WRITE_APPLIED").is_some() {
        anyhow::bail!("test abort after run write_applied");
    }
    Ok(())
}

pub fn apply_append_response(
    effects: &impl DirectRunEffects,
    file: &Path,
    baseline: &str,
    response: &str,
) -> Result<()> {
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

    let content_current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "direct_run_append_current",
    )?;
    let final_content = if content_current == baseline {
        content_ours
    } else {
        eprintln!("File was modified during run. Merging changes...");
        agent_doc_merge_io::merge_contents(baseline, &content_ours, &content_current)?
    };

    agent_doc_document_realtime_io::guard_visible_write_idle(file, "direct_run_append")?;
    agent_doc_snapshot_io::save(file, &final_content, agent_doc_ops_log_io::log_op)?;
    direct_run_atomic_write(effects, file, &final_content)?;
    drop(doc_lock);
    Ok(())
}

pub fn apply_template_response(
    effects: &impl DirectRunEffects,
    file: &Path,
    baseline: &str,
    response: &str,
    use_crdt: bool,
) -> Result<()> {
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let current_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "direct_run_template_current",
    )?;
    let (mut patches, unmatched) =
        template::parse_patches(response).context("failed to parse patch blocks from response")?;
    agent_doc_template::sanitize::sanitize_patches(&mut patches);
    let normalized =
        normalize_backlog_patch_response(file, &current_content, patches, unmatched, false)?;
    let patches = normalized.patches;
    let unmatched = normalized.unmatched;
    enforce_no_replace_pending(&patches, false)?;

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
        effects,
        file,
        baseline,
        snapshot_doc.as_deref(),
        &content_ours,
    )?;

    let content_current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "direct_run_template_merge_current",
    )?;
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
        if let Err(e) =
            agent_doc_crdt_relay_io::reconcile_disk_projection_for_file(file, &base_state)
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
        effects,
        file,
        baseline,
        snapshot_doc.as_deref(),
        &final_content,
    )?;

    agent_doc_document_realtime_io::guard_visible_write_idle(file, "direct_run_template")?;
    agent_doc_snapshot_io::save(file, &final_content, agent_doc_ops_log_io::log_op)?;
    if let Some(state) = crdt_state {
        agent_doc_merge_io::save_document_crdt(file, &state, &final_content)?;
    }
    direct_run_atomic_write(effects, file, &final_content)?;
    drop(doc_lock);
    Ok(())
}

pub fn normalize_direct_run_prompt_prefixes(
    effects: &impl DirectRunEffects,
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
    let normalized = normalize_user_prompts_in_exchange_safe(
        &boundary_normalized,
        &boundary_normalized,
        &snapshot_doc,
        file,
    );
    if normalized != content {
        agent_doc_document_realtime_io::guard_visible_write_idle(
            file,
            "direct_run_prefix_normalize",
        )?;
        direct_run_atomic_write(effects, file, &normalized)?;
        eprintln!("[run] normalized direct-run user prompt prefixes");
    }
    Ok(normalized)
}

pub fn normalize_direct_run_template_content(
    effects: &impl DirectRunEffects,
    file: &Path,
    baseline: &str,
    snapshot: Option<&str>,
    content: &str,
) -> Result<String> {
    let normalized = if let Some(snapshot_doc) = snapshot {
        normalize_user_prompts_in_exchange_safe(content, baseline, snapshot_doc, file)
    } else {
        content.to_string()
    };
    effects.normalize_template_structure_or_fail(&normalized, file)
}

pub fn update_resume_id(
    effects: &impl DirectRunEffects,
    file: &Path,
    session_id: &str,
) -> Result<()> {
    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "direct_run_update_resume_id",
    )?;
    let updated = frontmatter::set_resume_id(&current, session_id)?;
    agent_doc_document_realtime_io::guard_visible_write_idle(file, "direct_run_update_resume_id")?;
    direct_run_atomic_write(effects, file, &updated)?;
    agent_doc_snapshot_io::save(file, &updated, agent_doc_ops_log_io::log_op)?;
    Ok(())
}

pub fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
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

pub fn direct_run_atomic_write(
    effects: &impl DirectRunEffects,
    path: &Path,
    content: &str,
) -> Result<()> {
    effects.atomic_write(path, content)
}

#[cfg(unix)]
pub struct RunStderrRedirect {
    saved_stderr: Option<OwnedFd>,
}

#[cfg(unix)]
impl RunStderrRedirect {
    pub fn inactive() -> Self {
        Self { saved_stderr: None }
    }

    pub fn maybe_start(file: &Path) -> Self {
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
pub struct RunStderrRedirect;

#[cfg(not(unix))]
impl RunStderrRedirect {
    pub fn inactive() -> Self {
        Self
    }

    pub fn maybe_start(_file: &Path) -> Self {
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

pub fn run_stderr_redirect_harness(harness: &str) -> bool {
    matches!(harness, "claude" | "codex" | "opencode")
}

pub struct RunHeartbeat {
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl RunHeartbeat {
    pub fn start(
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

pub fn record_run_progress(file: &Path, phase: &str, agent_name: &str, timeout: Option<Duration>) {
    let timeout_detail = timeout
        .map(|timeout| format!(" timeout_s={}", timeout.as_secs()))
        .unwrap_or_default();
    let event = format!("run_progress phase={phase} agent={agent_name}{timeout_detail}");
    let _ = agent_doc_cycle_state_io::record_open_cycle_progress(file, &event);
    eprintln!("[run] progress phase={phase}{timeout_detail}");
}

pub fn mark_run_write_applied(file: &Path, event: &str) -> Result<()> {
    let file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "mark_run_write_applied",
    )?;
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    agent_doc_cycle_state_io::mark_write_applied(
        file,
        event,
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

pub fn start_run_cycle(file: &Path) -> Result<()> {
    agent_doc_cycle_state_io::admit_with_current_resolver(
        file,
        |file| {
            agent_doc_document_realtime_io::try_resolve_current_doc_from_file(file)
                .map(|resolved| resolved.content)
        },
        agent_doc_snapshot_io::load,
        agent_doc_ops_log_io::log_op,
    )?;
    Ok(())
}

pub fn record_run_preflight_timeout(file: &Path, event: &str, diagnostic: &str) -> Result<()> {
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

pub fn abandon_run_recursive_cycle(
    effects: &impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects,
    file: &Path,
    event: &str,
    diagnostic: &str,
) -> Result<()> {
    let compact = diagnostic.split_whitespace().collect::<Vec<_>>().join(" ");
    let event = format!("{event} {}", compact.chars().take(700).collect::<String>());
    let snapshot_content = agent_doc_snapshot_io::load(file)?;
    let file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "abandon_run_recursive_cycle",
    )
    .ok();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_abandoned(
        effects,
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

pub fn owned_pane_self_invocation_detail(
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

pub fn detect_owned_pane_self_invocation_with_options(
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
        && let Some(continuation) = agent_doc_queue_io::queue_continuation::detect(file)?
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

pub fn owner_pane_queue_edit_should_defer_until_closeout(
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

pub fn owner_pane_queue_edit_deferred_outcome(
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

pub fn recursive_codex_direct_invocation_diagnostic(
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

pub fn run_dispatch_timeout_diagnostic(file: &Path, agent_name: &str) -> String {
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

pub fn compute_run_diff(file: &Path) -> Result<Option<(String, bool)>> {
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

pub fn active_queue_prompt_diff(file: &Path) -> Result<Option<String>> {
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

pub fn active_queue_prompt_state(file: &Path) -> Result<ActiveQueuePromptState> {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "active_queue_prompt_state",
    )?;
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
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

pub fn typed_queue_prompt_state(file: &Path, content: &str) -> Option<ActiveQueuePromptState> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let ledger =
        agent_doc_controller_io::project_controller::load_state_event_ledger(&project_root).ok()?;
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

pub fn should_continue_auto_queue(
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

pub fn build_prompt(
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

pub fn prompt_cache_routing_affinity(
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
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
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
