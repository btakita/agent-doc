//! `agent-doc prompt` — Detect and surface permission prompts from Claude Code.
//!
//! Usage: agent-doc prompt <file.md>
//!
//! 1. Reads session UUID from file's frontmatter
//! 2. Looks up pane in sessions.json
//! 3. Captures pane content via `tmux capture-pane`
//! 4. Parses for Claude Code permission/question patterns
//! 5. Outputs JSON to stdout
//!
//! Usage: agent-doc prompt --answer <file.md> <option-number>
//!
//! Sends the selected option to the tmux pane.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;

use crate::sessions::Tmux;
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

    let pane_content = tmux.capture_pane(&pane_id)?;
    let info = parse_prompt(&pane_content);
    println!("{}", serde_json::to_string(&info)?);
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
    let pane_content = tmux.capture_pane(&pane_id)?;
    let info = parse_prompt(&pane_content);
    if !info.active {
        anyhow::bail!("no active prompt detected");
    }

    let options = info.options.as_ref().unwrap();
    if option_index == 0 || option_index > options.len() {
        anyhow::bail!(
            "option {} out of range (1-{})",
            option_index,
            options.len()
        );
    }

    // Navigate to the selected option and press Enter.
    // Claude Code's TUI uses arrow keys for navigation.
    // We need to move from the currently selected item to the target.
    let current = info.selected.unwrap_or(0);
    let target = option_index - 1; // convert to 0-based

    if target < current {
        // Move up
        for _ in 0..(current - target) {
            tmux.send_key(&pane_id, "Up")?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    } else if target > current {
        // Move down
        for _ in 0..(target - current) {
            tmux.send_key(&pane_id, "Down")?;
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }

    // Brief pause then press Enter to confirm
    std::thread::sleep(std::time::Duration::from_millis(50));
    tmux.send_key(&pane_id, "Enter")?;

    eprintln!(
        "Sent option {} to pane {}",
        option_index, pane_id
    );
    Ok(())
}

/// Parse tmux pane content for Claude Code permission prompts.
///
/// Looks for patterns like:
/// ```text
///  Do you want to proceed?
///    1. Yes
///  ❯ 2. Yes, and don't ask again for: ...
///    3. No
///
///  Esc to cancel · ctrl+e to explain
/// ```
pub fn parse_prompt(content: &str) -> PromptInfo {
    let lines: Vec<&str> = content.lines().collect();

    // Strip ANSI escape codes for pattern matching
    let stripped: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();

    // Search for the prompt pattern from the bottom up (most recent prompt)
    // Look for the footer pattern first
    let footer_idx = stripped.iter().rposition(|line| {
        line.contains("Esc to cancel")
    });

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

        // Check for numbered option pattern: "N. label" with optional ❯ prefix
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

/// Parse a single option line like "1. Yes" or "❯ 2. Yes, and don't ask..."
fn parse_option_line(line: &str) -> Option<PromptOption> {
    // Strip leading ❯ or > marker
    let stripped = line
        .trim_start_matches('❯')
        .trim_start_matches('>')
        .trim();

    // Match "N. label" where N is a digit
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
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Consume the escape sequence
            if let Some(next) = chars.next() {
                if next == '[' {
                    // CSI sequence: consume until a letter is found
                    for c2 in chars.by_ref() {
                        if c2.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // Otherwise just skip the next char (two-byte escape)
            }
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
   1. Yes
 ❯ 2. Yes, and don't ask again for: tmux capture-pane:*
   3. No

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
   1. Yes
   2. No

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
        let opt = parse_option_line("1. Yes").unwrap();
        assert_eq!(opt.index, 1);
        assert_eq!(opt.label, "Yes");
    }

    #[test]
    fn parse_option_line_with_cursor() {
        let opt = parse_option_line("❯ 2. Yes, and don't ask again").unwrap();
        assert_eq!(opt.index, 2);
        assert_eq!(opt.label, "Yes, and don't ask again");
    }

    #[test]
    fn parse_option_line_no_match() {
        assert!(parse_option_line("Not an option").is_none());
        assert!(parse_option_line("").is_none());
    }
}
