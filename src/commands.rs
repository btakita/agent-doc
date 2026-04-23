//! # Module: commands
//!
//! ## Spec
//! - Enumerates all commands available in the agent-doc ecosystem: agent-doc CLI subcommands,
//!   Claude Code built-in slash commands, installed skill commands, and installed plugin commands.
//! - Each entry carries a `name` (slash-prefixed), `args` (synopsis), and `description`.
//! - `run()` serializes the full list to pretty-printed JSON on stdout; consumed by editor plugins
//!   for autocomplete popups.
//! - Static base list (agent-doc CLI + Claude Code built-ins) is extended with dynamically
//!   discovered plugin commands from `~/.claude/plugins/marketplaces/*/plugins/*/commands/*.md`.
//!
//! ## Plugin Command Discovery
//! - Scans `~/.claude/plugins/marketplaces/<MARKETPLACE>/plugins/<NAMESPACE>/commands/*.md`
//! - Each `.md` file may have YAML frontmatter with `description:` and `argument-hint:` fields
//! - Command name: `/<NAMESPACE>:<stem>` (e.g. `/codex:setup`, `/codex:rescue`)
//! - Files with empty descriptions are skipped
//!
//! ## Agentic Contracts
//! - `run() -> anyhow::Result<()>` — always succeeds unless JSON serialization panics (it won't);
//!   prints to stdout and returns `Ok(())`.
//! - Every `CommandInfo` entry must have a non-empty `name` starting with `/` and a non-empty
//!   `description`; enforced by tests.
//! - Total command count is ≥ 50 (test guard against accidental truncation).
//!
//! ## Evals
//! - nonempty_list: `all_commands()` → at least 50 entries
//! - names_start_with_slash: every entry name starts with `/` and has non-empty description
//! - json_output: serialized JSON contains `"/agent-doc"`, `"/help"`, `"/existence"`

#![allow(clippy::vec_init_then_push)]

use serde::Serialize;

#[derive(Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub args: String,
    pub description: String,
}

/// Return all known commands: agent-doc CLI + Claude Code built-in + installed skills + plugins.
pub fn run() -> anyhow::Result<()> {
    let commands = all_commands();
    println!("{}", serde_json::to_string_pretty(&commands)?);
    Ok(())
}

fn all_commands() -> Vec<CommandInfo> {
    let mut cmds = Vec::new();

    // --- agent-doc CLI commands (exposed as /agent-doc <subcommand>) ---
    cmds.push(cmd(
        "/agent-doc",
        "<FILE>",
        "Submit document session (diff, respond, write back)",
    ));
    cmds.push(cmd(
        "/agent-doc claim",
        "<FILE>",
        "Claim file for current tmux pane",
    ));
    cmds.push(cmd(
        "/agent-doc run",
        "<FILE>",
        "Run session workflow with agent backend",
    ));
    cmds.push(cmd(
        "/agent-doc init",
        "<FILE>",
        "Scaffold a new session document",
    ));
    cmds.push(cmd(
        "/agent-doc diff",
        "<FILE>",
        "Preview the diff that would be sent",
    ));
    cmds.push(cmd(
        "/agent-doc reset",
        "<FILE>",
        "Clear session ID and delete snapshot",
    ));
    cmds.push(cmd(
        "/agent-doc clean",
        "<FILE>",
        "Squash session git history",
    ));
    cmds.push(cmd(
        "/agent-doc start",
        "<FILE>",
        "Start Claude in a tmux pane",
    ));
    cmds.push(cmd(
        "/agent-doc route",
        "<FILE>",
        "Route command to the correct tmux pane",
    ));
    cmds.push(cmd(
        "/agent-doc commit",
        "<FILE>",
        "Git add + commit with timestamp",
    ));
    cmds.push(cmd(
        "/agent-doc focus",
        "<FILE>",
        "Focus tmux pane for a session document",
    ));
    cmds.push(cmd(
        "/agent-doc sync",
        "--col <FILES>",
        "Sync tmux panes to columnar layout",
    ));
    cmds.push(cmd(
        "/agent-doc layout",
        "<FILES>",
        "Arrange tmux panes to mirror editor",
    ));
    cmds.push(cmd(
        "/agent-doc patch",
        "<FILE> <COMPONENT>",
        "Replace content in a named component",
    ));
    cmds.push(cmd(
        "/agent-doc watch",
        "",
        "Watch files for changes and auto-run",
    ));
    cmds.push(cmd(
        "/agent-doc outline",
        "<FILE>",
        "Display markdown outline with token counts",
    ));
    cmds.push(cmd(
        "/agent-doc resync",
        "",
        "Validate sessions.json, remove stale entries",
    ));
    cmds.push(cmd(
        "/agent-doc compact",
        "<FILE>",
        "Archive old exchanges to reduce document size",
    ));
    cmds.push(cmd(
        "/agent-doc compact exchange",
        "<FILE>",
        "Compact the exchange component of a document",
    ));
    cmds.push(cmd(
        "/agent-doc convert",
        "<FILE>",
        "Convert append-mode document to template mode",
    ));
    cmds.push(cmd(
        "/agent-doc mode",
        "<FILE>",
        "Get or set the document mode",
    ));
    cmds.push(cmd(
        "/agent-doc write",
        "<FILE>",
        "Append assistant response (reads from stdin)",
    ));
    cmds.push(cmd(
        "/agent-doc recover",
        "<FILE>",
        "Recover orphaned response after compaction",
    ));
    cmds.push(cmd(
        "/agent-doc template-info",
        "<FILE>",
        "Show template structure (components, modes)",
    ));
    cmds.push(cmd(
        "/agent-doc prompt",
        "<FILE>",
        "Detect permission prompts from Claude",
    ));
    cmds.push(cmd(
        "/agent-doc audit-docs",
        "",
        "Audit instruction files against codebase",
    ));
    cmds.push(cmd(
        "/agent-doc skill install",
        "",
        "Install Claude Code skill definition",
    ));
    cmds.push(cmd(
        "/agent-doc skill check",
        "",
        "Check if installed skill matches binary",
    ));
    cmds.push(cmd(
        "/agent-doc plugin install",
        "<EDITOR>",
        "Install editor plugin",
    ));
    cmds.push(cmd(
        "/agent-doc plugin update",
        "<EDITOR>",
        "Update editor plugin",
    ));
    cmds.push(cmd(
        "/agent-doc plugin list",
        "",
        "List installed editor plugins",
    ));
    cmds.push(cmd(
        "/agent-doc upgrade",
        "",
        "Check for updates and upgrade",
    ));
    cmds.push(cmd(
        "/agent-doc autoclaim",
        "",
        "Re-establish claims after context compaction",
    ));
    cmds.push(cmd(
        "/agent-doc stream",
        "<FILE>",
        "Stream agent output to document in real-time (CRDT)",
    ));
    cmds.push(cmd(
        "/agent-doc cleanup",
        "<FILE>",
        "Clean up: callback orchestration, compaction",
    ));
    cmds.push(cmd(
        "/agent-doc pending add",
        "<FILE> <ITEM>",
        "Add item to pending component",
    ));
    cmds.push(cmd(
        "/agent-doc pending remove",
        "<FILE> <TARGET>",
        "Remove item from pending component",
    ));
    cmds.push(cmd(
        "/agent-doc pending prune",
        "<FILE>",
        "Remove completed items from pending",
    ));
    cmds.push(cmd(
        "/agent-doc pending list",
        "<FILE>",
        "List pending items",
    ));
    cmds.push(cmd(
        "/agent-doc callback request",
        "<FILE> [<OPS>]",
        "Create IPC callback request",
    ));

    // --- Claude Code built-in commands ---
    cmds.push(cmd("/help", "", "Show help and available commands"));
    cmds.push(cmd(
        "/model",
        "<MODEL>",
        "Switch to a different Claude model",
    ));
    cmds.push(cmd("/clear", "", "Clear the conversation history"));
    cmds.push(cmd(
        "/compact",
        "[INSTRUCTIONS]",
        "Compact context to free up space",
    ));
    cmds.push(cmd(
        "/cost",
        "",
        "Show token usage and cost for this session",
    ));
    cmds.push(cmd("/login", "", "Switch Anthropic account"));
    cmds.push(cmd("/logout", "", "Sign out of current account"));
    cmds.push(cmd("/status", "", "Show account and session status"));
    cmds.push(cmd("/config", "", "Show or modify project configuration"));
    cmds.push(cmd("/memory", "", "Edit CLAUDE.md memory files"));
    cmds.push(cmd(
        "/review",
        "",
        "Review and give feedback on Claude Code",
    ));
    cmds.push(cmd("/bug", "", "Report a bug"));
    cmds.push(cmd(
        "/fast",
        "",
        "Toggle fast mode (same model, faster output)",
    ));
    cmds.push(cmd("/slow", "", "Toggle slow mode (thorough processing)"));
    cmds.push(cmd("/permissions", "", "Show or modify tool permissions"));
    cmds.push(cmd(
        "/terminal-setup",
        "",
        "Install shell integration (Shift+Enter)",
    ));
    cmds.push(cmd("/doctor", "", "Check system health and configuration"));
    cmds.push(cmd(
        "/init",
        "",
        "Initialize a new CLAUDE.md for the project",
    ));
    cmds.push(cmd("/pr-comments", "", "View PR comments from GitHub"));
    cmds.push(cmd("/vim", "", "Toggle vim keybinding mode"));
    cmds.push(cmd(
        "/diff",
        "",
        "Show a diff of all file changes since start",
    ));
    cmds.push(cmd("/undo", "", "Undo the last file change"));
    cmds.push(cmd("/resume", "", "Resume a previous conversation"));
    cmds.push(cmd(
        "/listen",
        "",
        "Toggle listen mode (transcribe audio input)",
    ));
    cmds.push(cmd("/mcp", "", "Show MCP server status and tools"));
    cmds.push(cmd("/approved-tools", "", "Show list of approved tools"));
    cmds.push(cmd("/add-dir", "<PATH>", "Add a directory to the context"));
    cmds.push(cmd(
        "/release-notes",
        "",
        "Show release notes for current version",
    ));
    cmds.push(cmd("/hooks", "", "Show configured hooks"));
    cmds.push(cmd(
        "/btw",
        "<MESSAGE>",
        "Quick side question without interrupting",
    ));

    // --- Static project skills (fallback for well-known skills) ---
    cmds.push(cmd(
        "/existence",
        "lookup|search|new|lint|scope [ARGS]",
        "Query and manage ontology terms",
    ));
    cmds.push(cmd(
        "/existence lookup",
        "<TERM>",
        "Read a term's full definition",
    ));
    cmds.push(cmd(
        "/existence search",
        "<QUERY>",
        "Search terms by relevance",
    ));
    cmds.push(cmd(
        "/existence new",
        "<TERM>",
        "Author a new term (interactive)",
    ));
    cmds.push(cmd("/existence lint", "", "Validate ontology nodes"));
    cmds.push(cmd(
        "/existence scope",
        "[RING]",
        "List terms by ring level",
    ));

    cmds.push(cmd(
        "/tagpath",
        "parse|alias|prose|search|lint|extract|graph|init [ARGS]",
        "Parse and analyze tag-based identifiers",
    ));
    cmds.push(cmd(
        "/tagpath parse",
        "<NAME>",
        "Decompose identifier into canonical tags",
    ));
    cmds.push(cmd(
        "/tagpath alias",
        "<NAME>",
        "Generate all convention variants",
    ));
    cmds.push(cmd(
        "/tagpath search",
        "<QUERY> <PATH>",
        "Semantic search across conventions",
    ));
    cmds.push(cmd(
        "/tagpath lint",
        "[PATH]",
        "Validate naming against .naming.toml rules",
    ));
    cmds.push(cmd(
        "/tagpath extract",
        "<PATH>",
        "Extract identifiers from files",
    ));

    // --- Dynamic: plugin commands from ~/.claude/plugins/marketplaces/ ---
    let static_names: std::collections::HashSet<String> =
        cmds.iter().map(|c| c.name.clone()).collect();
    let mut plugin_cmds = discover_plugin_commands();
    plugin_cmds.retain(|c| !static_names.contains(&c.name));
    cmds.extend(plugin_cmds);

    // --- Dynamic: user-invocable skills from .claude/skills/ (relative to CWD) ---
    let all_names: std::collections::HashSet<String> =
        cmds.iter().map(|c| c.name.clone()).collect();
    let mut skill_cmds = discover_skill_commands();
    skill_cmds.retain(|c| !all_names.contains(&c.name));
    cmds.extend(skill_cmds);

    cmds
}

/// Scan `~/.claude/plugins/marketplaces/<MARKETPLACE>/plugins/<NAMESPACE>/commands/*.md`
/// and return one `CommandInfo` per `.md` file with a non-empty `description` frontmatter field.
/// Command names follow the pattern `/<NAMESPACE>:<stem>`.
fn discover_plugin_commands() -> Vec<CommandInfo> {
    use std::fs;
    use std::path::PathBuf;

    let mut cmds = Vec::new();

    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return cmds;
    }
    let marketplaces_dir = PathBuf::from(&home).join(".claude/plugins/marketplaces");
    if !marketplaces_dir.is_dir() {
        return cmds;
    }

    let Ok(marketplace_entries) = fs::read_dir(&marketplaces_dir) else {
        return cmds;
    };

    for marketplace_entry in marketplace_entries.flatten() {
        let marketplace_path = marketplace_entry.path();
        if !marketplace_path.is_dir() {
            continue;
        }
        let plugins_dir = marketplace_path.join("plugins");
        if !plugins_dir.is_dir() {
            continue;
        }
        let Ok(plugin_entries) = fs::read_dir(&plugins_dir) else {
            continue;
        };
        for plugin_entry in plugin_entries.flatten() {
            let plugin_path = plugin_entry.path();
            if !plugin_path.is_dir() {
                continue;
            }
            let namespace = plugin_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if namespace.is_empty() || namespace.starts_with('.') {
                continue;
            }
            let commands_dir = plugin_path.join("commands");
            if !commands_dir.is_dir() {
                continue;
            }
            let Ok(cmd_entries) = fs::read_dir(&commands_dir) else {
                continue;
            };
            let mut plugin_cmds: Vec<CommandInfo> = cmd_entries
                .flatten()
                .filter_map(|entry| {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("md") {
                        return None;
                    }
                    let stem = path.file_stem()?.to_string_lossy().to_string();
                    if stem.is_empty() || stem.starts_with('.') {
                        return None;
                    }
                    let content = fs::read_to_string(&path).unwrap_or_default();
                    let (description, args) = parse_command_frontmatter(&content);
                    if description.is_empty() {
                        return None;
                    }
                    Some(CommandInfo {
                        name: format!("/{namespace}:{stem}"),
                        args,
                        description,
                    })
                })
                .collect();
            plugin_cmds.sort_by(|a, b| a.name.cmp(&b.name));
            cmds.extend(plugin_cmds);
        }
    }

    cmds
}

/// Parse `description:` and `argument-hint:` from a YAML frontmatter block.
/// Returns `(description, args)`.
fn parse_command_frontmatter(content: &str) -> (String, String) {
    let mut description = String::new();
    let mut args = String::new();

    let Some(rest) = content.strip_prefix("---") else {
        return (description, args);
    };
    let end = rest.find("\n---").unwrap_or(rest.len());
    let frontmatter = &rest[..end];

    for line in frontmatter.lines() {
        if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("argument-hint:") {
            args = val.trim().trim_matches('"').trim_matches('\'').to_string();
        }
    }

    (description, args)
}

/// Scan `.claude/skills/<name>/SKILL.md` in the current directory and return one
/// `CommandInfo` per skill with `user-invocable: true` in YAML frontmatter.
/// Command name: `/<skill-name>` (directory name).
fn discover_skill_commands() -> Vec<CommandInfo> {
    use std::fs;
    use std::path::PathBuf;

    let mut cmds = Vec::new();

    let skills_dir = PathBuf::from(".claude/skills");
    if !skills_dir.is_dir() {
        return cmds;
    }

    let Ok(entries) = fs::read_dir(&skills_dir) else {
        return cmds;
    };

    let mut discovered: Vec<CommandInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_string_lossy().to_string();
            if name.is_empty() || name.starts_with('.') {
                return None;
            }
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                return None;
            }
            let content = fs::read_to_string(&skill_md).unwrap_or_default();
            let (description, args, user_invocable) = parse_skill_frontmatter(&content);
            if !user_invocable || description.is_empty() {
                return None;
            }
            Some(CommandInfo {
                name: format!("/{name}"),
                args,
                description,
            })
        })
        .collect();

    discovered.sort_by(|a, b| a.name.cmp(&b.name));
    cmds.extend(discovered);
    cmds
}

/// Parse `description:`, `argument-hint:`, and `user-invocable:` from YAML frontmatter.
/// Returns `(description, args, user_invocable)`.
fn parse_skill_frontmatter(content: &str) -> (String, String, bool) {
    let mut description = String::new();
    let mut args = String::new();
    let mut user_invocable = false;

    let Some(rest) = content.strip_prefix("---") else {
        return (description, args, user_invocable);
    };
    let end = rest.find("\n---").unwrap_or(rest.len());
    let frontmatter = &rest[..end];

    for line in frontmatter.lines() {
        if let Some(val) = line.strip_prefix("description:") {
            description = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("argument-hint:") {
            args = val.trim().trim_matches('"').trim_matches('\'').to_string();
        } else if let Some(val) = line.strip_prefix("user-invocable:") {
            let v = val.trim();
            user_invocable = v == "true";
        }
    }

    (description, args, user_invocable)
}

fn cmd(name: &str, args: &str, description: &str) -> CommandInfo {
    CommandInfo {
        name: name.to_string(),
        args: args.to_string(),
        description: description.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_commands_is_nonempty() {
        let cmds = all_commands();
        assert!(cmds.len() > 50, "expected 50+ commands, got {}", cmds.len());
    }

    #[test]
    fn all_commands_have_name_and_description() {
        for cmd in all_commands() {
            assert!(!cmd.name.is_empty(), "command name must not be empty");
            assert!(
                !cmd.description.is_empty(),
                "command '{}' must have a description",
                cmd.name
            );
            assert!(
                cmd.name.starts_with('/'),
                "command '{}' must start with /",
                cmd.name
            );
        }
    }

    #[test]
    fn serializes_to_json() {
        let cmds = all_commands();
        let json = serde_json::to_string(&cmds).unwrap();
        assert!(json.contains("/agent-doc"));
        assert!(json.contains("/help"));
        assert!(json.contains("/existence"));
    }

    #[test]
    fn parse_command_frontmatter_extracts_description_and_args() {
        let content = r#"---
description: Check whether the local Codex CLI is ready
argument-hint: '[--enable-review-gate|--disable-review-gate]'
allowed-tools: Bash(node:*)
---

Body text here.
"#;
        let (desc, args) = parse_command_frontmatter(content);
        assert_eq!(desc, "Check whether the local Codex CLI is ready");
        assert_eq!(args, "[--enable-review-gate|--disable-review-gate]");
    }

    #[test]
    fn parse_command_frontmatter_handles_missing_fields() {
        let (desc, args) = parse_command_frontmatter("no frontmatter here");
        assert!(desc.is_empty());
        assert!(args.is_empty());
    }
}
