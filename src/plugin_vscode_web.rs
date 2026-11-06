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
//! The extensions directory is the one `server.main.ts` loads
//! (`#vscodewebdatadir`): `--extensions-dir` when it is set, otherwise
//! `<--server-data-dir>/extensions`, otherwise this process's own
//! `$VSCODE_AGENT_FOLDER/extensions`. Anything else is the CLI default
//! (`~/<serverDataFolderName>/extensions`), which the installing shell
//! resolves the same way, so it is left unset. The module never passes
//! `--server-data-dir`; that flag and a server-only `VSCODE_AGENT_FOLDER`
//! are a hand-started server or an environment the installing shell does
//! not share. The shell's own `VSCODE_AGENT_FOLDER` is not consulted here.
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
const SERVER_DATA_DIR_FLAG: &str = "--server-data-dir";
/// What `server.main.ts` reads. The installing shell's copy is not the server's.
#[cfg(unix)]
const AGENT_FOLDER_ENV: &str = "VSCODE_AGENT_FOLDER";

/// One running `serve-local` server.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct VsCodeWebServe {
    /// The module's `install_prefix`.
    pub prefix: PathBuf,
    /// The directory this server loads extensions from. `None` when that is
    /// the CLI default, because the server set neither `--extensions-dir`,
    /// `--server-data-dir`, nor its own `VSCODE_AGENT_FOLDER`.
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

/// The last value of `flag`, in either the `=` form or the two-argument form.
/// VS Code keeps the last of a repeated string flag and then ignores an empty
/// value (`args[flag] || …`). Arguments after `--` are not flags.
fn last_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut value = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == flag {
            value = args.next().map(String::as_str);
        } else if let Some(parsed) = arg
            .strip_prefix(flag)
            .and_then(|rest| rest.strip_prefix('='))
        {
            value = Some(parsed);
        }
    }
    value.filter(|value| !value.is_empty())
}

/// The extensions directory named on `args`, before the process environment.
/// `--extensions-dir` wins. Otherwise `--server-data-dir` is the parent of
/// `extensions`, which is how `server.main.ts` joins it. `None` means neither
/// flag named a directory.
fn extensions_dir_arg(args: &[String]) -> Option<PathBuf> {
    if let Some(dir) = last_flag_value(args, EXTENSIONS_DIR_FLAG) {
        return Some(PathBuf::from(dir));
    }
    last_flag_value(args, SERVER_DATA_DIR_FLAG).map(|dir| PathBuf::from(dir).join("extensions"))
}

/// `VSCODE_AGENT_FOLDER` from one `environ` blob.
#[cfg(unix)]
enum AgentFolder {
    /// Absent or empty. JavaScript treats an empty value as unset.
    Unset,
    /// The first entry, which is the one `getenv` returns.
    Set(PathBuf),
    /// The value is not UTF-8, so the server must be skipped rather than
    /// rewritten into a different directory.
    NotUtf8,
}

/// The first `VSCODE_AGENT_FOLDER` in `environ`.
#[cfg(unix)]
fn agent_folder(environ: &[u8]) -> AgentFolder {
    for entry in environ.split(|byte| *byte == 0) {
        let Some(split) = entry.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let (key, value) = (&entry[..split], &entry[split + 1..]);
        if key != AGENT_FOLDER_ENV.as_bytes() {
            continue;
        }
        if value.is_empty() {
            return AgentFolder::Unset;
        }
        return match std::str::from_utf8(value) {
            Ok(value) => AgentFolder::Set(PathBuf::from(value)),
            Err(_) => AgentFolder::NotUtf8,
        };
    }
    AgentFolder::Unset
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

/// The server under one `/proc/<pid>`. A relative extensions directory,
/// named with `--extensions-dir` or derived from `--server-data-dir` or
/// `VSCODE_AGENT_FOLDER`, is relative to that process, so it resolves against
/// its `cwd`. A path that is not UTF-8 is skipped rather than rewritten into
/// a different directory. An unreadable `environ` leaves a server that named
/// no directory on its argv at the CLI default.
#[cfg(unix)]
fn serve_of_process(process: &Path) -> Option<VsCodeWebServe> {
    let cmdline = String::from_utf8(fs::read(process.join("cmdline")).ok()?).ok()?;
    let argv: Vec<String> = cmdline
        .split('\0')
        .filter(|arg| !arg.is_empty())
        .map(str::to_string)
        .collect();
    let mut serve = vscode_web_serve_from_argv(&argv)?;
    if serve.extensions_dir.is_none() {
        let environ = fs::read(process.join("environ")).unwrap_or_default();
        match agent_folder(&environ) {
            AgentFolder::Set(folder) => serve.extensions_dir = Some(folder.join("extensions")),
            AgentFolder::Unset => {}
            AgentFolder::NotUtf8 => return None,
        }
    }
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

    /// `#vscodewebdatadir`: `server.main.ts` uses `--extensions-dir` or else
    /// `<--server-data-dir>/extensions`. The module never passes the data-dir
    /// flag; a hand-started server does. An empty value is ignored, and the
    /// last of a repeat wins, matching `parseArgs`.
    #[test]
    fn server_data_dir_is_the_extensions_parent_unless_extensions_dir_is_set() {
        let data_dir = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--server-data-dir=/srv/data",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&data_dir),
            Some(serve("/opt/vw", Some("/srv/data/extensions")))
        );
        let extensions_wins = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--server-data-dir=/srv/data",
            "--extensions-dir=/srv/ext",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&extensions_wins),
            Some(serve("/opt/vw", Some("/srv/ext")))
        );
        let data_dir_wins_when_extensions_is_empty = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--extensions-dir=",
            "--server-data-dir=/srv/data",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&data_dir_wins_when_extensions_is_empty),
            Some(serve("/opt/vw", Some("/srv/data/extensions")))
        );
        let last = argv(&[
            "/opt/vw/bin/code-server",
            "serve-local",
            "--server-data-dir",
            "/srv/a",
            "--server-data-dir=/srv/b",
        ]);
        assert_eq!(
            vscode_web_serve_from_argv(&last),
            Some(serve("/opt/vw", Some("/srv/b/extensions")))
        );
        for args in [
            &[
                "/opt/vw/bin/code-server",
                "serve-local",
                "--server-data-dir",
            ][..],
            &[
                "/opt/vw/bin/code-server",
                "serve-local",
                "--server-data-dir=",
            ],
            &[
                "/opt/vw/bin/code-server",
                "serve-local",
                "--",
                "--server-data-dir=/srv/a",
            ],
        ] {
            assert_eq!(
                vscode_web_serve_from_argv(&argv(args)),
                Some(serve("/opt/vw", None)),
                "{args:?}"
            );
        }
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

    fn write_environ(dir: &Path, entries: &[(&[u8], &[u8])]) {
        let mut bytes = Vec::new();
        for (key, value) in entries {
            bytes.extend_from_slice(key);
            bytes.push(b'=');
            bytes.extend_from_slice(value);
            bytes.push(0);
        }
        fs::write(dir.join("environ"), bytes).unwrap();
    }

    /// `#vscodewebdatadir`: the server's own `VSCODE_AGENT_FOLDER`, not the
    /// installing shell's. `--server-data-dir` and `--extensions-dir` still
    /// win, an empty value is unset, and the first entry is the one `getenv`
    /// returns.
    #[test]
    fn the_servers_own_agent_folder_sets_the_extensions_directory() {
        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path();
        let plain = process(
            proc_root,
            "10",
            &["sh", "/opt/vw/bin/code-server", "serve-local"],
        );
        // No environ file: the CLI default stays unset.
        assert_eq!(serve_of_process(&plain), Some(serve("/opt/vw", None)));
        write_environ(
            &plain,
            &[
                (b"OTHER", b"/not/this"),
                (b"VSCODE_AGENT_FOLDER", b"/srv/agent"),
                (b"VSCODE_AGENT_FOLDER", b"/srv/ignored"),
            ],
        );
        assert_eq!(
            serve_of_process(&plain),
            Some(serve("/opt/vw", Some("/srv/agent/extensions")))
        );
        // Same environment as the wrapper above: the scan must collapse them.
        let node = process(
            proc_root,
            "13",
            &["/opt/vw/node", "/opt/vw/out/server-main.js", "serve-local"],
        );
        write_environ(&node, &[(b"VSCODE_AGENT_FOLDER", b"/srv/agent")]);

        write_environ(&plain, &[(b"VSCODE_AGENT_FOLDER", b"")]);
        assert_eq!(serve_of_process(&plain), Some(serve("/opt/vw", None)));

        let data_dir = process(
            proc_root,
            "11",
            &[
                "sh",
                "/opt/vw/bin/code-server",
                "serve-local",
                "--server-data-dir=/srv/data",
            ],
        );
        write_environ(&data_dir, &[(b"VSCODE_AGENT_FOLDER", b"/srv/agent")]);
        assert_eq!(
            serve_of_process(&data_dir),
            Some(serve("/opt/vw", Some("/srv/data/extensions")))
        );

        let extensions = process(
            proc_root,
            "12",
            &[
                "sh",
                "/opt/vw/bin/code-server",
                "serve-local",
                "--extensions-dir=/srv/ext",
            ],
        );
        write_environ(&extensions, &[(b"VSCODE_AGENT_FOLDER", b"/srv/agent")]);
        assert_eq!(
            serve_of_process(&extensions),
            Some(serve("/opt/vw", Some("/srv/ext")))
        );

        // A non-UTF-8 value must not be rewritten into another directory.
        // A non-UTF-8 *other* variable is irrelevant.
        write_environ(&plain, &[(b"VSCODE_AGENT_FOLDER", b"/srv/\xff")]);
        assert_eq!(serve_of_process(&plain), None);
        write_environ(
            &plain,
            &[(b"OTHER", b"\xff"), (b"VSCODE_AGENT_FOLDER", b"/srv/agent")],
        );
        assert_eq!(
            serve_of_process(&plain),
            Some(serve("/opt/vw", Some("/srv/agent/extensions")))
        );
        fs::create_dir(proc_root.join("self")).unwrap();
        // One entry for the shared agent folder: wrapper and node child collapsed.
        assert_eq!(
            running_vscode_web_serves(proc_root)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![
                serve("/opt/vw", Some("/srv/agent/extensions")),
                serve("/opt/vw", Some("/srv/data/extensions")),
                serve("/opt/vw", Some("/srv/ext")),
            ]
        );
    }

    #[test]
    fn a_relative_data_dir_or_agent_folder_resolves_against_the_servers_cwd() {
        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path();
        let data_dir = process(
            proc_root,
            "10",
            &[
                "sh",
                "/opt/vw/bin/code-server",
                "serve-local",
                "--server-data-dir=data",
            ],
        );
        // No `cwd`: a relative directory would land in agent-doc's own cwd.
        assert_eq!(serve_of_process(&data_dir), None);
        std::os::unix::fs::symlink("/home/coder/project", data_dir.join("cwd")).unwrap();
        assert_eq!(
            serve_of_process(&data_dir),
            Some(serve(
                "/opt/vw",
                Some("/home/coder/project/data/extensions")
            ))
        );

        let agent = process(
            proc_root,
            "11",
            &["sh", "/opt/vw/bin/code-server", "serve-local"],
        );
        write_environ(&agent, &[(b"VSCODE_AGENT_FOLDER", b"agent")]);
        assert_eq!(serve_of_process(&agent), None);
        std::os::unix::fs::symlink("/home/coder/project", agent.join("cwd")).unwrap();
        assert_eq!(
            serve_of_process(&agent),
            Some(serve(
                "/opt/vw",
                Some("/home/coder/project/agent/extensions")
            ))
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

    /// The same real `/proc` shape, for the two inputs that are not on the
    /// module's argv: `--server-data-dir` wins over the process environment,
    /// and `VSCODE_AGENT_FOLDER` is that process's value.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_real_proc_reports_server_data_dir_and_the_servers_agent_folder() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("custom prefix");
        let cli = prefix.join("bin/code-server");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        fs::write(&cli, "read line\n").unwrap();
        let spawn = |data_dir: Option<&str>, agent: &str| {
            let mut command = std::process::Command::new("/bin/sh");
            command
                .arg(&cli)
                .arg("serve-local")
                .env("VSCODE_AGENT_FOLDER", agent)
                .stdin(std::process::Stdio::piped());
            if let Some(data_dir) = data_dir {
                command.arg(format!("--server-data-dir={data_dir}"));
            }
            command.spawn().unwrap()
        };
        let mut with_data_dir = spawn(Some("/srv/data"), "/srv/ignored");
        let mut with_agent = spawn(None, "/srv/agent");

        let serves = running_vscode_web_serves(Path::new("/proc"));

        with_data_dir.kill().unwrap();
        with_agent.kill().unwrap();
        with_data_dir.wait().unwrap();
        with_agent.wait().unwrap();
        assert!(
            serves.contains(&VsCodeWebServe {
                prefix: prefix.clone(),
                extensions_dir: Some(PathBuf::from("/srv/data/extensions")),
            }),
            "{serves:?}"
        );
        assert!(
            serves.contains(&VsCodeWebServe {
                prefix,
                extensions_dir: Some(PathBuf::from("/srv/agent/extensions")),
            }),
            "{serves:?}"
        );
    }
}
