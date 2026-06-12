//! # Module: orchestrate
//!
//! ## Spec
//! - `run(file, config)`: resolves orchestration tasks from `--task`,
//!   `--from-file`, and/or `--from-exchange`, then dispatches by
//!   `OrchestrateMode`.
//! - `--mode sequential` injects each task into the document exchange as a
//!   fresh prompt, runs `preflight`, sends one fresh agent request with no
//!   resume/session reuse, streams step responses into `exchange` for CRDT docs
//!   when the backend supports streaming, then persists the response through
//!   `finalize` followed by `session-check`.
//! - `--mode parallel` reuses the existing `parallel` worktree fan-out path
//!   after task resolution, and the legacy `agent-doc parallel` command routes
//!   through this same orchestrate dispatch surface.
//! - `--mode dag` parses dependency annotations, validates the graph, then
//!   executes tasks in deterministic topological order against the shared
//!   document lifecycle.
//! - `extract_tasks_from_text(text)` prefers the last fenced code block or
//!   contiguous markdown list that contains task-like lines; falls back to
//!   non-empty trimmed lines when no list structure exists.
//! - `inject_prompt(file, task)` inserts `❯ <task>` before the exchange
//!   boundary marker when present, otherwise at the end of the exchange
//!   component. Atomic write only; snapshot/commit remain the subsequent
//!   lifecycle's responsibility.
//! - Fresh step prompts use the same bounded warn/block accretion context pack
//!   as normal edited-session prompts, so orchestration can avoid replaying the
//!   full exchange tail once compacted sessions start churning.
//!
//! ## Agentic Contracts
//! - Sequential orchestration never resumes prior agent sessions between
//!   tasks: each step calls the backend with `session_id=None` and `fork=false`.
//! - Sequential orchestration uses the same document diff/full-doc prompt shape
//!   as a normal edited session, so each fresh agent sees the current document
//!   state and only the latest injected prompt as the new diff.
//! - DAG orchestration keeps the same single-document write/commit guarantees as
//!   sequential mode, so dependency ordering is respected without concurrent
//!   writes to the shared session document.
//! - `finalize` / `session-check` are the persistence boundary for each step;
//!   if either fails, orchestration stops immediately.
//! - Task resolution preserves source order.
//!
//! ## Evals
//! - `extract_tasks_prefers_last_fenced_list`
//! - `extract_tasks_uses_last_markdown_list`
//! - `resolve_dag_tasks_supports_fan_in_dependencies`
//! - `dag_schedule_rejects_unknown_dependency`
//! - `dag_schedule_rejects_cycles`
//! - `inject_prompt_inserts_before_boundary`
//! - `send_fresh_response_uses_no_resume`
//! - `sequential_orchestration_injects_prompt_and_finalizes`
//! - `sequential_orchestration_uses_streaming_backend_for_crdt_docs`
//! - `dag_orchestration_runs_topological_order`

use anyhow::{Context, Result};
use clap::ValueEnum;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    component,
    frontmatter::{self, ResolvedMode},
    parallel,
    queue_dispatch::{self, QueueItemKind},
};
use agent_doc_orchestration::{
    agent,
    agent::streaming::StreamChunk,
    config::{AgentConfig, Config},
    diff,
    preflight::PreflightOutput,
    snapshot, write,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OrchestrateMode {
    Sequential,
    Parallel,
    Dag,
}

#[derive(Debug, Clone)]
pub struct OrchestrateConfig {
    pub mode: OrchestrateMode,
    pub tasks_explicit: Vec<String>,
    pub from_file: Option<PathBuf>,
    pub from_exchange: bool,
    pub from_queue: bool,
    pub resume_schedule: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub no_git: bool,
    pub no_worktree: bool,
    pub timeout_secs: u64,
    pub dry_run: bool,
    pub plan: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExecutionTask {
    label: String,
    prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DagTask {
    id: String,
    prompt: String,
    deps: Vec<String>,
}

#[derive(Debug, Default)]
struct DagMetadata {
    id: Option<String>,
    after: Vec<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ResolvedTaskBatch {
    tasks: Vec<String>,
    requested_presets: Vec<String>,
    exchange_source: Option<ExchangeTaskSourceFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeTaskSourceFingerprint {
    tasks: Vec<String>,
    requested_presets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeTaskSourceBlock {
    tasks: Vec<String>,
    requested_presets: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct OrderedTaskRunOptions<'a> {
    exchange_source: Option<&'a ExchangeTaskSourceFingerprint>,
    agent_override: Option<&'a str>,
    model_override: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
struct OrderedTaskStepOptions<'a> {
    agent_override: Option<&'a str>,
    model_override: Option<&'a str>,
    graph_context: Option<&'a str>,
    graph_evidence: Option<&'a crate::tsift_graph::TsiftGraphEvidencePlan>,
    task_label: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct ScheduledDagRunOptions<'a> {
    prompt_preset_block: Option<&'a str>,
    ordered: OrderedTaskRunOptions<'a>,
    graph_evidence: Option<&'a crate::tsift_graph::TsiftGraphEvidencePlan>,
}

trait LifecycleOps {
    fn preflight(&self, file: &Path) -> Result<PreflightOutput>;
    fn finalize(
        &self,
        file: &Path,
        baseline_file: Option<&str>,
        response: &str,
        mode: ResolvedMode,
    ) -> Result<()>;
    fn session_check(&self, file: &Path) -> Result<()>;
}

trait FreshAgentRunner {
    fn send_fresh(
        &self,
        file: &Path,
        prompt: &str,
        agent_name: &str,
        agent_config: Option<&AgentConfig>,
        env: Vec<(String, Option<String>)>,
        model: Option<&str>,
    ) -> Result<String>;

    fn send_fresh_streaming(
        &self,
        _file: &Path,
        _prompt: &str,
        _agent_name: &str,
        _agent_config: Option<&AgentConfig>,
        _env: Vec<(String, Option<String>)>,
        _model: Option<&str>,
    ) -> Result<Option<Box<dyn Iterator<Item = Result<StreamChunk>>>>> {
        Ok(None)
    }
}

trait ParallelRunner {
    fn run(&self, file: &Path, config: parallel::ParallelConfig) -> Result<()>;
}

struct CliLifecycleOps;

impl CliLifecycleOps {
    fn run_output_json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let exe = current_agent_doc_binary()?;
        let output = Command::new(&exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("failed to run `{}`", args.join(" ")))?;
        if !output.status.success() {
            anyhow::bail!("`{}` failed with {}", args.join(" "), output.status);
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("failed to parse JSON from `{}`", args.join(" ")))
    }

    fn run_status(&self, args: &[&str]) -> Result<()> {
        let exe = current_agent_doc_binary()?;
        let status = Command::new(&exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .with_context(|| format!("failed to run `{}`", args.join(" ")))?;
        if !status.success() {
            anyhow::bail!("`{}` failed with {}", args.join(" "), status);
        }
        Ok(())
    }
}

impl LifecycleOps for CliLifecycleOps {
    fn preflight(&self, file: &Path) -> Result<PreflightOutput> {
        let file_arg = file.to_string_lossy().into_owned();
        self.run_output_json(&["preflight", &file_arg])
    }

    fn finalize(
        &self,
        file: &Path,
        baseline_file: Option<&str>,
        response: &str,
        mode: ResolvedMode,
    ) -> Result<()> {
        let exe = current_agent_doc_binary()?;
        let file_arg = file.to_string_lossy().into_owned();

        let mut cmd = Command::new(&exe);
        cmd.arg("finalize")
            .arg(&file_arg)
            .arg("--origin")
            .arg("orchestrate");
        if let Some(path) = baseline_file {
            cmd.arg("--baseline-file").arg(path);
        }
        if mode.is_crdt() {
            cmd.arg("--stream");
        } else if mode.is_template() {
            cmd.arg("--template");
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let mut child = cmd
            .spawn()
            .with_context(|| internal_command_spawn_context("finalize", &exe))?;
        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .context("failed to open stdin for `agent-doc finalize`")?;
            stdin
                .write_all(response.as_bytes())
                .context("failed to stream response to `agent-doc finalize`")?;
        }
        let status = child
            .wait()
            .context("failed to wait for `agent-doc finalize`")?;
        if !status.success() {
            anyhow::bail!("`agent-doc finalize` failed with {}", status);
        }
        Ok(())
    }

    fn session_check(&self, file: &Path) -> Result<()> {
        let file_arg = file.to_string_lossy().into_owned();
        self.run_status(&["session-check", &file_arg])
    }
}

fn current_agent_doc_binary() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    resolve_agent_doc_binary_from_env(
        std::env::current_exe().ok(),
        std::env::args_os().next(),
        std::env::var_os("PATH"),
        &cwd,
    )
}

fn resolve_agent_doc_binary_from_env(
    current_exe: Option<PathBuf>,
    argv0: Option<OsString>,
    path_env: Option<OsString>,
    cwd: &Path,
) -> Result<PathBuf> {
    let stale_current_exe = match current_exe {
        Some(path) if launchable_file(&path) => return Ok(path),
        other => other,
    };

    let mut path_search_names = Vec::new();
    if let Some(raw_argv0) = argv0.as_deref() {
        let argv0_path = Path::new(raw_argv0);
        if has_path_separator(argv0_path) {
            let candidate = if argv0_path.is_absolute() {
                argv0_path.to_path_buf()
            } else {
                cwd.join(argv0_path)
            };
            if launchable_file(&candidate) {
                return Ok(candidate);
            }
        } else if !raw_argv0.is_empty() {
            path_search_names.push(raw_argv0.to_os_string());
        }
    }
    if !path_search_names
        .iter()
        .any(|name| name == OsStr::new("agent-doc"))
    {
        path_search_names.push(OsString::from("agent-doc"));
    }

    if let Some(path_env) = path_env {
        for dir in std::env::split_paths(&path_env) {
            for name in &path_search_names {
                let candidate = dir.join(name);
                if launchable_file(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }

    let stale = stale_current_exe
        .as_ref()
        .map(|path| format!("; skipped missing current_exe {}", path.display()))
        .unwrap_or_default();
    anyhow::bail!("failed to locate launchable agent-doc binary{stale}");
}

fn launchable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn has_path_separator(path: &Path) -> bool {
    path.is_absolute() || path.components().count() > 1
}

fn internal_command_spawn_context(command: &str, exe: &Path) -> String {
    let cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|err| format!("<unavailable: {err}>"));
    let path_present = std::env::var_os("PATH").is_some();
    format!(
        "failed to spawn `agent-doc {command}` (binary={}, cwd={}, PATH_present={})",
        exe.display(),
        cwd,
        path_present
    )
}

struct CliAgentRunner;

impl FreshAgentRunner for CliAgentRunner {
    fn send_fresh(
        &self,
        file: &Path,
        prompt: &str,
        agent_name: &str,
        agent_config: Option<&AgentConfig>,
        env: Vec<(String, Option<String>)>,
        model: Option<&str>,
    ) -> Result<String> {
        let content = std::fs::read_to_string(file)?;
        let (fm, _) = crate::frontmatter::parse(&content)?;
        let backend = agent::resolve_for_file(agent_name, agent_config, env, file, &fm)?;
        let response = send_fresh_response(backend.as_ref(), prompt, model)?;
        Ok(response.text)
    }

    fn send_fresh_streaming(
        &self,
        file: &Path,
        prompt: &str,
        agent_name: &str,
        agent_config: Option<&AgentConfig>,
        env: Vec<(String, Option<String>)>,
        model: Option<&str>,
    ) -> Result<Option<Box<dyn Iterator<Item = Result<StreamChunk>>>>> {
        let content = std::fs::read_to_string(file)?;
        let (fm, _) = crate::frontmatter::parse(&content)?;
        let Some(backend) =
            agent::resolve_streaming_for_file(agent_name, agent_config, env, file, &fm)?
        else {
            return Ok(None);
        };
        Ok(Some(backend.send_streaming(prompt, None, false, model)?))
    }
}

struct CliParallelRunner;

impl ParallelRunner for CliParallelRunner {
    fn run(&self, file: &Path, config: parallel::ParallelConfig) -> Result<()> {
        parallel::run(file, config)
    }
}

fn send_fresh_response(
    backend: &dyn agent::Agent,
    prompt: &str,
    model: Option<&str>,
) -> Result<agent::AgentResponse> {
    backend.send(prompt, None, false, model)
}

pub fn run(file: &Path, config: OrchestrateConfig, global_config: &Config) -> Result<()> {
    let lifecycle = CliLifecycleOps;
    let agent_runner = CliAgentRunner;
    let parallel_runner = CliParallelRunner;
    run_with_dependencies(
        file,
        config,
        global_config,
        &lifecycle,
        &agent_runner,
        &parallel_runner,
        false,
    )
}

pub fn run_parallel_compat(
    file: &Path,
    config: parallel::ParallelConfig,
    global_config: &Config,
) -> Result<()> {
    let lifecycle = CliLifecycleOps;
    let agent_runner = CliAgentRunner;
    let parallel_runner = CliParallelRunner;
    run_with_dependencies(
        file,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: config
                .tasks
                .into_iter()
                .map(|task| task.description)
                .collect(),
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: config.model,
            no_git: config.no_git,
            no_worktree: config.no_worktree,
            timeout_secs: config.timeout_secs,
            dry_run: config.dry_run,
            plan: false,
        },
        global_config,
        &lifecycle,
        &agent_runner,
        &parallel_runner,
        true,
    )
}

fn run_with_dependencies(
    file: &Path,
    config: OrchestrateConfig,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
    parallel_runner: &impl ParallelRunner,
    allow_empty_parallel_tasks: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    match config.mode {
        OrchestrateMode::Sequential => {
            let batch = resolve_task_batch(file, &config)?;
            if batch.tasks.is_empty() {
                anyhow::bail!("no orchestration tasks found");
            }
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", batch.tasks.len());
            for (idx, task) in batch.tasks.iter().enumerate() {
                eprintln!("[orchestrate]   {}: {}", idx + 1, task);
            }
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            if config.no_git {
                anyhow::bail!(
                    "`agent-doc orchestrate --mode sequential` requires git-backed finalize"
                );
            }
            let graph_evidence = collect_graph_evidence_for_tasks(file, &batch.tasks, false)?;
            let execution_tasks = batch
                .tasks
                .into_iter()
                .map(|task| ExecutionTask {
                    label: task.clone(),
                    prompt: apply_prompt_preset_block(&task, prompt_preset_block.as_deref()),
                })
                .collect::<Vec<_>>();
            if config.plan {
                print_plan(&execution_tasks);
                print_graph_plan(graph_evidence.as_ref(), &execution_tasks)?;
                return Ok(());
            }
            // #misfire-recovery-snapshot: before a queue-sourced auto-run mutates
            // the document/backlog, drop a lightweight pre-auto-run recovery tag
            // at HEAD (mirroring compact's pre-compact tag) so a misfiring
            // auto-run is recoverable without git/sidecar archaeology. Best-effort
            // and non-fatal — a tag failure must not block the run.
            if config.from_queue
                && let Err(e) = agent_doc_orchestration::compact::create_pre_mutation_tag(
                    file,
                    "pre-auto-run",
                    None,
                )
            {
                eprintln!(
                    "[orchestrate] Warning: could not create pre-auto-run recovery tag: {}",
                    e
                );
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                OrderedTaskRunOptions {
                    exchange_source: batch.exchange_source.as_ref(),
                    agent_override: config.agent.as_deref(),
                    model_override: config.model.as_deref(),
                },
                global_config,
                lifecycle,
                agent_runner,
                graph_evidence.as_ref(),
            )
        }
        OrchestrateMode::Parallel => {
            let batch = resolve_task_batch(file, &config)?;
            if batch.tasks.is_empty() && !allow_empty_parallel_tasks {
                anyhow::bail!("no orchestration tasks found");
            }
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", batch.tasks.len());
            for (idx, task) in batch.tasks.iter().enumerate() {
                eprintln!("[orchestrate]   {}: {}", idx + 1, task);
            }
            let graph_evidence = if config.dry_run {
                None
            } else {
                collect_graph_evidence_for_tasks(file, &batch.tasks, true)?
            };
            let parallel_tasks = batch
                .tasks
                .into_iter()
                .map(|task| parallel::ParallelTask {
                    description: task.clone(),
                    prompt: apply_parallel_graph_context(
                        &task,
                        apply_prompt_preset_block(&task, prompt_preset_block.as_deref()),
                        graph_evidence.as_ref(),
                    ),
                })
                .collect::<Vec<_>>();
            if config.plan {
                let exec: Vec<ExecutionTask> = parallel_tasks
                    .iter()
                    .map(|t| ExecutionTask {
                        label: t.description.clone(),
                        prompt: t.prompt.clone(),
                    })
                    .collect();
                print_plan(&exec);
                print_graph_plan(graph_evidence.as_ref(), &exec)?;
                return Ok(());
            }
            parallel_runner.run(
                file,
                parallel::ParallelConfig {
                    tasks: parallel_tasks,
                    model: config.model,
                    no_git: config.no_git,
                    no_worktree: config.no_worktree,
                    timeout_secs: config.timeout_secs,
                    dry_run: config.dry_run,
                },
            )
        }
        OrchestrateMode::Dag => {
            let batch = resolve_task_batch(file, &config)?;
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            if config.from_queue || config.resume_schedule.is_some() {
                return run_auto_dag_mode(
                    file,
                    &config,
                    batch,
                    prompt_preset_block.as_deref(),
                    global_config,
                    lifecycle,
                    agent_runner,
                );
            }
            let dag_tasks = resolve_dag_tasks(&batch)?;
            if dag_tasks.is_empty() {
                anyhow::bail!("no orchestration tasks found");
            }
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", dag_tasks.len());
            for (idx, task) in dag_tasks.iter().enumerate() {
                if task.deps.is_empty() {
                    eprintln!("[orchestrate]   {}: [{}] {}", idx + 1, task.id, task.prompt);
                } else {
                    eprintln!(
                        "[orchestrate]   {}: [{}] {} (after: {})",
                        idx + 1,
                        task.id,
                        task.prompt,
                        task.deps.join(", ")
                    );
                }
            }
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            if config.no_git {
                anyhow::bail!("`agent-doc orchestrate --mode dag` requires git-backed finalize");
            }
            let graph_targets = dag_tasks
                .iter()
                .map(|task| task.prompt.clone())
                .collect::<Vec<_>>();
            let graph_evidence = collect_graph_evidence_for_tasks(file, &graph_targets, false)?;
            let execution_tasks = plan_dag_execution(&dag_tasks)?
                .into_iter()
                .map(|task| ExecutionTask {
                    label: task.label.clone(),
                    prompt: apply_prompt_preset_block(&task.prompt, prompt_preset_block.as_deref()),
                })
                .collect::<Vec<_>>();
            if config.plan {
                print_plan(&execution_tasks);
                print_graph_plan(graph_evidence.as_ref(), &execution_tasks)?;
                return Ok(());
            }
            // #misfire-recovery-snapshot: before a queue-sourced auto-run mutates
            // the document/backlog, drop a lightweight pre-auto-run recovery tag
            // at HEAD (mirroring compact's pre-compact tag) so a misfiring
            // auto-run is recoverable without git/sidecar archaeology. Best-effort
            // and non-fatal — a tag failure must not block the run.
            if config.from_queue
                && let Err(e) = agent_doc_orchestration::compact::create_pre_mutation_tag(
                    file,
                    "pre-auto-run",
                    None,
                )
            {
                eprintln!(
                    "[orchestrate] Warning: could not create pre-auto-run recovery tag: {}",
                    e
                );
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                OrderedTaskRunOptions {
                    exchange_source: None,
                    agent_override: config.agent.as_deref(),
                    model_override: config.model.as_deref(),
                },
                global_config,
                lifecycle,
                agent_runner,
                graph_evidence.as_ref(),
            )
        }
    }
}

fn run_ordered_tasks_internal(
    file: &Path,
    tasks: &[ExecutionTask],
    options: OrderedTaskRunOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
) -> Result<()> {
    let mut effective_model: Option<String> = options.model_override.map(String::from);
    let dispatch_ctx = build_dispatch_context(file);
    agent_doc_orchestration::flow::orchestration_batch::log_queue_freeze_event(
        file,
        tasks.len(),
        options.exchange_source.is_some(),
    );

    for (idx, task) in tasks.iter().enumerate() {
        eprintln!(
            "[orchestrate] step {}/{}: {}",
            idx + 1,
            tasks.len(),
            task.label
        );

        let item = queue_dispatch::classify(&task.label);
        match item.kind {
            QueueItemKind::Command => {
                let result = queue_dispatch::dispatch_command(&item, &dispatch_ctx)?;
                if let queue_dispatch::DispatchResult::ModelOverride(tier) = result {
                    eprintln!("[orchestrate] model override updated to: {}", tier);
                    effective_model = Some(tier);
                }
            }
            QueueItemKind::Prompt => {
                let graph_context = graph_evidence
                    .and_then(|evidence| evidence.prompt_context_for_task(&task.label).transpose())
                    .transpose()?;
                let child_result = match run_ordered_task_step(
                    file,
                    &task.prompt,
                    OrderedTaskStepOptions {
                        agent_override: options.agent_override,
                        model_override: effective_model.as_deref(),
                        graph_context: graph_context.as_deref(),
                        graph_evidence,
                        task_label: &task.label,
                    },
                    global_config,
                    lifecycle,
                    agent_runner,
                ) {
                    Ok(()) => {
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: task.label.clone(),
                            outcome: agent_doc_orchestration::flow::types::FlowOutcome::Completed,
                            proof: Some(
                                graph_evidence
                                    .and_then(|evidence| {
                                        evidence.closeout_audit_proof_for_task(&task.label)
                                    })
                                    .unwrap_or_else(|| "finalize_session_check".to_string()),
                            ),
                        }
                    }
                    Err(err) => {
                        let child =
                            agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                                label: task.label.clone(),
                                outcome:
                                    agent_doc_orchestration::flow::types::FlowOutcome::FailedClosed,
                                proof: Some("child_step_error".to_string()),
                            };
                        agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(file, &child);
                        return Err(err);
                    }
                };
                agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                    file,
                    &child_result,
                );
                if idx + 1 < tasks.len()
                    && !agent_doc_orchestration::flow::orchestration_batch::batch_should_continue(
                        options
                            .exchange_source
                            .map(|source| exchange_task_source_changed(file, source))
                            .transpose()?
                            .unwrap_or(false),
                        &child_result,
                    )
                {
                    finalize_orchestration_batch_changed(file, idx + 1, tasks.len(), lifecycle)?;
                }
            }
        }
    }
    Ok(())
}

fn run_auto_dag_mode(
    file: &Path,
    config: &OrchestrateConfig,
    batch: ResolvedTaskBatch,
    prompt_preset_block: Option<&str>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    if config.no_git {
        anyhow::bail!(
            "`agent-doc orchestrate --mode dag --from-queue` requires git-backed finalize"
        );
    }

    let graph_evidence = if config.resume_schedule.is_some() {
        None
    } else {
        collect_graph_evidence_for_tasks(file, &batch.tasks, false)?
    };
    let schedule = if let Some(schedule_id) = config.resume_schedule.as_deref() {
        crate::auto_dag::load_schedule(file, schedule_id)?
    } else {
        let guard = crate::auto_dag::session_review_guard_for_file(file)?;
        crate::auto_dag::build_schedule(
            file,
            &batch.tasks,
            graph_evidence.as_ref(),
            guard,
            "queue",
        )?
    };
    let schedule_path = crate::auto_dag::write_schedule(file, &schedule)?;
    eprintln!(
        "[orchestrate] auto-DAG schedule: {} ({})",
        schedule.schedule_id,
        schedule_path.display()
    );
    eprintln!("[orchestrate] auto-DAG batches: {}", schedule.batches.len());
    for (idx, batch) in schedule.batches.iter().enumerate() {
        eprintln!("[orchestrate]   batch {}: {}", idx + 1, batch.join(", "));
    }

    let jobs_index = crate::jobs::create_for_schedule(
        file,
        &schedule,
        crate::jobs::CreateOptions {
            operation_doc: true,
            audit: true,
            budget: 6000,
        },
    )?;
    eprintln!(
        "[orchestrate] auto-DAG job packets: {} job(s) cycle={}",
        jobs_index.jobs.len(),
        jobs_index.cycle_id
    );

    let schedule_blocker = crate::auto_dag::guard_blocker(&schedule);
    let schedule_decision = if schedule_blocker.is_some() {
        agent_doc_orchestration::flow::orchestration_batch::AutoDagScheduleDecision::SessionReviewBlocked
    } else {
        agent_doc_orchestration::flow::orchestration_batch::AutoDagScheduleDecision::Ready
    };
    agent_doc_orchestration::flow::orchestration_batch::log_auto_dag_schedule_event(
        file,
        schedule_decision,
        schedule.nodes.len(),
        schedule.batches.len(),
    );
    if let Some(blocker) = schedule_blocker {
        anyhow::bail!(blocker);
    }

    let execution_tasks = execution_tasks_from_schedule(&schedule, prompt_preset_block)?;
    if execution_tasks.is_empty() {
        eprintln!("[orchestrate] auto-DAG schedule already complete");
        return Ok(());
    }
    if config.plan || config.dry_run {
        print_plan(&execution_tasks);
        return Ok(());
    }

    run_scheduled_dag_tasks_internal(
        file,
        &schedule,
        ScheduledDagRunOptions {
            prompt_preset_block,
            ordered: OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: config.agent.as_deref(),
                model_override: config.model.as_deref(),
            },
            graph_evidence: graph_evidence.as_ref(),
        },
        global_config,
        lifecycle,
        agent_runner,
    )
}

fn execution_tasks_from_schedule(
    schedule: &crate::auto_dag::AutoDagSchedule,
    prompt_preset_block: Option<&str>,
) -> Result<Vec<ExecutionTask>> {
    let mut tasks = Vec::new();
    for node in &schedule.nodes {
        match node.state {
            crate::auto_dag::AutoDagNodeState::Complete => continue,
            crate::auto_dag::AutoDagNodeState::Blocked
            | crate::auto_dag::AutoDagNodeState::Failed => {
                anyhow::bail!(
                    "auto-DAG schedule {} has gated/failed node {}; refusing to launch dependents",
                    schedule.schedule_id,
                    node.id
                );
            }
            crate::auto_dag::AutoDagNodeState::Pending
            | crate::auto_dag::AutoDagNodeState::Ready
            | crate::auto_dag::AutoDagNodeState::Running => {
                tasks.push(ExecutionTask {
                    label: node.label.clone(),
                    prompt: apply_prompt_preset_block(&node.prompt, prompt_preset_block),
                });
            }
        }
    }
    Ok(tasks)
}

fn run_scheduled_dag_tasks_internal(
    file: &Path,
    schedule: &crate::auto_dag::AutoDagSchedule,
    options: ScheduledDagRunOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    let mut effective_model: Option<String> = options.ordered.model_override.map(String::from);
    let dispatch_ctx = build_dispatch_context(file);
    agent_doc_orchestration::flow::orchestration_batch::log_queue_freeze_event(
        file,
        schedule.nodes.len(),
        true,
    );

    for batch in &schedule.batches {
        eprintln!("[orchestrate] auto-DAG batch: {}", batch.join(", "));
        for node_id in batch {
            let Some(node) = schedule.nodes.iter().find(|node| &node.id == node_id) else {
                anyhow::bail!("auto-DAG schedule references missing node `{node_id}`");
            };
            if node.state == crate::auto_dag::AutoDagNodeState::Complete {
                continue;
            }
            if matches!(
                node.state,
                crate::auto_dag::AutoDagNodeState::Blocked
                    | crate::auto_dag::AutoDagNodeState::Failed
            ) {
                anyhow::bail!(
                    "auto-DAG node {} is {:?}; refusing to launch dependents",
                    node.id,
                    node.state
                );
            }

            crate::auto_dag::update_node_state(
                file,
                &schedule.schedule_id,
                &node.id,
                crate::auto_dag::AutoDagNodeState::Running,
            )?;
            let prompt = apply_prompt_preset_block(&node.prompt, options.prompt_preset_block);
            let item = queue_dispatch::classify(&node.label);
            let graph_context = options
                .graph_evidence
                .and_then(|evidence| evidence.prompt_context_for_task(&node.label).transpose())
                .transpose()?;
            let step_result = match item.kind {
                QueueItemKind::Command => {
                    let result = queue_dispatch::dispatch_command(&item, &dispatch_ctx)?;
                    if let queue_dispatch::DispatchResult::ModelOverride(tier) = result {
                        eprintln!("[orchestrate] model override updated to: {}", tier);
                        effective_model = Some(tier);
                    }
                    Ok(())
                }
                QueueItemKind::Prompt => run_ordered_task_step(
                    file,
                    &prompt,
                    OrderedTaskStepOptions {
                        agent_override: options.ordered.agent_override,
                        model_override: effective_model.as_deref(),
                        graph_context: graph_context.as_deref(),
                        graph_evidence: options.graph_evidence,
                        task_label: &node.label,
                    },
                    global_config,
                    lifecycle,
                    agent_runner,
                ),
            };

            match step_result {
                Ok(()) => {
                    crate::auto_dag::update_node_state(
                        file,
                        &schedule.schedule_id,
                        &node.id,
                        crate::auto_dag::AutoDagNodeState::Complete,
                    )?;
                    let child =
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: node.label.clone(),
                            outcome: agent_doc_orchestration::flow::types::FlowOutcome::Completed,
                            proof: Some(
                                options
                                    .graph_evidence
                                    .and_then(|evidence| {
                                        evidence.closeout_audit_proof_for_task(&node.label)
                                    })
                                    .unwrap_or_else(|| "finalize_session_check".to_string()),
                            ),
                        };
                    agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                        file, &child,
                    );
                }
                Err(err) => {
                    crate::auto_dag::update_node_state(
                        file,
                        &schedule.schedule_id,
                        &node.id,
                        crate::auto_dag::AutoDagNodeState::Failed,
                    )?;
                    let child =
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: node.label.clone(),
                            outcome:
                                agent_doc_orchestration::flow::types::FlowOutcome::FailedClosed,
                            proof: Some("auto_dag_child_step_error".to_string()),
                        };
                    agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                        file, &child,
                    );
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

/// Build a dispatch context for command dispatch from a document file.
fn build_dispatch_context(file: &Path) -> queue_dispatch::DispatchContext {
    queue_dispatch::DispatchContext::from_file(file).unwrap_or_else(|_| {
        queue_dispatch::DispatchContext {
            file: file.to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
        }
    })
}

/// Resolve frontmatter/config harness args using the same precedence as `start.rs`:
/// `fm.agent_args > fm.<harness>_args > config.agent_args > config.<harness>_args`
fn resolve_orchestrate_agent_args(
    fm: &frontmatter::Frontmatter,
    agent_name: &str,
    global_config: &Config,
) -> Option<String> {
    match agent_name {
        "claude" => fm
            .agent_args
            .clone()
            .or_else(|| fm.claude_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.claude_args.clone()),
        "codex" => fm
            .agent_args
            .clone()
            .or_else(|| fm.codex_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.codex_args.clone()),
        "opencode" => fm
            .agent_args
            .clone()
            .or_else(|| fm.opencode_args.clone())
            .or_else(|| global_config.agent_args.clone())
            .or_else(|| global_config.opencode_args.clone()),
        _ => fm
            .agent_args
            .clone()
            .or_else(|| global_config.agent_args.clone()),
    }
}

/// Build an effective `AgentConfig` that merges structural base args with
/// resolved frontmatter/config harness args. Returns `None` when no
/// frontmatter override exists and no global agent config is set (callers
/// fall through to `default_base_args`).
fn build_effective_agent_config(
    agent_name: &str,
    resolved_args: Option<&str>,
    global_config: &Config,
) -> Option<AgentConfig> {
    let global_agent_config = global_config.agents.get(agent_name);
    if let Some(args_str) = resolved_args {
        let mut args = match agent_name {
            "claude" => agent::claude::structural_base_args(),
            "codex" => agent::codex::structural_base_args(),
            _ => Vec::new(),
        };
        args.extend(args_str.split_whitespace().map(String::from));
        Some(AgentConfig {
            command: global_agent_config
                .map_or_else(|| agent_name.to_string(), |c| c.command.clone()),
            args,
            result_path: global_agent_config.and_then(|c| c.result_path.clone()),
            session_path: global_agent_config.and_then(|c| c.session_path.clone()),
        })
    } else {
        None
    }
}

fn run_ordered_task_step(
    file: &Path,
    task: &str,
    options: OrderedTaskStepOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    close_open_preflight_handoff_cycle(file)?;
    inject_prompt(file, task)?;
    let preflight = lifecycle.preflight(file)?;
    if preflight.no_changes {
        anyhow::bail!("orchestration step did not produce a prompt-bearing diff after injection");
    }

    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let mode = fm.resolve_mode();
    let agent_name = options
        .agent_override
        .or(fm.agent.as_deref())
        .or(global_config.default_agent.as_deref())
        .unwrap_or("claude");
    let harness = agent_doc::model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = options
        .model_override
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc::model_tier::canonical_model_name(m, &harness, &global_config.model));
    let model = resolved_model.as_deref();
    let session_accretion = agent_doc_orchestration::session_accretion::inspect(file).ok();
    let mut prompt = build_agent_prompt(
        file,
        mode,
        preflight.diff.as_deref(),
        &doc,
        session_accretion.as_ref(),
    );
    if let Some(image_block) = build_image_description_block(file, &doc, agent_name) {
        prompt.push_str("\n\n");
        prompt.push_str(&image_block);
    }
    if let Some(graph_context) = options.graph_context {
        prompt.push_str("\n\n");
        prompt.push_str(graph_context);
    }
    let expanded_env = expand_frontmatter_env(&fm);

    let resolved_harness_args = resolve_orchestrate_agent_args(&fm, agent_name, global_config);
    let effective_config =
        build_effective_agent_config(agent_name, resolved_harness_args.as_deref(), global_config);
    let agent_config = effective_config
        .as_ref()
        .or_else(|| global_config.agents.get(agent_name));
    let mut launch_env = expanded_env;
    if agent_name == "codex" {
        let codex_network_access =
            agent_doc_orchestration::agent::resolve_codex_network_access(&fm, global_config);
        agent_doc_orchestration::agent::apply_codex_network_access_env_overrides(
            &mut launch_env,
            codex_network_access,
        );
        let sandbox_args = agent_config
            .map(|cfg| cfg.args.clone())
            .unwrap_or_else(agent::codex::default_base_args);
        let status = agent_doc_orchestration::agent::codex_network_status_from_overrides(
            &sandbox_args,
            codex_network_access,
            &launch_env,
        );
        eprintln!("[orchestrate] codex network access: {}", status.summary());
        if let Some(err) = status.mismatch_error() {
            anyhow::bail!(err);
        }
    }

    let (response, finalize_response) = if mode.is_crdt() {
        if let Some(seed) = exchange_stream_seed(&doc)? {
            if let Some(chunks) = agent_runner.send_fresh_streaming(
                file,
                &prompt,
                agent_name,
                agent_config,
                launch_env.clone(),
                model,
            )? {
                let streamed = stream_step_response(file, &seed, chunks)?;
                (streamed.full_response, streamed.finalize_response)
            } else {
                let response = agent_runner.send_fresh(
                    file,
                    &prompt,
                    agent_name,
                    agent_config,
                    launch_env,
                    model,
                )?;
                let finalize = response.clone();
                (response, finalize)
            }
        } else {
            let response = agent_runner.send_fresh(
                file,
                &prompt,
                agent_name,
                agent_config,
                launch_env,
                model,
            )?;
            let finalize = response.clone();
            (response, finalize)
        }
    } else {
        let response =
            agent_runner.send_fresh(file, &prompt, agent_name, agent_config, launch_env, model)?;
        let finalize = response.clone();
        (response, finalize)
    };
    let response_text = if mode.is_template() {
        response
    } else {
        write::strip_assistant_heading(&response)
    };
    let finalize_text = if mode.is_template() {
        let normalization =
            agent_doc_orchestration::flow::orchestration_batch::normalize_child_template_response(
                finalize_response,
            );
        agent_doc_orchestration::flow::orchestration_batch::log_child_patchback_normalization_event(
            file,
            &normalization,
        );
        normalization.response
    } else {
        write::strip_assistant_heading(&finalize_response)
    };

    if let Some(diff_text) = preflight.diff.as_deref() {
        write::enforce_imperative_response_contract_for_diff(file, diff_text, &response_text)?;
    }

    let finalize_text = if let Some(worker_result_line) =
        options.graph_evidence.and_then(|evidence| {
            evidence.worker_result_line_for_task(options.task_label, &response_text)
        }) {
        append_worker_result_line(&finalize_text, &worker_result_line, mode)
    } else {
        finalize_text
    };

    lifecycle.finalize(
        file,
        preflight.baseline_file.as_deref(),
        &finalize_text,
        mode,
    )?;
    lifecycle.session_check(file)?;
    Ok(())
}

fn close_open_preflight_handoff_cycle(file: &Path) -> Result<()> {
    let Some(state) = agent_doc_orchestration::cycle_state::load(file)? else {
        return Ok(());
    };
    if state.phase != agent_doc_orchestration::cycle_state::CyclePhase::PreflightStarted {
        return Ok(());
    }
    if agent_doc_orchestration::capture::load_by_id(file, &state.cycle_id)?.is_some() {
        return Ok(());
    }

    eprintln!(
        "[orchestrate] closing preflight handoff cycle {} before task injection",
        state.cycle_id
    );
    let file_content = fs::read_to_string(file)
        .with_context(|| format!("failed to read {} before orchestrating", file.display()))?;
    let snapshot_content = snapshot::load(file)?;
    snapshot::save(file, &file_content)?;
    agent_doc_orchestration::cycle_state::mark_abandoned(
        file,
        "orchestrate_preflight_handoff_closed",
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExchangeStreamSeed {
    prefix: String,
    suffix: String,
}

#[derive(Debug, Clone)]
struct StreamStepResult {
    full_response: String,
    finalize_response: String,
}

fn exchange_stream_seed(doc: &str) -> Result<Option<ExchangeStreamSeed>> {
    let components = component::parse(doc).context("failed to parse document components")?;
    let Some(exchange) = components.iter().find(|comp| comp.name == "exchange") else {
        return Ok(None);
    };
    let content = exchange.content(doc);
    let boundary_prefix = "<!-- agent:boundary:";
    let relative_boundary = content
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .filter_map(|(start, line)| line.trim().starts_with(boundary_prefix).then_some(start))
        .last();
    if let Some(boundary_start) = relative_boundary {
        return Ok(Some(ExchangeStreamSeed {
            prefix: content[..boundary_start].to_string(),
            suffix: content[boundary_start..].to_string(),
        }));
    }

    let boundary = agent_doc::format_boundary_marker(&agent_doc::new_boundary_id());
    Ok(Some(ExchangeStreamSeed {
        prefix: content.to_string(),
        suffix: format!("{boundary}\n"),
    }))
}

fn render_streamed_exchange(seed: &ExchangeStreamSeed, response: &str) -> String {
    let trimmed = response.trim_end();
    if trimmed.is_empty() {
        return format!("{}{}", seed.prefix, seed.suffix);
    }

    let mut rendered =
        String::with_capacity(seed.prefix.len() + trimmed.len() + seed.suffix.len() + 2);
    rendered.push_str(&seed.prefix);
    if !rendered.is_empty() && !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered.push_str(trimmed);
    rendered.push('\n');
    rendered.push_str(&seed.suffix);
    rendered
}

fn stream_step_response(
    file: &Path,
    seed: &ExchangeStreamSeed,
    chunks: Box<dyn Iterator<Item = Result<StreamChunk>>>,
) -> Result<StreamStepResult> {
    let mut response = String::new();
    let mut last_streamed_response = None;
    let mut checkpoint_writer =
        agent_doc_orchestration::capture::PartialCheckpointWriter::new(file);

    for chunk_result in chunks {
        let chunk = chunk_result.context("stream chunk error")?;
        if !chunk.text.is_empty() {
            response = chunk.text;
            if !chunk.is_final {
                checkpoint_writer.maybe_checkpoint(&response)?;
            }
            if !chunk.is_final && should_stream_exchange_patch(&response) {
                let exchange = render_streamed_exchange(seed, &response);
                agent_doc_orchestration::stream::flush_to_document(
                    file, &exchange, "exchange", "",
                )?;
                last_streamed_response = Some(response.clone());
            }
        }
        if chunk.is_final {
            break;
        }
    }

    if response.trim().is_empty() {
        anyhow::bail!("empty response from streaming orchestrate step");
    }

    let finalize_response = last_streamed_response
        .as_deref()
        .and_then(|streamed| finalize_suffix_from_streamed_prefix(streamed, &response))
        .unwrap_or_else(|| response.clone());

    Ok(StreamStepResult {
        full_response: response,
        finalize_response,
    })
}

#[cfg(test)]
fn orchestrate_finalize_text_for_template(response: String) -> String {
    agent_doc_orchestration::flow::orchestration_batch::normalize_child_template_response(response)
        .response
}

fn should_stream_exchange_patch(response: &str) -> bool {
    response.contains("<!-- patch:exchange")
}

fn finalize_suffix_from_streamed_prefix(streamed: &str, full: &str) -> Option<String> {
    if let Some(delta) = finalize_suffix_from_open_patch_prefix(streamed, full) {
        return Some(delta);
    }

    if let (Ok((full_patches, full_unmatched)), Ok((streamed_patches, streamed_unmatched))) = (
        crate::template::parse_patches(full),
        crate::template::parse_patches(streamed),
    ) && full_unmatched.trim().is_empty()
        && streamed_unmatched.trim().is_empty()
        && full_patches.len() == streamed_patches.len()
    {
        let mut delta = String::new();
        for (full_patch, streamed_patch) in full_patches.iter().zip(streamed_patches.iter()) {
            if full_patch.name != streamed_patch.name
                || !full_patch.content.starts_with(&streamed_patch.content)
            {
                return None;
            }
            let suffix = &full_patch.content[streamed_patch.content.len()..];
            if suffix.is_empty() {
                continue;
            }
            delta.push_str(&format!(
                "<!-- patch:{} -->\n{}<!-- /patch:{} -->\n",
                full_patch.name, suffix, full_patch.name
            ));
        }

        if !delta.trim().is_empty() {
            return Some(delta);
        }
    }
    None
}

fn finalize_suffix_from_open_patch_prefix(streamed: &str, full: &str) -> Option<String> {
    if !full.starts_with(streamed) {
        return None;
    }

    let open_start = streamed.find("<!-- patch:")?;
    let open_end = streamed[open_start..].find("-->")? + open_start + 3;
    let open_marker = &streamed[open_start..open_end];
    let patch_name = open_marker
        .strip_prefix("<!-- patch:")?
        .strip_suffix(" -->")?
        .trim();
    if patch_name.is_empty() {
        return None;
    }

    let mut content_start = open_end;
    if streamed.as_bytes().get(content_start) == Some(&b'\n') {
        content_start += 1;
    }
    let close_marker = format!("<!-- /patch:{} -->", patch_name);
    let close_pos = full[content_start..].find(&close_marker)? + content_start;
    let full_content = &full[content_start..close_pos];
    let streamed_content = &streamed[content_start..];
    if !full_content.starts_with(streamed_content) {
        return None;
    }
    let suffix = &full_content[streamed_content.len()..];
    if suffix.is_empty() {
        return None;
    }

    Some(format!(
        "<!-- patch:{} -->\n{}<!-- /patch:{} -->\n",
        patch_name, suffix, patch_name
    ))
}

fn expand_frontmatter_env(fm: &frontmatter::Frontmatter) -> Vec<(String, Option<String>)> {
    if fm.env.is_empty() {
        return Vec::new();
    }
    match agent_doc_orchestration::env::expand_values(&fm.env) {
        Ok(values) => values,
        Err(err) => {
            eprintln!(
                "[orchestrate] env expansion failed: {} — continuing without env overrides",
                err
            );
            Vec::new()
        }
    }
}

fn build_image_description_block(file: &Path, doc: &str, agent_name: &str) -> Option<String> {
    let project_config = crate::project_config::load_project_for_doc(file);
    let vision = &project_config.vision;
    let agent_mode = vision.agent_mode(agent_name).unwrap_or("passthrough");
    if agent_mode != "describe" {
        return None;
    }
    let (provider, api_key, model) = match crate::describe_image::resolve_vision_config(
        vision.effective_provider(Some(agent_name)),
        vision.effective_model(Some(agent_name)),
        vision.effective_api_key(Some(agent_name)),
        None,
        None,
        None,
    ) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("[orchestrate] vision config resolution failed: {}", err);
            return None;
        }
    };
    let base_dir = file.parent().unwrap_or(Path::new("."));
    let descriptions = match crate::describe_image::describe_images_in_text(
        doc, &provider, &api_key, &model, base_dir,
    ) {
        Ok(descs) => descs,
        Err(err) => {
            eprintln!("[orchestrate] image description failed: {}", err);
            return None;
        }
    };
    if descriptions.is_empty() {
        return None;
    }
    let mut block = String::from(
        "<image-descriptions>\nThe following image references were found in the document and described using a vision model:\n\n",
    );
    for desc in &descriptions {
        block.push_str(&format!(
            "### Image: {}\n\n{}\n\n",
            desc.reference.path, desc.description
        ));
    }
    block.push_str("</image-descriptions>");
    Some(block)
}

fn build_agent_prompt(
    file: &Path,
    mode: ResolvedMode,
    diff_text: Option<&str>,
    doc: &str,
    session_accretion: Option<&agent_doc_orchestration::session_accretion::SessionAccretionReport>,
) -> String {
    let diff_text = diff_text.unwrap_or_default();
    let prompt_bearing = diff::format_prompt_bearing_changes(diff_text)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        agent_doc_orchestration::prompt_contract::format_active_format_requirements(doc)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let document_section = agent_doc_orchestration::prompt_context::build_document_section(
        file,
        diff_text,
        doc,
        session_accretion,
    );

    if mode.is_template() {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            diff_text, prompt_bearing, active_format_requirements, document_section
        )
    } else {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user inserted prompt-bearing edits inline, classify them as prompt targets vs content edits before responding.",
            diff_text, prompt_bearing, active_format_requirements, document_section
        )
    }
}

fn resolve_task_batch(file: &Path, config: &OrchestrateConfig) -> Result<ResolvedTaskBatch> {
    let mut batch = ResolvedTaskBatch::default();
    batch.tasks.extend(
        config
            .tasks_explicit
            .iter()
            .map(|task| normalize_task(task))
            .filter(|task| !task.is_empty()),
    );

    if let Some(path) = &config.from_file {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read task file {}", path.display()))?;
        extend_task_batch_from_text(&mut batch, &text);
    }

    if config.from_exchange {
        let doc = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let full_exchange = exchange_text(&doc)?;
        let scoped = scope_exchange_for_tasks(full_exchange, file);
        let mut exchange_batch = ResolvedTaskBatch::default();
        extend_task_batch_from_text(&mut exchange_batch, &scoped);
        if !exchange_batch.tasks.is_empty() {
            batch.exchange_source = Some(ExchangeTaskSourceFingerprint {
                tasks: exchange_batch.tasks.clone(),
                requested_presets: exchange_batch.requested_presets.clone(),
            });
        }
        merge_task_batch(&mut batch, exchange_batch);
    }

    if config.from_queue {
        let doc = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        merge_task_batch(&mut batch, queue_task_batch(&doc)?);
    }

    canonicalize_prompt_preset_requests(file, &mut batch.requested_presets)?;
    if let Some(source) = &mut batch.exchange_source {
        canonicalize_prompt_preset_requests(file, &mut source.requested_presets)?;
    }

    Ok(batch)
}

fn merge_task_batch(target: &mut ResolvedTaskBatch, source: ResolvedTaskBatch) {
    target.tasks.extend(source.tasks);
    for preset in source.requested_presets {
        if !target
            .requested_presets
            .iter()
            .any(|existing| existing == &preset)
        {
            target.requested_presets.push(preset);
        }
    }
}

fn canonicalize_prompt_preset_requests(
    file: &Path,
    requested_presets: &mut Vec<String>,
) -> Result<()> {
    if requested_presets.is_empty() {
        return Ok(());
    }
    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let mut canonical = Vec::new();
    for preset_name in requested_presets.drain(..) {
        let resolved = frontmatter::resolve_prompt_preset_key(&fm.prompt_presets, &preset_name)
            .unwrap_or(preset_name);
        if !canonical.iter().any(|existing| existing == &resolved) {
            canonical.push(resolved);
        }
    }
    *requested_presets = canonical;
    Ok(())
}

fn extend_task_batch_from_text(batch: &mut ResolvedTaskBatch, text: &str) {
    batch.tasks.extend(extract_tasks_from_text(text));
    for preset in diff::extract_prompt_preset_requests_from_text(text) {
        if !batch
            .requested_presets
            .iter()
            .any(|existing| existing == &preset)
        {
            batch.requested_presets.push(preset);
        }
    }
}

fn exchange_text(doc: &str) -> Result<&str> {
    let components = component::parse(doc).context("failed to parse document components")?;
    let exchange = components
        .iter()
        .find(|comp| comp.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:exchange` component"))?;
    Ok(exchange.content(doc))
}

fn queue_task_batch(doc: &str) -> Result<ResolvedTaskBatch> {
    let components = component::parse(doc).context("failed to parse document components")?;
    let queue = components
        .iter()
        .find(|comp| comp.name == "queue")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:queue` component"))?;
    let body = queue.content(doc);
    let entries = agent_doc_orchestration::queue::parse(body)?;
    let (fm, _) = frontmatter::parse(doc)?;
    let activation = agent_doc_orchestration::queue::resolve_activation(
        &entries,
        agent_doc_orchestration::queue::has_auto_attr(&queue.attrs),
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        anyhow::bail!(
            "agent:queue is not active; add `auto`, a start fence, or queue_active=true before --from-queue dispatch"
        );
    }
    let mut batch = ResolvedTaskBatch::default();
    for entry in activation.entries_after {
        match entry {
            agent_doc_orchestration::queue::QueueEntry::Prompt(prompt) => {
                batch.tasks.push(normalize_task(&prompt.text));
            }
            agent_doc_orchestration::queue::QueueEntry::Preset(preset)
            | agent_doc_orchestration::queue::QueueEntry::Dispatch(preset) => {
                if !batch
                    .requested_presets
                    .iter()
                    .any(|existing| existing == &preset)
                {
                    batch.requested_presets.push(preset);
                }
            }
            agent_doc_orchestration::queue::QueueEntry::Completed(_)
            | agent_doc_orchestration::queue::QueueEntry::StartFence(_)
            | agent_doc_orchestration::queue::QueueEntry::StopFence
            | agent_doc_orchestration::queue::QueueEntry::Freeform(_) => {}
        }
    }
    Ok(batch)
}

fn exchange_task_source_changed(
    file: &Path,
    original: &ExchangeTaskSourceFingerprint,
) -> Result<bool> {
    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let exchange = exchange_text(&doc)?;
    Ok(match find_exchange_task_source(exchange, original) {
        Some(current) => current != *original,
        None => true,
    })
}

fn find_exchange_task_source(
    exchange: &str,
    original: &ExchangeTaskSourceFingerprint,
) -> Option<ExchangeTaskSourceFingerprint> {
    if original.tasks.is_empty() {
        return None;
    }
    let mut candidates = collect_markdown_list_source_blocks(exchange)
        .into_iter()
        .filter(|block| contains_ordered_subsequence(&block.tasks, &original.tasks))
        .collect::<Vec<_>>();

    let candidate = candidates
        .iter()
        .rev()
        .find(|block| block.requested_presets == original.requested_presets)
        .cloned()
        .or_else(|| candidates.pop())?;

    Some(ExchangeTaskSourceFingerprint {
        tasks: candidate.tasks,
        requested_presets: candidate.requested_presets,
    })
}

fn collect_markdown_list_source_blocks(text: &str) -> Vec<ExchangeTaskSourceBlock> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut idx = 0usize;

    while idx < lines.len() {
        let Some(first_task) = parse_list_item(lines[idx]) else {
            idx += 1;
            continue;
        };

        let list_start = idx;
        let mut tasks = vec![first_task];
        idx += 1;
        while idx < lines.len() {
            let Some(task) = parse_list_item(lines[idx]) else {
                break;
            };
            tasks.push(task);
            idx += 1;
        }
        let list_end = idx;

        let mut context_start = list_start;
        while context_start > 0 {
            let previous = lines[context_start - 1].trim();
            if previous.is_empty()
                || previous.starts_with("### ")
                || previous.starts_with("## ")
                || previous.starts_with("<!-- agent:boundary:")
            {
                break;
            }
            context_start -= 1;
        }
        let source_text = lines[context_start..list_end].join("\n");
        blocks.push(ExchangeTaskSourceBlock {
            tasks,
            requested_presets: diff::extract_prompt_preset_requests_from_text(&source_text),
        });
    }

    blocks
}

fn contains_ordered_subsequence(haystack: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut cursor = 0usize;
    for item in haystack {
        if item == &needle[cursor] {
            cursor += 1;
            if cursor == needle.len() {
                return true;
            }
        }
    }
    false
}

/// Scope exchange text to the user's latest additions by comparing against
/// the snapshot. Prevents stale task lists in response content from being
/// picked by `extract_tasks_from_text` when the user's directive is a bare
/// line (not a list) at the exchange tail.
fn scope_exchange_for_tasks(exchange: &str, file: &Path) -> String {
    let snap_content = match snapshot::load(file) {
        Ok(Some(s)) => s,
        _ => return exchange.to_string(),
    };
    let snap_exchange = match exchange_text(&snap_content) {
        Ok(s) => s.to_string(),
        Err(_) => return exchange.to_string(),
    };

    let current_lines: Vec<&str> = exchange.lines().collect();
    let snap_lines: Vec<&str> = snap_exchange.lines().collect();

    // Compare lines, ignoring boundary artifacts like (HEAD) markers
    let normalize_for_compare = |line: &str| -> String {
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:boundary:") {
            return String::new();
        }
        line.replace(" (HEAD)", "")
    };

    let mut matching = 0;
    for (curr, snap) in current_lines.iter().zip(snap_lines.iter()) {
        if normalize_for_compare(curr) == normalize_for_compare(snap) {
            matching += 1;
        } else {
            break;
        }
    }

    if matching >= snap_lines.len() && current_lines.len() > snap_lines.len() {
        let tail: String = current_lines[snap_lines.len()..]
            .iter()
            .filter(|line| !line.trim().starts_with("<!--"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if !tail.trim().is_empty() {
            return tail;
        }
    }

    exchange.to_string()
}

fn resolve_dag_tasks(batch: &ResolvedTaskBatch) -> Result<Vec<DagTask>> {
    batch
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, line)| parse_dag_task_line(line, idx))
        .collect()
}

fn load_prompt_preset_block(file: &Path, requested_presets: &[String]) -> Result<Option<String>> {
    if requested_presets.is_empty() {
        return Ok(None);
    }

    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let mut block = String::new();

    for (idx, preset_name) in requested_presets.iter().enumerate() {
        let preset_key = frontmatter::resolve_prompt_preset_key(&fm.prompt_presets, preset_name)
            .ok_or_else(|| anyhow::anyhow!("unknown prompt preset `{}`", preset_name))?;
        let preset_body = fm
            .prompt_presets
            .get(&preset_key)
            .expect("resolved prompt preset key must exist");
        if idx > 0 {
            block.push('\n');
        }
        block.push_str(&format!("(preset {})\n", preset_key));
        block.push_str(preset_body.trim_end());
        block.push('\n');
    }

    Ok(Some(block))
}

fn apply_prompt_preset_block(task: &str, prompt_preset_block: Option<&str>) -> String {
    match prompt_preset_block {
        Some(block) if !block.trim().is_empty() => format!("{}\n{}", block.trim_end(), task),
        _ => task.to_string(),
    }
}

fn print_plan(tasks: &[ExecutionTask]) {
    eprintln!(
        "[orchestrate] plan — {} task(s) (no execution)",
        tasks.len()
    );
    for (idx, task) in tasks.iter().enumerate() {
        eprintln!(
            "[orchestrate] step {}/{}: {}",
            idx + 1,
            tasks.len(),
            task.label
        );
        eprintln!("[orchestrate] --- prompt ---");
        for line in task.prompt.lines() {
            eprintln!("[orchestrate] {}", line);
        }
        eprintln!("[orchestrate] --- end prompt ---");
    }
}

fn print_graph_plan(
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
    tasks: &[ExecutionTask],
) -> Result<()> {
    let Some(graph_evidence) = graph_evidence else {
        return Ok(());
    };
    eprintln!(
        "[orchestrate] tsift graph targets: {}",
        graph_evidence.targets.join(", ")
    );
    for task in tasks {
        if let Some(context) = graph_evidence.prompt_context_for_task(&task.label)? {
            eprintln!("[orchestrate] --- tsift graph evidence: {} ---", task.label);
            for line in context.lines() {
                eprintln!("[orchestrate] {}", line);
            }
            eprintln!("[orchestrate] --- end tsift graph evidence ---");
        }
    }
    Ok(())
}

fn collect_graph_evidence_for_tasks(
    file: &Path,
    tasks: &[String],
    fail_on_conflict_matrix: bool,
) -> Result<Option<crate::tsift_graph::TsiftGraphEvidencePlan>> {
    let graph_evidence = match crate::tsift_graph::collect_for_do_items(file, tasks) {
        Ok(graph_evidence) => graph_evidence,
        Err(err) => {
            eprintln!(
                "[orchestrate] warning: {}",
                crate::tsift_graph::graph_unavailable_warning(&err)
            );
            None
        }
    };
    if let Some(graph_evidence) = &graph_evidence {
        eprintln!(
            "[orchestrate] tsift graph evidence targets: {}",
            graph_evidence.targets.join(", ")
        );
        if fail_on_conflict_matrix
            && let Some(blocker) = graph_evidence.conflict_matrix.parallel_dispatch_blocker()
        {
            anyhow::bail!(
                "tsift conflict-matrix blocked parallel dispatch for {}: {}",
                graph_evidence.targets.join(", "),
                blocker
            );
        }
    }
    Ok(graph_evidence)
}

fn apply_parallel_graph_context(
    task_label: &str,
    prompt: String,
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
) -> String {
    let Some(graph_evidence) = graph_evidence else {
        return prompt;
    };
    match graph_evidence.prompt_context_for_task(task_label) {
        Ok(Some(context)) => format!("{prompt}\n\n{context}"),
        Ok(None) => prompt,
        Err(err) => {
            eprintln!(
                "[orchestrate] warning: failed to render tsift graph evidence for `{}`: {}",
                task_label, err
            );
            prompt
        }
    }
}

fn finalize_orchestration_batch_changed(
    file: &Path,
    completed_steps: usize,
    total_steps: usize,
    lifecycle: &impl LifecycleOps,
) -> Result<()> {
    agent_doc_orchestration::flow::orchestration_batch::log_source_changed_event(
        file,
        completed_steps,
        total_steps,
    );
    eprintln!(
        "[orchestrate] source task list changed after step {}/{}; stopping before next step",
        completed_steps, total_steps
    );
    let preflight = lifecycle.preflight(file)?;
    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let response = format!(
        "<!-- patch:exchange -->\n\
### Re: orchestration batch changed — gpt-5\n\n\
Stopped sequential orchestration after {completed_steps} of {total_steps} step(s) because the source task list changed while the batch was running. The remaining and newly added tasks are still open for the next explicit orchestration run.\n\
<!-- /patch:exchange -->\n"
    );
    lifecycle.finalize(
        file,
        preflight.baseline_file.as_deref(),
        &response,
        fm.resolve_mode(),
    )
    .with_context(|| {
        format!(
            "failed parent orchestration batch-change closeout after {completed_steps}/{total_steps} step(s)"
        )
    })?;
    lifecycle.session_check(file)?;
    anyhow::bail!(
        "orchestration batch changed during run after {}/{} step(s); stopped before launching the next step",
        completed_steps,
        total_steps
    );
}

fn append_worker_result_line(
    response: &str,
    worker_result_line: &str,
    mode: ResolvedMode,
) -> String {
    if response.contains(worker_result_line) {
        return response.to_string();
    }
    if mode.is_template() {
        const CLOSE: &str = "<!-- /patch:exchange -->";
        if let Some(idx) = response.rfind(CLOSE) {
            let mut out = String::with_capacity(response.len() + worker_result_line.len() + 2);
            out.push_str(response[..idx].trim_end());
            out.push('\n');
            out.push_str(worker_result_line);
            out.push('\n');
            out.push_str(&response[idx..]);
            return out;
        }
    }
    let mut out = response.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(worker_result_line);
    out.push('\n');
    out
}

fn extract_tasks_from_text(text: &str) -> Vec<String> {
    let code_blocks = collect_fenced_task_blocks(text);
    if let Some(block) = code_blocks.last()
        && !block.is_empty()
    {
        return block.clone();
    }

    let list_blocks = collect_markdown_list_blocks(text);
    if let Some(block) = list_blocks.last()
        && !block.is_empty()
    {
        return block.clone();
    }

    text.lines()
        .map(normalize_task)
        .filter(|line| !line.is_empty())
        .collect()
}

fn collect_fenced_task_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    let mut current = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                current.clear();
            }
            continue;
        }

        if fence_close(trimmed, fence_char, fence_len) {
            let tasks = collect_list_items(&current.join("\n"));
            if !tasks.is_empty() {
                blocks.push(tasks);
            }
            in_fence = false;
            current.clear();
            continue;
        }

        current.push(line.to_string());
    }

    blocks
}

fn collect_markdown_list_blocks(text: &str) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();

    for line in text.lines() {
        if let Some(task) = parse_list_item(line) {
            current.push(task);
        } else if !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

fn collect_list_items(text: &str) -> Vec<String> {
    text.lines().filter_map(parse_list_item).collect()
}

fn parse_list_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Strip the binary-owned prompt prefix that write-back adds to user prompts
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .or_else(|| trimmed.strip_prefix("❯"))
        .unwrap_or(trimmed);
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let task = normalize_task(rest);
        return (!task.is_empty()).then_some(task);
    }

    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }

    let rest = trimmed[digit_count..].trim_start();
    let rest = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix(".\t"))
        .or_else(|| rest.strip_prefix(")\t"))?;
    let task = normalize_task(rest);
    (!task.is_empty()).then_some(task)
}

fn fence_open(trimmed: &str) -> Option<(char, usize)> {
    let fence_char = trimmed.chars().next()?;
    if fence_char != '`' && fence_char != '~' {
        return None;
    }
    let fence_len = trimmed.chars().take_while(|ch| *ch == fence_char).count();
    (fence_len >= 3).then_some((fence_char, fence_len))
}

fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
    if !trimmed.starts_with(fence_char) {
        return false;
    }
    let close_len = trimmed.chars().take_while(|ch| *ch == fence_char).count();
    close_len >= fence_len && trimmed[close_len..].trim().is_empty()
}

fn normalize_task(task: &str) -> String {
    task.trim().trim_start_matches('❯').trim().to_string()
}

fn parse_dag_task_line(task: &str, index: usize) -> Result<DagTask> {
    let normalized = normalize_task(task);
    if normalized.is_empty() {
        anyhow::bail!("dag task {} is empty", index + 1);
    }

    let (metadata, prompt) = split_dag_metadata(&normalized)?;
    if prompt.is_empty() {
        anyhow::bail!("dag task {} is missing a prompt", index + 1);
    }

    let prompt_id = extract_prompt_task_id(&prompt);
    let id = metadata
        .id
        .or(prompt_id)
        .unwrap_or_else(|| format!("step-{}", index + 1));

    Ok(DagTask {
        id,
        prompt,
        deps: metadata.after,
    })
}

fn split_dag_metadata(task: &str) -> Result<(DagMetadata, String)> {
    let trimmed = task.trim();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return Ok((DagMetadata::default(), trimmed.to_string()));
    };

    let closing = rest
        .find(']')
        .ok_or_else(|| anyhow::anyhow!("dag task metadata is missing closing `]`"))?;
    let metadata_text = &rest[..closing];
    let prompt = rest[closing + 1..].trim().to_string();
    let metadata = parse_dag_metadata(metadata_text)?;
    Ok((metadata, prompt))
}

fn parse_dag_metadata(metadata: &str) -> Result<DagMetadata> {
    let mut parsed = DagMetadata::default();
    for token in metadata.split_whitespace() {
        if let Some(value) = token.strip_prefix("after=") {
            parsed.after = parse_dependency_list(value);
            continue;
        }
        if let Some(value) = token.strip_prefix("deps=") {
            parsed.after = parse_dependency_list(value);
            continue;
        }
        if let Some(value) = token.strip_prefix("id=") {
            let value = value.trim();
            if value.is_empty() {
                anyhow::bail!("dag task metadata has empty `id=`");
            }
            parsed.id = Some(value.to_string());
            continue;
        }
        if parsed.id.is_none() {
            parsed.id = Some(token.to_string());
            continue;
        }
        anyhow::bail!("unsupported dag task metadata token `{}`", token);
    }
    Ok(parsed)
}

fn parse_dependency_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|dep| !dep.is_empty())
        .map(str::to_string)
        .collect()
}

fn extract_prompt_task_id(prompt: &str) -> Option<String> {
    let bytes = prompt.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'#' {
            let start = idx;
            idx += 1;
            while idx < bytes.len() {
                let ch = bytes[idx] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    idx += 1;
                } else {
                    break;
                }
            }
            if idx > start + 1 {
                return Some(prompt[start..idx].to_string());
            }
            continue;
        }
        idx += 1;
    }
    None
}

fn plan_dag_execution(tasks: &[DagTask]) -> Result<Vec<ExecutionTask>> {
    let mut ids = HashSet::new();
    for task in tasks {
        if !ids.insert(task.id.clone()) {
            anyhow::bail!("duplicate dag task id `{}`", task.id);
        }
    }

    for task in tasks {
        for dep in &task.deps {
            if !ids.contains(dep) {
                anyhow::bail!("dag task `{}` depends on unknown task `{}`", task.id, dep);
            }
        }
    }

    let mut completed = HashSet::new();
    let mut remaining = (0..tasks.len()).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(tasks.len());

    while !remaining.is_empty() {
        let mut advanced = false;
        let mut cursor = 0usize;

        while cursor < remaining.len() {
            let idx = remaining[cursor];
            let task = &tasks[idx];
            if task.deps.iter().all(|dep| completed.contains(dep)) {
                let task = tasks[idx].clone();
                completed.insert(task.id.clone());
                ordered.push(ExecutionTask {
                    label: format!("[{}] {}", task.id, task.prompt),
                    prompt: task.prompt,
                });
                remaining.remove(cursor);
                advanced = true;
            } else {
                cursor += 1;
            }
        }

        if !advanced {
            let blocked = remaining
                .iter()
                .map(|idx| tasks[*idx].id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("dag dependency cycle detected among: {}", blocked);
        }
    }

    Ok(ordered)
}

fn inject_prompt(file: &Path, task: &str) -> Result<()> {
    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let updated = inject_prompt_into_doc(&doc, task)?;
    if updated != doc {
        write::atomic_write_pub(file, &updated)?;
    }
    Ok(())
}

fn inject_prompt_into_doc(doc: &str, task: &str) -> Result<String> {
    let components = component::parse(doc).context("failed to parse document components")?;
    let exchange = components
        .iter()
        .find(|comp| comp.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:exchange` component"))?;
    let prompt_line = format!("❯ {}", normalize_task(task));
    let existing = exchange.content(doc);
    if existing.lines().any(|line| line.trim() == prompt_line) {
        return Ok(doc.to_string());
    }

    let relative_boundary = existing
        .lines()
        .scan(0usize, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .filter_map(|(start, line)| {
            line.trim()
                .starts_with("<!-- agent:boundary:")
                .then_some(start)
        })
        .last();

    let insert_at = relative_boundary
        .map(|rel| exchange.open_end + rel)
        .unwrap_or(exchange.close_start);
    let mut result = String::with_capacity(doc.len() + prompt_line.len() + 4);
    result.push_str(&doc[..insert_at]);
    if insert_at > exchange.open_end && !result.ends_with('\n') {
        result.push('\n');
    }
    if insert_at > exchange.open_end && !result.ends_with("\n\n") {
        result.push('\n');
    }
    result.push_str(&prompt_line);
    result.push('\n');
    result.push_str(&doc[insert_at..]);
    Ok(result)
}

#[cfg(test)]
mod tests;
