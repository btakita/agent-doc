//! Process-entry descriptor ownership (`#statedbfdclobber`).
//!
//! `controller serve` drops descriptors an IDE or harness launcher leaked into
//! it. It used to decide what counted as "inherited" by listing
//! `/proc/self/fd` when `run_serve` started, which is after `main` had already
//! opened descriptors of its own. The one it always found was the
//! `AGENT_DOC_LOG` debug-log appender's file, still owned by the
//! `tracing_appender` worker thread. That is why
//! `controller_serve_inherited_fds_closed count=1` showed up even for a
//! controller launched with nothing but stdio. The next `open()`, SQLite
//! opening `state.db`, reused that descriptor number. The worker's next
//! write, usually the queued `agent-doc tracing initialized (filter: debug)`
//! line, then landed at offset 0 of `state.db` and destroyed its 100-byte
//! header, so every later open failed with `file is not a database`.
//!
//! **Invariant.** A process closes only descriptors it can prove it
//! inherited. A descriptor qualifies only if it appears in the snapshot taken
//! at process entry, before the process opened any descriptor of its own, and
//! still names the same open file (device and inode). Each snapshot entry is
//! closed at most once. Descriptors 0-2 are never closed. A missing one is
//! reopened onto `/dev/null` at entry, so a later `open()` can never become
//! stdio.

#[cfg(unix)]
use std::os::unix::io::RawFd;
#[cfg(unix)]
use std::sync::Mutex;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

/// Device and inode of the open file a descriptor names.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdIdentity {
    dev: u64,
    ino: u64,
}

/// One descriptor that was already open when the process entered `main`.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryFd {
    fd: RawFd,
    identity: FdIdentity,
}

#[cfg(unix)]
impl EntryFd {
    /// Record `fd` together with the identity of the file it names now.
    pub fn observe(fd: RawFd) -> Option<Self> {
        fd_identity(fd).map(|identity| Self { fd, identity })
    }

    pub fn fd(&self) -> RawFd {
        self.fd
    }

    fn still_names_same_file(&self) -> bool {
        fd_identity(self.fd) == Some(self.identity)
    }
}

/// What [`close_entry_fds`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntryFdCloseOutcome {
    /// Inherited descriptors closed.
    pub closed: usize,
    /// Snapshot entries left alone because the number no longer names the
    /// inherited file: someone already closed it, and the number may now belong
    /// to a file this process opened.
    pub skipped_not_inherited: usize,
    /// No process-entry snapshot was available, so nothing was closed.
    pub snapshot_missing: bool,
}

#[cfg(unix)]
static PROCESS_ENTRY_FDS: Mutex<Option<Vec<EntryFd>>> = Mutex::new(None);
#[cfg(unix)]
static PROCESS_ENTRY_FDS_CAPTURED: AtomicBool = AtomicBool::new(false);

/// Snapshot the descriptors this process inherited. `main` must call this
/// first, before it starts a thread or opens a file (tracing, logs, sockets,
/// databases), so every descriptor recorded here belongs to the launcher.
///
/// Also reopens a closed fd 0, 1 or 2 onto `/dev/null`, so no later `open()`
/// is handed a stdio number that `eprintln!` or a child's stdio would then
/// write through. Only the first call records a snapshot.
pub fn capture_process_entry_fds() {
    #[cfg(unix)]
    {
        if PROCESS_ENTRY_FDS_CAPTURED.swap(true, Ordering::SeqCst) {
            return;
        }
        ensure_standard_fds_open();
        let entries = scan_open_fds_above_stderr();
        *PROCESS_ENTRY_FDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(entries);
    }
}

/// Close the descriptors recorded by [`capture_process_entry_fds`] that still
/// name the file they named at entry. The snapshot is consumed, so a second
/// call closes nothing. Without a snapshot this closes nothing at all:
/// guessing from the current descriptor table is exactly how the debug-log
/// descriptor was closed out from under its writer.
pub fn close_process_entry_fds() -> EntryFdCloseOutcome {
    #[cfg(unix)]
    {
        let entries = PROCESS_ENTRY_FDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match entries {
            Some(entries) => close_entry_fds(&entries),
            None => EntryFdCloseOutcome {
                snapshot_missing: true,
                ..EntryFdCloseOutcome::default()
            },
        }
    }
    #[cfg(not(unix))]
    {
        EntryFdCloseOutcome::default()
    }
}

/// Close each entry that still names its inherited file. Stdio is never
/// touched, and a number that now names a different file is skipped.
#[cfg(unix)]
pub fn close_entry_fds(entries: &[EntryFd]) -> EntryFdCloseOutcome {
    let mut outcome = EntryFdCloseOutcome::default();
    for entry in entries {
        if entry.fd <= libc::STDERR_FILENO {
            continue;
        }
        if !entry.still_names_same_file() {
            outcome.skipped_not_inherited += 1;
            continue;
        }
        if unsafe { libc::close(entry.fd) } == 0 {
            outcome.closed += 1;
        }
    }
    outcome
}

#[cfg(unix)]
// `st_dev`/`st_ino` are `u64` on Linux but narrower or signed elsewhere.
#[allow(clippy::unnecessary_cast)]
fn fd_identity(fd: RawFd) -> Option<FdIdentity> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    Some(FdIdentity {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
    })
}

/// Every open descriptor above stderr, with its identity. The directory handle
/// the listing itself uses is closed again before the identities are read, so
/// it is never recorded (the identity probe fails with `EBADF`).
#[cfg(unix)]
fn scan_open_fds_above_stderr() -> Vec<EntryFd> {
    let candidates = list_fd_directory().unwrap_or_else(|| (3..fallback_fd_limit()).collect());
    candidates
        .into_iter()
        .filter(|fd| *fd > libc::STDERR_FILENO)
        .filter_map(EntryFd::observe)
        .collect()
}

#[cfg(unix)]
fn list_fd_directory() -> Option<Vec<RawFd>> {
    #[cfg(target_os = "linux")]
    const FD_DIRECTORY: &str = "/proc/self/fd";
    #[cfg(not(target_os = "linux"))]
    const FD_DIRECTORY: &str = "/dev/fd";
    let fds = std::fs::read_dir(FD_DIRECTORY)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<RawFd>().ok())
        .collect();
    Some(fds)
}

#[cfg(unix)]
fn fallback_fd_limit() -> RawFd {
    let limit = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
    if (4..=65_536).contains(&limit) {
        limit as RawFd
    } else {
        1024
    }
}

#[cfg(unix)]
fn ensure_standard_fds_open() {
    for fd in libc::STDIN_FILENO..=libc::STDERR_FILENO {
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } != -1 {
            continue;
        }
        let flags = if fd == libc::STDIN_FILENO {
            libc::O_RDONLY
        } else {
            libc::O_WRONLY
        };
        let opened = unsafe { libc::open(c"/dev/null".as_ptr(), flags) };
        if opened >= 0 && opened != fd {
            unsafe {
                libc::dup2(opened, fd);
                libc::close(opened);
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};
    use std::path::Path;

    const CHILD_MODE_ENV: &str = "AGENT_DOC_STATEDB_FD_CLOBBER_CHILD_MODE";
    const CHILD_ROOT_ENV: &str = "AGENT_DOC_STATEDB_FD_CLOBBER_CHILD_ROOT";
    const CHILD_TEST: &str = "inherited_fds::tests::statedb_fd_clobber_child_probe";
    /// Where the probe's parent plants a leaked launcher descriptor.
    const LAUNCHER_FD: RawFd = 40;
    /// The exact line production wrote over `state.db`'s header.
    const TRACING_LINE: &str = "2026-10-15T16:47:00.000000Z DEBUG ThreadId(01) agent_doc: agent-doc tracing initialized (filter: debug)\n";
    const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";

    fn open_scratch(dir: &Path, name: &str) -> std::fs::File {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(name))
            .unwrap()
    }

    /// Move `file` to a descriptor number at or above `floor`, far above the
    /// lowest-free numbers concurrently running tests are handed, so this test
    /// owns the number it reasons about.
    fn move_to_high_fd(file: std::fs::File, floor: RawFd) -> RawFd {
        let high = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, floor) };
        assert!(high >= floor, "F_DUPFD_CLOEXEC failed");
        drop(file);
        high
    }

    #[test]
    fn close_entry_fds_spares_descriptors_opened_after_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let inherited = move_to_high_fd(open_scratch(dir.path(), "inherited"), 700);
        let snapshot = [EntryFd::observe(inherited).unwrap()];
        let mut own_log = open_scratch(dir.path(), "own.log");

        let outcome = close_entry_fds(&snapshot);

        assert_eq!(outcome.closed, 1, "{outcome:?}");
        assert_eq!(fd_identity(inherited), None, "the inherited fd is closed");
        own_log
            .write_all(b"still mine\n")
            .expect("a descriptor opened after entry must survive the sanitizer");
    }

    /// The fd-recycle interleaving itself: the inherited number was closed and
    /// handed to a file this process opened. Closing by number would destroy
    /// that file's descriptor (and its writer would then write wherever the
    /// number goes next), so the sanitizer must refuse.
    #[test]
    fn close_entry_fds_refuses_a_recycled_descriptor_number() {
        let dir = tempfile::tempdir().unwrap();
        let inherited = move_to_high_fd(open_scratch(dir.path(), "inherited"), 760);
        let snapshot = [EntryFd::observe(inherited).unwrap()];
        unsafe { libc::close(inherited) };
        let own = open_scratch(dir.path(), "own.log");
        assert_eq!(
            unsafe { libc::dup2(own.as_raw_fd(), inherited) },
            inherited,
            "recycle the inherited number onto this process's own file"
        );
        drop(own);
        let own_identity = fd_identity(inherited).unwrap();

        let outcome = close_entry_fds(&snapshot);

        assert_eq!(outcome.closed, 0, "{outcome:?}");
        assert_eq!(outcome.skipped_not_inherited, 1, "{outcome:?}");
        assert_eq!(fd_identity(inherited), Some(own_identity));
        unsafe { libc::close(inherited) };
    }

    #[test]
    fn close_entry_fds_never_closes_stdio() {
        let snapshot: Vec<EntryFd> = (0..=2).filter_map(EntryFd::observe).collect();
        let outcome = close_entry_fds(&snapshot);
        assert_eq!(outcome.closed, 0, "{outcome:?}");
        for fd in 0..=2 {
            assert_ne!(unsafe { libc::fcntl(fd, libc::F_GETFD) }, -1, "fd {fd}");
        }
    }

    /// End-to-end, in a fresh process so the descriptor table is this
    /// scenario's alone and fd reuse is deterministic: a log writer owned by
    /// another thread, an event queued before `controller serve` sanitizes
    /// descriptors, `state.db` opened after, and the queued line written last.
    /// That is the `tracing_appender::non_blocking` worker's order when it is
    /// slow to be scheduled, which load makes likely.
    struct ProbeReport {
        head: Vec<u8>,
        log: String,
        closed: usize,
        launcher_fd_open: bool,
    }

    fn probe_field<'a>(stdout: &'a str, key: &str) -> &'a str {
        stdout
            .lines()
            .find_map(|line| line.split_once(key).map(|(_, value)| value.trim()))
            .unwrap_or_else(|| panic!("probe printed no {key}: {stdout}"))
    }

    fn run_clobber_probe(mode: &str) -> ProbeReport {
        use std::os::unix::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        // A descriptor an IDE launcher leaked: open, without CLOEXEC, at a fixed
        // number in the child before it runs a single line of Rust.
        let leaked = open_scratch(root.path(), "leaked-by-launcher");
        let leaked_fd = leaked.as_raw_fd();
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                CHILD_TEST,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_MODE_ENV, mode)
            .env(CHILD_ROOT_ENV, root.path());
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(leaked_fd, LAUNCHER_FD) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = command.output().expect("spawn fd clobber probe");
        drop(leaked);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "probe failed: stdout={stdout} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let head_hex = probe_field(&stdout, "PROBE_HEAD=");
        let head = (0..head_hex.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&head_hex[at..at + 2], 16).unwrap())
            .collect();
        let log = std::fs::read_to_string(root.path().join(".agent-doc/logs/debug.log"))
            .unwrap_or_default();
        ProbeReport {
            head,
            log,
            closed: probe_field(&stdout, "PROBE_CLOSED=").parse().unwrap(),
            launcher_fd_open: probe_field(&stdout, "PROBE_LAUNCHER_FD_OPEN=") == "true",
        }
    }

    #[test]
    fn controller_serve_sanitizer_never_lets_a_log_writer_clobber_state_db() {
        let report = run_clobber_probe("fixed");
        assert_eq!(
            report.head.as_slice(),
            SQLITE_HEADER,
            "state.db header was overwritten: {:?}",
            String::from_utf8_lossy(&report.head)
        );
        assert_eq!(
            report.log, TRACING_LINE,
            "the queued line belongs in the debug log"
        );
        assert_eq!(report.closed, 1, "exactly the launcher's leaked descriptor");
        assert!(
            !report.launcher_fd_open,
            "the leaked descriptor is still closed"
        );
    }

    /// Negative control: the pre-fix policy (close every descriptor above
    /// stderr that is open when `run_serve` starts) reproduces the production
    /// bytes in the same harness. If this stops clobbering, the harness above no
    /// longer exercises the interleaving and proves nothing.
    #[test]
    fn legacy_close_everything_policy_clobbers_state_db_in_the_same_harness() {
        let report = run_clobber_probe("legacy");
        assert_eq!(
            report.head.as_slice(),
            &TRACING_LINE.as_bytes()[..16],
            "expected the tracing line at offset 0 of state.db, got {:?}",
            String::from_utf8_lossy(&report.head)
        );
        assert!(
            report.log.is_empty(),
            "the line must have missed the debug log: {:?}",
            report.log
        );
        assert_eq!(
            report.closed, 2,
            "the launcher's descriptor plus this process's own debug log"
        );
    }

    /// The pre-fix `sanitize_controller_serve_inherited_fds`, kept only as the
    /// negative control's reproduction.
    fn legacy_close_every_fd_above_stderr() -> usize {
        let fds: Vec<RawFd> = list_fd_directory().unwrap();
        fds.into_iter()
            .filter(|fd| *fd > 2)
            .filter(|fd| unsafe { libc::close(*fd) } == 0)
            .count()
    }

    #[test]
    #[ignore = "child-process probe; driven by the state.db clobber tests"]
    fn statedb_fd_clobber_child_probe() {
        let (Some(mode), Some(root)) = (
            std::env::var(CHILD_MODE_ENV).ok(),
            std::env::var_os(CHILD_ROOT_ENV),
        ) else {
            return;
        };
        let root = Path::new(&root);
        // `main`'s order: snapshot first, then the tracing appender opens its
        // file and hands it to a worker thread.
        if mode == "fixed" {
            capture_process_entry_fds();
        }
        let logs = root.join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        let log = open_scratch(&logs, "debug.log");
        let log_fd = log.as_raw_fd();
        let (queue, queued) = std::sync::mpsc::channel::<&'static str>();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let mut log = log;
            let line = queued.recv().unwrap();
            released.recv().unwrap();
            // Like the appender worker, write through whatever the number now
            // names; a failure is dropped, not reported.
            let _ = log.write_all(line.as_bytes());
            // Never close a number this thread may no longer own.
            let _ = log.into_raw_fd();
        });
        queue.send(TRACING_LINE).unwrap();

        let closed = if mode == "fixed" {
            close_process_entry_fds().closed
        } else {
            legacy_close_every_fd_above_stderr()
        };
        let conn = agent_doc_sqlite::state_store::open_state_db(root).unwrap();
        let db_fd_target = std::fs::read_link(format!("/proc/self/fd/{log_fd}"))
            .map(|target| target.display().to_string())
            .unwrap_or_default();
        release.send(()).unwrap();
        worker.join().unwrap();
        // Read the header while the connection is still open: closing the last
        // connection checkpoints the WAL and rewrites page 1, which would hide
        // the clobber this probe exists to observe.
        let mut head = [0u8; 16];
        std::fs::File::open(root.join(".agent-doc/state.db"))
            .and_then(|mut db| db.read_exact(&mut head))
            .unwrap();
        let head_hex: String = head.iter().map(|byte| format!("{byte:02x}")).collect();
        println!("PROBE_HEAD={head_hex}");
        println!("PROBE_CLOSED={closed}");
        println!(
            "PROBE_LAUNCHER_FD_OPEN={}",
            unsafe { libc::fcntl(LAUNCHER_FD, libc::F_GETFD) } != -1
        );
        println!("PROBE_LOG_FD_TARGET={db_fd_target}");
        drop(conn);
        if mode == "fixed" {
            // The log descriptor was never closed, so dropping its number here
            // is this probe's own cleanup.
            drop(unsafe { std::fs::File::from_raw_fd(log_fd) });
        }
        println!("PROBE_DONE mode={mode} closed={closed} log_fd={log_fd}");
    }
}
