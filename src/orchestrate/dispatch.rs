//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_template_io::enforce_imperative_response_contract_for_diff;
use agent_doc_turn_executor::claude_launch::structural_base_args as claude_structural_base_args;
use agent_doc_turn_executor::codex_launch::{
    CODEX_SANDBOX_NETWORK_DISABLED_ENV, apply_codex_network_access_env_overrides,
    codex_network_status_from_overrides, default_base_args as codex_default_base_args,
    resolve_codex_network_access, structural_base_args as codex_structural_base_args,
};

fn parent_codex_network_disabled() -> bool {
    std::env::var(CODEX_SANDBOX_NETWORK_DISABLED_ENV)
        .ok()
        .as_deref()
        == Some("1")
}

/// Build a dispatch context for command dispatch from a document file.
pub(crate) fn build_dispatch_context(file: &Path) -> queue_dispatch::DispatchContext {
    queue_dispatch::DispatchContext::from_file(file).unwrap_or_else(|_| {
        queue_dispatch::DispatchContext {
            file: file.to_path_buf(),
            project_root: None,
            session_uuid: None,
            pane_id: None,
            harness: "claude".to_string(),
        }
    })
}

/// Resolve frontmatter/config harness args using the same precedence as `start.rs`:
/// `fm.agent_args > fm.<harness>_args > config.agent_args > config.<harness>_args`
pub(crate) fn resolve_orchestrate_agent_args(
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
pub(crate) fn build_effective_agent_config(
    agent_name: &str,
    resolved_args: Option<&str>,
    global_config: &Config,
) -> Option<AgentConfig> {
    let global_agent_config = global_config.agents.get(agent_name);
    if let Some(args_str) = resolved_args {
        let mut args = match agent_name {
            "claude" => claude_structural_base_args(),
            "codex" => codex_structural_base_args(),
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

pub(crate) fn run_ordered_task_step(
    file: &Path,
    task: &str,
    options: OrderedTaskStepOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    close_open_preflight_handoff_cycle(file)?;
    inject_prompt(file, task)?;
    lifecycle.admit(file)?;
    let injected_diff = injected_prompt_diff(task);

    let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "orchestrate_dispatch_frontmatter",
    )?;
    let (fm, _) = frontmatter::parse(&doc)?;
    let mode = fm.resolve_mode();
    let agent_name = options
        .agent_override
        .or(fm.agent.as_deref())
        .or(global_config.default_agent.as_deref())
        .unwrap_or("claude");
    let harness = agent_doc_model_tier::harness_key_for_agent_name(agent_name);
    let resolved_model = options
        .model_override
        .or(fm.resolve_harness_model(&harness))
        .map(|m| agent_doc_model_tier::canonical_model_name(m, &harness, &global_config.model));
    let model = resolved_model.as_deref();
    let session_accretion = agent_doc_session_accretion_io::inspect(file).ok();
    let mut prompt = build_agent_prompt(
        file,
        mode,
        Some(&injected_diff),
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
        let codex_network_access = resolve_codex_network_access(
            fm.codex_network_access,
            global_config.codex_network_access,
        );
        apply_codex_network_access_env_overrides(&mut launch_env, codex_network_access);
        let sandbox_args = agent_config
            .map(|cfg| cfg.args.clone())
            .unwrap_or_else(codex_default_base_args);
        let status = codex_network_status_from_overrides(
            &sandbox_args,
            codex_network_access,
            parent_codex_network_disabled(),
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
        agent_doc_turn::response_text::strip_assistant_heading(&response)
    };
    let finalize_text = if mode.is_template() {
        let normalization =
            agent_doc_template::patchback::normalize_child_template_response(finalize_response);
        agent_doc_flow_io::log_flow_event(
            file,
            agent_doc_template::patchback::child_patchback_normalization_event(&normalization),
            agent_doc_ops_log_io::log_op,
        );
        normalization.response
    } else {
        agent_doc_turn::response_text::strip_assistant_heading(&finalize_response)
    };

    enforce_imperative_response_contract_for_diff(file, &injected_diff, &response_text)?;

    let finalize_text = if let Some(worker_result_line) =
        options.graph_evidence.and_then(|evidence| {
            evidence.worker_result_line_for_task(options.task_label, &response_text)
        }) {
        append_worker_result_line(&finalize_text, &worker_result_line, mode)
    } else {
        finalize_text
    };

    lifecycle.finalize(file, &finalize_text, mode)?;
    lifecycle.session_check(file)?;
    Ok(())
}

fn injected_prompt_diff(task: &str) -> String {
    let task = super::normalize_task(task);
    let mut diff = String::from("--- snapshot\n+++ document\n");
    for (idx, line) in task.lines().enumerate() {
        if idx == 0 {
            diff.push_str("+❯ ");
        } else {
            diff.push('+');
        }
        diff.push_str(line);
        diff.push('\n');
    }
    diff
}

pub(crate) fn close_open_preflight_handoff_cycle(file: &Path) -> Result<()> {
    let Some(state) = agent_doc_cycle_state_io::load_with_closeout_projection(file)? else {
        return Ok(());
    };
    if state.phase != agent_doc_turn::CyclePhase::PreflightStarted {
        return Ok(());
    }
    if preflight_handoff_cycle_has_capture(file, &state)? {
        return Ok(());
    }

    eprintln!(
        "[orchestrate] closing preflight handoff cycle {} before task injection",
        state.cycle_id
    );
    let file_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "orchestrate_preflight_handoff_close",
    )?;
    let snapshot_content = agent_doc_snapshot_io::load_document_baseline(file)?;
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &file_content,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_cycle_state_io::mark_abandoned(
        file,
        "orchestrate_preflight_handoff_closed",
        snapshot_content.as_deref(),
        Some(&file_content),
    )?;
    Ok(())
}

fn preflight_handoff_cycle_has_capture(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> Result<bool> {
    if let Some(capture_id) = state.capture_id.as_deref()
        && let Some(projected) =
            agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
        && projected.cycle_id == state.cycle_id
        && state
            .response_sha256
            .as_deref()
            .is_none_or(|sha| sha == projected.response_sha256)
    {
        return Ok(true);
    }
    Ok(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExchangeStreamSeed {
    prefix: String,
    suffix: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StreamStepResult {
    full_response: String,
    finalize_response: String,
}

pub(crate) fn exchange_stream_seed(doc: &str) -> Result<Option<ExchangeStreamSeed>> {
    let components = element::parse(doc).context("failed to parse document components")?;
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

    let boundary =
        agent_doc_element::id::format_boundary_marker(&agent_doc_element::id::new_boundary_id());
    Ok(Some(ExchangeStreamSeed {
        prefix: content.to_string(),
        suffix: format!("{boundary}\n"),
    }))
}

#[cfg(test)]
pub(crate) fn render_streamed_exchange(seed: &ExchangeStreamSeed, response: &str) -> String {
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

pub(crate) fn stream_step_response(
    file: &Path,
    _seed: &ExchangeStreamSeed,
    chunks: Box<dyn Iterator<Item = Result<StreamChunk>>>,
) -> Result<StreamStepResult> {
    let mut response = String::new();
    let mut checkpoint_writer = agent_doc_capture_io::PartialCheckpointWriter::new(file);

    for chunk_result in chunks {
        let chunk = chunk_result.context("stream chunk error")?;
        if !chunk.text.is_empty() {
            response = chunk.text;
            if !chunk.is_final {
                let current_content =
                    agent_doc_document_realtime_io::try_resolve_current_document_content(
                        file,
                        "orchestrate_partial_response_checkpoint",
                    )?;
                checkpoint_writer
                    .maybe_checkpoint_with_current_content(&response, &current_content)?;
            }
        }
        if chunk.is_final {
            break;
        }
    }

    if response.trim().is_empty() {
        anyhow::bail!("empty response from streaming orchestrate step");
    }

    Ok(StreamStepResult {
        full_response: response.clone(),
        finalize_response: response,
    })
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::cell::RefCell;
    use tempfile::TempDir;
    #[test]
    fn close_open_preflight_handoff_cycle_snapshots_before_injection() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        Command::new("git")
            .current_dir(dir.path())
            .arg("init")
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let snapshot = template_doc();
        fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(dir.path())
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let handoff = snapshot.replace(
            "<!-- agent:boundary:keep -->",
            "synchronous orchestra\npreset #spec-test\n- do #first\n<!-- agent:boundary:keep -->",
        );
        fs::write(&doc, &handoff).unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&handoff)).unwrap();

        close_open_preflight_handoff_cycle(&doc).unwrap();
        inject_prompt(&doc, "do #first").unwrap();

        let snap = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        let live = fs::read_to_string(&doc).unwrap();
        assert!(snap.contains("synchronous orchestra"));
        assert!(!snap.contains("❯ do #first"));
        assert!(live.contains("❯ do #first"));
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::Abandoned
        );
    }

    #[test]
    fn close_open_preflight_handoff_cycle_preserves_captured_ledger_state() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = template_doc();
        fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let started =
            agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&snapshot))
                .unwrap();
        let capture = agent_doc_capture_io::capture_response(
            &doc,
            "<!-- patch:exchange -->\n### Re: do #first — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n",
        )
        .unwrap();

        assert!(!capture.capture_id.is_empty());

        close_open_preflight_handoff_cycle(&doc).unwrap();

        assert_eq!(
            agent_doc_cycle_state_io::load_with_closeout_projection(&doc)
                .unwrap()
                .unwrap()
                .phase,
            agent_doc_turn::CyclePhase::ResponseCaptured
        );
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc).unwrap().unwrap().phase,
            agent_doc_turn::CyclePhase::ResponseCaptured,
            "the ledger capture must prevent abandonment"
        );
        assert_eq!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .cycle_id,
            started.cycle_id
        );
    }
    #[test]
    fn injected_prompt_diff_preserves_multiline_task_as_prompt_bearing_diff() {
        let diff = injected_prompt_diff("(preset #1)\nKeep the work tree clean.\ndo #prep");

        assert!(diff.contains("+❯ (preset #1)\n"));
        assert!(diff.contains("+Keep the work tree clean.\n"));
        assert!(diff.contains("+do #prep\n"));
        assert_eq!(
            agent_doc_diff::extract_imperative_directives(&diff),
            vec!["do #prep".to_string()]
        );
        assert!(agent_doc_diff::format_prompt_bearing_changes(&diff).is_some());
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
    fn resolve_orchestrate_agent_args_claude_frontmatter() {
        let fm = frontmatter::Frontmatter {
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--dangerously-skip-permissions"));
    }
    #[test]
    fn resolve_orchestrate_agent_args_codex_frontmatter() {
        let fm = frontmatter::Frontmatter {
            codex_args: Some("-s danger-full-access".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "codex", &config);
        assert_eq!(result.as_deref(), Some("-s danger-full-access"));
    }
    #[test]
    fn resolve_orchestrate_agent_args_opencode_frontmatter() {
        let fm = frontmatter::Frontmatter {
            opencode_args: Some("--dangerously-skip-permissions".into()),
            codex_args: Some("-s danger-full-access".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "opencode", &config);
        assert_eq!(result.as_deref(), Some("--dangerously-skip-permissions"));
    }
    #[test]
    fn resolve_orchestrate_agent_args_agent_args_beats_harness_specific() {
        let fm = frontmatter::Frontmatter {
            agent_args: Some("--model sonnet".into()),
            claude_args: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--model sonnet"));
    }
    #[test]
    fn resolve_orchestrate_agent_args_falls_through_to_config() {
        let fm = frontmatter::Frontmatter::default();
        let config = Config {
            claude_args: Some("--from-config".into()),
            ..Default::default()
        };
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert_eq!(result.as_deref(), Some("--from-config"));
    }
    #[test]
    fn resolve_orchestrate_agent_args_none_when_no_args() {
        let fm = frontmatter::Frontmatter::default();
        let config = Config::default();
        let result = resolve_orchestrate_agent_args(&fm, "claude", &config);
        assert!(result.is_none());
    }
    #[test]
    fn build_effective_config_claude_with_frontmatter_args() {
        let config = Config::default();
        let effective =
            build_effective_agent_config("claude", Some("--dangerously-skip-permissions"), &config);
        let effective = effective.unwrap();
        assert_eq!(effective.command, "claude");
        assert_eq!(
            effective.args,
            vec![
                "-p",
                "--output-format",
                "json",
                "--dangerously-skip-permissions"
            ]
        );
    }
    #[test]
    fn build_effective_config_codex_with_frontmatter_args() {
        let config = Config::default();
        let effective =
            build_effective_agent_config("codex", Some("-s danger-full-access"), &config);
        let effective = effective.unwrap();
        assert_eq!(effective.command, "codex");
        assert_eq!(
            effective.args,
            vec!["exec", "--json", "-s", "danger-full-access"]
        );
    }
    #[test]
    fn build_effective_config_none_without_frontmatter_args() {
        let config = Config::default();
        let effective = build_effective_agent_config("claude", None, &config);
        assert!(effective.is_none());
    }
}
