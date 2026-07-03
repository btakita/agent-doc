//! Supervisor PTY reader and stdin-forwarder threads.
//!
//! This module owns the low-level thread loops. The caller supplies small
//! observers for prompt/state decisions that still live above the process layer.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use agent_doc_harness::HarnessConfig;

use crate::shared_writer::{SharedPtyWriter, lock_writer_interruptibly};

pub trait PtyReaderObserver: Send + Sync + 'static {
    fn on_filtered_pty_output(&self, harness: &HarnessConfig, bytes: &[u8]);
}

pub trait StdinForwardObserver: Send + Sync + 'static {
    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool;
    fn prompt_visible_once(&self) -> bool;
    fn normalize_permission_prompt_input(
        &self,
        harness: &HarnessConfig,
        data: &[u8],
    ) -> Option<Vec<u8>>;
}

/// Spawn the master-to-stdout forwarding thread with escape sequence filtering.
pub fn spawn_reader_thread<T>(
    observer: Arc<T>,
    harness: HarnessConfig,
    mut reader: Box<dyn Read + Send>,
) -> std::thread::JoinHandle<()>
where
    T: PtyReaderObserver,
{
    std::thread::Builder::new()
        .name("pty->stdout".into())
        .spawn(move || {
            let mut buf = [0u8; 8192];
            let mut filtered = Vec::with_capacity(8192);
            let stdout = std::io::stdout();
            let debug_filter = std::env::var("AGENT_DOC_DEBUG_FILTER").is_ok();
            let mut pty_filter = crate::pty::PtyFilter::for_harness(&harness);
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if debug_filter {
                            let raw = &buf[..n];
                            let mut display = String::new();
                            for &b in raw {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!("[pty-filter] raw ({n} bytes): {display}");
                        }
                        filtered.clear();
                        pty_filter.filter(&buf[..n], &mut filtered);
                        if debug_filter {
                            let mut display = String::new();
                            for &b in &filtered {
                                if b == 0x1b {
                                    display.push_str("\\x1b");
                                } else if b.is_ascii_graphic() || b == b' ' {
                                    display.push(b as char);
                                } else {
                                    display.push_str(&format!("\\x{b:02x}"));
                                }
                            }
                            eprintln!(
                                "[pty-filter] filtered ({} bytes): {display}",
                                filtered.len()
                            );
                        }
                        if filtered.is_empty() {
                            continue;
                        }
                        observer.on_filtered_pty_output(&harness, &filtered);
                        let mut lock = stdout.lock();
                        if lock.write_all(&filtered).is_err() || lock.flush().is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn pty->stdout thread")
}

/// Spawn the stdin-to-master forwarding thread using a shared writer.
///
/// Uses `poll()` on stdin plus a stop pipe so the thread can be interrupted
/// cleanly before the supervisor needs stdin for the restart prompt.
#[cfg(unix)]
pub fn spawn_writer_thread<T>(
    observer: Arc<T>,
    harness: HarnessConfig,
    writer: Arc<Mutex<SharedPtyWriter>>,
    stop_fd: std::os::unix::io::RawFd,
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()>
where
    T: StdinForwardObserver,
{
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let debug = std::env::var("AGENT_DOC_DEBUG_STDIN").is_ok();
            if debug {
                eprintln!("[stdin->pty] thread started");
            }
            loop {
                let mut fds = [
                    libc::pollfd {
                        fd: libc::STDIN_FILENO,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: stop_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                let ret = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                if ret <= 0 {
                    if debug {
                        eprintln!("[stdin->pty] poll returned {ret}, exiting");
                    }
                    break;
                }
                if fds[1].revents & libc::POLLIN != 0 {
                    if debug {
                        eprintln!("[stdin->pty] stop signal received, exiting");
                    }
                    break;
                }
                if fds[0].revents & libc::POLLIN != 0 {
                    let n = unsafe {
                        libc::read(
                            libc::STDIN_FILENO,
                            buf.as_mut_ptr() as *mut libc::c_void,
                            buf.len(),
                        )
                    };
                    if n <= 0 {
                        if debug {
                            eprintln!("[stdin->pty] read returned {n}, exiting");
                        }
                        break;
                    }
                    let data = &buf[..n as usize];
                    let maybe_filtered =
                        agent_doc_supervisor::input::strip_stale_ctrl_d_before_prompt(
                            data,
                            observer.suppress_stale_ctrl_d_until_prompt(),
                            observer.prompt_visible_once(),
                        );
                    if let Some(filtered) = maybe_filtered.as_deref() {
                        agent_doc_tmux_io::input_diag::log_transform_event(
                            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                None,
                                agent_doc_ops_log_io::log_op,
                            ),
                            "supervisor.stdin",
                            "child_pty",
                            "drop_stale_ctrl_d_before_prompt",
                            data,
                            filtered,
                            Some(&harness.binary),
                        );
                    }
                    let data = maybe_filtered.as_deref().unwrap_or(data);
                    if data.is_empty() {
                        if debug {
                            eprintln!(
                                "[stdin->pty] suppressed stale Ctrl+D before keepalive prompt"
                            );
                        }
                        continue;
                    }
                    let maybe_translated =
                        observer.normalize_permission_prompt_input(&harness, data);
                    if let Some(translated) = maybe_translated.as_deref() {
                        agent_doc_tmux_io::input_diag::log_prompt_detection(
                            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                None,
                                agent_doc_ops_log_io::log_op,
                            ),
                            "supervisor.stdin",
                            "child_pty",
                            &harness.binary,
                            "active permission prompt",
                            "active",
                        );
                        agent_doc_tmux_io::input_diag::log_transform_event(
                            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                None,
                                agent_doc_ops_log_io::log_op,
                            ),
                            "supervisor.stdin",
                            "child_pty",
                            "opencode_permission_arrow_translation",
                            data,
                            translated,
                            Some(&harness.binary),
                        );
                    }
                    let data = maybe_translated.as_deref().unwrap_or(data);
                    if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                        agent_doc_tmux_io::input_diag::log_byte_events(
                            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                None,
                                agent_doc_ops_log_io::log_op,
                            ),
                            "supervisor.stdin",
                            "child_pty",
                            "raw_forward",
                            data,
                            Some(&harness.binary),
                        );
                    }
                    if let Some(ref flag) = ctrl_d_flag
                        && data.contains(&0x04)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+D (\\x04) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    if let Some(ref flag) = ctrl_c_flag
                        && data.contains(&0x03)
                    {
                        if debug {
                            eprintln!("[stdin->pty] Ctrl+C (\\x03) detected in forwarded data");
                        }
                        flag.store(true, Ordering::Relaxed);
                    }
                    let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                        if debug {
                            eprintln!("[stdin->pty] stop requested while waiting for writer");
                        }
                        break;
                    };
                    if let Err(err) = w.write_all_interruptibly(data, stop.as_ref()) {
                        if debug {
                            eprintln!("[stdin->pty] pty write failed, exiting: {err}");
                        }
                        break;
                    }
                }
                if fds[0].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    if debug {
                        eprintln!(
                            "[stdin->pty] stdin hangup/error (revents=0x{:x}), exiting",
                            fds[0].revents
                        );
                    }
                    break;
                }
            }
            if debug {
                eprintln!("[stdin->pty] thread exiting");
            }
        })
        .expect("spawn stdin->pty thread")
}

/// Non-Unix fallback: blocking stdin read with no stop pipe support.
#[cfg(not(unix))]
pub fn spawn_writer_thread<T>(
    _observer: Arc<T>,
    _harness: HarnessConfig,
    writer: Arc<Mutex<SharedPtyWriter>>,
    _stop_fd: (),
    stop: Arc<AtomicBool>,
    ctrl_c_flag: Option<Arc<AtomicBool>>,
    ctrl_d_flag: Option<Arc<AtomicBool>>,
) -> std::thread::JoinHandle<()>
where
    T: StdinForwardObserver,
{
    std::thread::Builder::new()
        .name("stdin->pty".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let stdin = std::io::stdin();
            loop {
                let mut lock = stdin.lock();
                match std::io::Read::read(&mut lock, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        drop(lock);
                        if let Some(ref flag) = ctrl_d_flag
                            && buf[..n].contains(&0x04)
                        {
                            flag.store(true, Ordering::Relaxed);
                        }
                        if let Some(ref flag) = ctrl_c_flag
                            && buf[..n].contains(&0x03)
                        {
                            flag.store(true, Ordering::Relaxed);
                        }
                        if agent_doc_tmux_commands::input_diag::verbose_enabled() {
                            agent_doc_tmux_io::input_diag::log_byte_events(
                                agent_doc_tmux_io::input_diag::InputDiagSink::new(
                                    None,
                                    agent_doc_ops_log_io::log_op,
                                ),
                                "supervisor.stdin",
                                "child_pty",
                                "raw_forward",
                                &buf[..n],
                                None,
                            );
                        }
                        let Some(mut w) = lock_writer_interruptibly(&writer, stop.as_ref()) else {
                            break;
                        };
                        if w.write_all_interruptibly(&buf[..n], stop.as_ref()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn stdin->pty thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_writer::StopSignal;

    struct NoopObserver;

    impl PtyReaderObserver for NoopObserver {
        fn on_filtered_pty_output(&self, _harness: &HarnessConfig, _bytes: &[u8]) {}
    }

    impl StdinForwardObserver for NoopObserver {
        fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
            false
        }

        fn prompt_visible_once(&self) -> bool {
            false
        }

        fn normalize_permission_prompt_input(
            &self,
            _harness: &HarnessConfig,
            _data: &[u8],
        ) -> Option<Vec<u8>> {
            None
        }
    }

    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_stop_signal() {
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        let pty_write_fd = pty_fds[1];

        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_write_fd));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = spawn_writer_thread(
            Arc::new(NoopObserver),
            HarnessConfig::codex(),
            writer_arc,
            stop.read_fd(),
            stop_flag.clone(),
            None,
            None,
        );

        std::thread::sleep(std::time::Duration::from_millis(50));

        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();
        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit cleanly on stop signal"
        );

        unsafe {
            libc::close(pty_fds[0]);
            libc::close(pty_fds[1]);
        }
    }

    #[cfg(unix)]
    #[test]
    fn writer_thread_exits_on_pty_write_failure() {
        let mut pty_fds = [0i32; 2];
        unsafe { libc::pipe(pty_fds.as_mut_ptr()) };
        unsafe { libc::close(pty_fds[0]) };

        struct FdWriter(i32);
        impl Write for FdWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::write(self.0, buf.as_ptr() as *const libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let writer: Box<dyn Write + Send> = Box::new(FdWriter(pty_fds[1]));
        let writer_arc = Arc::new(Mutex::new(SharedPtyWriter::new(writer)));

        let stop = StopSignal::new().unwrap();
        let stop_fd = stop.read_fd();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let handle = spawn_writer_thread(
            Arc::new(NoopObserver),
            HarnessConfig::codex(),
            writer_arc,
            stop_fd,
            stop_flag.clone(),
            None,
            None,
        );

        std::thread::sleep(std::time::Duration::from_millis(50));
        stop_flag.store(true, Ordering::Relaxed);
        stop.signal();

        let result = handle.join();
        assert!(
            result.is_ok(),
            "writer thread should exit on write failure or stop"
        );

        unsafe { libc::close(pty_fds[1]) };
    }

    #[cfg(unix)]
    #[test]
    fn reader_thread_exits_on_eof() {
        let mut fds = [0i32; 2];
        unsafe { libc::pipe(fds.as_mut_ptr()) };

        struct FdReader(i32);
        impl Read for FdReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n =
                    unsafe { libc::read(self.0, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }
        }

        let reader: Box<dyn Read + Send> = Box::new(FdReader(fds[0]));
        let handle = spawn_reader_thread(Arc::new(NoopObserver), HarnessConfig::codex(), reader);

        unsafe { libc::close(fds[1]) };

        let result = handle.join();
        assert!(result.is_ok(), "reader thread should exit cleanly on EOF");

        unsafe { libc::close(fds[0]) };
    }
}
