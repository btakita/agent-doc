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
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Output};
use std::time::{Duration, Instant};

use self::streaming::StreamingAgent;
use agent_doc_config::AgentConfig;
use agent_doc_frontmatter::frontmatter::Frontmatter;
use agent_doc_git_io::dirs::workspace_access_dirs_for_doc;

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
            "claude" => agent_doc_turn_executor::claude_launch::default_base_args(),
            "codex" => agent_doc_turn_executor::codex_launch::default_base_args(),
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

    for dir in workspace_access_dirs_for_doc(file) {
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
    use std::fs;
    use std::process::Command;

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
}
