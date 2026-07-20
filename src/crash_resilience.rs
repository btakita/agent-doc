//! Process-wide crash resilience for the `agent-doc` binary — especially the
//! long-lived route-owned supervisor (`#supresilience`).
//!
//! Two failure modes are neutralized here:
//!
//! 1. **Broken-pipe SIGABRT.** Rust's default `println!` / `eprintln!` *panic*
//!    when the output pipe is broken (`EPIPE`). When the supervisor's tmux pane
//!    or terminal goes away (for example when the host editor crashed and tore
//!    down its panes), the next print panics, and the default panic hook's own
//!    stderr write panics *again* → "thread panicked while processing panic.
//!    aborting." → `SIGABRT`, leaving stale sockets and no supervisor. We reset
//!    `SIGPIPE` to `SIG_DFL` so a broken output pipe terminates the process
//!    cleanly via the default `SIGPIPE` action instead of surfacing as an
//!    `EPIPE` panic. A gone pane is normal end-of-life for a route-owned
//!    supervisor; the controller watchdog restarts it if the session is still
//!    live.
//!
//! 2. **Any other panic double-faulting on a broken stderr.** We install a panic
//!    hook that records the panic to `.agent-doc/logs/panic.log` (best-effort)
//!    and writes to stderr with an error-*ignoring* write, so the hook itself can
//!    never re-panic on a broken stderr. Under `panic = "abort"` a panic still
//!    ends the process, but as a single clean, logged abort rather than an opaque
//!    double-fault.

use std::io::Write;
use std::panic;
use std::path::PathBuf;

/// Install process-wide crash resilience. Call once, as early as possible in
/// `main()` (before any output that could hit a broken pipe).
pub fn install() {
    reset_sigpipe_to_default();
    install_panic_hook();
}

/// Reset the `SIGPIPE` disposition to `SIG_DFL`.
///
/// The Rust runtime sets `SIGPIPE` to `SIG_IGN` at startup, which converts a
/// write to a broken pipe into an `EPIPE` error that `print!` / `eprintln!` then
/// turn into a panic. Restoring the default disposition makes a broken pipe kill
/// the process quietly with `SIGPIPE` (no panic, no abort, no coredump).
#[cfg(unix)]
fn reset_sigpipe_to_default() {
    // SAFETY: `signal(2)` with `SIG_DFL` sets a process-wide disposition to the
    // OS default. We call it once during single-threaded startup, before any
    // threads are spawned, so there is no concurrent signal-state race.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe_to_default() {
    // No SIGPIPE on non-Unix targets; broken-pipe writes surface as ordinary
    // `io::Error`s handled at the write site.
}

fn install_panic_hook() {
    panic::set_hook(Box::new(move |info| {
        let line = panic_line(info);
        // Best-effort file log first — never allowed to panic.
        log_panic_to_file(&line);
        // Then a stderr line with an error-*ignoring* write, so a broken stderr
        // cannot re-panic (the double-fault that produced SIGABRT). We do NOT
        // delegate to the default hook: its internal stderr write panics on a
        // broken pipe, which is exactly the failure we are neutralizing.
        let _ = std::io::stderr().write_all(line.as_bytes());
        let _ = std::io::stderr().write_all(b"\n");
        let _ = std::io::stderr().flush();
    }));
}

/// Format a panic into a stable single line: thread, location, and message.
fn panic_line(info: &panic::PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown>".to_string());
    format!(
        "[agent-doc][panic] thread '{name}' panicked at {location}: {}",
        payload_str(info.payload())
    )
}

fn payload_str(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

fn log_panic_to_file(line: &str) {
    let Some(dir) = resolve_logs_dir() else {
        return;
    };
    log_panic_to_dir(&dir, line);
}

/// Append one panic line to an explicit logs directory.
///
/// `#crashresiliencelograce`: split out so the behavior can be tested without
/// `set_current_dir`. That call mutates PROCESS-global state, so a sibling test
/// running concurrently in the same binary observed this test's temp directory
/// (and then its deletion) — the same class of failure as the git-io
/// `ScopedCurrentDir` flake. A per-test temp path does not fix it; not touching
/// the process cwd does.
fn log_panic_to_dir(dir: &std::path::Path, line: &str) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("panic.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.write_all(b"\n");
    }
}

/// Walk up from the current directory to the nearest `.agent-doc/` and return its
/// `logs` subdirectory. Best-effort: returns `None` outside a project.
fn resolve_logs_dir() -> Option<PathBuf> {
    resolve_logs_dir_from(std::env::current_dir().ok()?)
}

/// Walk up from `start` to the nearest `.agent-doc/`. Pure apart from the
/// `is_dir` probes, so it needs no ambient cwd.
fn resolve_logs_dir_from(start: PathBuf) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(".agent-doc").is_dir() {
            return Some(dir.join(".agent-doc").join("logs"));
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_str_extracts_str_and_string() {
        assert_eq!(payload_str(&"boom"), "boom");
        assert_eq!(payload_str(&String::from("kaboom")), "kaboom");
        assert_eq!(payload_str(&42_i32), "<non-string panic payload>");
    }

    #[test]
    fn resolve_logs_dir_finds_nearest_dot_agent_doc() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        // `#crashresiliencelograce`: resolve from an explicit start instead of
        // mutating the process cwd, which raced sibling tests in this binary.
        let got = resolve_logs_dir_from(nested.clone());
        assert_eq!(
            got,
            Some(
                tmp.path()
                    .canonicalize()
                    .unwrap()
                    .join(".agent-doc")
                    .join("logs")
            )
        );
    }

    #[test]
    fn log_panic_to_file_appends_without_panicking() {
        // `#crashresiliencelograce`: no `set_current_dir` — that mutated
        // process-global state and raced sibling tests under a parallel
        // workspace run. Resolve the directory from an explicit root instead.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let dir = resolve_logs_dir_from(tmp.path().to_path_buf())
            .expect("a .agent-doc ancestor resolves to its logs dir");
        std::fs::create_dir_all(&dir).unwrap();

        log_panic_to_dir(&dir, "[agent-doc][panic] thread 'main' panicked at x.rs:1:1: boom");
        log_panic_to_dir(&dir, "[agent-doc][panic] second line");

        let contents = std::fs::read_to_string(dir.join("panic.log")).unwrap();
        assert!(contents.contains("boom"));
        assert_eq!(contents.lines().count(), 2, "appends, never truncates");
    }
}
