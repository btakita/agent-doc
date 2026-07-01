//! Stateful terminal escape filtering for supervisor PTY output.
//!
//! This module is pure policy: it decides which terminal capability queries,
//! responses, and terminal-owned strings should be suppressed before PTY output
//! is rendered or sampled. Callers own side effects such as logging.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TerminalFilterConfig {
    pub preserve_kitty_keyboard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFilterAction {
    Drop,
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFilterTraceKind {
    KittyKeyboardPush,
    KittyProgressiveEnhancement,
    KittyKeyboardPop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalFilterTrace {
    pub kind: TerminalFilterTraceKind,
    pub action: TerminalFilterAction,
    pub sequence_len: usize,
    pub preserve_kitty_keyboard: bool,
}

/// Stateful filter for terminal capability queries and responses from PTY output.
///
/// When a PTY child sends terminal queries (DSR, DA, XTVERSION), forwarding PTY
/// output to an outer terminal can cause the outer terminal to answer with escape
/// sequences that echo as visible garbage. This filter suppresses both outgoing
/// query sequences and defense-in-depth response sequences while preserving
/// ordinary terminal output such as colors and cursor movement.
///
/// OpenCode can opt into preserving Kitty keyboard mode sequences because
/// OpenTUI depends on those mode transitions for real arrow/tab key handling.
pub struct TerminalFilter {
    carryover: Vec<u8>,
    config: TerminalFilterConfig,
}

impl Default for TerminalFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalFilter {
    pub fn new() -> Self {
        Self::with_config(TerminalFilterConfig::default())
    }

    pub fn with_config(config: TerminalFilterConfig) -> Self {
        Self {
            carryover: Vec::new(),
            config,
        }
    }

    pub fn filter(&mut self, input: &[u8], output: &mut Vec<u8>) -> Vec<TerminalFilterTrace> {
        let mut trace = Vec::new();
        self.filter_with_trace(input, output, &mut trace);
        trace
    }

    pub fn filter_with_trace(
        &mut self,
        input: &[u8],
        output: &mut Vec<u8>,
        trace: &mut Vec<TerminalFilterTrace>,
    ) {
        let combined;
        let data: &[u8] = if self.carryover.is_empty() {
            input
        } else {
            combined = [self.carryover.as_slice(), input].concat();
            self.carryover.clear();
            &combined
        };

        let len = data.len();
        let mut i = 0;
        while i < len {
            if data[i] == 0x1b {
                if i + 1 >= len {
                    self.carryover.extend_from_slice(&data[i..]);
                    return;
                }
                if data[i + 1] == b'[' {
                    let start = i;
                    i += 2;
                    if i >= len {
                        self.carryover.extend_from_slice(&data[start..]);
                        return;
                    }

                    let has_question = data[i] == b'?';
                    let has_gt = data[i] == b'>';
                    let has_lt = data[i] == b'<';
                    let no_prefix = !has_question && !has_gt && !has_lt;
                    if has_question || has_gt || has_lt {
                        i += 1;
                    }

                    while i < len && (data[i].is_ascii_digit() || data[i] == b';') {
                        i += 1;
                    }
                    if i >= len {
                        self.carryover.extend_from_slice(&data[start..]);
                        return;
                    }

                    if data[i].is_ascii_alphabetic() {
                        let final_byte = data[i];
                        i += 1;
                        let should_filter = match final_byte {
                            b'c' if no_prefix => true,
                            b'n' | b'c' | b'h' | b'l' if has_question => true,
                            b'c' | b'q' if has_gt => true,
                            b'u' | b'm' if has_gt => !self.config.preserve_kitty_keyboard,
                            b'u' if has_lt => !self.config.preserve_kitty_keyboard,
                            _ => false,
                        };

                        if let Some(kind) = kitty_trace_kind(has_gt, has_lt, final_byte) {
                            trace.push(TerminalFilterTrace {
                                kind,
                                action: if should_filter {
                                    TerminalFilterAction::Drop
                                } else {
                                    TerminalFilterAction::Preserve
                                },
                                sequence_len: i - start,
                                preserve_kitty_keyboard: self.config.preserve_kitty_keyboard,
                            });
                        }

                        if should_filter {
                            continue;
                        }
                        output.extend_from_slice(&data[start..i]);
                        continue;
                    }

                    output.extend_from_slice(&data[start..i]);
                    continue;
                }
                if data[i + 1] == b'P' {
                    let start = i;
                    i += 2;
                    loop {
                        if i >= len {
                            self.carryover.extend_from_slice(&data[start..]);
                            return;
                        }
                        if data[i] == 0x1b {
                            if i + 1 >= len {
                                self.carryover.extend_from_slice(&data[start..]);
                                return;
                            }
                            if data[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                        }
                        i += 1;
                    }
                    continue;
                }
                if data[i + 1] == b']' {
                    let start = i;
                    i += 2;
                    loop {
                        if i >= len {
                            self.carryover.extend_from_slice(&data[start..]);
                            return;
                        }
                        if data[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if data[i] == 0x1b {
                            if i + 1 >= len {
                                self.carryover.extend_from_slice(&data[start..]);
                                return;
                            }
                            if data[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                        }
                        i += 1;
                    }
                    continue;
                }
            }
            output.push(data[i]);
            i += 1;
        }
    }
}

fn kitty_trace_kind(has_gt: bool, has_lt: bool, final_byte: u8) -> Option<TerminalFilterTraceKind> {
    if has_gt && final_byte == b'u' {
        Some(TerminalFilterTraceKind::KittyKeyboardPush)
    } else if has_gt && final_byte == b'm' {
        Some(TerminalFilterTraceKind::KittyProgressiveEnhancement)
    } else if has_lt && final_byte == b'u' {
        Some(TerminalFilterTraceKind::KittyKeyboardPop)
    } else {
        None
    }
}

#[cfg(test)]
fn filter_terminal_queries(input: &[u8], output: &mut Vec<u8>) {
    TerminalFilter::new().filter(input, output);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_strips_da1_query() {
        let input = b"\x1b[c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 query should be stripped");
    }

    #[test]
    fn filter_strips_da1_query_with_param() {
        let input = b"\x1b[0c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 query with param should be stripped");
    }

    #[test]
    fn filter_strips_xtversion_query() {
        let input = b"\x1b[>q";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "XTVERSION query should be stripped");
    }

    #[test]
    fn filter_strips_dsr_response() {
        let input = b"\x1b[?997;1n";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DSR response should be stripped");
    }

    #[test]
    fn filter_strips_da1_response() {
        let input = b"\x1b[?1;2;4c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA1 response should be stripped");
    }

    #[test]
    fn filter_strips_da2_response() {
        let input = b"\x1b[>0;115;0c";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DA2 response should be stripped");
    }

    #[test]
    fn filter_strips_dcs_string() {
        let input = b"\x1bP>|tmux 3.6a\x1b\\";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DCS string should be stripped");
    }

    #[test]
    fn filter_strips_interleaved_sequences() {
        let mut input = Vec::new();
        input.extend_from_slice(b"\x1b[?997;1n");
        input.extend_from_slice(b"\x1bP>|tmux 3.6a\x1b\\");
        input.extend_from_slice(b"\x1b[?1;2;4c");
        input.extend_from_slice(" Claude Code v2.1.109".as_bytes());
        let mut out = Vec::new();
        filter_terminal_queries(&input, &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            " Claude Code v2.1.109",
            "only the banner text should remain"
        );
    }

    #[test]
    fn filter_strips_dec_private_mode_set() {
        let input = b"\x1b[?2026h";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DEC private mode set should be stripped");
    }

    #[test]
    fn filter_strips_dec_private_mode_reset() {
        let input = b"\x1b[?2026l";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert!(out.is_empty(), "DEC private mode reset should be stripped");
    }

    #[test]
    fn filter_strips_kitty_keyboard_push() {
        let input = b"\x1b[>1u";
        let mut out = Vec::new();
        let trace = TerminalFilter::new().filter(input, &mut out);
        assert!(out.is_empty(), "Kitty keyboard push should be stripped");
        assert_eq!(
            trace,
            vec![TerminalFilterTrace {
                kind: TerminalFilterTraceKind::KittyKeyboardPush,
                action: TerminalFilterAction::Drop,
                sequence_len: input.len(),
                preserve_kitty_keyboard: false,
            }]
        );
    }

    #[test]
    fn filter_strips_kitty_keyboard_pop() {
        let input = b"\x1b[<u";
        let mut out = Vec::new();
        let trace = TerminalFilter::new().filter(input, &mut out);
        assert!(out.is_empty(), "Kitty keyboard pop should be stripped");
        assert_eq!(
            trace,
            vec![TerminalFilterTrace {
                kind: TerminalFilterTraceKind::KittyKeyboardPop,
                action: TerminalFilterAction::Drop,
                sequence_len: input.len(),
                preserve_kitty_keyboard: false,
            }]
        );
    }

    #[test]
    fn filter_strips_kitty_progressive_enhancement() {
        let input = b"\x1b[>4;2m";
        let mut out = Vec::new();
        let trace = TerminalFilter::new().filter(input, &mut out);
        assert!(
            out.is_empty(),
            "Kitty progressive enhancement should be stripped"
        );
        assert_eq!(
            trace,
            vec![TerminalFilterTrace {
                kind: TerminalFilterTraceKind::KittyProgressiveEnhancement,
                action: TerminalFilterAction::Drop,
                sequence_len: input.len(),
                preserve_kitty_keyboard: false,
            }]
        );
    }

    #[test]
    fn filter_preserves_kitty_keyboard_for_opencode() {
        let mut filter = TerminalFilter::with_config(TerminalFilterConfig {
            preserve_kitty_keyboard: true,
        });
        let input = b"\x1b[>1u\x1b[>4;2mkeys\x1b[<u";
        let mut out = Vec::new();
        let trace = filter.filter(input, &mut out);
        assert_eq!(
            out, input,
            "OpenCode relies on Kitty keyboard mode for permission prompt keys"
        );
        assert_eq!(
            trace,
            vec![
                TerminalFilterTrace {
                    kind: TerminalFilterTraceKind::KittyKeyboardPush,
                    action: TerminalFilterAction::Preserve,
                    sequence_len: b"\x1b[>1u".len(),
                    preserve_kitty_keyboard: true,
                },
                TerminalFilterTrace {
                    kind: TerminalFilterTraceKind::KittyProgressiveEnhancement,
                    action: TerminalFilterAction::Preserve,
                    sequence_len: b"\x1b[>4;2m".len(),
                    preserve_kitty_keyboard: true,
                },
                TerminalFilterTrace {
                    kind: TerminalFilterTraceKind::KittyKeyboardPop,
                    action: TerminalFilterAction::Preserve,
                    sequence_len: b"\x1b[<u".len(),
                    preserve_kitty_keyboard: true,
                },
            ]
        );
    }

    #[test]
    fn opencode_filter_still_strips_terminal_queries() {
        let mut filter = TerminalFilter::with_config(TerminalFilterConfig {
            preserve_kitty_keyboard: true,
        });
        let input = b"\x1b[?997;1n\x1b[>q\x1bP>|tmux 3.6a\x1b\\\x1b[>1u";
        let mut out = Vec::new();
        filter.filter(input, &mut out);
        assert_eq!(
            out,
            b"\x1b[>1u".to_vec(),
            "OpenCode preserves keyboard mode but not terminal query noise"
        );
    }

    #[test]
    fn filter_preserves_normal_csi() {
        let input = b"\x1b[32mhello\x1b[0m";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(out, input.to_vec(), "SGR sequences should be preserved");
    }

    #[test]
    fn filter_preserves_cursor_movement() {
        let input = b"\x1b[4A";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(out, input.to_vec(), "cursor movement should be preserved");
    }

    #[test]
    fn filter_preserves_plain_text() {
        let input = b"hello world\n";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(
            out,
            input.to_vec(),
            "plain text should pass through unchanged"
        );
    }

    #[test]
    fn filter_stateful_esc_split_across_reads() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"hello\x1b", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "hello",
            "text before ESC emitted, ESC buffered"
        );

        out.clear();
        f.filter(b"P>|tmux 3.6a\x1b\\\x1b[?1;2;4c world", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            " world",
            "DCS and DA1 stripped, trailing text preserved"
        );
    }

    #[test]
    fn filter_stateful_dcs_split_at_st() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"\x1bP>|tmux 3.6a\x1b", &mut out);
        assert!(out.is_empty(), "incomplete DCS buffered, nothing emitted");

        out.clear();
        f.filter(b"\\done", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "done",
            "DCS consumed after ST completed across boundary"
        );
    }

    #[test]
    fn filter_strips_osc_title_updates() {
        let input = b"before\x1b]0;Working (3s - esc to interrupt)\x07after";
        let mut out = Vec::new();
        filter_terminal_queries(input, &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "beforeafter",
            "OSC title text should not enter prompt sampling"
        );
    }

    #[test]
    fn filter_stateful_osc_split_across_reads() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"before\x1b]0;Working", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "before",
            "text before OSC emitted, incomplete OSC buffered"
        );

        out.clear();
        f.filter(b" (3s - esc to interrupt)\x1b\\after", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "after",
            "OSC title update stripped after ST completed across boundary"
        );
    }

    #[test]
    fn filter_stateful_csi_split_at_params() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"\x1b[?997", &mut out);
        assert!(out.is_empty(), "incomplete CSI buffered");

        out.clear();
        f.filter(b";1nok", &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ok",
            "DSR stripped after completion across boundary"
        );
    }

    #[test]
    fn filter_stateful_csi_split_at_bracket() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"a\x1b", &mut out);
        assert_eq!(String::from_utf8_lossy(&out), "a");

        out.clear();
        f.filter(b"[?2026h", &mut out);
        assert!(out.is_empty(), "DEC mode set stripped across boundary");
    }

    #[test]
    fn filter_stateful_real_world_banner() {
        let mut f = TerminalFilter::new();
        let full = b"\x1b[?997;1n\x1bP>|tmux 3.6a\x1b\\\x1b[?1;2;4c Claude Code v2.1.109";

        let mut out = Vec::new();
        f.filter(&full[..5], &mut out);
        f.filter(&full[5..12], &mut out);
        f.filter(&full[12..25], &mut out);
        f.filter(&full[25..], &mut out);

        assert_eq!(
            String::from_utf8_lossy(&out),
            " Claude Code v2.1.109",
            "all escape sequences stripped despite arbitrary split points"
        );
    }

    #[test]
    fn filter_stateful_normal_esc_preserved_across_boundary() {
        let mut f = TerminalFilter::new();
        let mut out = Vec::new();

        f.filter(b"hi\x1b", &mut out);
        assert_eq!(String::from_utf8_lossy(&out), "hi");

        out.clear();
        f.filter(b"[32mgreen\x1b[0m", &mut out);
        assert_eq!(
            out, b"\x1b[32mgreen\x1b[0m",
            "SGR preserved across boundary"
        );
    }
}
