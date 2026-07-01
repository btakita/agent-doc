//! Pure parsing for harness permission prompts rendered in tmux panes.
//!
//! This module owns deterministic prompt/chrome parsing over captured pane
//! text. It does not read session documents, inspect registries, call tmux, or
//! send keys.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptNavigationAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptNavigationKeys {
    pub prev: &'static str,
    pub next: &'static str,
}

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

/// Parse tmux pane content for Claude Code and OpenCode permission prompts.
pub fn parse_prompt(content: &str) -> PromptInfo {
    let claude = parse_claude_prompt(content);
    if claude.active {
        return claude;
    }
    parse_opencode_prompt(content)
}

fn parse_claude_prompt(content: &str) -> PromptInfo {
    let lines: Vec<&str> = content.lines().collect();

    // Strip ANSI escape codes for pattern matching.
    let stripped: Vec<String> = lines.iter().map(|l| strip_ansi(l)).collect();

    // Search from the bottom up so stale prompt-like scrollback does not win.
    let footer_idx = stripped.iter().rposition(|line| line.contains("to cancel"));

    let footer_idx = match footer_idx {
        Some(idx) => idx,
        None => return inactive_prompt(),
    };

    let mut options = Vec::new();
    let mut selected: Option<usize> = None;
    let mut question_line_idx: Option<usize> = None;

    for i in (0..footer_idx).rev() {
        let line = &stripped[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if let Some(opt) = parse_option_line(trimmed) {
            let is_selected = trimmed.starts_with('❯') || trimmed.starts_with('>');
            if is_selected {
                selected = Some(opt.index - 1);
            }
            options.push(opt);
        } else if !options.is_empty() {
            question_line_idx = Some(i);
            break;
        }
    }

    if options.is_empty() {
        return inactive_prompt();
    }

    options.reverse();

    let question = question_line_idx.map(|idx| stripped[idx].trim().to_string());

    PromptInfo {
        active: true,
        question,
        options: Some(options),
        selected,
    }
}

fn parse_opencode_prompt(content: &str) -> PromptInfo {
    let raw_lines: Vec<&str> = content.lines().collect();
    let lines: Vec<String> = raw_lines.iter().map(|line| strip_ansi(line)).collect();
    let footer_idx = lines.iter().rposition(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("enter confirm") && lower.contains("select")
    });
    let Some(footer_idx) = footer_idx else {
        return inactive_prompt();
    };

    let option_row = strip_box_prefix(&lines[footer_idx]);
    let option_prefix = opencode_option_prefix(option_row);
    let options = parse_opencode_option_row(option_prefix);
    if options.is_empty() {
        return inactive_prompt();
    }

    let question = opencode_question(&lines[..footer_idx]);
    let selected = opencode_selected_option_from_ansi(raw_lines[footer_idx], &options).or(Some(0));
    PromptInfo {
        active: true,
        question,
        options: Some(options),
        selected,
    }
}

pub fn navigation_axis_for_prompt(content: &str) -> PromptNavigationAxis {
    if parse_opencode_prompt(content).active {
        PromptNavigationAxis::Horizontal
    } else {
        PromptNavigationAxis::Vertical
    }
}

pub fn navigation_keys_for_prompt(content: &str) -> PromptNavigationKeys {
    match navigation_axis_for_prompt(content) {
        PromptNavigationAxis::Vertical => PromptNavigationKeys {
            prev: "Up",
            next: "Down",
        },
        PromptNavigationAxis::Horizontal => PromptNavigationKeys {
            prev: "BTab",
            next: "Tab",
        },
    }
}

pub fn opencode_option_requires_confirmation(option: &PromptOption) -> bool {
    option.label == "Allow always"
}

pub fn opencode_permission_prompt_active(content: &str, raw_output: &[u8]) -> bool {
    let prompt = parse_prompt(content);
    if prompt.active
        && prompt.options.as_ref().is_some_and(|options| {
            options.iter().any(|option| option.label == "Allow once")
                && options.iter().any(|option| option.label == "Allow always")
                && options.iter().any(|option| option.label == "Reject")
        })
    {
        return true;
    }

    // Fallback: detect via the orange selection highlight in raw output bytes.
    // OpenCode uses ANSI 48;2;245;167;66 (amber) to mark the selected permission
    // option. This fires even when the footer text changes across OpenCode versions.
    bytes_contains(raw_output, b"\x1b[48;2;245;167;66m")
        && (bytes_contains(raw_output, b"Allow once")
            || bytes_contains(raw_output, b"Allow always")
            || bytes_contains(raw_output, b"Reject"))
}

pub fn normalize_opencode_permission_stdin(
    content: &str,
    raw_output: &[u8],
    data: &[u8],
) -> Option<Vec<u8>> {
    if !opencode_permission_prompt_active(content, raw_output) {
        return None;
    }
    translate_opencode_permission_arrow_keys(data)
}

pub fn translate_opencode_permission_arrow_keys(data: &[u8]) -> Option<Vec<u8>> {
    let mut translated = Vec::with_capacity(data.len());
    let mut changed = false;
    let mut i = 0;
    while i < data.len() {
        let replacement = if data[i..].starts_with(b"\x1b[C")
            || data[i..].starts_with(b"\x1b[B")
            || data[i..].starts_with(b"\x1bOC")
            || data[i..].starts_with(b"\x1bOB")
        {
            Some((&b"\t"[..], 3))
        } else if data[i..].starts_with(b"\x1b[D")
            || data[i..].starts_with(b"\x1b[A")
            || data[i..].starts_with(b"\x1bOD")
            || data[i..].starts_with(b"\x1bOA")
        {
            Some((&b"\x1b[Z"[..], 3))
        } else {
            None
        };

        if let Some((bytes, consumed)) = replacement {
            translated.extend_from_slice(bytes);
            i += consumed;
            changed = true;
        } else {
            translated.push(data[i]);
            i += 1;
        }
    }

    changed.then_some(translated)
}

fn bytes_contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn strip_box_prefix(line: &str) -> &str {
    line.trim()
        .trim_start_matches('┃')
        .trim_start_matches('│')
        .trim_end_matches('┃')
        .trim_end_matches('│')
        .trim()
}

fn opencode_option_prefix(line: &str) -> &str {
    let lower = line.to_ascii_lowercase();
    for marker in ["ctrl+f fullscreen", "⇆ select", "enter confirm"] {
        if let Some(idx) = lower.find(marker) {
            return line[..idx].trim_end();
        }
    }
    line.trim_end()
}

fn parse_opencode_option_row(line: &str) -> Vec<PromptOption> {
    let mut options = Vec::new();
    let labels = ["Allow once", "Allow always", "Reject"];
    for label in labels {
        if line.contains(label) {
            options.push(PromptOption {
                index: options.len() + 1,
                label: label.to_string(),
            });
        }
    }
    options
}

fn opencode_selected_option_from_ansi(row: &str, options: &[PromptOption]) -> Option<usize> {
    for (index, option) in options.iter().enumerate() {
        let Some(label_start) = row.find(&option.label) else {
            continue;
        };
        let prefix = &row[..label_start];
        let Some(bg_start) = prefix.rfind("\x1b[48;2;") else {
            continue;
        };
        let bg = &prefix[bg_start..];
        let Some(bg_end) = bg.find('m') else {
            continue;
        };
        let bg = &bg[..=bg_end];
        if bg.contains("245;167;66") {
            return Some(index);
        }
    }
    None
}

fn opencode_question(lines: &[String]) -> Option<String> {
    if let Some(line) = lines.iter().rev().find(|line| line.contains('←')) {
        let cleaned = strip_box_prefix(line).trim_start_matches('←').trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }

    Some("Permission required".to_string())
}

/// Parse a single option line like "[1] Yes", "1. Yes", or "❯ [2] Yes".
fn parse_option_line(line: &str) -> Option<PromptOption> {
    let stripped = line.trim_start_matches('❯').trim_start_matches('>').trim();

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
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(next) = chars.next()
                && next == '['
            {
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub fn inactive_prompt() -> PromptInfo {
    PromptInfo {
        active: false,
        question: None,
        options: None,
        selected: None,
    }
}

pub fn is_codex_idle_placeholder_prompt(trimmed: &str) -> bool {
    codex_idle_placeholder_prompt(trimmed).is_some()
}

pub fn codex_idle_placeholder_prompt(trimmed: &str) -> Option<String> {
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

pub fn codex_idle_placeholder_candidate(output: &str) -> Option<String> {
    let recent = output
        .lines()
        .rev()
        .take(8)
        .map(strip_ansi)
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

pub fn codex_prompt_candidate_is_dim_placeholder(output: &str, candidate: &str) -> bool {
    let Some(raw_line) = output.lines().rev().find(|line| {
        let stripped = strip_ansi(line);
        stripped.trim() == candidate
    }) else {
        return false;
    };
    codex_prompt_line_body_starts_dim(raw_line)
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
    fn manifest_does_not_depend_on_orchestration() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            !manifest.contains("agent-doc-orchestration"),
            "agent-doc-turn-executor-tmux must stay below orchestration"
        );
    }

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
        assert_eq!(info.selected, Some(1));
    }

    #[test]
    fn parse_no_prompt() {
        let info = parse_prompt("Hello world\nSome regular output\n");
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
    fn parse_opencode_permission_prompt() {
        let content = r#"
   ⠙[[[Dd ~/work/btakita/corky/pyproject.toml
┃                                                                                                                       ┃  △ Permission required
┃    ← Access external directory ~/work/btakita/corky/.github/workflows                                                 ┃
┃  Patterns                                                                                                             ┃
┃  - /home/brian/work/btakita/corky/.github/workflows/*                                                                 ┃
┃                                                                                                                       ┃   Allow once   Allow always   Reject                                 ctrl+f fullscreen  ⇆ select  enter confirm
┃
"#;
        let info = parse_prompt(content);
        assert!(
            info.active,
            "OpenCode horizontal permission prompt should be detected"
        );
        assert_eq!(
            info.question.as_deref(),
            Some("Access external directory ~/work/btakita/corky/.github/workflows")
        );
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].index, 1);
        assert_eq!(opts[0].label, "Allow once");
        assert_eq!(opts[1].index, 2);
        assert_eq!(opts[1].label, "Allow always");
        assert_eq!(opts[2].index, 3);
        assert_eq!(opts[2].label, "Reject");
        assert_eq!(info.selected, Some(0));
        assert_eq!(
            navigation_axis_for_prompt(content),
            PromptNavigationAxis::Horizontal
        );
        assert_eq!(
            navigation_keys_for_prompt(content),
            PromptNavigationKeys {
                prev: "BTab",
                next: "Tab"
            }
        );
        assert!(!opencode_option_requires_confirmation(&opts[0]));
        assert!(opencode_option_requires_confirmation(&opts[1]));
        assert!(!opencode_option_requires_confirmation(&opts[2]));
    }

    #[test]
    fn parse_opencode_permission_prompt_without_question_uses_default_label() {
        let content = "\
bash mock-opencode-prompt.sh
printf '\\033[48;2;245;167;66mAllow once\\033[0m Allow always Reject ⇆ select enter confirm'
\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject ctrl+f fullscreen ⇆ select enter confirm
";

        let info = parse_prompt(content);
        assert!(info.active);
        assert_eq!(info.question.as_deref(), Some("Permission required"));
        let opts = info.options.as_ref().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].label, "Allow once");
        assert_eq!(opts[1].label, "Allow always");
        assert_eq!(opts[2].label, "Reject");
    }

    #[test]
    fn parse_opencode_selected_option_from_ansi() {
        let content = "\
\x1b[48;2;10;10;10m  \x1b[38;2;245;167;66m\x1b[48;2;20;20;20m┃\x1b[38;2;255;255;255m\x1b[48;2;30;30;30m   \x1b[38;2;128;128;128mAllow once\x1b[38;2;255;255;255m   \x1b[48;2;245;167;66m \x1b[38;2;10;10;10mAllow always\x1b[38;2;255;255;255m  \x1b[48;2;30;30;30mReject\x1b[38;2;238;238;238m ⇆ \x1b[38;2;128;128;128mselect\x1b[38;2;238;238;238m enter \x1b[38;2;128;128;128mconfirm\n";

        let info = parse_prompt(content);
        assert!(info.active);
        assert_eq!(info.selected, Some(1));
    }

    #[test]
    fn opencode_permission_prompt_translates_legacy_arrows_to_tab_controls() {
        let content = "\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject ctrl+f fullscreen ⇆ select enter confirm\n";

        let translated = normalize_opencode_permission_stdin(
            content,
            content.as_bytes(),
            b"\x1b[C\x1b[C\x1b[D\x1b[Atext",
        )
        .expect("OpenCode permission prompt should translate legacy arrow escapes");

        assert_eq!(translated, b"\t\t\x1b[Z\x1b[Ztext");
    }

    #[test]
    fn opencode_permission_prompt_translation_is_gated_to_permission_dialog() {
        assert!(
            normalize_opencode_permission_stdin(
                "Ask anything...\n",
                b"Ask anything...\n",
                b"\x1b[C"
            )
            .is_none(),
            "normal OpenCode prompt editing must keep arrow keys unchanged"
        );
    }

    #[test]
    fn opencode_permission_prompt_fallback_detects_orange_highlight_without_footer() {
        // Simulate a newer OpenCode version where the footer text changed but the
        // orange selection highlight (48;2;245;167;66) is still present.
        let raw = b"\x1b[48;2;245;167;66mAllow once\x1b[0m Allow always Reject\n";
        let translated = normalize_opencode_permission_stdin("", raw, b"\x1b[C").expect(
            "fallback detection must translate arrows even without the standard footer text",
        );
        assert_eq!(translated, b"\t");
    }

    #[test]
    fn opencode_permission_prompt_fallback_requires_allow_or_reject_label() {
        // Orange highlight alone (no permission labels) must not trigger translation.
        let raw = b"\x1b[48;2;245;167;66msome other highlighted text\x1b[0m\n";
        assert!(
            normalize_opencode_permission_stdin("", raw, b"\x1b[C").is_none(),
            "orange highlight without permission labels must not trigger arrow translation"
        );
    }

    #[test]
    fn codex_idle_placeholder_candidate_recovers_wrapped_placeholder() {
        let content = "\
gpt-5.4 medium · ~/work/btakita/agent-loop · Context 0% used
› Explain this module
in @filename
";

        assert_eq!(
            codex_idle_placeholder_candidate(content).as_deref(),
            Some("› Explain this module in @filename")
        );
        assert!(is_codex_idle_placeholder_prompt(
            "› Explain this module in @filename"
        ));
    }

    #[test]
    fn codex_idle_placeholder_rejects_drafted_prose() {
        assert_eq!(
            codex_idle_placeholder_prompt("› investigate this issue"),
            None
        );
        assert_eq!(
            codex_idle_placeholder_prompt("› Investigate this issue quickly."),
            None
        );
        assert_eq!(
            codex_idle_placeholder_candidate("› investigate this issue"),
            None
        );
    }

    #[test]
    fn codex_dim_placeholder_detection_ignores_rgb_color() {
        let dim = "\x1b[1m›\x1b[0m \x1b[2mAsk Codex to do anything\x1b[0m\n";
        assert!(codex_prompt_candidate_is_dim_placeholder(
            dim,
            "› Ask Codex to do anything"
        ));

        let rgb = "\x1b[1m›\x1b[0m \x1b[38;2;128;128;128mAsk Codex to do anything\x1b[0m\n";
        assert!(!codex_prompt_candidate_is_dim_placeholder(
            rgb,
            "› Ask Codex to do anything"
        ));
    }

    #[test]
    fn opencode_prompt_without_options_is_inactive() {
        let content = "ctrl+f fullscreen  ⇆ select  enter confirm\n";
        let info = parse_prompt(content);
        assert!(!info.active);
    }
}
