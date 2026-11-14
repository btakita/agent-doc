//! # Module: codex_code_mode_host
//!
//! Codex code-mode host probe (`#codexcodemodehost`, GH #411).
//!
//! ## Why this exists
//!
//! Current Codex releases run every model tool call through one `exec` tool, a
//! JavaScript cell that a separate executable, `codex-code-mode-host`, runs.
//! Codex ships that executable inside its package, next to the native `codex`
//! binary (`<package>/bin/`, or `vendor/<triple>/bin/` in the npm package). A
//! bare `codex` binary copied on its own, for example into `~/.local/bin`, has
//! no host beside it. Codex then reports `Code Mode is unavailable because ...;
//! enable features.code_mode_host and install codex-code-mode-host`, and an
//! agent-doc Codex session cannot run any tool. agent-doc never spawns the host
//! itself; it launches `codex`. So this probe only diagnoses: it names the
//! `codex` it found, where it looked for the host, and how to fix the install.
//!
//! ## Spec
//! - `probe_with(path_var, features)` resolves `codex` on `path_var`. When it is
//!   absent the probe is [`CodeModeHostProbe::CodexAbsent`].
//! - `features` returns the stdout of `codex features list` for that binary.
//!   Codex needs the host only when that output has a `code_mode_host` row, in
//!   either state. Without the row (an older Codex) the probe is
//!   [`CodeModeHostProbe::NotRequired`]; when the command fails it is
//!   [`CodeModeHostProbe::Unverified`].
//! - The native Codex directory is the directory of the canonicalized `codex`.
//!   When that file is a script (the npm `codex.js` launcher), the native
//!   directories are the npm package's `vendor/<triple>/bin` directories that
//!   hold a `codex` binary. When no native directory can be found, the probe is
//!   [`CodeModeHostProbe::Unverified`], never `Missing`.
//! - The host is found beside a native `codex` first, then on `path_var`.
//!   Found nowhere, the probe is [`CodeModeHostProbe::Missing`] and lists every
//!   directory it searched.
//!
//! ## Agentic Contracts
//! - Only `Missing` produces an install warning or a `session doctor` issue;
//!   every other state is silent in `session doctor`.
//! - The probe has no side effects beyond running `codex features list`.
//!
//! ## Evals
//! - bare_codex_without_host_is_missing: a lone native `codex` + code_mode_host row → `Missing`
//! - host_beside_codex_is_found / host_on_path_is_found
//! - npm_launcher_resolves_vendor_bin: `codex.js` script → host found in `vendor/<triple>/bin`
//! - npm_launcher_without_vendor_is_unverified
//! - codex_without_feature_row_does_not_need_host
//! - failed_features_command_is_unverified
//! - missing_codex_is_absent

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The executable Codex runs `exec` tool cells in.
pub const CODE_MODE_HOST_BIN: &str = "codex-code-mode-host";
const CODEX_BIN: &str = "codex";
const FEATURE_NAME: &str = "code_mode_host";
const FEATURES_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeModeHostProbe {
    /// `codex` is not on PATH; there is nothing to check.
    CodexAbsent,
    /// This Codex has no `code_mode_host` feature, so it does not use the host.
    NotRequired { codex: PathBuf },
    /// The native Codex binary could not be located, so absence of the host
    /// cannot be proven.
    Unverified { codex: PathBuf, reason: String },
    Found {
        codex: PathBuf,
        host: PathBuf,
        /// `true` when the host was found only on PATH, not beside `codex`.
        via_path: bool,
    },
    Missing {
        codex: PathBuf,
        /// `Some(false)` when `code_mode_host` is listed but disabled.
        feature_enabled: Option<bool>,
        searched: Vec<PathBuf>,
    },
}

impl CodeModeHostProbe {
    /// What to tell the operator when the host is missing, or `None`.
    pub fn missing_message(&self) -> Option<String> {
        let Self::Missing {
            codex,
            feature_enabled,
            searched,
        } = self
        else {
            return None;
        };
        let searched = searched
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let enable = if *feature_enabled == Some(false) {
            " Then enable it with `codex features enable code_mode_host`."
        } else {
            ""
        };
        Some(format!(
            "Codex at {} has no `{CODE_MODE_HOST_BIN}` (searched: {searched}), so Codex cannot run tools and reports `Code Mode is unavailable`. This codex is a bare binary outside a complete Codex package. Reinstall Codex from a package that ships `{CODE_MODE_HOST_BIN}` (npm `@openai/codex`, Homebrew, the official install script, or a distro package), or put `{CODE_MODE_HOST_BIN}` from the same Codex release beside that `codex`.{enable}",
            codex.display()
        ))
    }

    /// The `session doctor` line for this probe, or `None` when healthy.
    pub fn doctor_issue(&self) -> Option<String> {
        self.missing_message()
    }

    /// `agent-doc install` / `init` prerequisite lines.
    pub fn prereq_lines(&self) -> Vec<String> {
        match self {
            Self::CodexAbsent => Vec::new(),
            Self::NotRequired { codex } => vec![format!(
                "[install] codex ... ok ({}; no code-mode host needed)",
                codex.display()
            )],
            Self::Unverified { codex, reason } => vec![format!(
                "[install] codex ... ok ({}; code-mode host not verified: {reason})",
                codex.display()
            )],
            Self::Found {
                codex,
                host,
                via_path,
            } => vec![format!(
                "[install] codex ... ok ({}; {CODE_MODE_HOST_BIN}={}{})",
                codex.display(),
                host.display(),
                if *via_path { " via PATH" } else { "" }
            )],
            Self::Missing { codex, .. } => vec![
                format!(
                    "[install] codex ... BROKEN ({}; {CODE_MODE_HOST_BIN} missing)",
                    codex.display()
                ),
                format!(
                    "[install]   hint: {}",
                    self.missing_message().unwrap_or_default()
                ),
            ],
        }
    }
}

/// Parse the `code_mode_host` row of `codex features list`.
///
/// `None` = no such row; `Some(enabled)` = the row's effective state.
pub fn parse_code_mode_host_feature(features_stdout: &str) -> Option<bool> {
    features_stdout.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if words.next()? != FEATURE_NAME {
            return None;
        }
        Some(words.last() == Some("true"))
    })
}

fn find_on_path(path_var: &str, bin: &str) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(bin))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn is_script(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| &head == b"#!")
}

fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// `vendor/<triple>/bin` directories holding a native `codex`, for the npm
/// package whose launcher script is `launcher` (`<package>/bin/codex.js`).
/// The launcher resolves `@openai/codex-<platform>/vendor` first and falls back
/// to `<package>/vendor`.
fn npm_vendor_bin_dirs(launcher: &Path) -> Vec<PathBuf> {
    let Some(package) = launcher.parent().and_then(Path::parent) else {
        return Vec::new();
    };
    let mut vendor_roots: Vec<PathBuf> = child_dirs(&package.join("node_modules/@openai"))
        .into_iter()
        .filter(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("codex-"))
        })
        .map(|dir| dir.join("vendor"))
        .collect();
    vendor_roots.push(package.join("vendor"));
    vendor_roots
        .iter()
        .flat_map(|root| child_dirs(root))
        .map(|triple| triple.join("bin"))
        .filter(|bin| is_executable_file(&bin.join(CODEX_BIN)))
        .collect()
}

/// Probe for the Codex code-mode host with an explicit `PATH` and a
/// `codex features list` runner (tests inject both).
pub fn probe_with(
    path_var: &str,
    features: impl FnOnce(&Path) -> Option<String>,
) -> CodeModeHostProbe {
    let Some(codex) = find_on_path(path_var, CODEX_BIN) else {
        return CodeModeHostProbe::CodexAbsent;
    };
    let Some(features_stdout) = features(&codex) else {
        return CodeModeHostProbe::Unverified {
            reason: "`codex features list` failed".to_string(),
            codex,
        };
    };
    let Some(feature_enabled) = parse_code_mode_host_feature(&features_stdout) else {
        return CodeModeHostProbe::NotRequired { codex };
    };
    let resolved = std::fs::canonicalize(&codex).unwrap_or_else(|_| codex.clone());
    let native_dirs = if is_script(&resolved) {
        npm_vendor_bin_dirs(&resolved)
    } else {
        resolved
            .parent()
            .map(Path::to_path_buf)
            .into_iter()
            .collect()
    };
    if native_dirs.is_empty() {
        return CodeModeHostProbe::Unverified {
            reason: format!(
                "{} is a launcher script and no native codex was found in its package",
                resolved.display()
            ),
            codex,
        };
    }
    if let Some(host) = native_dirs
        .iter()
        .map(|dir| dir.join(CODE_MODE_HOST_BIN))
        .find(|host| is_executable_file(host))
    {
        return CodeModeHostProbe::Found {
            codex,
            host,
            via_path: false,
        };
    }
    if let Some(host) = find_on_path(path_var, CODE_MODE_HOST_BIN) {
        return CodeModeHostProbe::Found {
            codex,
            host,
            via_path: true,
        };
    }
    let mut searched = native_dirs;
    searched.push(PathBuf::from("$PATH"));
    CodeModeHostProbe::Missing {
        codex,
        feature_enabled: Some(feature_enabled),
        searched,
    }
}

/// Run `<codex> features list`, bounded so a wedged Codex cannot hang the caller.
fn codex_features_list(codex: &Path) -> Option<String> {
    let mut child = Command::new(codex)
        .args(["features", "list"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + FEATURES_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Probe the caller's `PATH` and the real `codex features list`.
pub fn probe() -> CodeModeHostProbe {
    let path_var = std::env::var("PATH").unwrap_or_default();
    probe_with(&path_var, codex_features_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const FEATURES_ON: &str = "code_mode                                under development  false\ncode_mode_host                           stable             true\n";
    const FEATURES_OFF: &str =
        "code_mode_host                           stable             false\n";

    fn write_exe(path: &Path, content: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    fn path_of(dirs: &[&Path]) -> String {
        std::env::join_paths(dirs).unwrap().into_string().unwrap()
    }

    #[test]
    fn parses_the_code_mode_host_row_only() {
        assert_eq!(parse_code_mode_host_feature(FEATURES_ON), Some(true));
        assert_eq!(parse_code_mode_host_feature(FEATURES_OFF), Some(false));
        assert_eq!(
            parse_code_mode_host_feature("code_mode_host_grpc  under development  true\n"),
            None,
            "a prefix-sharing feature is not the host feature"
        );
        assert_eq!(parse_code_mode_host_feature(""), None);
    }

    #[test]
    fn missing_codex_is_absent() {
        let empty = TempDir::new().unwrap();
        let probe = probe_with(&path_of(&[empty.path()]), |_| panic!("no codex to run"));
        assert_eq!(probe, CodeModeHostProbe::CodexAbsent);
        assert!(probe.prereq_lines().is_empty());
        assert_eq!(probe.doctor_issue(), None);
    }

    #[test]
    fn bare_codex_without_host_is_missing() {
        let local_bin = TempDir::new().unwrap();
        write_exe(&local_bin.path().join("codex"), b"\x7fELF native");
        let probe = probe_with(&path_of(&[local_bin.path()]), |_| {
            Some(FEATURES_ON.to_string())
        });
        let CodeModeHostProbe::Missing { searched, .. } = &probe else {
            panic!("expected Missing, got {probe:?}");
        };
        let canonical = std::fs::canonicalize(local_bin.path()).unwrap();
        assert_eq!(searched, &vec![canonical, PathBuf::from("$PATH")]);
        let issue = probe
            .doctor_issue()
            .expect("a missing host is a doctor issue");
        assert!(issue.contains("codex-code-mode-host"), "{issue}");
        assert!(issue.contains("Code Mode is unavailable"), "{issue}");
        assert!(issue.contains("Reinstall Codex"), "{issue}");
        assert!(!issue.contains("codex features enable"), "{issue}");
        let lines = probe.prereq_lines();
        assert!(lines[0].contains("BROKEN"), "{lines:?}");
    }

    #[test]
    fn a_disabled_feature_also_says_how_to_enable_it() {
        let local_bin = TempDir::new().unwrap();
        write_exe(&local_bin.path().join("codex"), b"\x7fELF native");
        let probe = probe_with(&path_of(&[local_bin.path()]), |_| {
            Some(FEATURES_OFF.to_string())
        });
        let issue = probe.doctor_issue().expect("missing");
        assert!(
            issue.contains("codex features enable code_mode_host"),
            "{issue}"
        );
    }

    #[test]
    fn host_beside_codex_is_found() {
        let package_bin = TempDir::new().unwrap();
        write_exe(&package_bin.path().join("codex"), b"\x7fELF native");
        write_exe(
            &package_bin.path().join(CODE_MODE_HOST_BIN),
            b"\x7fELF host",
        );
        let probe = probe_with(&path_of(&[package_bin.path()]), |_| {
            Some(FEATURES_ON.to_string())
        });
        assert!(
            matches!(
                probe,
                CodeModeHostProbe::Found {
                    via_path: false,
                    ..
                }
            ),
            "{probe:?}"
        );
        assert_eq!(probe.doctor_issue(), None);
        assert!(probe.prereq_lines()[0].contains("ok"));
    }

    #[cfg(unix)]
    #[test]
    fn host_beside_a_symlinked_codex_is_found() {
        let root = TempDir::new().unwrap();
        let package_bin = root.path().join("lib/openai-codex/bin");
        write_exe(&package_bin.join("codex"), b"\x7fELF native");
        write_exe(&package_bin.join(CODE_MODE_HOST_BIN), b"\x7fELF host");
        let usr_bin = root.path().join("bin");
        std::fs::create_dir_all(&usr_bin).unwrap();
        std::os::unix::fs::symlink(package_bin.join("codex"), usr_bin.join("codex")).unwrap();
        let probe = probe_with(&path_of(&[&usr_bin]), |_| Some(FEATURES_ON.to_string()));
        assert!(
            matches!(
                probe,
                CodeModeHostProbe::Found {
                    via_path: false,
                    ..
                }
            ),
            "{probe:?}"
        );
    }

    #[test]
    fn host_on_path_is_found() {
        let local_bin = TempDir::new().unwrap();
        let other_bin = TempDir::new().unwrap();
        write_exe(&local_bin.path().join("codex"), b"\x7fELF native");
        write_exe(&other_bin.path().join(CODE_MODE_HOST_BIN), b"\x7fELF host");
        let probe = probe_with(&path_of(&[local_bin.path(), other_bin.path()]), |_| {
            Some(FEATURES_ON.to_string())
        });
        assert!(
            matches!(probe, CodeModeHostProbe::Found { via_path: true, .. }),
            "{probe:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn npm_launcher_resolves_vendor_bin() {
        let root = TempDir::new().unwrap();
        let package = root.path().join("lib/node_modules/@openai/codex");
        let launcher = package.join("bin/codex.js");
        write_exe(&launcher, b"#!/usr/bin/env node\n");
        let vendor_bin = package
            .join("node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin");
        write_exe(&vendor_bin.join("codex"), b"\x7fELF native");
        let npm_bin = root.path().join("bin");
        std::fs::create_dir_all(&npm_bin).unwrap();
        std::os::unix::fs::symlink(&launcher, npm_bin.join("codex")).unwrap();

        let probe = probe_with(&path_of(&[&npm_bin]), |_| Some(FEATURES_ON.to_string()));
        let CodeModeHostProbe::Missing { searched, .. } = &probe else {
            panic!("expected Missing without a vendor host, got {probe:?}");
        };
        assert!(
            searched[0].ends_with("vendor/x86_64-unknown-linux-musl/bin"),
            "{searched:?}"
        );

        write_exe(&vendor_bin.join(CODE_MODE_HOST_BIN), b"\x7fELF host");
        let probe = probe_with(&path_of(&[&npm_bin]), |_| Some(FEATURES_ON.to_string()));
        assert!(
            matches!(
                probe,
                CodeModeHostProbe::Found {
                    via_path: false,
                    ..
                }
            ),
            "{probe:?}"
        );
    }

    #[test]
    fn npm_launcher_without_vendor_is_unverified() {
        let root = TempDir::new().unwrap();
        let launcher_dir = root.path().join("pkg/bin");
        write_exe(&launcher_dir.join("codex"), b"#!/usr/bin/env node\n");
        let probe = probe_with(&path_of(&[&launcher_dir]), |_| {
            Some(FEATURES_ON.to_string())
        });
        assert!(
            matches!(probe, CodeModeHostProbe::Unverified { .. }),
            "{probe:?}"
        );
        assert_eq!(
            probe.doctor_issue(),
            None,
            "unproven absence is not an issue"
        );
    }

    #[test]
    fn codex_without_feature_row_does_not_need_host() {
        let local_bin = TempDir::new().unwrap();
        write_exe(&local_bin.path().join("codex"), b"\x7fELF native");
        let probe = probe_with(&path_of(&[local_bin.path()]), |_| {
            Some("shell_tool  stable  true\n".to_string())
        });
        assert!(
            matches!(probe, CodeModeHostProbe::NotRequired { .. }),
            "{probe:?}"
        );
        assert_eq!(probe.doctor_issue(), None);
    }

    #[test]
    fn failed_features_command_is_unverified() {
        let local_bin = TempDir::new().unwrap();
        write_exe(&local_bin.path().join("codex"), b"\x7fELF native");
        let probe = probe_with(&path_of(&[local_bin.path()]), |_| None);
        assert!(
            matches!(probe, CodeModeHostProbe::Unverified { .. }),
            "{probe:?}"
        );
        assert_eq!(
            probe.doctor_issue(),
            None,
            "unproven absence is not an issue"
        );
        assert!(probe.prereq_lines()[0].contains("not verified"));
    }
}
