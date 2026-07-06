use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const AGENT_DOC_BIN_ENV: &str = "AGENT_DOC_BIN";

/// Resolve the freshly installed launchable `agent-doc` binary path.
///
/// This prefers a launchable `current_exe`, but skips missing or deleted mapped
/// inodes, rejects host executables from embedded editor/plugin processes, and
/// falls back to an explicit override, argv0, PATH, and common user install
/// locations so recycle and child-process launch paths target the binary on disk.
pub fn current_agent_doc_binary() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    resolve_agent_doc_binary_from_env(
        std::env::var_os(AGENT_DOC_BIN_ENV),
        std::env::current_exe().ok(),
        std::env::args_os().next(),
        std::env::var_os("PATH"),
        std::env::var_os("HOME"),
        &cwd,
    )
}

pub fn resolve_agent_doc_binary_from_env(
    override_bin: Option<OsString>,
    current_exe: Option<PathBuf>,
    argv0: Option<OsString>,
    path_env: Option<OsString>,
    home: Option<OsString>,
    cwd: &Path,
) -> Result<PathBuf> {
    if let Some(raw_override) = override_bin.as_deref()
        && !raw_override.is_empty()
    {
        let path = Path::new(raw_override);
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        if launchable_file(&candidate) && path_looks_like_agent_doc_binary(&candidate) {
            return Ok(candidate);
        }
    }

    let stale_current_exe = match current_exe {
        Some(path) if launchable_agent_doc_binary(&path) => return Ok(path),
        other => other,
    };

    let mut path_search_names = Vec::new();
    if let Some(raw_argv0) = argv0.as_deref() {
        let argv0_path = Path::new(raw_argv0);
        if has_path_separator(argv0_path) {
            let candidate = if argv0_path.is_absolute() {
                argv0_path.to_path_buf()
            } else {
                cwd.join(argv0_path)
            };
            if launchable_agent_doc_binary(&candidate) {
                return Ok(candidate);
            }
        } else if !raw_argv0.is_empty() && token_looks_like_agent_doc_binary(raw_argv0) {
            path_search_names.push(raw_argv0.to_os_string());
        }
    }
    if !path_search_names
        .iter()
        .any(|name| name == OsStr::new("agent-doc"))
    {
        path_search_names.push(OsString::from("agent-doc"));
    }

    if let Some(path_env) = path_env {
        for dir in std::env::split_paths(&path_env) {
            for name in &path_search_names {
                let candidate = dir.join(name);
                if launchable_agent_doc_binary(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }

    if let Some(home) = home {
        let home = PathBuf::from(home);
        for candidate in [
            home.join("bin/agent-doc"),
            home.join(".local/bin/agent-doc"),
            home.join(".cargo/bin/agent-doc"),
            PathBuf::from("/usr/local/bin/agent-doc"),
        ] {
            if launchable_agent_doc_binary(&candidate) {
                return Ok(candidate);
            }
        }
    } else {
        let candidate = PathBuf::from("/usr/local/bin/agent-doc");
        if launchable_agent_doc_binary(&candidate) {
            return Ok(candidate);
        }
    }

    let stale = stale_current_exe
        .as_ref()
        .map(|path| format!("; skipped missing current_exe {}", path.display()))
        .unwrap_or_default();
    anyhow::bail!("failed to locate launchable agent-doc binary{stale}");
}

fn launchable_agent_doc_binary(path: &Path) -> bool {
    path_looks_like_agent_doc_binary(path) && launchable_file(path)
}

fn launchable_file(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn path_looks_like_agent_doc_binary(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("agent-doc"))
}

fn token_looks_like_agent_doc_binary(token: &OsStr) -> bool {
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("agent-doc"))
}

fn has_path_separator(path: &Path) -> bool {
    path.is_absolute() || path.components().count() > 1
}

pub fn internal_command_spawn_context(command: &str, exe: &Path) -> String {
    let cwd = std::env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|err| format!("<unavailable: {err}>"));
    let path_present = std::env::var_os("PATH").is_some();
    format!(
        "failed to spawn `agent-doc {command}` (binary={}, cwd={}, PATH_present={})",
        exe.display(),
        cwd,
        path_present
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "agent-doc-turn-executor-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn agent_doc_binary_resolution_works_without_path_when_current_exe_exists() {
        let dir = temp_dir("current");
        let current = dir.join("agent-doc-current");
        std::fs::write(&current, "binary").unwrap();

        let resolved =
            resolve_agent_doc_binary_from_env(None, Some(current.clone()), None, None, None, &dir)
                .unwrap();

        assert_eq!(resolved, current);
    }

    #[test]
    fn agent_doc_binary_resolution_falls_back_when_current_exe_is_stale() {
        let dir = temp_dir("fallback");
        let bin_dir = dir.join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let fallback = bin_dir.join("agent-doc");
        std::fs::write(&fallback, "binary").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(dir.join("agent-doc (deleted)")),
            Some(OsString::from("agent-doc")),
            Some(bin_dir.into_os_string()),
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, fallback);
    }

    #[test]
    fn controller_binary_resolution_prefers_existing_current_exe() {
        let dir = temp_dir("prefer-current");
        let current = dir.join("agent-doc-current");
        let path_bin_dir = dir.join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&current, "current").unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(current.clone()),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, current);
    }

    #[test]
    fn controller_binary_resolution_falls_back_to_path_when_current_exe_is_missing() {
        let dir = temp_dir("missing-current");
        let path_bin_dir = dir.join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "path").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(dir.join("deleted-agent-doc")),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    #[test]
    fn controller_binary_resolution_skips_deleted_proc_self_exe_suffix() {
        let dir = temp_dir("deleted-suffix");
        let path_bin_dir = dir.join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&path_bin, "fresh").unwrap();

        let deleted_exe = dir.join("agent-doc (deleted)");
        assert!(!deleted_exe.exists());

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(deleted_exe),
            Some(OsString::from("agent-doc")),
            Some(path_bin_dir.into_os_string()),
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    #[test]
    fn controller_binary_resolution_rejects_embedded_ide_current_exe() {
        let dir = temp_dir("embedded-ide");
        let idea = dir.join("idea");
        let path_bin_dir = dir.join("bin");
        let path_bin = path_bin_dir.join("agent-doc");
        std::fs::create_dir_all(&path_bin_dir).unwrap();
        std::fs::write(&idea, "ide").unwrap();
        std::fs::write(&path_bin, "agent-doc").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(idea),
            Some(OsString::from("idea")),
            Some(path_bin_dir.into_os_string()),
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, path_bin);
    }

    #[test]
    fn controller_binary_resolution_uses_explicit_agent_doc_override() {
        let dir = temp_dir("override");
        let override_bin = dir.join("agent-doc-dev");
        let current = dir.join("idea");
        std::fs::write(&override_bin, "agent-doc").unwrap();
        std::fs::write(&current, "ide").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            Some(override_bin.clone().into_os_string()),
            Some(current),
            Some(OsString::from("idea")),
            None,
            None,
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, override_bin);
    }

    #[test]
    fn controller_binary_resolution_uses_home_install_when_ide_path_is_sparse() {
        let dir = temp_dir("home-install");
        let home = dir.join("home");
        let home_bin = home.join(".cargo/bin");
        let agent_doc = home_bin.join("agent-doc");
        std::fs::create_dir_all(&home_bin).unwrap();
        std::fs::write(&agent_doc, "agent-doc").unwrap();
        let current = dir.join("idea");
        std::fs::write(&current, "ide").unwrap();

        let resolved = resolve_agent_doc_binary_from_env(
            None,
            Some(current),
            Some(OsString::from("idea")),
            None,
            Some(home.into_os_string()),
            &dir,
        )
        .unwrap();

        assert_eq!(resolved, agent_doc);
    }

    #[test]
    fn internal_spawn_context_names_binary_cwd_and_path_presence() {
        let context = internal_command_spawn_context("finalize", Path::new("/tmp/agent-doc"));
        assert!(context.contains("agent-doc finalize"));
        assert!(context.contains("binary=/tmp/agent-doc"));
        assert!(context.contains("cwd="));
        assert!(context.contains("PATH_present="));
    }
}
