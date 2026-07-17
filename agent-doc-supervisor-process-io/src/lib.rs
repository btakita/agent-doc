//! Supervisor process observer adapters.

use agent_doc_frontmatter::frontmatter;
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor_io::detection::{
    SupervisorDetectionState, current_child_prompt_visible,
    normalize_stdin_for_harness_permission_prompt, prompt_visible_requires_ready_transition,
    record_recent_output, record_terminal_screen,
};
use agent_doc_supervisor_process::{
    REEXEC_CAPABILITY_PROOF_CONTRACT_ENV, REEXEC_CHILD_PID_ENV, REEXEC_MASTER_FD_ENV,
    io_threads::{PtyReaderObserver, StdinForwardObserver},
};
use agent_doc_turn_executor::codex_launch::{
    CODEX_SANDBOX_NETWORK_DISABLED_ENV, apply_codex_network_access_env_map,
    codex_network_status_from_env_map, resolve_codex_network_access,
};
use anyhow::{Context, Result};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

/// Log callbacks supplied by the concrete supervisor start host.
pub trait SupervisorLaunchLog {
    fn log_event(&mut self, msg: &str);
    fn start_console_status(&mut self, message: &str);
}

/// Resolved harness launch inputs for one supervisor child iteration.
///
/// Built from the current document frontmatter plus global configuration. The
/// supervisor restart loop reuses this shape so a changed `agent:` can force a
/// fresh child spawn with the new harness while unchanged frontmatter remains
/// byte-identical.
#[derive(Clone, Debug)]
pub struct HarnessLaunchSpec {
    pub harness: HarnessConfig,
    pub base_args: Vec<String>,
    pub resolved_env: HashMap<String, String>,
    pub capability_proof_required: bool,
}

#[cfg(unix)]
pub struct SupervisorStderrRedirect {
    saved_stderr: Option<OwnedFd>,
}

pub fn supervisor_stderr_redirect_needed(harness: &HarnessConfig, route_owned: bool) -> bool {
    route_owned && harness.is_tui_harness()
}

pub fn supervisor_stderr_redirect_path(project_root: &Path) -> PathBuf {
    agent_doc_supervisor_process::start_command::route_owned_stderr_log_path(project_root)
}

#[cfg(unix)]
impl SupervisorStderrRedirect {
    pub fn inactive() -> Self {
        Self { saved_stderr: None }
    }

    pub fn maybe_start(
        project_root: &Path,
        harness: &HarnessConfig,
        route_owned: bool,
        log: &mut dyn SupervisorLaunchLog,
    ) -> Self {
        if !supervisor_stderr_redirect_needed(harness, route_owned) {
            return Self::inactive();
        }
        match Self::start(project_root, harness, log) {
            Ok(guard) => guard,
            Err(err) => {
                log.log_event(&format!(
                    "supervisor_stderr_redirect_failed harness={} error={:?}",
                    harness.binary,
                    err.to_string()
                ));
                eprintln!(
                    "[start] warning: could not redirect supervisor stderr for {} TUI: {err:#}",
                    harness.binary
                );
                Self::inactive()
            }
        }
    }

    pub fn start(
        project_root: &Path,
        harness: &HarnessConfig,
        log: &mut dyn SupervisorLaunchLog,
    ) -> Result<Self> {
        let stderr_path = supervisor_stderr_redirect_path(project_root);
        let logs_dir = stderr_path
            .parent()
            .context("supervisor stderr path must include logs directory")?;
        std::fs::create_dir_all(logs_dir)
            .with_context(|| format!("failed to create {}", logs_dir.display()))?;
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&stderr_path)
            .with_context(|| format!("failed to open {}", stderr_path.display()))?;
        let saved_fd = agent_doc_supervisor_process::pty::dup_cloexec(libc::STDERR_FILENO)
            .map_err(|e| anyhow::anyhow!("dup(stderr) failed: {e}"))?;
        let saved_stderr = unsafe { OwnedFd::from_raw_fd(saved_fd) };
        let redirected = unsafe { libc::dup2(log_file.as_raw_fd(), libc::STDERR_FILENO) };
        if redirected < 0 {
            anyhow::bail!("dup2(stderr) failed: {}", std::io::Error::last_os_error());
        }
        log.log_event(&format!(
            "supervisor_stderr_redirect harness={} target={}",
            harness.binary,
            stderr_path.display()
        ));
        eprintln!(
            "[start] stderr redirected to {} for {} route-owned TUI",
            stderr_path.display(),
            harness.binary
        );
        Ok(Self {
            saved_stderr: Some(saved_stderr),
        })
    }
}

#[cfg(unix)]
impl Drop for SupervisorStderrRedirect {
    fn drop(&mut self) {
        let Some(saved_stderr) = self.saved_stderr.take() else {
            return;
        };
        let restored = unsafe { libc::dup2(saved_stderr.as_raw_fd(), libc::STDERR_FILENO) };
        if restored < 0 {
            let msg = b"[start] warning: failed to restore stderr after supervisor redirect\n";
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
pub struct SupervisorStderrRedirect;

#[cfg(not(unix))]
impl SupervisorStderrRedirect {
    pub fn inactive() -> Self {
        Self
    }

    pub fn maybe_start(
        _project_root: &Path,
        _harness: &HarnessConfig,
        _route_owned: bool,
        _log: &mut dyn SupervisorLaunchLog,
    ) -> Self {
        Self
    }
}

/// Assemble the harness launch spec from current frontmatter + global config.
pub fn build_harness_launch_spec(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    canonical: &Path,
    log: &mut dyn SupervisorLaunchLog,
) -> Result<HarnessLaunchSpec> {
    let harness = HarnessConfig::from_context(fm, global_config);
    let resolved_agent_args = agent_doc_supervisor::config::resolve_agent_launch_args(
        &harness.binary,
        agent_launch_args_sources(fm, global_config),
    );

    let env_spec = agent_doc_supervisor_io::env::EnvSpec::from_frontmatter(fm);
    let mut resolved_env = env_spec.resolve()?;
    strip_reexec_handoff_env(&mut resolved_env);
    if harness.supports_enable_tool_search && fm.enable_tool_search.unwrap_or(false) {
        resolved_env.insert("ENABLE_TOOL_SEARCH".into(), "true".into());
    }

    let mut base_args: Vec<String> = Vec::new();
    if let Some(ref args) = resolved_agent_args {
        base_args.extend(args.split_whitespace().map(String::from));
    }
    if !base_args.iter().any(|a| a == "--model") {
        let harness_key = agent_doc_model_tier::harness_key_for_agent_name(&harness.binary);
        if let Some(model) = fm.resolve_harness_model(&harness_key) {
            base_args.push("--model".into());
            base_args.push(agent_doc_model_tier::canonical_model_name(
                model,
                &harness_key,
                &global_config.model,
            ));
        }
    }
    agent_doc_git_io::dirs::append_workspace_access_args(
        &harness.binary,
        &mut base_args,
        canonical,
    );
    if harness.supports_no_mcp && fm.no_mcp.unwrap_or(false) {
        base_args.push("--no-mcp".into());
    }
    if harness.binary == "codex" {
        let codex_network_access = resolve_codex_network_access(
            fm.codex_network_access,
            global_config.codex_network_access,
        );
        apply_codex_network_access_env_map(&mut resolved_env, codex_network_access);
        let status = codex_network_status_from_env_map(
            &base_args,
            codex_network_access,
            parent_codex_network_disabled(),
            &resolved_env,
        );
        log.start_console_status(&format!(
            "[start] codex network access: {}",
            status.summary()
        ));
        if let Some(err) = status.mismatch_error() {
            anyhow::bail!(err);
        }
    }
    let capability_proof_required =
        agent_doc_harness::managed_capability::managed_capability_contract_required(
            &base_args,
            fm,
            global_config,
            &harness.binary,
        );
    if !capability_proof_required {
        log.log_event(&format!(
            "{}_capability_proof status=not_required",
            harness.binary
        ));
    }

    Ok(HarnessLaunchSpec {
        harness,
        base_args,
        resolved_env,
        capability_proof_required,
    })
}

/// Supervisor hot-reexec handoff facts belong to the replacement process, not
/// to the managed harness child or its exact capability-proof contract.
fn strip_reexec_handoff_env(env: &mut HashMap<String, String>) {
    for key in [
        REEXEC_CHILD_PID_ENV,
        REEXEC_MASTER_FD_ENV,
        REEXEC_CAPABILITY_PROOF_CONTRACT_ENV,
    ] {
        env.remove(key);
    }
}

pub fn parent_codex_network_disabled() -> bool {
    std::env::var(CODEX_SANDBOX_NETWORK_DISABLED_ENV)
        .ok()
        .as_deref()
        == Some("1")
}

fn agent_launch_args_sources(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
) -> agent_doc_supervisor::config::AgentLaunchArgsSources {
    agent_doc_supervisor::config::AgentLaunchArgsSources {
        frontmatter_agent_args: fm.agent_args.clone(),
        frontmatter_claude_args: fm.claude_args.clone(),
        frontmatter_codex_args: fm.codex_args.clone(),
        frontmatter_opencode_args: fm.opencode_args.clone(),
        config_agent_args: global_config.agent_args.clone(),
        config_claude_args: global_config.claude_args.clone(),
        config_codex_args: global_config.codex_args.clone(),
        config_opencode_args: global_config.opencode_args.clone(),
        env_claude_args: std::env::var("AGENT_DOC_CLAUDE_ARGS").ok(),
    }
}

pub trait SupervisorProcessIoState: SupervisorDetectionState + Send + Sync + 'static {
    fn transition_actor_ready_for_prompt(&self);
    fn clear_suppress_stale_ctrl_d_until_prompt(&self);
    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool;
    fn prompt_visible_once(&self) -> bool;
}

impl<T> SupervisorProcessIoState for std::sync::Arc<T>
where
    T: SupervisorProcessIoState + ?Sized,
{
    fn transition_actor_ready_for_prompt(&self) {
        self.as_ref().transition_actor_ready_for_prompt();
    }

    fn clear_suppress_stale_ctrl_d_until_prompt(&self) {
        self.as_ref().clear_suppress_stale_ctrl_d_until_prompt();
    }

    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
        self.as_ref().suppress_stale_ctrl_d_until_prompt()
    }

    fn prompt_visible_once(&self) -> bool {
        self.as_ref().prompt_visible_once()
    }
}

pub struct SupervisorProcessIoObserver<S> {
    state: S,
}

impl<S> SupervisorProcessIoObserver<S> {
    pub fn new(state: S) -> Self {
        Self { state }
    }
}

impl<S> PtyReaderObserver for SupervisorProcessIoObserver<S>
where
    S: SupervisorProcessIoState,
{
    fn on_filtered_pty_output(&self, harness: &HarnessConfig, bytes: &[u8]) {
        observe_filtered_pty_output(&self.state, harness, bytes);
    }
}

impl<S> StdinForwardObserver for SupervisorProcessIoObserver<S>
where
    S: SupervisorProcessIoState,
{
    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
        self.state.suppress_stale_ctrl_d_until_prompt()
    }

    fn prompt_visible_once(&self) -> bool {
        self.state.prompt_visible_once()
    }

    fn normalize_permission_prompt_input(
        &self,
        harness: &HarnessConfig,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        normalize_stdin_for_harness_permission_prompt(&self.state, harness, data)
    }
}

pub fn observe_filtered_pty_output<S>(state: &S, harness: &HarnessConfig, bytes: &[u8])
where
    S: SupervisorProcessIoState + ?Sized,
{
    record_terminal_screen(state, bytes);
    record_recent_output(state, bytes);
    if current_child_prompt_visible(state, harness) {
        if prompt_visible_requires_ready_transition(state) {
            state.transition_actor_ready_for_prompt();
        }
        state.clear_suppress_stale_ctrl_d_until_prompt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reexec_handoff_env_is_not_part_of_the_child_launch_contract() {
        let mut env = HashMap::from([
            (REEXEC_CHILD_PID_ENV.to_string(), "101".to_string()),
            (REEXEC_MASTER_FD_ENV.to_string(), "42".to_string()),
            (
                REEXEC_CAPABILITY_PROOF_CONTRACT_ENV.to_string(),
                "preserved".to_string(),
            ),
            ("STABLE_CHILD_ENV".to_string(), "kept".to_string()),
        ]);

        strip_reexec_handoff_env(&mut env);

        assert_eq!(env.len(), 1);
        assert_eq!(
            env.get("STABLE_CHILD_ENV").map(String::as_str),
            Some("kept")
        );
    }
}
