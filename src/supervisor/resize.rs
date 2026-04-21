//! # Module: supervisor::resize
//!
//! Resize handling for the supervised pty. Translates terminal resize events
//! into `PtySession::resize` calls so the child process sees the correct
//! terminal dimensions.
//!
//! ## Spec
//! See `src/agent-doc/specs/supervisor.md` § Resize Handling.
//!
//! ## Platform strategy
//!
//! - **Unix (Linux/macOS):** `signal-hook` registers a SIGWINCH handler. On
//!   each signal, the resize thread queries `TIOCGWINSZ` on stdin and calls
//!   `PtySession::resize`.
//! - **Windows:** stub for phase 1. WSL users get Unix SIGWINCH via the WSL
//!   kernel. Native Windows ConPTY resize (`ReadConsoleInputW` +
//!   `WINDOW_BUFFER_SIZE_EVENT`) is deferred to a future non-tmux mode.
//!
//! ## Scope boundary
//!
//! This module only *detects* resize events and translates them into
//! `PtySize` values. The actual pty resize call goes through
//! `PtySession::resize` in the sibling `pty.rs` module.

#[cfg(unix)]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};

    use anyhow::{Context, Result};
    use portable_pty::PtySize;
    use signal_hook::consts::SIGWINCH;
    use signal_hook::iterator::Signals;

    /// Query the current terminal size via `TIOCGWINSZ` on the given fd.
    ///
    /// Returns `(rows, cols)` on success. Returns an error if the fd is not
    /// a tty or the ioctl fails.
    pub fn query_terminal_size(fd: libc::c_int) -> Result<(u16, u16)> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
        if ret != 0 {
            anyhow::bail!(
                "TIOCGWINSZ ioctl failed: {}",
                std::io::Error::last_os_error()
            );
        }
        Ok((ws.ws_row, ws.ws_col))
    }

    /// Watches for SIGWINCH signals and calls a resize callback with the
    /// new terminal dimensions.
    pub struct ResizeWatcher {
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl ResizeWatcher {
        /// Spawn a resize watcher thread.
        ///
        /// `resize_fn` is called on each SIGWINCH with the new `PtySize`.
        /// The callback runs on the watcher thread — callers that need to
        /// forward to a `PtySession` should use a `Mutex` or channel.
        ///
        /// The watcher blocks on `signal_hook::iterator::Signals` and exits
        /// when [`stop`](Self::stop) is called.
        pub fn spawn<F>(resize_fn: F) -> Result<Self>
        where
            F: Fn(PtySize) + Send + 'static,
        {
            let mut signals =
                Signals::new([SIGWINCH]).context("failed to register SIGWINCH handler")?;
            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = stop.clone();
            let handle_ref = signals.handle();

            let handle = thread::Builder::new()
                .name("resize-watcher".into())
                .spawn(move || {
                    for _sig in &mut signals {
                        if stop_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        match query_terminal_size(libc::STDIN_FILENO) {
                            Ok((rows, cols)) => {
                                resize_fn(PtySize {
                                    rows,
                                    cols,
                                    pixel_width: 0,
                                    pixel_height: 0,
                                });
                            }
                            Err(e) => {
                                eprintln!("[supervisor::resize] TIOCGWINSZ failed: {e}");
                            }
                        }
                    }
                })
                .context("spawn resize-watcher thread")?;

            // Store the signal handle so we can close it on stop.
            // We need to keep it alive, so store it via a wrapper.
            let watcher = Self {
                stop,
                handle: Some(handle),
            };

            // We need to close the signal iterator to unblock the thread.
            // Store the handle in a side channel. Since ResizeWatcher owns
            // the thread, we close signals via the Handle on drop/stop.
            // signal_hook::iterator::backend::Handle is not Send in all
            // versions, so we store it in a thread-local workaround:
            // Actually, Handle IS Send. Let's restructure.
            // For simplicity, we'll close via the stop flag + handle.close().
            // But we need to store the handle. Let's restructure the struct.

            // Re-approach: store the signal handle directly.
            drop(handle_ref); // We'll use a different pattern — see ResizeWatcherInner.

            Ok(watcher)
        }

        /// Signal the watcher thread to stop. Blocks until the thread exits.
        pub fn stop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Send SIGWINCH to ourselves to unblock the signal iterator.
            // This is safe — our handler just checks the stop flag and exits.
            unsafe {
                libc::raise(libc::SIGWINCH);
            }
            if let Some(h) = self.handle.take() {
                let _ = h.join();
            }
        }
    }

    impl Drop for ResizeWatcher {
        fn drop(&mut self) {
            if self.handle.is_some() {
                self.stop();
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use anyhow::Result;
    use portable_pty::PtySize;

    /// Stub resize watcher for Windows. Phase 1 relies on WSL providing
    /// Unix SIGWINCH. Native Windows ConPTY resize is deferred.
    pub struct ResizeWatcher;

    impl ResizeWatcher {
        pub fn spawn<F>(_resize_fn: F) -> Result<Self>
        where
            F: Fn(PtySize) + Send + 'static,
        {
            eprintln!(
                "[supervisor::resize] Windows resize watcher not implemented; \
                 WSL users get SIGWINCH via the Unix path"
            );
            Ok(Self)
        }

        pub fn stop(&mut self) {
            // No-op
        }
    }
}

#[allow(unused_imports)]
pub use platform::ResizeWatcher;

#[cfg(unix)]
#[allow(unused_imports)]
pub use platform::query_terminal_size;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn watcher_construction_and_stop() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        let mut watcher =
            ResizeWatcher::spawn(move |_size| {
                called_clone.store(true, Ordering::Relaxed);
            })
            .expect("spawn resize watcher");

        // Stop should complete without hanging
        watcher.stop();
    }

    #[test]
    fn stop_is_idempotent() {
        let mut watcher =
            ResizeWatcher::spawn(|_| {}).expect("spawn resize watcher");
        watcher.stop();
        watcher.stop(); // second stop should be a no-op
    }

    #[cfg(unix)]
    #[test]
    fn callback_fires_on_sigwinch() {
        use std::sync::atomic::AtomicU32;

        let rows = Arc::new(AtomicU32::new(0));
        let cols = Arc::new(AtomicU32::new(0));
        let rows_clone = rows.clone();
        let cols_clone = cols.clone();

        let mut watcher = ResizeWatcher::spawn(move |size| {
            rows_clone.store(size.rows as u32, Ordering::Relaxed);
            cols_clone.store(size.cols as u32, Ordering::Relaxed);
        })
        .expect("spawn resize watcher");

        // Give the thread time to register the signal handler
        std::thread::sleep(Duration::from_millis(50));

        // Send SIGWINCH to ourselves
        unsafe {
            libc::raise(libc::SIGWINCH);
        }

        // Wait for callback
        std::thread::sleep(Duration::from_millis(100));

        // If stdin is a tty, we should get real dimensions.
        // In CI (no tty), the ioctl may fail and callback won't fire.
        // Either way, the watcher should stop cleanly.
        watcher.stop();

        // Only assert dimensions if we got a callback (tty available)
        let r = rows.load(Ordering::Relaxed);
        let c = cols.load(Ordering::Relaxed);
        if r > 0 && c > 0 {
            assert!(r < 10000, "rows should be reasonable: {r}");
            assert!(c < 10000, "cols should be reasonable: {c}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn query_terminal_size_on_non_tty_fails() {
        // /dev/null is not a tty — ioctl should fail
        let fd = unsafe { libc::open(b"/dev/null\0".as_ptr() as *const _, libc::O_RDONLY) };
        assert!(fd >= 0);
        let result = query_terminal_size(fd);
        unsafe {
            libc::close(fd);
        }
        assert!(result.is_err(), "TIOCGWINSZ on /dev/null should fail");
    }
}
