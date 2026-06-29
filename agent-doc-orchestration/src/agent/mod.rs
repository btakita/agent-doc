//! # Module: agent
//!
//! ## Spec
//! - Defines the `Agent` trait: one method `send(prompt, session_id, fork, model)` → `AgentResponse`.
//! - `AgentResponse` carries the response text and an optional session ID for session resumption.
//! - `resolve(name, config)` maps a backend name (`"claude"`, `"codex"`, `"opencode"`,
//!   `"junie"`, or a config-defined name)
//!   to a boxed `Agent` implementation.
//! - When `config` is provided for an unknown name, falls back to the Claude backend using
//!   the configured command and args.
//! - Errors on unknown names with no config entry.
//!
//! ## Agentic Contracts
//! - `resolve` always returns a `Box<dyn Agent>` on success; callers need not know the concrete type.
//! - `Agent::send` is synchronous (no async); callers block until the full response is available.
//! - `session_id` threads conversation context across multiple `send` calls.
//! - `fork = true` continues the most recent session and branches it (only when `session_id` is `None`).
//!
//! ## Evals
//! - resolve_claude: `resolve("claude", None)` → returns Claude backend without error
//! - resolve_codex: `resolve("codex", None)` → returns Codex backend without error
//! - resolve_junie: `resolve("junie", None)` → returns Junie backend without error
//! - resolve_unknown_no_config: `resolve("other", None)` → `Err("Unknown agent backend: other")`
//! - resolve_unknown_with_config: `resolve("custom", Some(&cfg))` → returns Claude backend using cfg

pub mod claude;
pub mod codex;
pub mod junie;
pub mod opencode;
pub mod streaming;

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::process::{Child, Output};
use std::time::{Duration, Instant};

use self::streaming::StreamingAgent;
use crate::config::{AgentConfig, Config};
use agent_doc_core::frontmatter::{CodexNetworkAccess, Frontmatter};

/// Response from an agent backend.
pub struct AgentResponse {
    pub text: String,
    pub session_id: Option<String>,
}

pub const CODEX_SANDBOX_NETWORK_DISABLED_ENV: &str = "CODEX_SANDBOX_NETWORK_DISABLED";
pub const AGENT_DOC_RUN_AGENT_TIMEOUT_SECS_ENV: &str = "AGENT_DOC_RUN_AGENT_TIMEOUT_SECS";
pub const DEFAULT_RUN_AGENT_TIMEOUT_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexNetworkPolicyStatus {
    pub access: CodexNetworkAccess,
    pub parent_disabled: bool,
    pub effective_disabled: bool,
    pub sandbox_mode: Option<String>,
}

impl CodexNetworkPolicyStatus {
    pub fn summary(&self) -> String {
        let effective = if self.effective_disabled {
            "disabled"
        } else {
            "enabled"
        };
        let requested = match self.access {
            CodexNetworkAccess::Inherit => "inherit",
            CodexNetworkAccess::Enabled => "enabled",
            CodexNetworkAccess::Disabled => "disabled",
        };
        let detail = match self.access {
            CodexNetworkAccess::Inherit if self.parent_disabled => {
                "inherited CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
            CodexNetworkAccess::Inherit => {
                "no inherited CODEX_SANDBOX_NETWORK_DISABLED override".to_string()
            }
            CodexNetworkAccess::Enabled if self.parent_disabled => {
                "agent-doc removed inherited CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
            CodexNetworkAccess::Enabled => {
                "agent-doc forced CODEX_SANDBOX_NETWORK_DISABLED off".to_string()
            }
            CodexNetworkAccess::Disabled => {
                "agent-doc forced CODEX_SANDBOX_NETWORK_DISABLED=1".to_string()
            }
        };
        match self.sandbox_mode.as_deref() {
            Some(mode) => format!("{effective} (policy: {requested}, sandbox: {mode}; {detail})"),
            None => format!("{effective} (policy: {requested}; {detail})"),
        }
    }

    pub fn mismatch_error(&self) -> Option<String> {
        match self.access {
            CodexNetworkAccess::Enabled if self.effective_disabled => Some(format!(
                "Codex launch policy mismatch: `codex_network_access: enabled` should remove \
                 `{}`, but the effective child env is still network-disabled.",
                CODEX_SANDBOX_NETWORK_DISABLED_ENV
            )),
            CodexNetworkAccess::Disabled if !self.effective_disabled => Some(format!(
                "Codex launch policy mismatch: `codex_network_access: disabled` should set \
                 `{}` to `1`, but the effective child env is still network-enabled.",
                CODEX_SANDBOX_NETWORK_DISABLED_ENV
            )),
            CodexNetworkAccess::Inherit
                if self.effective_disabled
                    && self.sandbox_mode.as_deref() == Some("danger-full-access") =>
            {
                Some(format!(
                    "Codex launch policy mismatch: sandbox is `danger-full-access`, but network \
                     is still disabled by inherited {}=1. Set `codex_network_access: enabled` \
                     (or `codex_network_access = \"enabled\"` in config) so agent-doc removes \
                     that launcher override, or remove the env var before starting the session.",
                    CODEX_SANDBOX_NETWORK_DISABLED_ENV
                ))
            }
            _ => None,
        }
    }
}

fn parent_codex_network_disabled() -> bool {
    std::env::var(CODEX_SANDBOX_NETWORK_DISABLED_ENV)
        .ok()
        .as_deref()
        == Some("1")
}

fn codex_sandbox_mode_from_config(value: &str) -> Option<String> {
    let raw = value.trim();
    let mode = raw.strip_prefix("sandbox_mode=")?;
    let mode = mode.trim().trim_matches(|c| c == '"' || c == '\'');
    if mode.is_empty() {
        None
    } else {
        Some(mode.to_string())
    }
}

fn codex_sandbox_mode_from_args(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-s" | "--sandbox" => {
                if let Some(mode) = iter.next() {
                    return Some(mode.clone());
                }
            }
            "-c" | "--config" => {
                if let Some(value) = iter.next()
                    && let Some(mode) = codex_sandbox_mode_from_config(value)
                {
                    return Some(mode);
                }
            }
            _ => {
                if let Some(mode) = arg.strip_prefix("--sandbox=") {
                    return Some(mode.to_string());
                }
                if let Some(value) = arg.strip_prefix("--config=")
                    && let Some(mode) = codex_sandbox_mode_from_config(value)
                {
                    return Some(mode);
                }
            }
        }
    }
    None
}

pub fn resolve_codex_network_access(
    fm: &Frontmatter,
    global_config: &Config,
) -> CodexNetworkAccess {
    fm.codex_network_access
        .or(global_config.codex_network_access)
        .unwrap_or_default()
}

/// Default number of managed-capability-proof attempts (the failing attempt plus
/// retries) before the dispatch gate commits to `Failed`.
pub const DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS: u32 = 3;
/// Default base back-off between managed-capability-proof retries.
pub const DEFAULT_MANAGED_PROOF_RETRY_BACKOFF: Duration = Duration::from_secs(2);
/// Default managed-capability child probe timeout.
pub const DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT: Duration = Duration::from_secs(45);
/// Upper bound on a single retry back-off so exponential growth stays sane.
pub const MAX_MANAGED_PROOF_BACKOFF: Duration = Duration::from_secs(30);

/// Resolved managed-capability-proof retry/timeout policy.
///
/// The proof runs a network/SSH/write-root probe in a background supervisor
/// thread; a transient failure used to set the dispatch gate to `Failed`
/// permanently (only a fresh `start` reset it). This policy bounds retries so a
/// short network blip self-heals instead of wedging the whole session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedProofPolicy {
    /// Total attempts (1 = no retry, legacy behavior).
    pub max_attempts: u32,
    /// Base back-off between retries (grows exponentially, capped).
    pub base_backoff: Duration,
    /// Child probe timeout.
    pub probe_timeout: Duration,
}

impl Default for ManagedProofPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS,
            base_backoff: DEFAULT_MANAGED_PROOF_RETRY_BACKOFF,
            probe_timeout: DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT,
        }
    }
}

/// Resolve the managed-capability-proof policy, preferring frontmatter over the
/// project/global config, falling back to built-in defaults.
pub fn resolve_managed_proof_policy(
    fm: &Frontmatter,
    global_config: &Config,
) -> ManagedProofPolicy {
    let max_attempts = fm
        .managed_proof_max_attempts
        .or(global_config.managed_proof_max_attempts)
        .map(|n| n.max(1))
        .unwrap_or(DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS);
    let base_backoff = fm
        .managed_proof_retry_backoff_secs
        .or(global_config.managed_proof_retry_backoff_secs)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_MANAGED_PROOF_RETRY_BACKOFF);
    let probe_timeout = fm
        .managed_proof_probe_timeout_secs
        .or(global_config.managed_proof_probe_timeout_secs)
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    ManagedProofPolicy {
        max_attempts,
        base_backoff,
        probe_timeout,
    }
}

/// Outcome of [`proof_retry_decision`]: either retry after a back-off or commit
/// to a permanent `Failed` gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProofRetryDecision {
    /// Retry the probe after sleeping `backoff`.
    Retry { backoff: Duration },
    /// Stop retrying; set the dispatch gate to `Failed`.
    GiveUp,
}

/// Decide whether a failed managed-capability-proof attempt should be retried.
///
/// `attempt` is the 1-based count of attempts already made (including the one
/// that just failed). Back-off grows exponentially from `base_backoff` and is
/// capped at [`MAX_MANAGED_PROOF_BACKOFF`]. Pure and deterministic so the
/// back-off schedule can be unit-tested without shelling out to the network.
pub fn proof_retry_decision(
    attempt: u32,
    max_attempts: u32,
    base_backoff: Duration,
) -> ProofRetryDecision {
    if attempt >= max_attempts.max(1) {
        return ProofRetryDecision::GiveUp;
    }
    let shift = attempt.saturating_sub(1).min(5);
    let multiplier = 1u32 << shift;
    let backoff = base_backoff
        .saturating_mul(multiplier)
        .min(MAX_MANAGED_PROOF_BACKOFF);
    ProofRetryDecision::Retry { backoff }
}

pub fn apply_codex_network_access_env_map(
    env: &mut HashMap<String, String>,
    access: CodexNetworkAccess,
) {
    match access {
        CodexNetworkAccess::Inherit => {}
        CodexNetworkAccess::Enabled => {
            env.remove(CODEX_SANDBOX_NETWORK_DISABLED_ENV);
        }
        CodexNetworkAccess::Disabled => {
            env.insert(
                CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(),
                "1".to_string(),
            );
        }
    }
}

pub fn apply_codex_network_access_env_overrides(
    env: &mut Vec<(String, Option<String>)>,
    access: CodexNetworkAccess,
) {
    match access {
        CodexNetworkAccess::Inherit => {}
        CodexNetworkAccess::Enabled => {
            env.push((CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(), None))
        }
        CodexNetworkAccess::Disabled => env.push((
            CODEX_SANDBOX_NETWORK_DISABLED_ENV.to_string(),
            Some("1".to_string()),
        )),
    }
}

pub fn codex_network_status_from_env_map(
    args: &[String],
    access: CodexNetworkAccess,
    env: &HashMap<String, String>,
) -> CodexNetworkPolicyStatus {
    CodexNetworkPolicyStatus {
        access,
        parent_disabled: parent_codex_network_disabled(),
        effective_disabled: env
            .get(CODEX_SANDBOX_NETWORK_DISABLED_ENV)
            .is_some_and(|value| value == "1"),
        sandbox_mode: codex_sandbox_mode_from_args(args),
    }
}

pub fn codex_network_status_from_overrides(
    args: &[String],
    access: CodexNetworkAccess,
    overrides: &[(String, Option<String>)],
) -> CodexNetworkPolicyStatus {
    let parent_disabled = parent_codex_network_disabled();
    let mut effective_disabled = parent_disabled;
    for (key, value) in overrides {
        if key == CODEX_SANDBOX_NETWORK_DISABLED_ENV {
            effective_disabled = value.as_deref() == Some("1");
        }
    }
    CodexNetworkPolicyStatus {
        access,
        parent_disabled,
        effective_disabled,
        sandbox_mode: codex_sandbox_mode_from_args(args),
    }
}

/// Agent backend trait — send a prompt, get a response.
pub trait Agent {
    fn send(
        &self,
        prompt: &str,
        session_id: Option<&str>,
        fork: bool,
        model: Option<&str>,
    ) -> Result<AgentResponse>;
}

pub fn run_agent_timeout() -> Duration {
    let secs = std::env::var(AGENT_DOC_RUN_AGENT_TIMEOUT_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_RUN_AGENT_TIMEOUT_SECS);
    Duration::from_secs(secs.max(1))
}

pub fn wait_with_output_timeout(mut child: Child, timeout: Duration) -> std::io::Result<Output> {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut stream) = stdout {
            stream.read_to_end(&mut buf)?;
        }
        Ok::<_, std::io::Error>(buf)
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut stream) = stderr {
            stream.read_to_end(&mut buf)?;
        }
        Ok::<_, std::io::Error>(buf)
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("agent child process timed out after {}s", timeout.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let stdout = stdout_handle
        .join()
        .map_err(|_| std::io::Error::other("stdout reader thread panicked"))??;
    let stderr = stderr_handle
        .join()
        .map_err(|_| std::io::Error::other("stderr reader thread panicked"))??;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Resolve an agent backend by name. `env` is applied to the spawned child process
/// (currently only honored by the Claude backend).
fn build_backend_command(
    name: &str,
    config: Option<&AgentConfig>,
    file: Option<&Path>,
) -> (Option<String>, Option<Vec<String>>) {
    let command = config.map(|ac| ac.command.clone());
    let mut args = config
        .map(|ac| ac.args.clone())
        .unwrap_or_else(|| match name {
            "claude" => claude::default_base_args(),
            "codex" => codex::default_base_args(),
            "opencode" => opencode::default_base_args(),
            _ => Vec::new(),
        });
    if let Some(file) = file {
        append_workspace_access_args(name, &mut args, file);
    }
    let args = if args.is_empty() { None } else { Some(args) };
    (command, args)
}

pub fn append_workspace_access_args(agent_name: &str, args: &mut Vec<String>, file: &Path) {
    if !matches!(agent_name, "claude" | "codex") {
        return;
    }

    let mut existing = HashSet::new();
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--add-dir" {
            if let Some(dir) = iter.next() {
                existing.insert(dir.clone());
            }
            continue;
        }
        if let Some(dir) = arg.strip_prefix("--add-dir=") {
            existing.insert(dir.to_string());
        }
    }

    for dir in crate::git::workspace_access_dirs_for_doc(file) {
        let dir = dir.to_string_lossy().into_owned();
        if existing.insert(dir.clone()) {
            args.push("--add-dir".into());
            args.push(dir);
        }
    }
}

#[allow(dead_code)]
pub fn resolve(
    name: &str,
    config: Option<&AgentConfig>,
    env: Vec<(String, Option<String>)>,
) -> Result<Box<dyn Agent>> {
    let (cmd, args) = build_backend_command(name, config, None);
    match name {
        "claude" => Ok(Box::new(claude::Claude::new(cmd, args).with_env(env))),
        "codex" => Ok(Box::new(codex::Codex::new(cmd, args).with_env(env))),
        "opencode" => Ok(Box::new(opencode::OpenCode::new(cmd, args).with_env(env))),
        "junie" => Ok(Box::new(junie::Junie::new(cmd, args))),
        other => {
            if config.is_some() {
                Ok(Box::new(claude::Claude::new(cmd, args).with_env(env)))
            } else {
                anyhow::bail!("Unknown agent backend: {}", other)
            }
        }
    }
}

pub fn resolve_for_file(
    name: &str,
    config: Option<&AgentConfig>,
    env: Vec<(String, Option<String>)>,
    file: &Path,
    fm: &Frontmatter,
) -> Result<Box<dyn Agent>> {
    let (cmd, args) = build_backend_command(name, config, Some(file));
    match name {
        "claude" => Ok(Box::new(claude::Claude::new(cmd, args).with_env(env))),
        "codex" => Ok(Box::new(
            codex::Codex::new(cmd, args)
                .with_env(env)
                .with_required_ssh_targets(fm.required_ssh_targets.clone()),
        )),
        "opencode" => Ok(Box::new(opencode::OpenCode::new(cmd, args).with_env(env))),
        "junie" => Ok(Box::new(junie::Junie::new(cmd, args))),
        other => {
            if config.is_some() {
                Ok(Box::new(claude::Claude::new(cmd, args).with_env(env)))
            } else {
                anyhow::bail!("Unknown agent backend: {}", other)
            }
        }
    }
}

pub fn resolve_streaming_for_file(
    name: &str,
    config: Option<&AgentConfig>,
    env: Vec<(String, Option<String>)>,
    file: &Path,
    fm: &Frontmatter,
) -> Result<Option<Box<dyn StreamingAgent>>> {
    let (cmd, args) = build_backend_command(name, config, Some(file));
    match name {
        "claude" => Ok(Some(Box::new(claude::Claude::new(cmd, args).with_env(env)))),
        "codex" => Ok(Some(Box::new(
            codex::Codex::new(cmd, args)
                .with_env(env)
                .with_required_ssh_targets(fm.required_ssh_targets.clone()),
        ))),
        "opencode" => Ok(None),
        "junie" => Ok(None),
        other => {
            if config.is_some() {
                Ok(None)
            } else {
                anyhow::bail!("Unknown agent backend: {}", other)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_core::frontmatter::CodexNetworkAccess;
    use std::fs;
    use std::process::Command;
    use std::sync::MutexGuard;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn has_add_dir(args: &[String], dir: &Path) -> bool {
        let dir = dir.to_string_lossy();
        args.windows(2)
            .any(|w| w[0] == "--add-dir" && w[1] == dir.as_ref())
    }

    fn init_repo(repo: &Path) {
        Command::new("git")
            .current_dir(repo)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
    }

    fn commit_file(repo: &Path, rel: &str, content: &str, msg: &str) {
        let path = repo.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, content).unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["add", "--", rel])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    fn add_submodule(repo: &Path, origin: &Path, target: &str, msg: &str) {
        let url = format!("file://{}", origin.display());
        let output = Command::new("git")
            .current_dir(repo)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &url,
                target,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Command::new("git")
            .current_dir(repo)
            .args(["commit", "-m", msg, "--no-verify"])
            .output()
            .unwrap();
    }

    #[test]
    fn resolve_claude() {
        let agent = resolve("claude", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_codex() {
        let agent = resolve("codex", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_junie() {
        let agent = resolve("junie", None, vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn resolve_unknown_no_config() {
        let agent = resolve("other", None, vec![]);
        assert!(agent.is_err());
        let err = agent.map(|_| ()).unwrap_err();
        assert!(err.to_string().contains("Unknown agent backend: other"));
    }

    #[test]
    fn resolve_unknown_with_config_falls_back() {
        let cfg = AgentConfig {
            command: "custom-binary".into(),
            args: vec!["--flag".into()],
            result_path: None,
            session_path: None,
        };
        let agent = resolve("custom", Some(&cfg), vec![]);
        assert!(agent.is_ok());
    }

    #[test]
    fn multi_harness_resolve_all_backends_independently() {
        let claude = resolve("claude", None, vec![]);
        let codex = resolve("codex", None, vec![]);
        let junie = resolve("junie", None, vec![]);
        assert!(claude.is_ok());
        assert!(codex.is_ok());
        assert!(junie.is_ok());
    }

    #[test]
    fn multi_harness_resolve_with_env_isolation() {
        let claude_env = vec![("CLAUDECODE".into(), None)];
        let codex_env = vec![("CODEX_CLI".into(), None), ("CODEX".into(), None)];
        let claude = resolve("claude", None, claude_env);
        let codex = resolve("codex", None, codex_env);
        assert!(claude.is_ok());
        assert!(codex.is_ok());
    }

    #[test]
    fn resolve_streaming_skips_non_streaming_backend() {
        let fm = Frontmatter::default();
        let streaming = resolve_streaming_for_file("junie", None, vec![], Path::new("doc.md"), &fm);
        assert!(streaming.is_ok());
        assert!(streaming.unwrap().is_none());
    }

    #[test]
    fn append_workspace_access_args_adds_superproject_root_for_submodule_docs() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();

        let sub_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_dir.path();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        fs::write(sub_origin.join("README.md"), "# sub\n").unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(sub_origin)
            .args(["commit", "-m", "init sub", "--no-verify"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(outer)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["config", "protocol.file.allow", "always"])
            .output()
            .unwrap();
        fs::write(outer.join("README.md"), "# outer\n").unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "init outer", "--no-verify"])
            .output()
            .unwrap();

        let sub_url = format!("file://{}", sub_origin.display());
        let sub_status = Command::new("git")
            .current_dir(outer)
            .args([
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                &sub_url,
                "src/sub",
            ])
            .output()
            .unwrap();
        assert!(
            sub_status.status.success(),
            "submodule add failed: {}",
            String::from_utf8_lossy(&sub_status.stderr)
        );
        Command::new("git")
            .current_dir(outer)
            .args(["commit", "-m", "add submodule", "--no-verify"])
            .output()
            .unwrap();

        let doc = outer.join("src/sub/session.md");
        fs::write(&doc, "test\n").unwrap();

        let mut args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "-s".to_string(),
            "danger-full-access".to_string(),
        ];
        append_workspace_access_args("codex", &mut args, &doc);

        assert!(has_add_dir(&args, outer));
        assert!(has_add_dir(&args, &outer.join(".git/modules/src/sub")));
        assert!(has_add_dir(&args, &outer.join(".git")));
    }

    #[test]
    fn append_workspace_access_args_adds_nested_submodule_gitdirs() {
        let outer_dir = tempfile::TempDir::new().unwrap();
        let outer = outer_dir.path();
        init_repo(outer);
        commit_file(outer, "README.md", "# outer\n", "init outer");

        let sub_origin_dir = tempfile::TempDir::new().unwrap();
        let sub_origin = sub_origin_dir.path();
        init_repo(sub_origin);
        commit_file(sub_origin, "README.md", "# sub\n", "init sub");

        let nested_origin_dir = tempfile::TempDir::new().unwrap();
        let nested_origin = nested_origin_dir.path();
        init_repo(nested_origin);
        commit_file(nested_origin, "README.md", "# nested\n", "init nested");

        add_submodule(outer, sub_origin, "src/sub", "add submodule");

        let submodule_root = outer.join("src/sub");
        add_submodule(
            &submodule_root,
            nested_origin,
            "src/nested",
            "add nested submodule",
        );

        let doc = submodule_root.join("tasks/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(&doc, "test\n").unwrap();

        let mut args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "-s".to_string(),
            "workspace-write".to_string(),
        ];
        append_workspace_access_args("codex", &mut args, &doc);

        assert!(has_add_dir(&args, outer));
        assert!(has_add_dir(&args, &outer.join(".git/modules/src/sub")));
        assert!(has_add_dir(
            &args,
            &outer.join(".git/modules/src/sub/modules/src/nested")
        ));
        assert!(has_add_dir(&args, &outer.join(".git")));
    }

    #[test]
    fn resolve_codex_network_access_prefers_frontmatter_over_config() {
        let fm = Frontmatter {
            codex_network_access: Some(CodexNetworkAccess::Enabled),
            ..Default::default()
        };
        let config = Config {
            codex_network_access: Some(CodexNetworkAccess::Disabled),
            ..Default::default()
        };
        assert_eq!(
            resolve_codex_network_access(&fm, &config),
            CodexNetworkAccess::Enabled
        );
    }

    #[test]
    fn resolve_managed_proof_policy_uses_defaults_when_unset() {
        let policy = resolve_managed_proof_policy(&Frontmatter::default(), &Config::default());
        assert_eq!(policy.max_attempts, DEFAULT_MANAGED_PROOF_MAX_ATTEMPTS);
        assert_eq!(policy.base_backoff, DEFAULT_MANAGED_PROOF_RETRY_BACKOFF);
        assert_eq!(policy.probe_timeout, DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    }

    #[test]
    fn resolve_managed_proof_policy_prefers_frontmatter_over_config() {
        let fm = Frontmatter {
            managed_proof_max_attempts: Some(5),
            managed_proof_retry_backoff_secs: Some(3),
            managed_proof_probe_timeout_secs: Some(90),
            ..Default::default()
        };
        let config = Config {
            managed_proof_max_attempts: Some(2),
            managed_proof_retry_backoff_secs: Some(1),
            managed_proof_probe_timeout_secs: Some(10),
            ..Default::default()
        };
        let policy = resolve_managed_proof_policy(&fm, &config);
        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.base_backoff, Duration::from_secs(3));
        assert_eq!(policy.probe_timeout, Duration::from_secs(90));
    }

    #[test]
    fn resolve_managed_proof_policy_clamps_and_falls_back_to_config() {
        let config = Config {
            managed_proof_max_attempts: Some(0),
            managed_proof_probe_timeout_secs: Some(0),
            ..Default::default()
        };
        let policy = resolve_managed_proof_policy(&Frontmatter::default(), &config);
        // max_attempts is clamped to at least 1; a zero probe timeout falls back
        // to the default rather than disabling the probe wait.
        assert_eq!(policy.max_attempts, 1);
        assert_eq!(policy.probe_timeout, DEFAULT_MANAGED_PROOF_PROBE_TIMEOUT);
    }

    #[test]
    fn proof_retry_decision_retries_with_exponential_backoff() {
        let base = Duration::from_secs(2);
        assert_eq!(
            proof_retry_decision(1, 3, base),
            ProofRetryDecision::Retry {
                backoff: Duration::from_secs(2)
            }
        );
        assert_eq!(
            proof_retry_decision(2, 3, base),
            ProofRetryDecision::Retry {
                backoff: Duration::from_secs(4)
            }
        );
    }

    #[test]
    fn proof_retry_decision_gives_up_at_budget() {
        let base = Duration::from_secs(2);
        assert_eq!(proof_retry_decision(3, 3, base), ProofRetryDecision::GiveUp);
        // A single-attempt budget (legacy behavior) never retries.
        assert_eq!(proof_retry_decision(1, 1, base), ProofRetryDecision::GiveUp);
    }

    #[test]
    fn proof_retry_decision_caps_backoff() {
        let base = Duration::from_secs(20);
        // attempt 2 → 20s * 2 = 40s, capped to MAX_MANAGED_PROOF_BACKOFF (30s).
        assert_eq!(
            proof_retry_decision(2, 10, base),
            ProofRetryDecision::Retry {
                backoff: MAX_MANAGED_PROOF_BACKOFF
            }
        );
    }

    #[test]
    fn codex_network_status_detects_inherited_disable_mismatch() {
        let _guard = EnvGuard::set(CODEX_SANDBOX_NETWORK_DISABLED_ENV, "1");
        let args = vec!["-s".to_string(), "danger-full-access".to_string()];
        let status =
            codex_network_status_from_overrides(&args, CodexNetworkAccess::Inherit, &Vec::new());
        assert!(status.effective_disabled);
        assert!(status.mismatch_error().is_some());
    }

    #[test]
    fn codex_network_status_reads_config_sandbox_mode() {
        let _guard = EnvGuard::set(CODEX_SANDBOX_NETWORK_DISABLED_ENV, "1");
        let args = vec![
            "-c".to_string(),
            "sandbox_mode=\"danger-full-access\"".to_string(),
        ];
        let status =
            codex_network_status_from_overrides(&args, CodexNetworkAccess::Inherit, &Vec::new());
        assert_eq!(status.sandbox_mode.as_deref(), Some("danger-full-access"));
        assert!(status.mismatch_error().is_some());
    }

    #[test]
    fn codex_network_status_clears_inherited_disable_when_enabled() {
        let _guard = EnvGuard::set(CODEX_SANDBOX_NETWORK_DISABLED_ENV, "1");
        let args = vec!["-s".to_string(), "danger-full-access".to_string()];
        let mut overrides = Vec::new();
        apply_codex_network_access_env_overrides(&mut overrides, CodexNetworkAccess::Enabled);
        let status =
            codex_network_status_from_overrides(&args, CodexNetworkAccess::Enabled, &overrides);
        assert!(!status.effective_disabled);
        assert!(status.mismatch_error().is_none());
    }
}
