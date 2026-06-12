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

    pub fn is_tui_harness(&self) -> bool {
        self.binary == "opencode"
    }

    /// Harness-native command that starts a fresh conversation context.
    ///
    /// Claude Code and Codex use `/clear`; OpenCode has no `/clear` command and
    /// uses `/new` for the same operator-visible reset.
    pub fn context_clear_command(&self) -> &'static str {
        if self.binary == "opencode" {
            "/new"
        } else {
            "/clear"
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
                    || is_opencode_footer_version_line(trimmed)
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

    /// Bottom-N counterpart of [`is_idle_chrome_only_output`] for OpenCode
    /// post-turn idle detection. After a turn completes the pane keeps
    /// completed-turn output (bash commands, "Thought:", "Click to expand")
    /// in scrollback above the idle bottom chrome. The all-lines
    /// [`is_idle_chrome_only_output`] returns `false` because those
    /// scrollback lines are non-ignorable, but the bottom shows a genuine
    /// idle composer. This method mirrors the bottom-N strategy used
    /// by [`dispatch_blocker_reason`] for busy-cue detection, but instead
    /// of requiring every bottom line to be chrome, it checks for a
    /// contiguous idle chrome suffix of at least `min_chrome` lines
    /// containing at least one status/footer line.
    pub fn is_bottom_idle_chrome(&self, output: &str, bottom_n: usize) -> bool {
        if !matches!(self.binary.as_str(), "codex" | "opencode") || self.has_busy_cue(output) {
            return false;
        }

        let recent: Vec<String> = output
            .lines()
            .rev()
            .take(bottom_n)
            .map(crate::prompt::strip_ansi)
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        if recent.is_empty() {
            return false;
        }

        let mut chrome_count = 0usize;
        let mut has_status = false;
        let mut has_dispatch_ready_prompt = false;
        for line in &recent {
            if !self.is_ignorable_output_line(line) {
                if self.is_dispatch_ready_prompt_line(line) {
                    has_dispatch_ready_prompt = true;
                    chrome_count += 1;
                    continue;
                }
                break;
            }
            chrome_count += 1;
            if self.is_idle_status_line(line) || is_opencode_idle_splash_anchor_line(line) {
                has_status = true;
            }
        }

        let min_chrome = if self.binary == "opencode" {
            if !recent.is_empty() && is_opencode_footer_version_line(&recent[0]) {
                1
            } else {
                4
            }
        } else {
            1
        };
        (chrome_count >= min_chrome && has_status) || has_dispatch_ready_prompt
    }

    /// Same logic as [`is_bottom_idle_chrome`] but without the `has_busy_cue()`
    /// guard. Used by `dispatch_blocker_reason()` to detect stale busy cues:
    /// when the very bottom of the pane shows a contiguous idle-composer chrome
    /// suffix, any `esc to interrupt` in scrollback above is from a completed
    /// turn and must not block dispatch. (#jb-stale-busy-idle-footer)
    fn bottom_idle_chrome_suffix_present(&self, output: &str, bottom_n: usize) -> bool {
        if !matches!(self.binary.as_str(), "opencode" | "claude") {
            return false;
        }

        let recent: Vec<String> = output
            .lines()
            .rev()
            .take(bottom_n)
            .map(crate::prompt::strip_ansi)
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        if recent.is_empty() {
            return false;
        }

        let mut chrome_count = 0usize;
        let mut has_status = false;
        let mut has_dispatch_ready_prompt = false;
        for line in &recent {
            if !self.is_ignorable_output_line(line) {
                if self.is_dispatch_ready_prompt_line(line) {
                    has_dispatch_ready_prompt = true;
                    chrome_count += 1;
                    continue;
                }
                break;
            }
            chrome_count += 1;
            if self.is_idle_status_line(line) || is_opencode_idle_splash_anchor_line(line) {
                has_status = true;
            }
        }

        let min_chrome = if self.binary == "opencode" { 4 } else { 2 };
        (chrome_count >= min_chrome && has_status) || has_dispatch_ready_prompt
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
            // #opencode-post-turn-false-active: check only the recent bottom
            // lines for a genuine busy cue instead of scanning the whole
            // capture for any non-idle line. After the first turn the pane
            // keeps completed-turn output in scrollback (bash commands,
            // "Thought:", "Click to expand") which is non-chrome but NOT an
            // active turn, and OpenCode's idle state renders no standalone `>`
            // prompt — so the old all-lines `has_non_idle_content &&
            // !has_ready_prompt` heuristic produced false "opencode active
            // turn" stalls on dispatch-only reopen. Mirror the Claude branch's
            // bottom-N busy-cue strategy.
            let recent = output
                .lines()
                .rev()
                .take(12)
                .map(crate::prompt::strip_ansi)
                .map(|line| line.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            if opencode_active_turn_busy(&recent) {
                // #jb-stale-busy-idle-footer: when `esc to interrupt` appears
                // in scrollback but the very bottom of the pane is a contiguous
                // idle-composer chrome suffix (ctrl+p commands, context %,
                // cwd/version status), the busy cue is stale — the turn has
                // completed and the TUI has redrawn the idle footer below the
                // old `Working` banner. Override the busy classification.
                if self.bottom_idle_chrome_suffix_present(output, 12) {
                    return None;
                }
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
                // #jb-stale-busy-idle-footer part 2: when a stale working spinner
                // or interrupt hint sits in scrollback but the bottom of the pane
                // shows idle chrome (status line + empty ❯ composer), override the
                // busy classification — the turn completed and the TUI drew the
                // idle composer below the old active-turn marker.
                if self.bottom_idle_chrome_suffix_present(output, 8) {
                    return None;
                }
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
        if Self::agent_doc_session_control_prompt(trimmed) {
            return None;
        }

        Some("drafted prompt input".to_string())
    }

    fn agent_doc_session_control_prompt(line: &str) -> bool {
        let mut chars = line.chars();
        if !matches!(chars.next(), Some('>' | '›' | '❯')) {
            return false;
        }
        let payload = chars.as_str().trim_start();
        let mut parts = payload.split_whitespace();
        let Some(command) = parts.next() else {
            return false;
        };
        let command_name = command.rsplit(['/', '\\']).next().unwrap_or(command);
        if command_name != "agent-doc" || parts.next() != Some("session") {
            return false;
        }
        matches!(
            parts.next(),
            Some("clear" | "interrupt-clear" | "stop" | "restart" | "restart-supervisor")
        )
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

/// True when any of the recent (already lower-cased, trimmed) OpenCode pane
/// lines shows a genuine active turn. The active-turn cue is the working banner
/// `Working (Ns - esc to interrupt)`; keying on `esc to interrupt` (with the
/// "to") deliberately excludes the idle keybinding hint `esc interrupt` (no
/// "to") and never matches completed-turn scrollback like `Thought:` or
/// `Click to expand` (#opencode-post-turn-false-active).
fn opencode_active_turn_busy(recent_lower: &[String]) -> bool {
    recent_lower
        .iter()
        .any(|line| line.contains("esc to interrupt"))
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
    if is_opencode_idle_keybinding_hint_line(trimmed) {
        return true;
    }
    is_opencode_box_art_line(trimmed) || is_opencode_context_bar_line(trimmed)
}

fn is_opencode_idle_splash_anchor_line(trimmed: &str) -> bool {
    trimmed.contains("Ask anything")
}

/// True for the combined OpenCode footer line that bundles the idle keybinding
/// hints and version number, e.g. `⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13`.
fn is_opencode_footer_version_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("esc interrupt") || lower.contains("esc to interrupt") {
        return false;
    }
    lower.contains("opencode")
        && lower.split_whitespace().next_back().is_some_and(|last| {
            let mut parts = last.split('.');
            parts
                .next()
                .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
                && parts
                    .next()
                    .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
                && parts
                    .next()
                    .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
                && parts.next().is_none()
        })
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

/// True for the idle keybinding hint `esc interrupt` (without "to").
/// The active-turn busy cue is `esc to interrupt`; the idle hint lacks "to".
/// Matches lines like `esc interrupt  ctrl+p commands  OpenCode 1.15.13`.
fn is_opencode_idle_keybinding_hint_line(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("esc interrupt") && !lower.contains("esc to interrupt")
}

/// True for lines consisting solely of the context-usage bar fill character ⬝
/// (U+2B0D) and whitespace. OpenCode renders context usage as a bar of ⬝
/// characters at the bottom of the idle TUI.
fn is_opencode_context_bar_line(trimmed: &str) -> bool {
    let mut saw_bar = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '⬝' {
            saw_bar = true;
            continue;
        }
        return false;
    }
    saw_bar
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
mod tests;
