//! # Module: supervisor::spawn_program
//!
//! Resolves the program a supervisor hands to the pty spawn (GH #380).
//!
//! ## Spec
//! See `src/agent-doc/specs/supervisor.md` § Env determinism / Harness program
//! resolution.
//!
//! The supervisor clears the ambient environment and spawns the harness with
//! exactly the resolved child env, so `portable-pty` looks a bare program name
//! (`claude`, `codex`, `opencode`, `grok`) up only on the child's `PATH`. A
//! route-owned supervisor inherits that `PATH` from the project controller, and
//! some hosts start the controller with a system-default `PATH` that leaves out
//! the user-local directory the harness installer wrote to — the Coder
//! workspace default omits `~/.local/bin`, where the Claude Code installer puts
//! `claude`. The spawn then fails with `No viable candidates found in PATH`
//! even though the harness is installed.
//!
//! [`resolve_spawn_program`] runs before the spawn:
//!
//! 1. A program that already names a path (contains a separator) is used as
//!    given; `portable-pty` reports its own error when it is missing.
//! 2. A bare name that resolves on the child `PATH` is used as given, so the
//!    common case spawns byte-identically to before (same `argv[0]`, same
//!    lookup, same precedence).
//! 3. Otherwise the well-known harness install directories under the child
//!    `HOME` (falling back to the supervisor's own `HOME`) and a few system
//!    prefixes are searched in order. The first executable hit is spawned by
//!    its absolute path, and its directory is appended to the child `PATH` so
//!    the harness's own subprocesses (hooks, helper binaries installed beside
//!    it) find the same install location. Appending keeps every existing
//!    `PATH` entry's precedence.
//! 4. When nothing is found the spawn fails before `portable-pty` with an
//!    error that names the program, the child `PATH` entries searched, and the
//!    fallback directories searched, plus the fix (install it, or put its
//!    directory on `PATH` / the document `env:` `PATH`).
//!
//! ## Agentic Contracts
//!
//! - **Pure over its inputs.** Resolution reads only the child env map, the
//!   supplied fallback `HOME`, the cwd (for relative `PATH` entries, mirroring
//!   `portable-pty`), and the filesystem. It never reads the parent `PATH`, so
//!   the deterministic-env invariant still holds: the parent `PATH` does not
//!   leak into the child.
//! - **No canonicalization.** A fallback hit is spawned through the path it
//!   was found at, not its symlink target, so a versioned install
//!   (`~/.local/bin/claude -> ~/.local/share/claude/versions/<v>`) keeps the
//!   `claude` process name that tmux `pane_current_command` reports.
//! - **Non-Unix passes through.** Windows lookup (`PATHEXT`, `%LOCALAPPDATA%`)
//!   stays with `portable-pty`; the fallback directories are Unix layouts.
//!
//! ## Evals
//!
//! - bare_name_on_child_path_is_used_as_given
//! - bare_name_missing_from_path_resolves_user_local_bin
//! - fallback_uses_parent_home_when_child_env_has_none
//! - child_path_entries_take_precedence_over_fallback_dirs
//! - non_executable_candidate_is_skipped
//! - explicit_path_program_is_used_as_given
//! - not_found_error_lists_searched_directories
//! - append_path_dir_preserves_existing_entries

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where the program handed to the pty spawn came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnProgramSource {
    /// The program was used as given: it names a path, or it resolves on the
    /// child `PATH` (or the platform defers lookup to `portable-pty`).
    AsGiven,
    /// The bare name was missing from the child `PATH` and was found in a
    /// well-known harness install directory.
    InstallDirFallback,
}

impl SpawnProgramSource {
    /// Stable tag for supervisor log lines.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AsGiven => "as_given",
            Self::InstallDirFallback => "install_dir_fallback",
        }
    }
}

/// The resolved spawn target for one child launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSpawnProgram {
    /// The program string to hand to `CommandBuilder::new`.
    pub program: String,
    pub source: SpawnProgramSource,
    /// The directory appended to the child `PATH`, set only for
    /// [`SpawnProgramSource::InstallDirFallback`].
    pub path_dir_appended: Option<PathBuf>,
}

/// User-local install directories (relative to `HOME`) searched after the
/// child `PATH`, in order. Covers the native Claude Code installer
/// (`~/.local/bin`), its legacy local install (`~/.claude/local`), the
/// OpenCode and Grok Build installers, and the common npm/bun/volta/cargo
/// global prefixes the harnesses are also installed through.
pub const HOME_RELATIVE_INSTALL_DIRS: &[&str] = &[
    ".local/bin",
    ".claude/local",
    ".opencode/bin",
    ".grok/bin",
    ".npm-global/bin",
    ".bun/bin",
    ".volta/bin",
    ".cargo/bin",
    "bin",
];

/// System prefixes searched after the user-local directories.
pub const SYSTEM_INSTALL_DIRS: &[&str] = &[
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/home/linuxbrew/.linuxbrew/bin",
];

/// Resolve the program for a pty spawn. See the module docs for the order.
///
/// `fallback_home` is consulted only when the child env carries no `HOME`;
/// callers pass the supervisor's own `HOME`.
pub fn resolve_spawn_program(
    program: &str,
    env: &HashMap<String, String>,
    cwd: &Path,
    fallback_home: Option<&str>,
) -> anyhow::Result<ResolvedSpawnProgram> {
    let as_given = || ResolvedSpawnProgram {
        program: program.to_string(),
        source: SpawnProgramSource::AsGiven,
        path_dir_appended: None,
    };
    if !cfg!(unix) || program.is_empty() || program.contains('/') {
        return Ok(as_given());
    }

    let path_dirs: Vec<PathBuf> = env
        .get("PATH")
        .map(|raw| std::env::split_paths(&OsString::from(raw)).collect())
        .unwrap_or_default();
    if path_dirs
        .iter()
        .any(|dir| is_executable_file(&cwd.join(dir).join(program)))
    {
        return Ok(as_given());
    }

    let fallback_dirs = install_fallback_dirs(env, fallback_home);
    for dir in &fallback_dirs {
        let candidate = dir.join(program);
        if is_executable_file(&candidate) {
            return Ok(ResolvedSpawnProgram {
                program: candidate.to_string_lossy().into_owned(),
                source: SpawnProgramSource::InstallDirFallback,
                path_dir_appended: Some(dir.clone()),
            });
        }
    }

    let render = |dirs: &[PathBuf]| -> String {
        if dirs.is_empty() {
            "<none>".to_string()
        } else {
            dirs.iter()
                .map(|dir| dir.display().to_string())
                .collect::<Vec<_>>()
                .join(":")
        }
    };
    let path_note = if env.contains_key("PATH") {
        render(&path_dirs)
    } else {
        "<PATH unset in child env>".to_string()
    };
    anyhow::bail!(
        "harness program `{program}` was not found: searched child PATH {path_note} \
         and install directories {}; install `{program}`, or add the directory that \
         contains it to the supervisor's PATH or to the document `env:` PATH",
        render(&fallback_dirs)
    )
}

/// The ordered fallback directories for `env`'s `HOME` (or `fallback_home`).
pub fn install_fallback_dirs(
    env: &HashMap<String, String>,
    fallback_home: Option<&str>,
) -> Vec<PathBuf> {
    let home = env
        .get("HOME")
        .map(String::as_str)
        .or(fallback_home)
        .filter(|home| !home.is_empty());
    let mut dirs = Vec::new();
    if let Some(home) = home {
        let home = Path::new(home);
        dirs.extend(HOME_RELATIVE_INSTALL_DIRS.iter().map(|rel| home.join(rel)));
    }
    dirs.extend(SYSTEM_INSTALL_DIRS.iter().map(PathBuf::from));
    dirs
}

/// Append `dir` to the child env `PATH`, keeping every existing entry first.
/// No-op when `dir` is already an entry.
pub fn append_path_dir(env: &mut HashMap<String, String>, dir: &Path) {
    let mut entries: Vec<PathBuf> = env
        .get("PATH")
        .map(|raw| std::env::split_paths(&OsString::from(raw)).collect())
        .unwrap_or_default();
    if entries.iter().any(|entry| entry == dir) {
        return;
    }
    entries.push(dir.to_path_buf());
    if let Ok(joined) = std::env::join_paths(entries) {
        env.insert("PATH".to_string(), joined.to_string_lossy().into_owned());
    }
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    if !std::fs::metadata(path).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn install(dir: &Path, name: &str, mode: u32) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// A program name unique to this suite, so a real install under
    /// `/usr/local/bin` (a system fallback dir) can never satisfy a lookup.
    const PROGRAM: &str = "agent-doc-gh380-fake-harness";

    #[test]
    fn bare_name_on_child_path_is_used_as_given() {
        let root = TempDir::new().unwrap();
        let bin = root.path().join("on-path");
        install(&bin, PROGRAM, 0o755);
        let env = env(&[("PATH", bin.to_str().unwrap()), ("HOME", "/nonexistent")]);
        let resolved = resolve_spawn_program(PROGRAM, &env, root.path(), None).unwrap();
        assert_eq!(resolved.program, PROGRAM);
        assert_eq!(resolved.source, SpawnProgramSource::AsGiven);
        assert_eq!(resolved.path_dir_appended, None);
    }

    #[test]
    fn bare_name_missing_from_path_resolves_user_local_bin() {
        let home = TempDir::new().unwrap();
        let local_bin = home.path().join(".local/bin");
        let installed = install(&local_bin, PROGRAM, 0o755);
        // The Coder workspace default PATH: system dirs only, no ~/.local/bin.
        let env = env(&[
            (
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            ),
            ("HOME", home.path().to_str().unwrap()),
        ]);
        let resolved = resolve_spawn_program(PROGRAM, &env, home.path(), None).unwrap();
        assert_eq!(resolved.program, installed.to_str().unwrap());
        assert_eq!(resolved.source, SpawnProgramSource::InstallDirFallback);
        assert_eq!(resolved.path_dir_appended, Some(local_bin));
    }

    #[test]
    fn fallback_uses_parent_home_when_child_env_has_none() {
        let home = TempDir::new().unwrap();
        let installed = install(&home.path().join(".claude/local"), PROGRAM, 0o755);
        let env = env(&[("PATH", "/usr/bin:/bin")]);
        let resolved =
            resolve_spawn_program(PROGRAM, &env, home.path(), home.path().to_str()).unwrap();
        assert_eq!(resolved.program, installed.to_str().unwrap());
    }

    #[test]
    fn child_path_entries_take_precedence_over_fallback_dirs() {
        let home = TempDir::new().unwrap();
        install(&home.path().join(".local/bin"), PROGRAM, 0o755);
        let custom = home.path().join("custom");
        install(&custom, PROGRAM, 0o755);
        let env = env(&[
            ("PATH", custom.to_str().unwrap()),
            ("HOME", home.path().to_str().unwrap()),
        ]);
        let resolved = resolve_spawn_program(PROGRAM, &env, home.path(), None).unwrap();
        assert_eq!(resolved.source, SpawnProgramSource::AsGiven);
        assert_eq!(resolved.program, PROGRAM);
    }

    #[test]
    fn non_executable_candidate_is_skipped() {
        let home = TempDir::new().unwrap();
        install(&home.path().join(".local/bin"), PROGRAM, 0o644);
        let installed = install(&home.path().join(".cargo/bin"), PROGRAM, 0o755);
        let env = env(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", home.path().to_str().unwrap()),
        ]);
        let resolved = resolve_spawn_program(PROGRAM, &env, home.path(), None).unwrap();
        assert_eq!(resolved.program, installed.to_str().unwrap());
    }

    #[test]
    fn explicit_path_program_is_used_as_given() {
        let root = TempDir::new().unwrap();
        let env = env(&[("PATH", "/usr/bin:/bin")]);
        let resolved =
            resolve_spawn_program("/nonexistent/claude", &env, root.path(), None).unwrap();
        assert_eq!(resolved.program, "/nonexistent/claude");
        assert_eq!(resolved.source, SpawnProgramSource::AsGiven);
    }

    #[test]
    fn not_found_error_lists_searched_directories() {
        let home = TempDir::new().unwrap();
        let env = env(&[
            ("PATH", "/usr/bin:/bin"),
            ("HOME", home.path().to_str().unwrap()),
        ]);
        let err = resolve_spawn_program(PROGRAM, &env, home.path(), None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains(&format!("`{PROGRAM}` was not found")), "{msg}");
        assert!(msg.contains("child PATH /usr/bin:/bin"), "{msg}");
        let local_bin = home.path().join(".local/bin");
        assert!(msg.contains(local_bin.to_str().unwrap()), "{msg}");
        assert!(msg.contains("/usr/local/bin"), "{msg}");
        assert!(msg.contains("document `env:` PATH"), "{msg}");
    }

    #[test]
    fn append_path_dir_preserves_existing_entries() {
        let mut env = env(&[("PATH", "/usr/bin:/bin")]);
        append_path_dir(&mut env, Path::new("/home/u/.local/bin"));
        assert_eq!(env["PATH"], "/usr/bin:/bin:/home/u/.local/bin");
        append_path_dir(&mut env, Path::new("/home/u/.local/bin"));
        assert_eq!(env["PATH"], "/usr/bin:/bin:/home/u/.local/bin");

        let mut empty = HashMap::new();
        append_path_dir(&mut empty, Path::new("/home/u/.local/bin"));
        assert_eq!(empty["PATH"], "/home/u/.local/bin");
    }
}
