//! # Module: harness
//!
//! ## Spec
//! - Defines `HarnessConfig`: per-agent harness configuration for the supervisor.
//! - Parameterizes binary name, restart behavior, prompt patterns, trigger command,
//!   env vars to remove, and feature support flags.
//! - `RestartBehavior`: how the supervisor restarts after a crash — either append args
//!   to base_args (Claude: `--continue`) or prefix a resume subcommand while preserving
//!   the resolved base args (Codex: `resume --last` + existing sandbox/model flags).
//! - `HarnessConfig::claude()` and `HarnessConfig::codex()` provide defaults.
//! - `HarnessConfig::from_context()` resolves from frontmatter `agent` field,
//!   config `default_agent`, with Claude as fallback.
//!
//! ## Agentic Contracts
//! - `from_context` never fails — falls back to Claude defaults for unknown agents.
//! - `restart_args` returns the full arg list for a restart iteration.
//! - `trigger_command` substitutes the file path into the template.

use crate::config::Config;
use crate::frontmatter::Frontmatter;

/// How the supervisor builds args on restart after a crash.
#[derive(Debug, Clone, PartialEq)]
pub enum RestartBehavior {
    /// Append these args to base_args (Claude: `["--continue"]`).
    Append(Vec<String>),
    /// Prefix these args ahead of base_args (Codex: `["resume", "--last"]`).
    Prepend(Vec<String>),
}

/// What the supervisor should do after a clean child exit (code 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanExitBehavior {
    /// Show the local restart prompt and let the human choose.
    PromptUser,
    /// Immediately restart the harness in its resume/continue mode.
    RestartContinue,
}

/// Per-agent harness configuration for the supervisor.
#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub binary: String,
    pub restart_behavior: RestartBehavior,
    pub clean_exit_behavior: CleanExitBehavior,
    pub prompt_patterns: Vec<String>,
    /// Template for the trigger command sent via tmux send-keys.
    /// `{file}` is replaced with the document path.
    pub trigger_command_template: String,
    /// Env vars to remove from the child process (used by route.rs and agent backends).
    #[allow(dead_code)]
    pub env_remove: Vec<String>,
    /// Whether `--no-mcp` flag is supported (Claude-specific).
    pub supports_no_mcp: bool,
    /// Whether `ENABLE_TOOL_SEARCH` env var is supported (Claude-specific).
    pub supports_enable_tool_search: bool,
    /// Fallback tmux session name when not inside tmux and no config is set.
    pub tmux_session_fallback: String,
    /// Process names recognized as agent processes for lazy-claim gating.
    pub process_names: Vec<String>,
}

impl HarnessConfig {
    pub fn claude() -> Self {
        Self {
            binary: "claude".into(),
            restart_behavior: RestartBehavior::Append(vec!["--continue".into()]),
            clean_exit_behavior: CleanExitBehavior::PromptUser,
            prompt_patterns: vec!["❯".into(), "⏵".into()],
            trigger_command_template: "/agent-doc {file}".into(),
            env_remove: vec!["CLAUDECODE".into()],
            supports_no_mcp: true,
            supports_enable_tool_search: true,
            tmux_session_fallback: "claude".into(),
            process_names: vec!["agent-doc".into(), "claude".into(), "node".into()],
        }
    }

    pub fn codex() -> Self {
        Self {
            binary: "codex".into(),
            restart_behavior: RestartBehavior::Prepend(vec!["resume".into(), "--last".into()]),
            clean_exit_behavior: CleanExitBehavior::RestartContinue,
            prompt_patterns: vec!["❯".into(), ">".into(), "›".into()],
            trigger_command_template: "agent-doc {file}".into(),
            env_remove: vec!["CODEX_CLI".into(), "CODEX".into()],
            supports_no_mcp: false,
            supports_enable_tool_search: false,
            tmux_session_fallback: "codex".into(),
            process_names: vec!["agent-doc".into(), "codex".into(), "node".into()],
        }
    }

    /// Resolve harness config from frontmatter and global config.
    /// Precedence: `fm.agent` > `config.default_agent` > `"claude"`.
    pub fn from_context(fm: &Frontmatter, config: &Config) -> Self {
        let agent_name = fm
            .agent
            .as_deref()
            .or(config.default_agent.as_deref())
            .unwrap_or("claude");
        Self::from_agent_name(agent_name)
    }

    pub fn from_agent_name(name: &str) -> Self {
        match name {
            "codex" => Self::codex(),
            _ => Self::claude(),
        }
    }

    /// Build the full arg list for a restart iteration.
    /// On first run, returns `base_args` unchanged.
    /// On restart, applies the `restart_behavior`.
    pub fn restart_args(&self, base_args: &[String]) -> Vec<String> {
        match &self.restart_behavior {
            RestartBehavior::Append(extra) => {
                let mut args = base_args.to_vec();
                args.extend(extra.iter().cloned());
                args
            }
            RestartBehavior::Prepend(prefix) => {
                let mut args = prefix.clone();
                args.extend(base_args.iter().cloned());
                args
            }
        }
    }

    /// Substitute `{file}` in the trigger command template.
    pub fn trigger_command(&self, file: &str) -> String {
        self.trigger_command_template.replace("{file}", file)
    }

    /// Check if a trimmed line matches any prompt pattern.
    pub fn matches_prompt(&self, trimmed_line: &str) -> bool {
        self.prompt_patterns
            .iter()
            .any(|p| trimmed_line == p || trimmed_line.ends_with(p))
    }

    /// Check if a line (potentially with ANSI codes) matches a prompt pattern.
    /// Used by route.rs for pane prompt detection.
    pub fn is_prompt_line(&self, line: &str) -> bool {
        let stripped = crate::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        self.prompt_patterns
            .iter()
            .any(|p| trimmed == p || trimmed.starts_with(&format!("{} ", p)))
    }

    /// Return true when the line is harness UI chrome that should not be treated as
    /// prompt-bearing user/agent output.
    pub fn is_ignorable_output_line(&self, line: &str) -> bool {
        let stripped = crate::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            return true;
        }
        self.binary == "codex" && trimmed.contains("·") && trimmed.contains("Context ")
    }

    /// Return the most recent non-empty, non-footer line from a captured transcript.
    pub fn last_prompt_candidate(&self, output: &str) -> Option<String> {
        output
            .lines()
            .rev()
            .map(crate::prompt::strip_ansi)
            .map(|line| line.trim().to_string())
            .find(|line| !self.is_ignorable_output_line(line))
    }

    /// Check if a process name is recognized as an agent process for this harness.
    pub fn is_agent_process_name(&self, cmd: &str) -> bool {
        cmd.is_empty() || self.process_names.contains(&cmd.to_string())
    }

    /// Check if a command line (from ps) belongs to an agent session.
    #[allow(dead_code)]
    pub fn cmdline_is_agent(&self, cmdline: &str) -> bool {
        self.process_names.iter().any(|name| cmdline.contains(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_defaults() {
        let h = HarnessConfig::claude();
        assert_eq!(h.binary, "claude");
        assert!(h.supports_no_mcp);
        assert!(h.supports_enable_tool_search);
        assert_eq!(
            h.restart_behavior,
            RestartBehavior::Append(vec!["--continue".into()])
        );
        assert_eq!(h.clean_exit_behavior, CleanExitBehavior::PromptUser);
        assert!(h.env_remove.contains(&"CLAUDECODE".to_string()));
        assert_eq!(h.tmux_session_fallback, "claude");
        assert!(h.process_names.contains(&"claude".to_string()));
        assert!(h.process_names.contains(&"node".to_string()));
    }

    #[test]
    fn codex_defaults() {
        let h = HarnessConfig::codex();
        assert_eq!(h.binary, "codex");
        assert!(!h.supports_no_mcp);
        assert!(!h.supports_enable_tool_search);
        assert_eq!(
            h.restart_behavior,
            RestartBehavior::Prepend(vec!["resume".into(), "--last".into()])
        );
        assert_eq!(h.clean_exit_behavior, CleanExitBehavior::RestartContinue);
        assert!(h.env_remove.contains(&"CODEX_CLI".to_string()));
        assert!(h.env_remove.contains(&"CODEX".to_string()));
        assert_eq!(h.tmux_session_fallback, "codex");
        assert!(h.process_names.contains(&"codex".to_string()));
    }

    #[test]
    fn from_agent_name_claude() {
        let h = HarnessConfig::from_agent_name("claude");
        assert_eq!(h.binary, "claude");
    }

    #[test]
    fn from_agent_name_codex() {
        let h = HarnessConfig::from_agent_name("codex");
        assert_eq!(h.binary, "codex");
    }

    #[test]
    fn from_agent_name_unknown_defaults_to_claude() {
        let h = HarnessConfig::from_agent_name("junie");
        assert_eq!(h.binary, "claude");
    }

    #[test]
    fn from_context_uses_frontmatter_agent() {
        let fm = Frontmatter {
            agent: Some("codex".into()),
            ..Default::default()
        };
        let config = Config::default();
        let h = HarnessConfig::from_context(&fm, &config);
        assert_eq!(h.binary, "codex");
    }

    #[test]
    fn from_context_falls_back_to_config_default_agent() {
        let fm = Frontmatter::default();
        let config = Config {
            default_agent: Some("codex".into()),
            ..Default::default()
        };
        let h = HarnessConfig::from_context(&fm, &config);
        assert_eq!(h.binary, "codex");
    }

    #[test]
    fn from_context_falls_back_to_claude() {
        let fm = Frontmatter::default();
        let config = Config::default();
        let h = HarnessConfig::from_context(&fm, &config);
        assert_eq!(h.binary, "claude");
    }

    #[test]
    fn restart_args_append() {
        let h = HarnessConfig::claude();
        let base = vec!["--flag".to_string()];
        let args = h.restart_args(&base);
        assert_eq!(args, vec!["--flag", "--continue"]);
    }

    #[test]
    fn restart_args_prepend() {
        let h = HarnessConfig::codex();
        let base = vec!["--some-flag".to_string()];
        let args = h.restart_args(&base);
        assert_eq!(args, vec!["resume", "--last", "--some-flag"]);
    }

    #[test]
    fn trigger_command_substitution_claude() {
        let h = HarnessConfig::claude();
        assert_eq!(h.trigger_command("plan.md"), "/agent-doc plan.md");
    }

    #[test]
    fn trigger_command_substitution_codex() {
        let h = HarnessConfig::codex();
        assert_eq!(h.trigger_command("plan.md"), "agent-doc plan.md");
    }

    #[test]
    fn matches_prompt_exact() {
        let h = HarnessConfig::claude();
        assert!(h.matches_prompt("❯"));
        assert!(h.matches_prompt("⏵"));
    }

    #[test]
    fn matches_prompt_suffix() {
        let h = HarnessConfig::claude();
        assert!(h.matches_prompt("path/to/dir ❯"));
    }

    #[test]
    fn matches_prompt_no_match() {
        let h = HarnessConfig::claude();
        assert!(!h.matches_prompt("some random text"));
    }

    #[test]
    fn is_prompt_line_unicode() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("❯"));
        assert!(h.is_prompt_line("❯ "));
        assert!(h.is_prompt_line("  ❯  "));
    }

    #[test]
    fn is_prompt_line_with_ansi() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("\x1b[32m❯\x1b[0m"));
        assert!(h.is_prompt_line("\x1b[1m⏵\x1b[0m"));
    }

    #[test]
    fn is_prompt_line_codex_patterns() {
        let h = HarnessConfig::codex();
        assert!(h.is_prompt_line("❯"));
        assert!(h.is_prompt_line(">"));
        assert!(h.is_prompt_line("> "));
        assert!(h.is_prompt_line("  >  "));
        assert!(h.is_prompt_line("›"));
        assert!(h.is_prompt_line("› "));
    }

    #[test]
    fn last_prompt_candidate_skips_codex_footer() {
        let h = HarnessConfig::codex();
        let output = "\
›
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(h.last_prompt_candidate(output).as_deref(), Some("›"));
    }

    #[test]
    fn last_prompt_candidate_preserves_busy_codex_output_above_footer() {
        let h = HarnessConfig::codex();
        let output = "\
›
Working...
gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("Working...")
        );
    }

    #[test]
    fn is_prompt_line_rejects_non_prompt() {
        let h = HarnessConfig::claude();
        assert!(!h.is_prompt_line("Starting claude..."));
        assert!(!h.is_prompt_line(""));
        assert!(!h.is_prompt_line("  "));
        assert!(!h.is_prompt_line("## User"));
    }

    #[test]
    fn is_agent_process_name_claude() {
        let h = HarnessConfig::claude();
        assert!(h.is_agent_process_name("claude"));
        assert!(h.is_agent_process_name("node"));
        assert!(h.is_agent_process_name("agent-doc"));
        assert!(h.is_agent_process_name(""));
        assert!(!h.is_agent_process_name("vim"));
        assert!(!h.is_agent_process_name("codex"));
    }

    #[test]
    fn is_agent_process_name_codex() {
        let h = HarnessConfig::codex();
        assert!(h.is_agent_process_name("codex"));
        assert!(h.is_agent_process_name("node"));
        assert!(h.is_agent_process_name("agent-doc"));
        assert!(!h.is_agent_process_name("claude"));
    }

    #[test]
    fn cmdline_is_agent_claude() {
        let h = HarnessConfig::claude();
        assert!(h.cmdline_is_agent("claude -p --output-format stream-json"));
        assert!(h.cmdline_is_agent("agent-doc start plan.md"));
        assert!(!h.cmdline_is_agent("vim plan.md"));
    }

    #[test]
    fn cmdline_is_agent_codex() {
        let h = HarnessConfig::codex();
        assert!(h.cmdline_is_agent("codex exec --json"));
        assert!(h.cmdline_is_agent("agent-doc start plan.md"));
        assert!(!h.cmdline_is_agent("claude -p"));
    }

    // --- Multi-harness isolation tests ---

    #[test]
    fn harness_isolation_no_shared_binary() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert_ne!(claude.binary, codex.binary);
    }

    #[test]
    fn harness_isolation_no_shared_tmux_session() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert_ne!(claude.tmux_session_fallback, codex.tmux_session_fallback);
    }

    #[test]
    fn harness_isolation_env_remove_disjoint() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        for var in &claude.env_remove {
            assert!(
                !codex.env_remove.contains(var),
                "env_remove overlap: {var} in both claude and codex"
            );
        }
        for var in &codex.env_remove {
            assert!(
                !claude.env_remove.contains(var),
                "env_remove overlap: {var} in both codex and claude"
            );
        }
    }

    #[test]
    fn harness_isolation_process_names_no_cross_claim() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert!(
            claude.is_agent_process_name("claude"),
            "claude harness should claim 'claude'"
        );
        assert!(
            !claude.is_agent_process_name("codex"),
            "claude harness must not claim 'codex'"
        );
        assert!(
            codex.is_agent_process_name("codex"),
            "codex harness should claim 'codex'"
        );
        assert!(
            !codex.is_agent_process_name("claude"),
            "codex harness must not claim 'claude'"
        );
    }

    #[test]
    fn harness_isolation_shared_agent_doc_process() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert!(claude.is_agent_process_name("agent-doc"));
        assert!(codex.is_agent_process_name("agent-doc"));
    }

    #[test]
    fn harness_isolation_trigger_commands_both_route_file() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let claude_cmd = claude.trigger_command("tasks/bugs.md");
        let codex_cmd = codex.trigger_command("tasks/bugs.md");
        assert_eq!(claude_cmd, "/agent-doc tasks/bugs.md");
        assert_eq!(codex_cmd, "agent-doc tasks/bugs.md");
    }

    #[test]
    fn harness_isolation_restart_behavior_types_differ() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let base = vec!["--flag".to_string()];
        let claude_args = claude.restart_args(&base);
        let codex_args = codex.restart_args(&base);
        assert!(
            claude_args.contains(&"--flag".to_string()),
            "claude appends to base"
        );
        assert!(
            codex_args.contains(&"--flag".to_string()),
            "codex preserves base args across resume"
        );
        assert_eq!(
            codex_args[..2],
            ["resume".to_string(), "--last".to_string()],
            "codex restart still prefixes resume mode"
        );
        assert_eq!(claude.clean_exit_behavior, CleanExitBehavior::PromptUser);
        assert_eq!(
            codex.clean_exit_behavior,
            CleanExitBehavior::RestartContinue
        );
    }

    #[test]
    fn harness_isolation_cmdline_cross_rejection() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert!(claude.cmdline_is_agent("claude -p --output-format stream-json"));
        assert!(!claude.cmdline_is_agent("codex exec --json"));
        assert!(codex.cmdline_is_agent("codex exec --json"));
        assert!(!codex.cmdline_is_agent("claude -p --output-format stream-json"));
    }

    #[test]
    fn multi_harness_from_context_independent_resolution() {
        let fm_claude = Frontmatter {
            agent: Some("claude".into()),
            ..Default::default()
        };
        let fm_codex = Frontmatter {
            agent: Some("codex".into()),
            ..Default::default()
        };
        let config = Config::default();
        let h1 = HarnessConfig::from_context(&fm_claude, &config);
        let h2 = HarnessConfig::from_context(&fm_codex, &config);
        assert_eq!(h1.binary, "claude");
        assert_eq!(h2.binary, "codex");
        assert_ne!(h1.tmux_session_fallback, h2.tmux_session_fallback);
    }

    #[test]
    fn multi_harness_config_default_overridden_by_frontmatter() {
        let fm_claude = Frontmatter {
            agent: Some("claude".into()),
            ..Default::default()
        };
        let config = Config {
            default_agent: Some("codex".into()),
            ..Default::default()
        };
        let h = HarnessConfig::from_context(&fm_claude, &config);
        assert_eq!(
            h.binary, "claude",
            "frontmatter agent overrides config default_agent"
        );
    }

    #[test]
    fn multi_harness_prompt_pattern_overlap_is_intentional() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        // Both share ❯ — that's fine, it just means both detect it
        assert!(claude.matches_prompt("❯"));
        assert!(codex.matches_prompt("❯"));
        // > is codex-only
        assert!(!claude.matches_prompt(">"));
        assert!(codex.matches_prompt(">"));
        // ⏵ is claude-only
        assert!(claude.matches_prompt("⏵"));
        assert!(!codex.matches_prompt("⏵"));
    }

    #[test]
    fn multi_harness_feature_flags_exclusive() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        assert!(claude.supports_no_mcp);
        assert!(claude.supports_enable_tool_search);
        assert!(!codex.supports_no_mcp);
        assert!(!codex.supports_enable_tool_search);
    }
}
