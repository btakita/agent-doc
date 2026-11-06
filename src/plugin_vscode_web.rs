//! `#vscodewebextdir`: find Coder `vscode-web` servers from their own argv.
//!
//! The module's `install_prefix` and `extensions_dir` are Terraform variables
//! with no file on the workspace that records them, so the fixed install paths
//! cannot see a template that changes either. The running server can: the
//! module starts `<prefix>/bin/code-server serve-local [--extensions-dir=<dir>]`
//! and the `serve-local` argument survives into both of its processes, the
//! `sh <prefix>/bin/code-server serve-local ...` wrapper and its
//! `<prefix>/node <prefix>/out/server-main.js serve-local ...` child. A
//! Remote-SSH server (`--start-server`) and an extension host
//! (`out/bootstrap-fork`) never match.
//!
//! agent-doc goes on to execute the CLI it finds here, so only processes owned
//! by this user count: another user's argv must not choose a program to run.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::plugin::VSCODE_SERVER_CLIS;

/// The subcommand the module passes, and what separates its server from every
/// other VS Code Server process.
const SERVE_SUBCOMMAND: &str = "serve-local";
const EXTENSIONS_DIR_FLAG: &str = "--extensions-dir";

/// One running `serve-local` server.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VsCodeWebServe {
    /// The module's `install_prefix`.
    pub prefix: PathBuf,
    /// The module's `extensions_dir`; `None` when the server uses its default.
    pub extensions_dir: Option<PathBuf>,
}

/// The install root of a server script, when `script` is an absolute
/// `<prefix>/bin/<cli>` or `<prefix>/out/server-main.js`.
fn server_prefix(script: &Path) -> Option<PathBuf> {
    if !script.is_absolute() {
        return None;
    }
    let is_cli = script.parent()?.ends_with("bin")
        && VSCODE_SERVER_CLIS
            .iter()
            .any(|cli| script.file_name() == Some(std::ffi::OsStr::new(cli)));
    (is_cli || script.ends_with("out/server-main.js"))
        .then(|| script.parent()?.parent().map(Path::to_path_buf))
        .flatten()
}

/// The last `--extensions-dir` value, in either the `=` form the module passes
/// or the two-argument form. VS Code keeps the last of a repeated flag.
fn extensions_dir_arg(args: &[String]) -> Option<PathBuf> {
    let mut dir = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == EXTENSIONS_DIR_FLAG {
            dir = args.next().map(String::as_str);
        } else if let Some(value) = arg
            .strip_prefix(EXTENSIONS_DIR_FLAG)
            .and_then(|rest| rest.strip_prefix('='))
        {
            dir = Some(value);
        }
    }
    dir.filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

/// The server one process's argv runs, or `None` when it is not `serve-local`.
/// The script is `argv[0]` when it ran directly, otherwise the first
/// non-flag argument after the `sh` or `node` interpreter.
pub fn vscode_web_serve_from_argv(argv: &[String]) -> Option<VsCodeWebServe> {
    let first = argv.first()?;
    let script_index = if server_prefix(Path::new(first)).is_some() {
        0
    } else {
        1 + argv[1..].iter().position(|arg| !arg.starts_with('-'))?
    };
    let prefix = server_prefix(Path::new(&argv[script_index]))?;
    let args = &argv[script_index + 1..];
    (args.first()?.as_str() == SERVE_SUBCOMMAND).then(|| VsCodeWebServe {
        prefix,
        extensions_dir: extensions_dir_arg(&args[1..]),
    })
}

/// The server under one `/proc/<pid>`. A relative `--extensions-dir` is
/// relative to that process, so it resolves against its `cwd`. A path that is
/// not UTF-8 is skipped rather than rewritten into a different directory.
#[cfg(unix)]
fn serve_of_process(process: &Path) -> Option<VsCodeWebServe> {
    let cmdline = String::from_utf8(fs::read(process.join("cmdline")).ok()?).ok()?;
    let argv: Vec<String> = cmdline
        .split('\0')
        .filter(|arg| !arg.is_empty())
        .map(str::to_string)
        .collect();
    let mut serve = vscode_web_serve_from_argv(&argv)?;
    if let Some(dir) = serve.extensions_dir.take() {
        serve.extensions_dir = Some(if dir.is_relative() {
            fs::read_link(process.join("cwd")).ok()?.join(dir)
        } else {
            dir
        });
    }
    Some(serve)
}

/// Servers among the processes `uid` owns. A wrapper and its node child report
/// one server, and two pids of one server collapse here.
#[cfg(unix)]
fn serves_owned_by(proc_root: &Path, uid: u32) -> BTreeSet<VsCodeWebServe> {
    use std::os::unix::fs::MetadataExt as _;

    let Ok(entries) = fs::read_dir(proc_root) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .filter(|entry| entry.metadata().is_ok_and(|metadata| metadata.uid() == uid))
        .filter_map(|entry| serve_of_process(&entry.path()))
        .collect()
}

/// This user's running `serve-local` servers, from `proc_root` (`/proc` in
/// production, a fixture tree in tests). A host without one has none, and a
/// process that exits or cannot be read mid-scan is simply absent.
#[cfg(unix)]
pub fn running_vscode_web_serves(proc_root: &Path) -> BTreeSet<VsCodeWebServe> {
    use std::os::unix::fs::MetadataExt as _;

    match fs::metadata(proc_root.join("self")) {
        Ok(metadata) => serves_owned_by(proc_root, metadata.uid()),
        Err(_) => BTreeSet::new(),
    }
}

#[cfg(not(unix))]
pub fn running_vscode_web_serves(_proc_root: &Path) -> BTreeSet<VsCodeWebServe> {
    BTreeSet::new()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt as _;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    fn serve(prefix: &str, extensions_dir: Option<&str>) -> VsCodeWebServe {
        VsCodeWebServe {
            prefix: PathBuf::from(prefix),
            extensions_dir: extensions_dir.map(PathBuf::from),
        }
    }

    fn process(proc_root: &Path, pid: &str, args: &[&str]) -> PathBuf {
        let dir = proc_root.join(pid);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cmdline"), format!("{}\0", args.join("\0"))).unwrap();
        dir
    }

    /// The argv `run.sh` and `code-server-linux.sh` produce at the commits
    /// pinned in editors/SPEC.md section 13, in both processes.
    #[test]
    fn both_processes_of_the_module_server_report_prefix_and_extensions_dir() {
        let wrapper = argv(&[
            "sh",
            "/opt/vw/bin/code-server",
            "serve-local",
            "--extensions-dir=/srv/ext",
            "--port",
            "13338",
            "--host",
            "127.0.0.1",
            "--accept-server-license-terms",
        ]);
        let node = argv(&[
            "/opt/vw/node",
            "/opt/vw/out/server-main.js",
            "serve-local",
            "--extensions-dir=/srv/ext",
            "--port",
            "13338",
        ]);
        let expected = serve("/opt/vw", Some("/srv/ext"));
        assert_eq!(vscode_web_serve_from_argv(&wrapper), Some(expected.clone()));
        assert_eq!(vscode_web_serve_from_argv(&node), Some(expected));
    }

    #[test]
    fn a_custom_prefix_with_no_extensions_dir_keeps_the_server_default() {
        let wrapper = argv(&[
            "/bin/sh",
            "/opt/vw/bin/code-server",
            "serve-local",
            "--port",
            "1",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&wrapper),
            Some(serve("/opt/vw", None))
        );
    }

    #[test]
    fn the_two_argument_flag_and_a_repeat_read_as_vs_code_reads_them() {
        let two_args = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--extensions-dir",
            "/srv/a",
            "--extensions-dir=/srv/b",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&two_args),
            Some(serve("/opt/vw", Some("/srv/b")))
        );
        let after_terminator = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--",
            "--extensions-dir=/srv/a",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&after_terminator),
            Some(serve("/opt/vw", None))
        );
        let no_value = argv(&["/opt/vw/bin/code-server", "serve-local", "--extensions-dir"]);
        assert_eq!(
            vscode_web_serve_from_argv(&no_value),
            Some(serve("/opt/vw", None))
        );
    }

    #[test]
    fn a_node_inspect_flag_does_not_hide_the_script() {
        let node = argv(&[
            "/opt/vw/node",
            "--inspect=9229",
            "/opt/vw/out/server-main.js",
            "serve-local",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&node),
            Some(serve("/opt/vw", None))
        );
    }

    /// Every other process shaped like a VS Code Server must stay out: each of
    /// these would otherwise add a target that is not a `vscode-web` server.
    #[test]
    fn other_server_processes_and_lookalikes_are_not_serve_local() {
        for args in [
            // Remote-SSH: same CLI name, no `serve-local`.
            &[
                "sh",
                "/home/u/.vscode-server/bin/abc/bin/code-server",
                "--start-server",
                "--host=127.0.0.1",
            ][..],
            &[
                "/home/u/.vscode-server/bin/abc/node",
                "/home/u/.vscode-server/bin/abc/out/server-main.js",
                "--start-server",
            ],
            // Extension host of a real vscode-web server.
            &[
                "/opt/vw/node",
                "/opt/vw/out/bootstrap-fork",
                "--type=extensionHost",
            ],
            // code-server proper.
            &[
                "/usr/lib/code-server/lib/node",
                "/usr/lib/code-server",
                "--bind-addr",
                "0:0",
            ],
            // Text naming the script, and `serve-local` not as the subcommand.
            &[
                "vim",
                "/opt/vw/bin/code-server",
                "--extensions-dir=/srv/ext",
            ],
            &["sh", "/opt/vw/bin/code-server", "--port", "serve-local"],
            &["sh", "-c", "/opt/vw/bin/code-server serve-local"],
            // A relative script names no install: the node child carries it.
            &["sh", "./bin/code-server", "serve-local"],
            // A `bin/<name>` that is not a server CLI.
            &["sh", "/opt/vw/bin/other", "serve-local"],
            // Nothing after the interpreter.
            &["sh"],
        ] {
            assert_eq!(vscode_web_serve_from_argv(&argv(args)), None, "{args:?}");
        }
        assert_eq!(vscode_web_serve_from_argv(&[]), None);
    }

    #[test]
    fn a_scan_collapses_one_server_and_keeps_two() {
        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path();
        fs::create_dir(proc_root.join("self")).unwrap();
        process(
            proc_root,
            "10",
            &["sh", "/opt/a/bin/code-server", "serve-local"],
        );
        process(
            proc_root,
            "11",
            &["/opt/a/node", "/opt/a/out/server-main.js", "serve-local"],
        );
        process(
            proc_root,
            "20",
            &[
                "sh",
                "/opt/b/bin/code-server",
                "serve-local",
                "--extensions-dir=/srv/b",
            ],
        );
        process(proc_root, "30", &["bash"]);
        // Not a pid directory, and a pid whose cmdline cannot be read.
        process(
            proc_root,
            "acpi",
            &["sh", "/opt/c/bin/code-server", "serve-local"],
        );
        fs::create_dir(proc_root.join("40")).unwrap();

        assert_eq!(
            running_vscode_web_serves(proc_root)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![serve("/opt/a", None), serve("/opt/b", Some("/srv/b"))]
        );
    }

    /// The argv is attacker-reachable on a shared host: `/proc/<pid>/cmdline`
    /// shows any user's process, and agent-doc executes what it names.
    #[test]
    fn another_users_process_is_never_a_server() {
        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path();
        process(
            proc_root,
            "10",
            &["sh", "/tmp/evil/bin/code-server", "serve-local"],
        );
        let owner = fs::metadata(proc_root.join("10")).unwrap().uid();

        assert_eq!(serves_owned_by(proc_root, owner).len(), 1);
        assert!(serves_owned_by(proc_root, owner + 1).is_empty());
    }

    #[test]
    fn a_relative_extensions_dir_resolves_against_the_servers_cwd() {
        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path();
        let dir = process(
            proc_root,
            "10",
            &[
                "sh",
                "/opt/vw/bin/code-server",
                "serve-local",
                "--extensions-dir=ext",
            ],
        );
        // No `cwd` evidence: installing relative to agent-doc's own cwd would
        // be a wrong directory, so the server is not reported.
        assert_eq!(serve_of_process(&dir), None);
        std::os::unix::fs::symlink("/home/coder/project", dir.join("cwd")).unwrap();
        assert_eq!(
            serve_of_process(&dir),
            Some(serve("/opt/vw", Some("/home/coder/project/ext")))
        );
    }

    #[test]
    fn a_non_utf8_argv_is_skipped_rather_than_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("10");
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("cmdline"),
            b"sh\0/opt/vw/bin/code-server\0serve-local\0--extensions-dir=/srv/\xff\0",
        )
        .unwrap();
        assert_eq!(serve_of_process(&dir), None);
    }

    #[test]
    fn a_host_without_proc_has_no_servers() {
        let root = tempfile::tempdir().unwrap();
        assert!(running_vscode_web_serves(&root.path().join("missing")).is_empty());
    }

    /// The real `/proc`, with a real process in the wrapper's shape: `/bin/sh`
    /// runs the script as `env sh` does, so no fixture stands in for what the
    /// kernel reports. `spawn` returns after exec, so the argv is already set.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_real_proc_reports_a_running_server() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("custom prefix");
        let cli = prefix.join("bin/code-server");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        fs::write(&cli, "read line\n").unwrap();
        let mut child = std::process::Command::new("/bin/sh")
            .arg(&cli)
            .arg("serve-local")
            .arg("--extensions-dir=/srv/ext")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let serves = running_vscode_web_serves(Path::new("/proc"));

        child.kill().unwrap();
        child.wait().unwrap();
        assert!(
            serves.contains(&VsCodeWebServe {
                prefix,
                extensions_dir: Some(PathBuf::from("/srv/ext")),
            }),
            "{serves:?}"
        );
    }
}
