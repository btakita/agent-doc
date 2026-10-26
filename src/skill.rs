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
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[path = "skill_grok.rs"]
pub mod grok;

/// The shared SKILL.md source bundled at build time.
const SKILL_TEMPLATE: &str = include_str!("../SKILL.md");

const CLAUDE_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes /agent-doc <file>, or a bare `/agent-doc` (the file this tmux pane claims). Requires a markdown session document, installed CLI, and write+commit every cycle.";

const CODEX_DESCRIPTION: &str = "Interactive markdown session for Codex. TRIGGER: user writes agent-doc <file> (or a bare `agent-doc`, for the file this tmux pane claims) as a normal Codex message. Requires a markdown session document, installed CLI, and write+commit every cycle. Do not use slash commands; Codex rejects project-defined /agent-doc.";

const OPENCODE_DESCRIPTION: &str = "Interactive markdown session for OpenCode. TRIGGER: user invokes /agent-doc <file> command, or a bare `/agent-doc` (the file this tmux pane claims). Requires a markdown session document, installed CLI, and write+commit every cycle.";

const GENERIC_DESCRIPTION: &str = "Interactive markdown session. TRIGGER: user invokes the harness-native agent-doc entrypoint. Requires a markdown session document, installed CLI, and write+commit every cycle.";

const CLAUDE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`). With no FILE, `/agent-doc` runs on the document this tmux pane claims; the hook resolves it and the admitted context names it (`no FILE argument: this turn's FILE is ...`). Use that path as FILE. A refusal (not in tmux, no claim, several claims) is reported, never guessed.

**Note:** Slash commands (`/agent-doc`) are Claude Code-specific. Other harnesses receive the document path directly.
"#;

const CODEX_INVOCATION_SECTION: &str = r#"## Invocation

Codex does not support project-defined slash commands. Do **not** type `/agent-doc`; the Codex CLI will reject it before these instructions run.

In Codex, invoke agent-doc by writing one of these as a normal message:

```
agent-doc <FILE>
agent-doc
agent-doc claim <FILE>
agent-doc compact <FILE>
agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`). With no FILE, `agent-doc` runs on the document this tmux pane claims; the hook resolves it and the admitted context names it (`no FILE argument: this turn's FILE is ...`). Use that path as FILE. A refusal (not in tmux, no claim, several claims) is reported, never guessed.

Claude Code slash-command equivalents are `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, and `/agent-doc compact exchange <FILE>`.
"#;

const OPENCODE_INVOCATION_SECTION: &str = r#"## Invocation

```
/agent-doc <FILE>
/agent-doc
/agent-doc claim <FILE>
/agent-doc compact <FILE>
/agent-doc compact exchange <FILE>
```

Arguments: `FILE` — path to the session document (e.g., `plan.md`). With no FILE, run `agent-doc document` first and use the path it prints as FILE for the whole cycle. If it exits nonzero (not in tmux, no claim, several claims), report its reason and stop; never guess a document.

**Note:** Slash commands (`/agent-doc`) are installed as an OpenCode command via `agent-doc skill install --harness opencode`. Other harnesses receive the document path directly.
"#;

const GENERIC_INVOCATION_SECTION: &str = r#"## Invocation

Use the harness-native `agent-doc` entrypoint for the environment you are in:

- Claude Code: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`
- Codex: `agent-doc <FILE>`, `agent-doc claim <FILE>`, `agent-doc compact <FILE>`, `agent-doc compact exchange <FILE>`
- OpenCode: `/agent-doc <FILE>`, `/agent-doc claim <FILE>`, `/agent-doc compact <FILE>`, `/agent-doc compact exchange <FILE>`

Arguments: `FILE` — path to the session document (e.g., `plan.md`). With no FILE (`/agent-doc`, or `agent-doc` in Codex), the cycle runs on the document this tmux pane claims. Claude Code and Codex hooks resolve it and name it in the admitted context; connected admission (`agent_doc_admit` without `file`) returns it as `file`; other harnesses run `agent-doc document` and use the printed path. A refusal is reported, never guessed.

**Note:** Codex does not support project-defined slash commands. If this root file disagrees with the installed harness-specific instructions, treat the installed harness file as authoritative for the current turn.
"#;

const AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`; if the binary is newer, follow the active-harness update procedure in [runbooks/harness-invocation.md](runbooks/harness-invocation.md), then continue as that runbook directs.";

const CLAUDE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`; if newer, follow the Claude update procedure in [runbooks/harness-invocation.md](runbooks/harness-invocation.md), including any emitted reload boundary.";

const CODEX_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`; if newer, follow the Codex update procedure in [runbooks/harness-invocation.md](runbooks/harness-invocation.md), re-read the installed skill after a real update, and continue the same turn.";

const OPENCODE_AUTO_UPDATE_LINE: &str = "**Auto-update skill:** Compare `agent-doc --version` to `agent-doc-version`; if newer, follow the OpenCode update procedure in [runbooks/harness-invocation.md](runbooks/harness-invocation.md), then continue as directed.";

/// Claude-Code-only auto-loop instruction appended to the rendered SKILL.md.
///
/// Codex has a `Stop` hook in user-level `$CODEX_HOME/hooks.json` that blocks final-answer
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
## Claude queue continuation

When queue continuation or supervisor handoff fields are present, read [runbooks/queue-auto-loop.md](runbooks/queue-auto-loop.md) completely and follow its Claude Code branch. It owns the submitted `/loop agent-doc <FILE>` protocol, drainability fields, subagent claim fencing, iteration caps, and exhaustive stop conditions.
"#;

/// Bundled runbooks installed alongside the skill.
const BUNDLED_RUNBOOKS: &[(&str, &str)] = &[
    (
        "session-cycle.md",
        include_str!("../runbooks/session-cycle.md"),
    ),
    (
        "queue-auto-loop.md",
        include_str!("../runbooks/queue-auto-loop.md"),
    ),
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
        "jb-cache-conflict.md",
        include_str!("../runbooks/jb-cache-conflict.md"),
    ),
    (
        "code-enforced-directives.md",
        include_str!("../runbooks/code-enforced-directives.md"),
    ),
    (
        "harness-invocation.md",
        include_str!("../runbooks/harness-invocation.md"),
    ),
    (
        "coder-workspace.md",
        include_str!("../runbooks/coder-workspace.md"),
    ),
    (
        "multi-tmux-sessions.md",
        include_str!("../runbooks/multi-tmux-sessions.md"),
    ),
    (
        "dynamic-context.md",
        include_str!("../runbooks/dynamic-context.md"),
    ),
    (
        "preset-runbooks.md",
        include_str!("../runbooks/preset-runbooks.md"),
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
    (
        "describe-image.md",
        include_str!("../runbooks/describe-image.md"),
    ),
];

/// Runbook mirror directories that predate the managed harness skill directories
/// (`#skillinstallstalemirror`). `skill install` migrates these away instead of
/// leaving a copy nothing refreshes; `audit-docs` blocks while one survives.
const RETIRED_RUNBOOK_MIRRORS: &[&str] = &[".codex/runbooks"];

/// Bundled OKF concept files installed alongside the skill.
const BUNDLED_OKF: &[(&str, &str)] = &[
    ("index.md", include_str!("../okf/index.md")),
    ("session-cycle.md", include_str!("../okf/session-cycle.md")),
    (
        "instruction-surface.md",
        include_str!("../okf/instruction-surface.md"),
    ),
    (
        "dynamic-context.md",
        include_str!("../okf/dynamic-context.md"),
    ),
];

/// Current binary version (from Cargo.toml).
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_USER_PROMPT_COMMAND: &str = "agent-doc hook codex-user-prompt-submit";
const CODEX_STOP_COMMAND: &str = "agent-doc hook codex-stop";
const CODEX_MCP_SERVER_NAME: &str = "agent-doc";
const CODEX_MCP_COMMAND: &str = "agent-doc";
const CODEX_MCP_APPROVAL_MODE: &str = "approve";

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
        && ((content.contains("## Harness Compatibility") && content.contains("## Workflow"))
            || (content.contains("## Required cycle")
                && content.contains("## Runbook table")
                && content.contains("runbooks/session-cycle.md")))
}

fn skill_rel_path_for_env(env: agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        // Codex loads `.codex/AGENTS.md` as always-on repository instructions. The
        // agent-doc workflow is trigger-scoped, so install it as a real Codex skill
        // instead of leaking it into every normal Codex task.
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/SKILL.md"),
        // A generic root `AGENTS.md` is also always-on for several harnesses. Keep
        // the generic rendering available for tests/content parity, but never
        // install it into the root instruction file.
        Environment::Generic => std::path::PathBuf::from(".agent/skills/agent-doc/SKILL.md"),
        _ => env.skill_rel_path("agent-doc"),
    }
}

fn skill_path_for_env(
    env: agent_kit::detect::Environment,
    root: Option<&Path>,
) -> std::path::PathBuf {
    let rel = skill_rel_path_for_env(env);
    match root {
        Some(root) => root.join(rel),
        None => rel,
    }
}

fn is_skill_current_for_env(
    env: agent_kit::detect::Environment,
    root: Option<&Path>,
) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    if !path.exists() {
        return Ok(false);
    }
    let existing = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(existing == content_for_env(env))
}

fn install_skill_for_env(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    let content = content_for_env(env);

    if path.exists() {
        let existing = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if existing == content {
            eprintln!("[{}] skill already up to date (v{}).", env, VERSION);
            return Ok(false);
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    eprintln!(
        "[{}] installed skill v{} -> {}",
        env,
        VERSION,
        path.display()
    );
    Ok(true)
}

fn check_skill_for_env(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<bool> {
    let path = skill_path_for_env(env, root);
    if !path.exists() {
        eprintln!("Not installed. Run `agent-doc skill install` to install.");
        return Ok(false);
    }
    if is_skill_current_for_env(env, root)? {
        eprintln!("Up to date (v{}).", VERSION);
        Ok(true)
    } else {
        eprintln!(
            "Outdated. Run `agent-doc skill install` to update to v{}.",
            VERSION
        );
        Ok(false)
    }
}

fn normalized_managed_instruction_surface_for_audit(content: &str) -> String {
    strip_tsift_code_navigation_block(content)
        .trim_end()
        .to_string()
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

fn retire_managed_agents_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let existing =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if !looks_like_managed_root_agents(&existing) {
        return Ok(());
    }

    if let Some(tsift_block) = extract_tsift_code_navigation_block(&existing) {
        std::fs::write(path, format!("{}\n", tsift_block.trim_end()))
            .with_context(|| format!("write {}", path.display()))?;
        eprintln!(
            "[agent-doc] retired managed instruction surface, preserved tsift block -> {}",
            path.display()
        );
        return Ok(());
    }

    std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    eprintln!(
        "[agent-doc] retired managed instruction surface -> {}",
        path.display()
    );
    Ok(())
}

fn retire_managed_always_on_agents(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    retire_managed_agents_file(&base.join("AGENTS.md"))?;
    retire_managed_agents_file(&base.join(".codex/AGENTS.md"))?;
    Ok(())
}

pub(crate) fn audit_managed_instruction_surfaces(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    grok::audit(&base)?;

    for path in [base.join("AGENTS.md"), base.join(".codex/AGENTS.md")] {
        if !path.exists() {
            continue;
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if looks_like_managed_root_agents(&existing) {
            anyhow::bail!(
                "retired managed agent-doc instruction surface is still present: {}. Run `agent-doc skill install --all` or reinstall the active harness to migrate it out of always-on AGENTS.md.",
                path.display()
            );
        }
    }

    for env in [
        agent_kit::detect::Environment::OpenCode,
        agent_kit::detect::Environment::Codex,
        agent_kit::detect::Environment::ClaudeCode,
    ] {
        let path = skill_path_for_env(env, Some(&base));
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
            anyhow::bail!(
                "managed agent-doc instruction surface is stale: {}. Run `agent-doc skill install --all` or reinstall the active harness before release.",
                path.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn audit_managed_okf_bundles(root: Option<&Path>) -> Result<()> {
    audit_bundled_okf_shape()?;

    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    for env in [
        agent_kit::detect::Environment::Generic,
        agent_kit::detect::Environment::OpenCode,
        agent_kit::detect::Environment::Codex,
        agent_kit::detect::Environment::Cursor,
        agent_kit::detect::Environment::ClaudeCode,
    ] {
        let instruction_path = skill_path_for_env(env, Some(&base));
        let okf_dir = base.join(okf_rel_path(&env));
        if instruction_path.exists() {
            let content = std::fs::read_to_string(&instruction_path)
                .with_context(|| format!("read {}", instruction_path.display()))?;
            audit_okf_links_for_surface(&env, &instruction_path, &content)?;
            if content.contains("okf/") && !okf_dir.is_dir() {
                anyhow::bail!(
                    "managed agent-doc OKF bundle is missing for {env}: {} references okf/ but {} does not exist. Run `agent-doc skill install --all`.",
                    instruction_path.display(),
                    okf_dir.display()
                );
            }
        }
        if okf_dir.is_dir() {
            audit_okf_dir(&env, &okf_dir)?;
        }
    }
    Ok(())
}

fn audit_bundled_okf_shape() -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    let mut concept_ids = HashSet::new();
    for (name, content) in BUNDLED_OKF {
        for link in markdown_okf_links(content, true) {
            if !canonical_names.contains(link.as_str()) {
                anyhow::bail!(
                    "bundled OKF file {name} references okf/{link}, but {link} is not bundled"
                );
            }
        }
        if content.contains("type: concept") {
            let Some(concept_id) = frontmatter_value(content, "concept_id") else {
                anyhow::bail!("bundled OKF concept {name} is missing concept_id frontmatter");
            };
            if !concept_ids.insert(concept_id.to_string()) {
                anyhow::bail!("duplicate bundled OKF concept_id `{concept_id}`");
            }
        }
    }
    Ok(())
}

fn audit_okf_links_for_surface(
    env: &agent_kit::detect::Environment,
    path: &Path,
    content: &str,
) -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    for link in markdown_okf_links(content, false) {
        if !canonical_names.contains(link.as_str()) {
            anyhow::bail!(
                "managed agent-doc instruction surface for {env} references unbundled OKF file okf/{link}: {}",
                path.display()
            );
        }
    }
    Ok(())
}

fn audit_okf_dir(env: &agent_kit::detect::Environment, okf_dir: &Path) -> Result<()> {
    let canonical_names: HashSet<&str> = BUNDLED_OKF.iter().map(|(name, _)| *name).collect();
    for (name, expected) in BUNDLED_OKF {
        let path = okf_dir.join(name);
        if !path.exists() {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} is missing {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if existing != *expected {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} is stale: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }

    let entries = std::fs::read_dir(okf_dir)
        .with_context(|| format!("read OKF dir {}", okf_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("read OKF dir entry under {}", okf_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect OKF entry {}", entry.path().display()))?;
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
        if !canonical_names.contains(file_name) {
            anyhow::bail!(
                "managed agent-doc OKF bundle for {env} contains stale concept markdown: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }
    Ok(())
}

fn markdown_okf_links(content: &str, include_local_okf_links: bool) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = content;
    while let Some(open) = rest.find("](") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find(')') else {
            break;
        };
        let target = &rest[..close];
        if let Some(name) = okf_link_name(target, include_local_okf_links) {
            links.push(name.to_string());
        }
        rest = &rest[close + 1..];
    }
    links
}

fn okf_link_name(target: &str, include_local_okf_links: bool) -> Option<&str> {
    let target = target.split('#').next().unwrap_or(target);
    let target = target.strip_prefix("./").unwrap_or(target);
    if let Some(name) = target.strip_prefix("okf/") {
        return name.ends_with(".md").then_some(name);
    }
    if include_local_okf_links && !target.contains('/') && target.ends_with(".md") {
        return Some(target);
    }
    None
}

fn frontmatter_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        let Some((candidate, value)) = line.split_once(':') else {
            continue;
        };
        if candidate.trim() == key {
            return Some(value.trim().trim_matches('"'));
        }
    }
    None
}

/// True when no agent harness can be detected from the calling shell, so a
/// bare install falls back to the generic `.agent/skills/` target that no
/// harness reads automatically. `agent-doc init` uses it to tell a new user to
/// name their harness explicitly.
pub(crate) fn install_env_is_generic() -> bool {
    matches!(
        detect_install_env(),
        agent_kit::detect::Environment::Generic
    )
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
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/runbooks"),
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

fn resolve_install_base(root: Option<&Path>) -> std::path::PathBuf {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn bundled_runbook_names() -> std::collections::HashSet<&'static str> {
    BUNDLED_RUNBOOKS.iter().map(|(name, _)| *name).collect()
}

/// Project-owned runbook directories that opted into carrying bundled copies
/// (`#skillinstallstalemirror`). Only directories that already exist are
/// returned: the installer refreshes a mirror, it never creates one.
fn configured_runbook_mirrors(base: &Path) -> Vec<std::path::PathBuf> {
    let config_path = base.join(".agent-doc").join("config.toml");
    let Ok(content) = std::fs::read_to_string(&config_path) else {
        return Vec::new();
    };
    let config = match agent_doc_frontmatter::project_config::parse_project_toml(&content) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "warning: failed to parse {} while resolving runbook mirrors: {err}",
                config_path.display()
            );
            return Vec::new();
        }
    };
    let Some(mirrors) = config.agent_doc_skill_runbook_mirrors.as_ref() else {
        return Vec::new();
    };
    mirrors
        .iter()
        .map(|rel| rel.trim())
        .filter(|rel| !rel.is_empty())
        .map(|rel| base.join(rel))
        .filter(|dir| dir.is_dir())
        .collect()
}

/// Refresh bundled runbooks inside opted-in project-owned mirror directories.
///
/// Writes every bundled runbook whose content differs, and never touches a file
/// agent-doc does not bundle — these directories also hold project-owned
/// runbooks, so reaping unknown Markdown here would delete project content.
fn install_runbook_mirrors(root: Option<&Path>) -> Result<()> {
    let base = resolve_install_base(root);
    for dir in configured_runbook_mirrors(&base) {
        for (name, content) in BUNDLED_RUNBOOKS {
            let path = dir.join(name);
            let needs_write = !path.exists()
                || std::fs::read_to_string(&path)
                    .map(|existing| existing != *content)
                    .unwrap_or(true);
            if needs_write {
                std::fs::write(&path, content)
                    .with_context(|| format!("write runbook mirror {}", path.display()))?;
                eprintln!("[mirror] refreshed runbook → {}", path.display());
            }
        }
    }
    Ok(())
}

/// Remove retired runbook mirror directories that `skill install` no longer
/// manages, so a stale copy cannot masquerade as current instructions.
///
/// Only bundled runbook filenames are removed; anything else in the directory
/// is left in place (and keeps the directory alive) rather than silently
/// deleting content agent-doc never wrote.
fn retire_managed_runbook_mirrors(root: Option<&Path>) -> Result<()> {
    let base = resolve_install_base(root);
    let canonical_names = bundled_runbook_names();
    for rel in RETIRED_RUNBOOK_MIRRORS {
        let dir = base.join(rel);
        if !dir.is_dir() {
            continue;
        }
        for name in &canonical_names {
            let path = dir.join(name);
            if !path.is_file() {
                continue;
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("retire runbook mirror {}", path.display()))?;
            eprintln!("[retired] removed runbook mirror → {}", path.display());
        }
        let still_populated = std::fs::read_dir(&dir)
            .with_context(|| format!("read retired runbook mirror {}", dir.display()))?
            .next()
            .is_some();
        if !still_populated {
            std::fs::remove_dir(&dir)
                .with_context(|| format!("remove retired runbook mirror {}", dir.display()))?;
            eprintln!("[retired] removed runbook mirror dir → {}", dir.display());
        }
    }
    Ok(())
}

/// Audit installed runbook bundles against the running binary.
///
/// Managed harness directories must match the bundle exactly (missing, stale,
/// and unbundled Markdown all block). Opted-in project mirrors are checked for
/// missing and stale bundled files only, because they also hold project-owned
/// runbooks. A surviving retired mirror blocks outright.
pub(crate) fn audit_managed_runbook_bundles(root: Option<&Path>) -> Result<()> {
    let base = resolve_install_base(root);
    let canonical_names = bundled_runbook_names();

    for rel in RETIRED_RUNBOOK_MIRRORS {
        let dir = base.join(rel);
        if !dir.is_dir() {
            continue;
        }
        if canonical_names.iter().any(|name| dir.join(name).is_file()) {
            anyhow::bail!(
                "retired agent-doc runbook mirror is still present: {}. Run `agent-doc skill install --all` to migrate it out.",
                dir.display()
            );
        }
    }

    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        let runbooks_dir = base.join(runbooks_rel_path(&env));
        if !runbooks_dir.is_dir() {
            continue;
        }
        audit_runbook_dir(&format!("{env}"), &runbooks_dir, true)?;
    }

    for dir in configured_runbook_mirrors(&base) {
        let label = format!("project mirror {}", dir.display());
        audit_runbook_dir(&label, &dir, false)?;
    }

    Ok(())
}

/// Compare one runbook directory to the bundle.
///
/// `exclusive` marks a directory agent-doc fully owns, where unbundled Markdown
/// is itself a finding. Project mirrors are shared, so they only fail on missing
/// or stale bundled files.
fn audit_runbook_dir(label: &str, runbooks_dir: &Path, exclusive: bool) -> Result<()> {
    for (name, expected) in BUNDLED_RUNBOOKS {
        let path = runbooks_dir.join(name);
        if !path.exists() {
            anyhow::bail!(
                "agent-doc runbook bundle for {label} is missing {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
        let existing =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        if existing != *expected {
            anyhow::bail!(
                "agent-doc runbook bundle for {label} is stale: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }

    if !exclusive {
        return Ok(());
    }

    let canonical_names = bundled_runbook_names();
    let entries = std::fs::read_dir(runbooks_dir)
        .with_context(|| format!("read runbooks dir {}", runbooks_dir.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("read runbooks dir entry under {}", runbooks_dir.display()))?;
        if !entry
            .file_type()
            .with_context(|| format!("inspect runbook entry {}", entry.path().display()))?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !canonical_names.contains(file_name) {
            anyhow::bail!(
                "agent-doc runbook bundle for {label} contains stale runbook markdown: {}. Run `agent-doc skill install --all`.",
                path.display()
            );
        }
    }
    Ok(())
}

/// Install bundled OKF concept files alongside the skill for the detected environment.
fn install_okf(root: Option<&Path>) -> Result<()> {
    let env = detect_install_env();
    install_okf_for(env, root)
}

/// Resolve the OKF directory for a specific environment.
fn okf_rel_path(env: &agent_kit::detect::Environment) -> std::path::PathBuf {
    use agent_kit::detect::Environment;
    match env {
        Environment::ClaudeCode => std::path::PathBuf::from(".claude/skills/agent-doc/okf"),
        Environment::OpenCode => std::path::PathBuf::from(".opencode/skills/agent-doc/okf"),
        Environment::Codex => std::path::PathBuf::from(".codex/skills/agent-doc/okf"),
        Environment::Cursor => std::path::PathBuf::from(".cursor/rules/okf"),
        Environment::Generic => std::path::PathBuf::from("okf"),
    }
}

/// Install bundled OKF files for a specific environment.
///
/// The installer reconciles only Markdown files in the managed OKF directory.
/// Non-Markdown files are left alone for local notes or tool artifacts.
fn install_okf_for(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let okf_dir = base.join(okf_rel_path(&env));
    std::fs::create_dir_all(&okf_dir)?;

    let canonical_names: std::collections::HashSet<&str> =
        BUNDLED_OKF.iter().map(|(name, _)| *name).collect();

    for (name, content) in BUNDLED_OKF {
        let path = okf_dir.join(name);
        let needs_write = !path.exists()
            || std::fs::read_to_string(&path)
                .map(|existing| existing != *content)
                .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, content)?;
        }
    }

    let entries = std::fs::read_dir(&okf_dir)
        .with_context(|| format!("read OKF dir {}", okf_dir.display()))?;
    for entry in entries {
        let entry =
            entry.with_context(|| format!("read OKF dir entry under {}", okf_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect OKF entry {}", entry.path().display()))?;
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
            .with_context(|| format!("reap stale OKF {}", path.display()))?;
        eprintln!("[{}] reaped stale OKF → {}", env, path.display());
    }

    Ok(())
}

/// Install bundled OKF files for all environments.
fn install_okf_all(root: Option<&Path>) -> Result<()> {
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        install_okf_for(env, root)?;
    }
    Ok(())
}

fn install_env_artifacts(env: agent_kit::detect::Environment, root: Option<&Path>) -> Result<()> {
    if matches!(env, agent_kit::detect::Environment::ClaudeCode) {
        install_claude_hook_artifacts(root)?;
    }
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

fn install_claude_hook_artifacts(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let claude_dir = base.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    merge_claude_turn_status_hooks(&claude_dir.join("settings.json"))
}

fn install_codex_hook_artifacts(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let base = resolved.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let codex_dir = base.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    // `#codex-user-hook-owner`: Codex loads both user and project hook layers.
    // Keeping the consequential admission/closeout commands in both places
    // runs preflight twice whenever project-hook discovery works, while relying
    // on the project layer alone misses nested submodules and worktrees. The
    // user hook is therefore the sole owner; remove only our managed commands
    // from an existing project file and preserve every unrelated hook.
    retire_codex_project_hook_commands(&codex_dir.join("hooks.json"))?;
    merge_codex_config(&codex_dir.join("config.toml"))?;
    Ok(())
}

fn install_codex_user_hook_artifacts() -> Result<()> {
    let Some(codex_dir) = codex_user_config_dir() else {
        return Ok(());
    };
    install_codex_user_hook_artifacts_at(&codex_dir)
}

fn install_codex_user_hook_artifacts_at(codex_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(codex_dir)?;
    merge_codex_hooks_json(&codex_dir.join("hooks.json"))
}

fn codex_user_config_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        })
}

fn retire_codex_project_hook_commands(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }

    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut root = serde_json::from_str::<serde_json::Value>(&content)
        .with_context(|| format!("parse {}", path.display()))?;
    let hooks = root
        .as_object_mut()
        .context("Codex hooks.json root must be an object")?
        .get_mut("hooks")
        .and_then(serde_json::Value::as_object_mut);
    let Some(hooks_map) = hooks else {
        return Ok(());
    };

    retire_codex_hook_command(hooks_map, "UserPromptSubmit", CODEX_USER_PROMPT_COMMAND);
    retire_codex_hook_command(hooks_map, "Stop", CODEX_STOP_COMMAND);
    retire_codex_hook_command(hooks_map, "PostToolUse", STEERING_POSTTOOLUSE_COMMAND);

    let rendered = ensure_trailing_newline(&serde_json::to_string_pretty(&root)?);
    write_if_changed(path, &rendered)
}

fn retire_codex_hook_command(
    hooks_map: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
) {
    let remove_event = if let Some(entries) = hooks_map
        .get_mut(event)
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in entries.iter_mut() {
            if let Some(hooks) = entry
                .get_mut("hooks")
                .and_then(serde_json::Value::as_array_mut)
            {
                hooks.retain(|hook| {
                    hook.get("type").and_then(serde_json::Value::as_str) != Some("command")
                        || hook.get("command").and_then(serde_json::Value::as_str) != Some(command)
                });
            }
        }
        entries.retain(|entry| {
            entry
                .get("hooks")
                .and_then(serde_json::Value::as_array)
                .is_none_or(|hooks| !hooks.is_empty())
        });
        entries.is_empty()
    } else {
        false
    };

    if remove_event {
        hooks_map.remove(event);
    }
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
        Some("Preparing agent-doc cycle"),
        Some(PREFLIGHT_HOOK_TIMEOUT_SECS),
    );
    ensure_codex_hook_command(
        hooks_map,
        "Stop",
        CODEX_STOP_COMMAND,
        Some("Checking agent-doc completion boundary"),
        Some(CODEX_STOP_HOOK_TIMEOUT_SECS),
    );
    // `#midturn-steering`: Codex `PostToolUse` accepts the same
    // `hookSpecificOutput.additionalContext` envelope as Claude Code. The
    // shared hook also carries throttled persistent status-sync checkpoints.
    ensure_codex_hook_command(
        hooks_map,
        "PostToolUse",
        STEERING_POSTTOOLUSE_COMMAND,
        None,
        Some(STEERING_HOOK_TIMEOUT_SECS),
    );

    let rendered = ensure_trailing_newline(&serde_json::to_string_pretty(&root)?);
    write_if_changed(path, &rendered)
}

/// `#coinedid` mid-turn guard. `PreToolUse` fires before the tool runs, so a
/// coined `#id` is refused BEFORE it reaches source or a commit message — the
/// post-commit `session-check` warning can only report damage already done.
const COINED_ID_PRETOOLUSE_COMMAND: &str = "agent-doc hook coined-id-pre-tool-use";

/// `#preflightinbinary`. `UserPromptSubmit` fires when the `agent-doc <FILE>`
/// trigger arrives, so the binary runs its own preflight and its stdout becomes
/// the injected cycle contract — the agent gets the contract *with* the prompt
/// instead of having to remember to shell back for it. The handler recognizes
/// only a bare `agent-doc <FILE>` first line and no-ops on everything else, so
/// it cannot perturb an ordinary prompt.
const PREFLIGHT_USER_PROMPT_COMMAND: &str = "agent-doc hook preflight-user-prompt-submit";
/// `#midturn-steering`. `PostToolUse` fires after every tool call, so the
/// binary can hand the running turn operator edits to `agent:queue` and the
/// exchange as soon as they settle, instead of at the next cycle boundary. The
/// same owner-only surface periodically re-prompts a persistent status-sync
/// contract during long turns. The handler is silent with no active cycle or
/// nothing due, short-circuits steering on an unchanged document stat, and
/// never fails the tool call.
pub(crate) const STEERING_POSTTOOLUSE_COMMAND: &str = "agent-doc hook steering-post-tool-use";
/// Harness deadline for the steering hook. The hot path is a stat plus two
/// `state.db` reads; the bound only covers a cold cycle-state lookup.
pub(crate) const STEERING_HOOK_TIMEOUT_SECS: u64 = 10;
/// Consequential Claude Stop boundary: a clean closeout with a durable next
/// queue head is not allowed to become a final answer.
const CLAUDE_STOP_COMMAND: &str = "agent-doc hook claude-stop";

/// `timeout` written onto installed Claude/Codex `UserPromptSubmit` preflight hooks.
///
/// Claude Code defaults an untimed hook to 30s and **discards its output** on
/// expiry. Preflight measured 23-34s on an 87KB session document (2026-08-09),
/// so the default sat right on the cliff: a slow admission produced no contract
/// and no reason, which is indistinguishable from an unwired hook
/// (`#hookcontractlost`). This is deliberately well above the observed worst case
/// and above [`crate::preflight_hook::HOOK_ADMISSION_BUDGET_SECS`], so the binary
/// always gets to name its own overrun before the harness kills it silently.
pub(crate) const PREFLIGHT_HOOK_TIMEOUT_SECS: u64 = 120;

/// Outer harness timeout for the Codex Stop backstop. The Stop implementation
/// returns its own fail-closed JSON response first; this larger deadline exists
/// only so the harness never discards that response.
pub(crate) const CODEX_STOP_HOOK_TIMEOUT_SECS: u64 = 60;
pub(crate) const CLAUDE_STOP_HOOK_TIMEOUT_SECS: u64 = 15;

/// The harness timeout is the outer bound and the binary's own budget the inner
/// one. If they ever crossed, the harness would kill the hook before it could
/// name its overrun and the silence both exist to prevent would return — so this
/// is a compile-time invariant rather than a test.
const _: () = assert!(
    PREFLIGHT_HOOK_TIMEOUT_SECS > crate::preflight_hook::HOOK_ADMISSION_BUDGET_SECS,
    "the installed hook timeout must exceed the binary's admission budget"
);
const _: () = assert!(
    CODEX_STOP_HOOK_TIMEOUT_SECS > agent_doc_codex_stop_io::STOP_HOOK_BUDGET_SECS,
    "the installed Codex Stop timeout must exceed the binary's Stop budget"
);

/// Claude Code's deadline for a `UserPromptSubmit` hook entry that carries no
/// explicit `timeout`. On expiry Claude **discards the hook's output**, so an
/// untimed preflight entry silently reproduces `#hookcontractlost`.
pub(crate) const CLAUDE_DEFAULT_HOOK_TIMEOUT_SECS: u64 = 30;

/// The preflight `timeout` a Claude `settings.json` actually wires, in seconds.
///
/// `None` means the file does not wire our preflight hook at all (it may be
/// installed in another settings layer). `Some(30)` is returned for an entry
/// present without a `timeout`, because that is the deadline Claude enforces —
/// the distinction that matters downstream is the effective deadline, not
/// whether the key was written.
pub(crate) fn installed_preflight_hook_timeout_secs(settings: &str) -> Option<u64> {
    let root = serde_json::from_str::<serde_json::Value>(settings).ok()?;
    root.get("hooks")?
        .get("UserPromptSubmit")?
        .as_array()?
        .iter()
        .filter_map(|entry| entry.get("hooks")?.as_array())
        .flatten()
        .find(|hook| {
            hook.get("command").and_then(|v| v.as_str()) == Some(PREFLIGHT_USER_PROMPT_COMMAND)
        })
        .map(|hook| {
            hook.get("timeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(CLAUDE_DEFAULT_HOOK_TIMEOUT_SECS)
        })
}

/// Raise a preflight hook entry this settings file already wires up to
/// [`PREFLIGHT_HOOK_TIMEOUT_SECS`], returning the previous effective deadline
/// when it changed.
///
/// Deliberately narrower than [`merge_claude_turn_status_hooks`]: it adds no
/// hook the file does not already have and touches no other key. A project that
/// was initialized before the timeout existed — or any checkout whose
/// `.claude/settings.json` was written by an older binary — otherwise keeps
/// Claude's 30s default forever, because the merge only runs on an explicit
/// install in *that* directory. Live 2026-09-11: a submodule session
/// (`src/haiven-dev`) lost its cycle contract to
/// `UserPromptSubmit hook timed out after 30s — output discarded` while the
/// superproject beside it carried the 120s entry.
/// The preflight hook `timeout` that fits an admission `budget` (GH #127).
///
/// The binary clamps its budget to the installed timeout minus its report
/// headroom, so an operator who raises `AGENT_DOC_PREFLIGHT_HOOK_BUDGET_SECS`
/// past the installed timeout gets nothing unless the timeout rises too. This
/// keeps the default gap between [`PREFLIGHT_HOOK_TIMEOUT_SECS`] and
/// [`crate::preflight_hook::HOOK_ADMISSION_BUDGET_SECS`] above any larger
/// budget, and never goes below the default timeout.
pub(crate) fn preflight_hook_timeout_for_budget(budget: std::time::Duration) -> u64 {
    const GAP: u64 =
        PREFLIGHT_HOOK_TIMEOUT_SECS - crate::preflight_hook::HOOK_ADMISSION_BUDGET_SECS;
    PREFLIGHT_HOOK_TIMEOUT_SECS.max(budget.as_secs().saturating_add(GAP))
}

pub(crate) fn repair_claude_preflight_hook_timeout(
    path: &Path,
    required_secs: u64,
) -> Result<Option<u64>> {
    if !path.exists() {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let Some(installed) = installed_preflight_hook_timeout_secs(&content) else {
        return Ok(None);
    };
    // Raise-only. An operator who deliberately set a LARGER timeout has made the
    // invariant safer, not weaker, and this repair must not quietly undo that.
    // Only a deadline below the installed default can produce the silent
    // output-discard this exists to prevent.
    let required_secs = required_secs.max(PREFLIGHT_HOOK_TIMEOUT_SECS);
    if installed >= required_secs {
        return Ok(None);
    }
    let mut root = serde_json::from_str::<serde_json::Value>(&content)
        .with_context(|| format!("parse {}", path.display()))?;
    let Some(entries) = root
        .get_mut("hooks")
        .and_then(|hooks| hooks.get_mut("UserPromptSubmit"))
        .and_then(|entries| entries.as_array_mut())
    else {
        return Ok(None);
    };
    for hook in entries
        .iter_mut()
        .filter_map(|entry| entry.get_mut("hooks")?.as_array_mut())
        .flatten()
    {
        if hook.get("command").and_then(|v| v.as_str()) != Some(PREFLIGHT_USER_PROMPT_COMMAND) {
            continue;
        }
        let Some(hook) = hook.as_object_mut() else {
            continue;
        };
        hook.insert("timeout".to_string(), serde_json::json!(required_secs));
    }
    let rendered = ensure_trailing_newline(&serde_json::to_string_pretty(&root)?);
    std::fs::write(path, rendered).with_context(|| format!("write {}", path.display()))?;
    Ok(Some(installed))
}

const TURN_STATUS_ACTIVE_COMMAND: &str = "agent-doc turn-status active";
const TURN_STATUS_IDLE_COMMAND: &str = "agent-doc turn-status idle";

/// Install the Claude Code hooks that drive the turn-in-progress pane monitor
/// (`#claude-busy-status-during-active-turn`) into a `settings.json`. Idempotent
/// and additive (existing hooks, including the SessionStart autoclaim hook, are
/// preserved).
///
/// `UserPromptSubmit` → `turn-status active` (turn start); `Stop` runs both the
/// consequential queue-continuation guard and `turn-status idle`; `SessionStart`
/// → `turn-status idle`. The status clear keeps the registered document name,
/// while the queue guard is exact-session scoped and consults durable closeout
/// state.
/// OpenCode plugin that drives the monitor: `chat.message` (a new user message =
/// turn start) → `turn-status active`; the `session.idle` bus event (turn end) →
/// `turn-status idle`. Best-effort — `agent-doc turn-status` no-ops outside tmux.
const OPENCODE_TURN_STATUS_PLUGIN: &str = r#"// agent-doc turn-in-progress pane monitor (#claude-busy-status-during-active-turn).
// Auto-installed by `agent-doc turn-status install`. Sets the agent's own tmux
// pane border title on turn start (chat.message) and keeps the document name on
// turn end (session.idle). Best-effort: agent-doc turn-status no-ops outside tmux.
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
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None, None);
    let rendered = ensure_trailing_newline(&serde_json::to_string_pretty(&root)?);
    write_if_changed(path, &rendered)
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
        None,
    );
    ensure_codex_hook_command(hooks_map, "Stop", TURN_STATUS_IDLE_COMMAND, None, None);
    ensure_codex_hook_command(
        hooks_map,
        "Stop",
        CLAUDE_STOP_COMMAND,
        Some("Checking agent-doc queue continuation"),
        Some(CLAUDE_STOP_HOOK_TIMEOUT_SECS),
    );
    ensure_codex_hook_command(
        hooks_map,
        "SessionStart",
        TURN_STATUS_IDLE_COMMAND,
        None,
        None,
    );
    // No matcher: the handler filters by tool name itself and allows everything
    // it does not recognize, so the guard cannot wedge a turn on an unknown tool.
    ensure_codex_hook_command(
        hooks_map,
        "PreToolUse",
        COINED_ID_PRETOOLUSE_COMMAND,
        None,
        None,
    );
    // `#midturn-steering`: no matcher, so steering reaches the turn after any
    // tool call (a long Bash build is exactly when the operator steers).
    ensure_codex_hook_command(
        hooks_map,
        "PostToolUse",
        STEERING_POSTTOOLUSE_COMMAND,
        None,
        Some(STEERING_HOOK_TIMEOUT_SECS),
    );
    // `#preflightinbinary`: run preflight in-binary on the trigger prompt so the
    // cycle contract arrives with the prompt rather than a round trip later.
    ensure_codex_hook_command(
        hooks_map,
        "UserPromptSubmit",
        PREFLIGHT_USER_PROMPT_COMMAND,
        None,
        Some(PREFLIGHT_HOOK_TIMEOUT_SECS),
    );

    let rendered = ensure_trailing_newline(&serde_json::to_string_pretty(&root)?);
    write_if_changed(path, &rendered)
}

fn ensure_codex_hook_command(
    hooks_map: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
    command: &str,
    status_message: Option<&str>,
    timeout_secs: Option<u64>,
) {
    let entries = hooks_map
        .entry(event.to_string())
        .or_insert_with(|| serde_json::json!([]));
    let Some(entry_array) = entries.as_array_mut() else {
        *entries = serde_json::json!([]);
        return ensure_codex_hook_command(hooks_map, event, command, status_message, timeout_secs);
    };

    let already_present = entry_array.iter_mut().any(|entry| {
        entry
            .get_mut("hooks")
            .and_then(|hooks| hooks.as_array_mut())
            .map(|hooks| {
                hooks.iter_mut().any(|hook| {
                    let matches = hook.get("type").and_then(|v| v.as_str()) == Some("command")
                        && hook.get("command").and_then(|v| v.as_str()) == Some(command);
                    if matches && let Some(hook) = hook.as_object_mut() {
                        if let Some(status) = status_message {
                            hook.insert("statusMessage".to_string(), serde_json::json!(status));
                        }
                        // Repair an already-installed entry too, not just a fresh
                        // write: the checkouts that need the timeout most are the
                        // ones whose hook is already wired without it.
                        // Raise-only (GH #127): an installed timeout already
                        // above ours was chosen to fit a larger admission
                        // budget, and lowering it would silently re-cap that
                        // budget at the next session.
                        if let Some(timeout) = timeout_secs {
                            let installed = hook.get("timeout").and_then(|v| v.as_u64());
                            if installed.is_none_or(|installed| installed < timeout) {
                                hook.insert("timeout".to_string(), serde_json::json!(timeout));
                            }
                        }
                    }
                    matches
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
    if let Some(timeout) = timeout_secs {
        hook.insert("timeout".to_string(), serde_json::json!(timeout));
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
        "default_tools_approval_mode".to_string(),
        toml::Value::String(CODEX_MCP_APPROVAL_MODE.to_string()),
    );
    // `#skillinstallconfigpath`: no `--project-root`. This file is project-local
    // and usually tracked, so baking in the installing machine's absolute path
    // made it churn per operator — and a temp-dir test install once committed
    // `/tmp/agent-doc-v035139-final` here. `mcp serve` resolves the project root
    // from its own working directory instead. Reinstalling migrates an existing
    // entry because the whole server table is replaced.
    server.insert(
        "args".to_string(),
        toml::Value::Array(vec![
            toml::Value::String("mcp".to_string()),
            toml::Value::String("serve".to_string()),
        ]),
    );
    mcp_servers_table.insert(
        CODEX_MCP_SERVER_NAME.to_string(),
        toml::Value::Table(server),
    );

    write_if_changed(
        path,
        &ensure_trailing_newline(&toml::to_string_pretty(&root)?),
    )
}

/// Installed config artifacts are usually tracked, so a byte-identical rewrite
/// is churn in someone's `git status`. Write only on a real change, and always
/// end with a newline — a serializer that omits it leaves every install showing
/// a `\ No newline at end of file` diff (`#skillinstallconfigpath`).
fn write_if_changed(path: &Path, content: &str) -> Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path)
        && existing == content
    {
        return Ok(());
    }
    std::fs::write(path, content).with_context(|| format!("write {}", path.display()))
}

fn ensure_trailing_newline(content: &str) -> String {
    if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    }
}

/// Install the bundled SKILL.md to the project.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
#[allow(dead_code)]
pub fn install_at(root: Option<&Path>) -> Result<bool> {
    let install_user_codex_hooks = root.is_none();
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let env = detect_install_env();
    let updated = install_skill_for_env(env, resolved.as_deref())?;
    install_runbooks(resolved.as_deref())?;
    install_runbook_mirrors(resolved.as_deref())?;
    retire_managed_runbook_mirrors(resolved.as_deref())?;
    install_okf(resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    if install_user_codex_hooks && matches!(env, agent_kit::detect::Environment::Codex) {
        install_codex_user_hook_artifacts()?;
    }
    retire_managed_always_on_agents(resolved.as_deref())?;
    Ok(updated)
}

/// Public entry point (resolves to superproject root, called from main).
#[allow(dead_code)]
pub fn install() -> Result<()> {
    install_at(None).map(|_| ())
}

/// Install and return whether the file was actually updated (not just already up to date).
pub fn install_and_check_updated() -> Result<bool> {
    install_at(None)
}

/// Install the skill for a specific harness environment under an explicit root.
///
/// An explicit root is how a submodule-local install gets refreshed: bare root
/// resolution prefers the superproject, so `src/agent-doc/.claude/...` is
/// otherwise unreachable from inside the submodule (`#skillinstallstalemirror`).
pub fn install_for_at(
    env: agent_kit::detect::Environment,
    root: Option<&Path>,
) -> Result<bool> {
    let install_user_codex_hooks = root.is_none();
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let updated = install_skill_for_env(env, resolved.as_deref())?;
    install_runbooks_for(env, resolved.as_deref())?;
    install_runbook_mirrors(resolved.as_deref())?;
    retire_managed_runbook_mirrors(resolved.as_deref())?;
    install_okf_for(env, resolved.as_deref())?;
    install_env_artifacts(env, resolved.as_deref())?;
    if install_user_codex_hooks && matches!(env, agent_kit::detect::Environment::Codex) {
        install_codex_user_hook_artifacts()?;
    }
    retire_managed_always_on_agents(resolved.as_deref())?;
    Ok(updated)
}

/// Install the skill for all supported harnesses under an explicit root.
/// A `None` root resolves to the superproject (or git toplevel) install root.
pub fn install_all_at(root: Option<&Path>) -> Result<bool> {
    let install_user_codex_hooks = root.is_none();
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    let mut updated = grok::install(resolved.as_deref())?;
    for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
        updated |= install_skill_for_env(env, resolved.as_deref())?;
    }
    install_runbooks_all(resolved.as_deref())?;
    install_runbook_mirrors(resolved.as_deref())?;
    retire_managed_runbook_mirrors(resolved.as_deref())?;
    install_okf_all(resolved.as_deref())?;
    install_env_artifacts_all(resolved.as_deref())?;
    if install_user_codex_hooks {
        install_codex_user_hook_artifacts()?;
    }
    retire_managed_always_on_agents(resolved.as_deref())?;
    Ok(updated)
}

/// Check if the installed skill matches the bundled version.
/// When `root` is None, resolves to git superproject root (or CWD fallback).
pub fn check_at(root: Option<&Path>) -> Result<()> {
    let resolved = root.map(|p| p.to_path_buf()).or_else(resolve_root);
    if std::env::var_os("GROK_SESSION_ID").is_some() {
        return grok::check(resolved.as_deref());
    }
    let up_to_date = check_skill_for_env(detect_install_env(), resolved.as_deref())?;
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

    fn preflight_hook_entry(settings: &serde_json::Value) -> &serde_json::Value {
        settings["hooks"]["UserPromptSubmit"]
            .as_array()
            .expect("UserPromptSubmit entries")
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().expect("hooks array"))
            .find(|hook| hook["command"] == serde_json::json!(PREFLIGHT_USER_PROMPT_COMMAND))
            .expect("preflight hook entry")
    }

    fn claude_stop_hook_entry(settings: &serde_json::Value) -> &serde_json::Value {
        settings["hooks"]["Stop"]
            .as_array()
            .expect("Stop entries")
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().expect("hooks array"))
            .find(|hook| hook["command"] == serde_json::json!(CLAUDE_STOP_COMMAND))
            .expect("Claude Stop hook entry")
    }

    /// `#hookcontractlost`: an untimed Claude hook defaults to 30s and discards
    /// its output on expiry, which is exactly the silent no-contract state the
    /// admission-failure marker exists to remove. Preflight measured 23-34s on a
    /// large document, so the entry must carry its own timeout.
    #[test]
    fn installed_preflight_hook_carries_an_explicit_timeout() {
        let tmp = tempfile::TempDir::new().unwrap();
        let settings = tmp.path().join("settings.json");

        merge_claude_turn_status_hooks(&settings).unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            preflight_hook_entry(&parsed)["timeout"],
            serde_json::json!(PREFLIGHT_HOOK_TIMEOUT_SECS)
        );
        assert_eq!(
            claude_stop_hook_entry(&parsed)["timeout"],
            serde_json::json!(CLAUDE_STOP_HOOK_TIMEOUT_SECS)
        );
    }

    /// The checkouts that need the timeout most are the ones already wired
    /// without it, so the merge must repair an existing entry rather than only
    /// writing a fresh one.
    #[test]
    fn preflight_hook_timeout_is_repaired_on_an_already_installed_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let settings = tmp.path().join("settings.json");
        std::fs::write(
            &settings,
            serde_json::to_string_pretty(&serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [
                        { "hooks": [ { "type": "command", "command": PREFLIGHT_USER_PROMPT_COMMAND } ] }
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        merge_claude_turn_status_hooks(&settings).unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            preflight_hook_entry(&parsed)["timeout"],
            serde_json::json!(PREFLIGHT_HOOK_TIMEOUT_SECS)
        );
        assert_eq!(
            parsed["hooks"]["UserPromptSubmit"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|entry| entry["hooks"].as_array().unwrap())
                .filter(|hook| hook["command"] == serde_json::json!(PREFLIGHT_USER_PROMPT_COMMAND))
                .count(),
            1,
            "repairing the timeout must not duplicate the entry"
        );
    }

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
    fn bundled_skill_hot_path_stays_compact() {
        assert!(
            line_count(SKILL_TEMPLATE) <= 50,
            "SKILL.md routing table grew to {} lines",
            line_count(SKILL_TEMPLATE)
        );
        assert!(
            SKILL_TEMPLATE.len() <= 4_000,
            "SKILL.md routing table grew to {} bytes",
            SKILL_TEMPLATE.len()
        );
        assert!(SKILL_TEMPLATE.contains("This file is a routing table"));
        assert!(SKILL_TEMPLATE.contains("runbooks/session-cycle.md"));
    }

    #[test]
    fn rendered_claude_skill_includes_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::ClaudeCode);
        assert!(
            rendered.contains("## Claude queue continuation")
                && rendered.contains("runbooks/queue-auto-loop.md"),
            "Claude-rendered SKILL.md must route queue continuation to the bundled runbook"
        );
        let auto_loop = bundled_runbook("queue-auto-loop.md");
        assert!(
            auto_loop.contains("SUBMIT")
                && auto_loop.contains("`/loop agent-doc <FILE>`")
                && auto_loop.contains("`ScheduleWakeup`"),
            "auto-loop section must re-enter through a submitted `/loop agent-doc <FILE>` prompt"
        );
        assert!(
            auto_loop.contains("Do not invoke\n  the `loop` Skill directly"),
            "`#loopskillnoadmit`: a loop Skill call admits no cycle, so it must not be the re-entry"
        );
        assert!(
            auto_loop.contains("AGENT_DOC_QUEUE_MAX_ITERATIONS_HARD_CAP"),
            "auto-loop section must name the env hard-cap"
        );
        assert!(
            auto_loop.contains("queue_active") && auto_loop.contains("queue_prompts"),
            "auto-loop section must reference preflight queue fields"
        );
        assert!(
            auto_loop.contains("`persisted`"),
            "auto-loop section must make persisted-active queues continuation-eligible (#active-queue-persisted-no-continue)"
        );
        // #degraded-ipc-no-stall: a proven committed transition continues the
        // queue, while a missing Lazily visibility proof leaves the captured
        // intent retained in the sole state ledger.
        assert!(
            auto_loop.contains("#degraded-ipc-no-stall"),
            "auto-loop section must carry the degraded-IPC no-stall rule"
        );
        assert!(
            auto_loop.contains("missing Lazily visibility proof")
                && auto_loop.contains("state.db")
                && auto_loop.contains("registered endpoint recovers"),
            "auto-loop section must distinguish a proven ledger closeout from retained Lazily delivery"
        );
        assert!(
            auto_loop.contains("queue_continuation_guidance"),
            "auto-loop section must point at the binary-authoritative queue_continuation_guidance field"
        );
        // #bgclearpolicy: the drain contract names the shipped reset owner and
        // its opt-out, and never implies a clear fixes exchange-size accretion.
        assert!(
            auto_loop.contains("queue_auto_clear_yield")
                && auto_loop.contains("only compaction reduces"),
            "auto-loop section must describe queue auto-clear as shipped"
        );
    }

    #[test]
    fn rendered_codex_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Codex);
        assert!(
            !rendered.contains("## Claude queue continuation"),
            "Codex-rendered SKILL.md must NOT include the Claude-only auto-loop section (Codex uses its own Stop hook)"
        );
    }

    #[test]
    fn rendered_opencode_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::OpenCode);
        assert!(
            !rendered.contains("## Claude queue continuation"),
            "OpenCode-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_generic_skill_omits_queue_auto_loop_section() {
        let rendered = super::content_for_env(Environment::Generic);
        assert!(
            !rendered.contains("## Claude queue continuation"),
            "Generic-rendered SKILL.md must NOT include the Claude-only auto-loop section"
        );
    }

    #[test]
    fn rendered_harness_content_stays_compact() {
        for env in [
            Environment::ClaudeCode,
            Environment::OpenCode,
            Environment::Codex,
            Environment::Generic,
        ] {
            let content = super::content_for_env(env);
            assert!(
                line_count(&content) <= 60,
                "{env:?} rendered instruction surface grew to {} lines",
                line_count(&content)
            );
            assert!(
                content.len() <= 5_500,
                "{env:?} rendered instruction surface grew to {} bytes",
                content.len()
            );
        }
    }

    #[test]
    fn bundled_skill_template_contains_auto_update_line() {
        assert!(SKILL_TEMPLATE.contains(AUTO_UPDATE_LINE));
    }

    #[test]
    fn detect_install_env_treats_codex_thread_id_as_codex() {
        let _env_lock = crate::test_support::env_lock();
        let _claude = EnvVarGuard::unset("CLAUDE_CODE");
        let _claude_ep = EnvVarGuard::unset("CLAUDE_CODE_ENTRYPOINT");
        let _opencode = EnvVarGuard::unset("OPENCODE");
        let _cursor = EnvVarGuard::unset("CURSOR_SESSION_ID");
        let _cursor2 = EnvVarGuard::unset("CURSOR");
        let _code = EnvVarGuard::unset("CODEX");
        let _code_cli = EnvVarGuard::unset("CODEX_CLI");
        let _thread = EnvVarGuard::set("CODEX_THREAD_ID", "thread-123");
        let _ci = EnvVarGuard::unset("CODEX_CI");

        assert_eq!(super::detect_install_env(), Environment::Codex);
    }

    /// Resolve expected skill path using the explicit test environment.
    fn expected_path(dir: &std::path::Path) -> std::path::PathBuf {
        super::skill_path_for_env(Environment::ClaudeCode, Some(dir))
    }

    fn install_test(root: Option<&std::path::Path>) -> anyhow::Result<()> {
        super::install_skill_for_env(Environment::ClaudeCode, root).map(|_| ())
    }

    fn line_count(content: &str) -> usize {
        content.lines().count()
    }

    fn bundled_runbook(name: &str) -> &'static str {
        BUNDLED_RUNBOOKS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, body)| *body)
            .unwrap_or_else(|| panic!("{name} missing from BUNDLED_RUNBOOKS"))
    }

    fn bundled_instruction_text() -> String {
        let mut text = SKILL_TEMPLATE.to_string();
        for (_, body) in BUNDLED_RUNBOOKS {
            text.push('\n');
            text.push_str(body);
        }
        text
    }

    fn assert_codex_mcp_config(config: &toml::Value, _root: &std::path::Path) {
        let server = &config["mcp_servers"][CODEX_MCP_SERVER_NAME];
        assert_eq!(server["command"].as_str(), Some(CODEX_MCP_COMMAND));
        assert_eq!(
            server["default_tools_approval_mode"].as_str(),
            Some(CODEX_MCP_APPROVAL_MODE)
        );
        let args: Vec<&str> = server["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        // `#skillinstallconfigpath`: no machine-local absolute path in a
        // project-local, usually-tracked config file.
        assert_eq!(args, vec!["mcp", "serve",]);
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
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_creates_runbooks_codex() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let runbook_path = dir
            .path()
            .join(".codex/skills/agent-doc/runbooks/compact-exchange.md");
        assert!(
            runbook_path.exists(),
            "codex runbook not found at {}",
            runbook_path.display()
        );
        let content = std::fs::read_to_string(&runbook_path).unwrap();
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("VCS refresh signal"));
    }

    #[test]
    fn install_runbooks_reaps_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        std::fs::create_dir_all(&runbooks_dir).unwrap();

        let sentinel = runbooks_dir.join("sentinel-stale.md");
        std::fs::write(&sentinel, "# stale runbook removed in a later release\n").unwrap();
        let plugin_install = runbooks_dir.join("plugin-install.md");
        std::fs::write(&plugin_install, "# stale runbook\n").unwrap();
        let kept_non_md = runbooks_dir.join("README.txt");
        std::fs::write(&kept_non_md, "non-md file should survive\n").unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        assert!(
            !sentinel.exists(),
            "sentinel stale runbook should be reaped: {}",
            sentinel.display()
        );
        assert!(
            !plugin_install.exists(),
            "stale plugin-install.md should be reaped"
        );
        assert!(
            kept_non_md.exists(),
            "non-md files in runbooks dir must not be reaped"
        );

        let installed: std::collections::HashSet<String> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .filter_map(|entry| entry.file_name().to_str().map(|s| s.to_string()))
            .collect();
        let canonical: std::collections::HashSet<String> = super::BUNDLED_RUNBOOKS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        assert_eq!(
            installed, canonical,
            "post-install runbook set must match canonical embedded set"
        );
    }

    #[test]
    fn install_runbooks_is_no_op_on_clean_tree() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let runbooks_dir = dir.path().join(".claude/skills/agent-doc/runbooks");
        let first: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(&runbooks_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let mtime = entry.metadata().ok()?.modified().ok()?;
                let name = entry.file_name().to_str()?.to_string();
                Some((name, mtime))
            })
            .collect();

        std::thread::sleep(std::time::Duration::from_millis(20));
        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        for (name, mtime_before) in &first {
            let path = runbooks_dir.join(name);
            let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert_eq!(
                *mtime_before, mtime_after,
                "second install must not rewrite unchanged runbook {}",
                name
            );
        }
    }

    #[test]
    fn install_runbooks_reaps_for_codex_and_opencode() {
        let dir = tempfile::tempdir().unwrap();
        for (env, rel) in [
            (Environment::Codex, ".codex/skills/agent-doc/runbooks"),
            (Environment::OpenCode, ".opencode/skills/agent-doc/runbooks"),
        ] {
            let runbooks_dir = dir.path().join(rel);
            std::fs::create_dir_all(&runbooks_dir).unwrap();
            let sentinel = runbooks_dir.join("legacy-runbook.md");
            std::fs::write(&sentinel, "# legacy\n").unwrap();

            super::install_runbooks_for(env, Some(dir.path())).unwrap();

            assert!(
                !sentinel.exists(),
                "stale runbook under {} should be reaped",
                rel
            );
            assert!(runbooks_dir.join("commit.md").exists());
        }
    }

    #[test]
    fn install_okf_creates_concepts_for_each_harness() {
        let dir = tempfile::tempdir().unwrap();
        for (env, rel) in [
            (Environment::ClaudeCode, ".claude/skills/agent-doc/okf"),
            (Environment::Codex, ".codex/skills/agent-doc/okf"),
            (Environment::OpenCode, ".opencode/skills/agent-doc/okf"),
            (Environment::Cursor, ".cursor/rules/okf"),
            (Environment::Generic, "okf"),
        ] {
            super::install_okf_for(env, Some(dir.path())).unwrap();

            let index = dir.path().join(rel).join("index.md");
            assert!(index.exists(), "missing OKF index at {}", index.display());
            let content = std::fs::read_to_string(&index).unwrap();
            assert!(content.contains("Agent Doc OKF Index"));
            assert!(content.contains("session-cycle.md"));
        }
    }

    #[test]
    fn install_okf_reaps_stale_markdown_but_keeps_local_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let okf_dir = dir.path().join(".claude/skills/agent-doc/okf");
        std::fs::create_dir_all(&okf_dir).unwrap();
        let stale = okf_dir.join("legacy-concept.md");
        std::fs::write(&stale, "---\ntype: concept\n---\n# Legacy\n").unwrap();
        let kept = okf_dir.join("notes.txt");
        std::fs::write(&kept, "local note\n").unwrap();

        super::install_okf_for(Environment::ClaudeCode, Some(dir.path())).unwrap();

        assert!(!stale.exists(), "stale OKF markdown should be reaped");
        assert!(kept.exists(), "non-md OKF artifacts should be preserved");
        assert!(okf_dir.join("instruction-surface.md").exists());
    }

    #[test]
    fn audit_managed_okf_bundles_accepts_current_install() {
        let dir = tempfile::tempdir().unwrap();
        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();

        super::audit_managed_okf_bundles(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_missing_concept() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::remove_file(
            dir.path()
                .join(".codex/skills/agent-doc/okf/session-cycle.md"),
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert!(err.to_string().contains("session-cycle.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_stale_concept() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path()
                .join(".codex/skills/agent-doc/okf/dynamic-context.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale"));
        assert!(err.to_string().contains("dynamic-context.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_extra_managed_markdown() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path().join(".codex/skills/agent-doc/okf/legacy.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale concept markdown"));
        assert!(err.to_string().contains("legacy.md"));
    }

    #[test]
    fn audit_managed_okf_bundles_rejects_unbundled_surface_link() {
        let dir = tempfile::tempdir().unwrap();
        super::install_okf_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex/skills/agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".codex/skills/agent-doc/SKILL.md"),
            "# agent-doc\n\nSee [missing](okf/missing.md).\n",
        )
        .unwrap();

        let err = super::audit_managed_okf_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("unbundled OKF file"));
        assert!(err.to_string().contains("missing.md"));
    }

    fn write_runbook_mirror_config(root: &std::path::Path, mirrors: &str) {
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            format!("skill_runbook_mirrors = [{mirrors}]\n"),
        )
        .unwrap();
    }

    #[test]
    fn audit_managed_runbook_bundles_accepts_current_install() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        super::audit_managed_runbook_bundles(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_runbook_bundles_rejects_stale_runbook() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/commit.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_runbook_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale"));
        assert!(err.to_string().contains("commit.md"));
    }

    #[test]
    fn audit_managed_runbook_bundles_rejects_missing_runbook() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::remove_file(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/respond.md"),
        )
        .unwrap();

        let err = super::audit_managed_runbook_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("missing"));
        assert!(err.to_string().contains("respond.md"));
    }

    #[test]
    fn audit_managed_runbook_bundles_rejects_unbundled_markdown() {
        let dir = tempfile::tempdir().unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        std::fs::write(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/legacy.md"),
            "# stale\n",
        )
        .unwrap();

        let err = super::audit_managed_runbook_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("stale runbook markdown"));
        assert!(err.to_string().contains("legacy.md"));
    }

    #[test]
    fn audit_managed_runbook_bundles_rejects_surviving_retired_mirror() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex/runbooks")).unwrap();
        std::fs::write(dir.path().join(".codex/runbooks/commit.md"), "# stale\n").unwrap();

        let err = super::audit_managed_runbook_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("retired"));
        assert!(err.to_string().contains(".codex/runbooks"));
    }

    #[test]
    fn install_retires_the_legacy_codex_runbook_mirror() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex/runbooks")).unwrap();
        std::fs::write(dir.path().join(".codex/runbooks/commit.md"), "# stale\n").unwrap();

        super::retire_managed_runbook_mirrors(Some(dir.path())).unwrap();

        assert!(!dir.path().join(".codex/runbooks").exists());
    }

    #[test]
    fn retiring_the_legacy_mirror_keeps_files_agent_doc_never_bundled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codex/runbooks")).unwrap();
        std::fs::write(dir.path().join(".codex/runbooks/commit.md"), "# stale\n").unwrap();
        std::fs::write(
            dir.path().join(".codex/runbooks/local-notes.md"),
            "# mine\n",
        )
        .unwrap();

        super::retire_managed_runbook_mirrors(Some(dir.path())).unwrap();

        assert!(!dir.path().join(".codex/runbooks/commit.md").exists());
        assert!(dir.path().join(".codex/runbooks/local-notes.md").exists());
    }

    #[test]
    fn project_runbook_mirrors_refresh_only_when_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("runbooks")).unwrap();
        std::fs::write(dir.path().join("runbooks/commit.md"), "# stale\n").unwrap();

        super::install_runbook_mirrors(Some(dir.path())).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("runbooks/commit.md")).unwrap(),
            "# stale\n",
            "an unconfigured directory must never be written"
        );

        write_runbook_mirror_config(dir.path(), "\"runbooks\"");
        super::install_runbook_mirrors(Some(dir.path())).unwrap();

        let expected = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "commit.md")
            .map(|(_, content)| *content)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("runbooks/commit.md")).unwrap(),
            expected
        );
    }

    #[test]
    fn project_runbook_mirrors_never_reap_project_owned_runbooks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("runbooks")).unwrap();
        std::fs::write(
            dir.path().join("runbooks/browser-automation.md"),
            "# mine\n",
        )
        .unwrap();
        write_runbook_mirror_config(dir.path(), "\"runbooks\"");

        super::install_runbook_mirrors(Some(dir.path())).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("runbooks/browser-automation.md")).unwrap(),
            "# mine\n"
        );
        super::audit_managed_runbook_bundles(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_blocks_a_stale_configured_project_runbook_mirror() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("runbooks")).unwrap();
        write_runbook_mirror_config(dir.path(), "\"runbooks\"");
        super::install_runbook_mirrors(Some(dir.path())).unwrap();
        std::fs::write(dir.path().join("runbooks/commit.md"), "# stale\n").unwrap();

        let err = super::audit_managed_runbook_bundles(Some(dir.path())).unwrap_err();
        assert!(err.to_string().contains("project mirror"));
        assert!(err.to_string().contains("commit.md"));
    }

    #[test]
    fn install_runbook_mirrors_does_not_create_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_runbook_mirror_config(dir.path(), "\"runbooks\"");

        super::install_runbook_mirrors(Some(dir.path())).unwrap();

        assert!(!dir.path().join("runbooks").exists());
    }

    #[test]
    fn installed_harness_runbooks_include_commit_invariant() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/commit.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains(
                "Every appended `agent-doc` session response is one complete, validated write+commit"
            ));
            assert!(content.contains("agent-doc respond <FILE>"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("agent-doc session-check <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
    }

    #[test]
    fn installed_harness_pending_ops_runbooks_cover_plan_backed_items() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/pending-ops.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("create the plan file"));
            assert!(content.contains("include that exact plan"));
            assert!(content.contains("file path in the item text"));
            assert!(content.contains("plan-spec2-rollout.md"));
            assert!(content.contains("one flush-left backlog item per"));
            assert!(content.contains("queue entries and closeouts should target"));
        }
    }

    #[test]
    fn installed_harness_runbooks_share_manual_repair_rule() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_for(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();

        let claude = std::fs::read_to_string(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let codex = std::fs::read_to_string(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();
        let opencode = std::fs::read_to_string(
            dir.path()
                .join(".opencode/skills/agent-doc/runbooks/harness-invocation.md"),
        )
        .unwrap();

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("## Harness-Native Entrypoints"));
            assert!(content.contains("executable workflow entry"));
            assert!(content.contains(
                "Do **not** end a normal harness-native `agent-doc` turn with \"not committed\""
            ));
            assert!(content.contains("## Manual Repair Default"));
            assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
            assert!(content.contains("agent-doc write --commit <FILE>"));
            assert!(content.contains("bare `agent-doc write`"));
        }
        assert!(codex.contains("agent-doc session-check <FILE>"));
        assert!(codex.contains("Do **not** report success or stop"));
        assert!(opencode.contains("## OpenCode"));
        assert!(opencode.contains("Write-back"));
    }

    #[test]
    fn install_for_codex_writes_codex_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();

        let path = dir.path().join(".codex/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("follow the Codex update procedure"));
        assert!(content.contains("re-read the installed skill"));
        assert!(content.contains("runbooks/session-cycle.md"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
        assert!(
            !dir.path().join(".codex/AGENTS.md").exists(),
            "Codex install must not write always-on .codex/AGENTS.md"
        );
    }

    #[test]
    fn install_for_opencode_writes_opencode_specific_content() {
        let dir = tempfile::tempdir().unwrap();

        super::install_skill_for_env(Environment::OpenCode, Some(dir.path())).unwrap();

        let path = dir.path().join(".opencode/skills/agent-doc/SKILL.md");
        let content = std::fs::read_to_string(&path).unwrap();

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("follow the OpenCode update procedure"));
        assert!(content.contains("runbooks/session-cycle.md"));
        assert!(content.contains(&format!("agent-doc-version: \"{VERSION}\"")));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn install_for_codex_retires_managed_always_on_agents_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        stale_root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();
        std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
        std::fs::write(
            dir.path().join(".codex/AGENTS.md"),
            super::content_for_env(Environment::Codex),
        )
        .unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(root.contains("<!-- tsift:code-navigation v=0.1.42 -->"));
        assert!(!root.contains("# agent-doc"));
        assert!(!dir.path().join(".codex/AGENTS.md").exists());
        let codex_skill =
            std::fs::read_to_string(dir.path().join(".codex/skills/agent-doc/SKILL.md")).unwrap();
        assert!(codex_skill.contains("follow the Codex update procedure"));
    }

    #[test]
    fn install_for_codex_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        let custom = "# Custom Project Instructions\n\nKeep this file untouched.\n";
        std::fs::write(dir.path().join("AGENTS.md"), custom).unwrap();

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert_eq!(root, custom);
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_root_agents_surface() {
        let dir = tempfile::tempdir().unwrap();
        let stale_root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(dir.path().join("AGENTS.md"), stale_root).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains("AGENTS.md"));
        assert!(message.contains("agent-doc skill install --all"));
    }

    #[test]
    fn retire_managed_root_agents_preserves_tsift_navigation_block() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic);
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        super::retire_managed_always_on_agents(Some(dir.path())).unwrap();

        let root = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(root.contains("<!-- tsift:code-navigation v=0.1.42 -->"));
        assert!(root.contains("Run `tsift status`."));
        assert!(!root.contains("# agent-doc"));
        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_root_agents_even_with_tsift() {
        let dir = tempfile::tempdir().unwrap();
        let mut root = super::content_for_env(Environment::Generic).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        root.push_str(
            "\n<!-- tsift:code-navigation v=0.1.42 -->\n## Code Navigation\n\nRun `tsift status`.\n<!-- /tsift:code-navigation -->\n",
        );
        std::fs::write(dir.path().join("AGENTS.md"), root).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains("AGENTS.md"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_rejects_retired_codex_agents() {
        let dir = tempfile::tempdir().unwrap();
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let stale_codex = super::content_for_env(Environment::Codex).replace(
            &format!("agent-doc-version: \"{VERSION}\""),
            "agent-doc-version: \"0.33.12\"",
        );
        std::fs::write(codex_dir.join("AGENTS.md"), stale_codex).unwrap();

        let err = super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("retired managed agent-doc instruction surface"));
        assert!(message.contains(".codex"));
    }

    #[test]
    fn audit_managed_instruction_surfaces_preserves_custom_root_agents() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# Custom Project Instructions\n\nKeep this file untouched.\n",
        )
        .unwrap();

        super::audit_managed_instruction_surfaces(Some(dir.path())).unwrap();
    }

    #[test]
    fn install_for_codex_writes_user_hooks_and_project_feature_flag() {
        let dir = tempfile::tempdir().unwrap();
        let user_codex = dir.path().join("codex-home");

        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();
        super::install_runbooks_for(Environment::Codex, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::Codex, Some(dir.path())).unwrap();
        super::install_codex_user_hook_artifacts_at(&user_codex).unwrap();

        let hooks_path = user_codex.join("hooks.json");
        let config_path = dir.path().join(".codex/config.toml");
        assert!(hooks_path.exists(), "missing {}", hooks_path.display());
        assert!(config_path.exists(), "missing {}", config_path.display());
        assert!(
            !dir.path().join(".codex/hooks.json").exists(),
            "a fresh project install must not duplicate the user-owned Codex lifecycle hooks"
        );

        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&hooks_path).unwrap()).unwrap();
        let post_hooks = hooks["hooks"]["PostToolUse"][0]["hooks"]
            .as_array()
            .unwrap();
        assert!(post_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(STEERING_POSTTOOLUSE_COMMAND)
                && hook["timeout"].as_u64() == Some(STEERING_HOOK_TIMEOUT_SECS)
        }));
        let stop_hooks = hooks["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
        assert!(stop_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(CODEX_STOP_COMMAND)
                && hook["type"].as_str() == Some("command")
                && hook["timeout"].as_u64() == Some(CODEX_STOP_HOOK_TIMEOUT_SECS)
        }));
        let submit_hooks = hooks["hooks"]["UserPromptSubmit"][0]["hooks"]
            .as_array()
            .unwrap();
        assert!(submit_hooks.iter().any(|hook| {
            hook["command"].as_str() == Some(CODEX_USER_PROMPT_COMMAND)
                && hook["statusMessage"].as_str() == Some("Preparing agent-doc cycle")
                && hook["timeout"].as_u64() == Some(PREFLIGHT_HOOK_TIMEOUT_SECS)
        }));

        let config: toml::Value =
            toml::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
        assert!(
            dir.path().join(".codex/skills/agent-doc/SKILL.md").exists(),
            "Codex workflow should be installed as a skill, not .codex/AGENTS.md"
        );
        assert!(!dir.path().join(".codex/AGENTS.md").exists());
    }

    #[test]
    fn codex_install_repairs_existing_lifecycle_hook_timeouts() {
        let dir = tempfile::tempdir().unwrap();
        let user_codex = dir.path().join("codex-home");
        std::fs::create_dir_all(&user_codex).unwrap();
        std::fs::write(
            user_codex.join("hooks.json"),
            serde_json::json!({
                "hooks": {
                    "UserPromptSubmit": [{
                        "hooks": [{ "type": "command", "command": CODEX_USER_PROMPT_COMMAND }]
                    }],
                    "Stop": [{
                        "hooks": [{ "type": "command", "command": CODEX_STOP_COMMAND }]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();

        super::install_codex_user_hook_artifacts_at(&user_codex).unwrap();

        let hooks: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(user_codex.join("hooks.json")).unwrap())
                .unwrap();
        let find = |event: &str, command: &str| {
            hooks["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|entry| entry["hooks"].as_array().unwrap())
                .find(|hook| hook["command"].as_str() == Some(command))
                .unwrap()
        };
        assert_eq!(
            find("UserPromptSubmit", CODEX_USER_PROMPT_COMMAND)["timeout"].as_u64(),
            Some(PREFLIGHT_HOOK_TIMEOUT_SECS),
        );
        assert_eq!(
            find("Stop", CODEX_STOP_COMMAND)["timeout"].as_u64(),
            Some(CODEX_STOP_HOOK_TIMEOUT_SECS),
        );
    }

    #[test]
    fn codex_install_migrates_a_legacy_absolute_project_root_away() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            "[mcp_servers.agent-doc]\ncommand = \"agent-doc\"\nargs = [\"mcp\", \"serve\", \"--project-root\", \"/tmp/agent-doc-v035139-final\"]\n",
        )
        .unwrap();

        super::install_codex_hook_artifacts(Some(dir.path())).unwrap();

        let written = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            !written.contains("--project-root"),
            "a stale install-machine path must be migrated away, got:\n{written}"
        );
        assert_codex_mcp_config(&toml::from_str(&written).unwrap(), dir.path());
    }

    #[test]
    fn codex_install_artifacts_end_with_a_newline_and_do_not_churn() {
        let dir = tempfile::tempdir().unwrap();
        let user_codex = dir.path().join("codex-home");
        super::install_codex_hook_artifacts(Some(dir.path())).unwrap();
        super::install_codex_user_hook_artifacts_at(&user_codex).unwrap();

        let hooks_path = user_codex.join("hooks.json");
        let config_path = dir.path().join(".codex/config.toml");
        for path in [&hooks_path, &config_path] {
            let content = std::fs::read_to_string(path).unwrap();
            assert!(
                content.ends_with('\n'),
                "{} must end with a newline",
                path.display()
            );
        }

        // A byte-identical rewrite shows up as churn in a tracked file, and the
        // mtime bump alone triggers audit-docs staleness advisories.
        let hooks_mtime = std::fs::metadata(&hooks_path).unwrap().modified().unwrap();
        let config_mtime = std::fs::metadata(&config_path).unwrap().modified().unwrap();
        let hooks_before = std::fs::read_to_string(&hooks_path).unwrap();
        let config_before = std::fs::read_to_string(&config_path).unwrap();

        super::install_codex_hook_artifacts(Some(dir.path())).unwrap();
        super::install_codex_user_hook_artifacts_at(&user_codex).unwrap();

        assert_eq!(std::fs::read_to_string(&hooks_path).unwrap(), hooks_before);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            config_before
        );
        assert_eq!(
            std::fs::metadata(&hooks_path).unwrap().modified().unwrap(),
            hooks_mtime,
            "an unchanged hooks.json must not be rewritten"
        );
        assert_eq!(
            std::fs::metadata(&config_path).unwrap().modified().unwrap(),
            config_mtime,
            "an unchanged config.toml must not be rewritten"
        );
    }

    #[test]
    fn install_for_claude_writes_binary_admission_hook_and_preserves_existing_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        std::fs::write(
            &settings_path,
            r#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"custom-hook"}]}]}}"#,
        )
        .unwrap();

        super::install_env_artifacts(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_env_artifacts(Environment::ClaudeCode, Some(dir.path())).unwrap();

        let settings: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(settings_path).unwrap()).unwrap();
        let commands: Vec<_> = settings["hooks"]["UserPromptSubmit"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().cloned().unwrap_or_default())
            .filter_map(|hook| hook["command"].as_str().map(String::from))
            .collect();
        assert!(commands.contains(&"custom-hook".to_string()));
        assert_eq!(
            commands
                .iter()
                .filter(|command| command.as_str() == PREFLIGHT_USER_PROMPT_COMMAND)
                .count(),
            1,
        );
        // `#midturn-steering`: exactly one PostToolUse steering hook, timed.
        let post: Vec<_> = settings["hooks"]["PostToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().cloned().unwrap_or_default())
            .filter(|hook| hook["command"].as_str() == Some(STEERING_POSTTOOLUSE_COMMAND))
            .collect();
        assert_eq!(post.len(), 1, "{settings:#}");
        assert_eq!(post[0]["type"], "command");
        assert_eq!(
            post[0]["timeout"].as_u64(),
            Some(STEERING_HOOK_TIMEOUT_SECS)
        );
    }

    #[test]
    fn install_turn_status_hooks_all_harnesses_idempotent_and_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        // A pre-existing unrelated Claude hook (autoclaim SessionStart) must survive.
        let claude_settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(claude_settings.parent().unwrap()).unwrap();
        std::fs::write(
            &claude_settings,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"agent-doc autoclaim"}]}]}}"#,
        )
        .unwrap();

        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap();
        super::install_turn_status_hooks(Some(dir.path()), false, false).unwrap(); // idempotent re-run

        let cmds = |v: &serde_json::Value, event: &str| -> Vec<String> {
            v["hooks"][event]
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .flat_map(|e| e["hooks"].as_array().cloned().unwrap_or_default())
                        .filter_map(|h| h["command"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };

        // Claude: start + (Stop, SessionStart) end hooks; autoclaim preserved; idempotent.
        let claude: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_settings).unwrap()).unwrap();
        assert!(
            cmds(&claude, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string())
        );
        let claude_stop = cmds(&claude, "Stop");
        assert!(claude_stop.contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        assert!(claude_stop.contains(&CLAUDE_STOP_COMMAND.to_string()));
        assert_eq!(
            claude_stop
                .iter()
                .filter(|command| command.as_str() == CLAUDE_STOP_COMMAND)
                .count(),
            1,
            "the consequential Claude Stop guard must stay idempotent"
        );
        let ss = cmds(&claude, "SessionStart");
        assert!(
            ss.contains(&"agent-doc autoclaim".to_string()),
            "autoclaim preserved: {ss:?}"
        );
        assert!(ss.contains(&TURN_STATUS_IDLE_COMMAND.to_string()));
        assert_eq!(
            ss.iter()
                .filter(|c| c.as_str() == TURN_STATUS_IDLE_COMMAND)
                .count(),
            1,
            "idempotent: {ss:?}"
        );

        // Codex: start + Stop end (no SessionStart event for Codex).
        let codex: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
        )
        .unwrap();
        assert!(cmds(&codex, "UserPromptSubmit").contains(&TURN_STATUS_ACTIVE_COMMAND.to_string()));
        assert!(cmds(&codex, "Stop").contains(&TURN_STATUS_IDLE_COMMAND.to_string()));

        // OpenCode: plugin file wired to both turn-status commands via chat.message + session.idle.
        let oc =
            std::fs::read_to_string(dir.path().join(".opencode/plugin/agent-doc-turn-status.js"))
                .unwrap();
        assert!(oc.contains("turn-status active"), "{oc}");
        assert!(oc.contains("session.idle"), "{oc}");
        assert!(oc.contains("turn-status idle"), "{oc}");
    }

    #[test]
    fn install_for_codex_retires_project_commands_and_preserves_other_entries() {
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
                ],
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": CODEX_USER_PROMPT_COMMAND,
                                "statusMessage": "Tracking active agent-doc session"
                            }
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
        assert!(
            hooks["hooks"].get("UserPromptSubmit").is_none(),
            "the project-layer managed admission hook must be retired to prevent duplicate preflight"
        );

        let config: toml::Value = toml::from_str(
            &std::fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config["sandbox"]["default"].as_str(),
            Some("workspace-write")
        );
        assert_eq!(config["features"]["hooks"].as_bool(), Some(true));
        assert!(config["features"].get("codex_hooks").is_none());
        assert_codex_mcp_config(&config, dir.path());
    }

    #[test]
    fn install_runbooks_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_runbooks_all(Some(dir.path())).unwrap();

        const ACTIONABLE_OPERATOR_VERIFY_RULE: &str =
            "An `[operator-verify]` tag alone never justifies `agent:review`";
        for (env, _) in agent_kit::detect::Environment::all_skill_rel_paths("agent-doc") {
            let path = dir
                .path()
                .join(super::runbooks_rel_path(&env))
                .join("pending-ops.md");
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read installed {}: {error}", path.display()));
            assert!(
                content.contains(ACTIONABLE_OPERATOR_VERIFY_RULE),
                "{} installed pending-ops runbook is missing the actionable operator-verify lifecycle rule",
                env
            );
        }

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/runbooks/compact-exchange.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/skills/agent-doc/runbooks/compact-exchange.md")
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
    fn install_okf_all_creates_for_each_env() {
        let dir = tempfile::tempdir().unwrap();

        super::install_okf_all(Some(dir.path())).unwrap();

        assert!(
            dir.path()
                .join(".claude/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".codex/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(
            dir.path()
                .join(".opencode/skills/agent-doc/okf/index.md")
                .exists()
        );
        assert!(dir.path().join(".cursor/rules/okf/index.md").exists());
    }

    #[test]
    fn bundled_skill_is_a_complete_runbook_table() {
        assert!(SKILL_TEMPLATE.contains("## Required cycle"));
        assert!(SKILL_TEMPLATE.contains("## Runbook table"));
        assert!(SKILL_TEMPLATE.contains("session-cycle.md"));
        assert!(SKILL_TEMPLATE.contains("queue-auto-loop.md"));
        assert!(SKILL_TEMPLATE.contains("harness-invocation.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/commit.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/command-synonyms.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/compound-task-steering.md"));
        assert!(SKILL_TEMPLATE.contains("runbooks/planning-dispatch.md"));
        assert!(SKILL_TEMPLATE.contains("okf/index.md"));
    }

    #[test]
    fn bundled_skill_contains_backlog_capture_rules() {
        assert!(bundled_instruction_text().contains("Backlog capture rule"));
        assert!(bundled_instruction_text().contains("[recommended]"));
        assert!(bundled_instruction_text().contains("--backlog-queue-placement append"));
        assert!(bundled_instruction_text().contains("must not preempt the existing head"));
        assert!(bundled_instruction_text().contains("--backlog-add-after <id>"));
        assert!(bundled_instruction_text().contains("--backlog-add-before <id>"));
        assert!(bundled_instruction_text().contains("does not override them"));
        assert!(bundled_instruction_text().contains("multi-phase implementation work"));
        assert!(bundled_instruction_text().contains("one backlog ID per actionable phase"));
        assert!(bundled_instruction_text().contains("`do #id` closeout rule"));
        assert!(bundled_instruction_text().contains("--done <id>"));
        assert!(bundled_instruction_text().contains("pending_done_guard"));
        assert!(bundled_instruction_text().contains("An `[operator-verify]` tag alone never justifies `agent:review`"));
        assert!(bundled_instruction_text().contains("`--backlog-ungate <id>`"));
        assert!(bundled_instruction_text().contains("not `--review-edit`"));
    }

    #[test]
    fn bundled_skill_treats_imperative_document_edits_as_executable_work() {
        assert!(bundled_instruction_text().contains("Imperative edits are executable directives"));
        assert!(bundled_instruction_text().contains("authorize the named work"));
        assert!(bundled_instruction_text().contains("Do not ask the operator\n  to say `go` / `push` again"));
        assert!(bundled_instruction_text().contains("or invoke the preset again"));
        assert!(bundled_instruction_text().contains("harness-native tool-permission prompt"));
        assert!(
            bundled_instruction_text()
                .contains("MCP auth / OAuth is a sub-step, not a closeout boundary")
        );
        assert!(bundled_instruction_text().contains(
            "\"starting/continuing\" status prose while requested work remains undone"
        ));
    }

    #[test]
    fn bundled_skill_routes_natural_language_focused_extract_requests() {
        assert!(bundled_instruction_text().contains("The operator may ask naturally inside the source document"));
        assert!(SKILL_TEMPLATE.contains("[transfer-extract.md](runbooks/transfer-extract.md)"));
        assert!(bundled_instruction_text().contains(
            "An imperative extraction request authorizes the agent"
        ));
        assert!(bundled_instruction_text().contains(
            "--apply --coordinate-live-editors"
        ));
        assert!(bundled_instruction_text().contains(
            "If an endpoint lacks `coordinated_extract_v1`"
        ));
        assert!(bundled_instruction_text().contains(
            "Exploratory/question wording authorizes"
        ));
        assert!(bundled_instruction_text().contains(
            "supply IDs or a manifest"
        ));
        assert!(bundled_instruction_text().contains("There is no `split` alias"));
    }

    /// qsubattr4: the queue-level `subagents` attribute makes the in-session
    /// agent a coordinator. The hot path names the contract and links it; the
    /// respond and planning-dispatch runbooks own the full procedure.
    #[test]
    fn bundled_surfaces_carry_queue_subagents_coordinator_contract() {
        assert!(bundled_instruction_text().contains("Under `<!-- agent:run subagents -->`"));
        assert!(bundled_instruction_text().contains("queue claim --refresh --generation <N>"));
        assert!(bundled_instruction_text().contains("--refresh --generation <N>"));
        assert!(bundled_instruction_text().contains(
            "queue report-complete <FILE> --item <...> --generation <N> --response '<worker response>'"
        ));
        assert!(bundled_instruction_text().contains("queue release <FILE> --item <...> --generation <N>"));
        assert!(bundled_instruction_text().contains("`🟡` means reserved/dispatching"));
        assert!(bundled_instruction_text().contains("`make install` per integrated batch"));
        assert!(bundled_instruction_text().contains("agent-doc orchestrate <FILE> --mode parallel"));
        assert!(bundled_instruction_text().contains("(`#queuesubagents`)"));
        // `#waypostauthorization`: the coordinator pastes the binary-resolved
        // operator authorization into each subagent prompt.
        assert!(bundled_instruction_text().contains("`authorization.subagent_prompt_preamble` VERBATIM"));
        assert!(bundled_instruction_text().contains("agent-doc queue brief <FILE> --item <...>"));
        let runbook = |name: &str| {
            BUNDLED_RUNBOOKS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, body)| *body)
                .unwrap_or_else(|| panic!("{name} missing from BUNDLED_RUNBOOKS"))
        };
        let respond = runbook("respond.md");
        for needle in [
            "Queue subagents coordinator (`#queuesubagents`)",
            "`queue_subagent_dispatch` is the list to dispatch THIS cycle",
            "--owner subagent:<label> --refresh",
            "--refresh --generation <N>",
            "queue report-complete <FILE> --item <...> --generation <N> --response '<worker response>'",
            "OUTSIDE the IDE-watched project",
            "never run two subagents in\n     one checkout",
            "ONE\n     `make install` per integrated batch",
            "agent-doc queue release <FILE> --item <...>",
            "agent-doc queue release <FILE> --item <...> --generation <N>",
            "`agent-doc orchestrate <FILE> --mode parallel --task \"<item>\" ...`",
            "end the turn\n  quietly",
            "`authorization.subagent_prompt_preamble` VERBATIM",
            "`unresolved_preset`",
            "agent-doc queue brief <FILE> --item <...>",
        ] {
            assert!(respond.contains(needle), "respond.md missing {needle:?}");
        }
        let planning = runbook("planning-dispatch.md");
        assert!(planning.contains("Under `<!-- agent:run subagents -->`"));
        assert!(planning.contains("`agent-doc orchestrate <FILE> --mode parallel`"));
        assert!(planning.contains("(`#queuesubagents`)"));
        assert!(planning.contains("`authorization.subagent_prompt_preamble` pasted verbatim"));
    }

    #[test]
    fn bundled_skill_preserves_operator_visible_document_authority() {
        assert!(bundled_instruction_text().contains("Operator-visible document text is authoritative"));
        assert!(bundled_instruction_text().contains("content_ours"));
        assert!(bundled_instruction_text().contains("backup/audit state, not\n  hot-path authority"));
        assert!(bundled_instruction_text().contains("fail closed or retry through the editor"));
    }

    #[test]
    fn bundled_skill_treats_harness_native_entrypoints_as_binary_owned_cycles() {
        assert!(bundled_instruction_text().contains(
            "Harness-native `agent-doc` entrypoints start the binary-owned response cycle"
        ));
        assert!(bundled_instruction_text().contains("executable workflow start"));
        assert!(bundled_instruction_text().contains("generic\n  document-editing request"));
        assert!(SKILL_TEMPLATE.contains("Never manually patch the final response into the session document"));
        assert!(bundled_instruction_text().contains("agent-doc respond <FILE>"));
        assert!(bundled_instruction_text().contains("binary resolves and commits the turn"));
        assert!(bundled_instruction_text().contains("$CODEX_HOME/hooks.json"));
        assert!(bundled_instruction_text().contains("Manual repository commits exclude the active session document"));
        assert!(bundled_instruction_text().contains("code-enforced-directives.md"));
    }

    #[test]
    fn bundled_skill_contains_manual_repair_write_commit_rule() {
        assert!(bundled_instruction_text().contains("Manual repair / missed patchback rule (all harnesses)"));
        assert!(
            bundled_instruction_text()
                .contains("do **not** patch the\nassistant response directly into the file")
        );
        assert!(bundled_instruction_text().contains("Use `agent-doc write --commit <FILE>`"));
        assert!(bundled_instruction_text().contains("bare `agent-doc write`"));
    }

    #[test]
    fn bundled_skill_delegates_captured_finalize_recovery_to_the_binary() {
        assert!(bundled_instruction_text().contains("Captured finalize is\nbinary-owned"));
        assert!(bundled_instruction_text().contains("keyed supervisor worker"));
        assert!(bundled_instruction_text().contains("do not recapture, re-answer"));
        assert!(bundled_instruction_text().contains("bounded retry and exact-once commit semantics"));
        assert!(bundled_instruction_text().contains("whose exact bytes reach disk clears it"));
        assert!(!bundled_instruction_text().contains("then retry the same closeout once"));
    }

    /// `#respondbaselineflag`: no response-persistence command accepts a baseline
    /// path, and preflight emits no `baseline_file` field. An instruction surface
    /// that tells an agent to pass one produces a clap parse error on the single
    /// step SKILL.md calls MANDATORY, so the bundled surfaces must never mention it.
    #[test]
    fn bundled_instruction_surfaces_never_document_a_baseline_flag() {
        // Prescriptive use is what breaks an agent: `--baseline-file <...>` in a
        // command template produces a clap parse error. Naming the flag to forbid
        // it is the fix, so only the usage form and the retired preflight field
        // are banned.
        let banned_usage = "--baseline-file <";
        assert!(
            !bundled_instruction_text().contains(banned_usage),
            "SKILL.md must not put --baseline-file in a command template; no agent-doc command accepts it"
        );
        assert!(
            !bundled_instruction_text().contains("baseline_file"),
            "SKILL.md must not reference a preflight baseline_file field; preflight emits none"
        );
        assert!(
            bundled_instruction_text().contains("no `--baseline-file` flag"),
            "SKILL.md must state the prohibition explicitly so an agent does not reinvent the flag"
        );
        for (name, body) in BUNDLED_RUNBOOKS {
            assert!(
                !body.contains(banned_usage),
                "bundled runbook {name} must not put --baseline-file in a command template"
            );
            assert!(
                !body.contains("baseline_file"),
                "bundled runbook {name} must not reference a preflight baseline_file field"
            );
        }
    }

    #[test]
    fn development_claude_skill_matches_the_bundle() {
        let development_skill = include_str!("../.claude/skills/agent-doc/SKILL.md");
        let expected = format!("agent-doc-version: \"{VERSION}\"");
        assert!(SKILL_TEMPLATE.contains(&expected));
        // `#skillinstallstalemirror`: compare the whole rendering, not just the
        // version marker. A marker-only assertion passed for a copy whose body
        // was several versions stale, which is exactly how 0.35.224 shipped.
        assert_eq!(
            development_skill,
            content_for_env(Environment::ClaudeCode),
            "development Claude skill is stale — run `agent-doc skill install --root . --all`"
        );
    }

    /// GH #125 / #140 (`#chatprompt`): the routing table must load the cycle
    /// owner, and that runbook must retain the hook-independent chat contract.
    #[test]
    fn bundled_surfaces_record_chat_originated_prompts() {
        let cycle = bundled_runbook("session-cycle.md");
        assert!(SKILL_TEMPLATE.contains("runbooks/session-cycle.md"));
        assert!(cycle.contains("`#chatprompt`, GH #125"));
        assert!(cycle.contains("`#chatprompt`, GH #140"));
        assert!(cycle.contains("OpenCode has no `UserPromptSubmit` hook"));
        assert!(cycle.contains(
            "> **Chat prompt (#chatprompt:<id>):** <verbatim prompt>"
        ));
        assert!(cycle.contains("A non-empty `chat_prompts` is never `no_changes`"));
        assert!(cycle.contains("`session-check` warns `chat prompt not recorded`"));
        for env in [
            Environment::ClaudeCode,
            Environment::Codex,
            Environment::OpenCode,
            Environment::Generic,
        ] {
            assert!(
                content_for_env(env).contains("runbooks/session-cycle.md"),
                "{env:?} skill omits the mandatory cycle route"
            );
        }
    }

    #[test]
    fn bundled_skill_contains_binary_owned_respond_commit_invariant() {
        assert!(bundled_instruction_text().contains("agent-doc respond <FILE>"));
        assert!(
            bundled_instruction_text()
                .contains("unless the user explicitly asked to leave the response uncommitted")
        );
        assert!(bundled_instruction_text().contains("requires the cycle to reach `committed`"));
        assert!(bundled_instruction_text().contains("agent-doc session-check <FILE>"));
        assert!(bundled_instruction_text().contains("document-mutation boundary for the cycle"));
        assert!(bundled_instruction_text().contains(
            "`write --commit`, start no more long-running work for that turn"
        ));
        assert!(bundled_instruction_text().contains("`finalize` is its compatibility alias"));
    }

    #[test]
    fn bundled_skill_compact_entry_uses_commit_closeout() {
        assert!(bundled_instruction_text().contains("agent-doc compact <FILE> --commit"));
        assert!(bundled_instruction_text().contains("compact exchange <FILE>"));
    }

    #[test]
    fn bundled_skill_contains_model_short_name_attribution_rule() {
        // `#timestampresponseheader`: the heading now carries model AND timestamp.
        assert!(bundled_instruction_text().contains("### Re: topic — opus-5 · 2026-09-11T23:45-04:00"));
        assert!(bundled_instruction_text().contains("YYYY-MM-DDTHH:MM"));
        assert!(bundled_instruction_text().contains("never the harness label (`codex`, `claude`)"));
        // The one-em-dash invariant is what keeps all four heading parsers agreeing;
        // three split on the first em dash and `strip_re_heading_attribution` on the
        // last, so a second one leaves the model name behind as permanent commit drift.
        assert!(bundled_instruction_text().contains("exactly ONE spaced em dash"));
        assert!(
            !bundled_instruction_text().contains("### Re: topic — gpt-5"),
            "the untimestamped example must not survive alongside the timestamped rule"
        );
    }

    #[test]
    fn bundled_skill_requires_oldest_first_exchange_tail_reconciliation() {
        assert!(bundled_instruction_text().contains("do not stop at the newest question"));
        assert!(bundled_instruction_text().contains("each unresolved prompt in that tail"));
    }

    #[test]
    fn codex_content_uses_plain_text_invocation() {
        let content = super::content_for_env(Environment::Codex);

        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("Codex CLI will reject it"));
        assert!(content.contains("follow the Codex update procedure"));
        assert!(content.contains("re-read the installed skill"));
        assert!(content.contains("runbooks/session-cycle.md"));
        assert!(content.contains("runbooks/harness-invocation.md"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
    }

    #[test]
    fn opencode_content_uses_slash_command_invocation() {
        let content = super::content_for_env(Environment::OpenCode);

        assert!(content.contains("Interactive markdown session for OpenCode"));
        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("follow the OpenCode update procedure"));
        assert!(content.contains("runbooks/session-cycle.md"));
        assert!(!content.contains("TRIGGER: user invokes /agent-doc <file>."));
        assert!(!content.contains("Codex CLI will reject it"));
    }

    /// The no-argument entrypoint is documented for every harness in the
    /// spelling that harness uses, and each one names how FILE is found:
    /// the hook for Claude Code / Codex, the resolver for OpenCode. Grok's
    /// connected-admission text is pinned in `skill_grok`.
    #[test]
    fn every_harness_documents_the_no_argument_entrypoint() {
        let claude = super::content_for_env(Environment::ClaudeCode);
        assert!(claude.contains("argument-hint: \"[file]\""), "{claude}");
        assert!(claude.contains("\n/agent-doc\n"), "{claude}");
        assert!(claude.contains("the hook resolves it"), "{claude}");

        let codex = super::content_for_env(Environment::Codex);
        assert!(codex.contains("\nagent-doc\n"), "{codex}");
        assert!(codex.contains("the hook resolves it"), "{codex}");

        let opencode = super::content_for_env(Environment::OpenCode);
        assert!(opencode.contains("\n/agent-doc\n"), "{opencode}");
        assert!(
            opencode.contains("run `agent-doc document` first"),
            "{opencode}"
        );

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("never guess"), "{content}");
        }
        // A bare `/agent-doc` in OpenCode expands `$ARGUMENTS` to nothing, so
        // the prompt the skill sees is a plain `agent-doc`.
        let bare_prompt = super::OPENCODE_COMMAND_CONTENT.replace("$ARGUMENTS", "");
        assert_eq!(bare_prompt.lines().last().map(str::trim), Some("agent-doc"));
    }

    #[test]
    fn install_for_opencode_creates_command_file() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();

        let path = dir.path().join(".opencode/commands/agent-doc.md");
        assert!(path.exists(), "command file should exist at {path:?}");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("agent-doc $ARGUMENTS"));
        assert!(content.contains("description:"));
    }

    #[test]
    fn install_for_opencode_command_file_idempotent() {
        let dir = tempfile::tempdir().unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let first =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        super::install_opencode_command_file(Some(dir.path())).unwrap();
        let second =
            std::fs::read_to_string(dir.path().join(".opencode/commands/agent-doc.md")).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn install_env_artifacts_creates_opencode_command() {
        let dir = tempfile::tempdir().unwrap();

        super::install_env_artifacts(Environment::OpenCode, Some(dir.path())).unwrap();

        assert!(dir.path().join(".opencode/commands/agent-doc.md").exists());
    }

    #[test]
    fn claude_content_keeps_slash_invocation() {
        let content = super::content_for_env(Environment::ClaudeCode);

        assert!(content.contains("/agent-doc <FILE>"));
        assert!(content.contains("TRIGGER: user invokes /agent-doc <file>"));
        assert!(content.contains("follow the Claude update procedure"));
        assert!(content.contains("## Claude queue continuation"));
        assert!(content.contains("runbooks/queue-auto-loop.md"));
        assert!(content.contains("runbooks/session-cycle.md"));
    }

    #[test]
    fn generic_content_handles_stale_duplicate_instructions() {
        let content = super::content_for_env(Environment::Generic);

        assert!(content.contains("Claude Code: `/agent-doc <FILE>`"));
        assert!(content.contains("Codex: `agent-doc <FILE>`"));
        assert!(content.contains("OpenCode: `/agent-doc <FILE>`"));
        assert!(content.contains("active-harness update procedure"));
        assert!(content.contains("runbooks/session-cycle.md"));
    }

    #[test]
    fn generated_harness_content_shares_hot_path_outside_invocation() {
        let claude = super::content_for_env(Environment::ClaudeCode);
        let codex = super::content_for_env(Environment::Codex);
        let opencode = super::content_for_env(Environment::OpenCode);

        let claude_shared = super::remove_markdown_section(&claude, "## Invocation");
        let codex_shared = super::remove_markdown_section(&codex, "## Invocation");
        let opencode_shared = super::remove_markdown_section(&opencode, "## Invocation");
        let claude_shared = super::remove_markdown_section(
            &claude_shared,
            "## Claude queue continuation",
        );
        let claude_shared = claude_shared.replace(CLAUDE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let codex_shared = codex_shared.replace(CODEX_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");
        let opencode_shared = opencode_shared.replace(OPENCODE_AUTO_UPDATE_LINE, "<AUTO_UPDATE>");

        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            codex_shared.replace(CODEX_DESCRIPTION, "<DESC>")
        );
        assert_eq!(
            claude_shared.replace(CLAUDE_DESCRIPTION, "<DESC>"),
            opencode_shared.replace(OPENCODE_DESCRIPTION, "<DESC>")
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
    fn bundled_runbooks_include_dynamic_context() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "dynamic-context.md"),
            "dynamic-context.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            SKILL_TEMPLATE.contains("dynamic-context.md"),
            "SKILL.md should list dynamic-context in the runbook catalog"
        );
    }

    #[test]
    fn bundled_okf_includes_agent_doc_concepts() {
        assert!(
            BUNDLED_OKF.iter().any(|(name, _)| *name == "index.md"),
            "index.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, _)| *name == "session-cycle.md"),
            "session-cycle.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, _)| *name == "instruction-surface.md"),
            "instruction-surface.md should be in BUNDLED_OKF"
        );
        assert!(
            BUNDLED_OKF
                .iter()
                .any(|(name, content)| *name == "dynamic-context.md"
                    && content.contains("Dynamic Context")),
            "dynamic-context.md should be in BUNDLED_OKF"
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
    fn bundled_runbooks_include_jb_cache_conflict_runbook() {
        assert!(
            BUNDLED_RUNBOOKS.iter().any(|(name, content)| {
                *name == "jb-cache-conflict.md" && content.contains("## Quiescent CRDT Delivery")
            }),
            "jb-cache-conflict.md and its CRDT convergence contract should be bundled"
        );
        assert!(
            SKILL_TEMPLATE.contains("jb-cache-conflict"),
            "SKILL.md should list jb-cache-conflict in the runbook catalog"
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
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "planning-dispatch.md"),
            "planning-dispatch.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_split_spec_files_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "split-spec-files.md"),
            "split-spec-files.md should be in BUNDLED_RUNBOOKS"
        );
    }

    #[test]
    fn bundled_runbooks_include_baseline_drift_runbook() {
        assert!(
            BUNDLED_RUNBOOKS
                .iter()
                .any(|(name, _)| *name == "baseline-drift.md"),
            "baseline-drift.md should be in BUNDLED_RUNBOOKS"
        );
        assert!(
            SKILL_TEMPLATE.contains("baseline-drift"),
            "SKILL.md should list baseline-drift in the runbook catalog"
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
        assert!(content.contains("For **Claude Code**, **Codex**, and **OpenCode**"));
        assert!(content.contains("Claude Code"));
        assert!(content.contains("Codex"));
        assert!(content.contains("OpenCode"));
        assert!(content.contains("Harness Detection"));
        assert!(content.contains("Response Header Attribution"));
        assert!(content.contains("Do **not** type `/agent-doc`"));
        assert!(content.contains("agent-doc <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        // `#timestampresponseheader`: the timestamped example, plus the two
        // harness-label counter-examples this runbook shows as WRONG.
        assert!(content.contains("### Re: topic — opus-5 · 2026-09-11T23:45-04:00"));
        assert!(content.contains("YYYY-MM-DDTHH:MM"));
        assert!(content.contains("exactly one** spaced em dash"));
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
        assert!(
            content.contains("the manual repo commit must exclude the active session document")
        );
        assert!(content.contains("Resolve the intended non-session path set first"));
        assert!(content.contains("stop immediately on any stage failure"));
        assert!(content.contains("verify the staged diff still matches the intended set"));
        assert!(content.contains(".codex/hooks.json"));
        assert!(content.contains("UserPromptSubmit"));
        assert!(content.contains("agent-doc hook codex-stop"));
    }

    #[test]
    fn harness_invocation_runbook_opencode_section_requires_session_check() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "harness-invocation.md")
            .expect("harness-invocation.md not found");

        let opencode_start = content
            .find("## OpenCode")
            .expect("## OpenCode section not found");
        let next_section = content[opencode_start + 1..]
            .find("\n## ")
            .map(|i| opencode_start + 1 + i)
            .unwrap_or(content.len());
        let opencode_section = &content[opencode_start..next_section];

        assert!(
            opencode_section.contains("agent-doc session-check"),
            "OpenCode section must require session-check after finalize: {opencode_section}"
        );
        assert!(
            opencode_section.contains("Fail closed"),
            "OpenCode section must include fail-closed guard: {opencode_section}"
        );
        assert!(
            opencode_section.contains("response text visible in the console but absent"),
            "OpenCode section must name the CLI-only-output anti-pattern: {opencode_section}"
        );
    }

    #[test]
    fn commit_runbook_content() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "commit.md")
            .expect("commit.md not found");
        assert!(content.contains(
            "Every appended `agent-doc` session response is one complete, validated write+commit"
        ));
        assert!(content.contains("agent-doc respond <FILE>"));
        assert!(content.contains("agent-doc write --commit <FILE>"));
        assert!(content.contains("bare `agent-doc write`"));
        assert!(content.contains("keep the active session document out of that manual git commit"));
        assert!(content.contains("Resolve the exact intended non-session path set first"));
        assert!(content.contains("verify `git diff --cached --name-only`"));
        assert!(content.contains(
            "Do **not** continue to `git commit` after a narrowed `git add` / stage failure"
        ));
        assert!(content.contains(
            "Do **not** stage the active session document into an ordinary repo `git commit`"
        ));
    }

    #[test]
    fn compound_task_runbook_defers_session_doc_commit_until_finalize() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compound-task-steering.md")
            .expect("compound-task-steering.md not found");
        assert!(content.contains(
            "run `agent-doc finalize` / `write --commit` so the session document gets its own binary-owned closeout commit"
        ));
        assert!(content.contains(
            "validate and commit only the intended non-session repo files, finalize the session document, then push"
        ));
        assert!(content.contains("stop on any stage failure"));
    }

    #[test]
    fn compact_exchange_runbook_content_uses_binary_owned_closeout() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "compact-exchange.md")
            .expect("compact-exchange.md not found");
        assert!(content.contains("agent-doc compact <FILE> --component exchange --commit"));
        assert!(content.contains("binary-owned continuation"));
        assert!(content.contains("VCS refresh signal"));
        assert!(content.contains("agent:backlog"));
        assert!(content.contains("agent:run"));
        assert!(content.contains("agent:icebox"));
        assert!(content.contains("prompt_presets"));
    }

    #[test]
    fn pending_ops_runbook_content_contains_backlog_capture_rules() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "pending-ops.md")
            .expect("pending-ops.md not found");
        assert!(content.contains("beginning of the backlog"));
        assert!(content.contains("[recommended]"));
        assert!(content.contains("preserve the order you presented them in"));
        assert!(content.contains("follow-on step from an ordered batch"));
        assert!(content.contains("--backlog-reorder gkke,9pw9,step3"));
        assert!(content.contains("Existing `do #id` work that completed this cycle"));
        assert!(content.contains("--done <id>"));
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
        assert!(content.contains("one flush-left backlog item per"));
        assert!(content.contains("queue entries and closeouts should target"));
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
    fn planning_dispatch_runbook_content_covers_plan_contract() {
        let (_, content) = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "planning-dispatch.md")
            .expect("planning-dispatch.md not found");
        assert!(content.contains("agent-doc plan <FILE>"));
        assert!(content.contains("prompt_targets"));
        assert!(content.contains("repo_actions"));
        assert!(content.contains("required_commands"));
        assert!(content.contains("pending_mutations"));
        assert!(content.contains("handoff"));
        assert!(content.contains("blockers"));
        assert!(content.contains("handoff=orchestrate"));
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

        super::install_skill_for_env(Environment::ClaudeCode, Some(dir.path())).unwrap();
        super::install_skill_for_env(Environment::OpenCode, Some(dir.path())).unwrap();
        super::install_skill_for_env(Environment::Codex, Some(dir.path())).unwrap();

        let claude =
            std::fs::read_to_string(dir.path().join(".claude/skills/agent-doc/SKILL.md")).unwrap();
        let opencode =
            std::fs::read_to_string(dir.path().join(".opencode/skills/agent-doc/SKILL.md"))
                .unwrap();
        let codex =
            std::fs::read_to_string(dir.path().join(".codex/skills/agent-doc/SKILL.md")).unwrap();
        let persist_closeout = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "persist-closeout.md")
            .unwrap()
            .1;
        let manual_job_packets = BUNDLED_RUNBOOKS
            .iter()
            .find(|(name, _)| *name == "manual-job-packets.md")
            .unwrap()
            .1;
        let session_cycle = bundled_runbook("session-cycle.md");

        for content in [&claude, &codex, &opencode] {
            assert!(content.contains("This file is a routing table"));
            assert!(content.contains("runbooks/session-cycle.md"));
            assert!(content.contains("runbooks/persist-closeout.md"));
            assert!(content.len() <= 5_500);
        }
        assert!(session_cycle.contains("agent-doc respond <FILE>"));
        assert!(session_cycle.contains("agent-doc write --commit <FILE>"));
        assert!(session_cycle.contains("agent-doc session-check <FILE>"));
        assert!(session_cycle.contains("Imperative edits are executable directives"));
        assert!(session_cycle.contains("never the harness label (`codex`, `claude`)"));
        assert!(persist_closeout.contains("Agent harnesses own tiered verification"));
        assert!(persist_closeout.contains("make check-fast"));
        assert!(persist_closeout.contains("exactly one\nauthoritative `make check`"));
        assert!(persist_closeout.contains("repository-bytes/toolchain fingerprint"));
        assert!(persist_closeout.contains("Do not rely on a pre-commit hook"));
        assert!(persist_closeout.contains("Capture two independent proofs"));
        assert!(persist_closeout.contains("deliberately failing run"));
        assert!(manual_job_packets.contains("target runner's native"));
        assert!(manual_job_packets.contains("generic cross-runner"));
        assert!(manual_job_packets.contains("grep is not acceptable verification"));
        assert!(claude.contains("## Claude queue continuation"));
        assert!(codex.contains("follow the Codex update procedure"));
    }
}
