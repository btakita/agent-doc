//! Alacritty-backed screen state for supervisor-owned PTY output.
//!
//! The supervisor already owns the child PTY through `portable-pty`; this
//! module gives that owned stream a terminal emulator model so prompt/readiness
//! detection can inspect the current child screen without asking tmux to
//! `capture-pane`.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;
use portable_pty::PtySize;

const DEFAULT_ROWS: usize = 24;
const DEFAULT_COLS: usize = 80;
const SCROLLBACK_LINES: usize = 2000;

#[derive(Debug, Clone, Copy)]
struct ScreenSize {
    rows: usize,
    cols: usize,
}

impl ScreenSize {
    fn from_pty(size: PtySize) -> Self {
        Self {
            rows: usize::from(size.rows).max(1),
            cols: usize::from(size.cols).max(1),
        }
    }
}

impl Default for ScreenSize {
    fn default() -> Self {
        Self {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
        }
    }
}

impl Dimensions for ScreenSize {
    fn total_lines(&self) -> usize {
        self.rows + SCROLLBACK_LINES
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Terminal-emulator state for the supervised child.
pub struct OwnedPtyScreen {
    term: Term<VoidListener>,
    parser: ansi::Processor,
    size: ScreenSize,
}

impl OwnedPtyScreen {
    pub fn new(size: PtySize) -> Self {
        let size = ScreenSize::from_pty(size);
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Default::default()
        };
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: ansi::Processor::new(),
            size,
        }
    }

    pub fn default_size() -> Self {
        let size = ScreenSize::default();
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Default::default()
        };
        Self {
            term: Term::new(config, &size, VoidListener),
            parser: ansi::Processor::new(),
            size,
        }
    }

    pub fn reset(&mut self, size: PtySize) {
        *self = Self::new(size);
    }

    pub fn resize(&mut self, size: PtySize) {
        let size = ScreenSize::from_pty(size);
        self.term.resize(size);
        self.size = size;
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Render the current visible terminal viewport as plain text.
    pub fn visible_text(&self) -> String {
        let mut lines = vec![String::new(); self.size.rows];
        for indexed in self.term.renderable_content().display_iter {
            if indexed.point.line.0 < 0 {
                continue;
            }
            let line_index = indexed.point.line.0 as usize;
            if line_index >= lines.len() {
                continue;
            }
            let cell = indexed.cell;
            let flags = cell.flags;
            if flags.intersects(
                Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
            ) {
                lines[line_index].push(' ');
            } else {
                lines[line_index].push(cell.c);
            }
        }

        lines
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for OwnedPtyScreen {
    fn default() -> Self {
        Self::default_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(rows: u16, cols: u16) -> PtySize {
        PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    #[test]
    fn screen_tracks_cursor_rewrites_without_tmux_capture() {
        let mut screen = OwnedPtyScreen::new(size(3, 12));
        screen.push(b"busy");
        screen.push(b"\rready");

        let text = screen.visible_text();
        assert!(
            text.lines().any(|line| line.trim() == "ready"),
            "screen should expose rewritten visible line, got {text:?}"
        );
        assert!(
            !text.lines().any(|line| line.trim() == "busydy"),
            "terminal state must not behave like an append-only byte log: {text:?}"
        );
    }

    #[test]
    fn screen_tracks_visible_prompt_after_clear_line() {
        let mut screen = OwnedPtyScreen::new(size(3, 16));
        screen.push(b"working");
        screen.push(b"\r\x1b[2K/tmp/project \xe2\x9d\xaf");

        assert!(
            screen.visible_text().contains("/tmp/project ❯"),
            "screen should include prompt after EL rewrite"
        );
    }

    #[test]
    fn screen_resize_keeps_parser_state_live() {
        let mut screen = OwnedPtyScreen::new(size(2, 8));
        screen.push(b"one\n");
        screen.resize(size(4, 20));
        screen.push(b"two");

        let text = screen.visible_text();
        assert!(
            text.contains("one"),
            "resized screen lost existing text: {text:?}"
        );
        assert!(
            text.contains("two"),
            "resized screen did not accept new text: {text:?}"
        );
    }
}
