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
//! - Claude/Codex/OpenCode installed instructions are rendered from one shared source
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

const CLAUDE_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes /agent-doc <file>. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const CODEX_DESCRIPTION: &str = "Interactive markdown session for Codex. TRIGGER: user writes agent-doc <file> as a normal Codex message. Requires a markdown session document, installed CLI, and write+commit every cycle. Do not use slash commands; Codex rejects project-defined /agent-doc.";

const OPENCODE_DESCRIPTION: &str = "Interactive markdown session for OpenCode. TRIGGER: user invokes /agent-doc <file> command. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const GENERIC_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes the harness-native agent-doc entrypoint. Requires a markdown session document, installed CLI, and write+commit every cycle.";

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

const OPENCODE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Slash commands (`/agent-doc`) are installed as an OpenCode command via `agent-doc skill install --harness opencode`. Other harnesses receive the document path directly.
"#;

const GENERIC_INVOCATION_SECTION: &str = r#"## Invocation

Use the harness-native `agent-doc` entrypoint for the environment you are in:

- Claude Code: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`
- Codex: `agent-doc <FILE>`, `agent-doc claim <FILE>`, `agent-doc compact <FILE>`, `agent-doc compact exchange <FILE>`
- OpenCode: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`

Arguments: `FILE` — path to the session document (e.g., `plan.md`).

**Note:** Codex does not support project-defined slash commands. If this root file disagrees with the installed harness-specific instructions, treat the installed harness file as authoritative for the current turn.
"#;

const AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run the active-harness install: Claude Code `agent-doc skill install --harness claude --reload restart` unless `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`; Codex `agent-doc skill install --harness codex --reload restart`; OpenCode `agent-doc skill install --harness opencode`; other harnesses `agent-doc skill install`. If install says already up to date, treat this file as stale duplicate instructions, use installed harness instructions, and continue with the task. Stop only on a real `SKILL_RELOAD=...`; see [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const CLAUDE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness claude --reload restart` unless `agent_doc_auto_compact` is explicitly set in frontmatter or `.agent-doc/config.toml`. On `SKILL_RELOAD=restart`, ask the user to restart Claude Code and re-invoke `/agent-doc <FILE>`, then stop. Use `--reload compact` and ask for `/compact` only when that explicit opt-in exists. If already up to date, treat as stale instruction drift, continue this turn, and use the installed Claude skill. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const CODEX_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness codex --reload restart`; on `SKILL_RELOAD=restart`, tell the user to restart this Codex session and re-invoke `agent-doc <FILE>`, then stop. If already up to date, treat as stale instruction drift, continue this turn, and use the installed Codex instructions. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

const OPENCODE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`. If newer, run `agent-doc skill install --harness opencode`; if it says already up to date, treat as stale instruction drift, continue this turn, and use the installed OpenCode skill. If `agent-doc` is missing or versions match, skip. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md).";

/// Claude-Code-only auto-loop instruction appended to the rendered SKILL.md.
///
/// Codex has a `Stop` hook in `.codex/hooks.json` that blocks final-answer
/// completion and instructs the active turn to continue the next queue head
/// in-pane when the queue is still active. Claude Code has no equivalent Stop
/// hook, so without this instruction the operator has to retype `agent-doc
/// <FILE>` after every cycle to drain the queue.
///
/// The instruction tells the Claude Code agent to invoke `/loop agent-doc
/// <FILE>` via the `Skill` tool after a successful closeout when the queue is
/// still active. `/loop` drives the next cycle on its self-paced cadence,
/// terminating naturally when the queue drains, when the user types a non-
/// queue prompt, when `agent_doc_queue_max_iterations` is hit, or when the
/// environment hard-cap (`AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP`, default
/// 50) is exceeded.
///
/// Plan: tasks/agent-doc/plan-claude-code-queue-auto-loop.md
const CLAUDE_QUEUE_AUTO_LOOP_SECTION: &str = r#"
## Auto-loop while queue is active (Claude Code)

After a successful `agent-doc finalize` / `agent-doc write --commit` cycle whose `agent-doc session-check` returns OK, check preflight's queue fields:

- `preflight.queue_active == true`
- `preflight.queue_trigger == "auto"` **or** `preflight.queue_trigger == "persisted"` — `auto` is a start trigger only; once the queue is active, a persisted-active queue (`queue_active: true` with no `auto` attribute) is equally continuation-eligible (`#active-queue-persisted-no-continue`). Do not require the `auto` attribute to keep draining an already-active queue.
- `preflight.queue_prompts.len() >= 1`
- `preflight.user_intent_prompt_changes` is empty (a real user prompt mid-loop takes precedence; do NOT auto-loop over it). Managed-component state edits — queue activity toggle, queue item add/strike, backlog/review/done item edits, `queue_active:` frontmatter flip — appear in `prompt_bearing_changes` for compatibility but are filtered out of `user_intent_prompt_changes` so routine session bookkeeping does not block the auto-loop. Likewise, an edit the affectedness classifier scopes as independent of the current turn (`op_affectedness.turn_affected == false`, `#queue-no-stop-unrelated-edit`) is filtered out — an edit unrelated to the active turn never halts the drain; only a real user prompt, which edits the in-scope `exchange` tail and classifies as turn-affecting, preempts.

When all four hold, first run `agent-doc drain-claim <FILE>` to claim the drain-owner lease, then invoke the `Skill` tool with `skill: "loop"` and `args: "agent-doc <FILE>"` to drive the next cycle from the same Claude Code session. The `drain-claim` step (#kp5z / #qflood) tells the supervisor idle-queue watch that `/loop` owns this drain, so it defers instead of *also* injecting `agent-doc <FILE>` into the input queue and flooding it with duplicate triggers. The lease is short-TTL and self-expiring — if the loop stops, the supervisor resumes draining on its own; you never need to release it manually. `/loop` self-paces the next invocation and terminates naturally when the queue drains, when the user interrupts, when `agent_doc_queue_max_iterations` (frontmatter or `.agent-doc/config.toml`) is hit, or when the environment hard-cap `AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP` (default `50`) is exceeded.

Skip the auto-loop on any failed closeout, `session-check` interruption, or `lint-gate` block — those need explicit operator attention. Skip when `preflight.queue_active == false` (queue drained or halted).

This section is Claude-Code-specific. Codex auto-loops via its `Stop` hook in `.codex/hooks.json`; OpenCode currently has no auto-loop. See [runbooks/harness-invocation.md](runbooks/harness-invocation.md) and `tasks/agent-doc/plan-claude-code-queue-auto-loop.md`.
"#;

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
        "planning-dispatch.md",
        include_str!("../runbooks/planning-dispatch.md"),
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
    (
        "split-spec-files.md",
        include_str!("../runbooks/split-spec-files.md"),
    ),
    (
        "manual-job-packets.md",
        include_str!("../runbooks/manual-job-packets.md"),
    ),
    (
        "baseline-drift.md",
        include_str!("../runbooks/baseline-drift.md"),
    ),
    ("respond.md", include_str!("../runbooks/respond.md")),
    (
        "persist-closeout.md",
        include_str!("../runbooks/persist-closeout.md"),
    ),
];

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_USER_PROMPT_COMMAND: &str = "agent-doc hook codex-user-prompt-submit";
const CODEX_STOP_COMMAND: &str = "agent-doc hook codex-stop";
const CODEX_MCP_SERVER_NAME: &str = "agent-doc";
const CODEX_MCP_COMMAND: &str = "agent-doc";

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
        Environment::OpenCode => (OPENCODE_DESCRIPTION, OPENCODE_INVOCATION_SECTION),
        Environment::Generic => (GENERIC_DESCRIPTION, GENERIC_INVOCATION_SECTION),
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
    let mut rendered = replace_auto_update_line(&rendered, auto_update_line_for_env(env))
        .expect("SKILL.md must contain the auto-update instructions");
    if is_claude_environment(env) {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        rendered.push_str(CLAUDE_QUEUE_AUTO_LOOP_SECTION.trim_start_matches('\n'));
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
    }
    rendered
}

fn is_claude_environment(env: agent_kit::detect::Environment) -> bool {
    use agent_kit::detect::Environment;
    !matches!(
        env,
        Environment::Codex | Environment::OpenCode | Environment::Generic
    )
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
        Environment::OpenCode => OPENCODE_AUTO_UPDATE_LINE,
        Environment::Generic => AUTO_UPDATE_LINE,
        _ => CLAUDE_AUTO_UPDATE_LINE,
    }
}

fn looks_like_managed_root_agents(content: &str) -> bool {
    content.contains("user-invocable: true")
        && content.contains("agent-doc-version:")
        && content.contains("# agent-doc")
        && content.contains("## Harness Compatibility")
        && content.contains("## Workflow")
}

fn normalized_managed_instruction_surface_for_audit(content: &str) -> String {
    strip_tsift_code_navigation_block(content)
        .trim_end()
        .to_string()
}

fn has_tsift_code_navigation_block(content: &str) -> bool {
    content.contains("<!-- tsift:code-navigation")
        && content.contains("<!-- /tsift:code-navigation -->")
}

fn extract_tsift_code_navigation_block(content: &str) -> Option<String> {
    const START: &str = "<!-- tsift:code-navigation";
    const END: &str = "<!-- /tsift:code-navigation -->";
    let start = content.find(START)?;
    let relative_end = content[start..].find(END)?;
    let mut end = start + relative_end + END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    Some(content[start..end].trim_matches('\n').to_string())
}

fn strip_tsift_code_navigation_block(content: &str) -> String {
    const START: &str = "<!-- tsift:code-navigation";
    const END: &str = "<!-- /tsift:code-navigation -->";
    let Some(start) = content.find(START) else {
        return content.to_string();
    };
    let Some(relative_end) = content[start..].find(END) else {
        return content.to_string();
    };
    let mut end = start + relative_end + END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    let mut rendered = String::new();
    rendered.push_str(content[..start].trim_end());
    rendered.push('\n');
    rendered.push_str(content[end..].trim_start_matches('\n'));
    rendered
}

fn sync_managed_root_agents(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let path = base.join(agent_kit::detect::Environment::Generic.skill_rel_path("agent-doc"));

    if !path.exists() {
        return Ok(());
    }

    let existing =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if !looks_like_managed_root_agents(&existing) {
        return Ok(());
    }

    let mut rendered = content_for_env(agent_kit::detect::Environment::Generic);
    if let Some(tsift_block) = extract_tsift_code_navigation_block(&existing) {
        rendered = format!("{}\n\n{}\n", rendered.trim_end(), tsift_block);
    }
    if existing == rendered {
        return Ok(());
    }

    std::fs::write(&path, rendered).with_context(|| format!("write {}", path.display()))?;
    eprintln!(
        "[Generic] refreshed managed AGENTS.md mirror → {}",
        path.display()
    );
    Ok(())
}

pub(crate) fn audit_managed_instruction_surfaces(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    for env in [
        agent_kit::detect::Environment::Generic,
        agent_kit::detect::Environment::OpenCode,
        agent_kit::detect::Environment::Codex,
        agent_kit::detect::Environment::ClaudeCode,
    ] {
        let path = base.join(env.skill_rel_path("agent-doc"));
        if !path.exists() {
            continue;
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if !looks_like_managed_root_agents(&existing) {
            continue;
        }
        let expected = content_for_env(env);
        let normalized_existing = normalized_managed_instruction_surface_for_audit(&existing);
        let normalized_expected = normalized_managed_instruction_surface_for_audit(&expected);
        if normalized_existing != normalized_expected {
            if matches!(env, agent_kit::detect::Environment::Generic)
                && has_tsift_code_navigation_block(&existing)
            {
                continue;
            }
            anyhow::bail!(
                "managed agent-doc instruction surface is stale: {}. Run `agent-doc skill install --all` or reinstall the active harness before release.",
                path.display()
            );
        }
    }
    Ok(())
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
///
/// Reconciles the target directory against the canonical embedded set:
/// - Writes missing or out-of-date `.md` files from `BUNDLED_RUNBOOKS`.
/// - Reaps any `.md` files in the target directory that are not in the
///   canonical set, so the on-disk set converges to the embedded set across
///   upgrades (renames, deletions, splits).
///
/// Non-`.md` files are left alone — only the install-managed runbook surface
/// is reconciled.
fn install_runbooks_for(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let runbooks_dir = base.join(runbooks_rel_path(&env));
    std::fs::create_dir_all(&runbooks_dir)?;

    let canonical_names: std::collections::HashSet<&str> =
        BUNDLED_RUNBOOKS.iter().map(|(name, _)| *name).collect();

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

    let entries = std::fs::read_dir(&runbooks_dir)
        .with_context(|| format!("read runbooks dir {}", runbooks_dir.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("read runbooks dir entry under {}", runbooks_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect runbook entry {}", entry.path().display()))?;
        if !file_type.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if canonical_names.contains(file_name) {
            continue;
        }
        std::fs::remove_file(&path)
            .with_context(|| format!("reap stale runbook {}", path.display()))?;
        eprintln!("[{}] reaped stale runbook → {}", env, path.display());
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
    if matches!(env, agent_kit::detect::Environment::OpenCode) {
        install_opencode_command_file(root)?;
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
    merge_codex_config(&codex_dir.join("config.toml"), &base)?;
    Ok(())
}

const OPENCODE_COMMAND_CONTENT: &str = "\
---
description: \"Interactive document session with agent-doc\"
---

agent-doc $ARGUMENTS
";

fn install_opencode_command_file(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let commands_dir = base.join(".opencode/commands");
    std::fs::create_dir_all(&commands_dir)?;
    let path = commands_dir.join("agent-doc.md");
    let needs_write = !path.exists()
        || std::fs::read_to_string(&path)
            .map(|existing| existing != OPENCODE_COMMAND_CONTENT)
            .unwrap_or(true);
    if needs_write {
        std::fs::write(&path, OPENCODE_COMMAND_CONTENT)
            .with_context(|| format!("write {}", path.display()))?;
    }
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

const TURN_STATUS_ACTIVE_COMMAND: &str = "agent-doc turn-status active";
const TURN_STATUS_IDLE_COMMAND: &str = "agent-doc turn-status idle";

/// Install the Claude Code hooks that drive the turn-in-progress pane monitor
/// (`#claude-busy-status-during-active-turn`) into a `settings.json`. Idempotent
/// and additive (existing hooks, including the SessionStart autoclaim hook, are
/// preserved).
///
/// `UserPromptSubmit` → `turn-status active` (turn start); `Stop` **and**
/// `SessionStart` → `turn-status idle`. The double clear is the reliability
/// belt-and-suspenders: the Stop hook here does only trivial, idempotent,
/// best-effort work (clear a pane title) — unlike the consequential Codex closeout
/// Stop hook — so a missed Stop can only leave a stale cosmetic title, and the
/// SessionStart clear self-heals it at the next session start.
/// OpenCode plugin that drives the monitor: `chat.message` (a new user message =
/// turn start) → `turn-status active`; the `session.idle` bus event (turn end) →
/// `turn-status idle`. Best-effort — `agent-doc turn-status` no-ops outside tmux.
const OPENCODE_TURN_STATUS_PLUGIN: &str = r#"// agent-doc turn-in-progress pane monitor (#claude-busy-status-during-active-turn).
// Auto-installed by `agent-doc turn-status install`. Sets the agent's own tmux
// pane border title on turn start (chat.message) and clears it on turn end
// (session.idle). Best-effort: agent-doc turn-status no-ops outside tmux.
export const AgentDocTurnStatus = async ({ $ }) => ({
  "chat.message": async () => {
    try { await $`agent-doc turn-status active` } catch {}
  },
  event: async ({ event }) => {
    if (event.type === "session.idle") {
      try { await $`agent-doc turn-status idle` } catch {}
    }
  },
})
"#;

/// Install the turn-in-progress pane monitor for all three harnesses
/// (#claude-busy-status-during-active-turn). Idempotent + additive per harness.
/// `base`, when set, roots every harness path under it (project-relative; used by
/// tests and `--dir`). Otherwise `user` selects each harness's user config dir,
/// else the project-local (cwd) paths.
pub fn install_turn_status_hooks(base: Option<&Path>, user: bool, tmux: bool) -> Result<()> {
    // Claude — settings.json: UserPromptSubmit/Stop/SessionStart.
    let claude = turn_status_path(base, user, ".claude/settings.json", ".claude/settings.json");
    ensure_parent_dir(&claude)?;
    merge_claude_turn_status_hooks(&claude)?;
    println!("[turn-status] claude   -> {}", claude.display());

    // Codex — hooks.json: UserPromptSubmit/Stop (Codex has no SessionStart event).
    let codex = turn_status_path(base, user, ".codex/hooks.json", ".codex/hooks.json");
    ensure_parent_dir(&codex)?;
    merge_codex_turn_status_hooks(&codex)?;
    println!("[turn-status] codex    -> {}", codex.display());

    // OpenCode — plugin file: chat.message + session.idle (no JSON hook events).
    let opencode = turn_status_path(
        base,
        user,
        ".opencode/plugin/agent-doc-turn-status.js",
        ".config/opencode/plugin/agent-doc-turn-status.js",
    );
    ensure_parent_dir(&opencode)?;
    std::fs::write(&opencode, OPENCODE_TURN_STATUS_PLUGIN)
        .with_context(|| format!("write {}", opencode.display()))?;
    println!("[turn-status] opencode -> {}", opencode.display());

    println!("  start (UserPromptSubmit / chat.message)   -> {TURN_STATUS_ACTIVE_COMMAND}");
    println!("  end   (Stop / SessionStart / session.idle) -> {TURN_STATUS_IDLE_COMMAND}");

    // Visibility: the pane-border title is only shown when tmux `pane-border-status`
    // is enabled. `--tmux` applies it to the running server immediately (safe,
    // non-destructive); persisting it stays a recommendation because tmux config
    // paths are user-specific (symlinks / dotfiles repos) and risky to auto-edit.
    if tmux {
        let applied = std::process::Command::new("tmux")
            .args(["set", "-g", "pane-border-status", "top"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if applied {
            println!("  tmux: applied `pane-border-status top` to the running server");
        } else {
            println!(
                "  tmux: could not apply `pane-border-status` (no running server / not in tmux)"
            );
        }
        println!("  tmux: to persist, add to your tmux config: set -g pane-border-status top");
    } else {
        println!(
            "  visibility: enable the pane border to see it — `agent-doc turn-status install --tmux` (applies now), or add `set -g pane-border-status top` to your tmux config"
        );
    }
    Ok(())
}

fn turn_status_path(
    base: Option<&Path>,
    user: bool,
    project_rel: &str,
    user_rel: &str,
) -> std::path::PathBuf {
    if let Some(b) = base {
        return b.join(project_rel);
    }
    if user && let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home).join(user_rel);
    }
    std::path::PathBuf::from(project_rel)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    Ok(())
}

/// Merge the turn-status hooks into a Codex `hooks.json`. Same shape as the
/// Claude settings.json; Codex has no `SessionStart` event so only the start/end
/// pair is wired (a missed Stop self-heals when the next `UserPromptSubmit`
/// re-asserts the active title).
fn merge_codex_turn_status_hooks(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<serde_json::Value>(&content)
                .with_context(|| format!("parse {}", path.display()))?
        }
    } else {
        serde_json::json!({})
    };
    let hooks = root
        .as_object_mut()
        .context("hooks.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("hooks.json `hooks` must be an object")?;
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        TURN_STATUS_ACTIVE_COMMAND,
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None);
    let rendered = serde_json::to_string_pretty(&root)?;
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Merge the turn-status hooks into a Claude `settings.json`. Claude's hook shape
/// matches Codex `hooks.json`, so this reuses the same idempotent merge helper.
fn merge_claude_turn_status_hooks(path: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        if content.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<serde_json::Value>(&content)
                .with_context(|| format!("parse {}", path.display()))?
        }
    } else {
        serde_json::json!({})
    };

    let hooks = root
        .as_object_mut()
        .context("settings.json root must be an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_map = hooks
        .as_object_mut()
        .context("settings.json `hooks` must be an object")?;

    // Shared shape with Codex hooks.json — `ensure_codex_hook_command` is
    // harness-agnostic (a command-hook merger), reused here for Claude.
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        TURN_STATUS_ACTIVE_COMMAND,
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None);
    ensure_codex_hook_command(hooks_map, "SessionStart", TURN_STATUS_IDLE_COMMAND, None);

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

fn merge_codex_config(path: &Path, project_root: &Path) -> Result<()> {
    let mut root = if path.exists() {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        toml::from_str::<toml::Value>(&content)
            .with_context(|| format!("parse {}", path.display()))?
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let project_root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.into());

    if !root.is_table() {
        anyhow::bail!("Codex config at {} must be a TOML table", path.display());
    }

    let root_table = root.as_table_mut().expect("checked table");
    let features = root_table
        .entry("features".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let features_table = features
        .as_table_mut()
        .context("Codex config `features` must be a table")?;
    features_table.remove("codex_hooks");
    features_table.insert("hooks".to_string(), toml::Value::Boolean(true));

    let mcp_servers = root_table
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let mcp_servers_table = mcp_servers
        .as_table_mut()
        .context("Codex config `mcp_servers` must be a table")?;
    let mut server = toml::map::Map::new();
    server.insert(
        "command".to_string(),
        toml::Value::String(CODEX_MCP_COMMAND.to_string()),
    );
    server.insert(
        "args".to_string(),
        toml::Value::Array(vec![
            toml::Value::String("mcp".to_string()),
            toml::Value::String("serve".to_string()),
            toml::Value::String("--project-root".to_string()),
            toml::Value::String(project_root.display().to_string()),
        ]),
    );
    mcp_servers_table.insert(
        CODEX_MCP_SERVER_NAME.to_string(),
        toml::Value::Table(server),
    );

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
    install_env_artifacts(env, resolved.as_deref())?;
    sync_managed_root_agents(resolved.as_deref())
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
    sync_managed_root_agents(resolved.as_deref())?;
    Ok(!was_current)
}

/// Install the skill for a specific harness environment.
pub fn install_for(env: agent_kit::detect::Environment) -> Result<()> {
    let resolved = resolve_root();
    config_for_env(env).install_for(env, resolved.as_deref())?;
    install_runbooks_for(env, resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    sync_managed_root_agents(resolved.as_deref())
}

/// Install the skill for all supported harnesses.
pub fn install_all() -> Result<()> {
    let resolved = resolve_root();
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        config_for_env(env).install_for(env, resolved.as_deref())?;
    }
    install_runbooks_all(resolved.as_deref())?;
    install_env_artifacts_all(resolved.as_deref())?;
    sync_managed_root_agents(resolved.as_deref())
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
mod tests;
