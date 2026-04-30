//! # Module: prompt
//!
//! ## Spec
//! - Detects active Claude Code permission/question prompts in a tmux pane and surfaces them as JSON.
//! - `run(file)` — resolves the session's tmux pane from the document frontmatter, captures pane content, parses for prompt patterns, and prints a `PromptInfo` JSON object to stdout.
//! - `run_all()` — iterates every entry in `sessions.json`, skips dead panes, and prints a JSON array of `PromptAllEntry` objects (one per live session with its prompt state).
//! - `answer(file, option_index)` — navigates the Claude Code TUI to the target option using Up/Down arrow keys (30 ms between presses) then sends Enter. Validates that a prompt is active and the index is in range before sending keys.
//! - Prompt detection anchors on the footer pattern `"Esc to cancel"` (searched bottom-up), then scans upward for options in bracket format (`[N] label`) or numbered list format (`N. label`), then identifies the question as the first non-empty, non-option line above.
//! - ANSI escape codes are stripped before pattern matching; `strip_ansi` is `pub(crate)` for reuse.
//! - `selected` is 0-based (reflecting TUI cursor position); `options[*].index` is 1-based (matching the TUI display).
//! - Both bracket format (`[N] label`) and numbered list format (`N. label`) are detected as prompt options.
//! - When no pane is registered or the pane is dead, `run` emits `{"active":false}` and returns `Ok(())`.
//! - Optional fields (`question`, `options`, `selected`) are omitted from JSON serialization when `None`.
//!
//! ## Agentic Contracts
//! - `run(file)` / `run_with_tmux(file, tmux)` — returns `Err` only on file I/O or tmux command failure; missing/dead pane produces `active: false` output, not an error.
//! - `answer(file, option_index)` — returns `Err` when: file missing, no pane registered, pane dead, no active prompt, or index out of range (1-based).
//! - `parse_prompt(content)` is `pub` and deterministic; callers may unit-test it directly.
//! - `PromptAllEntry` serializes flat (prompt fields at top level via `#[serde(flatten)]`).
//!
//! ## Evals
//! - parse_permission_prompt: three-option prompt with `❯` cursor on option 2 → active, correct question, 3 options, selected=1 (0-based)
//! - parse_no_prompt: plain output without footer → inactive PromptInfo
//! - parse_yes_no_prompt: two-option prompt → active, 2 options
//! - markdown_list_not_prompt: numbered markdown list above `Esc to cancel` → inactive (not detected as options)
//! - strip_ansi_basic: bold/reset escape sequence → plain text without escape codes
//! - parse_option_line_with_cursor: `❯ [2] label` → index=2, label extracted
//! - parse_option_line_numbered_format: `1. Yes` / `❯ 3. No` → index + label extracted
//! - prompt_all_entry_serializes_flat: active entry → `session_id`, `file`, `active`, `question` all at JSON top level
//! - prompt_all_entry_inactive_omits_optional: inactive entry → no `question`/`options` keys in JSON

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::sessions::{SessionRegistry, Tmux};
use crate::{frontmatter, sessions};

#[derive(Debug, Serialize)]
pub struct PromptInfo {
    /// Whether a prompt is currently active
    pub active: bool,
    /// The question text (if active)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// Available options (if active)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<PromptOption>>,
    /// Index of the currently selected option (0-based)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct PromptOption {
    /// 1-based index as shown in the TUI
    pub index: usize,
    /// The option label text
    pub label: String,
}

pub fn run(file: &Path) -> Result<()> {
    run_with_tmux(file, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (_updated, session_id) = frontmatter::ensure_session(&content)?;

    let pane = sessions::lookup(&session_id)?;
    let pane_id = match pane {
        Some(p) => p,
        None => {
            let info = PromptInfo {
                active: false,
                question: None,
                options: None,
                selected: None,
            };
            println!("{}", serde_json::to_string(&info)?);
            return Ok(());
        }
    };

    if !tmux.pane_alive(&pane_id) {
        let info = PromptInfo {
            active: false,
            question: None,
            options: None,
            selected: None,
        };
        println!("{}", serde_json::to_string(&info)?);
        return Ok(());
    }

    let pane_content = sessions::capture_pane(tmux, &pane_id)?;
    let info = parse_prompt(&pane_content);
    println!("{}", serde_json::to_string(&info)?);
    Ok(())
}

/// Entry in the `--all` output: one per live session.
#[derive(Debug, Serialize)]
pub struct PromptAllEntry {
    pub session_id: String,
    pub file: String,
    #[serde(flatten)]
    pub prompt: PromptInfo,
}

/// Poll all live sessions for active prompts.
pub fn run_all() -> Result<()> {
    run_all_with_tmux(&Tmux::default_server())
}

pub fn run_all_with_tmux(tmux: &Tmux) -> Result<()> {
    let registry: SessionRegistry = sessions::load()?;
    let mut entries: Vec<PromptAllEntry> = Vec::new();
    let verbose = std::env::var("AGENT_DOC_PROMPT_DEBUG").is_ok();

    for entry in registry.values() {
        if !tmux.pane_alive(&entry.pane) {
            if verbose {
                eprintln!(
                    "[prompt] pane {} dead for session {} ({})",
                    entry.pane, entry.session_id, entry.file
                );
            }
            continue;
        }

        let prompt = match sessions::capture_pane(tmux, &entry.pane) {
            Ok(content) => {
                if verbose {
                    // Log the last 5 non-empty lines for debugging prompt detection
                    let last_lines: Vec<&str> = content
                        .lines()
                        .rev()
                        .filter(|l| !l.trim().is_empty())
                        .take(5)
                        .collect();
                    eprintln!("[prompt] pane {} ({}) last lines:", entry.pane, entry.file);
                    for line in last_lines.iter().rev() {
                        eprintln!("[prompt]   {}", strip_ansi(line));
                    }
                }
                parse_prompt(&content)
            }
            Err(e) => {
                if verbose {
                    eprintln!("[prompt] capture failed for pane {}: {}", entry.pane, e);
                }
                inactive()
            }
        };

        if verbose {
            eprintln!(
                "[prompt] session {} active={} question={:?}",
                entry.session_id, prompt.active, prompt.question
            );
        }

        entries.push(PromptAllEntry {
            session_id: entry.session_id.clone(),
            file: entry.file.clone(),
            prompt,
        });
    }

    println!("{}", serde_json::to_string(&entries)?);
    Ok(())
}

pub fn answer(file: &Path, option_index: usize) -> Result<()> {
    answer_with_tmux(file, option_index, &Tmux::default_server())
}

pub fn answer_with_tmux(file: &Path, option_index: usize, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (_updated, session_id) = frontmatter::ensure_session(&content)?;

    let pane = sessions::lookup(&session_id)?;
    let pane_id = pane.context("no pane registered for this session")?;

    if !tmux.pane_alive(&pane_id) {
        anyhow::bail!("pane {} is not alive", pane_id);
    }

    // Verify there's actually a prompt active
    let pane_content = sessions::capture_pane(tmux, &pane_id)?;
    let info = parse_prompt(&pane_content);
    if !info.active {
        anyhow::bail!("no active prompt detected");
    }

    let options = info.options.as_ref().unwrap();
    if option_index == 0 || option_index > options.len() {
        anyhow::bail!("option {} out of range (1-{})", option_index, options.len());
    }

    // Navigate to the selected option and press Enter.
    // Claude Code's TUI uses arrow keys for navigation.
    // We need to move from the currently selected item to the target.
    let current = info.selected.unwrap_or(0);
    let target = option_index - 1; // convert to 0-based

    if target < current {
        // Move up
        for _ in 0..(current - target) {
            sessions::send_key(tmux, &pane_id, "Up")?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    } else if target > current {
        // Move down
        for _ in 0..(target - current) {
            sessions::send_key(tmux, &pane_id, "Down")?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    // Brief pause then press Enter to confirm
    std::thread::sleep(std::time::Duration::from_millis(50));
    sessions::send_key(tmux, &pane_id, "Enter")?;

    eprintln!("Sent option {} to pane {}", option_index, pane_id);
    Ok(())
}

/// Parse tmux pane content for Claude Code permission prompts.
///
/// Looks for patterns like:
/// ```text
///  Do you want to proceed?
///    [1] Yes
///  ❯ [2] Yes, and don't ask again for: ...
///    [3] No
///
///  Esc to cancel · ctrl+e to explain
/// ```
pub fn parse_prompt(content: &str) -> PromptInfo {
    let lines: Vec<&str> = content.lines().collect();

    // Strip ANSI escape codes for pattern matching
    let stripped: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();

    // Search for the prompt pattern from the bottom up (most recent prompt)
    // Look for the footer pattern first — Claude Code uses variations:
    //   "Esc to cancel" (old format)
    //   "No to cancel" (new format, Claude Code v2.1+)
    //   "to cancel" (catch-all for future variations)
    let footer_idx = stripped.iter().rposition(|line| line.contains("to cancel"));

    let footer_idx = match footer_idx {
        Some(idx) => idx,
        None => return inactive(),
    };

    // Work backwards from the footer to find options and the question
    let mut options = Vec::new();
    let mut selected: Option<usize> = None;
    let mut question_line_idx: Option<usize> = None;

    // Scan upward from footer for numbered options
    for i in (0..footer_idx).rev() {
        let line = &stripped[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // Empty line above options = we've passed all options
            if !options.is_empty() {
                // Continue scanning for the question
                continue;
            }
            continue;
        }

        // Check for bracket option pattern: "[N] label" with optional ❯ prefix
        if let Some(opt) = parse_option_line(trimmed) {
            let is_selected = trimmed.starts_with('❯') || trimmed.starts_with('>');
            if is_selected {
                selected = Some(opt.index - 1); // 0-based
            }
            options.push(opt);
        } else if !options.is_empty() {
            // First non-option, non-empty line above options = the question
            question_line_idx = Some(i);
            break;
        }
    }

    if options.is_empty() {
        return inactive();
    }

    // Options were found bottom-up, reverse to get top-down order
    options.reverse();

    let question = question_line_idx.map(|idx| stripped[idx].trim().to_string());

    PromptInfo {
        active: true,
        question,
        options: Some(options),
        selected,
    }
}

/// Parse a single option line like "[1] Yes", "1. Yes", or "❯ [2] Yes, and don't ask..."
///
/// Matches two formats used by Claude Code's TUI:
/// - Bracket: `[N] label` (older versions)
/// - Numbered: `N. label` (Claude Code v2.1+)
fn parse_option_line(line: &str) -> Option<PromptOption> {
    // Strip leading ❯ or > marker
    let stripped = line.trim_start_matches('❯').trim_start_matches('>').trim();

    // Try bracket format: "[N] label"
    if stripped.starts_with('[') {
        let bracket_close = stripped.find(']')?;
        let num_str = &stripped[1..bracket_close];
        let index: usize = num_str.parse().ok()?;
        let label = stripped[bracket_close + 1..].trim().to_string();
        if label.is_empty() {
            return None;
        }
        return Some(PromptOption { index, label });
    }

    // Try numbered list format: "N. label"
    let dot_pos = stripped.find('.')?;
    let num_str = &stripped[..dot_pos];
    let index: usize = num_str.parse().ok()?;
    let label = stripped[dot_pos + 1..].trim().to_string();
    if label.is_empty() {
        return None;
    }
    Some(PromptOption { index, label })
}

/// Strip ANSI escape codes from a string.
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Consume the escape sequence
            if let Some(next) = chars.next()
                && next == '['
            {
                // CSI sequence: consume until a letter is found
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // Otherwise just skip the next char (two-byte escape)
        } else {
            result.push(c);
        }
    }
    result
}

fn inactive() -> PromptInfo {
    PromptInfo {
        active: false,
        question: None,
        options: None,
        selected: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_permission_prompt() {
        let content = r#"
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

        let info = parse_prompt(content);
        assert!(info.active);
        assert_eq!(info.question.as_deref(), Some("Do you want to proceed?"));
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].index, 1);
        assert_eq!(opts[0].label, "Yes");
        assert_eq!(opts[1].index, 2);
        assert!(opts[1].label.starts_with("Yes, and don't ask again"));
        assert_eq!(opts[2].index, 3);
        assert_eq!(opts[2].label, "No");
        assert_eq!(info.selected, Some(1)); // 0-based, option 2
    }

    #[test]
    fn parse_no_prompt() {
        let content = "Hello world\nSome regular output\n";
        let info = parse_prompt(content);
        assert!(!info.active);
    }

    #[test]
    fn parse_yes_no_prompt() {
        let content = r#"
 Read tool

   /home/brian/file.txt

 Allow this action?
   [1] Yes
   [2] No

 Esc to cancel
"#;
        let info = parse_prompt(content);
        assert!(info.active);
        assert_eq!(info.question.as_deref(), Some("Allow this action?"));
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 2);
    }

    #[test]
    fn strip_ansi_basic() {
        let s = "\x1b[1mBold\x1b[0m Normal";
        assert_eq!(strip_ansi(s), "Bold Normal");
    }

    #[test]
    fn strip_ansi_colors() {
        let s = "\x1b[32mGreen\x1b[0m \x1b[31mRed\x1b[0m";
        assert_eq!(strip_ansi(s), "Green Red");
    }

    #[test]
    fn parse_option_line_basic() {
        let opt = parse_option_line("[1] Yes").unwrap();
        assert_eq!(opt.index, 1);
        assert_eq!(opt.label, "Yes");
    }

    #[test]
    fn parse_option_line_with_cursor() {
        let opt = parse_option_line("❯ [2] Yes, and don't ask again").unwrap();
        assert_eq!(opt.index, 2);
        assert_eq!(opt.label, "Yes, and don't ask again");
    }

    #[test]
    fn parse_option_line_no_match() {
        assert!(parse_option_line("Not an option").is_none());
        assert!(parse_option_line("").is_none());
    }

    #[test]
    fn parse_option_line_numbered_format() {
        // Claude Code v2.1+ uses numbered list format
        let opt = parse_option_line("1. Yes").unwrap();
        assert_eq!(opt.index, 1);
        assert_eq!(opt.label, "Yes");

        let opt =
            parse_option_line("2. Yes, allow reading from agent-loop/ from this project").unwrap();
        assert_eq!(opt.index, 2);
        assert_eq!(
            opt.label,
            "Yes, allow reading from agent-loop/ from this project"
        );

        let opt = parse_option_line("❯ 3. No").unwrap();
        assert_eq!(opt.index, 3);
        assert_eq!(opt.label, "No");
    }

    #[test]
    fn parse_numbered_format_prompt() {
        let content = r#"
────────────────────────────────────────────────────────
 Bash command

   agent-doc preflight tasks/software/agent-doc.md

 Do you want to proceed?
   1. Yes
   2. Yes, allow reading from agent-loop/ from this project
   3. No

 Esc to cancel · Tab to amend · ctrl+e to explain
"#;
        let info = parse_prompt(content);
        assert!(info.active, "numbered format prompt should be detected");
        assert_eq!(info.question.as_deref(), Some("Do you want to proceed?"));
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].index, 1);
        assert_eq!(opts[0].label, "Yes");
        assert_eq!(opts[2].index, 3);
        assert_eq!(opts[2].label, "No");
    }

    #[test]
    fn parse_new_format_no_to_cancel() {
        let content = r#"
────────────────────────────────────────────────────────
 Bash command

   cp tmp/file.txt /tmp/dest.txt

 Do you want to proceed?
   [1] Yes
   [2] Yes, and always allow Claude to edit for this project
   [3] No

 No to cancel · ctrl+e to explain
"#;
        let info = parse_prompt(content);
        assert!(info.active, "new 'No to cancel' format should be detected");
        assert_eq!(info.question.as_deref(), Some("Do you want to proceed?"));
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 3);
    }

    #[test]
    fn prompt_all_entry_serializes_flat() {
        let entry = PromptAllEntry {
            session_id: "abc-123".to_string(),
            file: "tasks/plan.md".to_string(),
            prompt: PromptInfo {
                active: true,
                question: Some("Allow?".to_string()),
                options: Some(vec![
                    PromptOption {
                        index: 1,
                        label: "Yes".to_string(),
                    },
                    PromptOption {
                        index: 2,
                        label: "No".to_string(),
                    },
                ]),
                selected: Some(0),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"session_id\":\"abc-123\""));
        assert!(json.contains("\"file\":\"tasks/plan.md\""));
        assert!(json.contains("\"active\":true"));
        assert!(json.contains("\"question\":\"Allow?\""));
    }

    #[test]
    fn prompt_all_entry_inactive_omits_optional_fields() {
        let entry = PromptAllEntry {
            session_id: "def-456".to_string(),
            file: "tasks/resume.md".to_string(),
            prompt: inactive(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"active\":false"));
        assert!(!json.contains("\"question\""));
        assert!(!json.contains("\"options\""));
    }
}
