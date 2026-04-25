//! # Module: skill
//!
//! ## Spec
//! - Bundles SKILL.md into the binary at compile time via `include_str!`.
//! - `install()` writes the bundled SKILL.md to `.claude/skills/agent-doc/SKILL.md`
//!   under the git superproject root (or toplevel if not a submodule).
//! - `install_at(root)` accepts an explicit root override; used by tests.
//! - `install_and_check_updated()` installs and returns `true` if the file was
//!   absent or stale, `false` if already up to date.
//! - `check()` / `check_at(root)` verify the installed skill matches the bundled
//!   version; exit code 1 if out of date.
//! - Install is idempotent: calling it multiple times with identical content is a no-op.
//! - When CWD is inside a git submodule, resolves to the superproject root so that
//!   the skill file lands in the workspace root that Claude Code actually reads.
//! - Claude/Codex installed instructions are rendered from one shared source
//!   surface; only harness-specific invocation wording and frontmatter
//!   description may differ.
//!
//! ## Agentic Contracts
//! - `install()` and `check()` never require arguments; resolution is automatic.
//! - `install_at(Some(path))` is deterministic and safe for isolated tests.
//! - `install_and_check_updated()` is the preferred call site for startup skill sync;
//!   callers can branch on the bool to print "skill updated" notices.
//! - `check_at` exits the process (code 1) rather than returning `Err` when outdated,
//!   making it safe to call from CI scripts.
//!
//! ## Evals
//! - install_creates_file: fresh temp dir → `.claude/skills/agent-doc/SKILL.md` created with bundled content
//! - install_idempotent: install twice → file content unchanged, no error
//! - install_overwrites_outdated: stale "old content" present → replaced with bundled content
//! - check_not_installed: no SKILL.md present → path does not exist (check would return false)
//! - bundled_skill_is_not_empty: shared skill template length > 0 at compile time
//! - bundled_skill_contains_agent_doc: bundled content references "agent-doc"

use anyhow::{Context, Result};
use std::path::Path;

use agent_kit::skill::SkillConfig;

/// The shared SKILL.md source bundled at build time.
const SKILL_TEMPLATE: &str = include_str!("../SKILL.md");

const CLAUDE_DESCRIPTION: &str = "Interactive document session — respond to user edits in a markdown file. TRIGGER: user invokes /agent-doc <file>. ALL-OF: (1) file is a markdown session document, (2) CLI is installed, (3) write+commit are executed every cycle without exception.";

const CODEX_DESCRIPTION: &str = "Interactive document session for Codex — respond to user edits in a markdown file. TRIGGER: user writes agent-doc <file> as a normal Codex message. ALL-OF: (1) file is a markdown session document, (2) CLI is installed, (3) write+commit are executed every cycle without exception. Do not use slash commands; Codex rejects project-defined /agent-doc.";

const CLAUDE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are Claude Code-specific. Other harnesses receive the document path directly.
"#;

const CODEX_INVOCATION_SECTION: &str = r#"## Invocation

Codex does not support project-defined slash commands. Do **not** type `/agent-doc`; the Codex CLI will reject it before these instructions run.

In Codex, invoke agent-doc by writing one of these as a normal message:

```
agent-doc <FILE>
agent-doc claim <FILE>
agent-doc compact <FILE>
agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

Claude Code slash-command equivalents are `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, and `/agent-doc compact exchange <FILE>`.
"#;

const AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --reload compact`; if output contains `SKILL_RELOAD=compact`, prompt the user to run `/compact` (or equivalent) and re-invoke the skill, then stop. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.";

const CLAUDE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --harness claude --reload compact`; if output contains `SKILL_RELOAD=compact`, prompt the user to run `/compact` and re-invoke the skill, then stop. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.";

const CODEX_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Run `agent-doc --version` and compare against `agent-doc-version` in this file's frontmatter. If the binary is newer, run `agent-doc skill install --harness codex --reload restart`; if output contains `SKILL_RELOAD=restart`, tell the user to restart this Codex session and re-invoke `agent-doc <FILE>`, then stop. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) for harness-specific prompting.";

/// Bundled runbooks installed alongside the skill.
const BUNDLED_RUNBOOKS: &[(&str, &str)] = &[
    (
        "compact-exchange.md",
        include_str!("../runbooks/compact-exchange.md"),
    ),
    (
        "transfer-extract.md",
        include_str!("../runbooks/transfer-extract.md"),
    ),
    ("pending-ops.md", include_str!("../runbooks/pending-ops.md")),
    (
        "model-tier-gate.md",
        include_str!("../runbooks/model-tier-gate.md"),
    ),
    (
        "command-synonyms.md",
        include_str!("../runbooks/command-synonyms.md"),
    ),
    (
        "compound-task-steering.md",
        include_str!("../runbooks/compound-task-steering.md"),
    ),
    (
        "streaming-checkpoints.md",
        include_str!("../runbooks/streaming-checkpoints.md"),
    ),
    (
        "document-format.md",
        include_str!("../runbooks/document-format.md"),
    ),
    ("commit.md", include_str!("../runbooks/commit.md")),
    (
        "code-enforced-directives.md",
        include_str!("../runbooks/code-enforced-directives.md"),
    ),
    (
        "harness-invocation.md",
        include_str!("../runbooks/harness-invocation.md"),
    ),
];

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_USER_PROMPT_COMMAND: &str = "agent-doc hook codex-user-prompt-submit";
const CODEX_STOP_COMMAND: &str = "agent-doc hook codex-stop";

fn config() -> SkillConfig {
    let env = detect_install_env();
    config_for_env(env)
}

fn config_for_env(env: agent_kit::detect::Environment) -> SkillConfig {
    SkillConfig::with_environment("agent-doc", content_for_env(env), VERSION, env)
}

fn content_for_env(env: agent_kit::detect::Environment) -> String {
    use agent_kit::detect::Environment;
    let (description, invocation_section) = match env {
        Environment::Codex => (CODEX_DESCRIPTION, CODEX_INVOCATION_SECTION),
        _ => (CLAUDE_DESCRIPTION, CLAUDE_INVOCATION_SECTION),
    };
    render_skill(env, description, invocation_section)
}

fn render_skill(
    env: agent_kit::detect::Environment,
    description: &str,
    invocation_section: &str,
) -> String {
    let rendered = replace_frontmatter_field(SKILL_TEMPLATE, "description", description)
        .expect("SKILL.md must contain a description field in frontmatter");
    let rendered = replace_frontmatter_field(&rendered, "agent-doc-version", VERSION)
        .expect("SKILL.md must contain an agent-doc-version field in frontmatter");
    let rendered = replace_markdown_section(&rendered, "## Invocation", invocation_section)
        .expect("SKILL.md must contain an ## Invocation section");
    replace_auto_update_line(&rendered, auto_update_line_for_env(env))
        .expect("SKILL.md must contain the auto-update instructions")
}

fn replace_frontmatter_field(content: &str, field: &str, value: &str) -> Option<String> {
    let mut replaced = false;
    let mut in_frontmatter = false;
    let mut rendered = String::new();

    for line in content.lines() {
        if line == "---" {
            in_frontmatter = !in_frontmatter;
            rendered.push_str(line);
            rendered.push('\n');
            continue;
        }

        if in_frontmatter && line.starts_with(&format!("{field}: ")) {
            rendered.push_str(&format!("{field}: \"{value}\"\n"));
            replaced = true;
            continue;
        }

        rendered.push_str(line);
        rendered.push('\n');
    }

    if !content.ends_with('\n') {
        rendered.pop();
    }

    replaced.then_some(rendered)
}

fn replace_markdown_section(content: &str, heading: &str, replacement: &str) -> Option<String> {
    let start = content.find(heading)?;
    let after_heading = &content[start + heading.len()..];
    let next_heading = after_heading.find("\n## ");
    let end = next_heading
        .map(|idx| start + heading.len() + idx + 1)
        .unwrap_or(content.len());

    let mut rendered = String::new();
    rendered.push_str(&content[..start]);
    rendered.push_str(replacement.trim_end());
    rendered.push('\n');
    rendered.push_str(&content[end..]);
    Some(rendered)
}

fn replace_auto_update_line(content: &str, replacement: &str) -> Option<String> {
    content
        .contains(AUTO_UPDATE_LINE)
        .then(|| content.replacen(AUTO_UPDATE_LINE, replacement, 1))
}

fn auto_update_line_for_env(env: agent_kit::detect::Environment) -> &'static str {
    use agent_kit::detect::Environment;
    match env {
        Environment::Codex => CODEX_AUTO_UPDATE_LINE,
        _ => CLAUDE_AUTO_UPDATE_LINE,
    }
}

fn detect_install_env() -> agent_kit::detect::Environment {
    use agent_kit::detect::Environment;

    let detected = Environment::detect();
    if !matches!(detected, Environment::Generic) {
        return detected;
    }

    if std::env::var_os("CODEX_THREAD_ID").is_some() || std::env::var_os("CODEX_CI").is_some() {
        return Environment::Codex;
    }

    detected
}

#[cfg(test)]
fn remove_markdown_section(content: &str, heading: &str) -> String {
    let Some(start) = content.find(heading) else {
        return content.to_string();
    };
    let after_heading = &content[start + heading.len()..];
    let next_heading = after_heading.find("\n## ");
    let end = next_heading
        .map(|idx| start + heading.len() + idx + 1)
        .unwrap_or(content.len());

    let mut rendered = String::new();
    rendered.push_str(&content[..start]);
    rendered.push_str(&content[end..]);
    rendered
}

/// Resolve the project root for skill installation.
///
/// When CWD is inside a git submodule (e.g., `src/agent-doc/`), the skill
/// should be installed to the superproject root, not the submodule. This
/// ensures the SKILL.md that Claude Code reads (from the project root's
/// `.claude/skills/`) matches the binary version.
fn resolve_root() -> Option<std::path::PathBuf> {
    // Try superproject first (handles submodule CWD)
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    // Fall back to git toplevel
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !root.is_empty() {
        return Some(std::path::PathBuf::from(root));
    }

    None
}

/// Install bundled runbooks alongside the skill for the detected environment.
fn install_runbooks(root: Option<&Path>) -> Result<()> {
    let env = detect_install_env();
    install_runbooks_for(env, root)
}

/// Resolve the runbooks directory for a specific environment.
fn runbooks_rel_path(env: &agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        Environment::ClaudeCode => std::path::PathBuf::from(".claude/skills/agent-doc/runbooks"),
        Environment::OpenCode => std::path::PathBuf::from(".opencode/skills/agent-doc/runbooks"),
        Environment::Codex => std::path::PathBuf::from(".codex/runbooks"),
        Environment::Cursor => std::path::PathBuf::from(".cursor/rules/runbooks"),
        Environment::Generic => std::path::PathBuf::from("runbooks"),
    }
}

/// Install bundled runbooks for a specific environment.
fn install_runbooks_for(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let runbooks_dir = base.join(runbooks_rel_path(&env));
    std::fs::create_dir_all(&runbooks_dir)?;
    for (name, content) in BUNDLED_RUNBOOKS {
        let path = runbooks_dir.join(name);
        let needs_write = !path.exists()
            || std::fs::read_to_string(&path)
                .map(|existing| existing != *content)
                .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, content)?;
        }
    }
    Ok(())
}

/// Install bundled runbooks for all environments.
fn install_runbooks_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_runbooks_for(env, root)?;
    }
    Ok(())
}

fn install_env_artifacts(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    if matches!(env, agent_kit::detect::Environment::Codex) {
        install_codex_hook_artifacts(root)?;
    }
    Ok(())
}

fn install_env_artifacts_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_env_artifacts(env, root)?;
    }
    Ok(())
}

fn install_codex_hook_artifacts(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let codex_dir = base.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    merge_codex_hooks_json(&codex_dir.join("hooks.json"))?;
    merge_codex_config(&codex_dir.join("config.toml"))?;
    Ok(())
}

fn merge_codex_hooks_json(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str::<serde_json::Value>(&content)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        serde_json::json!({})
    };

    let hooks = root
        .as_object_mut()
        .context("Codex hooks.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("Codex hooks.json `hooks` must be an object")?;

    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        CODEX_USER_PROMPT_COMMAND,
        Some("Tracking active agent-doc session"),
    );
    ensure_codex_hook_command(
        hooks_map,
        "Stop",
        CODEX_STOP_COMMAND,
        Some("Checking agent-doc completion boundary"),
    );

    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn ensure_codex_hook_command(
    hooks_map: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
    status_message: Option<&str>,
) {
    let entries = hooks_map
        .entry(event.to_string())
        .or_insert_with(|| serde_json::json!([]));
    let Some(entry_array) = entries.as_array_mut() else {
        *entries = serde_json::json!([]);
        return ensure_codex_hook_command(hooks_map, event, command, status_message);
    };

    let already_present = entry_array.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(|hooks| hooks.as_array())
            .map(|hooks| {
                hooks.iter().any(|hook| {
                    hook.get("type").and_then(|v| v.as_str()) == Some("command")
                        && hook.get("command").and_then(|v| v.as_str()) == Some(command)
                })
            })
            .unwrap_or(false)
    });
    if already_present {
        return;
    }

    let mut hook = serde_json::Map::new();
    hook.insert("type".to_string(), serde_json::json!("command"));
    hook.insert("command".to_string(), serde_json::json!(command));
    if let Some(status) = status_message {
        hook.insert("statusMessage".to_string(), serde_json::json!(status));
    }
    entry_array.push(serde_json::json!({ "hooks": [hook] }));
}

fn merge_codex_config(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        toml::from_str::<toml::Value>(&content)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    if !root.is_table() {
        anyhow::bail!("Codex config at {} must be a TOML table", path.display());
    }

    let features = root
        .as_table_mut()
        .expect("checked table")
        .entry("features".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let features_table = features
        .as_table_mut()
        .context("Codex config `features` must be a table")?;
    features_table.insert("codex_hooks".to_string(), toml::Value::Boolean(true));

    std::fs::write(path, toml::to_string_pretty(&root)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
#[allow(dead_code)]
pub fn install_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let env = agent_kit::detect::Environment::detect();
    config().install(resolved.as_deref())?;
    install_runbooks(resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())
}

/// Public entry point (resolves to superproject root, called from main).
#[allow(dead_code)]
pub fn install() -> Result<()> {
    install_at(None)
}

/// Install and return whether the file was actually updated (not just already up to date).
pub fn install_and_check_updated() -> Result<bool> {
    let cfg = config();
    let resolved = resolve_root();
    let path = cfg.skill_path(resolved.as_deref());

    // Check if already up to date before install
    let was_current = path.exists()
        && std::fs::read_to_string(&path)
            .map(|existing| existing == cfg.content)
            .unwrap_or(false);

    cfg.install(resolved.as_deref())?;
    install_runbooks(resolved.as_deref())?;
    install_env_artifacts(detect_install_env(), resolved.as_deref())?;
    Ok(!was_current)
}

/// Install the skill for a specific harness environment.
pub fn install_for(env: agent_kit::detect::Environment) -> Result<()> {
    let resolved = resolve_root();
    config_for_env(env).install_for(env, resolved.as_deref())?;
    install_runbooks_for(env, resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())
}

/// Install the skill for all supported harnesses.
pub fn install_all() -> Result<()> {
    let resolved = resolve_root();
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        config_for_env(env).install_for(env, resolved.as_deref())?;
    }
    install_runbooks_all(resolved.as_deref())?;
    install_env_artifacts_all(resolved.as_deref())
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let up_to_date = config().check(resolved.as_deref())?;
    if !up_to_date {
        std::process::exit(1);
    }
    Ok(())
}

/// Public entry point (resolves to superproject root, called from main).
pub fn check() -> Result<()> {
    check_at(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_kit::detect::Environment;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: test-only process-local env mutation, restored in Drop.
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => {
                    // SAFETY: test-only process-local env mutation, restored to prior value.
                    unsafe { std::env::set_var(self.key, value) };
                }
                None => {
                    // SAFETY: test-only process-local env mutation, restored to prior absence.
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
    }

    #[test]
    fn bundled_skill_is_not_empty() {
        assert!(!SKILL_TEMPLATE.is_empty());
    }

    #[test]
    fn bundled_skill_contains_agent_doc() {
        assert!(SKILL_TEMPLATE.contains("agent-doc"));
    }

    #[test]
    fn bundled_skill_template_contains_auto_update_line() {
        assert!(SKILL_TEMPLATE.contains(AUTO_UPDATE_LINE));
    }

    #[test]
    fn detect_install_env_treats_codex_thread_id_as_codex() {
        let _code = EnvVarGuard::unset("CODEX");
        let _code_cli = EnvVarGuard::unset("CODEX_CLI");
        let _thread = EnvVarGuard::set("CODEX_THREAD_ID", "thread-123");
        let _ci = EnvVarGuard::unset("CODEX_CI");

        assert_eq!(super::detect_install_env(), Environment::Codex);
    }

    /// Use an explicit ClaudeCode environment for deterministic test paths.
    /// Environment::detect() is non-deterministic in CI (depends on env vars).
    fn test_config() -> SkillConfig {
        SkillConfig::with_environment(
            "agent-doc",
            content_for_env(Environment::ClaudeCode),
            VERSION,
            Environment::ClaudeCode,
        )
    }

    /// Resolve expected skill path using the explicit test environment.
    fn expected_path(dir: &std::path::Path) -> std::path::PathBuf {
        test_config().skill_path(Some(dir))
    }

    fn install_test(root: Option<&std::path::Path>) -> anyhow::Result<()> {
        test_config().install(root)
    }

    #[test]
    fn install_creates_file() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        assert!(path.exists(), "skill not found at {}", path.display());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        install_test(Some(dir.path())).unwrap();

        let path = expected_path(dir.path());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn check_not_installed() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        assert!(!path.exists());
    }

    #[test]
    fn install_creates_runbooks_claude() {
        let dir = tempfile::tempdir().unwrap();

        install_test(Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbook_path = dir
            .path()
            .join(".claude/skills/agent-doc/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("Compact Exchange"));
    }

    #[test]
    fn install_creates_runbooks_codex() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let runbook_path = dir.path().join(".codex/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "codex runbook not found at {}",
            runbook_path.display()
        );
    }

    #[test]
    fn installed_harness_runbooks_include_commit_invariant() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(dir.path().join(".codex/runbooks/commit.md")).unwrap();

        for content in [&claude, &codex] {
            assert!(content.contains("Every appended `agent-doc` response must be committed"));
            assert!(content.contains("agent-doc finalize <FILE>"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
    }

    #[test]
    fn installed_harness_pending_ops_runbooks_cover_plan_backed_items() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/runbooks/pending-ops.md")).unwrap();

        for content in [&claude, &codex] {
            assert!(content.contains("create the plan file"));
            assert!(content.contains("include that exact plan"));
            assert!(content.contains("file path in the item text"));
            assert!(content.contains("plan-spec2-rollout.md"));
        }
    }

    #[test]
    fn installed_harness_runbooks_share_manual_repair_rule() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/runbooks/harness-invocation.md"))
                .unwrap();

        for content in [&claude, &codex] {
            assert!(content.contains("## Manual Repair Default"));
            assert!(content.contains("For both **Claude Code** and **Codex**"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
        assert!(codex.contains("agent-doc session-check <FILE>"));
        assert!(codex.contains("Do **not** report success or stop"));
    }

    #[test]
    fn install_for_codex_writes_codex_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();

        let path = dir.path().join(".codex/AGENTS.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("agent-doc skill install --harness codex --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>"));
    }

    #[test]
    fn install_for_codex_writes_hooks_json_and_feature_flag() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks_path = dir.path().join(".codex/hooks.json");
        let config_path = dir.path().join(".codex/config.toml");
        assert!(hooks_path.exists(), "missing {}", hooks_path.display());
        assert!(config_path.exists(), "missing {}", config_path.display());

        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let stop_hooks = hooks["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(CODEX_STOP_COMMAND)
                && hook["type"].as_str() == Some("command")
        }));
        let submit_hooks = hooks["hooks"]["UserPromptSubmit"][0]["hooks"]
            .as_array()
            .unwrap();
        assert!(
            submit_hooks
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_USER_PROMPT_COMMAND))
        );

        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(config["features"]["codex_hooks"].as_bool(), Some(true));
    }

    #[test]
    fn install_for_codex_preserves_existing_hook_and_config_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex/hooks.json"),
            serde_json::json!({
                "hooks": {
                    "Stop": [
                        {
                            "hooks": [
                                { "type": "command", "command": "echo existing-stop" }
                            ]
                        }
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".codex/config.toml"),
            "[sandbox]\ndefault = \"workspace-write\"\n",
        )
        .unwrap();

        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();

        let hooks: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        let stop_hooks = hooks["hooks"]["Stop"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some("echo existing-stop"))
        }));
        assert!(stop_hooks.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hook| hook["command"].as_str() == Some(CODEX_STOP_COMMAND))
        }));

        let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["sandbox"]["default"].as_str(),
            Some("workspace-write")
        );
        assert_eq!(config["features"]["codex_hooks"].as_bool(), Some(true));
    }

    #[test]
    fn install_runbooks_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_all(Some(dir.path())).unwrap();

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".cursor/rules/runbooks/compact-exchange.md")
                .exists()
        );
    }

    #[test]
    fn bundled_skill_contains_harness_preamble() {
        assert!(SKILL_TEMPLATE.contains("Harness Compatibility"));
        assert!(SKILL_TEMPLATE.contains("harness-invocation.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/commit.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/command-synonyms.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/compound-task-steering.md"));
    }

    #[test]
    fn bundled_skill_contains_pending_capture_rules() {
        assert!(SKILL_TEMPLATE.contains("Pending capture rule"));
        assert!(SKILL_TEMPLATE.contains("[recommended]"));
        assert!(SKILL_TEMPLATE.contains("beginning of `agent:pending`"));
        assert!(SKILL_TEMPLATE.contains("adjacent to its predecessor"));
        assert!(SKILL_TEMPLATE.contains("`do #id` closeout rule"));
        assert!(SKILL_TEMPLATE.contains("--pending-done <id>"));
        assert!(SKILL_TEMPLATE.contains("pending_done_guard"));
    }

    #[test]
    fn bundled_skill_treats_imperative_document_edits_as_executable_work() {
        assert!(SKILL_TEMPLATE.contains("Imperative edits are executable directives"));
        assert!(
            SKILL_TEMPLATE.contains("Do not require the same instruction to be repeated in chat")
        );
        assert!(SKILL_TEMPLATE.contains(
            "Do not keep appending \"starting/continuing\" status prose while the requested work remains undone"
        ));
    }

    #[test]
    fn bundled_skill_contains_manual_repair_write_commit_rule() {
        assert!(SKILL_TEMPLATE.contains("Manual repair / missed patchback rule (all harnesses)"));
        assert!(
            SKILL_TEMPLATE
                .contains("do **not** patch the assistant response directly into the file")
        );
        assert!(SKILL_TEMPLATE.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(SKILL_TEMPLATE.contains("bare `agent-doc write`"));
    }

    #[test]
    fn bundled_skill_contains_finalize_commit_invariant() {
        assert!(SKILL_TEMPLATE.contains("agent-doc finalize <FILE>"));
        assert!(
            SKILL_TEMPLATE
                .contains("unless the user explicitly told you to leave the response uncommitted")
        );
        assert!(SKILL_TEMPLATE.contains("requires the cycle to reach `committed`"));
        assert!(SKILL_TEMPLATE.contains("agent-doc session-check <FILE>"));
        assert!(SKILL_TEMPLATE.contains("final document-mutation boundary for the cycle"));
        assert!(SKILL_TEMPLATE.contains(
            "After `finalize` / `write --commit`, do not start more long-running task work"
        ));
    }

    #[test]
    fn bundled_skill_contains_model_short_name_attribution_rule() {
        assert!(SKILL_TEMPLATE.contains("### Re: topic — gpt-5"));
        assert!(SKILL_TEMPLATE.contains("### Re: topic — opus-4-6"));
        assert!(SKILL_TEMPLATE.contains("Never use the harness label (`codex`, `claude`)"));
    }

    #[test]
    fn bundled_skill_requires_oldest_first_exchange_tail_reconciliation() {
        assert!(SKILL_TEMPLATE.contains("Do not stop at the newest question"));
        assert!(SKILL_TEMPLATE.contains("each unresolved prompt in that tail"));
    }

    #[test]
    fn codex_content_uses_plain_text_invocation() {
        let content = super::content_for_env(Environment::Codex);

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("agent-doc skill install --harness codex --reload restart"));
        assert!(content.contains("SKILL_RELOAD=restart"));
        assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(content.contains("final document-mutation boundary for the cycle"));
        assert!(content.contains("do not start more long-running task work for that same turn"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains("UserPromptSubmit"));
        assert!(content.contains("last_assistant_message"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
        assert!(content.contains("Imperative edits are executable directives"));
        assert!(content.contains("Do not require the same instruction to be repeated in chat"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>"));
    }

    #[test]
    fn claude_content_keeps_slash_invocation() {
        let content = super::content_for_env(Environment::ClaudeCode);

        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("TRIGGER: user invokes /agent-doc <file>"));
        assert!(content.contains("agent-doc skill install --harness claude --reload compact"));
        assert!(content.contains("SKILL_RELOAD=compact"));
    }

    #[test]
    fn generated_harness_content_shares_hot_path_outside_invocation() {
        let claude = super::content_for_env(Environment::ClaudeCode);
        let codex = super::content_for_env(Environment::Codex);

        let claude_shared = super::remove_markdown_section(&claude, "## Invocation");
        let codex_shared = super::remove_markdown_section(&codex, "## Invocation");
        let claude_shared = claude_shared.replace(CLAUDE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let codex_shared = codex_shared.replace(CODEX_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");

        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            codex_shared.replace(CODEX_DESCRIPTION, "<DESC>")
        );
    }

    #[test]
    fn bundled_runbooks_include_harness_invocation() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "harness-invocation.md"),
            "harness-invocation.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_commit_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "commit.md"),
            "commit.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_command_synonyms_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "command-synonyms.md"),
            "command-synonyms.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "compound-task-steering.md"),
            "compound-task-steering.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn harness_invocation_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");
        assert!(content.contains("## Directive Semantics"));
        assert!(content.contains(
            "Imperative user edits inside an `agent-doc` session document are executable directives"
        ));
        assert!(content.contains("Do **not** emit status-only progress prose while doing neither"));
        assert!(content.contains("## Manual Repair Default"));
        assert!(content.contains("For both **Claude Code** and **Codex**"));
        assert!(content.contains("Claude Code"));
        assert!(content.contains("Codex"));
        assert!(content.contains("Harness Detection"));
        assert!(content.contains("Response Header Attribution"));
        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("### Re: topic — gpt-5"));
        assert!(content.contains("### Re: topic — opus-4-6"));
        assert!(content.contains("### Re: topic — codex"));
        assert!(content.contains("### Re: topic — claude"));
        assert!(content.contains("Manual repair / missed patchback"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("agent-doc session-check <FILE>"));
        assert!(
            content.contains(
                "Do not patch the document early and then keep working for the same turn"
            )
        );
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains("UserPromptSubmit"));
        assert!(content.contains("agent-doc hook codex-stop"));
    }

    #[test]
    fn commit_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "commit.md")
            .expect("commit.md not found");
        assert!(content.contains("Every appended `agent-doc` response must be committed"));
        assert!(content.contains("agent-doc finalize <FILE>"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_pending_capture_rules() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("beginning of the list"));
        assert!(content.contains("[recommended]"));
        assert!(content.contains("preserve the order you presented them in"));
        assert!(content.contains("follow-on step from an ordered batch"));
        assert!(content.contains("--pending-reorder gkke,9pw9,step3"));
        assert!(content.contains("Existing `do #id` work that completed this cycle"));
        assert!(content.contains("--pending-done <id>"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_custom_id_docs() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("id=<custom>"));
        assert!(content.contains("id=#spec1"));
        assert!(content.contains("ASCII alphanumeric"));
    }

    #[test]
    fn pending_ops_runbook_content_covers_plan_backed_items() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("create the plan file"));
        assert!(content.contains("include that exact plan"));
        assert!(content.contains("file path in the item text"));
        assert!(content.contains("plan-spec2-rollout.md"));
    }

    #[test]
    fn command_synonyms_runbook_content_covers_orchestrate_modes() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "command-synonyms.md")
            .expect("command-synonyms.md not found");
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode parallel"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode dag"));
        assert!(content.contains("Run these in order"));
        assert!(content.contains("fan out"));
        assert!(content.contains("after X do Y"));
        assert!(content.contains("default to `--mode sequential`"));
    }

    #[test]
    fn compound_task_steering_runbook_covers_explicit_normalization() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains("do #ntoc. Add to today's news. commit + push"));
        assert!(content.contains("agent-doc orchestrate <FILE> --mode sequential"));
        assert!(content.contains("Do not invent binary-owned prose grammar"));
        assert!(content.contains("commit + push"));
        assert!(content.contains("Preserve it. Do not rewrite explicit orchestration"));
    }

    #[test]
    fn install_overwrites_outdated() {
        let dir = tempfile::tempdir().unwrap();

        let path = expected_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old content").unwrap();

        install_test(Some(dir.path())).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, content_for_env(Environment::ClaudeCode));
    }

    #[test]
    fn installed_harness_skill_content_shares_completion_boundary_text() {
        let dir = tempfile::tempdir().unwrap();

        super::config_for_env(Environment::ClaudeCode)
            .install_for(Environment::ClaudeCode, Some(dir.path()))
            .unwrap();
        super::config_for_env(Environment::Codex)
            .install_for(Environment::Codex, Some(dir.path()))
            .unwrap();

        let claude =
            std::fs::read_to_string(dir.path().join(".claude/skills/agent-doc/SKILL.md")).unwrap();
        let codex = std::fs::read_to_string(dir.path().join(".codex/AGENTS.md")).unwrap();

        for content in [&claude, &codex] {
            assert!(content.contains("agent-doc finalize <FILE>"));
            assert!(content.contains("Use `agent-doc write --commit <FILE>`"));
            assert!(content.contains("requires the cycle to reach `committed`"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("final document-mutation boundary for the cycle"));
            assert!(content.contains("Imperative edits are executable directives"));
            assert!(content.contains("Never use the harness label (`codex`, `claude`)"));
            assert!(content.contains("Agent harnesses own full-suite verification"));
            assert!(content.contains("Do not rely on a pre-commit hook"));
        }
        assert!(claude.contains("final document-mutation boundary for the cycle"));
        assert!(codex.contains(".codex/hooks.json"));
        assert!(codex.contains("last_assistant_message"));
    }
}
