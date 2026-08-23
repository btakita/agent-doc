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
use std::io::Write;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

const DEFAULT_SUPERVISOR_STDERR_LOG: &str = ".agent-doc/logs/supervisor-stderr.log";
const FALLBACK_SUPERVISOR_LOG_DIR: &str = "agent-doc/supervisor-logs";

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
    /// Fresh, document-scoped launch args before any conversation resume shape.
    pub fresh_base_args: Vec<String>,
    /// Effective args for this launch, including an exact resume when requested.
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

#[derive(Debug)]
pub struct SupervisorStderrLog {
    file: std::fs::File,
    path: PathBuf,
    fallback_from: Option<PathBuf>,
}

impl SupervisorStderrLog {
    pub fn file(&self) -> &std::fs::File {
        &self.file
    }

    pub fn into_file(self) -> std::fs::File {
        self.file
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn fallback_from(&self) -> Option<&Path> {
        self.fallback_from.as_deref()
    }
}

fn configured_supervisor_stderr_log(project_root: &Path) -> (Option<String>, Option<String>) {
    let config_path = project_root.join(".agent-doc").join("config.toml");
    if !config_path.exists() {
        return (None, None);
    }
    let content = match std::fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(err) => {
            return (
                None,
                Some(format!(
                    "failed to read supervisor stderr configuration {}: {err}",
                    config_path.display()
                )),
            );
        }
    };
    match agent_doc_frontmatter::project_config::parse_project_toml(&content) {
        Ok(config) => (config.agent_doc_supervisor_stderr_log, None),
        Err(err) => (
            None,
            Some(format!(
                "failed to parse supervisor stderr configuration {}: {err}",
                config_path.display()
            )),
        ),
    }
}

fn resolve_supervisor_stderr_log_path(project_root: &Path, configured: Option<&str>) -> PathBuf {
    let configured = configured.map(str::trim).filter(|path| !path.is_empty());
    let path = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SUPERVISOR_STDERR_LOG));
    if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    }
}

pub fn supervisor_stderr_log_path(project_root: &Path) -> Result<PathBuf> {
    let (configured, warning) = configured_supervisor_stderr_log(project_root);
    if let Some(warning) = warning {
        anyhow::bail!(warning);
    }
    Ok(resolve_supervisor_stderr_log_path(
        project_root,
        configured.as_deref(),
    ))
}

pub fn supervisor_stderr_fallback_path(project_root: &Path) -> PathBuf {
    let stable_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let hash = agent_doc_hash::path_string_hash(&stable_root.to_string_lossy());
    std::env::temp_dir()
        .join(FALLBACK_SUPERVISOR_LOG_DIR)
        .join(&hash[..16])
        .join("supervisor-stderr.log")
}

fn open_append_log(path: &Path) -> Result<std::fs::File> {
    let parent = path
        .parent()
        .context("supervisor stderr path must include a parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))
}

pub fn open_supervisor_stderr_log(project_root: &Path) -> Result<SupervisorStderrLog> {
    let (configured, config_warning) = configured_supervisor_stderr_log(project_root);
    let primary_path = resolve_supervisor_stderr_log_path(project_root, configured.as_deref());
    let (mut file, path, fallback_from, primary_warning) = match open_append_log(&primary_path) {
        Ok(file) => (file, primary_path, None, None),
        Err(primary_error) => {
            let fallback_path = supervisor_stderr_fallback_path(project_root);
            let file = open_append_log(&fallback_path).with_context(|| {
                format!(
                    "primary supervisor stderr log {} was unavailable ({primary_error:#}); fallback {} also failed",
                    primary_path.display(),
                    fallback_path.display()
                )
            })?;
            (
                file,
                fallback_path,
                Some(primary_path.clone()),
                Some(format!(
                    "primary supervisor stderr log {} was unavailable: {primary_error:#}; using deterministic fallback",
                    primary_path.display()
                )),
            )
        }
    };
    for warning in [config_warning.as_deref(), primary_warning.as_deref()]
        .into_iter()
        .flatten()
    {
        writeln!(file, "[start] warning: {warning}")
            .with_context(|| format!("failed to write startup warning to {}", path.display()))?;
    }
    Ok(SupervisorStderrLog {
        file,
        path,
        fallback_from,
    })
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
        let stderr_log = match open_supervisor_stderr_log(project_root) {
            Ok(stderr_log) => stderr_log,
            Err(err) => {
                log.log_event(&format!(
                    "supervisor_stderr_log_open_failed harness={} error={:?}",
                    harness.binary,
                    err.to_string()
                ));
                eprintln!(
                    "[start] warning: could not open supervisor stderr log for {}: {err:#}",
                    harness.binary
                );
                return Self::inactive();
            }
        };
        if !supervisor_stderr_redirect_needed(harness, route_owned) {
            let fallback = stderr_log
                .fallback_from()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "none".to_string());
            log.log_event(&format!(
                "supervisor_stderr_log_ready harness={} target={} fallback_from={}",
                harness.binary,
                stderr_log.path().display(),
                fallback
            ));
            return Self::inactive();
        }
        match Self::start_opened(stderr_log, harness, log) {
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
        let stderr_log = open_supervisor_stderr_log(project_root)?;
        Self::start_opened(stderr_log, harness, log)
    }

    fn start_opened(
        stderr_log: SupervisorStderrLog,
        harness: &HarnessConfig,
        log: &mut dyn SupervisorLaunchLog,
    ) -> Result<Self> {
        let saved_fd = agent_doc_supervisor_process::pty::dup_cloexec(libc::STDERR_FILENO)
            .map_err(|e| anyhow::anyhow!("dup(stderr) failed: {e}"))?;
        let saved_stderr = unsafe { OwnedFd::from_raw_fd(saved_fd) };
        let redirected = unsafe { libc::dup2(stderr_log.file().as_raw_fd(), libc::STDERR_FILENO) };
        if redirected < 0 {
            anyhow::bail!("dup2(stderr) failed: {}", std::io::Error::last_os_error());
        }
        let fallback = stderr_log
            .fallback_from()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_string());
        log.log_event(&format!(
            "supervisor_stderr_redirect harness={} target={} fallback_from={}",
            harness.binary,
            stderr_log.path().display(),
            fallback
        ));
        eprintln!(
            "[start] stderr redirected to {} for {} route-owned TUI",
            stderr_log.path().display(),
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
        project_root: &Path,
        harness: &HarnessConfig,
        _route_owned: bool,
        log: &mut dyn SupervisorLaunchLog,
    ) -> Self {
        match open_supervisor_stderr_log(project_root) {
            Ok(stderr_log) => {
                let fallback = stderr_log
                    .fallback_from()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "none".to_string());
                log.log_event(&format!(
                    "supervisor_stderr_log_ready harness={} target={} fallback_from={}",
                    harness.binary,
                    stderr_log.path().display(),
                    fallback
                ));
            }
            Err(err) => {
                log.log_event(&format!(
                    "supervisor_stderr_log_open_failed harness={} error={:?}",
                    harness.binary,
                    err.to_string()
                ));
                eprintln!(
                    "[start] warning: could not open supervisor stderr log for {}: {err:#}",
                    harness.binary
                );
            }
        }
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
    build_harness_launch_spec_with_resume(fm, global_config, canonical, log, None)
}

/// [`build_harness_launch_spec`] plus an `agent-doc start --resume` request.
pub fn build_harness_launch_spec_with_resume(
    fm: &frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
    canonical: &Path,
    log: &mut dyn SupervisorLaunchLog,
    resume: Option<&agent_doc_harness::ResumeRequest>,
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
    let fresh_base_args = base_args.clone();

    // Resume only an exact document-bound conversation. In particular, Codex
    // resume args must be derived from the complete fresh arg set so sandbox
    // flags are translated and unsupported `--add-dir` flags are removed.
    let resolved_resume =
        agent_doc_harness::resolve_resume_request(resume, fm.resume_for_harness(&harness.binary));
    if let Some(agent_doc_harness::ResumeRequest::Id(id)) = resolved_resume.as_ref() {
        match harness.exact_resume_args(&fresh_base_args, id)? {
            Some(resume_args) => {
                base_args = resume_args;
                log.log_event(&format!("{}_start_resume id={id}", harness.binary));
            }
            None => {
                let degrade = agent_doc_harness::ResumeDegrade::HarnessUnsupported;
                log.start_console_status(&format!(
                    "[start] --resume requested but starting a FRESH {} conversation: {}",
                    harness.binary,
                    degrade.reason()
                ));
                log.log_event(&format!(
                    "{}_start_resume_fresh reason={:?}",
                    harness.binary, degrade
                ));
            }
        }
    } else if matches!(resume, Some(agent_doc_harness::ResumeRequest::Latest)) {
        let degrade = agent_doc_harness::ResumeDegrade::NoRecordedId;
        log.start_console_status(&format!(
            "[start] --resume requested but starting a FRESH {} conversation: {}",
            harness.binary,
            degrade.reason()
        ));
        log.log_event(&format!(
            "{}_start_resume_fresh reason={:?}",
            harness.binary, degrade
        ));
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
        fresh_base_args,
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
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingLaunchLog {
        events: Vec<String>,
    }

    impl SupervisorLaunchLog for RecordingLaunchLog {
        fn log_event(&mut self, msg: &str) {
            self.events.push(msg.to_string());
        }

        fn start_console_status(&mut self, _message: &str) {}
    }

    #[test]
    fn launch_resume_reads_only_the_selected_harness_entry() {
        let project = TempDir::new().unwrap();
        let document = project.path().join("plan.md");
        let content = concat!(
            "---\n",
            "agent: codex\n",
            "resume:\n",
            "  claude: claude-thread\n",
            "  codex: codex-thread\n",
            "  opencode: opencode-thread\n",
            "---\n",
        );
        let (mut fm, _) = frontmatter::parse(content).unwrap();
        let config = agent_doc_config::Config::default();
        let request = agent_doc_harness::ResumeRequest::Latest;
        let mut log = RecordingLaunchLog::default();

        let codex = build_harness_launch_spec_with_resume(
            &fm,
            &config,
            &document,
            &mut log,
            Some(&request),
        )
        .unwrap();
        assert!(codex.base_args.iter().any(|arg| arg == "codex-thread"));
        assert!(!codex.base_args.iter().any(|arg| arg == "claude-thread"));

        fm.agent = Some("claude".to_string());
        let claude = build_harness_launch_spec_with_resume(
            &fm,
            &config,
            &document,
            &mut log,
            Some(&request),
        )
        .unwrap();
        assert!(claude.base_args.iter().any(|arg| arg == "claude-thread"));
        assert!(!claude.base_args.iter().any(|arg| arg == "codex-thread"));
    }

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

    #[test]
    fn fresh_project_start_creates_default_supervisor_log_tree() {
        let project = TempDir::new().unwrap();
        let harness = HarnessConfig::codex();
        let mut log = RecordingLaunchLog::default();

        let guard =
            SupervisorStderrRedirect::maybe_start(project.path(), &harness, false, &mut log);
        drop(guard);

        let path = project.path().join(".agent-doc/logs/supervisor-stderr.log");
        assert!(path.is_file());
        assert_eq!(
            log.events.len(),
            1,
            "start setup should emit one ownership event: {:?}",
            log.events
        );
        assert!(log.events[0].contains(&format!("target={}", path.display())));
        assert!(log.events[0].contains("fallback_from=none"));
    }

    #[test]
    fn project_config_resolves_relative_supervisor_log_from_project_root() {
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(project.path().join(".agent-doc")).unwrap();
        std::fs::write(
            project.path().join(".agent-doc/config.toml"),
            "agent_doc_supervisor_stderr_log = \"var/log/agent-doc-supervisor.log\"\n",
        )
        .unwrap();

        let opened = open_supervisor_stderr_log(project.path()).unwrap();

        assert_eq!(
            opened.path(),
            project.path().join("var/log/agent-doc-supervisor.log")
        );
        assert!(opened.path().is_file());
        assert!(opened.fallback_from().is_none());
    }

    #[test]
    fn unavailable_project_log_uses_deterministic_fallback() {
        let project = TempDir::new().unwrap();
        std::fs::create_dir_all(project.path().join(".agent-doc")).unwrap();
        std::fs::write(
            project.path().join(".agent-doc/config.toml"),
            "supervisor_stderr_log = \"blocked/supervisor.log\"\n",
        )
        .unwrap();
        std::fs::write(project.path().join("blocked"), "not a directory").unwrap();

        let opened = open_supervisor_stderr_log(project.path()).unwrap();
        let primary_path = project.path().join("blocked/supervisor.log");

        assert_eq!(
            opened.path(),
            supervisor_stderr_fallback_path(project.path())
        );
        assert_eq!(opened.fallback_from(), Some(primary_path.as_path()));
        assert!(opened.path().is_file());
        let fallback_dir = opened.path().parent().unwrap().to_path_buf();
        drop(opened);
        std::fs::remove_dir_all(fallback_dir).unwrap();
    }
}
