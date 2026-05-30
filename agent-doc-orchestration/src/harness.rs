//! # Module: harness
//!
//! ## Spec
//! - Defines `HarnessConfig`: per-agent harness configuration for the supervisor.
//! - Parameterizes binary name, restart behavior, prompt patterns, trigger command,
//!   env vars to remove, and feature support flags.
//! - `RestartBehavior`: how the supervisor restarts after a crash — either append args
//!   to base_args (Claude/OpenCode: `--continue`) or prefix a resume subcommand while preserving
//!   the resolved base args (Codex: `resume --last` + resume-compatible sandbox/model flags).
//! - `HarnessConfig::claude()`, `HarnessConfig::codex()`, and `HarnessConfig::opencode()`
//!   provide defaults.
//! - `HarnessConfig::from_context()` resolves from frontmatter `agent` field,
//!   config `default_agent`, with Claude as fallback.
//!
//! ## Agentic Contracts
//! - `from_context` never fails — falls back to Claude defaults for unknown agents.
//! - `restart_args` returns the full arg list for a restart iteration.
//! - `trigger_command` substitutes the file path into the template.

use crate::config::Config;
use crate::frontmatter::Frontmatter;
use anyhow::{Result, bail};

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

    pub fn opencode() -> Self {
        Self {
            binary: "opencode".into(),
            restart_behavior: RestartBehavior::Append(vec!["--continue".into()]),
            clean_exit_behavior: CleanExitBehavior::RestartContinue,
            prompt_patterns: vec![">".into(), "›".into()],
            trigger_command_template: "/agent-doc {file}".into(),
            env_remove: vec!["OPENCODE_CLIENT".into()],
            supports_no_mcp: false,
            supports_enable_tool_search: false,
            tmux_session_fallback: "opencode".into(),
            process_names: vec![
                "agent-doc".into(),
                "opencode".into(),
                "bun".into(),
                "node".into(),
            ],
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
            "opencode" | "open-code" | "open_code" => Self::opencode(),
            _ => Self::claude(),
        }
    }

    /// Infer harness from a pane's `pane_current_command`. Returns `None` when
    /// the command is ambiguous (e.g. `node`, `agent-doc`) and the caller
    /// should fall back to the document's configured harness.
    pub fn from_pane_command(cmd: &str) -> Option<Self> {
        match cmd {
            "codex" => Some(Self::codex()),
            "opencode" => Some(Self::opencode()),
            "claude" => Some(Self::claude()),
            "bun" => Some(Self::opencode()),
            _ => None,
        }
    }

    /// Build the full arg list for a restart iteration.
    /// On first run, returns `base_args` unchanged.
    /// On restart, applies the `restart_behavior`.
    pub fn restart_args(&self, base_args: &[String]) -> Result<Vec<String>> {
        match &self.restart_behavior {
            RestartBehavior::Append(extra) => {
                let mut args = base_args.to_vec();
                args.extend(extra.iter().cloned());
                Ok(args)
            }
            RestartBehavior::Prepend(prefix) => {
                if self.binary == "codex" {
                    return codex_resume_restart_args(prefix, base_args);
                }
                let mut args = prefix.clone();
                args.extend(base_args.iter().cloned());
                Ok(args)
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
            "opencode" => matches!(trimmed, ">" | "›"),
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
        if is_managed_capability_proof_line(trimmed) {
            return true;
        }
        match self.binary.as_str() {
            "codex" => trimmed.contains("·") && trimmed.contains("Context "),
            "opencode" => is_opencode_idle_chrome_line(trimmed),
            // #jb-stale-busy-idle-footer: skip Claude's static model/context status
            // line (`Opus … ctx:N% … <cwd> <branch> <user>@<host>`) so a genuinely
            // idle pane resolves to the `❯`/`⏵⏵` composer below it. Only the static
            // status line — never the active-turn spinner — matches; combined with
            // the Claude busy cue (checked first in `live_pane_prompt_ready`), this
            // can only surface the composer for panes with no active turn.
            "claude" => is_claude_status_chrome_line(trimmed),
            _ => false,
        }
    }

    pub fn is_idle_status_line(&self, line: &str) -> bool {
        let stripped = crate::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        match self.binary.as_str() {
            "codex" => is_context_usage_status_line(trimmed),
            "opencode" => {
                is_context_usage_status_line(trimmed)
                    || is_opencode_cwd_version_status_line(trimmed)
            }
            _ => false,
        }
    }

    /// Return true when the visible pane contains only status/footer UI
    /// chrome and no busy cue or prompt input. This is not enough for route
    /// dispatch for Codex, but it is enough for operator status/clear to avoid
    /// trusting a stale projected busy state over a visibly idle terminal. For
    /// OpenCode, the TUI can render an idle composer as status chrome only, so
    /// callers may use this as a dispatch-ready signal.
    pub fn is_idle_chrome_only_output(&self, output: &str) -> bool {
        if !matches!(self.binary.as_str(), "codex" | "opencode") || self.has_busy_cue(output) {
            return false;
        }

        let mut saw_status = false;
        for line in output.lines().map(crate::prompt::strip_ansi) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !self.is_ignorable_output_line(trimmed) {
                return false;
            }
            if self.is_idle_status_line(trimmed) || is_opencode_idle_splash_anchor_line(trimmed) {
                saw_status = true;
            }
        }
        saw_status
    }

    /// Return true when recent pane output indicates the harness is present but
    /// not actually idle for a routed trigger yet.
    pub fn has_busy_cue(&self, output: &str) -> bool {
        self.dispatch_blocker_reason(output).is_some()
    }

    /// Return the recent pane line that proves an active turn — the interrupt
    /// hint (`esc to interrupt`) or a working spinner with an elapsed-seconds
    /// timer. Busy-guard refusals cite this concrete proof instead of the
    /// ambiguous composer/permission footer, which shows in both idle and busy
    /// states (#session-restart-refusal-shows-busy-proof). Returns the
    /// original-case, trimmed line, or None when no active-turn proof line is
    /// present (idle, or a non-turn blocker such as a permission prompt).
    pub fn busy_proof_line(&self, output: &str) -> Option<String> {
        output
            .lines()
            .rev()
            .take(8)
            .map(crate::prompt::strip_ansi)
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .find(|line| {
                let lower = line.to_ascii_lowercase();
                lower.contains("esc to interrupt") || is_claude_working_spinner_line(&lower)
            })
    }

    pub fn is_help_screen_output(&self, output: &str) -> bool {
        match self.binary.as_str() {
            "opencode" => is_opencode_help_screen(output),
            _ => false,
        }
    }

    /// Return a short reason when recent pane output shows that route should
    /// not inject a new trigger yet.
    pub fn dispatch_blocker_reason(&self, output: &str) -> Option<String> {
        if crate::prompt::parse_prompt(output).active {
            return Some("active permission prompt".to_string());
        }

        if self.is_help_screen_output(output) {
            return Some("help/usage screen detected".to_string());
        }

        if self.binary == "opencode" {
            let mut has_ready_prompt = false;
            let mut has_non_idle_content = false;

            for line in output.lines().map(crate::prompt::strip_ansi) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if self.is_dispatch_ready_prompt_line(trimmed) {
                    has_ready_prompt = true;
                    continue;
                }
                if !self.is_ignorable_output_line(trimmed) {
                    has_non_idle_content = true;
                }
            }

            if has_non_idle_content && !has_ready_prompt {
                return Some("opencode active turn".to_string());
            }
            return None;
        }

        if self.binary == "claude" {
            // #jb-stale-busy-idle-footer part 1: give Claude a busy cue keyed on the
            // active-turn markers (interrupt hint or the working spinner with an
            // elapsed-seconds timer). This must fire before any composer/footer-based
            // idle inference so a live turn is never mis-read as idle once the static
            // status line becomes ignorable (part 2).
            let recent = output
                .lines()
                .rev()
                .take(8)
                .map(crate::prompt::strip_ansi)
                .map(|line| line.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            if claude_active_turn_busy(&recent) {
                return Some("active claude turn".to_string());
            }
            return None;
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

        if recent.iter().any(|line| {
            (line.starts_with("working (") || line.starts_with("• working ("))
                && line.contains("esc to interrupt")
        }) {
            return Some("active codex turn".to_string());
        }

        if recent.iter().any(|line| {
            line.contains("hook needs review") || line.contains("open /hooks to review")
        }) {
            return Some("codex hook review prompt".to_string());
        }

        recent.iter().find_map(|line| {
            if line.contains("reverse-i-search") {
                Some("interactive shell reverse-i-search".to_string())
            } else if line.contains("i-search")
                && line.contains("accept")
                && line.contains("cancel")
            {
                Some("interactive shell history search".to_string())
            } else if line.contains("press enter to restart") && line.contains("to exit") {
                Some("clean-exit restart prompt".to_string())
            } else {
                None
            }
        })
    }

    /// Return a reason when the latest pane output shows live user input or
    /// another interactive composer state that must block replacement.
    pub fn protected_prompt_input_reason(&self, output: &str) -> Option<String> {
        if let Some(reason) = self.dispatch_blocker_reason(output) {
            match reason.as_str() {
                "active permission prompt" => return Some(reason),
                "queued draft in composer"
                | "interactive shell reverse-i-search"
                | "interactive shell history search" => return Some(reason),
                _ => {}
            }
        }

        if self.binary != "codex" {
            return None;
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
        if codex_prompt_candidate_is_dim_placeholder(output, trimmed) {
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

fn is_opencode_help_screen(output: &str) -> bool {
    let stripped = crate::prompt::strip_ansi(output);
    let subcommand_lines = stripped
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("opencode ") && trimmed.len() > 10
        })
        .count();
    subcommand_lines >= 3
}

fn is_context_usage_status_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("context ") && lower.contains("% used")
}

/// True for Claude Code's static model/context status line, e.g.
/// `Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664` or
/// `Opus 4.8 (1M context) ctx:23% ~/…/agent-loop/resume main brian@host`.
/// Keyed on the `ctx:N%` context indicator, which is unique to this chrome line —
/// it never appears on the `❯`/`⏵⏵` composer or the active-turn spinner — so
/// marking it ignorable cannot hide a real prompt or a busy cue.
/// (#jb-stale-busy-idle-footer)
fn is_claude_status_chrome_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    let Some(pos) = lower.find("ctx:") else {
        return false;
    };
    let rest = &lower.as_bytes()[pos + "ctx:".len()..];
    let mut j = 0;
    while j < rest.len() && rest[j].is_ascii_digit() {
        j += 1;
    }
    j > 0 && j < rest.len() && rest[j] == b'%'
}

/// True when any of the recent (already lower-cased, trimmed) Claude pane lines
/// shows an active turn: the interrupt hint (`esc to interrupt`) or a working
/// spinner with an elapsed-seconds timer (e.g. `· roosting… (14s · ↓ 487 tokens
/// · thinking with high effort)`). (#jb-stale-busy-idle-footer part 1)
fn claude_active_turn_busy(recent_lower: &[String]) -> bool {
    recent_lower
        .iter()
        .any(|line| line.contains("esc to interrupt") || is_claude_working_spinner_line(line))
}

/// True for a Claude working-spinner line: a spinner glyph at the start, a gerund
/// ellipsis (`…`), and an elapsed-seconds timer (`(<N>s`). Requiring all three
/// keeps the idle composer, status, and permissions lines from matching.
fn is_claude_working_spinner_line(line: &str) -> bool {
    let has_spinner = line.starts_with('·')
        || line.starts_with('✶')
        || line.starts_with('✳')
        || line.starts_with('✻')
        || line.starts_with('●')
        || line.starts_with('*');
    has_spinner && line.contains('…') && contains_elapsed_seconds_timer(line)
}

/// True if `line` contains an elapsed-seconds timer token `(<digits>s` (e.g. the
/// `(14s` in a Claude spinner). Byte-scanned so it is safe across the multi-byte
/// glyphs that surround it.
fn contains_elapsed_seconds_timer(line: &str) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'(' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b's' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_opencode_idle_chrome_line(trimmed: &str) -> bool {
    if is_context_usage_status_line(trimmed)
        || is_opencode_idle_splash_anchor_line(trimmed)
        || is_opencode_cwd_version_status_line(trimmed)
    {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("tab agents") && lower.contains("ctrl+p commands") {
        return true;
    }
    if trimmed.contains("Build ·") || trimmed.starts_with("● Tip ") {
        return true;
    }
    is_opencode_box_art_line(trimmed)
}

fn is_opencode_idle_splash_anchor_line(trimmed: &str) -> bool {
    trimmed.contains("Ask anything")
}

fn is_opencode_cwd_version_status_line(trimmed: &str) -> bool {
    let mut parts = trimmed.split_whitespace();
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(last) = trimmed.split_whitespace().next_back() else {
        return false;
    };
    if !(first.starts_with("~/") || first.starts_with('/')) || !first.contains(':') {
        return false;
    }
    let mut segments = last.split('.');
    let Some(major) = segments.next() else {
        return false;
    };
    let Some(minor) = segments.next() else {
        return false;
    };
    let Some(patch) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && major.chars().all(|ch| ch.is_ascii_digit())
        && minor.chars().all(|ch| ch.is_ascii_digit())
        && patch.chars().all(|ch| ch.is_ascii_digit())
}

fn is_opencode_box_art_line(trimmed: &str) -> bool {
    let mut saw_art = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if matches!(
            ch,
            '┃' | '╹'
                | '▀'
                | '▄'
                | '─'
                | '│'
                | '┌'
                | '┐'
                | '└'
                | '┘'
                | '═'
                | '╔'
                | '╗'
                | '╚'
                | '╝'
        ) {
            saw_art = true;
            continue;
        }
        return false;
    }
    saw_art
}

fn is_managed_capability_proof_line(trimmed: &str) -> bool {
    trimmed.contains("_capability_proof status=")
}

fn parse_sandbox_mode_config(value: &str) -> Option<String> {
    let raw = value.trim();
    let mode = raw.strip_prefix("sandbox_mode=")?;
    let mode = mode.trim().trim_matches(|c| c == '"' || c == '\'');
    if mode.is_empty() {
        None
    } else {
        Some(mode.to_string())
    }
}

fn record_codex_resume_sandbox_mode(seen: &mut Option<String>, mode: &str) -> Result<()> {
    if let Some(existing) = seen
        && existing != mode
    {
        bail!(
            "Codex launch policy mismatch: resume args contain conflicting sandbox modes \
             `{existing}` and `{mode}`. Refusing to resume because this could silently \
             downgrade the requested sandbox before task work starts."
        );
    }
    *seen = Some(mode.to_string());
    Ok(())
}

fn push_codex_resume_sandbox_config(
    args: &mut Vec<String>,
    seen_sandbox_mode: &mut Option<String>,
    mode: &str,
) -> Result<()> {
    record_codex_resume_sandbox_mode(seen_sandbox_mode, mode)?;
    args.push("-c".to_string());
    args.push(format!("sandbox_mode={mode:?}"));
    Ok(())
}

fn codex_resume_restart_args(prefix: &[String], base_args: &[String]) -> Result<Vec<String>> {
    let mut args = prefix.to_vec();
    let mut base = base_args.iter().peekable();
    let mut seen_sandbox_mode: Option<String> = None;
    while let Some(arg) = base.next() {
        match arg.as_str() {
            "exec" | "--json" => {}
            "-s" | "--sandbox" => {
                let Some(mode) = base.next() else {
                    bail!(
                        "Codex launch policy mismatch: `{arg}` was provided without a sandbox \
                         mode. Refusing to resume because the session could fall back to the \
                         Codex default sandbox."
                    );
                };
                push_codex_resume_sandbox_config(&mut args, &mut seen_sandbox_mode, mode)?;
            }
            "--add-dir" => {
                // `codex resume` does not accept --add-dir. A resumed session must inherit
                // writable roots from the original fresh launch.
                let _ = base.next();
            }
            "-c" | "--config" => {
                let Some(value) = base.next() else {
                    bail!("Codex launch policy mismatch: `{arg}` was provided without a value.");
                };
                if let Some(mode) = parse_sandbox_mode_config(value) {
                    record_codex_resume_sandbox_mode(&mut seen_sandbox_mode, &mode)?;
                }
                args.push(arg.clone());
                args.push(value.clone());
            }
            _ if arg.starts_with("--sandbox=") => {
                let mode = &arg["--sandbox=".len()..];
                push_codex_resume_sandbox_config(&mut args, &mut seen_sandbox_mode, mode)?;
            }
            _ if arg.starts_with("--add-dir=") => {
                // Same as --add-dir <DIR> above.
            }
            _ if arg.starts_with("--config=") => {
                let value = &arg["--config=".len()..];
                if let Some(mode) = parse_sandbox_mode_config(value) {
                    record_codex_resume_sandbox_mode(&mut seen_sandbox_mode, &mode)?;
                }
                args.push(arg.clone());
            }
            _ => {
                args.push(arg.clone());
            }
        }
    }
    Ok(args)
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
    if normalized == "Ask Codex to do anything" || normalized == "Explain this codebase" {
        return Some(format!("› {}", normalized));
    }

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

    let has_placeholder_target = normalized.ends_with("in @filename")
        || normalized.ends_with("for @filename")
        || normalized.ends_with("on my current changes");
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

    if let Some(index) = normalized.rfind('›') {
        let candidate = normalized[index..].trim();
        if candidate == "›" {
            return Some(candidate.to_string());
        }
        return codex_idle_placeholder_prompt(candidate);
    }

    None
}

fn codex_prompt_candidate_is_dim_placeholder(output: &str, candidate: &str) -> bool {
    let Some(raw_line) = output.lines().rev().find(|line| {
        let stripped = crate::prompt::strip_ansi(line);
        stripped.trim() == candidate
    }) else {
        return false;
    };
    codex_prompt_line_body_starts_dim(raw_line)
}

fn codex_prompt_line_body_starts_dim(raw_line: &str) -> bool {
    let mut faint = false;
    let mut after_prompt = false;
    let mut chars = raw_line.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == '\x1b' && chars.peek().is_some_and(|(_, next)| *next == '[') {
            let _ = chars.next();
            let mut sequence = String::new();
            for (_, seq_ch) in chars.by_ref() {
                if seq_ch.is_ascii_alphabetic() {
                    if seq_ch == 'm' {
                        apply_sgr_sequence(&sequence, &mut faint);
                    }
                    break;
                }
                sequence.push(seq_ch);
            }
            continue;
        }

        if !after_prompt {
            if matches!(ch, '>' | '›' | '❯') {
                after_prompt = true;
            }
            continue;
        }

        if ch.is_whitespace() {
            continue;
        }
        return faint;
    }
    false
}

fn apply_sgr_sequence(sequence: &str, faint: &mut bool) {
    if sequence.is_empty() {
        *faint = false;
        return;
    }
    let codes = sequence
        .split(';')
        .filter_map(|code| code.parse::<u16>().ok())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < codes.len() {
        match codes[index] {
            0 => *faint = false,
            2 => *faint = true,
            22 => *faint = false,
            38 | 48 => {
                if codes.get(index + 1) == Some(&2) {
                    index += 4;
                } else if codes.get(index + 1) == Some(&5) {
                    index += 2;
                }
            }
            _ => {}
        }
        index += 1;
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
    fn opencode_defaults() {
        let h = HarnessConfig::opencode();
        assert_eq!(h.binary, "opencode");
        assert!(!h.supports_no_mcp);
        assert!(!h.supports_enable_tool_search);
        assert_eq!(
            h.restart_behavior,
            RestartBehavior::Append(vec!["--continue".into()])
        );
        assert_eq!(h.clean_exit_behavior, CleanExitBehavior::RestartContinue);
        assert!(h.env_remove.contains(&"OPENCODE_CLIENT".to_string()));
        assert_eq!(h.tmux_session_fallback, "opencode");
        assert!(h.process_names.contains(&"opencode".to_string()));
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
    fn from_agent_name_opencode() {
        let h = HarnessConfig::from_agent_name("opencode");
        assert_eq!(h.binary, "opencode");
    }

    #[test]
    fn from_agent_name_unknown_defaults_to_claude() {
        let h = HarnessConfig::from_agent_name("junie");
        assert_eq!(h.binary, "claude");
    }

    #[test]
    fn from_pane_command_unambiguous_binaries() {
        assert_eq!(
            HarnessConfig::from_pane_command("codex").unwrap().binary,
            "codex"
        );
        assert_eq!(
            HarnessConfig::from_pane_command("opencode").unwrap().binary,
            "opencode"
        );
        assert_eq!(
            HarnessConfig::from_pane_command("claude").unwrap().binary,
            "claude"
        );
        assert_eq!(
            HarnessConfig::from_pane_command("bun").unwrap().binary,
            "opencode"
        );
    }

    #[test]
    fn from_pane_command_ambiguous_returns_none() {
        assert!(HarnessConfig::from_pane_command("node").is_none());
        assert!(HarnessConfig::from_pane_command("agent-doc").is_none());
        assert!(HarnessConfig::from_pane_command("zsh").is_none());
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
        let args = h.restart_args(&base).unwrap();
        assert_eq!(args, vec!["--flag", "--continue"]);
    }

    #[test]
    fn restart_args_prepend() {
        let h = HarnessConfig::codex();
        let base = vec!["--some-flag".to_string()];
        let args = h.restart_args(&base).unwrap();
        assert_eq!(args, vec!["resume", "--last", "--some-flag"]);
    }

    #[test]
    fn codex_restart_translates_sandbox_for_resume() {
        let h = HarnessConfig::codex();
        let base = vec![
            "-s".to_string(),
            "danger-full-access".to_string(),
            "--model".to_string(),
            "gpt-5".to_string(),
        ];
        let args = h.restart_args(&base).unwrap();
        assert_eq!(
            args,
            vec![
                "resume",
                "--last",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
                "--model",
                "gpt-5",
            ]
        );
    }

    #[test]
    fn codex_restart_strips_add_dir_for_resume() {
        let h = HarnessConfig::codex();
        let base = vec![
            "-s".to_string(),
            "danger-full-access".to_string(),
            "--add-dir".to_string(),
            "/tmp/project/.git/modules/sub".to_string(),
            "--add-dir=/tmp/project".to_string(),
        ];
        let args = h.restart_args(&base).unwrap();
        assert_eq!(
            args,
            vec![
                "resume",
                "--last",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
            ]
        );
    }

    #[test]
    fn codex_restart_rejects_conflicting_sandbox_modes() {
        let h = HarnessConfig::codex();
        let base = vec![
            "-s".to_string(),
            "danger-full-access".to_string(),
            "-c".to_string(),
            "sandbox_mode=\"workspace-write\"".to_string(),
        ];
        let err = h.restart_args(&base).unwrap_err().to_string();
        assert!(
            err.contains("conflicting sandbox modes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn codex_restart_rejects_missing_sandbox_value() {
        let h = HarnessConfig::codex();
        let base = vec!["-s".to_string()];
        let err = h.restart_args(&base).unwrap_err().to_string();
        assert!(
            err.contains("provided without a sandbox mode"),
            "unexpected error: {err}"
        );
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

    // #jb-stale-busy-idle-footer — live captures from a real Claude Code session.

    // Idle Claude pane: composer + status + permissions. No active-turn marker.
    const CLAUDE_IDLE_PANE: &str = concat!(
        "────────────────────────────────────────\n",
        "❯\n",
        "────────────────────────────────────────\n",
        "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
    );

    // Busy Claude pane (mid-turn): spinner with elapsed timer above the composer;
    // the permissions line drops `(shift+tab to cycle)` for `· N shell`.
    const CLAUDE_BUSY_PANE: &str = concat!(
        "· Roosting… (14s · ↓ 487 tokens · thinking with high effort)\n",
        "────────────────────────────────────────\n",
        "❯\n",
        "────────────────────────────────────────\n",
        "  Opus 4.8 (1M context) ctx:23% ~/work/btakita/agent-loop/resume main brian@cachyos-x8664\n",
        "  ⏵⏵ bypass permissions on · 1 shell\n",
    );

    #[test]
    fn claude_busy_pane_spinner_is_a_busy_cue() {
        let h = HarnessConfig::claude();
        assert!(
            h.has_busy_cue(CLAUDE_BUSY_PANE),
            "mid-turn spinner with elapsed timer must read as busy"
        );
        assert_eq!(
            h.dispatch_blocker_reason(CLAUDE_BUSY_PANE).as_deref(),
            Some("active claude turn")
        );
    }

    #[test]
    fn claude_esc_to_interrupt_is_a_busy_cue() {
        let h = HarnessConfig::claude();
        let pane = concat!(
            "✶ Generating… (3s · esc to interrupt)\n",
            "❯\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
            "  ⏵⏵ bypass permissions on · 1 shell\n",
        );
        assert!(h.has_busy_cue(pane), "esc to interrupt must read as busy");
    }

    #[test]
    fn claude_idle_pane_has_no_busy_cue() {
        let h = HarnessConfig::claude();
        assert!(
            !h.has_busy_cue(CLAUDE_IDLE_PANE),
            "idle composer pane must not read as busy"
        );
    }

    #[test]
    fn busy_proof_line_returns_active_turn_line_not_footer() {
        // #session-restart-refusal-shows-busy-proof: surface the interrupt/working
        // line, not the ambiguous permission footer.
        let h = HarnessConfig::claude();
        let pane = concat!(
            "• Working (7m 47s · esc to interrupt)\n",
            "› Summarize recent commits\n",
            "  gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/boost-client · Context 60% used\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
        );
        assert_eq!(
            h.busy_proof_line(pane).as_deref(),
            Some("• Working (7m 47s · esc to interrupt)")
        );
    }

    #[test]
    fn busy_proof_line_is_none_for_idle_pane() {
        let h = HarnessConfig::claude();
        assert!(
            h.busy_proof_line(CLAUDE_IDLE_PANE).is_none(),
            "idle composer/permission footer is not a busy-proof line"
        );
    }

    #[test]
    fn claude_status_chrome_line_is_ignorable_but_composer_is_not() {
        let h = HarnessConfig::claude();
        assert!(
            h.is_ignorable_output_line("  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host")
        );
        assert!(h.is_ignorable_output_line("Opus 4.8 (1M context) ctx:23% ~/x/resume main b@h"));
        // The composer and permissions lines must NEVER be ignorable.
        assert!(!h.is_ignorable_output_line("❯"));
        assert!(!h.is_ignorable_output_line("⏵⏵ bypass permissions on (shift+tab to cycle)"));
        assert!(!h.is_ignorable_output_line("⏵⏵ bypass permissions on · 1 shell"));
        // The active-turn spinner must NEVER be ignorable.
        assert!(!h.is_ignorable_output_line(
            "· Roosting… (14s · ↓ 487 tokens · thinking with high effort)"
        ));
    }

    #[test]
    fn last_prompt_candidate_skips_claude_status_line_to_composer() {
        // The plan's question-1 state: the static status line is the last
        // meaningful line. Skipping it must surface the `⏵⏵` composer below.
        let h = HarnessConfig::claude();
        let pane = concat!(
            "❯\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host\n",
        );
        let candidate = h.last_prompt_candidate(pane).unwrap();
        assert!(
            h.is_dispatch_ready_prompt_line(&candidate),
            "status line must be skipped so the composer is the candidate: {candidate:?}"
        );
    }

    #[test]
    fn claude_idle_pane_resolves_to_ready_composer() {
        // End-to-end at the harness layer: idle pane is not busy and its last
        // candidate is the dispatch-ready `⏵⏵ … (shift+tab to cycle)` composer.
        let h = HarnessConfig::claude();
        assert!(!h.has_busy_cue(CLAUDE_IDLE_PANE));
        let candidate = h.last_prompt_candidate(CLAUDE_IDLE_PANE).unwrap();
        assert!(h.is_dispatch_ready_prompt_line(&candidate), "{candidate:?}");
    }

    #[test]
    fn trigger_command_substitution_opencode() {
        let h = HarnessConfig::opencode();
        assert_eq!(h.trigger_command("plan.md"), "/agent-doc plan.md");
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
    fn is_prompt_line_opencode_patterns() {
        let h = HarnessConfig::opencode();
        assert!(h.is_prompt_line(">"));
        assert!(h.is_prompt_line("> "));
        assert!(h.is_prompt_line("  >  "));
        assert!(h.is_prompt_line("›"));
        assert!(h.is_prompt_line("› "));
        assert!(!h.is_prompt_line("❯"));
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
    fn last_prompt_candidate_uses_latest_codex_prompt_after_shell_command() {
        let h = HarnessConfig::codex();
        let output = r#"
$ exec /bin/sh -c 'printf "Starting codex...\n"; printf "› \n"'
Starting codex...
›
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
"#;

        assert_eq!(h.last_prompt_candidate(output).as_deref(), Some("›"));
    }

    #[test]
    fn idle_chrome_only_output_accepts_codex_status_footer_without_prompt() {
        let h = HarnessConfig::codex();
        let output = "\
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
";

        assert!(h.is_idle_chrome_only_output(output));
    }

    #[test]
    fn idle_chrome_only_output_accepts_opencode_status_after_capability_proof() {
        let h = HarnessConfig::opencode();
        let output = "\
[start] managed opencode capability proof: opencode_capability_proof status=proven network=proven network_probe=child_dns_https ssh_targets=0 writable_roots=0 timings_ms=network_host_dns:8,network_child:18812,ssh:not_required,writable_launcher:not_required,writable_child:not_required,total:18820
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";

        assert!(h.is_idle_chrome_only_output(output));
        assert!(h.last_prompt_candidate(output).is_none());
    }

    #[test]
    fn idle_chrome_only_output_accepts_opencode_idle_splash_without_prompt_glyph() {
        let h = HarnessConfig::opencode();
        let output = "\
                                                                                                      ▄
                                                                                                     ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                                   ┃
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";

        assert!(h.is_idle_chrome_only_output(output));
        assert!(h.last_prompt_candidate(output).is_none());
    }

    #[test]
    fn idle_chrome_only_output_rejects_opencode_working_text() {
        let h = HarnessConfig::opencode();
        let output = "\
Working (21s - esc to interrupt)
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";

        assert!(!h.is_idle_chrome_only_output(output));
    }

    #[test]
    fn dispatch_blocker_reason_detects_opencode_active_turn() {
        let h = HarnessConfig::opencode();
        let output = "\
Working (21s - esc to interrupt)
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";

        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("opencode active turn")
        );
    }

    #[test]
    fn idle_chrome_only_output_rejects_codex_status_with_busy_output() {
        let h = HarnessConfig::codex();
        let output = "\
exploring repository
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
";

        assert!(!h.is_idle_chrome_only_output(output));
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
    fn last_prompt_candidate_detects_codex_write_tests_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Write tests for @filename")
        );
        assert!(h.is_dispatch_ready_prompt_line("› Write tests for @filename"));
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
    fn last_prompt_candidate_detects_codex_codebase_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Explain this codebase
gpt-5.5 high · ~/work/btakita/agent-loop · Context 27% used
";
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Explain this codebase")
        );
        assert!(h.is_dispatch_ready_prompt_line("› Explain this codebase"));
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
    fn has_busy_cue_detects_codex_working_status_with_idle_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
• Working (1m 34s • esc to interrupt)

› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("active codex turn")
        );
        assert!(h.has_busy_cue(output));
    }

    #[test]
    fn dispatch_blocker_reason_detects_codex_hook_review_prompt() {
        let h = HarnessConfig::codex();
        let output = "\
Starting codex...
⚠ 1 hook needs review before it can run. Open /hooks to review it.

› [start] managed codex capability proof: codex_capability_proof status=proven network=proven network_probe=child_dns_https ssh_targets=0 writable_roots=0 timings_ms=network_host_dns:8,network_child:9806,ssh:not_required,writable_launcher:not_required,writable_child:not_required,total:9815
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("codex hook review prompt")
        );
        assert!(
            !h.is_idle_chrome_only_output(output),
            "hook review chrome requires operator action and must not count as idle"
        );
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
    fn protected_prompt_input_reason_detects_non_dim_codex_text_with_ansi() {
        let h = HarnessConfig::codex();
        let output = "\
\x1b[1m›\x1b[0m investigate this issue
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("drafted prompt input")
        );
    }

    #[test]
    fn protected_prompt_input_reason_does_not_treat_rgb_color_as_dim() {
        let h = HarnessConfig::codex();
        let output = "\
\x1b[1m›\x1b[0m \x1b[38;2;128;128;128minvestigate this issue\x1b[0m
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("drafted prompt input")
        );
    }

    #[test]
    fn protected_prompt_input_reason_ignores_dim_codex_placeholder_text() {
        let h = HarnessConfig::codex();
        let output = "\
\x1b[1m›\x1b[0m \x1b[2mAsk Codex to do anything\x1b[0m
gpt-5.5 high · ~/work/btakita/agent-loop · Context 21% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
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
    fn protected_prompt_input_reason_ignores_active_codex_turn() {
        let h = HarnessConfig::codex();
        let output = "\
• Working (1m 34s • esc to interrupt)

› Write tests for @filename
gpt-5.5 high · ~/work/btakita/agent-loop · Context 41% used
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("active codex turn")
        );
        assert_eq!(h.protected_prompt_input_reason(output), None);
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
    fn protected_prompt_input_reason_ignores_codebase_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Explain this codebase
gpt-5.5 high · ~/work/btakita/agent-loop · Context 27% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
    }

    #[test]
    fn protected_prompt_input_reason_ignores_default_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
› Ask Codex to do anything
gpt-5.5 high · ~/work/btakita/agent-loop · Context 55% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Ask Codex to do anything")
        );
        assert!(h.is_dispatch_ready_prompt_line("› Ask Codex to do anything"));
    }

    #[test]
    fn protected_prompt_input_reason_skips_non_codex_harnesses() {
        let opencode_output = "\
› investigate this issue
gpt-5.5 high · ~/work/btakita/agent-loop · Context 28% used
";
        assert_eq!(
            HarnessConfig::opencode().protected_prompt_input_reason(opencode_output),
            None,
            "OpenCode harness must not trigger Codex-specific draft detection"
        );
        assert_eq!(
            HarnessConfig::claude().protected_prompt_input_reason(opencode_output),
            None,
            "Claude harness must not trigger Codex-specific draft detection"
        );
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
    fn dispatch_blocker_reason_detects_codex_interactive_history_search() {
        let h = HarnessConfig::codex();
        let output = "\
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
i-search: bug accept · cancel
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("interactive shell history search")
        );
    }

    #[test]
    fn dispatch_blocker_reason_detects_codex_clean_exit_restart_prompt() {
        let h = HarnessConfig::codex();
        let output = "\
Press Enter to restart, or 'q' to exit.
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("clean-exit restart prompt")
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
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("active permission prompt")
        );
    }

    #[test]
    fn dispatch_blocker_reason_detects_opencode_permission_prompt() {
        let h = HarnessConfig::opencode();
        let output = r#"
   ⠙[[[Dd ~/work/btakita/corky/pyproject.toml
┃                                                                                                                       ┃  △ Permission required
┃    ← Access external directory ~/work/btakita/corky/.github/workflows                                                 ┃
┃  Patterns                                                                                                             ┃
┃  - /home/brian/work/btakita/corky/.github/workflows/*                                                                 ┃
┃                                                                                                                       ┃   Allow once   Allow always   Reject                                 ctrl+f fullscreen  ⇆ select  enter confirm
┃
"#;
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("active permission prompt")
        );
        assert_eq!(
            h.protected_prompt_input_reason(output).as_deref(),
            Some("active permission prompt")
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
    fn is_agent_process_name_opencode() {
        let h = HarnessConfig::opencode();
        assert!(h.is_agent_process_name("opencode"));
        assert!(h.is_agent_process_name("bun"));
        assert!(h.is_agent_process_name("node"));
        assert!(h.is_agent_process_name("agent-doc"));
        assert!(!h.is_agent_process_name("claude"));
        assert!(!h.is_agent_process_name("codex"));
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

    #[test]
    fn cmdline_is_agent_opencode() {
        let h = HarnessConfig::opencode();
        assert!(h.cmdline_is_agent("opencode --model zai/glm-5"));
        assert!(h.cmdline_is_agent("agent-doc start plan.md"));
        assert!(!h.cmdline_is_agent("codex exec --json"));
    }

    // --- Multi-harness isolation tests ---

    #[test]
    fn harness_isolation_no_shared_binary() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
        assert_ne!(claude.binary, codex.binary);
        assert_ne!(claude.binary, opencode.binary);
        assert_ne!(codex.binary, opencode.binary);
    }

    #[test]
    fn harness_isolation_no_shared_tmux_session() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
        assert_ne!(claude.tmux_session_fallback, codex.tmux_session_fallback);
        assert_ne!(claude.tmux_session_fallback, opencode.tmux_session_fallback);
        assert_ne!(codex.tmux_session_fallback, opencode.tmux_session_fallback);
    }

    #[test]
    fn harness_isolation_env_remove_disjoint() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
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
        for var in &opencode.env_remove {
            assert!(
                !claude.env_remove.contains(var) && !codex.env_remove.contains(var),
                "env_remove overlap: {var} in opencode and another harness"
            );
        }
    }

    #[test]
    fn harness_isolation_process_names_no_cross_claim() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
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
        assert!(
            opencode.is_agent_process_name("opencode"),
            "opencode harness should claim 'opencode'"
        );
        assert!(
            !opencode.is_agent_process_name("claude") && !opencode.is_agent_process_name("codex"),
            "opencode harness must not claim claude/codex"
        );
    }

    #[test]
    fn harness_isolation_shared_agent_doc_process() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
        assert!(claude.is_agent_process_name("agent-doc"));
        assert!(codex.is_agent_process_name("agent-doc"));
        assert!(opencode.is_agent_process_name("agent-doc"));
    }

    #[test]
    fn harness_isolation_trigger_commands_both_route_file() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let opencode = HarnessConfig::opencode();
        let claude_cmd = claude.trigger_command("tasks/bugs.md");
        let codex_cmd = codex.trigger_command("tasks/bugs.md");
        let opencode_cmd = opencode.trigger_command("tasks/bugs.md");
        assert_eq!(claude_cmd, "/agent-doc tasks/bugs.md");
        assert_eq!(codex_cmd, "agent-doc tasks/bugs.md");
        assert_eq!(opencode_cmd, "/agent-doc tasks/bugs.md");
    }

    #[test]
    fn harness_isolation_restart_behavior_types_differ() {
        let claude = HarnessConfig::claude();
        let codex = HarnessConfig::codex();
        let base = vec!["--flag".to_string()];
        let claude_args = claude.restart_args(&base).unwrap();
        let codex_args = codex.restart_args(&base).unwrap();
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
        let fm_opencode = Frontmatter {
            agent: Some("opencode".into()),
            ..Default::default()
        };
        let config = Config::default();
        let h1 = HarnessConfig::from_context(&fm_claude, &config);
        let h2 = HarnessConfig::from_context(&fm_codex, &config);
        let h3 = HarnessConfig::from_context(&fm_opencode, &config);
        assert_eq!(h1.binary, "claude");
        assert_eq!(h2.binary, "codex");
        assert_eq!(h3.binary, "opencode");
        assert_ne!(h1.tmux_session_fallback, h2.tmux_session_fallback);
        assert_ne!(h2.tmux_session_fallback, h3.tmux_session_fallback);
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
        let opencode = HarnessConfig::opencode();
        // Both share ❯ — that's fine, it just means both detect it
        assert!(claude.matches_prompt("❯"));
        assert!(codex.matches_prompt("❯"));
        assert!(!opencode.matches_prompt("❯"));
        // > is codex-only
        assert!(!claude.matches_prompt(">"));
        assert!(codex.matches_prompt(">"));
        assert!(opencode.matches_prompt(">"));
        // ⏵ is claude-only
        assert!(claude.matches_prompt("⏵"));
        assert!(!codex.matches_prompt("⏵"));
        assert!(!opencode.matches_prompt("⏵"));
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

    #[test]
    fn opencode_help_screen_detected() {
        let h = HarnessConfig::opencode();
        let help_output = "\
opencode [project]           start opencode tui                                          [default]
opencode attach <url>        attach to a running opencode server
opencode run [message..]     run opencode with a message
opencode debug               debugging and troubleshooting tools
opencode providers           manage AI providers and credentials                   [aliases: auth]
";
        assert!(h.is_help_screen_output(help_output));
    }

    #[test]
    fn opencode_help_screen_with_ansi_detected() {
        let h = HarnessConfig::opencode();
        let help_output = "\
\x1b[1mopencode\x1b[0m [project]           start opencode tui
\x1b[1mopencode\x1b[0m run [message..]     run opencode with a message
\x1b[1mopencode\x1b[0m debug               debugging and troubleshooting tools
";
        assert!(h.is_help_screen_output(help_output));
    }

    #[test]
    fn opencode_help_screen_rejects_normal_output() {
        let h = HarnessConfig::opencode();
        assert!(!h.is_help_screen_output("opencode is running\n>"));
        assert!(!h.is_help_screen_output("some output\nmore output\n>"));
        assert!(!h.is_help_screen_output(""));
    }

    #[test]
    fn opencode_help_screen_dispatch_blocker() {
        let h = HarnessConfig::opencode();
        let help_output = "\
opencode [project]           start opencode tui
opencode run [message..]     run opencode with a message
opencode debug               debugging and troubleshooting tools
";
        assert_eq!(
            h.dispatch_blocker_reason(help_output),
            Some("help/usage screen detected".to_string())
        );
    }

    #[test]
    fn claude_help_screen_not_detected() {
        let h = HarnessConfig::claude();
        let help_output =
            "opencode [project]           start opencode tui\nopencode run\nopencode debug\n";
        assert!(!h.is_help_screen_output(help_output));
    }
}
