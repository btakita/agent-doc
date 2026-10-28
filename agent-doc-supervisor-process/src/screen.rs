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

/// Narrowest screen model the supervisor keeps (`#paneoomrunaway`).
///
/// A wide (2-cell) glyph cannot fit a 1-column grid, and `alacritty_terminal`
/// 0.26 does not survive that shape:
/// - `Term::resize` down to 1 column reflows forever. `Grid::shrink_columns`
///   moves the wide char to the next row, finds it still does not fit, and
///   repeats, pushing a fresh row each pass without ever reaching its
///   scrollback truncation. That grew a supervisor by about 1.5 GB/s until
///   `systemd-oomd` killed the pane at 135-142 GB.
/// - `Term::input` of a wide char at 1 column indexes past the row and panics.
///
/// tmux can hand a pane a 1-column width (a 3-column layout that leaves the
/// last pane at `x = window_width - 1`). The model is detection-only, so it
/// stays at least 2 columns wide; the child PTY still gets the real size.
pub const MIN_SCREEN_COLS: usize = 2;
/// Widest / tallest screen model the supervisor keeps (`#paneoomrunaway`): the
/// grid holds `(rows + scrollback) * cols` cells, so an absurd `TIOCGWINSZ`
/// (`u16::MAX`) must not size a multi-gigabyte detection model.
pub const MAX_SCREEN_COLS: usize = 1024;
pub const MAX_SCREEN_ROWS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScreenSize {
    rows: usize,
    cols: usize,
}

impl ScreenSize {
    fn from_pty(size: PtySize) -> Self {
        Self {
            rows: usize::from(size.rows).clamp(1, MAX_SCREEN_ROWS),
            cols: usize::from(size.cols).clamp(MIN_SCREEN_COLS, MAX_SCREEN_COLS),
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
        if size == self.size {
            return;
        }
        self.term.resize(size);
        self.size = size;
    }

    /// Columns of the detection model (after the [`MIN_SCREEN_COLS`] /
    /// [`MAX_SCREEN_COLS`] clamp).
    pub fn columns(&self) -> usize {
        self.size.cols
    }

    /// Visible rows of the detection model (after the [`MAX_SCREEN_ROWS`] clamp).
    pub fn rows(&self) -> usize {
        self.size.rows
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

    /// Render the visible viewport as text that keeps the composer ghost style.
    ///
    /// Every harness renders composer placeholders and autosuggestions faint
    /// (SGR `2`) or italic (SGR `3`), while operator keystrokes render at
    /// normal intensity. That style is the only signal that tells an empty
    /// composer from a typed draft once the placeholder text is not on a
    /// static list (`prompt_candidate_is_dim_placeholder`). [`Self::visible_text`]
    /// drops it, so the supervisor read Codex's post-turn `› Ask a follow-up
    /// question` placeholder as operator input and never saw the idle prompt
    /// (`#clearqueuedliveturn`). Only faint and italic are kept, as the same SGR a
    /// `tmux capture-pane -e` snapshot carries; colour and every other attribute
    /// stay out, so the text is otherwise identical to [`Self::visible_text`].
    pub fn visible_text_with_ghost_style(&self) -> String {
        let mut lines: Vec<Vec<(char, GhostStyle)>> = vec![Vec::new(); self.size.rows];
        for indexed in self.term.renderable_content().display_iter {
            if indexed.point.line.0 < 0 {
                continue;
            }
            let line_index = indexed.point.line.0 as usize;
            if line_index >= lines.len() {
                continue;
            }
            let flags = indexed.cell.flags;
            let ch = if flags.intersects(
                Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
            ) {
                ' '
            } else {
                indexed.cell.c
            };
            lines[line_index].push((
                ch,
                GhostStyle {
                    faint: flags.contains(Flags::DIM),
                    italic: flags.contains(Flags::ITALIC),
                },
            ));
        }

        lines
            .iter()
            .map(|cells| render_ghost_styled_line(cells))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The two cell attributes that mark harness-rendered ghost text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GhostStyle {
    faint: bool,
    italic: bool,
}

/// One screen row, trailing blanks trimmed, with each change of ghost style
/// opened by its SGR and closed by a reset.
fn render_ghost_styled_line(cells: &[(char, GhostStyle)]) -> String {
    let end = cells
        .iter()
        .rposition(|(ch, _)| !ch.is_whitespace())
        .map_or(0, |last| last + 1);
    let mut out = String::new();
    let mut current = GhostStyle::default();
    for &(ch, style) in &cells[..end] {
        if style != current {
            if current != GhostStyle::default() {
                out.push_str("\x1b[0m");
            }
            if style.faint {
                out.push_str("\x1b[2m");
            }
            if style.italic {
                out.push_str("\x1b[3m");
            }
            current = style;
        }
        out.push(ch);
    }
    if current != GhostStyle::default() {
        out.push_str("\x1b[0m");
    }
    out
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

    /// `#clearqueuedliveturn`: Codex renders its post-turn placeholder bold `›`
    /// then a faint body. The ghost-styled projection keeps exactly that SGR
    /// (and nothing for the normal-intensity glyph or a typed draft), so the
    /// composer projection can tell the placeholder from operator input.
    #[test]
    fn ghost_styled_text_keeps_faint_and_italic_runs_only() {
        let mut screen = OwnedPtyScreen::new(size(4, 60));
        screen.push(
            "\x1b[1m\u{203a}\x1b[0m \x1b[2mAsk a follow-up question\x1b[0m\r\n\
             \x1b[1m\u{203a}\x1b[0m \x1b[31mtyped draft\x1b[0m\r\n\
             \x1b[3mitalic\x1b[0m plain"
                .as_bytes(),
        );

        let styled = screen.visible_text_with_ghost_style();
        let lines: Vec<&str> = styled.lines().collect();
        assert_eq!(lines[0], "\u{203a} \x1b[2mAsk a follow-up question\x1b[0m");
        assert_eq!(
            lines[1], "\u{203a} typed draft",
            "colour is not ghost style"
        );
        assert_eq!(lines[2], "\x1b[3mitalic\x1b[0m plain");
        assert_eq!(
            agent_doc_strip_sgr(&styled),
            screen.visible_text(),
            "apart from the ghost SGR the text is the plain projection"
        );
    }

    fn agent_doc_strip_sgr(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && chars.peek() == Some(&'[') {
                for next in chars.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    /// `#paneoomrunaway`: the model never goes below [`MIN_SCREEN_COLS`] nor
    /// above the [`MAX_SCREEN_COLS`] / [`MAX_SCREEN_ROWS`] bound, whatever
    /// `TIOCGWINSZ` reports.
    #[test]
    fn screen_size_is_clamped_to_a_bounded_model() {
        assert_eq!(OwnedPtyScreen::new(size(55, 1)).columns(), MIN_SCREEN_COLS);
        assert_eq!(OwnedPtyScreen::new(size(0, 0)).rows(), 1);
        let huge = OwnedPtyScreen::new(size(u16::MAX, u16::MAX));
        assert_eq!(huge.columns(), MAX_SCREEN_COLS);
        assert_eq!(huge.rows(), MAX_SCREEN_ROWS);

        let mut screen = OwnedPtyScreen::new(size(10, 40));
        screen.resize(size(10, 1));
        assert_eq!(screen.columns(), MIN_SCREEN_COLS);
        screen.resize(size(10, 119));
        assert_eq!(screen.columns(), 119);
    }

    /// `#paneoomrunaway`: a wide glyph printed while the pane is 1 column wide
    /// panicked inside `alacritty_terminal` (`index out of bounds: the len is 1
    /// but the index is 1`), killing the `pty->stdout` reader.
    #[test]
    fn wide_char_on_a_one_column_pane_does_not_panic() {
        let mut screen = OwnedPtyScreen::new(size(5, 1));
        screen.push("ab\u{4f60}\u{597d}\r\n".as_bytes());
        screen.push("\u{1f4e5}".as_bytes());
        assert!(
            screen.visible_text().contains('\u{1f4e5}'),
            "{:?}",
            screen.visible_text()
        );
    }

    /// Env var that turns [`one_column_reflow_probe_child`] into the probe.
    const REFLOW_PROBE_ENV: &str = "AGENT_DOC_SCREEN_REFLOW_PROBE";
    /// Address-space ceiling of the probe child: the pre-fix reflow loop hits
    /// it within about a second (an allocation failure aborts the child)
    /// instead of growing toward the machine's memory.
    const REFLOW_PROBE_ADDRESS_SPACE: libc::rlim_t = 1 << 30;
    const REFLOW_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// Probe body, run only inside the capped child spawned by
    /// [`one_column_resize_with_a_wide_char_terminates_in_bounded_memory`].
    #[test]
    fn one_column_reflow_probe_child() {
        if std::env::var_os(REFLOW_PROBE_ENV).is_none() {
            return;
        }
        // The `sdk-demo.md` pane: drawn at 60/119 columns, then shrunk to the
        // 1-column slot of a 3-column layout, then restored.
        for target in 1..=4u16 {
            let mut screen = OwnedPtyScreen::new(size(55, 119));
            screen.push(
                "\u{1f4e5} inbox\r\n\u{2611}\u{fe0f} done [#id] \u{4f60}\u{597d}\r\n\
                 plain text that wraps across the narrow width \u{1f680}\r\n"
                    .as_bytes(),
            );
            screen.resize(size(55, target));
            screen.push("\u{1f4e5}\u{4f60}\r\n".as_bytes());
            screen.resize(size(55, 119));
            assert!(
                screen.visible_text().contains("inbox"),
                "target={target}: {}",
                screen.visible_text()
            );
        }
    }

    /// `#paneoomrunaway`: shrinking the model to 1 column with a wide glyph on
    /// screen reflowed forever (`Grid::shrink_columns`), allocating rows at
    /// about 1.5 GB/s while holding the screen lock. The probe runs in a
    /// child process under an address-space cap and a timeout, so the
    /// regression fails fast and bounded rather than eating the machine.
    #[test]
    fn one_column_resize_with_a_wide_char_terminates_in_bounded_memory() {
        use std::os::unix::process::CommandExt;
        let exe = std::env::current_exe().expect("test binary path");
        let mut command = std::process::Command::new(exe);
        command
            .args([
                "--exact",
                "screen::tests::one_column_reflow_probe_child",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(REFLOW_PROBE_ENV, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // SAFETY: `setrlimit` is async-signal-safe and touches only the child.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: REFLOW_PROBE_ADDRESS_SPACE,
                    rlim_max: REFLOW_PROBE_ADDRESS_SPACE,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // A regression aborts the child; never write a 1 GiB core.
                let no_core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                libc::setrlimit(libc::RLIMIT_CORE, &no_core);
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn reflow probe child");
        let started = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll probe child") {
                break Some(status);
            }
            if started.elapsed() > REFLOW_PROBE_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let output = child.wait_with_output().expect("probe child output");
        let transcript = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let status = status.unwrap_or_else(|| {
            panic!(
                "1-column reflow did not terminate within {REFLOW_PROBE_TIMEOUT:?}: {transcript}"
            )
        });
        assert!(
            status.success(),
            "1-column reflow probe failed ({status}); an allocation-failure abort means \
             the reflow grew without bound: {transcript}"
        );
        assert!(
            transcript.contains("1 passed"),
            "probe child did not run the probe: {transcript}"
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
