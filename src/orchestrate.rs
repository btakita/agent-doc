//! # Module: orchestrate
//!
//! ## Spec
//! - `run(file, config)`: resolves orchestration tasks from `--task`,
//!   `--from-file`, and/or `--from-exchange`, then dispatches by
//!   `OrchestrateMode`.
//! - `--mode sequential` injects each task into the document exchange as a
//!   fresh prompt, admits the response cycle, sends one fresh agent request with no
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

use agent_doc_run_context_io::AgentDocContextExt;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
use agent_doc_session_accretion::SessionAccretionReport;
use anyhow::{Context, Result};
use clap::ValueEnum;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agent_doc_element::element;

use crate::{
    frontmatter::{self, ResolvedMode},
    parallel, queue_dispatch,
};
use agent_doc_agent_io::agent;
use agent_doc_config::{AgentConfig, Config};
use agent_doc_preflight_io::PreflightOutput;
use agent_doc_prompt_context::AgentPromptContext;
use agent_doc_queue::dispatch_item::{QueueItemKind, classify};
#[cfg(test)]
use agent_doc_template::patchback::child_template_finalize_text as orchestrate_finalize_text_for_template;
use agent_doc_template::patchback::{
    finalize_suffix_from_streamed_prefix, should_stream_exchange_patch,
};
use agent_doc_turn_executor::{
    agent_stream::StreamChunk,
    binary::{current_agent_doc_binary, internal_command_spawn_context},
};
#[cfg(test)]
use agent_doc_workflow::orchestrate_tasks::{
    DagTask, extract_tasks_from_text, parse_dag_task_line, parse_list_item,
};
use agent_doc_workflow::orchestrate_tasks::{
    ExchangeTaskSourceFingerprint, ExecutionTask, ResolvedTaskBatch, append_worker_result_line,
    apply_prompt_preset_block, extend_task_batch_from_text, find_exchange_task_source,
    merge_task_batch, normalize_task, plan_dag_execution, resolve_dag_tasks, scope_exchange_tail,
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderedTaskRunOptions<'a> {
    exchange_source: Option<&'a ExchangeTaskSourceFingerprint>,
    agent_override: Option<&'a str>,
    model_override: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OrderedTaskStepOptions<'a> {
    agent_override: Option<&'a str>,
    model_override: Option<&'a str>,
    graph_context: Option<&'a str>,
    graph_evidence: Option<&'a crate::tsift_graph::TsiftGraphEvidencePlan>,
    task_label: &'a str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScheduledDagRunOptions<'a> {
    prompt_preset_block: Option<&'a str>,
    ordered: OrderedTaskRunOptions<'a>,
    graph_evidence: Option<&'a crate::tsift_graph::TsiftGraphEvidencePlan>,
}

pub(crate) trait LifecycleOps {
    fn admit(&self, file: &Path) -> Result<()> {
        let _ = self.preflight(file)?;
        Ok(())
    }

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

pub(crate) trait FreshAgentRunner {
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

pub(crate) trait ParallelRunner {
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
    fn admit(&self, file: &Path) -> Result<()> {
        let file_arg = file.to_string_lossy().into_owned();
        let _: serde_json::Value = self.run_output_json(&["admit", &file_arg])?;
        Ok(())
    }

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
        let content = resolve_orchestrate_current_document(file, "orchestrate_fresh_agent")?;
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
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
        let content = resolve_orchestrate_current_document(file, "orchestrate_streaming_agent")?;
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
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

mod dag;
pub(crate) use dag::*;

mod dispatch;
pub(crate) use dispatch::*;

fn expand_frontmatter_env(fm: &frontmatter::Frontmatter) -> Vec<(String, Option<String>)> {
    if fm.env.is_empty() {
        return Vec::new();
    }
    match agent_doc_config::env::expand_values(&fm.env) {
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
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
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
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let diff_text = diff_text.unwrap_or_default();
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let ssh_context = rc.ssh_context();
    let document_section = agent_doc_prompt_context_io::build_document_section_with_ssh_context(
        file,
        diff_text,
        doc,
        session_accretion,
        &ssh_context,
    );

    agent_doc_prompt_context::render_agent_prompt(AgentPromptContext {
        template_mode: mode.is_template(),
        diff_text,
        doc,
        document_section: &document_section,
    })
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
        let doc = resolve_orchestrate_current_document(file, "orchestrate_from_exchange")?;
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
        let doc = resolve_orchestrate_current_document(file, "orchestrate_from_queue")?;
        merge_task_batch(&mut batch, queue_task_batch(file, &doc)?);
    }

    canonicalize_prompt_preset_requests(file, &mut batch.requested_presets)?;
    if let Some(source) = &mut batch.exchange_source {
        canonicalize_prompt_preset_requests(file, &mut source.requested_presets)?;
    }

    Ok(batch)
}

fn resolve_orchestrate_current_document(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
}

fn canonicalize_prompt_preset_requests(
    file: &Path,
    requested_presets: &mut Vec<String>,
) -> Result<()> {
    if requested_presets.is_empty() {
        return Ok(());
    }
    let doc = resolve_orchestrate_current_document(file, "orchestrate_prompt_presets")?;
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

fn exchange_text(doc: &str) -> Result<&str> {
    let components = element::parse(doc).context("failed to parse document components")?;
    let exchange = components
        .iter()
        .find(|comp| comp.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:exchange` component"))?;
    Ok(exchange.content(doc))
}

fn active_queue_hash(doc: &str) -> Result<Option<String>> {
    let components = element::parse(doc).context("failed to parse document components")?;
    let queue = components
        .iter()
        .find(|comp| comp.name == "queue")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:queue` component"))?;
    let body = queue.content(doc);
    let entries = agent_doc_queue::document_queue::parse(body)?;
    let (fm, _) = frontmatter::parse(doc)?;
    let activation = agent_doc_queue::document_queue::resolve_activation(
        &entries,
        agent_doc_queue::document_queue::has_auto_attr(&queue.attrs),
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return Ok(None);
    }
    Ok(Some(agent_doc_hash::content_hash(
        &agent_doc_queue::document_queue::render(&activation.entries_after),
    )))
}

fn queue_task_batch(file: &Path, doc: &str) -> Result<ResolvedTaskBatch> {
    use agent_doc_state_backbone::QueueWorklistEntryKind;

    let Some(current_queue_hash) = active_queue_hash(doc)? else {
        anyhow::bail!(
            "agent:queue is not active; add `auto`, a start fence, or queue_active=true before --from-queue dispatch"
        );
    };
    let canonical = file.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize {} for typed queue dispatch",
            file.display()
        )
    })?;
    let project_root = agent_doc_fs::find_project_root(&canonical).ok_or_else(|| {
        anyhow::anyhow!(
            "typed queue dispatch requires a .agent-doc project root for {}",
            file.display()
        )
    })?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical)?;
    let ledger =
        agent_doc_controller_io::project_controller::load_state_event_ledger(&project_root)
            .with_context(|| {
                format!(
                    "failed to load typed queue state ledger under {}",
                    project_root.display()
                )
            })?;
    let projection = ledger.project_document(&document_hash).ok_or_else(|| {
        anyhow::anyhow!(
            "typed queue worklist is unavailable for {}; run queue maintenance/admission before --from-queue dispatch",
            file.display()
        )
    })?;
    if !projection.queue.worklist_active {
        anyhow::bail!(
            "typed queue worklist is inactive for {}; run queue maintenance/admission before --from-queue dispatch",
            file.display()
        );
    }
    if projection.queue.worklist_queue_hash.as_deref() != Some(current_queue_hash.as_str()) {
        anyhow::bail!(
            "typed queue worklist is stale for {}; current queue hash {} does not match projected hash {}",
            file.display(),
            current_queue_hash,
            projection
                .queue
                .worklist_queue_hash
                .as_deref()
                .unwrap_or("<missing>")
        );
    }
    let mut batch = ResolvedTaskBatch::default();
    for entry in projection.queue.worklist {
        match entry.kind {
            QueueWorklistEntryKind::Prompt => {
                batch.tasks.push(normalize_task(&entry.text));
            }
            QueueWorklistEntryKind::Preset | QueueWorklistEntryKind::Dispatch => {
                if !batch
                    .requested_presets
                    .iter()
                    .any(|existing| existing == &entry.text)
                {
                    batch.requested_presets.push(entry.text);
                }
            }
        }
    }
    Ok(batch)
}

fn exchange_task_source_changed(
    file: &Path,
    original: &ExchangeTaskSourceFingerprint,
) -> Result<bool> {
    let doc = resolve_orchestrate_current_document(file, "orchestrate_exchange_source_changed")?;
    let exchange = exchange_text(&doc)?;
    Ok(match find_exchange_task_source(exchange, original) {
        Some(current) => current != *original,
        None => true,
    })
}

/// Scope exchange text to the user's latest additions by comparing against
/// the snapshot. Prevents stale task lists in response content from being
/// picked by `extract_tasks_from_text` when the user's directive is a bare
/// line (not a list) at the exchange tail.
fn scope_exchange_for_tasks(exchange: &str, file: &Path) -> String {
    let snap_content = match agent_doc_snapshot_io::load(file) {
        Ok(Some(s)) => s,
        _ => return exchange.to_string(),
    };
    let snap_exchange = match exchange_text(&snap_content) {
        Ok(s) => s.to_string(),
        Err(_) => return exchange.to_string(),
    };

    scope_exchange_tail(exchange, &snap_exchange)
}

fn load_prompt_preset_block(file: &Path, requested_presets: &[String]) -> Result<Option<String>> {
    if requested_presets.is_empty() {
        return Ok(None);
    }

    let doc = resolve_orchestrate_current_document(file, "orchestrate_prompt_preset_block")?;
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
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_work_graph::source_changed_event(completed_steps, total_steps),
        agent_doc_ops_log_io::log_op,
    );
    eprintln!(
        "[orchestrate] source task list changed after step {}/{}; stopping before next step",
        completed_steps, total_steps
    );
    lifecycle.admit(file)?;
    let doc = resolve_orchestrate_current_document(file, "orchestrate_batch_changed")?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let response = format!(
        "<!-- patch:exchange -->\n\
### Re: orchestration batch changed — gpt-5\n\n\
Stopped sequential orchestration after {completed_steps} of {total_steps} step(s) because the source task list changed while the batch was running. The remaining and newly added tasks are still open for the next explicit orchestration run.\n\
<!-- /patch:exchange -->\n"
    );
    lifecycle
        .finalize(file, None, &response, fm.resolve_mode())
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

fn inject_prompt(file: &Path, task: &str) -> Result<()> {
    let doc = resolve_orchestrate_current_document(file, "orchestrate_inject_prompt")?;
    let prompt_line = format!("❯ {}", normalize_task(task));
    let updated =
        agent_doc_element_exchange::insert_prompt_line_before_boundary(&doc, &prompt_line)?;
    if updated != doc {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, &updated)?;
    }
    Ok(())
}

#[cfg(test)]
mod th {
    use super::*;
    use std::cell::RefCell;

    pub(crate) struct EnvGuard {
        pub(crate) key: &'static str,
        pub(crate) prev: Option<String>,
        pub(crate) _lock: std::sync::MutexGuard<'static, ()>,
    }
    impl EnvGuard {
        pub(crate) fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock().unwrap();
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
    pub(crate) struct FakeLifecycleOps {
        pub(crate) baseline_file: String,
        pub(crate) admit_calls: RefCell<usize>,
        pub(crate) preflight_calls: RefCell<usize>,
        pub(crate) finalize_calls: RefCell<Vec<String>>,
        pub(crate) session_checks: RefCell<usize>,
    }
    impl LifecycleOps for FakeLifecycleOps {
        fn admit(&self, file: &Path) -> Result<()> {
            *self.admit_calls.borrow_mut() += 1;
            ensure_git_baseline_commit(file)?;
            Ok(())
        }

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
            mode: ResolvedMode,
        ) -> Result<()> {
            self.finalize_calls.borrow_mut().push(response.to_string());
            agent_doc_write_runtime_io::run_command_with_response(
                agent_doc_write_command_io::CommandOptions {
                    file: file.to_path_buf(),
                    baseline_file: None,
                    is_template: mode.is_template(),
                    is_stream: mode.is_crdt(),
                    is_ipc: false,
                    force_disk: true,
                    origin: Some("orchestrate_test".to_string()),
                    no_pending_capture: false,
                    pending_add: Vec::new(),
                    pending_add_to: Vec::new(),
                    pending_add_gated: Vec::new(),
                    pending_add_after: Vec::new(),
                    pending_add_before: Vec::new(),
                    pending_add_back: Vec::new(),
                    icebox_add: Vec::new(),
                    icebox_add_after: Vec::new(),
                    icebox_add_before: Vec::new(),
                    icebox_add_back: Vec::new(),
                    icebox_edit: Vec::new(),
                    icebox_clear: false,
                    icebox_reorder: None,
                    pending_done: Vec::new(),
                    pending_edit: Vec::new(),
                    pending_clear: false,
                    pending_reorder: None,
                    pending_gate: Vec::new(),
                    pending_ungate: Vec::new(),
                    pending_resolve_gate: Vec::new(),
                    pending_set_gate_type: Vec::new(),
                    pending_set_verify: Vec::new(),
                    review_add: Vec::new(),
                    review_edit: Vec::new(),
                    review_remove: Vec::new(),
                    review_resolve: Vec::new(),
                    queue_completion_ids: Vec::new(),
                    allow_replace_pending: false,
                    pending_only: false,
                    status: None,
                    lint_override: None,
                    commit_sibling: Vec::new(),
                    commit_sibling_message: Vec::new(),
                },
                agent_doc_write_command_io::CommitMode::BestEffort,
                response.to_string(),
            )
        }

        fn session_check(&self, _file: &Path) -> Result<()> {
            *self.session_checks.borrow_mut() += 1;
            Ok(())
        }
    }

    fn ensure_git_baseline_commit(file: &Path) -> Result<()> {
        let repo = file
            .parent()
            .ok_or_else(|| anyhow::anyhow!("test document has no parent: {}", file.display()))?;
        if !repo.join(".git").exists() {
            run_git(repo, ["init"])?;
            run_git(repo, ["config", "user.email", "test@example.com"])?;
            run_git(repo, ["config", "user.name", "Agent Doc Test"])?;
        }

        let add_status = Command::new("git")
            .current_dir(repo)
            .arg("add")
            .arg(file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("failed to git add {}", file.display()))?;
        if !add_status.success() {
            anyhow::bail!("git add {} failed with {}", file.display(), add_status);
        }
        let staged_status = Command::new("git")
            .current_dir(repo)
            .args(["diff", "--cached", "--quiet", "--exit-code"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("failed to inspect staged diff for {}", file.display()))?;
        if staged_status.success() {
            return Ok(());
        }

        let output = Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", "test baseline", "--no-verify"])
            .output()
            .with_context(|| format!("failed to git commit {}", file.display()))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("nothing to commit") || stderr.contains("no changes added") {
            return Ok(());
        }
        anyhow::bail!("git commit {} failed: {}", file.display(), stderr.trim());
    }

    fn run_git<const N: usize>(repo: &Path, args: [&str; N]) -> Result<()> {
        let status = Command::new("git")
            .current_dir(repo)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("failed to run git in {}", repo.display()))?;
        if !status.success() {
            anyhow::bail!("git command failed with {}", status);
        }
        Ok(())
    }
    type AgentEnv = Vec<(String, Option<String>)>;
    type ParallelRunCall = (
        String,
        Vec<parallel::ParallelTask>,
        Option<String>,
        bool,
        bool,
        u64,
        bool,
    );
    pub(crate) fn test_graph_evidence_plan() -> crate::tsift_graph::TsiftGraphEvidencePlan {
        crate::tsift_graph::TsiftGraphEvidencePlan {
        targets: vec!["gkke".to_string()],
        graph_db_status: crate::tsift_graph::TsiftGraphDbStatus {
            root: Some("/tmp/repo".to_string()),
            graph_db: Some("/tmp/repo/.tsift/graph.db".to_string()),
            status: "current".to_string(),
            content_hash: Some("abc".to_string()),
            source_watermark: Some("abc".to_string()),
            diagnostics: Vec::new(),
        },
        prompt_target_handles: vec![crate::tsift_graph::TsiftPromptTargetHandle {
            prompt_target: "do #gkke".to_string(),
            target: "gkke".to_string(),
            contract_version: Some("graph-db-evidence-v1".to_string()),
            evidence_packet_id: "gevd-gkke".to_string(),
            target_node_id: "gbak-gkke".to_string(),
            target_kind: "backlog".to_string(),
            target_label: "#gkke".to_string(),
            projection_hash: Some("abc".to_string()),
            worker_context_handles: vec!["wctx-gkke".to_string()],
            source_handles: vec!["src-gkke".to_string()],
            semantic_handles: Vec::new(),
            next_commands: Vec::new(),
            replay_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
            repair_commands: Vec::new(),
        }],
        conflict_matrix: crate::tsift_graph::TsiftConflictMatrixSummary {
            contract_version: Some("conflict-matrix-v1".to_string()),
            can_parallel: true,
            fail_closed: false,
            inputs: Some(crate::tsift_graph::TsiftConflictMatrixInputs {
                graph_db_evidence_targets: vec!["gkke".to_string()],
                evidence_packets: vec![crate::tsift_graph::TsiftConflictMatrixEvidencePacket {
                    target: "gkke".to_string(),
                    packet_id: "gevd-gkke".to_string(),
                    target_node_id: "gbak-gkke".to_string(),
                    projection_hash: Some("abc".to_string()),
                    replay_command: Some("tsift graph-db evidence gkke --json".to_string()),
                }],
                context_pack_command: Some("tsift --envelope context-pack session.md --budget normal".to_string()),
                cached_diff_command: Some("tsift diff-digest --cached /tmp/repo --json".to_string()),
                impact_command: Some("tsift impact /tmp/repo --cached --limit 20 --json".to_string()),
            }),
            context_pack: Some(crate::tsift_graph::TsiftConflictMatrixContextSummary {
                target: "session.md".to_string(),
                target_kind: "agent_doc_session".to_string(),
                prompt_targets: vec!["do #gkke".to_string()],
                touched_files: vec!["src/orchestrate.rs".to_string()],
                touched_symbols: vec!["run_ordered_tasks_internal".to_string()],
                files_changed: 1,
                worker_context: vec!["orchestration worker context".to_string()],
                source_windows: vec!["src/orchestrate.rs:1-80".to_string()],
                status_reminders: Vec::new(),
            }),
            candidates: vec![crate::tsift_graph::TsiftConflictMatrixCandidate {
                target: "gkke".to_string(),
                rank: 1,
                risk: "low".to_string(),
                risk_score: 0,
                risk_reasons: Vec::new(),
                evidence_packet_id: "gevd-gkke".to_string(),
                target_node_id: "gbak-gkke".to_string(),
                target_kind: "backlog".to_string(),
                target_label: "#gkke".to_string(),
                owned_files: vec!["src/orchestrate.rs".to_string()],
                owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                config_files: Vec::new(),
                affected_tests: vec!["cargo test orchestrate".to_string()],
                staged_files: Vec::new(),
                staged_symbols: Vec::new(),
                staged_tests: Vec::new(),
                staged_config_files: Vec::new(),
                semantic_dispatch_score: 4,
                semantic_dispatch_reasons: vec!["source handle matched orchestration".to_string()],
                worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                    total: 1,
                    completed: 1,
                    blocked: 0,
                    touched_files: vec!["src/orchestrate.rs".to_string()],
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    follow_up_ids: Vec::new(),
                    outcome_history: vec!["completed #gkke".to_string()],
                    repeated_blockage: false,
                    stale_expected_tests: Vec::new(),
                    follow_up_debt: Vec::new(),
                    closure_rank_score: 0,
                    closure_rank_reasons: Vec::new(),
                    warnings: Vec::new(),
                }),
            }],
            conflicts: Vec::new(),
            evidence_packet_ids: vec!["gevd-gkke".to_string()],
            decisions: vec!["candidate #1 gkke risk=low".to_string()],
            worker_ownership_blocks: vec!["Worker 1 owns gkke (#gkke)".to_string()],
            worker_prompt_packets: vec![crate::tsift_graph::TsiftWorkerPromptPacket {
                contract_version: Some("worker-prompt-packet-v1".to_string()),
                packet_id: Some("wpp-gkke".to_string()),
                target: "gkke".to_string(),
                rank: 1,
                risk: "low".to_string(),
                projection_hash: Some("abc".to_string()),
                title: "Worker 1 owns gkke (#gkke)".to_string(),
                owned_files: vec!["src/orchestrate.rs".to_string()],
                owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                read_only_context: vec!["src-gkke".to_string()],
                forbidden_files: Vec::new(),
                expected_tests: vec!["cargo test orchestrate".to_string()],
                expansion_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
                token_budget: Some(crate::tsift_graph::TsiftWorkerPromptTokenBudget {
                    prompt_estimated_tokens: 32,
                    max_prompt_tokens: 256,
                    source_window_count: 1,
                    source_window_lines: 80,
                    max_context_bytes: 9600,
                }),
                semantic_dispatch_score: 4,
                semantic_dispatch_reasons: vec![
                    "source handle matched orchestration".to_string()
                ],
                worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                    total: 1,
                    completed: 1,
                    blocked: 0,
                    touched_files: vec!["src/orchestrate.rs".to_string()],
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    follow_up_ids: Vec::new(),
                    outcome_history: vec!["completed #gkke".to_string()],
                    repeated_blockage: false,
                    stale_expected_tests: Vec::new(),
                    follow_up_debt: Vec::new(),
                    closure_rank_score: 0,
                    closure_rank_reasons: Vec::new(),
                    warnings: Vec::new(),
                }),
                prompt: Some(
                    "Worker 1 owns gkke (#gkke)\n\nFail closed if the task requires a forbidden/shared file."
                        .to_string(),
                ),
            }],
            next_commands: Vec::new(),
            warnings: Vec::new(),
        },
        dispatch_trace: Some(crate::tsift_graph::TsiftDispatchTraceSummary {
            contract_version: Some("dispatch-trace-v1".to_string()),
            projection_freshness: crate::tsift_graph::TsiftProjectionFreshness {
                status: "current".to_string(),
                fail_closed: false,
                content_hash: Some("abc".to_string()),
                source_watermark: Some("abc".to_string()),
                diagnostics: Vec::new(),
            },
            projection_hashes: vec!["abc".to_string()],
            evidence_packet_ids: vec!["gevd-gkke".to_string()],
            worker_prompt_packets: vec![crate::tsift_graph::TsiftWorkerPromptPacket {
                contract_version: Some("worker-prompt-packet-v1".to_string()),
                packet_id: Some("wpp-gkke".to_string()),
                target: "gkke".to_string(),
                rank: 1,
                risk: "low".to_string(),
                projection_hash: Some("abc".to_string()),
                title: "Worker 1 owns gkke (#gkke)".to_string(),
                owned_files: vec!["src/orchestrate.rs".to_string()],
                owned_symbols: vec!["run_ordered_tasks_internal".to_string()],
                read_only_context: vec!["src-gkke".to_string()],
                forbidden_files: Vec::new(),
                expected_tests: vec!["cargo test orchestrate".to_string()],
                expansion_commands: vec!["tsift graph-db evidence gkke --json".to_string()],
                token_budget: Some(crate::tsift_graph::TsiftWorkerPromptTokenBudget {
                    prompt_estimated_tokens: 32,
                    max_prompt_tokens: 256,
                    source_window_count: 1,
                    source_window_lines: 80,
                    max_context_bytes: 9600,
                }),
                semantic_dispatch_score: 4,
                semantic_dispatch_reasons: vec![
                    "source handle matched orchestration".to_string()
                ],
                worker_feedback: Some(crate::tsift_graph::TsiftWorkerFeedbackSummary {
                    total: 1,
                    completed: 1,
                    blocked: 0,
                    touched_files: vec!["src/orchestrate.rs".to_string()],
                    expected_tests: vec!["cargo test orchestrate".to_string()],
                    follow_up_ids: Vec::new(),
                    outcome_history: vec!["completed #gkke".to_string()],
                    repeated_blockage: false,
                    stale_expected_tests: Vec::new(),
                    follow_up_debt: Vec::new(),
                    closure_rank_score: 0,
                    closure_rank_reasons: Vec::new(),
                    warnings: Vec::new(),
                }),
                prompt: Some(
                    "Worker 1 owns gkke (#gkke)\n\nFail closed if the task requires a forbidden/shared file."
                        .to_string(),
                ),
            }],
            worker_feedback: vec![crate::tsift_graph::TsiftWorkerFeedbackSummary {
                total: 1,
                completed: 1,
                blocked: 0,
                touched_files: vec!["src/orchestrate.rs".to_string()],
                expected_tests: vec!["cargo test orchestrate".to_string()],
                follow_up_ids: Vec::new(),
                outcome_history: vec!["completed #gkke".to_string()],
                repeated_blockage: false,
                stale_expected_tests: Vec::new(),
                follow_up_debt: Vec::new(),
                closure_rank_score: 0,
                closure_rank_reasons: Vec::new(),
                warnings: Vec::new(),
            }],
            graph_nodes: vec![
                crate::tsift_graph::TsiftDispatchTraceGraphNode {
                    id: "gbak-gkke".to_string(),
                    kind: "backlog".to_string(),
                    label: "#gkke".to_string(),
                    properties: std::collections::BTreeMap::new(),
                },
                crate::tsift_graph::TsiftDispatchTraceGraphNode {
                    id: "wres-gkke".to_string(),
                    kind: "worker_result".to_string(),
                    label: "completed #gkke".to_string(),
                    properties: std::collections::BTreeMap::new(),
                },
            ],
            graph_edges: vec![crate::tsift_graph::TsiftDispatchTraceGraphEdge {
                from_id: "gbak-gkke".to_string(),
                to_id: "wres-gkke".to_string(),
                kind: "has_result".to_string(),
            }],
            replay_commands: vec!["tsift conflict-matrix --path /tmp/repo gkke --json".to_string()],
            repair_commands: vec!["tsift graph-db --path /tmp/repo refresh --json".to_string()],
            warnings: Vec::new(),
        }),
        next_commands: Vec::new(),
    }
    }
    pub(crate) struct FakeAgentRunner {
        pub(crate) prompts: RefCell<Vec<String>>,
        pub(crate) envs: RefCell<Vec<AgentEnv>>,
        pub(crate) fresh_calls: RefCell<usize>,
        pub(crate) streaming_calls: RefCell<usize>,
        pub(crate) response: String,
        pub(crate) streaming_chunks: Option<Vec<StreamChunk>>,
    }
    pub(crate) struct MutatingAgentRunner {
        pub(crate) fresh_calls: RefCell<usize>,
        pub(crate) response: String,
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
            let mut calls = self.fresh_calls.borrow_mut();
            *calls += 1;
            let call = *calls;
            self.prompts.borrow_mut().push(prompt.to_string());
            self.envs.borrow_mut().push(_env);
            Ok(numbered_fake_response(&self.response, call))
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
            self.envs.borrow_mut().push(_env);
            Ok(Some(Box::new(chunks.clone().into_iter().map(Ok))))
        }
    }
    impl FreshAgentRunner for MutatingAgentRunner {
        fn send_fresh(
            &self,
            file: &Path,
            _prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<String> {
            let mut calls = self.fresh_calls.borrow_mut();
            *calls += 1;
            if *calls == 1 {
                let doc = fs::read_to_string(file)?;
                let updated = doc.replace(
                    "- do #first\n- do #second",
                    "- do #first\n- do #inserted\n- do #second",
                );
                fs::write(file, updated)?;
            }
            Ok(self.response.clone())
        }
    }

    fn numbered_fake_response(response: &str, call: usize) -> String {
        if call <= 1 {
            return response.to_string();
        }
        let marker = format!("<!-- agent-doc-test-call:{call} -->\n");
        if let Some(idx) = response.rfind("<!-- /patch:exchange -->") {
            let mut numbered = String::with_capacity(response.len() + marker.len());
            numbered.push_str(&response[..idx]);
            numbered.push_str(&marker);
            numbered.push_str(&response[idx..]);
            numbered
        } else {
            format!("{response}\n{marker}")
        }
    }
    pub(crate) struct CaptureAgent {
        pub(crate) seen_prompt: RefCell<Vec<String>>,
        pub(crate) seen_session_id: RefCell<Vec<Option<String>>>,
        pub(crate) seen_fork: RefCell<Vec<bool>>,
    }
    #[derive(Default)]
    pub(crate) struct FakeParallelRunner {
        pub(crate) calls: RefCell<Vec<ParallelRunCall>>,
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
    pub(crate) fn template_doc() -> String {
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nBody\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n".to_string()
    }
    // --- parse_list_item prompt-prefix stripping (bug #orch3) ---
    pub(crate) fn setup_snapshot_dir(dir: &Path) {
        fs::create_dir_all(dir.join(".agent-doc")).unwrap();
    }
}
#[cfg(test)]
pub(crate) use th::{
    CaptureAgent, EnvGuard, FakeAgentRunner, FakeLifecycleOps, FakeParallelRunner,
    MutatingAgentRunner, setup_snapshot_dir, template_doc, test_graph_evidence_plan,
};

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_state_backbone::{
        QueueWorklistEntry, QueueWorklistEntryKind, StateEvent, StateFact,
    };
    use std::cell::RefCell;
    use tempfile::TempDir;

    fn seed_typed_queue_worklist(
        root: &Path,
        doc: &Path,
        queue_hash: &str,
        entries: Vec<QueueWorklistEntry>,
    ) {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let document_hash =
            agent_doc_fs::document_state_hash(&doc.canonicalize().unwrap()).unwrap();
        let event = StateEvent::new(
            format!("test-queue-worklist:{queue_hash}"),
            StateFact::QueueWorklistProjected {
                document_hash,
                queue_hash: queue_hash.to_string(),
                entries,
                active: true,
                hosting_epoch: None,
            },
        );
        agent_doc_controller_io::project_controller::append_state_event(root, &event).unwrap();
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
                from_queue: false,
                resume_schedule: None,
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
    fn resolve_task_batch_canonicalizes_bare_hashtag_prompt_preset() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(
        &doc,
        "---\nprompt_presets:\n  \"#spec-test\": |\n    Run checks.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset spec-test\n- do #prep\n<!-- /agent:exchange -->\n",
    )
    .unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
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

        assert_eq!(batch.requested_presets, vec!["#spec-test".to_string()]);
        assert_eq!(
            load_prompt_preset_block(&doc, &batch.requested_presets)
                .unwrap()
                .as_deref(),
            Some("(preset #spec-test)\nRun checks.\n")
        );
    }
    #[test]
    fn resolve_task_batch_collects_active_queue_for_auto_dag() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let content = "---\nqueue_active: true\nprompt_presets:\n  \"#spec-test\": |\n    Run checks.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\npreset spec-test\n- do #prep\n- do #report after #prep\n<!-- /agent:queue -->\n";
        fs::write(&doc, content).unwrap();
        let queue_hash = active_queue_hash(content).unwrap().unwrap();
        seed_typed_queue_worklist(
            dir.path(),
            &doc,
            &queue_hash,
            vec![
                QueueWorklistEntry {
                    kind: QueueWorklistEntryKind::Preset,
                    text: "spec-test".to_string(),
                    node_key: None,
                    backlog_id: None,
                    drainable: false,
                },
                QueueWorklistEntry {
                    kind: QueueWorklistEntryKind::Prompt,
                    text: "do #prep".to_string(),
                    node_key: Some("queue:entry:0:prep".to_string()),
                    backlog_id: Some("prep".to_string()),
                    drainable: true,
                },
                QueueWorklistEntry {
                    kind: QueueWorklistEntryKind::Prompt,
                    text: "do #report after #prep".to_string(),
                    node_key: Some("queue:entry:1:report".to_string()),
                    backlog_id: Some("report".to_string()),
                    drainable: true,
                },
            ],
        );

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Dag,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: false,
                from_queue: true,
                resume_schedule: None,
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
            vec!["do #prep".to_string(), "do #report after #prep".to_string()]
        );
        assert_eq!(batch.requested_presets, vec!["#spec-test".to_string()]);
    }

    #[test]
    fn resolve_task_batch_rejects_stale_typed_queue_worklist() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        let original = "---\nqueue_active: true\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue auto -->\n- do #old\n<!-- /agent:queue -->\n";
        fs::write(&doc, original).unwrap();
        let old_hash = active_queue_hash(original).unwrap().unwrap();
        seed_typed_queue_worklist(
            dir.path(),
            &doc,
            &old_hash,
            vec![QueueWorklistEntry {
                kind: QueueWorklistEntryKind::Prompt,
                text: "do #old".to_string(),
                node_key: Some("queue:entry:0:old".to_string()),
                backlog_id: Some("old".to_string()),
                drainable: true,
            }],
        );
        let changed = original.replace("do #old", "do #new");
        fs::write(&doc, changed).unwrap();

        let err = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Dag,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: false,
                from_queue: true,
                resume_schedule: None,
                agent: None,
                model: None,
                no_git: true,
                no_worktree: true,
                timeout_secs: 30,
                dry_run: true,
                plan: false,
            },
        )
        .unwrap_err();

        assert!(
            err.to_string().contains("typed queue worklist is stale"),
            "unexpected error: {err:#}"
        );
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
    fn exchange_task_source_fingerprint_detects_list_mutations() {
        let original = ExchangeTaskSourceFingerprint {
            tasks: vec!["do #first".to_string(), "do #second".to_string()],
            requested_presets: vec!["#spec".to_string()],
        };
        let source = find_exchange_task_source(
        "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:keep -->\n",
        &original,
    )
    .unwrap();
        assert_eq!(source, original);

        let boundary_only = find_exchange_task_source(
        "sync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:new -->\n",
        &source,
    )
    .unwrap();
        assert_eq!(boundary_only, source);

        let inserted = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #first\n- do #inserted\n- do #second\n",
            &source,
        )
        .unwrap();
        assert_ne!(inserted, source);

        let reordered = find_exchange_task_source(
            "sync orchestra\npreset #spec\n- do #second\n- do #first\n",
            &source,
        );
        assert!(reordered.is_none());

        let quoted_later = find_exchange_task_source(
        "sync orchestra\npreset #spec\n- do #first\n- do #second\n\n### Re: response — gpt-5\n\n- do #first\n- do #extra\n- do #second\n",
        &source,
    )
    .unwrap();
        assert_eq!(quoted_later, source);
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
    fn parallel_graph_context_maps_worker_packet_to_lower_agent_job_packet() {
        let graph_evidence = test_graph_evidence_plan();
        let prompt =
            apply_parallel_graph_context("do #gkke", "do #gkke".to_string(), Some(&graph_evidence));

        assert!(prompt.contains("\"lower_agent_job_packet\""));
        assert!(prompt.contains("\"contract_version\": \"agent-doc-lower-agent-job-v1\""));
        assert!(prompt.contains("\"source_contract_version\": \"worker-prompt-packet-v1\""));
        assert!(prompt.contains("\"packet_id\": \"wpp-gkke\""));
        assert!(prompt.contains("\"owned_files\": ["));
        assert!(prompt.contains("\"read_only_context\": ["));
        assert!(prompt.contains("\"forbidden_files\": []"));
        assert!(prompt.contains("\"expected_tests\": ["));
        assert!(prompt.contains("\"expansion_commands\": ["));
        assert!(prompt.contains("\"fail_closed_prompt\""));
        assert!(prompt.contains("Fail closed if the task requires a forbidden/shared file"));
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
    fn orchestrate_template_finalize_wraps_plain_response_as_exchange_patch() {
        let finalize = orchestrate_finalize_text_for_template(
            "### Re: plain orch — gpt-5\n\nImplemented and verified.".to_string(),
        );

        assert!(finalize.starts_with("<!-- patch:exchange -->"));
        assert!(finalize.contains("### Re: plain orch"));
        assert!(finalize.ends_with("<!-- /patch:exchange -->\n"));
    }
    #[test]
    fn orchestrate_template_finalize_does_not_wrap_transcript_response() {
        let transcript = "❯ do #next\n### Re: malformed — gpt-5\nBody";
        let finalize = orchestrate_finalize_text_for_template(transcript.to_string());

        assert_eq!(finalize, transcript);
    }
    #[test]
    fn streamed_flush_waits_for_exchange_patch_marker() {
        assert!(!should_stream_exchange_patch(
            "### Re: malformed streaming closeout — gpt-5\nBody"
        ));
        assert!(should_stream_exchange_patch(
            "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n"
        ));
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
    fn parse_list_item_strips_prompt_prefix() {
        let result = parse_list_item("❯ - do #task1");
        assert_eq!(result, Some("do #task1".to_string()));
    }
    #[test]
    fn parse_list_item_strips_prompt_prefix_with_star() {
        let result = parse_list_item("❯ * do #task2");
        assert_eq!(result, Some("do #task2".to_string()));
    }
    #[test]
    fn parse_list_item_without_prefix_still_works() {
        let result = parse_list_item("- do #task3");
        assert_eq!(result, Some("do #task3".to_string()));
    }
    #[test]
    fn parse_list_item_strips_prompt_prefix_numbered() {
        let result = parse_list_item("❯ 1. do #task4");
        assert_eq!(result, Some("do #task4".to_string()));
    }
    #[test]
    fn collect_markdown_list_blocks_with_prompt_prefix() {
        let text = "❯ - do #a\n❯ - do #b\n\nsome other text\n";
        assert_eq!(
            extract_tasks_from_text(text),
            vec!["do #a".to_string(), "do #b".to_string()]
        );
    }
    #[test]
    fn from_exchange_scopes_to_tail_bare_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted summary.\n\n",
            "❯ Previous prompt\n\n",
            "### Re: response — opus-4-6\n\n",
            "Recommendations:\n\n",
            "- **#stale1** — fix stale-task parsing\n",
            "- **#envt1** — fix env test\n\n",
            "All marked [recommended].\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted summary.\n\n",
            "❯ Previous prompt\n\n",
            "### Re: response — opus-4-6 (HEAD)\n\n",
            "Recommendations:\n\n",
            "- **#stale1** — fix stale-task parsing\n",
            "- **#envt1** — fix env test\n\n",
            "All marked [recommended].\n\n",
            "do #stale1\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
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
            vec!["do #stale1".to_string()],
            "should extract user's bare directive, not response list items"
        );
    }
    #[test]
    fn from_exchange_scopes_to_tail_list_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "❯ Old prompt\n\n",
            "### Re: old response — opus-4-6\n\n",
            "Old list:\n\n",
            "- old item 1\n",
            "- old item 2\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "❯ Old prompt\n\n",
            "### Re: old response — opus-4-6 (HEAD)\n\n",
            "Old list:\n\n",
            "- old item 1\n",
            "- old item 2\n\n",
            "- do #new1\n",
            "- do #new2\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
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
            vec!["do #new1".to_string(), "do #new2".to_string()],
            "should extract user's new list items, not old response list items"
        );
    }
    #[test]
    fn from_exchange_falls_back_without_snapshot() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "- do #task1\n",
            "- do #task2\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, content).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
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
            vec!["do #task1".to_string(), "do #task2".to_string()],
            "without snapshot should fall back to full exchange extraction"
        );
    }
    #[test]
    fn from_exchange_multiple_responses_picks_latest_directive() {
        let dir = TempDir::new().unwrap();
        setup_snapshot_dir(dir.path());
        let doc = dir.path().join("session.md");

        let snapshot_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted: old orchestration ran tasks #a, #b, #c.\n\n",
            "❯ What should we do next?\n\n",
            "### Re: next steps — opus-4-6\n\n",
            "I recommend:\n\n",
            "1. Fix stale-task parsing\n",
            "2. Fix env test\n",
            "3. Manual test orchestrate\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let current_content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "Compacted: old orchestration ran tasks #a, #b, #c.\n\n",
            "❯ What should we do next?\n\n",
            "### Re: next steps — opus-4-6 (HEAD)\n\n",
            "I recommend:\n\n",
            "1. Fix stale-task parsing\n",
            "2. Fix env test\n",
            "3. Manual test orchestrate\n\n",
            "do #stale1\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );

        fs::write(&doc, current_content).unwrap();
        agent_doc_snapshot_io::save(&doc, snapshot_content, agent_doc_ops_log_io::log_op).unwrap();

        let batch = resolve_task_batch(
            &doc,
            &OrchestrateConfig {
                mode: OrchestrateMode::Sequential,
                tasks_explicit: Vec::new(),
                from_file: None,
                from_exchange: true,
                from_queue: false,
                resume_schedule: None,
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
            vec!["do #stale1".to_string()],
            "should extract user's directive, not the numbered list from the response"
        );
    }
    #[test]
    fn build_agent_prompt_carries_forward_active_format_requirements() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization — gpt-5\n",
            "Done.\n",
        );

        let prompt = build_agent_prompt(
            Path::new("session.md"),
            ResolvedMode {
                format: agent_doc_frontmatter::frontmatter::AgentDocFormat::Template,
                write: agent_doc_frontmatter::frontmatter::AgentDocWrite::Crdt,
            },
            Some("diff"),
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
    fn build_agent_prompt_uses_bounded_context_pack_for_warn_level_prompt_targets() {
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

        let prompt = build_agent_prompt(
            Path::new("session.md"),
            ResolvedMode {
                format: agent_doc_frontmatter::frontmatter::AgentDocFormat::Template,
                write: agent_doc_frontmatter::frontmatter::AgentDocWrite::Crdt,
            },
            Some(diff),
            doc,
            Some(&report),
        );
        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(!prompt.contains("<document>\n## Exchange"));
    }
}
