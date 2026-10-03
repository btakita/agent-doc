//! JetBrains plugin install discovery: which IDE plugin directories exist and
//! which `agent-doc-jetbrains-<version>.jar` is installed in them. Lives in this
//! leaf crate so `agent-doc plugin install`, the activation probe, and the
//! preflight `stale_plugin` warning all look where an install actually lands.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The newest installed plugin artifact found under one IDE plugins directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPluginArtifact {
    /// The jar itself, e.g. `…/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.35.400.jar`.
    pub path: PathBuf,
    /// Version parsed from the jar filename.
    pub version: String,
    /// When the jar was written — the moment the install landed.
    pub modified: SystemTime,
}

/// Newest installed `agent-doc-jetbrains-<version>.jar` across `plugins_dirs`.
pub fn newest_installed_artifact(plugins_dirs: &[PathBuf]) -> Option<InstalledPluginArtifact> {
    plugins_dirs
        .iter()
        .flat_map(|dir| installed_artifacts_in(dir))
        .max_by_key(|artifact| artifact.modified)
}

pub fn installed_artifacts_in(plugins_dir: &Path) -> Vec<InstalledPluginArtifact> {
    let lib_dir = plugins_dir.join("agent-doc-jetbrains/lib");
    let Ok(entries) = std::fs::read_dir(&lib_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name
                .strip_prefix("agent-doc-jetbrains-")?
                .strip_suffix(".jar")?
                .to_string();
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some(InstalledPluginArtifact {
                path: entry.path(),
                version,
                modified,
            })
        })
        .collect()
}

pub fn jetbrains_plugin_dirs() -> Vec<PathBuf> {
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return vec![],
    };

    let search_roots = if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support/JetBrains")]
    } else {
        vec![
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("JetBrains"),
        ]
    };

    jetbrains_plugin_dirs_in_roots(&search_roots)
}

pub fn jetbrains_plugin_dirs_in_roots(search_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in search_roots {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                if !path.is_dir() || !is_jetbrains_ide_data_dir(&name.to_string_lossy()) {
                    continue;
                }
                let plugins = path.join("plugins");
                if plugins.is_dir() {
                    dirs.push(plugins);
                } else {
                    // Modern IDEs commonly expose the product-version data root itself as
                    // `idea.plugins.path`.
                    dirs.push(path);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

pub fn is_jetbrains_ide_data_dir(name: &str) -> bool {
    const PRODUCTS: &[&str] = &[
        "Aqua",
        "CLion",
        "DataGrip",
        "GoLand",
        "IdeaIC",
        "IntelliJIdea",
        "PhpStorm",
        "PyCharm",
        "Rider",
        "RubyMine",
        "RustRover",
        "WebStorm",
    ];
    name.chars().any(|ch| ch.is_ascii_digit())
        && PRODUCTS.iter().any(|product| name.starts_with(product))
}

/// `#gh76secondary`: the newest on-disk installed JetBrains plugin version.
pub fn installed_jetbrains_plugin_version() -> Option<String> {
    newest_installed_artifact(&jetbrains_plugin_dirs()).map(|artifact| artifact.version)
}

/// GH #87: marker line recording which package version an install staged for
/// the next IDE start. Written beside the reason in
/// [`crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER`].
pub const STAGED_VERSION_MARKER_PREFIX: &str = "staged_version=";

/// GH #87: the package version a restart-required marker says is staged, when
/// the install staged one (`staged_version=<v>` line).
pub fn staged_version_from_restart_marker(marker: &str) -> Option<String> {
    marker.lines().find_map(|line| {
        line.trim()
            .strip_prefix(STAGED_VERSION_MARKER_PREFIX)
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(str::to_string)
    })
}

/// GH #87: versions of `agent-doc-jetbrains-<v>.zip` that IntelliJ's pending
/// install queue (`<system>/plugins/action.script`) will unzip at the next start.
/// Lines look like `unzip:<source zip>:<destination dir>`.
pub fn staged_versions_from_action_script(script: &str) -> Vec<String> {
    script
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("unzip:")?;
            let source = rest.split(':').next()?;
            let name = Path::new(source).file_name()?.to_str()?;
            name.strip_prefix("agent-doc-jetbrains-")?
                .strip_suffix(".zip")
                .map(str::to_string)
        })
        .collect()
}

/// JetBrains system (cache) roots holding each IDE's `plugins/action.script`.
fn jetbrains_system_roots() -> Vec<PathBuf> {
    let Ok(home) = std::env::var("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    if cfg!(target_os = "macos") {
        vec![home.join("Library/Caches/JetBrains")]
    } else {
        vec![
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
                .join("JetBrains"),
        ]
    }
}

/// GH #87: is plugin `version` already staged for the next IDE start? Either
/// IntelliJ's own pending-install queue unzips it, or agent-doc's install
/// recorded staging it. A restart is then the whole remedy.
pub fn jetbrains_plugin_staged_for_restart(version: &str) -> bool {
    jetbrains_plugin_staged_in(&jetbrains_plugin_dirs(), &jetbrains_system_roots(), version)
}

/// GH #108: retire a staged-restart marker once the restart it demanded has
/// happened. The receipt is the plugin tree itself: the IDE unzips the staged
/// package at its next start, so an installed jar at `staged_version` written
/// no earlier than the marker proves the staging was applied. A same-version
/// jar older than the marker (a rebuilt local package) is not a receipt.
/// Returns whether the marker was removed.
pub fn retire_satisfied_restart_marker(plugins_dir: &Path) -> bool {
    let marker = plugins_dir.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER);
    let Some(staged) = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|body| staged_version_from_restart_marker(&body))
    else {
        return false;
    };
    let Some(recorded_at) = std::fs::metadata(&marker).and_then(|m| m.modified()).ok() else {
        return false;
    };
    let applied = installed_artifacts_in(plugins_dir)
        .iter()
        .any(|artifact| artifact.version == staged && artifact.modified >= recorded_at);
    applied && std::fs::remove_file(&marker).is_ok()
}

/// Checks for `version` staged in any plugins dir or IDE pending-install queue.
/// Satisfied staged-restart markers are retired first (GH #108), so a marker
/// whose restart already happened never reads as still staged.
pub fn jetbrains_plugin_staged_in(
    plugins_dirs: &[PathBuf],
    system_roots: &[PathBuf],
    version: &str,
) -> bool {
    let version = version.trim();
    for dir in plugins_dirs {
        retire_satisfied_restart_marker(dir);
    }
    let marker_staged = plugins_dirs.iter().any(|dir| {
        std::fs::read_to_string(dir.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER))
            .ok()
            .and_then(|marker| staged_version_from_restart_marker(&marker))
            .is_some_and(|staged| staged == version)
    });
    marker_staged
        || system_roots.iter().any(|root| {
            let Ok(entries) = std::fs::read_dir(root) else {
                return false;
            };
            entries.flatten().any(|entry| {
                is_jetbrains_ide_data_dir(&entry.file_name().to_string_lossy())
                    && std::fs::read(entry.path().join("plugins/action.script"))
                        .ok()
                        .is_some_and(|bytes| {
                            staged_versions_from_action_script(&String::from_utf8_lossy(&bytes))
                                .iter()
                                .any(|staged| staged == version)
                        })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_script_unzip_lines_name_the_staged_version() {
        let script = "delete:/h/.local/share/JetBrains/IntelliJIdea2026.3/agent-doc-jetbrains\n\
                      unzip:/h/.cache/JetBrains/IntelliJIdea2026.3/plugins/agent-doc-jetbrains-0.2.459.zip:/h/.local/share/JetBrains/IntelliJIdea2026.3\n\
                      delete:/h/.cache/JetBrains/IntelliJIdea2026.3/plugins/agent-doc-jetbrains-0.2.459.zip\n\
                      unzip:/h/.cache/JetBrains/IntelliJIdea2026.3/plugins/other-plugin-1.0.zip:/h/x\n";
        assert_eq!(staged_versions_from_action_script(script), vec!["0.2.459"]);
    }

    #[test]
    fn restart_marker_records_the_staged_version_after_the_reason() {
        let marker =
            "dynamic upgrade unavailable, staged for restart: pid 9\nstaged_version=0.2.459\n";
        assert_eq!(
            staged_version_from_restart_marker(marker).as_deref(),
            Some("0.2.459")
        );
        assert_eq!(staged_version_from_restart_marker("refused only\n"), None);
    }

    /// GH #108: the marker outlived the restart that satisfied it.
    #[test]
    fn satisfied_staged_marker_is_retired_once_the_staged_jar_lands() {
        let tmp = std::env::temp_dir().join(format!("adoc-gh108-{}", std::process::id()));
        let plugins = tmp.join("IntelliJIdea2026.3");
        let lib = plugins.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        let marker = plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(lib.join("agent-doc-jetbrains-0.2.479.jar"), b"old").unwrap();
        let old_jar_time = SystemTime::now() - std::time::Duration::from_secs(120);
        std::fs::File::options()
            .write(true)
            .open(lib.join("agent-doc-jetbrains-0.2.479.jar"))
            .unwrap()
            .set_modified(old_jar_time)
            .unwrap();
        std::fs::write(&marker, "staged for restart: pid 1\nstaged_version=0.2.480\n").unwrap();

        // Restart not yet happened: the staged version is not on disk.
        assert!(!retire_satisfied_restart_marker(&plugins));
        assert!(marker.exists());

        // A same-version jar older than the marker is not a receipt.
        let early = lib.join("agent-doc-jetbrains-0.2.480.jar");
        std::fs::write(&early, b"rebuilt").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&early)
            .unwrap()
            .set_modified(old_jar_time)
            .unwrap();
        assert!(!retire_satisfied_restart_marker(&plugins));
        assert!(marker.exists());

        // The IDE unzipped the staged package at its next start.
        std::fs::remove_file(lib.join("agent-doc-jetbrains-0.2.479.jar")).unwrap();
        std::fs::write(&early, b"new").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&early)
            .unwrap()
            .set_modified(SystemTime::now() + std::time::Duration::from_secs(1))
            .unwrap();
        assert!(!jetbrains_plugin_staged_in(&[plugins.clone()], &[], "0.2.480"));
        assert!(!marker.exists(), "satisfied marker must be retired");

        // A reason-only (non-staged) marker is never touched by this rule.
        std::fs::write(&marker, "refused only\n").unwrap();
        assert!(!retire_satisfied_restart_marker(&plugins));
        assert!(marker.exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn staged_detection_reads_marker_and_action_script() {
        let tmp = std::env::temp_dir().join(format!("adoc-gh87-{}", std::process::id()));
        let plugins = tmp.join("data/IntelliJIdea2026.3");
        let system = tmp.join("cache");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(system.join("IntelliJIdea2026.3/plugins")).unwrap();
        let dirs = vec![plugins.clone()];
        let roots = vec![system.clone()];
        assert!(!jetbrains_plugin_staged_in(&dirs, &roots, "0.2.459"));

        std::fs::write(
            system.join("IntelliJIdea2026.3/plugins/action.script"),
            "unzip:/c/agent-doc-jetbrains-0.2.459.zip:/d\n",
        )
        .unwrap();
        assert!(jetbrains_plugin_staged_in(&dirs, &roots, "0.2.459"));
        assert!(!jetbrains_plugin_staged_in(&dirs, &roots, "0.2.460"));

        std::fs::write(
            plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
            "staged for restart\nstaged_version=0.2.460\n",
        )
        .unwrap();
        assert!(jetbrains_plugin_staged_in(&dirs, &roots, "0.2.460"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
