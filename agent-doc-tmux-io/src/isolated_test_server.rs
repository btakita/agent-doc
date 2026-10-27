//! Process-unique isolated tmux servers for tests.
//!
//! `tmux_router::IsolatedTmux::new(name)` runs `tmux -L <name>`, and tmux
//! resolves `-L <name>` to `$TMUX_TMPDIR/tmux-<uid>/<name>` (default
//! `/tmp/tmux-<uid>/<name>`). That directory is shared by every process of the
//! same user, so a fixed name is one server for every concurrent test run:
//! two suites (separate worktrees, or two `cargo test` invocations) that both
//! construct `IsolatedTmux::new("gh218-controller-lifecycle")` attach to the
//! same server, collide on `new-session -s main` ("duplicate session: main"),
//! and the first one to finish kills the server out from under the other
//! ("server exited unexpectedly").
//!
//! Tests construct their server through [`isolated_test_tmux`] instead. The
//! label stays human-readable for diagnostics; the suffix (pid, per-process
//! sequence, and a start-time nonce) makes the socket unique per construction,
//! so concurrent runs can never share a server and a stale server left by a
//! killed run whose pid is later reused is never re-adopted. Session names
//! inside the server (`main`, ...) need no uniqueness because the server
//! itself is private to the test.
//!
//! ## One socket per server (`#isotmuxtmpdir`)
//!
//! A test server and every client of it must agree on ONE socket file. Every
//! client this crate and `tmux_router` spawn names the server with
//! `-L <socket>`, which tmux resolves against the CALLER's `TMUX_TMPDIR`. A
//! server started with a cleared environment (`env_clear()`) used to resolve
//! the same `-L` name against `/tmp` instead, so with `TMUX_TMPDIR` set the
//! server bound `/tmp/tmux-<uid>/<socket>` while its clients looked in
//! `$TMUX_TMPDIR/tmux-<uid>/<socket>`: the first client command failed with
//! `error connecting to ... (No such file or directory)` and teardown, also a
//! `-L` client, never reached the server, which leaked. So:
//!
//! - [`isolated_test_tmux`] resolves the absolute socket path once, from the
//!   test process's environment ([`isolated_tmux_socket_path`]), and records
//!   it on the handle.
//! - A server started from a cleared environment is started with `-S <that
//!   path>` (which tmux prefers over `-L`), and keeps the test process's
//!   `TMUX_TMPDIR` so a tmux client started inside one of its panes resolves
//!   the same socket too.
//! - Teardown kills the server by that exact `-S` path, so a server is never
//!   left behind even when a test panics between starting it and using it.
//! - The socket name is fitted to the Unix socket path limit for the resolved
//!   directory, and a directory too long to hold any socket is a clear panic
//!   instead of tmux's `(File name too long)`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ISOLATED_TEST_TMUX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Longest label kept in the socket name. Labels are shortened further when
/// the socket directory leaves less room (see [`max_socket_path_bytes`]).
const MAX_LABEL_BYTES: usize = 48;

/// Fewest label bytes worth keeping when the socket directory is long; below
/// this the socket keeps only its unique suffix.
const MIN_LABEL_BYTES: usize = 8;

/// Return a socket name for `tmux -L` that no other test construction, in this
/// process or any concurrent one, can produce.
pub fn unique_isolated_tmux_socket(label: &str) -> String {
    format!(
        "{}-{}",
        sanitized_label(label, MAX_LABEL_BYTES),
        unique_socket_suffix()
    )
}

/// The process-unique part of a socket name: pid, per-process sequence, and a
/// start-time nonce.
fn unique_socket_suffix() -> String {
    let sequence = ISOLATED_TEST_TMUX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();
    format!(
        "{}-{sequence}-{:08x}",
        std::process::id(),
        nonce & 0xffff_ffff
    )
}

/// `label` with every byte outside `[A-Za-z0-9_-]` replaced by `-`, at most
/// `max_bytes` long. The result is ASCII, so bytes and chars agree.
fn sanitized_label(label: &str, max_bytes: usize) -> String {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(max_bytes)
        .collect()
}

/// Longest socket path tmux can bind: `sockaddr_un.sun_path` less its NUL
/// terminator (107 on Linux, 103 on macOS and the BSDs).
#[cfg(unix)]
pub fn max_socket_path_bytes() -> usize {
    // SAFETY: `sockaddr_un` is plain old data; all-zero is a valid value.
    let address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_path.len() - 1
}

/// tmux does not run natively on non-unix hosts; any bound keeps the crate
/// building there.
#[cfg(not(unix))]
pub fn max_socket_path_bytes() -> usize {
    107
}

/// A socket name unique like [`unique_isolated_tmux_socket`] whose full path
/// under `dir` fits [`max_socket_path_bytes`]. The label is shortened to fit;
/// a `dir` too long to hold even the unique suffix is an error naming it.
fn fitted_unique_socket(label: &str, dir: &Path) -> Result<String, String> {
    let suffix = unique_socket_suffix();
    let limit = max_socket_path_bytes();
    // `<dir>/<label>-<suffix>`; the bare suffix drops the label and its dash.
    let dir_bytes = dir.as_os_str().len() + 1;
    let room = limit.saturating_sub(dir_bytes);
    if room < suffix.len() {
        return Err(format!(
            "isolated tmux socket directory {} is too long: a socket under it needs \
             {} bytes but a Unix socket path holds at most {limit}; set TMUX_TMPDIR to a \
             shorter directory (or unset it)",
            dir.display(),
            dir_bytes + suffix.len()
        ));
    }
    let label_room = room.saturating_sub(suffix.len() + 1).min(MAX_LABEL_BYTES);
    let label = sanitized_label(label, label_room);
    if label_room < MIN_LABEL_BYTES || label.is_empty() {
        return Ok(suffix);
    }
    Ok(format!("{label}-{suffix}"))
}

/// The directory tmux binds `-L` sockets in for this process:
/// `$TMUX_TMPDIR/tmux-<uid>`, with `/tmp` when `TMUX_TMPDIR` is unset or empty.
pub fn isolated_tmux_socket_dir() -> PathBuf {
    let base = std::env::var_os("TMUX_TMPDIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(format!("tmux-{}", current_uid()))
}

/// Where tmux puts the socket for `tmux -L <socket>`:
/// `$TMUX_TMPDIR/tmux-<uid>/<socket>`, with `/tmp` when `TMUX_TMPDIR` is unset.
pub fn isolated_tmux_socket_path(socket: &str) -> PathBuf {
    isolated_tmux_socket_dir().join(socket)
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// tmux does not run natively on non-unix hosts; the path only has to be
/// well-formed so the crate builds there.
#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

/// A test-private tmux server. Dereferences to [`tmux_router::IsolatedTmux`]
/// (and through it to [`tmux_router::Tmux`]).
///
/// Dropping it kills the server and removes the socket file. tmux leaves the
/// socket file behind after `kill-server`; with a fresh name per construction
/// that would otherwise grow `/tmp/tmux-<uid>/` by one file per test per run.
pub struct IsolatedTestTmux {
    tmux: Option<tmux_router::IsolatedTmux>,
    socket_path: PathBuf,
}

impl IsolatedTestTmux {
    /// The socket file this server binds, removed on drop.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// A tmux command aimed at this server by its exact socket path (`-S`,
    /// which tmux prefers over the `-L` that [`tmux_router::Tmux::cmd`]
    /// adds), with `$TMUX` removed. Use it for a client whose environment
    /// differs from the test process's, such as a cleared one.
    pub fn exact_socket_cmd(&self) -> std::process::Command {
        exact_socket_command(self.binary_path(), &self.socket_path)
    }
}

/// `tmux -S <socket> -f /dev/null` with `$TMUX` removed, run by `binary`.
fn exact_socket_command(binary: &Path, socket: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(binary);
    command
        .env_remove("TMUX")
        .arg("-S")
        .arg(socket)
        .args(["-f", "/dev/null"]);
    command
}

impl std::ops::Deref for IsolatedTestTmux {
    type Target = tmux_router::IsolatedTmux;

    fn deref(&self) -> &Self::Target {
        self.tmux
            .as_ref()
            .expect("isolated test tmux is only taken on drop")
    }
}

impl Drop for IsolatedTestTmux {
    fn drop(&mut self) {
        let Some(tmux) = self.tmux.take() else {
            return;
        };
        let binary = tmux.binary_path().to_path_buf();
        // `IsolatedTmux::drop` runs a `-L` `kill-server`, which resolves the
        // socket from this process's `TMUX_TMPDIR`.
        drop(tmux);
        // Backstop: kill by the exact socket path, so teardown never misses a
        // server whatever directory a `-L` client resolves. Drop runs while
        // unwinding too, so a test that panics after starting its server
        // still leaves none behind. Silent: normally no server is left.
        if self.socket_path.exists() {
            let _ = exact_socket_command(&binary, &self.socket_path)
                .arg("kill-server")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        // Only now is the socket file unused.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Start a test-private tmux server handle. Dropping it kills the server and
/// removes its socket file.
///
/// Use this instead of `tmux_router::IsolatedTmux::new` with a fixed name; see
/// the module docs for the collision this prevents. Panics when the socket
/// directory (from `TMUX_TMPDIR`) is too long to hold a Unix socket path.
pub fn isolated_test_tmux(label: &str) -> IsolatedTestTmux {
    let dir = isolated_tmux_socket_dir();
    let socket = fitted_unique_socket(label, &dir).unwrap_or_else(|error| panic!("{error}"));
    IsolatedTestTmux {
        socket_path: dir.join(&socket),
        tmux: Some(tmux_router::IsolatedTmux::new(&socket)),
    }
}

/// Create the `tmux-<uid>` socket directory the way tmux itself does for
/// `-L` (mode 0700), because a `-S` server does not create it.
fn ensure_socket_dir(socket_path: &Path) {
    let Some(dir) = socket_path.parent() else {
        return;
    };
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => panic!(
            "create isolated tmux socket directory {}: {error}",
            dir.display()
        ),
    }
}

/// [`isolated_test_tmux`] whose server runs with a clean environment (`HOME`
/// = `home`, a minimal `PATH`) and `default-shell` = `shell`, so a test can
/// reproduce an operator shell (for example zsh with no `~/.zshrc`) without
/// touching the operator's own shell or rc files.
///
/// The server holds a `main` session whose only pane is a long `sleep`, so it
/// outlives the panes a test creates; every later pane starts `shell`.
///
/// The cleared environment must not move the server off the socket its
/// clients resolve (`#isotmuxtmpdir`): the server starts with `-S` at the
/// handle's [`IsolatedTestTmux::socket_path`] and keeps the test process's
/// `TMUX_TMPDIR`, so its panes' own tmux clients resolve that socket too.
pub fn isolated_test_tmux_with_default_shell(
    label: &str,
    shell: &Path,
    home: &Path,
) -> IsolatedTestTmux {
    let tmux = isolated_test_tmux(label);
    ensure_socket_dir(tmux.socket_path());
    let mut start = tmux.exact_socket_cmd();
    start
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("SHELL", shell);
    if let Some(tmux_tmpdir) = std::env::var_os("TMUX_TMPDIR").filter(|dir| !dir.is_empty()) {
        start.env("TMUX_TMPDIR", tmux_tmpdir);
    }
    let status = start
        .args([
            "new-session",
            "-d",
            "-s",
            "main",
            "-x",
            "120",
            "-y",
            "30",
            "/bin/sh",
            "-c",
            "exec sleep 600",
        ])
        .status()
        .expect("start isolated tmux server");
    assert!(
        status.success(),
        "isolated tmux new-session failed: {status}"
    );
    // An ordinary `-L` client, as every caller under test uses: it reaching
    // the server proves the server bound the socket the clients resolve.
    tmux.raw_cmd(&[
        "set-option",
        "-g",
        "default-shell",
        &shell.to_string_lossy(),
    ])
    .unwrap_or_else(|error| {
        panic!(
            "set isolated default-shell on {}: {error:#}",
            tmux.socket_path().display()
        )
    });
    tmux
}

/// Write an executable stand-in shell to `dir` whose startup reads one byte
/// from the terminal before it reads any command line, recording that byte in
/// `<dir>/consumed`, then becomes an interactive `sh`.
///
/// It reproduces zsh with no `~/.zshrc`, whose `zsh-newuser-install` menu
/// swallowed the first typed byte, without needing zsh. It shows no prompt
/// until it gets input, so a test can also show that no readiness wait could
/// ever end.
#[cfg(unix)]
pub fn write_terminal_reading_test_shell(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let shell = dir.join("newuser-shell");
    let consumed = dir.join("consumed");
    std::fs::write(
        &shell,
        format!(
            "#!/bin/sh\ndd bs=1 count=1 of='{}' 2>/dev/null\nexec /bin/sh -i\n",
            consumed.display()
        ),
    )
    .expect("write stand-in shell");
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755))
        .expect("chmod stand-in shell");
    shell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_names_are_unique_per_construction_and_keep_the_label() {
        let first = unique_isolated_tmux_socket("gh218-controller-lifecycle");
        let second = unique_isolated_tmux_socket("gh218-controller-lifecycle");
        assert_ne!(first, second);
        let pid = std::process::id().to_string();
        for socket in [&first, &second] {
            assert!(
                socket.starts_with("gh218-controller-lifecycle-"),
                "{socket}"
            );
            assert!(socket.contains(&format!("-{pid}-")), "{socket}");
        }
    }

    #[test]
    fn socket_names_are_bounded_and_path_safe() {
        let socket = unique_isolated_tmux_socket(&format!("a/b c{}", "x".repeat(200)));
        assert!(!socket.contains('/'), "{socket}");
        assert!(!socket.contains(' '), "{socket}");
        assert!(socket.len() <= MAX_LABEL_BYTES + 40, "{socket}");
    }

    /// `#isotmuxtmpdir`: a long `TMUX_TMPDIR` pushed the socket path past the
    /// Unix limit and tmux failed with `(File name too long)`. The socket name
    /// is fitted to the directory: the label shrinks, then goes, and only a
    /// directory too long for the unique suffix alone is refused, by name.
    #[test]
    fn socket_names_fit_the_unix_socket_path_limit_for_their_directory() {
        let limit = max_socket_path_bytes();
        let label = "agent-doc-parallel-fanout-launch";
        let fits = |dir: &Path, socket: &str| dir.join(socket).as_os_str().len() <= limit;

        let short = Path::new("/tmp/tmux-1000");
        let socket = fitted_unique_socket(label, short).expect("short dir");
        assert!(socket.starts_with(&format!("{label}-")), "{socket}");
        assert!(fits(short, &socket), "{socket}");

        // A directory that leaves room for only part of the label keeps a
        // shortened label and still fits.
        let suffix_len = unique_socket_suffix().len();
        let partial_room = MIN_LABEL_BYTES + 4;
        let dir_len = limit - 1 - suffix_len - 1 - partial_room;
        let partial = PathBuf::from(format!("/{}", "d".repeat(dir_len - 1)));
        let socket = fitted_unique_socket(label, &partial).expect("partial room");
        assert!(socket.starts_with(&label[..MIN_LABEL_BYTES]), "{socket}");
        assert!(!socket.starts_with(label), "label is shortened: {socket}");
        assert!(fits(&partial, &socket), "{socket}");

        // No room for a useful label: the unique suffix alone, still fitting.
        let tight = PathBuf::from(format!("/{}", "d".repeat(limit - 1 - suffix_len - 2)));
        let socket = fitted_unique_socket(label, &tight).expect("tight dir");
        assert!(!socket.contains("parallel"), "{socket}");
        assert!(
            socket.starts_with(&format!("{}-", std::process::id())),
            "{socket}"
        );
        assert!(fits(&tight, &socket), "{socket}");
        assert_ne!(
            socket,
            fitted_unique_socket(label, &tight).expect("tight dir"),
            "the bare suffix stays unique per construction"
        );

        // Too long for any socket: a clear refusal instead of tmux's error.
        let too_long = PathBuf::from(format!("/{}", "d".repeat(limit)));
        let error = fitted_unique_socket(label, &too_long).expect_err("dir too long");
        assert!(error.contains("too long"), "{error}");
        assert!(error.contains("TMUX_TMPDIR"), "{error}");
        assert!(error.contains(&too_long.display().to_string()), "{error}");
    }

    #[cfg(unix)]
    fn process_alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks that `pid` exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(unix)]
    fn wait_for_exit(pid: i32) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while process_alive(pid) {
            if std::time::Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

    /// `#isotmuxtmpdir`: `isolated_test_tmux_with_default_shell` starts its
    /// server from a cleared environment. With `TMUX_TMPDIR` set, that server
    /// used to bind under `/tmp` while its `-L` clients looked under
    /// `$TMUX_TMPDIR`; the first client command failed and teardown leaked
    /// the server. Run under `make tmux-ci`, which runs it both with and
    /// without `TMUX_TMPDIR`.
    ///
    /// The control reproduces the mismatch with a private `TMUX_TMPDIR` for
    /// the server only. Then the fixed helper: a `-L` client reaches the
    /// server at the recorded socket, the server keeps the test process's
    /// `TMUX_TMPDIR` for its panes, and dropping the handle, including while
    /// unwinding from a panic, kills that exact server and removes its socket.
    #[cfg(unix)]
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn default_shell_server_binds_the_socket_its_clients_resolve() {
        if !tmux_available() {
            eprintln!("tmux unavailable; skipping");
            return;
        }
        let home = tempfile::tempdir().expect("home");
        let shell = Path::new("/bin/sh");

        // Control: a server whose environment resolves `-L` to another
        // directory is unreachable by the test process's `-L` clients.
        let elsewhere = tempfile::tempdir().expect("server-only TMUX_TMPDIR");
        let control = isolated_test_tmux("isotmuxtmpdir-control");
        let status = control
            .cmd()
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", elsewhere.path())
            .args([
                "new-session",
                "-d",
                "-s",
                "main",
                "/bin/sh",
                "-c",
                "exec sleep 600",
            ])
            .status()
            .expect("start control server");
        assert!(status.success(), "control server starts: {status}");
        let socket_name = control
            .socket_path()
            .file_name()
            .expect("socket name")
            .to_owned();
        let misplaced = elsewhere
            .path()
            .join(format!("tmux-{}", current_uid()))
            .join(&socket_name);
        let unreachable = control
            .raw_cmd(&["display-message", "-p", "#{pid}"])
            .expect_err("a -L client resolves its own TMUX_TMPDIR, not the server's");
        assert!(
            format!("{unreachable:#}").contains("error connecting"),
            "{unreachable:#}"
        );
        let killed = std::process::Command::new(control.binary_path())
            .env_remove("TMUX")
            .arg("-S")
            .arg(&misplaced)
            .arg("kill-server")
            .status()
            .expect("kill control server by its exact socket");
        assert!(killed.success(), "control server found at {misplaced:?}");
        let _ = std::fs::remove_file(&misplaced);
        drop(control);

        // Fix: one socket for the server and every client.
        let server =
            isolated_test_tmux_with_default_shell("isotmuxtmpdir-fixed", shell, home.path());
        let reported = server
            .raw_cmd(&["display-message", "-p", "#{socket_path}"])
            .expect("a -L client reaches the server");
        assert_eq!(Path::new(reported.trim()), server.socket_path());
        let server_tmpdir = server.raw_cmd(&["show-environment", "-g", "TMUX_TMPDIR"]);
        match std::env::var("TMUX_TMPDIR")
            .ok()
            .filter(|dir| !dir.is_empty())
        {
            Some(dir) => assert_eq!(
                server_tmpdir.expect("server keeps TMUX_TMPDIR").trim(),
                format!("TMUX_TMPDIR={dir}"),
                "panes inherit the TMUX_TMPDIR their -L clients need"
            ),
            None => assert!(
                server_tmpdir.is_err(),
                "no TMUX_TMPDIR to keep: {server_tmpdir:?}"
            ),
        }
        let pid: i32 = server
            .raw_cmd(&["display-message", "-p", "#{pid}"])
            .expect("server pid")
            .trim()
            .parse()
            .expect("numeric pid");
        let socket = server.socket_path().to_path_buf();
        drop(server);
        assert!(wait_for_exit(pid), "dropping the handle kills server {pid}");
        assert!(!socket.exists(), "dropping the handle removes {socket:?}");

        // A test that panics right after starting the server leaks nothing.
        let started = std::sync::Mutex::new(None);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let server =
                isolated_test_tmux_with_default_shell("isotmuxtmpdir-panic", shell, home.path());
            let pid: i32 = server
                .raw_cmd(&["display-message", "-p", "#{pid}"])
                .expect("server pid")
                .trim()
                .parse()
                .expect("numeric pid");
            *started.lock().unwrap() = Some((pid, server.socket_path().to_path_buf()));
            panic!("test body panics while the server runs");
        }));
        assert!(unwound.is_err());
        let (pid, socket) = started.lock().unwrap().take().expect("server started");
        assert!(wait_for_exit(pid), "unwinding kills server {pid}");
        assert!(!socket.exists(), "unwinding removes {socket:?}");
    }

    /// A fixed name passed to `IsolatedTmux::new` is one shared server for
    /// every concurrent run, and a hand-rolled unique name leaks its socket
    /// file; tests construct servers only through `isolated_test_tmux`.
    #[test]
    fn workspace_builds_isolated_tmux_servers_only_through_the_helper() {
        fn visit(dir: &std::path::Path, offenders: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if path.is_dir() {
                    if !name.starts_with('.')
                        && name != "target"
                        && name != "tmp"
                        && name != "node_modules"
                    {
                        visit(&path, offenders);
                    }
                    continue;
                }
                if !name.ends_with(".rs") || path.ends_with(file!()) {
                    continue;
                }
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for (line_number, line) in source.lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with("//") {
                        continue;
                    }
                    if code.contains(concat!("IsolatedTmux::new", "(")) {
                        offenders.push(format!("{}:{}", path.display(), line_number + 1));
                    }
                }
            }
        }
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let mut offenders = Vec::new();
        visit(workspace, &mut offenders);
        assert!(
            offenders.is_empty(),
            "build isolated tmux servers with agent_doc_tmux_io::isolated_test_tmux \
             (fixed names collide across concurrent test runs): {offenders:#?}"
        );
    }

    fn tmux_available() -> bool {
        std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// `#tmuxkittyterminal`: tmux picks its server from `-L`/`-S`, then `$TMUX`,
    /// and only then `$TMUX_TMPDIR/tmux-<uid>/default`. A harness running in
    /// the operator's tmux inherits `$TMUX`, so `TMUX_TMPDIR=<dir> tmux
    /// kill-server` reaches the operator's server. On 2026-10-26 a subagent
    /// killed the operator's server, every agent pane, and kitty that way.
    ///
    /// Server A stands in for the operator's live server. The control
    /// reproduces the incident against A: `$TMUX` names A, `TMUX_TMPDIR` names
    /// an empty private dir, and `kill-server` still kills A. Then the required
    /// form, `env -u TMUX tmux -L <private>`, leaves server B alive, and so does
    /// `-L <private>` with `$TMUX` pointing at B. No client inherits `$TMUX`.
    /// Each one removes it or sets it to a stand-in socket that is asserted
    /// not to be the operator's.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_env_beats_tmux_tmpdir_so_only_a_named_socket_isolates() {
        if !tmux_available() {
            eprintln!("tmux unavailable; skipping");
            return;
        }
        let dir = std::env::temp_dir();
        let private = tempfile::tempdir().expect("private TMUX_TMPDIR");
        // The operator's sockets: the server this test may be running inside,
        // and the default socket. No client below may name either one.
        let operator_sockets: Vec<PathBuf> = std::env::var("TMUX")
            .ok()
            .and_then(|tmux| tmux.split(',').next().map(PathBuf::from))
            .into_iter()
            .chain([isolated_tmux_socket_path("default")])
            .collect();
        // No client inherits `$TMUX`. A `StandIn` client sets it to a stand-in
        // server this test owns. That is only for proving precedence, and it is
        // asserted never to be an operator socket. A `Private` client is the
        // form agents must use: `env -u TMUX` AND `-L <unique>`.
        enum Client<'a> {
            StandIn(&'a Path),
            Private(&'a str),
        }
        let run = |client: Client<'_>, args: &[&str]| {
            let mut command = std::process::Command::new("tmux");
            command.env("TMUX_TMPDIR", private.path());
            match client {
                Client::StandIn(socket) => {
                    assert!(
                        !operator_sockets.iter().any(|live| live == socket),
                        "refusing to aim a client at an operator socket: {}",
                        socket.display()
                    );
                    command.env("TMUX", format!("{},1,0", socket.display()));
                }
                Client::Private(label) => {
                    command.env_remove("TMUX").args(["-L", label]);
                }
            }
            command.args(args).output().expect("spawn tmux client")
        };

        // Control: the incident shape (`$TMUX` set, a private `TMUX_TMPDIR`,
        // no socket flag), aimed at stand-in server A instead of the operator.
        let server_a = isolated_test_tmux("tmux-env-precedence-a");
        server_a.new_session("main", &dir).expect("server A starts");
        let listed = run(Client::StandIn(server_a.socket_path()), &["ls"]);
        assert!(
            String::from_utf8_lossy(&listed.stdout).contains("main:"),
            "with $TMUX set, `tmux ls` lists the $TMUX server despite TMUX_TMPDIR: {listed:?}"
        );
        run(Client::StandIn(server_a.socket_path()), &["kill-server"]);
        assert!(
            !server_a.session_exists("main"),
            "TMUX_TMPDIR alone must not isolate: kill-server followed $TMUX and killed server A"
        );

        // The required form, `env -u TMUX tmux -L <unique> kill-server`,
        // leaves stand-in server B alive.
        let server_b = isolated_test_tmux("tmux-env-precedence-b");
        server_b.new_session("main", &dir).expect("server B starts");
        let other = unique_isolated_tmux_socket("tmux-env-precedence-other");
        let private_kill = run(Client::Private(&other), &["kill-server"]);
        assert!(
            !private_kill.status.success(),
            "the private socket has no server to kill: {private_kill:?}"
        );
        assert!(
            server_b.session_exists("main"),
            "`env -u TMUX tmux -L <private>` must not reach server B"
        );
        // `-L` alone already beats an inherited `$TMUX`; the harness helpers
        // rely on it.
        run(
            Client::StandIn(server_b.socket_path()),
            &["-L", &other, "kill-server"],
        );
        assert!(
            server_b.session_exists("main"),
            "`-L <private>` must win over `$TMUX`"
        );
    }

    /// Two concurrent runs of one test (same label, same session name) must get
    /// separate servers: both create `main`, and tearing one down leaves the
    /// other alive. The raw fixed-name constructor is exercised first as the
    /// control: it reproduces `duplicate session: main` and the shared-server
    /// teardown, proving this test can see the collision it guards against.
    /// Every server here is `-L`-isolated; the default server is never touched.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn concurrent_runs_of_one_test_get_separate_tmux_servers() {
        if !tmux_available() {
            eprintln!("tmux unavailable; skipping");
            return;
        }
        let dir = std::env::temp_dir();

        // Control: the pre-fix shape — two runs sharing one fixed `-L` name.
        let shared = unique_isolated_tmux_socket("concurrent-control");
        {
            let run_a = tmux_router::IsolatedTmux::new(&shared);
            let run_b = tmux_router::IsolatedTmux::new(&shared);
            run_a
                .new_session("main", &dir)
                .expect("first run creates main");
            let collision = run_b
                .new_session("main", &dir)
                .expect_err("a shared fixed-name server must collide on `main`");
            assert!(
                format!("{collision:#}").contains("duplicate session"),
                "unexpected collision error: {collision:#}"
            );
            drop(run_a);
            assert!(
                !run_b.session_exists("main"),
                "dropping one run kills the server the other run shares"
            );
        }
        // The raw handles kill the server but leave its socket file.
        let _ = std::fs::remove_file(isolated_tmux_socket_path(&shared));

        // Fix: same label, separate servers.
        let run_a = isolated_test_tmux("concurrent-runs");
        let run_b = isolated_test_tmux("concurrent-runs");
        assert_ne!(run_a.server_socket, run_b.server_socket);
        run_a.new_session("main", &dir).expect("run a creates main");
        run_b.new_session("main", &dir).expect("run b creates main");
        let reported = run_a
            .raw_cmd(&["display-message", "-p", "#{socket_path}"])
            .expect("query socket path");
        assert_eq!(Path::new(reported.trim()), run_a.socket_path());
        let socket_a = run_a.socket_path().to_path_buf();
        assert!(socket_a.exists());
        drop(run_a);
        assert!(
            !socket_a.exists(),
            "dropping a test server removes its socket file"
        );
        assert!(
            run_b.session_exists("main"),
            "tearing down one run must not kill the other run's server"
        );
    }
}
