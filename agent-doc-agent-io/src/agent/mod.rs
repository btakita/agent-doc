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
pub mod grok;
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

/// Build the `Command` for a headless harness launch (`#headlessharnesspath`,
/// the headless counterpart of GH #380).
///
/// A bare `program` is resolved over the effective child env (the inherited
/// parent env, minus `removals`, plus the document `env:` `overrides`) through
/// `agent_doc_harness::spawn_program::resolve_headless_program`: a name on that
/// `PATH`, or one containing `/`, is launched exactly as before; a name missing
/// from it (the Coder workspace default `PATH` leaves out `~/.local/bin`) is
/// launched from the first well-known install directory that holds it, and that
/// directory is appended to the child `PATH`. When nothing is found the launch
/// fails with an error that lists every directory searched.
///
/// The returned `Command` already carries `removals` and `overrides` (applied
/// in that order); callers add args, stdio, and the process group.
pub fn harness_command(
    program: &str,
    removals: &[&str],
    overrides: &[(String, Option<String>)],
) -> Result<Command> {
    let path_overridden = overrides.iter().any(|(key, _)| key == "PATH");
    // A non-UTF-8 inherited PATH cannot be searched as a `String`; leave the
    // lookup to `Command` exactly as before rather than misreport it missing.
    let parent_path_unreadable = !path_overridden
        && !removals.contains(&"PATH")
        && std::env::var_os("PATH").is_some_and(|raw| raw.to_str().is_none());
    let resolved = if parent_path_unreadable {
        None
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Some(agent_doc_harness::spawn_program::resolve_headless_program(
            program,
            removals,
            overrides,
            |key| std::env::var(key).ok(),
            &cwd,
        )?)
    };
    let launch_program = resolved
        .as_ref()
        .map_or(program, |headless| headless.resolved.program.as_str());
    let mut command = Command::new(launch_program);
    for key in removals {
        command.env_remove(key);
    }
    for (key, value) in overrides {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    if let Some(headless) = resolved.as_ref()
        && let Some(child_path) = headless.child_path.as_deref()
    {
        command.env("PATH", child_path);
        eprintln!(
            "[agent] harness_program_resolved harness={program} source={} program={} path_dir_appended={}",
            headless.resolved.source.as_str(),
            headless.resolved.program,
            headless
                .resolved
                .path_dir_appended
                .as_deref()
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
        );
    }
    Ok(command)
}

/// Isolate an agent backend in a child-owned process group so a timeout can
/// terminate and reap the backend together with any preflight/background
/// descendants it started.
pub fn configure_agent_child_process_group(command: &mut Command) {
    // The child harness publishes its own identity. Carrying a parent Grok ID
    // into another harness would misattribute its tools/admission to Grok.
    command.env_remove("GROK_SESSION_ID");
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
        "grok" | "grok-build" => Ok(Box::new(grok::Grok::new(cmd, args).with_env(env))),
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
        "grok" | "grok-build" => Ok(Box::new(grok::Grok::new(cmd, args).with_env(env))),
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
        "opencode" | "grok" | "grok-build" => Ok(None),
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

/// `#headlessharnesspath` — every headless backend resolves a bare harness name
/// the same way the supervisor's pty spawn does (GH #380). Each test gives the
/// child the Coder workspace default `PATH` and a temp `HOME` through the
/// document `env:` overrides (the real `HOME` and process env are never
/// touched), installs a fake harness only under `$HOME/.local/bin`, and has it
/// report the `PATH` it was started with.
#[cfg(all(test, unix))]
mod headless_harness_path_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// The Coder workspace default `PATH`: system dirs only, no `~/.local/bin`.
    const CODER_DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

    struct FakeHome {
        home: TempDir,
        program: String,
    }

    impl FakeHome {
        /// Install `body` as `$HOME/.local/bin/<unique name>`. The name is
        /// unique to the suite, so a real install under `/usr/local/bin` (a
        /// system fallback dir) can never satisfy the lookup.
        fn install(harness: &str, body: &str) -> Self {
            let home = TempDir::new().unwrap();
            let program = format!("agent-doc-headlessharnesspath-fake-{harness}");
            let local_bin = home.path().join(".local/bin");
            std::fs::create_dir_all(&local_bin).unwrap();
            let path = local_bin.join(&program);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            Self { home, program }
        }

        fn local_bin(&self) -> String {
            self.home.path().join(".local/bin").display().to_string()
        }

        /// The document `env:` overrides that recreate a Coder controller.
        fn coder_env(&self) -> Vec<(String, Option<String>)> {
            vec![
                ("PATH".to_string(), Some(CODER_DEFAULT_PATH.to_string())),
                (
                    "HOME".to_string(),
                    Some(self.home.path().display().to_string()),
                ),
            ]
        }

        /// The child `PATH` a fallback launch must see: every Coder entry
        /// first, then the install directory it was found in.
        fn expected_child_path(&self) -> String {
            format!("{CODER_DEFAULT_PATH}:{}", self.local_bin())
        }
    }

    #[test]
    fn harness_command_launches_bare_name_from_user_local_bin_under_coder_default_path() {
        let fake = FakeHome::install("cmd", "printf '%s' \"$PATH\"\n");
        let output = harness_command(&fake.program, &[], &fake.coder_env())
            .unwrap()
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            fake.expected_child_path()
        );
    }

    #[test]
    fn harness_command_not_found_lists_every_searched_directory() {
        let home = TempDir::new().unwrap();
        let env = vec![
            ("PATH".to_string(), Some(CODER_DEFAULT_PATH.to_string())),
            ("HOME".to_string(), Some(home.path().display().to_string())),
        ];
        let err = harness_command("agent-doc-headlessharnesspath-missing", &[], &env)
            .expect_err("a harness installed nowhere must not resolve");
        let msg = format!("{err:#}");
        assert!(msg.contains("was not found"), "{msg}");
        assert!(
            msg.contains(&format!("child PATH {CODER_DEFAULT_PATH}")),
            "{msg}"
        );
        let local_bin = home.path().join(".local/bin");
        assert!(msg.contains(local_bin.to_str().unwrap()), "{msg}");
    }

    #[test]
    fn harness_command_keeps_a_name_on_the_child_path_as_given() {
        let fake = FakeHome::install("aspath", "exit 0\n");
        let env = vec![
            ("PATH".to_string(), Some(fake.local_bin())),
            (
                "HOME".to_string(),
                Some(fake.home.path().display().to_string()),
            ),
        ];
        let mut command = harness_command(&fake.program, &[], &env).unwrap();
        assert_eq!(command.get_program(), fake.program.as_str());
        assert!(command.status().unwrap().success());
    }

    #[test]
    fn claude_send_resolves_bare_name_missing_from_coder_default_path() {
        let fake = FakeHome::install(
            "claude",
            "cat >/dev/null\nprintf '{\"result\":\"%s\",\"session_id\":\"s1\"}' \"$PATH\"\n",
        );
        let claude =
            claude::Claude::new(Some(fake.program.clone()), None).with_env(fake.coder_env());
        let response = claude.send("prompt", None, false, None).unwrap();
        assert_eq!(response.text, fake.expected_child_path());
    }

    #[test]
    fn claude_send_streaming_resolves_bare_name_missing_from_coder_default_path() {
        let fake = FakeHome::install(
            "claude-stream",
            "cat >/dev/null\nprintf '{\"type\":\"result\",\"result\":\"%s\",\"session_id\":\"s1\"}\\n' \"$PATH\"\n",
        );
        let claude =
            claude::Claude::new(Some(fake.program.clone()), None).with_env(fake.coder_env());
        let last = claude
            .send_streaming("prompt", None, false, None)
            .unwrap()
            .map(Result::unwrap)
            .last()
            .unwrap();
        assert!(last.is_final);
        assert_eq!(last.text, fake.expected_child_path());
    }

    #[test]
    fn codex_send_resolves_bare_name_missing_from_coder_default_path() {
        let fake = FakeHome::install(
            "codex",
            concat!(
                "cat >/dev/null\n",
                "printf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"t1\"}'\n",
                "printf '{\"type\":\"item.completed\",\"item\":{\"id\":\"m1\",\"type\":\"agent_message\",\"text\":\"%s\"}}\\n' \"$PATH\"\n",
                "printf '%s\\n' '{\"type\":\"turn.completed\",\"usage\":{}}'\n",
            ),
        );
        let codex = codex::Codex::new(Some(fake.program.clone()), None).with_env(fake.coder_env());
        let response = codex.send("prompt", None, false, None).unwrap();
        assert_eq!(response.text, fake.expected_child_path());
    }

    #[test]
    fn opencode_send_resolves_bare_name_missing_from_coder_default_path() {
        let fake = FakeHome::install("opencode", "printf '%s' \"$PATH\"\n");
        let opencode =
            opencode::OpenCode::new(Some(fake.program.clone()), None).with_env(fake.coder_env());
        let response = opencode.send("prompt", None, false, None).unwrap();
        assert_eq!(response.text, fake.expected_child_path());
    }

    #[test]
    fn grok_send_resolves_bare_name_missing_from_coder_default_path() {
        let fake = FakeHome::install(
            "grok",
            "printf '{\"text\":\"%s\",\"stopReason\":\"end_turn\",\"sessionId\":\"g1\"}' \"$PATH\"\n",
        );
        let grok = grok::Grok::new(Some(fake.program.clone()), None).with_env(fake.coder_env());
        let response = grok.send("prompt", None, false, None).unwrap();
        assert_eq!(response.text, fake.expected_child_path());
    }
}
