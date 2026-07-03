//! Supervisor-owned PTY output state.
//!
//! The orchestration loop decides what prompt/busy evidence means. This module
//! owns the low-level terminal projection and bounded recent-output buffer that
//! those decisions read.

use std::sync::Mutex;

use portable_pty::PtySize;

use crate::screen::OwnedPtyScreen;

const DEFAULT_RECENT_OUTPUT_BYTES_MAX: usize = 64 * 1024;

pub struct SupervisorOutputState {
    recent_output: Mutex<Vec<u8>>,
    terminal_screen: Mutex<OwnedPtyScreen>,
    recent_output_bytes_max: usize,
}

impl SupervisorOutputState {
    pub fn new(recent_output_bytes_max: usize) -> Self {
        Self {
            recent_output: Mutex::new(Vec::new()),
            terminal_screen: Mutex::new(OwnedPtyScreen::default()),
            recent_output_bytes_max,
        }
    }

    pub fn record_recent_output(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut recent = self.recent_output.lock().unwrap();
        recent.extend_from_slice(bytes);
        if recent.len() > self.recent_output_bytes_max {
            let overflow = recent.len() - self.recent_output_bytes_max;
            recent.drain(..overflow);
        }
    }

    pub fn record_terminal_screen(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.terminal_screen.lock().unwrap().push(bytes);
    }

    pub fn reset_terminal_screen(&self, size: PtySize) {
        self.terminal_screen.lock().unwrap().reset(size);
    }

    pub fn resize_terminal_screen(&self, size: PtySize) {
        self.terminal_screen.lock().unwrap().resize(size);
    }

    pub fn clear_recent_output(&self) {
        self.recent_output.lock().unwrap().clear();
    }

    pub fn child_output_for_detection(&self) -> String {
        let screen = self.terminal_screen.lock().unwrap().visible_text();
        if screen.trim().is_empty() {
            let recent = self.recent_output.lock().unwrap();
            String::from_utf8_lossy(&recent).into_owned()
        } else {
            screen
        }
    }

    pub fn with_recent_output<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        let recent = self.recent_output.lock().unwrap();
        f(&recent)
    }
}

impl Default for SupervisorOutputState {
    fn default() -> Self {
        Self::new(DEFAULT_RECENT_OUTPUT_BYTES_MAX)
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
    fn recent_output_is_bounded() {
        let state = SupervisorOutputState::new(4);
        state.record_recent_output(b"abcdef");

        state.with_recent_output(|recent| assert_eq!(recent, b"cdef"));
    }

    #[test]
    fn detection_output_prefers_visible_screen_when_present() {
        let state = SupervisorOutputState::new(64);
        state.record_recent_output(b"stale prompt");
        state.record_terminal_screen(b"fresh");

        assert!(state.child_output_for_detection().contains("fresh"));
    }

    #[test]
    fn reset_terminal_screen_clears_visible_screen() {
        let state = SupervisorOutputState::new(64);
        state.record_terminal_screen(b"busy");
        state.reset_terminal_screen(size(3, 12));

        assert!(state.child_output_for_detection().trim().is_empty());
    }
}
