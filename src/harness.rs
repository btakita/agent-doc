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
    #[cfg(test)]
    pub fn is_prompt_line(&self, line: &str) -> bool {
        let stripped = crate::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        self.prompt_patterns
            .iter()
            .any(|p| trimmed == p || trimmed.starts_with(&format!("{} ", p)))
            || (self.binary == "claude"
                && trimmed.starts_with("⏵⏵ ")
                && trimmed.contains("(shift+tab to cycle)"))
    }

    /// Return true when the line represents an empty composer that route may
    /// safely inject into. Prompt lines with drafted user text are not idle for
    /// dispatch even if they still begin with the harness prompt glyph.
    pub fn is_dispatch_ready_prompt_line(&self, line: &str) -> bool {
        let stripped = crate::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        match self.binary.as_str() {
            "claude" => {
                matches!(trimmed, "❯" | "⏵")
                    || (trimmed.starts_with("⏵⏵ ") && trimmed.contains("(shift+tab to cycle)"))
            }
            "codex" => {
                matches!(trimmed, "❯" | ">" | "›") || is_codex_idle_placeholder_prompt(trimmed)
            }
            _ => self.matches_prompt(trimmed),
        }
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

    /// Return true when recent pane output indicates the harness is present but
    /// not actually idle for a routed trigger yet.
    pub fn has_busy_cue(&self, output: &str) -> bool {
        self.dispatch_blocker_reason(output).is_some()
    }

    /// Return a short reason when recent pane output shows that route should
    /// not inject a new trigger yet.
    pub fn dispatch_blocker_reason(&self, output: &str) -> Option<String> {
        if crate::prompt::parse_prompt(output).active {
            return Some("active permission prompt".to_string());
        }

        if self.binary != "codex" {
            return None;
        }

        let recent = output
            .lines()
            .rev()
            .take(8)
            .map(crate::prompt::strip_ansi)
            .map(|line| line.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();

        if recent.iter().any(|line| {
            line == "tab to queue message"
                || line.starts_with("tab to queue message ")
                || line.contains(" tab to queue message")
        }) {
            return Some("queued draft in composer".to_string());
        }

        recent.iter().find_map(|line| {
            if line.contains("reverse-i-search") {
                Some("interactive shell reverse-i-search".to_string())
            } else if line.contains("i-search")
                && line.contains("accept")
                && line.contains("cancel")
            {
                Some("interactive shell history search".to_string())
            } else {
                None
            }
        })
    }

    /// Return a reason when the latest Codex pane output shows live user input
    /// or another interactive composer state that must block replacement.
    pub fn protected_prompt_input_reason(&self, output: &str) -> Option<String> {
        if self.binary != "codex" {
            return None;
        }

        if let Some(reason) = self.dispatch_blocker_reason(output) {
            match reason.as_str() {
                "queued draft in composer"
                | "interactive shell reverse-i-search"
                | "interactive shell history search" => return Some(reason),
                _ => {}
            }
        }

        let candidate = self.last_prompt_candidate(output)?;
        let trimmed = crate::prompt::strip_ansi(&candidate);
        let trimmed = trimmed.trim();
        if !matches!(trimmed.chars().next(), Some('>' | '›' | '❯')) {
            return None;
        }
        if self.is_dispatch_ready_prompt_line(trimmed) {
            return None;
        }

        Some("drafted prompt input".to_string())
    }

    /// Return the most recent non-empty, non-footer line from a captured transcript.
    pub fn last_prompt_candidate(&self, output: &str) -> Option<String> {
        if self.binary == "codex"
            && let Some(placeholder) = codex_idle_placeholder_candidate(output)
        {
            return Some(placeholder);
        }
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

fn is_codex_idle_placeholder_prompt(trimmed: &str) -> bool {
    codex_idle_placeholder_prompt(trimmed).is_some()
}

fn codex_idle_placeholder_prompt(trimmed: &str) -> Option<String> {
    let body = trimmed.strip_prefix('›')?.trim();
    if body.is_empty()
        || body
            .chars()
            .any(|c| matches!(c, ':' | ';' | '"' | '\'' | '`' | '\\' | '|' | '&'))
        || matches!(body.chars().last(), Some('.' | '!' | '?' | ',' | ':' | ';'))
    {
        return None;
    }

    let normalized = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let words = normalized.split_whitespace().collect::<Vec<_>>();
    if words.len() < 4 || words.len() > 8 {
        return None;
    }

    let first = words[0];
    if !first
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        return None;
    }

    if !words
        .iter()
        .all(|word| is_safe_codex_placeholder_token(word))
    {
        return None;
    }

    let has_placeholder_target =
        normalized.ends_with("in @filename") || normalized.ends_with("on my current changes");
    if !has_placeholder_target {
        return None;
    }

    Some(format!("› {}", normalized))
}

fn is_safe_codex_placeholder_token(word: &str) -> bool {
    if word == "@filename" {
        return true;
    }

    if let Some(command) = word.strip_prefix('/') {
        return !command.is_empty()
            && command
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-' || ch == '_');
    }

    word.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '-')
}

fn codex_idle_placeholder_candidate(output: &str) -> Option<String> {
    let recent = output
        .lines()
        .rev()
        .take(8)
        .map(crate::prompt::strip_ansi)
        .collect::<Vec<_>>();
    if recent.is_empty() {
        return None;
    }
    let normalized = recent
        .iter()
        .rev()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.contains("· Context "))
        .collect::<Vec<_>>()
        .join(" ");
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if let Some(index) = normalized.find('›') {
        let candidate = normalized[index..].trim();
        return codex_idle_placeholder_prompt(candidate);
    }

    None
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
        assert!(h.is_prompt_line("⏵⏵ bypass permissions on (shift+tab to cycle)"));
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
    fn is_dispatch_ready_prompt_line_rejects_drafted_codex_text() {
        let h = HarnessConfig::codex();
        assert!(h.is_dispatch_ready_prompt_line("›"));
        assert!(h.is_dispatch_ready_prompt_line("> "));
        assert!(!h.is_dispatch_ready_prompt_line("> agent-doc /tmp/session.md"));
        assert!(!h.is_dispatch_ready_prompt_line("› investigate this issue"));
    }

    #[test]
    fn is_dispatch_ready_prompt_line_accepts_idle_codex_placeholders() {
        let h = HarnessConfig::codex();
        assert!(h.is_dispatch_ready_prompt_line("› Run /review on my current changes"));
        assert!(h.is_dispatch_ready_prompt_line("› Find and fix a bug in @filename"));
        assert!(h.is_dispatch_ready_prompt_line("› Improve documentation in @filename"));
        assert!(h.is_dispatch_ready_prompt_line("› Explain this module in @filename"));
    }

    #[test]
    fn is_dispatch_ready_prompt_line_accepts_claude_composer_hint() {
        let h = HarnessConfig::claude();
        assert!(h.is_dispatch_ready_prompt_line("❯"));
        assert!(h.is_dispatch_ready_prompt_line("⏵⏵ bypass permissions on (shift+tab to cycle)"));
        assert!(!h.is_dispatch_ready_prompt_line("❯ investigate this issue"));
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
    fn last_prompt_candidate_detects_wrapped_codex_idle_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Run /review on my current
changes
gpt-5.4 high · ~/work/btakita/agent-loop · Context 20% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Run /review on my current changes")
        );
    }

    #[test]
    fn last_prompt_candidate_detects_new_codex_idle_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Improve documentation in @filename
gpt-5.4 medium · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Improve documentation in @filename")
        );
    }

    #[test]
    fn last_prompt_candidate_detects_future_codex_idle_placeholder_shape() {
        let h = HarnessConfig::codex();
        let output = "\
› Explain this module in @filename
gpt-5.4 medium · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Explain this module in @filename")
        );
    }

    #[test]
    fn last_prompt_candidate_rejects_codex_drafted_filename_text() {
        let h = HarnessConfig::codex();
        let output = "\
› investigate this module in @filename
gpt-5.4 medium · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› investigate this module in @filename")
        );
        assert!(!h.is_dispatch_ready_prompt_line("› investigate this module in @filename"));
    }

    #[test]
    fn has_busy_cue_detects_codex_queue_message_footer() {
        let h = HarnessConfig::codex();
        let output = "\
›
tab to queue message
gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used
";
        assert!(h.has_busy_cue(output));
    }

    #[test]
    fn protected_prompt_input_reason_detects_drafted_codex_text() {
        let h = HarnessConfig::codex();
        let output = "\
› investigate this issue
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("drafted prompt input")
        );
    }

    #[test]
    fn protected_prompt_input_reason_detects_queue_state() {
        let h = HarnessConfig::codex();
        let output = "\
›
tab to queue message
gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used
";
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("queued draft in composer")
        );
    }

    #[test]
    fn protected_prompt_input_reason_ignores_idle_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Explain this module in @filename
gpt-5.4 medium · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
    }

    #[test]
    fn dispatch_blocker_reason_detects_codex_reverse_history_search() {
        let h = HarnessConfig::codex();
        let output = "\
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
reverse-i-search: bugs enter accept · esc cancel
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("interactive shell reverse-i-search")
        );
    }

    #[test]
    fn has_busy_cue_detects_active_permission_prompt() {
        let h = HarnessConfig::claude();
        let output = r#"
  ⎿  Running…

────────────────────────────────────────────────────────
 Bash command

   tmux capture-pane -t %73 -p
   Capture pane content

 Do you want to proceed?
   [1] Yes
 ❯ [2] Yes, and don't ask again for: tmux capture-pane:*
   [3] No

 Esc to cancel · ctrl+e to explain
"#;
        assert!(h.has_busy_cue(output));
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
