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

use agent_doc_config::Config;
use agent_doc_frontmatter::frontmatter::Frontmatter;
use agent_doc_turn_executor::codex_launch::codex_resume_restart_args;
use agent_doc_turn_executor_tmux::prompt::{
    codex_idle_placeholder_candidate, codex_prompt_candidate_is_dim_placeholder,
    is_codex_idle_placeholder_prompt,
};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub mod managed_capability;
pub mod prompt_source;
pub mod timeout;

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

pub fn normalize_harness_name(raw: &str) -> String {
    match raw.trim() {
        "" => "default".to_string(),
        "claude" => "claude-code".to_string(),
        other => other.to_string(),
    }
}

pub fn document_harness_from_content(content: &str) -> Option<String> {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.agent)
        .map(|value| normalize_harness_name(&value))
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
        matches!(self.binary.as_str(), "claude" | "codex" | "opencode")
    }

    pub fn supports_goal_command(&self, opencode_goal_extension_available: bool) -> bool {
        match self.binary.as_str() {
            "claude" | "codex" => true,
            "opencode" => opencode_goal_extension_available,
            _ => false,
        }
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
                    return Ok(codex_resume_restart_args(prefix, base_args)?);
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

    /// Force route reopen triggers to the portable bare `agent-doc <file>` form.
    pub fn apply_plain_trigger_override(&mut self) {
        self.trigger_command_template = "agent-doc {file}".to_string();
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
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        self.prompt_patterns
            .iter()
            .any(|p| trimmed == p || trimmed.starts_with(&format!("{} ", p)))
            || (self.binary == "claude"
                && trimmed.starts_with("⏵⏵ ")
                && trimmed.contains("(shift+tab to cycle)"))
    }

    /// True when the line is a rendered composer placeholder rather than real
    /// operator input — an empty composer wearing hint text.
    pub fn is_idle_placeholder_prompt_line(&self, line: &str) -> bool {
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        match self.binary.as_str() {
            "claude" => is_claude_idle_placeholder_prompt(trimmed),
            "codex" => is_codex_idle_placeholder_prompt(trimmed),
            _ => false,
        }
    }

    /// Return true when the line represents an empty composer that route may
    /// safely inject into. Prompt lines with drafted user text are not idle for
    /// dispatch even if they still begin with the harness prompt glyph.
    pub fn is_dispatch_ready_prompt_line(&self, line: &str) -> bool {
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(line);
        let trimmed = stripped.trim();
        match self.binary.as_str() {
            "claude" => {
                matches!(trimmed, "❯" | "⏵")
                    || is_claude_idle_placeholder_prompt(trimmed)
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
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(line);
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
            "claude" => {
                is_claude_status_chrome_line(trimmed)
                    || is_claude_artifact_attachment_line(trimmed)
                    || is_claude_permission_mode_chrome_line(trimmed)
                    || is_claude_composer_rule_line(trimmed)
            }
            _ => false,
        }
    }

    pub fn is_idle_status_line(&self, line: &str) -> bool {
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(line);
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
        for line in output
            .lines()
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        {
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
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
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

    /// Return true when a captured child transcript shows an idle harness
    /// prompt or prompt-equivalent idle chrome.
    pub fn output_prompt_visible(&self, output: &str) -> bool {
        // #opencode-idle-detection-post-turn: for OpenCode, check only the bottom
        // N lines for idle chrome instead of requiring the entire scrollback to be
        // ignorable chrome. After a turn completes the pane keeps completed-turn
        // output in scrollback above the idle bottom chrome; the all-lines
        // is_idle_chrome_only_output returns false for those non-ignorable
        // scrollback lines, preventing idle detection. The bottom-N approach
        // mirrors dispatch_blocker_reason's strategy.
        if self.binary == "opencode" && self.is_bottom_idle_chrome(output, 12) {
            return true;
        }
        if self.is_idle_chrome_only_output(output) {
            return true;
        }
        let Some(line) = self.last_prompt_candidate(output) else {
            return false;
        };
        let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(&line);
        self.matches_prompt(stripped.trim())
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
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
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
            .take(CLAUDE_BUSY_CUE_SCAN_LINES)
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
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
        if agent_doc_turn_executor_tmux::prompt::parse_prompt(output).active {
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
                .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
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
            // #jbsteerinterrupt: Claude's composer chrome is tall (blank row +
            // two box borders + composer + status + permissions hint = 6 rows)
            // and Claude Code renders transient rows above it (rotating `Tip:`
            // hints, `⎿` tool-result rows). A live capture put the spinner at
            // exactly row 8 — the last slot of the old window — so a single extra
            // transient row dropped the busy cue and the route dispatched into
            // the running turn. Scan deep enough to clear the chrome with margin;
            // `bottom_idle_chrome_suffix_present` below still cancels a stale
            // scrollback cue sitting above a redrawn idle footer.
            let recent = output
                .lines()
                .rev()
                .take(CLAUDE_BUSY_CUE_SCAN_LINES)
                .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
                .map(|line| line.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            if claude_artifact_picker_open(&recent) {
                return Some("claude artifact picker open".to_string());
            }
            if claude_active_turn_busy(&recent) {
                // #jbsteerinterrupt: no idle-chrome override for Claude. The
                // `#jb-stale-busy-idle-footer` part-2 override cancelled a live
                // cue here, because the only thing it ever matched on a Claude
                // pane was `has_dispatch_ready_prompt` — the bare `❯` composer
                // and the `⏵⏵ … (shift+tab to cycle)` permissions hint. A live
                // capture proves Claude Code renders BOTH of those *during* an
                // active turn (it accepts type-ahead while working), so they are
                // static chrome, not idle evidence, and the override fired on
                // genuinely busy panes. That is what let the route promote a busy
                // actor to ready and dispatch into the running turn, which Claude
                // Code renders as "Interrupted".
                //
                // Unlike OpenCode's lingering `Working (21s - esc to interrupt)`
                // banner, Claude Code replaces the spinner row with the final
                // response when a turn ends, so there is no stale-spinner shape
                // to suppress. If a cue ever does linger in scrollback the result
                // is a deferred dispatch (queue behind the pane), which is the
                // safe direction: `#realtime-steering-verbatim` says the prompt is
                // already in the document and the running turn should consume it
                // as steering, so never interrupting is strictly correct.
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
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
            .map(|line| line.trim().to_ascii_lowercase())
            .collect::<Vec<_>>();

        if recent.iter().any(|line| {
            line == "tab to queue message"
                || line.starts_with("tab to queue message ")
                || line.contains(" tab to queue message")
        }) {
            return Some("queued draft in composer".to_string());
        }

        if codex_active_turn_busy(&recent) {
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
        let trimmed = agent_doc_turn_executor_tmux::prompt::strip_ansi(&candidate);
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
            .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
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

pub fn protected_prompt_draft_preview(harness: &HarnessConfig, content: &str) -> Option<String> {
    let candidate = harness.last_prompt_candidate(content)?;
    let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(&candidate);
    let redacted = agent_doc_secret_redact::redact(stripped.trim());
    let preview = redacted.trim();
    if preview.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 160;
    let mut chars = preview.chars();
    let shortened: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        Some(format!("{shortened}..."))
    } else {
        Some(shortened)
    }
}

/// Return the operator's unsent composer text when the pane's last prompt
/// candidate IS a harness prompt line that is NOT dispatch-ready — i.e. the
/// composer holds drafted input.
///
/// Returns `None` for an empty/idle composer, and `None` when the last candidate
/// is ordinary output rather than a prompt line, so a busy pane's scrollback is
/// never misreported as an operator draft. Route uses this to distinguish "the
/// run is still booting, waiting will help" from "an operator draft is parked in
/// the composer, waiting will NEVER help" (`#panedraftunblocker`).
pub fn pane_composer_draft(harness: &HarnessConfig, content: &str) -> Option<String> {
    let candidate = harness.last_prompt_candidate(content)?;
    if !harness.is_prompt_line(&candidate) || harness.is_dispatch_ready_prompt_line(&candidate) {
        return None;
    }
    protected_prompt_draft_preview(harness, content)
}

pub fn dispatch_only_blocker_reason(harness: &HarnessConfig, content: &str) -> Option<String> {
    if let Some(reason) = harness.dispatch_blocker_reason(content) {
        return Some(reason);
    }
    if harness.binary != "codex" {
        return None;
    }

    let normalized = agent_doc_turn_executor_tmux::prompt::strip_ansi(content).to_ascii_lowercase();
    if normalized.contains("reverse-i-search") {
        Some("interactive shell reverse-i-search".to_string())
    } else if normalized.contains("i-search")
        && normalized.contains("accept")
        && normalized.contains("cancel")
    {
        Some("interactive shell history search".to_string())
    } else {
        None
    }
}

/// True when the pane's last prompt candidate is an idle, dispatch-ready harness
/// prompt (composer empty and waiting for input), not an active turn.
pub fn pane_idle_dispatch_ready(content: &str, harness: &HarnessConfig) -> bool {
    harness
        .last_prompt_candidate(content)
        .map(|line| harness.is_dispatch_ready_prompt_line(&line))
        .unwrap_or(false)
}

/// Return the latest captured prompt/chrome segment that proves the harness can
/// accept routed input.
pub fn ready_prompt_candidate(content: &str, harness: &HarnessConfig) -> Option<String> {
    let latest_dispatch_ready_prompt = harness
        .last_prompt_candidate(content)
        .filter(|line| harness.is_dispatch_ready_prompt_line(line));
    // See `live_pane_prompt_ready`: only a rendered placeholder composer (which
    // proves input was accepted) may outrank Claude's potentially-stale busy
    // cue. A bare `❯` under a live spinner is an active turn.
    if harness.binary == "claude"
        && latest_dispatch_ready_prompt
            .as_deref()
            .is_some_and(|line| harness.is_idle_placeholder_prompt_line(line))
    {
        return latest_dispatch_ready_prompt;
    }
    if harness.has_busy_cue(content) {
        return None;
    }
    if harness.binary == "opencode" && harness.is_idle_chrome_only_output(content) {
        return Some("opencode idle status chrome".to_string());
    }
    if harness.binary == "codex" && harness.is_bottom_idle_chrome(content, 12) {
        return latest_dispatch_ready_prompt
            .or_else(|| Some("codex idle status chrome".to_string()));
    }
    if harness.binary == "opencode" && harness.is_bottom_idle_chrome(content, 12) {
        return latest_dispatch_ready_prompt.or_else(|| Some("bottom idle chrome".to_string()));
    }
    if harness.binary == "codex"
        && latest_dispatch_ready_prompt.is_some()
        && harness.is_bottom_idle_chrome(content, 12)
    {
        return latest_dispatch_ready_prompt;
    }
    latest_dispatch_ready_prompt
}

fn is_opencode_help_screen(output: &str) -> bool {
    let stripped = agent_doc_turn_executor_tmux::prompt::strip_ansi(output);
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
    lower.contains("context ") && lower.contains("% use")
}

/// True for Claude Code's static model/context status line, e.g.
/// `Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@cachyos-x8664` or
/// `Opus 4.8 (1M context) ctx:23% ~/…/agent-loop/resume main brian@host`.
/// Keyed on the `ctx:N%` context indicator, which is unique to this chrome line —
/// it never appears on the `❯`/`⏵⏵` composer or the active-turn spinner — so
/// marking it ignorable cannot hide a real prompt or a busy cue.
/// (#jb-stale-busy-idle-footer)
/// True for Claude Code's permission-mode footer, e.g.
/// `⏵⏵ bypass permissions on (shift+tab to cycle)` or the shell-bearing variant
/// `⏵⏵ bypass permissions on · 1 shell · ← for agents`.
///
/// Keyed on the `⏵⏵ ` prefix rather than the trailing hint, because that hint
/// varies with pane state. `is_dispatch_ready_prompt_line` only ever recognized
/// the `(shift+tab to cycle)` spelling, so in every other variant this footer
/// became the "last prompt candidate" and masked the real `❯` composer below it
/// — making a genuinely idle pane read as never dispatch-ready and stranding
/// route with `timed_out` (`#panedraftunblocker`). Marking it ignorable resolves
/// the candidate down to the composer, where empty-vs-drafted is decided.
/// Busy panes stay protected by the separate Claude busy cue, which callers
/// check before the prompt candidate.
fn is_claude_permission_mode_chrome_line(trimmed: &str) -> bool {
    trimmed.starts_with("⏵⏵ ")
}

/// True for Claude Code composer text that is a rendered placeholder rather than
/// operator input, e.g. `❯ Press up to edit queued messages` (shown when input
/// was queued during a busy turn — the composer itself is empty).
///
/// The Claude analogue of `is_codex_idle_placeholder_prompt`: such a line is
/// dispatch-ready, and must not be reported as an unsent operator draft.
fn is_claude_idle_placeholder_prompt(trimmed: &str) -> bool {
    const PLACEHOLDERS: &[&str] = &["Press up to edit queued messages"];
    let Some(rest) = trimmed.strip_prefix("❯ ") else {
        return false;
    };
    PLACEHOLDERS.contains(&rest.trim())
}

/// True for the box-drawing rules Claude Code renders above and below the
/// composer. Skipping them lets the prompt candidate resolve to the composer
/// once the permission footer below it is also treated as chrome.
fn is_claude_composer_rule_line(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.chars().all(|c| c == '─')
}

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

/// True when any of the recent (already lower-cased, trimmed) Codex pane lines
/// shows a live turn. Codex can expose an input prompt while a background
/// terminal is still running; that prompt queues text instead of dispatching it.
fn codex_active_turn_busy(recent_lower: &[String]) -> bool {
    recent_lower.iter().any(|line| {
        (line.starts_with("working (")
            || line.starts_with("• working (")
            || line.starts_with("waiting for background terminal")
            || line.starts_with("• waiting for background terminal"))
            && line.contains("esc to interrupt")
    })
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

/// Claude's online-artifact chooser occupies the composer even though no model
/// turn is running. Treat it as a typed dispatch blocker so routed prompts can
/// queue behind the operator-owned modal instead of attempting repair or
/// injecting the trigger as an artifact selection.
fn claude_artifact_picker_open(recent_lower: &[String]) -> bool {
    recent_lower
        .iter()
        .take(3)
        .any(|line| line.starts_with("https://claude.ai/code/artifact/"))
        && recent_lower
            .iter()
            .take(5)
            .any(|line| line.contains("enter to open"))
}

/// A completed Claude online artifact remains attached to the idle composer as
/// a `⧉ <label>` chip. The label is session-owned and arbitrary; the icon is the
/// stable UI shape. This is idle composer chrome, unlike the artifact picker,
/// whose separate `Enter to open` + artifact URL shape remains a typed blocker.
fn is_claude_artifact_attachment_line(line: &str) -> bool {
    line.strip_prefix('⧉').is_some_and(|label| {
        let label = label.trim();
        !label.is_empty() && !label.to_ascii_lowercase().contains("enter to open")
    })
}

/// How many bottom pane rows to scan for a Claude active-turn cue.
///
/// `#jbsteerinterrupt`: Claude Code's idle composer chrome alone is 6 rows and it
/// renders transient rows (rotating `Tip:` hints, `⎿` tool-result lines) between
/// the spinner and that chrome. A live busy capture put the spinner at row 8, so
/// the previous 8-row window had zero margin.
const CLAUDE_BUSY_CUE_SCAN_LINES: usize = 16;

/// True for a Claude working-spinner line: a spinner glyph at the start, a gerund
/// ellipsis (`…`), and an elapsed timer (`(<N>s`, `(<N>m <N>s`, `(<N>h <N>m <N>s`).
/// Requiring all three keeps the idle composer, status, and permissions lines from
/// matching.
///
/// `#jbsteerinterrupt`: the leading glyph is matched structurally (first
/// non-whitespace char is not alphanumeric) rather than against a hardcoded frame
/// list. Claude Code cycles through more spinner frames than any fixed set
/// captured — a live busy pane rendered `✽ Cooking… (3m 43s · ↓ 9.5k tokens)`,
/// whose `✽` (U+273D) was absent from the old `· ✶ ✳ ✻ ● *` set, so a genuinely
/// busy pane read as idle. The `…` + elapsed-timer pair already carries the
/// discrimination; prose and status lines start with a letter and are rejected.
fn is_claude_working_spinner_line(line: &str) -> bool {
    let has_spinner = line
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| !c.is_alphanumeric());
    has_spinner && line.contains('…') && contains_elapsed_seconds_timer(line)
}

/// True if `line` contains an elapsed timer token: `(<digits>s` (e.g. the `(14s`
/// in a Claude spinner) or an hour/minute-qualified form such as `(3m 43s` /
/// `(1h 2m 3s`. Byte-scanned so it is safe across the multi-byte glyphs that
/// surround it.
///
/// `#jbsteerinterrupt`: the minute/hour forms are why a long-running Claude turn
/// used to read as idle. Claude Code switches from `(47s` to `(1m 3s` once a turn
/// passes a minute; the old scanner required the digit run to be followed
/// immediately by `s`, so every turn running longer than 60 seconds lost its busy
/// cue and the route promoted the actor to ready mid-turn.
fn contains_elapsed_seconds_timer(line: &str) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'(' {
            let mut j = i + 1;
            // Accept a chain of `<digits><unit>` segments (`3m 43s`, `1h 2m 3s`)
            // and report a match as soon as one segment closes with `s`.
            loop {
                let digits_start = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j == digits_start || j >= b.len() {
                    break;
                }
                match b[j] {
                    b's' => return true,
                    b'm' | b'h' | b'd' => {
                        j += 1;
                        while j < b.len() && b[j] == b' ' {
                            j += 1;
                        }
                    }
                    _ => break,
                }
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

/// Return true when an OpenCode project/config root exposes an agent-doc
/// compatible `/goal` command extension.
pub fn opencode_goal_extension_available(file: &Path, project_root: Option<&Path>) -> bool {
    opencode_goal_extension_roots(file, project_root)
        .into_iter()
        .any(|root| {
            [
                root.join(".opencode/commands/goal.md"),
                root.join(".opencode/plugin/goal.js"),
                root.join(".opencode/plugin/agent-doc-goal.js"),
                root.join("commands/goal.md"),
                root.join("plugin/goal.js"),
                root.join("plugin/agent-doc-goal.js"),
            ]
            .into_iter()
            .any(|path| path.is_file())
        })
}

fn opencode_goal_extension_roots(file: &Path, project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = project_root {
        roots.push(root.to_path_buf());
    }
    if let Some(parent) = file.parent() {
        roots.push(parent.to_path_buf());
    }
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        roots.push(PathBuf::from(config_home).join("opencode"));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".config/opencode"));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_harness_name_maps_empty_and_claude() {
        assert_eq!(normalize_harness_name(""), "default");
        assert_eq!(normalize_harness_name("   "), "default");
        assert_eq!(normalize_harness_name("claude"), "claude-code");
        assert_eq!(normalize_harness_name(" codex "), "codex");
    }

    #[test]
    fn document_harness_from_content_reads_and_normalizes_agent_frontmatter() {
        let content = "---\nagent: claude\n---\n# Plan\n";
        assert_eq!(
            document_harness_from_content(content),
            Some("claude-code".to_string())
        );
        assert_eq!(
            document_harness_from_content("---\nagent: codex\n---\n# Plan\n"),
            Some("codex".to_string())
        );
        assert_eq!(document_harness_from_content("# No frontmatter\n"), None);
    }

    #[test]
    fn manifest_does_not_depend_on_orchestration() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            !manifest.contains("agent-doc-orchestration"),
            "agent-doc-harness must stay below orchestration"
        );
    }

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
    fn is_tui_harness() {
        assert!(HarnessConfig::claude().is_tui_harness());
        assert!(HarnessConfig::codex().is_tui_harness());
        assert!(HarnessConfig::opencode().is_tui_harness());
    }

    #[test]
    fn goal_command_support_is_harness_and_extension_aware() {
        assert!(HarnessConfig::claude().supports_goal_command(false));
        assert!(HarnessConfig::codex().supports_goal_command(false));
        assert!(!HarnessConfig::opencode().supports_goal_command(false));
        assert!(HarnessConfig::opencode().supports_goal_command(true));
    }

    #[test]
    fn opencode_goal_extension_detects_project_command() {
        let root = unique_temp_dir("agent-doc-harness-goal-project");
        std::fs::create_dir_all(root.join(".opencode/commands")).unwrap();
        std::fs::write(root.join(".opencode/commands/goal.md"), "# goal").unwrap();
        let file = root.join("session.md");

        assert!(opencode_goal_extension_available(&file, Some(&root)));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn opencode_goal_extension_detects_document_parent_plugin() {
        let root = unique_temp_dir("agent-doc-harness-goal-parent");
        std::fs::create_dir_all(root.join("plugin")).unwrap();
        std::fs::write(root.join("plugin/agent-doc-goal.js"), "// goal").unwrap();
        let file = root.join("session.md");

        assert!(opencode_goal_extension_available(&file, None));

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
    fn from_context_picks_up_mid_session_agent_change_for_next_dispatch() {
        // `#agentchange` — the operator's scenario, modelable WITHOUT a live pane:
        // the supervisor re-reads CURRENT frontmatter at each restart/dispatch
        // boundary, so editing `agent:` between turns makes the NEXT dispatch
        // resolve the NEW harness. This is the single source of truth — the only
        // input that changes is the frontmatter `agent:` value.
        let config = Config::default();

        // Turn N: frontmatter says `claude`.
        let fm_turn_n = Frontmatter {
            agent: Some("claude".into()),
            ..Default::default()
        };
        let resolved_turn_n = HarnessConfig::from_context(&fm_turn_n, &config);
        assert_eq!(resolved_turn_n.binary, "claude");

        // Operator edits `agent:` to `codex` between turns.
        let fm_turn_n_plus_1 = Frontmatter {
            agent: Some("codex".into()),
            ..Default::default()
        };

        // Turn N+1: re-resolution from the CURRENT frontmatter reflects the new
        // agent (not the value cached at supervisor startup).
        let resolved_turn_n_plus_1 = HarnessConfig::from_context(&fm_turn_n_plus_1, &config);
        assert_eq!(
            resolved_turn_n_plus_1.binary, "codex",
            "next dispatch must resolve the NEW agent from current frontmatter"
        );
    }

    #[test]
    fn from_context_unchanged_agent_is_inert_across_turns() {
        // INERTNESS guard: an unchanged `agent:` re-resolves to the SAME harness
        // across turns (the same-harness restart path keeps seeing byte-identical
        // harness identity).
        let config = Config::default();
        let fm = Frontmatter {
            agent: Some("claude".into()),
            ..Default::default()
        };
        let turn_n = HarnessConfig::from_context(&fm, &config);
        let turn_n_plus_1 = HarnessConfig::from_context(&fm, &config);
        assert_eq!(turn_n.binary, turn_n_plus_1.binary);
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

    // Idle Claude pane whose composer holds an unsent operator draft. Captured
    // live from pane %38 on 2026-07-19: the turn had finished, the operator had
    // typed a follow-up, and route reported `wait_for_dispatch_ready_prompt` —
    // an unblocker no amount of waiting can satisfy (#panedraftunblocker).
    const CLAUDE_DRAFTED_PANE: &str = concat!(
        "  Pushed as 9c492fd to btakita/acadian-take-home.\n",
        "✻ Churned for 2m 33s · 1 shell still running\n",
        "────────────────────────────────────────\n",
        "❯ keep the uv.lock\n",
        "────────────────────────────────────────\n",
        "  Opus 4.8 ctx:18% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        "  ⏵⏵ bypass permissions on · 1 shell · ← for agents\n",
    );

    // Same idle pane as CLAUDE_IDLE_PANE, but with the shell-bearing permission
    // footer variant that omits `(shift+tab to cycle)`.
    const CLAUDE_IDLE_PANE_SHELL_FOOTER: &str = concat!(
        "────────────────────────────────────────\n",
        "❯\n",
        "────────────────────────────────────────\n",
        "  Opus 4.8 ctx:18% ~/work/btakita/agent-loop main brian@cachyos-x8664\n",
        "  ⏵⏵ bypass permissions on · 1 shell · ← for agents\n",
    );

    #[test]
    fn idle_pane_is_dispatch_ready_under_every_permission_footer_variant() {
        let h = HarnessConfig::claude();
        // Regression: this footer variant used to mask the `❯` composer, so an
        // idle pane read as never dispatch-ready and route stranded on timed_out.
        let candidate = h
            .last_prompt_candidate(CLAUDE_IDLE_PANE_SHELL_FOOTER)
            .unwrap();
        assert_eq!(candidate, "❯", "footer must not mask the composer");
        assert!(h.is_dispatch_ready_prompt_line(&candidate));
        assert!(ready_prompt_candidate(CLAUDE_IDLE_PANE_SHELL_FOOTER, &h).is_some());
        assert_eq!(pane_composer_draft(&h, CLAUDE_IDLE_PANE_SHELL_FOOTER), None);
    }

    #[test]
    fn pane_composer_draft_reports_unsent_operator_input() {
        let h = HarnessConfig::claude();
        assert_eq!(
            pane_composer_draft(&h, CLAUDE_DRAFTED_PANE).as_deref(),
            Some("❯ keep the uv.lock"),
        );
        // The draft is what makes the pane non-dispatchable, so the safety guard
        // and the reported unblocker must agree.
        assert!(ready_prompt_candidate(CLAUDE_DRAFTED_PANE, &h).is_none());
    }

    #[test]
    fn pane_composer_draft_ignores_empty_and_busy_composers() {
        let h = HarnessConfig::claude();
        // Empty composer: dispatch-ready, so there is no draft to clear.
        assert_eq!(pane_composer_draft(&h, CLAUDE_IDLE_PANE), None);
        // Busy pane: the blocker is an active turn, not an operator draft, so
        // scrollback must never be reported as unsent input.
        assert_eq!(pane_composer_draft(&h, CLAUDE_BUSY_PANE), None);
        // A rendered placeholder is an empty composer, not operator input.
        assert!(!h.is_ignorable_output_line("❯ Press up to edit queued messages"));
        assert!(h.is_dispatch_ready_prompt_line("❯ Press up to edit queued messages"));
        assert_eq!(
            pane_composer_draft(
                &h,
                "────────\n❯ Press up to edit queued messages\n⏵⏵ bypass permissions on · 1 shell\n"
            ),
            None
        );
    }

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

    // #jbsteerinterrupt — reproduced row-for-row from a live busy Claude Code
    // pane (2026-07-18). Two properties this capture pins that the older
    // synthetic fixtures missed: the elapsed timer is minute-qualified
    // (`3m 43s`), and the spinner glyph is `✽` (U+273D), which was absent from
    // the old hardcoded frame list. Either alone made a genuinely busy pane read
    // as idle, which let `route_eager_busy_cue_recovery` promote the actor to
    // ready and dispatch the trigger into the running turn — the "Interrupted"
    // the operator saw after JB `Run Agent Doc`.
    const CLAUDE_BUSY_PANE_LONG_RUNNING: &str = concat!(
        "Some earlier tool output line here\n",
        "✽ Cooking… (3m 43s · ↓ 9.5k tokens)\n",
        "  ⎏ ⏵ Tip: Use /voice to enable push-to-talk dictation\n",
        "\n",
        "────────────────────────────────────────\n",
        "❯\n",
        "────────────────────────────────────────\n",
        "  Opus 4.8 ctx:13% ~/…/src/agent-doc main brian@cachyos-x8664\n",
        "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
    );

    #[test]
    fn claude_long_running_turn_is_a_busy_cue() {
        let h = HarnessConfig::claude();
        assert!(
            h.has_busy_cue(CLAUDE_BUSY_PANE_LONG_RUNNING),
            "a turn running past a minute must still read as busy (#jbsteerinterrupt)"
        );
        assert_eq!(
            h.dispatch_blocker_reason(CLAUDE_BUSY_PANE_LONG_RUNNING)
                .as_deref(),
            Some("active claude turn")
        );
        assert!(
            h.busy_proof_line(CLAUDE_BUSY_PANE_LONG_RUNNING)
                .is_some_and(|line| line.contains("Cooking")),
            "busy-guard refusals must cite the live spinner as proof"
        );
    }

    #[test]
    fn claude_elapsed_timer_accepts_minute_and_hour_forms() {
        assert!(contains_elapsed_seconds_timer("(14s · ↓ 200 tokens)"));
        assert!(contains_elapsed_seconds_timer("(3m 43s · ↓ 9.5k tokens)"));
        assert!(contains_elapsed_seconds_timer("(1h 2m 3s)"));
        // No elapsed timer: minutes alone never render without a seconds field.
        assert!(!contains_elapsed_seconds_timer("(shift+tab to cycle)"));
        assert!(!contains_elapsed_seconds_timer("ctx:13% (1M context)"));
    }

    #[test]
    fn claude_spinner_matches_unlisted_frame_glyphs() {
        // Claude Code cycles more frames than any fixed set we captured; the
        // `…` + elapsed-timer pair carries the discrimination.
        for glyph in ['·', '✶', '✳', '✻', '✽', '✢', '∗', '●', '*'] {
            let line = format!("{glyph} thinking… (12s · ↓ 4 tokens)");
            assert!(
                is_claude_working_spinner_line(&line),
                "spinner frame {glyph:?} must read as busy (#jbsteerinterrupt)"
            );
        }
        // Prose and status rows start with a letter and must not match.
        assert!(!is_claude_working_spinner_line(
            "Opus 4.8 ctx:13% … running (12s)"
        ));
    }

    #[test]
    fn claude_busy_cue_survives_transient_rows_above_the_composer() {
        let h = HarnessConfig::claude();
        // Two extra tool-result rows push the spinner past the old 8-row window.
        let pane = concat!(
            "✽ Cooking… (2m 7s · ↓ 9.5k tokens)\n",
            "  ⎿ Read agent-doc-harness/src/lib.rs (120 lines)\n",
            "  ⎿ Ran cargo test -p agent-doc-harness\n",
            "  ⎏ ⏵ Tip: Use /voice to enable push-to-talk dictation\n",
            "\n",
            "────────────────────────────────────────\n",
            "❯\n",
            "────────────────────────────────────────\n",
            "  Opus 4.8 ctx:13% ~/…/src/agent-doc main brian@cachyos-x8664\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
        );
        assert!(
            h.has_busy_cue(pane),
            "transient rows above the composer must not drop the busy cue (#jbsteerinterrupt)"
        );
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
    fn claude_artifact_picker_is_a_typed_dispatch_blocker() {
        let h = HarnessConfig::claude();
        let pane = concat!(
            "❯\n",
            "Opus 4.8 ctx:24% ~/work/btakita/agent-loop/tasks/recruit/haiven main brian@host\n",
            "⏵⏵ bypass permissions on (shift+tab to cycle)\n",
            "hub-benchmarks · Enter to open\n",
            "https://claude.ai/code/artifact/02561a6e-9d8b-462a-82f3-685b92470e57\n",
        );

        assert_eq!(
            h.dispatch_blocker_reason(pane).as_deref(),
            Some("claude artifact picker open")
        );
        assert!(
            !h.is_dispatch_ready_prompt_line("hub-benchmarks · Enter to open"),
            "artifact selection must never be treated as an injectable composer"
        );
    }

    #[test]
    fn claude_attached_artifact_chip_is_idle_composer_chrome() {
        let h = HarnessConfig::claude();
        let pane = concat!(
            "────────────────────────────────────────\n",
            "❯\n",
            "────────────────────────────────────────\n",
            "  Opus 4.8 ctx:24% ~/work/project main brian@host\n",
            "  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents\n",
            "  ⧉  arbitrary-session-artifact-label\n",
        );

        assert!(h.is_ignorable_output_line("⧉ arbitrary label"));
        assert_eq!(h.dispatch_blocker_reason(pane), None);
        let candidate = h.last_prompt_candidate(pane).unwrap();
        assert!(
            h.is_dispatch_ready_prompt_line(&candidate),
            "attachment chip must be skipped to the idle composer: {candidate:?}"
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
            "  gpt-5.5 xhigh · ~/work/btakita/agent-loop/src/sample-app · Context 60% used\n",
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
        assert!(h.is_ignorable_output_line(
            "  Opus 4.8 ctx:40% ~/work/btakita/agent-loop main brian@host"
        ));
        assert!(h.is_ignorable_output_line("Opus 4.8 (1M context) ctx:23% ~/x/resume main b@h"));
        // The composer itself must NEVER be ignorable — it is where
        // empty-vs-drafted is decided.
        assert!(!h.is_ignorable_output_line("❯"));
        assert!(!h.is_ignorable_output_line("❯ keep the uv.lock"));
        // The permission-mode footer IS chrome, in every hint variant. It used
        // to stand in for the composer, but a proxy one line below the real
        // composer cannot see an operator draft parked in it — route would read
        // the footer as dispatch-ready and inject over unsent input. Skipping it
        // resolves the candidate to the `❯` line above (#panedraftunblocker).
        assert!(h.is_ignorable_output_line("⏵⏵ bypass permissions on (shift+tab to cycle)"));
        assert!(h.is_ignorable_output_line("⏵⏵ bypass permissions on · 1 shell"));
        assert!(h.is_ignorable_output_line("⏵⏵ bypass permissions on · 1 shell · ← for agents"));
        // Box-drawing rules that frame the composer are chrome too.
        assert!(h.is_ignorable_output_line("────────────────────────"));
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
        assert!(h.is_dispatch_ready_prompt_line("› Implement {feature}"));
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
    fn idle_chrome_only_output_accepts_codex_context_use_suffix() {
        let h = HarnessConfig::codex();
        let output = "\
gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% use
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
    fn dispatch_blocker_reason_post_turn_output_with_idle_bottom() {
        // #opencode-post-turn-false-active: after a turn completes the pane
        // keeps completed-turn output in scrollback (bash commands, "Thought:",
        // "Click to expand") ABOVE the idle bottom chrome. None of that is an
        // active turn, so dispatch must be allowed (None). The old all-lines
        // scan flagged the scrollback as "opencode active turn".
        let h = HarnessConfig::opencode();
        let output = "\
$ cargo test -p agent-doc-orchestration
   Compiling agent-doc-orchestration
    Finished test profile
Thought: 7.6s
Click to expand
The change is complete and all tests pass.
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
  ~/work/btakita/agent-loop:main                                              esc interrupt                          26.6K (13%)  ctrl+p commands  OpenCode 1.15.13
";

        assert_eq!(
            h.dispatch_blocker_reason(output),
            None,
            "completed-turn scrollback above idle bottom chrome must not read as an active turn"
        );
    }

    #[test]
    fn dispatch_blocker_reason_post_turn_with_active_working_bottom() {
        // The complement: completed-turn scrollback but the BOTTOM shows the
        // live `Working (Ns - esc to interrupt)` banner — still a real active
        // turn, must block dispatch.
        let h = HarnessConfig::opencode();
        let output = "\
$ cargo test
Thought: 3.1s
Click to expand
Working (30s - esc to interrupt)
";

        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("opencode active turn")
        );
    }

    #[test]
    fn dispatch_blocker_reason_allows_opencode_idle_build_chrome() {
        // #opencode-build-chrome-stall: the post-turn OpenCode TUI keeps a
        // `Build · MODEL` status line (and box/footer chrome) without redrawing
        // the `>` prompt glyph. That static chrome must NOT be read as an active
        // turn — only the live `Working (Ns - esc to interrupt)` cue blocks
        // dispatch. Without this, route reported "opencode active turn" forever
        // and the queued preset (#opencode-preset-not-dispatched) never injected.
        let h = HarnessConfig::opencode();
        let output = "\
                                                                                   ┃
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";

        assert_eq!(
            h.dispatch_blocker_reason(output),
            None,
            "idle Build chrome must not be classified as an active turn"
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
    fn is_bottom_idle_chrome_accepts_opencode_post_turn_with_scrollback() {
        let h = HarnessConfig::opencode();
        let output = "\
$ cargo test -p agent-doc-orchestration
   Compiling agent-doc-orchestration
    Finished test profile
     Running unittests src/lib.rs
test result: ok. 2219 passed; 0 failed
Thought: 7.6s
Click to expand
The change is complete and all tests pass. Here is a summary.
src/harness.rs: added is_bottom_idle_chrome method (bottom-N idle detection)
src/harness.rs: tests for is_bottom_idle_chrome (4 tests)
src/start.rs: updated child_output_prompt_visible to use bottom-N for OpenCode
src/start.rs: test for post-turn idle detection
cargo test -p agent-doc-orchestration — 2219 passed, 0 failed
cargo check --bin agent-doc — clean
cargo install — installed agent-doc 0.34.0
                                                                                   ┃
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                    tab agents  ctrl+p commands
                                                                                         ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";

        assert!(
            h.is_bottom_idle_chrome(output, 12),
            "post-turn scrollback above idle bottom chrome must still pass bottom-N idle detection"
        );
        assert!(
            !h.is_idle_chrome_only_output(output),
            "same output must fail the all-lines scan (scrollback is non-ignorable)"
        );
    }

    #[test]
    fn is_bottom_idle_chrome_rejects_opencode_active_turn_bottom() {
        let h = HarnessConfig::opencode();
        let output = "\
$ cargo test
Thought: 3.1s
Click to expand
Working (30s - esc to interrupt)
";

        assert!(
            !h.is_bottom_idle_chrome(output, 12),
            "active working banner in bottom lines must fail idle detection"
        );
    }

    #[test]
    fn is_bottom_idle_chrome_rejects_non_opencode() {
        let h = HarnessConfig::claude();
        assert!(!h.is_bottom_idle_chrome("anything", 12));
    }

    #[test]
    fn is_bottom_idle_chrome_accepts_codex_post_turn_with_scrollback() {
        let h = HarnessConfig::codex();
        let output = "\
Some turn output from Codex
Completed running tests
gpt-5.5 high · ~/work/btakita/agent-loop · Context 69% used
›
";

        assert!(
            h.is_bottom_idle_chrome(output, 12),
            "post-turn Codex output with context status and idle prompt must pass bottom-N idle detection"
        );
    }

    #[test]
    fn is_bottom_idle_chrome_accepts_codex_context_status_only() {
        let h = HarnessConfig::codex();
        let output = "\
Previous turn scrollback line
Another scrollback line
gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used

";

        assert!(
            h.is_bottom_idle_chrome(output, 12),
            "Codex context status line at bottom must pass idle detection"
        );
    }

    #[test]
    fn is_bottom_idle_chrome_rejects_codex_active_turn() {
        let h = HarnessConfig::codex();
        let output = "\
Some output
Working (45s - esc to interrupt)
";

        assert!(
            !h.is_bottom_idle_chrome(output, 12),
            "active Codex turn must fail idle detection"
        );
    }

    #[test]
    fn ready_prompt_candidate_accepts_codex_bottom_idle_chrome() {
        let h = HarnessConfig::codex();
        let output = "\
Previous turn output
gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used
";

        assert_eq!(
            ready_prompt_candidate(output, &h),
            Some("codex idle status chrome".to_string())
        );
    }

    #[test]
    fn ready_prompt_candidate_rejects_codex_busy_footer() {
        let h = HarnessConfig::codex();
        let output = "\
Working (12s - esc to interrupt)
gpt-5.5 high · ~/work/btakita/agent-loop · Context 45% used
";

        assert_eq!(ready_prompt_candidate(output, &h), None);
    }

    #[test]
    fn ready_prompt_candidate_accepts_opencode_idle_splash() {
        let h = HarnessConfig::opencode();
        let output = "\
▄
▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
┃
┃  Ask anything... \"What is the tech stack of this project?\"
┃
┃  Build · GLM-5.1 Z.AI Coding Plan
╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
tab agents  ctrl+p commands
● Tip Toggle username display in chat via command palette (Ctrl+P)
~/work/btakita/agent-loop:main                                                                                                                               1.14.48
";

        assert_eq!(
            ready_prompt_candidate(output, &h),
            Some("opencode idle status chrome".to_string())
        );
    }

    #[test]
    fn is_bottom_idle_chrome_rejects_empty_output() {
        let h = HarnessConfig::opencode();
        assert!(!h.is_bottom_idle_chrome("", 12));
    }

    #[test]
    fn output_prompt_visible_uses_latest_nonempty_line() {
        let h = HarnessConfig::codex();
        let output = "\
old output
❯
resumed child still printing
";
        assert!(
            !h.output_prompt_visible(output),
            "an earlier prompt in the current child transcript should not count once newer non-prompt output follows it"
        );
    }

    #[test]
    fn output_prompt_visible_accepts_prompt_from_current_child_output() {
        let h = HarnessConfig::codex();
        let output = "\
resumed child ready
❯
";
        assert!(h.output_prompt_visible(output));
    }

    #[test]
    fn output_prompt_visible_handles_suffix_prompt_line() {
        let h = HarnessConfig::codex();
        assert!(h.output_prompt_visible("/tmp/project ❯\n"));
    }

    #[test]
    fn output_prompt_visible_skips_codex_footer_line() {
        let h = HarnessConfig::codex();
        let output = "\
›
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
";
        assert!(h.output_prompt_visible(output));
    }

    #[test]
    fn output_prompt_visible_rejects_busy_output_above_codex_footer() {
        let h = HarnessConfig::codex();
        let output = "\
›
resumed child still printing
gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used
";
        assert!(!h.output_prompt_visible(output));
    }

    #[test]
    fn output_prompt_visible_accepts_opencode_status_chrome_without_proof_output() {
        let h = HarnessConfig::opencode();
        assert!(
            h.output_prompt_visible("zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n")
        );
    }

    #[test]
    fn output_prompt_visible_accepts_opencode_idle_splash_without_prompt_glyph() {
        let h = HarnessConfig::opencode();
        let output = "\
                                                                                                 ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                               ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                               ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                               ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                               tab agents  ctrl+p commands
                                                                                    ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";
        assert!(h.output_prompt_visible(output));
    }

    #[test]
    fn output_prompt_visible_detects_opencode_post_turn_idle() {
        let h = HarnessConfig::opencode();
        let output = "\
$ cargo test -p agent-doc-orchestration
   Compiling agent-doc-orchestration
Finished test profile
 Running unittests src/lib.rs
test result: ok. 2219 passed; 0 failed
Thought: 7.6s
Click to expand
The change is complete and all tests pass.
src/harness.rs: added is_bottom_idle_chrome method
src/harness.rs: tests for is_bottom_idle_chrome
src/start.rs: updated child_output_prompt_visible
src/start.rs: test for post-turn idle detection
cargo test -p agent-doc-orchestration -- 2219 passed
cargo check --bin agent-doc -- clean
cargo install -- installed agent-doc 0.34.0
                                                                               ┃
                                                                               ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                               ┃
                                                                               ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                               ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                tab agents  ctrl+p commands
                                                                                     ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";
        assert!(
            h.output_prompt_visible(output),
            "post-turn OpenCode pane with idle bottom chrome must be detected as prompt-visible"
        );
    }

    #[test]
    fn dispatch_blocker_reason_opencode_stale_busy_with_idle_footer() {
        // #jb-stale-busy-idle-footer: the `Working (Ns - esc to interrupt)`
        // banner from a completed turn stays in scrollback within the bottom 12
        // lines, but the actual pane bottom shows idle chrome (box art, Build
        // status, ctrl+p footer, cwd/version line). The busy cue is stale —
        // the turn has completed and the TUI has redrawn the idle footer below
        // the old `Working` banner. The busy guard must NOT block.
        let h = HarnessConfig::opencode();
        let output = "\
previous turn output line 1
previous turn output line 2
Working (21s - esc to interrupt)
                                                                                    ┃
                                                                                    ┃  Ask anything... \"What is the tech stack?\"
                                                                                    ┃
                                                                                    ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                    ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                    tab agents  ctrl+p commands
                                                                                         ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";

        assert_eq!(
            h.dispatch_blocker_reason(output),
            None,
            "stale Working banner above idle footer must not block dispatch (#jb-stale-busy-idle-footer)"
        );
    }

    #[test]
    fn dispatch_blocker_reason_opencode_active_turn_no_idle_footer() {
        // Complement: when `Working (Ns - esc to interrupt)` is the actual
        // bottom (no idle footer suffix below it), it IS a real active turn.
        let h = HarnessConfig::opencode();
        let output = "\
previous output
Working (21s - esc to interrupt)
";

        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("opencode active turn"),
            "real active Working banner without idle footer must still block"
        );
    }

    #[test]
    fn dispatch_blocker_reason_opencode_stale_busy_with_context_footer() {
        // #jb-stale-busy-idle-footer variant: stale `esc to interrupt` in
        // scrollback, but the bottom lines are the full idle footer (box art,
        // ctrl+p, cwd/version status, tip line).
        let h = HarnessConfig::opencode();
        let output = "\
output from previous turn
more previous turn output
Working (15s - esc to interrupt)
thought output
                                                                                    ┃
                                                                                    ┃  Ask anything...
                                                                                    ┃
                                                                                    ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                         ● Tip some tip text
                                                                                                                    tab agents  ctrl+p commands
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";

        assert_eq!(
            h.dispatch_blocker_reason(output),
            None,
            "stale esc to interrupt with idle context footer must not block (#jb-stale-busy-idle-footer)"
        );
    }

    #[test]
    fn bottom_idle_chrome_suffix_present_opencode_with_stale_busy() {
        let h = HarnessConfig::opencode();
        let output = "\
Working (21s - esc to interrupt)
                                                                                    ┃  Ask anything...
                                                                                    ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                    ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                    tab agents  ctrl+p commands
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";
        assert!(
            h.bottom_idle_chrome_suffix_present(output, 12),
            "idle footer below stale Working banner must be detected"
        );
    }

    #[test]
    fn bottom_idle_chrome_suffix_present_rejects_active_turn() {
        let h = HarnessConfig::opencode();
        let output = "\
previous output
Working (21s - esc to interrupt)
";
        assert!(
            !h.bottom_idle_chrome_suffix_present(output, 12),
            "active Working banner with no idle footer must not match"
        );
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
    fn has_busy_cue_detects_codex_background_terminal_with_idle_prompt() {
        let h = HarnessConfig::codex();
        let output = "\
• Waiting for background terminal (18m 36s • esc to interrupt) · 1 background terminal running. /ps to view
  make install

›
29% context left
";
        assert_eq!(
            h.dispatch_blocker_reason(output).as_deref(),
            Some("active codex turn")
        );
        assert_eq!(
            h.busy_proof_line(output).as_deref(),
            Some(
                "• Waiting for background terminal (18m 36s • esc to interrupt) · 1 background terminal running. /ps to view"
            )
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
    fn protected_prompt_input_reason_ignores_agent_doc_session_control_commands() {
        let h = HarnessConfig::codex();
        for prompt in [
            "❯ agent-doc session clear tasks/sampleorders.md",
            "› agent-doc session interrupt-clear tasks/sampleorders.md",
            "> /usr/local/bin/agent-doc session stop tasks/sampleorders.md",
        ] {
            let output =
                format!("{prompt}\ngpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used\n");
            assert_eq!(
                h.protected_prompt_input_reason(&output),
                None,
                "{prompt} should be treated as operator control input"
            );
        }
    }

    #[test]
    fn protected_prompt_input_reason_keeps_agent_doc_non_control_text_protected() {
        let h = HarnessConfig::codex();
        let output = "\
› agent-doc should inspect this session clear bug
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
    fn protected_prompt_input_reason_ignores_unlisted_dim_codex_suggestion() {
        let h = HarnessConfig::codex();
        let output = "\
\x1b[1m›\x1b[0m \x1b[2mSummarize recent commits\x1b[0m
gpt-5.6-sol xhigh · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
        assert!(!h.is_dispatch_ready_prompt_line("› Summarize recent commits"));
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
    fn protected_prompt_input_reason_ignores_startup_feature_placeholder() {
        let h = HarnessConfig::codex();
        let output = "\
╭─────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.142.5)                  │
│                                             │
│ model:     gpt-5.5 xhigh   /model to change │
│ directory: ~/work/sample                    │
╰─────────────────────────────────────────────╯

› Implement {feature}

gpt-5.5 xhigh · ~/work/sample · Context 0% used
";
        assert_eq!(h.protected_prompt_input_reason(output), None);
        assert_eq!(
            h.last_prompt_candidate(output).as_deref(),
            Some("› Implement {feature}")
        );
        assert!(h.is_dispatch_ready_prompt_line("› Implement {feature}"));
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
    fn dispatch_only_blocker_reason_scans_full_codex_capture_for_shell_search() {
        let h = HarnessConfig::codex();
        let output = "\
reverse-i-search: bugs enter accept · esc cancel
pane row 1
pane row 2
pane row 3
pane row 4
pane row 5
pane row 6
pane row 7
pane row 8
pane row 9
";

        assert_eq!(
            h.dispatch_blocker_reason(output),
            None,
            "the normal blocker classifier only inspects recent pane lines"
        );
        assert_eq!(
            dispatch_only_blocker_reason(&h, output).as_deref(),
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
    fn plain_trigger_override_uses_bare_agent_doc_reopen_for_route() {
        let mut claude = HarnessConfig::claude();
        claude.apply_plain_trigger_override();
        assert_eq!(claude.trigger_command("test.md"), "agent-doc test.md");

        let mut opencode = HarnessConfig::opencode();
        opencode.apply_plain_trigger_override();
        assert_eq!(opencode.trigger_command("test.md"), "agent-doc test.md");
    }

    #[test]
    fn protected_prompt_draft_preview_redacts_and_bounds_latest_draft() {
        let harness = HarnessConfig::codex();
        let draft = format!(
            "Implement feature using OPENAI_API_KEY=sk-proj-{} and then {}",
            "a".repeat(32),
            "continue ".repeat(40)
        );
        let content = format!(
            "\
history
› {}
gpt-5.5 xhigh · ~/work/btakita/agent-loop · Context 0% used
",
            draft
        );

        let preview = protected_prompt_draft_preview(&harness, &content).unwrap();

        assert!(preview.starts_with("› Implement feature"), "{preview}");
        assert!(
            preview.contains("OPENAI_API_KEY=[REDACTED]"),
            "preview must redact secrets before surfacing draft text: {preview}"
        );
        assert!(
            !preview.contains("sk-proj-"),
            "raw secret must not leak into route diagnostics: {preview}"
        );
        assert!(preview.ends_with("..."), "{preview}");
        assert!(
            preview.chars().count() <= 163,
            "preview should be bounded plus ellipsis: {preview}"
        );
    }

    #[test]
    fn pane_idle_dispatch_ready_distinguishes_non_dispatch_from_fast_submit() {
        let h = HarnessConfig::claude();
        assert!(
            pane_idle_dispatch_ready("prior output\n\n❯\n", &h),
            "empty composer at an idle prompt is a non-dispatch"
        );
        assert!(
            !pane_idle_dispatch_ready("❯ agent-doc tasks/x.md\n", &h),
            "a drafted trigger in the composer is not idle"
        );
        assert!(
            !pane_idle_dispatch_ready("Working… (esc to interrupt)\n", &h),
            "a processing pane is not idle"
        );
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
    fn context_clear_command_uses_new_for_opencode_only() {
        assert_eq!(HarnessConfig::claude().context_clear_command(), "/clear");
        assert_eq!(HarnessConfig::codex().context_clear_command(), "/clear");
        assert_eq!(HarnessConfig::opencode().context_clear_command(), "/new");
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

    #[test]
    fn opencode_context_bar_line_recognized() {
        assert!(is_opencode_context_bar_line("⬝⬝⬝⬝⬝⬝⬝⬝"));
        assert!(is_opencode_context_bar_line("  ⬝⬝⬝⬝  "));
        assert!(!is_opencode_context_bar_line("⬝⬝⬝ some text"));
        assert!(!is_opencode_context_bar_line(""));
    }

    #[test]
    fn opencode_idle_keybinding_hint_line_recognized() {
        assert!(is_opencode_idle_keybinding_hint_line(
            "esc interrupt  ctrl+p commands  OpenCode 1.15.13"
        ));
        assert!(is_opencode_idle_keybinding_hint_line("esc interrupt"));
        assert!(!is_opencode_idle_keybinding_hint_line("esc to interrupt"));
        assert!(!is_opencode_idle_keybinding_hint_line(
            "Working (14s - esc to interrupt)"
        ));
    }

    #[test]
    fn opencode_idle_chrome_line_with_context_bar() {
        assert!(is_opencode_idle_chrome_line("⬝⬝⬝⬝⬝⬝⬝⬝"));
        assert!(is_opencode_idle_chrome_line(
            "esc interrupt  ctrl+p commands  OpenCode 1.15.13"
        ));
    }

    #[test]
    fn opencode_bottom_idle_chrome_with_context_bar_and_scrollback() {
        let h = HarnessConfig::opencode();
        let output = "\
Thought: checking files
Click to expand
  ~/work/btakita/agent-loop:main                                        1.15.13
⬝⬝⬝⬝⬝⬝⬝⬝  esc interrupt  ctrl+p commands  OpenCode 1.15.13
";
        assert!(h.is_bottom_idle_chrome(output, 12));
        assert!(!h.is_idle_chrome_only_output(output));
    }
}


