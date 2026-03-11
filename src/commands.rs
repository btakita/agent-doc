//! `agent-doc commands` — Output available commands as JSON for editor plugin autocomplete.

use serde::Serialize;

#[derive(Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub args: String,
    pub description: String,
}

/// Return all known commands: agent-doc CLI + Claude Code built-in + installed skills.
pub fn run() -> anyhow::Result<()> {
    let commands = all_commands();
    println!("{}", serde_json::to_string_pretty(&commands)?);
    Ok(())
}

fn all_commands() -> Vec<CommandInfo> {
    let mut cmds = Vec::new();

    // --- agent-doc CLI commands (exposed as /agent-doc <subcommand>) ---
    cmds.push(cmd("/agent-doc", "<FILE>", "Submit document session (diff, respond, write back)"));
    cmds.push(cmd("/agent-doc claim", "<FILE>", "Claim file for current tmux pane"));
    cmds.push(cmd("/agent-doc run", "<FILE>", "Run session workflow with agent backend"));
    cmds.push(cmd("/agent-doc init", "<FILE>", "Scaffold a new session document"));
    cmds.push(cmd("/agent-doc diff", "<FILE>", "Preview the diff that would be sent"));
    cmds.push(cmd("/agent-doc reset", "<FILE>", "Clear session ID and delete snapshot"));
    cmds.push(cmd("/agent-doc clean", "<FILE>", "Squash session git history"));
    cmds.push(cmd("/agent-doc start", "<FILE>", "Start Claude in a tmux pane"));
    cmds.push(cmd("/agent-doc route", "<FILE>", "Route command to the correct tmux pane"));
    cmds.push(cmd("/agent-doc commit", "<FILE>", "Git add + commit with timestamp"));
    cmds.push(cmd("/agent-doc focus", "<FILE>", "Focus tmux pane for a session document"));
    cmds.push(cmd("/agent-doc sync", "--col <FILES>", "Sync tmux panes to columnar layout"));
    cmds.push(cmd("/agent-doc layout", "<FILES>", "Arrange tmux panes to mirror editor"));
    cmds.push(cmd("/agent-doc patch", "<FILE> <COMPONENT>", "Replace content in a named component"));
    cmds.push(cmd("/agent-doc watch", "", "Watch files for changes and auto-submit"));
    cmds.push(cmd("/agent-doc outline", "<FILE>", "Display markdown outline with token counts"));
    cmds.push(cmd("/agent-doc resync", "", "Validate sessions.json, remove stale entries"));
    cmds.push(cmd("/agent-doc compact", "<FILE>", "Archive old exchanges to reduce document size"));
    cmds.push(cmd("/agent-doc convert", "<FILE>", "Convert append-mode document to template mode"));
    cmds.push(cmd("/agent-doc mode", "<FILE>", "Get or set the document mode"));
    cmds.push(cmd("/agent-doc write", "<FILE>", "Append assistant response (reads from stdin)"));
    cmds.push(cmd("/agent-doc recover", "<FILE>", "Recover orphaned response after compaction"));
    cmds.push(cmd("/agent-doc template-info", "<FILE>", "Show template structure (components, modes)"));
    cmds.push(cmd("/agent-doc prompt", "<FILE>", "Detect permission prompts from Claude"));
    cmds.push(cmd("/agent-doc audit-docs", "", "Audit instruction files against codebase"));
    cmds.push(cmd("/agent-doc skill install", "", "Install Claude Code skill definition"));
    cmds.push(cmd("/agent-doc skill check", "", "Check if installed skill matches binary"));
    cmds.push(cmd("/agent-doc plugin install", "<EDITOR>", "Install editor plugin"));
    cmds.push(cmd("/agent-doc plugin update", "<EDITOR>", "Update editor plugin"));
    cmds.push(cmd("/agent-doc plugin list", "", "List installed editor plugins"));
    cmds.push(cmd("/agent-doc upgrade", "", "Check for updates and upgrade"));
    cmds.push(cmd("/agent-doc autoclaim", "", "Re-establish claims after context compaction"));

    // --- Claude Code built-in commands ---
    cmds.push(cmd("/help", "", "Show help and available commands"));
    cmds.push(cmd("/model", "<MODEL>", "Switch to a different Claude model"));
    cmds.push(cmd("/clear", "", "Clear the conversation history"));
    cmds.push(cmd("/compact", "[INSTRUCTIONS]", "Compact context to free up space"));
    cmds.push(cmd("/cost", "", "Show token usage and cost for this session"));
    cmds.push(cmd("/login", "", "Switch Anthropic account"));
    cmds.push(cmd("/logout", "", "Sign out of current account"));
    cmds.push(cmd("/status", "", "Show account and session status"));
    cmds.push(cmd("/config", "", "Show or modify project configuration"));
    cmds.push(cmd("/memory", "", "Edit CLAUDE.md memory files"));
    cmds.push(cmd("/review", "", "Review and give feedback on Claude Code"));
    cmds.push(cmd("/bug", "", "Report a bug"));
    cmds.push(cmd("/fast", "", "Toggle fast mode (same model, faster output)"));
    cmds.push(cmd("/slow", "", "Toggle slow mode (thorough processing)"));
    cmds.push(cmd("/permissions", "", "Show or modify tool permissions"));
    cmds.push(cmd("/terminal-setup", "", "Install shell integration (Shift+Enter)"));
    cmds.push(cmd("/doctor", "", "Check system health and configuration"));
    cmds.push(cmd("/init", "", "Initialize a new CLAUDE.md for the project"));
    cmds.push(cmd("/pr-comments", "", "View PR comments from GitHub"));
    cmds.push(cmd("/vim", "", "Toggle vim keybinding mode"));
    cmds.push(cmd("/diff", "", "Show a diff of all file changes since start"));
    cmds.push(cmd("/undo", "", "Undo the last file change"));
    cmds.push(cmd("/resume", "", "Resume a previous conversation"));
    cmds.push(cmd("/listen", "", "Toggle listen mode (transcribe audio input)"));
    cmds.push(cmd("/mcp", "", "Show MCP server status and tools"));
    cmds.push(cmd("/approved-tools", "", "Show list of approved tools"));
    cmds.push(cmd("/add-dir", "<PATH>", "Add a directory to the context"));
    cmds.push(cmd("/release-notes", "", "Show release notes for current version"));
    cmds.push(cmd("/hooks", "", "Show configured hooks"));
    cmds.push(cmd("/btw", "<MESSAGE>", "Quick side question without interrupting"));

    // --- Other installed skills (from .claude/skills/) ---
    cmds.push(cmd("/existence", "lookup|search|new|lint|scope [ARGS]", "Query and manage ontology terms"));
    cmds.push(cmd("/existence lookup", "<TERM>", "Read a term's full definition"));
    cmds.push(cmd("/existence search", "<QUERY>", "Search terms by relevance"));
    cmds.push(cmd("/existence new", "<TERM>", "Author a new term (interactive)"));
    cmds.push(cmd("/existence lint", "", "Validate ontology nodes"));
    cmds.push(cmd("/existence scope", "[RING]", "List terms by ring level"));

    cmds.push(cmd("/tagpath", "parse|alias|prose|search|lint|extract|graph|init [ARGS]", "Parse and analyze tag-based identifiers"));
    cmds.push(cmd("/tagpath parse", "<NAME>", "Decompose identifier into canonical tags"));
    cmds.push(cmd("/tagpath alias", "<NAME>", "Generate all convention variants"));
    cmds.push(cmd("/tagpath search", "<QUERY> <PATH>", "Semantic search across conventions"));
    cmds.push(cmd("/tagpath lint", "[PATH]", "Validate naming against .naming.toml rules"));
    cmds.push(cmd("/tagpath extract", "<PATH>", "Extract identifiers from files"));

    cmds
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
            assert!(!cmd.description.is_empty(), "command '{}' must have a description", cmd.name);
            assert!(cmd.name.starts_with('/'), "command '{}' must start with /", cmd.name);
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
}
