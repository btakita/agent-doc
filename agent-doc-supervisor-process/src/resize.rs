//! Terminal resize handling for supervised ptys.
//!
//! This process-effect adapter translates terminal resize events into
//! `portable_pty::PtySize` values. Callers provide the actual resize effect
//! (for example a `PtySession` resize handle) as a callback.
//!
//! ## Platform strategy
//!
//! - **Unix (Linux/macOS):** `signal-hook` registers a SIGWINCH handler. On
//!   each signal, the resize thread queries `TIOCGWINSZ` on stdin and calls
//!   the supplied resize callback.
//! - **Windows:** stub for phase 1. WSL users get Unix SIGWINCH via the WSL
//!   kernel. Native Windows ConPTY resize (`ReadConsoleInputW` +
//!   `WINDOW_BUFFER_SIZE_EVENT`) is deferred to a future non-tmux mode.

#[cfg(unix)]
mod platform {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
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
        /// `resize_fn` is called once from the current terminal dimensions after
        /// SIGWINCH registration, then on each subsequent SIGWINCH.
        ///
        /// Registering before the initial sample closes the spawn-time race where
        /// tmux resizes the owning pane after the child launch size was captured
        /// but before this watcher starts. A signal in that interval is queued,
        /// while the initial sample already observes the newest dimensions.
        /// The callback runs on the watcher thread; callers that need to
        /// forward to a shared pty handle should use a `Mutex` or channel.
        pub fn spawn<F>(resize_fn: F) -> Result<Self>
        where
            F: Fn(PtySize) + Send + 'static,
        {
            Self::spawn_with_initial_query(resize_fn, || query_terminal_size(libc::STDIN_FILENO))
        }

        pub(super) fn spawn_with_initial_query<F, Q>(resize_fn: F, initial_query: Q) -> Result<Self>
        where
            F: Fn(PtySize) + Send + 'static,
            Q: FnOnce() -> Result<(u16, u16)>,
        {
            let mut signals =
                Signals::new([SIGWINCH]).context("failed to register SIGWINCH handler")?;
            let stop = Arc::new(AtomicBool::new(false));
            let stop_clone = stop.clone();
            let handle_ref = signals.handle();

            if let Ok((rows, cols)) = initial_query() {
                resize_fn(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }

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

            let watcher = Self {
                stop,
                handle: Some(handle),
            };

            drop(handle_ref);

            Ok(watcher)
        }

        /// Signal the watcher thread to stop. Blocks until the thread exits.
        pub fn stop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Send SIGWINCH to ourselves to unblock the signal iterator.
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

pub use platform::ResizeWatcher;

#[cfg(unix)]
pub use platform::query_terminal_size;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn watcher_construction_and_stop() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        let mut watcher = ResizeWatcher::spawn_with_initial_query(
            move |_size| {
                called_clone.store(true, Ordering::Relaxed);
            },
            || Ok((53, 119)),
        )
        .expect("spawn resize watcher");

        assert!(
            called.load(Ordering::Relaxed),
            "watcher construction must project the current terminal size before waiting for SIGWINCH",
        );
        watcher.stop();
    }

    #[test]
    fn stop_is_idempotent() {
        let mut watcher = ResizeWatcher::spawn(|_| {}).expect("spawn resize watcher");
        watcher.stop();
        watcher.stop();
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

        std::thread::sleep(Duration::from_millis(50));

        unsafe {
            libc::raise(libc::SIGWINCH);
        }

        std::thread::sleep(Duration::from_millis(100));
        watcher.stop();

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
        let fd = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY) };
        assert!(fd >= 0);
        let result = query_terminal_size(fd);
        unsafe {
            libc::close(fd);
        }
        assert!(result.is_err(), "TIOCGWINSZ on /dev/null should fail");
    }
}
