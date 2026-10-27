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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static ISOLATED_TEST_TMUX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Longest label kept in the socket name. A Unix socket path is limited to
/// ~108 bytes and the socket lives under `/tmp/tmux-<uid>/`, so long labels are
/// truncated rather than letting `tmux` fail to bind.
const MAX_LABEL_BYTES: usize = 48;

/// Return a socket name for `tmux -L` that no other test construction, in this
/// process or any concurrent one, can produce.
pub fn unique_isolated_tmux_socket(label: &str) -> String {
    let sequence = ISOLATED_TEST_TMUX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();
    let label: String = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .take(MAX_LABEL_BYTES)
        .collect();
    format!(
        "{label}-{}-{sequence}-{:x}",
        std::process::id(),
        nonce & 0xffff_ffff
    )
}

/// Where tmux puts the socket for `tmux -L <socket>`:
/// `$TMUX_TMPDIR/tmux-<uid>/<socket>`, with `/tmp` when `TMUX_TMPDIR` is unset.
pub fn isolated_tmux_socket_path(socket: &str) -> PathBuf {
    let base = std::env::var_os("TMUX_TMPDIR")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(format!("tmux-{}", current_uid())).join(socket)
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
        // `IsolatedTmux::drop` runs `kill-server`; only then is the socket
        // file unused.
        drop(self.tmux.take());
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Start a test-private tmux server handle. Dropping it kills the server and
/// removes its socket file.
///
/// Use this instead of `tmux_router::IsolatedTmux::new` with a fixed name; see
/// the module docs for the collision this prevents.
pub fn isolated_test_tmux(label: &str) -> IsolatedTestTmux {
    let socket = unique_isolated_tmux_socket(label);
    IsolatedTestTmux {
        socket_path: isolated_tmux_socket_path(&socket),
        tmux: Some(tmux_router::IsolatedTmux::new(&socket)),
    }
}

/// [`isolated_test_tmux`] whose server runs with a clean environment (`HOME`
/// = `home`, a minimal `PATH`) and `default-shell` = `shell`, so a test can
/// reproduce an operator shell (for example zsh with no `~/.zshrc`) without
/// touching the operator's own shell or rc files.
///
/// The server holds a `main` session whose only pane is a long `sleep`, so it
/// outlives the panes a test creates; every later pane starts `shell`.
pub fn isolated_test_tmux_with_default_shell(
    label: &str,
    shell: &Path,
    home: &Path,
) -> IsolatedTestTmux {
    let tmux = isolated_test_tmux(label);
    let status = tmux
        .cmd()
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("TERM", "xterm-256color")
        .env("SHELL", shell)
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
    tmux.raw_cmd(&[
        "set-option",
        "-g",
        "default-shell",
        &shell.to_string_lossy(),
    ])
    .expect("set isolated default-shell");
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
    /// an empty private dir, and `kill-server` still kills A. Then the forms
    /// the `PreToolUse` guard allows (`env -u TMUX` with a private
    /// `TMUX_TMPDIR`, and `-L <private>`) leave server B alive. Every command
    /// names its target explicitly; the operator's real server is never
    /// reachable, because `$TMUX` is set to A's socket or removed on every
    /// spawned client.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_env_beats_tmux_tmpdir_so_only_a_named_socket_isolates() {
        if !tmux_available() {
            eprintln!("tmux unavailable; skipping");
            return;
        }
        let dir = std::env::temp_dir();
        let private = tempfile::tempdir().expect("private TMUX_TMPDIR");
        let client = |tmux_env: Option<&Path>, args: &[&str]| {
            let mut command = std::process::Command::new("tmux");
            command.env("TMUX_TMPDIR", private.path()).args(args);
            match tmux_env {
                Some(socket) => command.env("TMUX", format!("{},1,0", socket.display())),
                None => command.env_remove("TMUX"),
            };
            command.output().expect("spawn tmux client")
        };

        // Control: the incident shape, aimed at stand-in server A.
        let server_a = isolated_test_tmux("tmux-env-precedence-a");
        server_a.new_session("main", &dir).expect("server A starts");
        let listed = client(Some(server_a.socket_path()), &["ls"]);
        assert!(
            String::from_utf8_lossy(&listed.stdout).contains("main:"),
            "with $TMUX set, `tmux ls` lists the $TMUX server despite TMUX_TMPDIR: {listed:?}"
        );
        client(Some(server_a.socket_path()), &["kill-server"]);
        assert!(
            !server_a.session_exists("main"),
            "TMUX_TMPDIR alone must not isolate: kill-server followed $TMUX and killed server A"
        );

        // The guarded forms leave the stand-in server alive.
        let server_b = isolated_test_tmux("tmux-env-precedence-b");
        server_b.new_session("main", &dir).expect("server B starts");
        let unset = client(None, &["kill-server"]);
        assert!(
            !unset.status.success(),
            "with $TMUX removed, kill-server targets the empty private TMUX_TMPDIR: {unset:?}"
        );
        let other = unique_isolated_tmux_socket("tmux-env-precedence-other");
        client(Some(server_b.socket_path()), &["-L", &other, "kill-server"]);
        assert!(
            server_b.session_exists("main"),
            "`env -u TMUX TMUX_TMPDIR=<private>` and `-L <private>` must not reach server B"
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
