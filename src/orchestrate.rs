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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    agent,
    agent::streaming::StreamChunk,
    component,
    config::{AgentConfig, Config},
    diff,
    frontmatter::{self, ResolvedMode},
    parallel,
    preflight::PreflightOutput,
    write,
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
        let exe = std::env::current_exe().context("failed to resolve current agent-doc binary")?;
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
        let exe = std::env::current_exe().context("failed to resolve current agent-doc binary")?;
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
        let exe = std::env::current_exe().context("failed to resolve current agent-doc binary")?;
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
            .context("failed to spawn `agent-doc finalize`")?;
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
        let backend = agent::resolve_for_file(agent_name, agent_config, env, file)?;
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
        let Some(backend) = agent::resolve_streaming_for_file(agent_name, agent_config, env, file)?
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
                return Ok(());
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                config.agent.as_deref(),
                config.model.as_deref(),
                global_config,
                lifecycle,
                agent_runner,
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
            let parallel_tasks = batch
                .tasks
                .into_iter()
                .map(|task| parallel::ParallelTask {
                    description: task.clone(),
                    prompt: apply_prompt_preset_block(&task, prompt_preset_block.as_deref()),
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
            let execution_tasks = plan_dag_execution(&dag_tasks)?
                .into_iter()
                .map(|task| ExecutionTask {
                    label: task.label.clone(),
                    prompt: apply_prompt_preset_block(&task.prompt, prompt_preset_block.as_deref()),
                })
                .collect::<Vec<_>>();
            if config.plan {
                print_plan(&execution_tasks);
                return Ok(());
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                config.agent.as_deref(),
                config.model.as_deref(),
                global_config,
                lifecycle,
                agent_runner,
            )
        }
    }
}

fn run_ordered_tasks_internal(
    file: &Path,
    tasks: &[ExecutionTask],
    agent_override: Option<&str>,
    model_override: Option<&str>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    for (idx, task) in tasks.iter().enumerate() {
        eprintln!(
            "[orchestrate] step {}/{}: {}",
            idx + 1,
            tasks.len(),
            task.label
        );
        run_ordered_task_step(
            file,
            &task.prompt,
            agent_override,
            model_override,
            global_config,
            lifecycle,
            agent_runner,
        )?;
    }
    Ok(())
}

fn run_ordered_task_step(
    file: &Path,
    task: &str,
    agent_override: Option<&str>,
    model_override: Option<&str>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    inject_prompt(file, task)?;
    let preflight = lifecycle.preflight(file)?;
    if preflight.no_changes {
        anyhow::bail!("orchestration step did not produce a prompt-bearing diff after injection");
    }

    let doc =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let mode = fm.resolve_mode();
    let agent_name = agent_override
        .or(fm.agent.as_deref())
        .or(global_config.default_agent.as_deref())
        .unwrap_or("claude");
    let model = model_override.or(fm.model.as_deref());
    let prompt = build_agent_prompt(mode, preflight.diff.as_deref(), &doc);
    let expanded_env = expand_frontmatter_env(&fm);
    let (response, finalize_response) = if mode.is_crdt() {
        if let Some(seed) = exchange_stream_seed(&doc)? {
            if let Some(chunks) = agent_runner.send_fresh_streaming(
                file,
                &prompt,
                agent_name,
                global_config.agents.get(agent_name),
                expanded_env.clone(),
                model,
            )? {
                let streamed = stream_step_response(file, &seed, chunks)?;
                (streamed.full_response, streamed.finalize_response)
            } else {
                let response = agent_runner.send_fresh(
                    file,
                    &prompt,
                    agent_name,
                    global_config.agents.get(agent_name),
                    expanded_env,
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
                global_config.agents.get(agent_name),
                expanded_env,
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
            global_config.agents.get(agent_name),
            expanded_env,
            model,
        )?;
        let finalize = response.clone();
        (response, finalize)
    };
    let response_text = if mode.is_template() {
        response
    } else {
        write::strip_assistant_heading(&response)
    };
    let finalize_text = if mode.is_template() {
        finalize_response
    } else {
        write::strip_assistant_heading(&finalize_response)
    };

    if let Some(diff_text) = preflight.diff.as_deref() {
        write::enforce_imperative_response_contract_for_diff(file, diff_text, &response_text)?;
    }

    lifecycle.finalize(
        file,
        preflight.baseline_file.as_deref(),
        &finalize_text,
        mode,
    )?;
    lifecycle.session_check(file)?;
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

    for chunk_result in chunks {
        let chunk = chunk_result.context("stream chunk error")?;
        if !chunk.text.is_empty() {
            response = chunk.text;
            if !chunk.is_final {
                let exchange = render_streamed_exchange(seed, &response);
                crate::stream::flush_to_document(file, &exchange, "exchange", "")?;
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

fn finalize_suffix_from_streamed_prefix(streamed: &str, full: &str) -> Option<String> {
    if let Some(delta) = finalize_suffix_from_open_patch_prefix(streamed, full) {
        return Some(delta);
    }

    if let (Ok((full_patches, full_unmatched)), Ok((streamed_patches, streamed_unmatched))) = (
        crate::template::parse_patches(full),
        crate::template::parse_patches(streamed),
    ) {
        if full_unmatched.trim().is_empty()
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
    match crate::env::expand_values(&fm.env) {
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

fn build_agent_prompt(mode: ResolvedMode, diff_text: Option<&str>, doc: &str) -> String {
    let diff_text = diff_text.unwrap_or_default();
    let prompt_bearing = diff::format_prompt_bearing_changes(diff_text)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();

    if mode.is_template() {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            diff_text, prompt_bearing, doc
        )
    } else {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}\
             The full document is now:\n\n\
             <document>\n{}\n</document>\n\n\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user inserted prompt-bearing edits inline, classify them as prompt targets vs content edits before responding.",
            diff_text, prompt_bearing, doc
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
        extend_task_batch_from_text(&mut batch, exchange_text(&doc)?);
    }

    Ok(batch)
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
        let preset_body = fm
            .prompt_presets
            .get(preset_name)
            .ok_or_else(|| anyhow::anyhow!("unknown prompt preset `{}`", preset_name))?;
        if idx > 0 {
            block.push('\n');
        }
        block.push_str(&format!("(preset {})\n", preset_name));
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
    eprintln!("[orchestrate] plan — {} task(s) (no execution)", tasks.len());
    for (idx, task) in tasks.iter().enumerate() {
        eprintln!("[orchestrate] step {}/{}: {}", idx + 1, tasks.len(), task.label);
        eprintln!("[orchestrate] --- prompt ---");
        for line in task.prompt.lines() {
            eprintln!("[orchestrate] {}", line);
        }
        eprintln!("[orchestrate] --- end prompt ---");
    }
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
    if trimmed.chars().next() != Some(fence_char) {
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
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;

    struct FakeLifecycleOps {
        baseline_file: String,
        preflight_calls: RefCell<usize>,
        finalize_calls: RefCell<Vec<String>>,
        session_checks: RefCell<usize>,
    }

    impl LifecycleOps for FakeLifecycleOps {
        fn preflight(&self, file: &Path) -> Result<PreflightOutput> {
            *self.preflight_calls.borrow_mut() += 1;
            let doc = fs::read_to_string(file)?;
            Ok(PreflightOutput {
                diff: Some(format!(
                    "--- snapshot\n+++ document\n+{}",
                    doc.lines().last().unwrap_or("")
                )),
                no_changes: false,
                baseline_file: Some(self.baseline_file.clone()),
                ..PreflightOutput::default()
            })
        }

        fn finalize(
            &self,
            file: &Path,
            _baseline_file: Option<&str>,
            response: &str,
            _mode: ResolvedMode,
        ) -> Result<()> {
            self.finalize_calls.borrow_mut().push(response.to_string());
            write::apply_template_from_string(file, response)
        }

        fn session_check(&self, _file: &Path) -> Result<()> {
            *self.session_checks.borrow_mut() += 1;
            Ok(())
        }
    }

    struct FakeAgentRunner {
        prompts: RefCell<Vec<String>>,
        fresh_calls: RefCell<usize>,
        streaming_calls: RefCell<usize>,
        response: String,
        streaming_chunks: Option<Vec<StreamChunk>>,
    }

    impl FreshAgentRunner for FakeAgentRunner {
        fn send_fresh(
            &self,
            _file: &Path,
            prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<String> {
            *self.fresh_calls.borrow_mut() += 1;
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(self.response.clone())
        }

        fn send_fresh_streaming(
            &self,
            _file: &Path,
            prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<Option<Box<dyn Iterator<Item = Result<StreamChunk>>>>> {
            let Some(chunks) = &self.streaming_chunks else {
                return Ok(None);
            };
            *self.streaming_calls.borrow_mut() += 1;
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(Some(Box::new(chunks.clone().into_iter().map(Ok))))
        }
    }

    struct CaptureAgent {
        seen_prompt: RefCell<Vec<String>>,
        seen_session_id: RefCell<Vec<Option<String>>>,
        seen_fork: RefCell<Vec<bool>>,
    }

    #[derive(Default)]
    struct FakeParallelRunner {
        calls: RefCell<
            Vec<(
                String,
                Vec<parallel::ParallelTask>,
                Option<String>,
                bool,
                bool,
                u64,
                bool,
            )>,
        >,
    }

    impl ParallelRunner for FakeParallelRunner {
        fn run(&self, file: &Path, config: parallel::ParallelConfig) -> Result<()> {
            self.calls.borrow_mut().push((
                file.display().to_string(),
                config.tasks,
                config.model,
                config.no_git,
                config.no_worktree,
                config.timeout_secs,
                config.dry_run,
            ));
            Ok(())
        }
    }

    impl agent::Agent for CaptureAgent {
        fn send(
            &self,
            prompt: &str,
            session_id: Option<&str>,
            fork: bool,
            _model: Option<&str>,
        ) -> Result<agent::AgentResponse> {
            self.seen_prompt.borrow_mut().push(prompt.to_string());
            self.seen_session_id
                .borrow_mut()
                .push(session_id.map(str::to_string));
            self.seen_fork.borrow_mut().push(fork);
            Ok(agent::AgentResponse {
                text: "ok".to_string(),
                session_id: None,
            })
        }
    }

    fn template_doc() -> String {
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nBody\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n".to_string()
    }

    #[test]
    fn extract_tasks_prefers_last_fenced_list() {
        let text = "Notes\n\n- old one\n\n```md\n- do first\n- do second\n```\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["do first".to_string(), "do second".to_string()]
        );
    }

    #[test]
    fn extract_tasks_uses_last_markdown_list() {
        let text = "alpha\n\n- first\n- second\n\nTail\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn resolve_task_batch_collects_exchange_prompt_presets() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nprompt_presets:\n  \"#1\": |\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n- do #report\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap();

        assert_eq!(
            batch.tasks,
            vec!["do #prep".to_string(), "do #report".to_string()]
        );
        assert_eq!(batch.requested_presets, vec!["#1".to_string()]);
    }

    #[test]
    fn apply_prompt_preset_block_prefixes_task_prompt() {
        let rendered = apply_prompt_preset_block(
            "do #prep",
            Some("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\n"),
        );
        assert_eq!(
            rendered,
            "(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean.\ndo #prep"
        );
    }

    #[test]
    fn inject_prompt_inserts_before_boundary() {
        let updated = inject_prompt_into_doc(&template_doc(), "do #gkke").unwrap();
        let prompt_pos = updated.find("❯ do #gkke").unwrap();
        let boundary_pos = updated.find("<!-- agent:boundary:keep -->").unwrap();
        assert!(prompt_pos < boundary_pos);
    }

    #[test]
    fn send_fresh_response_uses_no_resume() {
        let agent = CaptureAgent {
            seen_prompt: RefCell::new(Vec::new()),
            seen_session_id: RefCell::new(Vec::new()),
            seen_fork: RefCell::new(Vec::new()),
        };
        let response = send_fresh_response(&agent, "prompt", Some("gpt-5")).unwrap();
        assert_eq!(response.text, "ok");
        assert_eq!(agent.seen_session_id.borrow().as_slice(), &[None]);
        assert_eq!(agent.seen_fork.borrow().as_slice(), &[false]);
        assert_eq!(
            agent.seen_prompt.borrow().as_slice(),
            &["prompt".to_string()]
        );
    }

    #[test]
    fn sequential_orchestration_injects_prompt_and_finalizes() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        let tasks = vec![ExecutionTask {
            label: "do #gkke".to_string(),
            prompt: "do #gkke".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            None,
            Some("gpt-5"),
            &Config::default(),
            &lifecycle,
            &agent,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("❯ do #gkke"));
        assert!(final_doc.contains("### Re: task — gpt-5"));
        assert_eq!(*lifecycle.preflight_calls.borrow(), 1);
        assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
        assert_eq!(*lifecycle.session_checks.borrow(), 1);
        assert!(
            agent.prompts.borrow()[0].contains("<diff>"),
            "sequential prompt should include the document diff"
        );
        assert!(
            agent.prompts.borrow()[0].contains("❯ do #gkke"),
            "fresh agent prompt should include the injected task"
        );
        assert_eq!(*agent.fresh_calls.borrow(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 0);
    }

    #[test]
    fn sequential_orchestration_always_reruns_preflight_after_injection() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        let tasks = vec![ExecutionTask {
            label: "do #opcc".to_string(),
            prompt: "do #opcc".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            None,
            Some("gpt-5"),
            &Config::default(),
            &lifecycle,
            &agent,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert_eq!(
            *lifecycle.preflight_calls.borrow(),
            1,
            "sequential mode should always rerun preflight after prompt injection"
        );
        assert!(final_doc.contains("❯ do #opcc"));
        assert!(agent.prompts.borrow()[0].contains("❯ do #opcc"));
        assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
        assert_eq!(*lifecycle.session_checks.borrow(), 1);
        assert_eq!(*agent.fresh_calls.borrow(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 0);
    }

    #[test]
    fn sequential_orchestration_expands_prompt_presets_into_task_prompt() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nprompt_presets:\n  \"#1\": |\n    Today is 2026-04-25.\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, content).unwrap();
        fs::write(&baseline, content).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                agent: None,
                model: Some("gpt-5".to_string()),
                no_git: false,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        let prompt = agent.prompts.borrow()[0].clone();
        assert!(prompt.contains("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean."));
        assert!(prompt.contains("❯ (preset #1)\nToday is 2026-04-25."));
    }

    #[test]
    fn render_streamed_exchange_inserts_response_before_boundary() {
        let seed = ExchangeStreamSeed {
            prefix: "❯ do #4qja\n".to_string(),
            suffix: "<!-- agent:boundary:keep -->\n".to_string(),
        };

        let rendered = render_streamed_exchange(
            &seed,
            "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n<!-- /patch:exchange -->\n",
        );
        let response_pos = rendered.find("### Re: streamed — gpt-5").unwrap();
        let boundary_pos = rendered.find("<!-- agent:boundary:keep -->").unwrap();
        assert!(response_pos < boundary_pos);
    }

    #[test]
    fn finalize_suffix_uses_only_unseen_stream_tail() {
        let streamed = "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n";
        let full = "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n";
        let delta = finalize_suffix_from_streamed_prefix(streamed, full).unwrap();
        assert!(!delta.contains("### Re: streamed — gpt-5"));
        assert!(delta.contains("Implemented and verified."));
    }

    #[test]
    fn sequential_orchestration_uses_streaming_backend_for_crdt_docs() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: String::new(),
            streaming_chunks: Some(vec![
                StreamChunk {
                    text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n".to_string(),
                    thinking: None,
                    is_final: false,
                    session_id: None,
                },
                StreamChunk {
                    text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n".to_string(),
                    thinking: None,
                    is_final: true,
                    session_id: Some("sess-stream".to_string()),
                },
            ]),
        };

        let tasks = vec![ExecutionTask {
            label: "do #4qja".to_string(),
            prompt: "do #4qja".to_string(),
        }];

        run_ordered_tasks_internal(
            &doc,
            &tasks,
            None,
            Some("gpt-5"),
            &Config::default(),
            &lifecycle,
            &agent,
        )
        .unwrap();

        let final_doc = fs::read_to_string(&doc).unwrap();
        assert!(final_doc.contains("❯ do #4qja"));
        assert!(final_doc.contains("### Re: streamed — gpt-5"));
        assert_eq!(final_doc.matches("### Re: streamed — gpt-5").count(), 1);
        assert_eq!(*agent.streaming_calls.borrow(), 1);
        assert_eq!(*agent.fresh_calls.borrow(), 0);
    }

    #[test]
    fn resolve_dag_tasks_supports_fan_in_dependencies() {
        let tasks = [
            "do #prep. Prepare context",
            "[after=#prep] do #bench. Run benchmarks",
            "[id=report after=#prep,#bench] Summarize both results",
        ];

        let parsed = tasks
            .iter()
            .enumerate()
            .map(|(idx, task)| parse_dag_task_line(task, idx).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(parsed[0].id, "#prep");
        assert!(parsed[0].deps.is_empty());
        assert_eq!(parsed[1].id, "#bench");
        assert_eq!(parsed[1].deps, vec!["#prep".to_string()]);
        assert_eq!(parsed[2].id, "report");
        assert_eq!(
            parsed[2].deps,
            vec!["#prep".to_string(), "#bench".to_string()]
        );
        assert_eq!(parsed[2].prompt, "Summarize both results");
    }

    #[test]
    fn dag_schedule_rejects_unknown_dependency() {
        let tasks = vec![DagTask {
            id: "#prep".to_string(),
            prompt: "do #prep".to_string(),
            deps: vec!["#missing".to_string()],
        }];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("unknown task `#missing`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dag_schedule_rejects_cycles() {
        let tasks = vec![
            DagTask {
                id: "#a".to_string(),
                prompt: "do #a".to_string(),
                deps: vec!["#b".to_string()],
            },
            DagTask {
                id: "#b".to_string(),
                prompt: "do #b".to_string(),
                deps: vec!["#a".to_string()],
            },
        ];

        let err = plan_dag_execution(&tasks).unwrap_err().to_string();
        assert!(
            err.contains("dag dependency cycle detected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn dag_orchestration_runs_topological_order() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let baseline = dir.path().join("baseline.md");
        fs::write(&doc, template_doc()).unwrap();
        fs::write(&baseline, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: baseline.to_string_lossy().into_owned(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                    .to_string(),
            streaming_chunks: None,
        };
        let dag_tasks = vec![
            DagTask {
                id: "#prep".to_string(),
                prompt: "do #prep".to_string(),
                deps: Vec::new(),
            },
            DagTask {
                id: "#report".to_string(),
                prompt: "do #report".to_string(),
                deps: vec!["#prep".to_string(), "#bench".to_string()],
            },
            DagTask {
                id: "#bench".to_string(),
                prompt: "do #bench".to_string(),
                deps: vec!["#prep".to_string()],
            },
        ];

        let execution = plan_dag_execution(&dag_tasks).unwrap();
        assert_eq!(
            execution
                .iter()
                .map(|task| task.prompt.as_str())
                .collect::<Vec<_>>(),
            vec!["do #prep", "do #bench", "do #report"]
        );

        run_ordered_tasks_internal(
            &doc,
            &execution,
            None,
            Some("gpt-5"),
            &Config::default(),
            &lifecycle,
            &agent,
        )
        .unwrap();

        assert_eq!(lifecycle.finalize_calls.borrow().len(), 3);
        assert_eq!(*lifecycle.session_checks.borrow(), 3);
        let prompts = agent.prompts.borrow();
        assert!(prompts[0].contains("❯ do #prep"));
        assert!(prompts[1].contains("❯ do #bench"));
        assert!(prompts[2].contains("❯ do #report"));
    }

    #[test]
    fn parallel_mode_uses_shared_parallel_runner() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: vec!["  ❯ do #9pw9  ".to_string()],
                from_file: None,
                from_exchange: false,
                agent: None,
                model: Some("gpt-5".to_string()),
                no_git: true,
                no_worktree: true,
                timeout_secs: 45,
                dry_run: true,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        let calls = parallel_runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1,
            vec![parallel::ParallelTask {
                description: "do #9pw9".to_string(),
                prompt: "do #9pw9".to_string(),
            }]
        );
        assert_eq!(calls[0].2.as_deref(), Some("gpt-5"));
        assert!(calls[0].3);
        assert!(calls[0].4);
        assert_eq!(calls[0].5, 45);
        assert!(calls[0].6);
        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn parallel_mode_expands_prompt_presets_into_task_prompt_only() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
            &doc,
            "---\nprompt_presets:\n  \"#1\": |\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        let call = &parallel_runner.calls.borrow()[0];
        assert_eq!(call.1[0].description, "do #prep");
        assert_eq!(
            call.1[0].prompt,
            "(preset #1)\nKeep the work tree clean.\ndo #prep"
        );
    }

    #[test]
    fn legacy_parallel_compat_allows_empty_task_list() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: false,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 600,
                dry_run: false,
                plan: false,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            true,
        )
        .unwrap();

        let calls = parallel_runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.is_empty());
    }

    #[test]
    fn plan_flag_sequential_prints_expanded_prompts_without_executing() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: vec!["do #prep".to_string(), "do #report".to_string()],
                from_file: None,
                from_exchange: false,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn plan_flag_sequential_expands_preset_in_output() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let preset_doc = "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: claude\nprompt_presets:\n  \"#1\": \"Today is 2026-04-25.\\nKeep the work tree clean.\"\n---\n<!-- agent:exchange -->\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n<!-- agent:pending patch=replace -->\n<!-- /agent:pending -->\n";
        fs::write(&doc, preset_doc).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: vec!["do #prep".to_string()],
                from_file: None,
                from_exchange: false,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &FakeParallelRunner::default(),
            false,
        )
        .unwrap();

        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn plan_flag_parallel_exits_without_calling_runner() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            preflight_calls: RefCell::new(0),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            fresh_calls: RefCell::new(0),
            streaming_calls: RefCell::new(0),
            response: "unused".to_string(),
            streaming_chunks: None,
        };
        let parallel_runner = FakeParallelRunner::default();

        run_with_dependencies(
            &doc,
            OrchestrateConfig {
                mode: OrchestrateMode::Parallel,
                tasks_explicit: vec!["do #a".to_string(), "do #b".to_string()],
                from_file: None,
                from_exchange: false,
                agent: None,
                model: None,
                no_git: false,
                no_worktree: false,
                timeout_secs: 30,
                dry_run: false,
                plan: true,
            },
            &Config::default(),
            &lifecycle,
            &agent,
            &parallel_runner,
            false,
        )
        .unwrap();

        assert!(parallel_runner.calls.borrow().is_empty());
    }
}
