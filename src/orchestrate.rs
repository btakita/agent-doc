//! # Module: orchestrate
//!
//! ## Spec
//! - `run(file, config)`: resolves orchestration tasks from `--task`,
//!   `--from-file`, and/or `--from-exchange`, then dispatches by
//!   `OrchestrateMode`.
//! - `--mode sequential` injects each task into the document exchange as a
//!   fresh prompt, runs `preflight`, sends one fresh agent request with no
//!   resume/session reuse, then persists the response through `finalize`
//!   followed by `session-check`.
//! - `--mode parallel` reuses the existing `parallel` worktree fan-out path
//!   after task resolution, and the legacy `agent-doc parallel` command routes
//!   through this same orchestrate dispatch surface.
//! - `--mode dag` is reserved for future work and currently errors.
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
//! - `finalize` / `session-check` are the persistence boundary for each step;
//!   if either fails, orchestration stops immediately.
//! - Task resolution preserves source order.
//!
//! ## Evals
//! - `extract_tasks_prefers_last_fenced_list`
//! - `extract_tasks_uses_last_markdown_list`
//! - `inject_prompt_inserts_before_boundary`
//! - `send_fresh_response_uses_no_resume`
//! - `sequential_orchestration_injects_prompt_and_finalizes`

use anyhow::{Context, Result};
use clap::ValueEnum;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    agent, component,
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
        prompt: &str,
        agent_name: &str,
        agent_config: Option<&AgentConfig>,
        env: Vec<(String, Option<String>)>,
        model: Option<&str>,
    ) -> Result<String>;
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
        prompt: &str,
        agent_name: &str,
        agent_config: Option<&AgentConfig>,
        env: Vec<(String, Option<String>)>,
        model: Option<&str>,
    ) -> Result<String> {
        let backend = agent::resolve(agent_name, agent_config, env)?;
        let response = send_fresh_response(backend.as_ref(), prompt, model)?;
        Ok(response.text)
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
            tasks_explicit: config.tasks,
            from_file: None,
            from_exchange: false,
            agent: None,
            model: config.model,
            no_git: config.no_git,
            no_worktree: config.no_worktree,
            timeout_secs: config.timeout_secs,
            dry_run: config.dry_run,
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

    let tasks = resolve_tasks(file, &config)?;
    if tasks.is_empty() && !(allow_empty_parallel_tasks && config.mode == OrchestrateMode::Parallel)
    {
        anyhow::bail!("no orchestration tasks found");
    }

    eprintln!(
        "[orchestrate] mode: {}",
        config.mode.to_possible_value().unwrap().get_name()
    );
    eprintln!("[orchestrate] tasks: {}", tasks.len());
    for (idx, task) in tasks.iter().enumerate() {
        eprintln!("[orchestrate]   {}: {}", idx + 1, task);
    }

    match config.mode {
        OrchestrateMode::Sequential => {
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            if config.no_git {
                anyhow::bail!(
                    "`agent-doc orchestrate --mode sequential` requires git-backed finalize"
                );
            }
            run_sequential_internal(
                file,
                &tasks,
                config.agent.as_deref(),
                config.model.as_deref(),
                global_config,
                lifecycle,
                agent_runner,
            )
        }
        OrchestrateMode::Parallel => parallel_runner.run(
            file,
            parallel::ParallelConfig {
                tasks,
                model: config.model,
                no_git: config.no_git,
                no_worktree: config.no_worktree,
                timeout_secs: config.timeout_secs,
                dry_run: config.dry_run,
            },
        ),
        OrchestrateMode::Dag => {
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            anyhow::bail!("`agent-doc orchestrate --mode dag` is not implemented yet")
        }
    }
}

fn run_sequential_internal(
    file: &Path,
    tasks: &[String],
    agent_override: Option<&str>,
    model_override: Option<&str>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    for (idx, task) in tasks.iter().enumerate() {
        eprintln!("[orchestrate] step {}/{}: {}", idx + 1, tasks.len(), task);
        inject_prompt(file, task)?;

        let preflight = lifecycle.preflight(file)?;
        if preflight.no_changes {
            anyhow::bail!(
                "orchestration step {} did not produce a prompt-bearing diff after injection",
                idx + 1
            );
        }

        let doc = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let (fm, _) = frontmatter::parse(&doc)?;
        let mode = fm.resolve_mode();
        let agent_name = agent_override
            .or(fm.agent.as_deref())
            .or(global_config.default_agent.as_deref())
            .unwrap_or("claude");
        let model = model_override.or(fm.model.as_deref());
        let prompt = build_agent_prompt(mode, preflight.diff.as_deref(), &doc);
        let expanded_env = expand_frontmatter_env(&fm);
        let response = agent_runner.send_fresh(
            &prompt,
            agent_name,
            global_config.agents.get(agent_name),
            expanded_env,
            model,
        )?;
        let response_text = if mode.is_template() {
            response
        } else {
            write::strip_assistant_heading(&response)
        };

        if let Some(diff_text) = preflight.diff.as_deref() {
            write::enforce_imperative_response_contract_for_diff(file, diff_text, &response_text)?;
        }

        lifecycle.finalize(
            file,
            preflight.baseline_file.as_deref(),
            &response_text,
            mode,
        )?;
        lifecycle.session_check(file)?;
    }
    Ok(())
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

fn resolve_tasks(file: &Path, config: &OrchestrateConfig) -> Result<Vec<String>> {
    let mut tasks = Vec::new();
    tasks.extend(
        config
            .tasks_explicit
            .iter()
            .map(|task| normalize_task(task))
            .filter(|task| !task.is_empty()),
    );

    if let Some(path) = &config.from_file {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read task file {}", path.display()))?;
        tasks.extend(extract_tasks_from_text(&text));
    }

    if config.from_exchange {
        let doc = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        tasks.extend(extract_tasks_from_exchange(&doc)?);
    }

    Ok(tasks)
}

fn extract_tasks_from_exchange(doc: &str) -> Result<Vec<String>> {
    let components = component::parse(doc).context("failed to parse document components")?;
    let exchange = components
        .iter()
        .find(|comp| comp.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no `agent:exchange` component"))?;
    Ok(extract_tasks_from_text(exchange.content(doc)))
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
        finalize_calls: RefCell<Vec<String>>,
        session_checks: RefCell<usize>,
    }

    impl LifecycleOps for FakeLifecycleOps {
        fn preflight(&self, file: &Path) -> Result<PreflightOutput> {
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
        response: String,
    }

    impl FreshAgentRunner for FakeAgentRunner {
        fn send_fresh(
            &self,
            prompt: &str,
            _agent_name: &str,
            _agent_config: Option<&AgentConfig>,
            _env: Vec<(String, Option<String>)>,
            _model: Option<&str>,
        ) -> Result<String> {
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(self.response.clone())
        }
    }

    struct CaptureAgent {
        seen_prompt: RefCell<Vec<String>>,
        seen_session_id: RefCell<Vec<Option<String>>>,
        seen_fork: RefCell<Vec<bool>>,
    }

    #[derive(Default)]
    struct FakeParallelRunner {
        calls: RefCell<Vec<(String, Vec<String>, Option<String>, bool, bool, u64, bool)>>,
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
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            response:
                "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                    .to_string(),
        };

        run_sequential_internal(
            &doc,
            &["do #gkke".to_string()],
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
    }

    #[test]
    fn parallel_mode_uses_shared_parallel_runner() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            response: "unused".to_string(),
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
        assert_eq!(calls[0].1, vec!["do #9pw9".to_string()]);
        assert_eq!(calls[0].2.as_deref(), Some("gpt-5"));
        assert!(calls[0].3);
        assert!(calls[0].4);
        assert_eq!(calls[0].5, 45);
        assert!(calls[0].6);
        assert!(lifecycle.finalize_calls.borrow().is_empty());
        assert!(agent.prompts.borrow().is_empty());
    }

    #[test]
    fn legacy_parallel_compat_allows_empty_task_list() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("session.md");
        fs::write(&doc, template_doc()).unwrap();

        let lifecycle = FakeLifecycleOps {
            baseline_file: "unused".to_string(),
            finalize_calls: RefCell::new(Vec::new()),
            session_checks: RefCell::new(0),
        };
        let agent = FakeAgentRunner {
            prompts: RefCell::new(Vec::new()),
            response: "unused".to_string(),
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
}
