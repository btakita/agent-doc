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

use anyhow::Result;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

use agent_doc_config::AgentConfig;
use agent_doc_frontmatter::frontmatter::Frontmatter;
use agent_doc_git_io::dirs::append_workspace_access_args;
use agent_doc_turn_executor::agent_stream::StreamingAgent;

/// Response from an agent backend.
pub struct AgentResponse {
    pub text: String,
    pub session_id: Option<String>,
}

pub const AGENT_DOC_RUN_AGENT_TIMEOUT_SECS_ENV: &str = "AGENT_DOC_RUN_AGENT_TIMEOUT_SECS";
pub const DEFAULT_RUN_AGENT_TIMEOUT_SECS: u64 = 30 * 60;

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

/// Isolate an agent backend in a child-owned process group so a timeout can
/// terminate and reap the backend together with any preflight/background
/// descendants it started.
pub fn configure_agent_child_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

#[cfg(unix)]
fn signal_agent_process_group(child_pid: u32, signal: libc::c_int) -> std::io::Result<()> {
    let result = unsafe { libc::kill(-(child_pid as libc::pid_t), signal) };
    if result == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(err)
    }
}

#[cfg(unix)]
fn agent_process_group_exists(child_pid: u32) -> std::io::Result<bool> {
    let result = unsafe { libc::kill(-(child_pid as libc::pid_t), 0) };
    if result == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(err),
    }
}

/// Classify timeout shutdown as cancellation and synchronously reap the child
/// process tree before a later preflight can contend with it.
pub fn terminate_agent_child_process_group(child: &mut Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        const TERMINATE_GRACE: Duration = Duration::from_millis(500);
        let child_pid = child.id();
        signal_agent_process_group(child_pid, libc::SIGTERM)?;
        let started = Instant::now();
        while started.elapsed() < TERMINATE_GRACE && agent_process_group_exists(child_pid)? {
            let _observed_status = child.try_wait()?;
            std::thread::sleep(Duration::from_millis(20));
        }
        if agent_process_group_exists(child_pid)? {
            signal_agent_process_group(child_pid, libc::SIGKILL)?;
        }
        let _reaped_status = child.wait()?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        child.kill()?;
        let _reaped_status = child.wait()?;
        Ok(())
    }
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
            let termination = terminate_agent_child_process_group(&mut child);
            if termination.is_ok() {
                for (name, handle) in [("stdout", stdout_handle), ("stderr", stderr_handle)] {
                    match handle.join() {
                        Ok(Ok(_)) => {}
                        Ok(Err(err)) => {
                            eprintln!("[agent] timed-out {name} reader failed: {err}");
                        }
                        Err(_) => {
                            eprintln!("[agent] timed-out {name} reader thread panicked");
                        }
                    }
                }
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "agent child process group timed out after {}s; classification=cancellation; process_group_reaped={}",
                    timeout.as_secs(),
                    termination.is_ok()
                ),
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
            "claude" => agent_doc_turn_executor::claude_launch::default_base_args(),
            "codex" => agent_doc_turn_executor::codex_launch::default_base_args(),
            "opencode" => agent_doc_turn_executor::opencode_launch::default_base_args(),
            _ => Vec::new(),
        });
    if let Some(file) = file {
        append_workspace_access_args(name, &mut args, file);
    }
    let args = if args.is_empty() { None } else { Some(args) };
    (command, args)
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

    #[cfg(unix)]
    fn process_exists(pid: libc::pid_t) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_agent_reaps_background_descendants() {
        let temp = tempfile::tempdir().unwrap();
        let descendant_pid_path = temp.path().join("descendant.pid");
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(format!(
                "sleep 30 & echo $! > '{}'; wait",
                descendant_pid_path.display()
            ))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        configure_agent_child_process_group(&mut command);
        let child = command.spawn().unwrap();

        // Wait for a pid that PARSES, not merely for the path to exist.
        // `echo $! > file` creates the file and writes it in two observable
        // steps, so `exists()` goes true while the content is still empty and
        // the parse below then panics on `""`. Observed on CI 2026-08-12
        // (run 31600939949) as a bare panic at the `.parse().unwrap()`, which
        // says nothing about which of the two races fired.
        let started = Instant::now();
        let descendant_pid: libc::pid_t = loop {
            if let Some(pid) = std::fs::read_to_string(&descendant_pid_path)
                .ok()
                .and_then(|raw| raw.trim().parse::<libc::pid_t>().ok())
            {
                break pid;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "background descendant never published a pid to {}",
                descendant_pid_path.display(),
            );
            std::thread::sleep(Duration::from_millis(10));
        };

        let err = wait_with_output_timeout(child, Duration::from_millis(100)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("classification=cancellation"));
        assert!(err.to_string().contains("process_group_reaped=true"));

        let started = Instant::now();
        while process_exists(descendant_pid) && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!process_exists(descendant_pid));
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
}
