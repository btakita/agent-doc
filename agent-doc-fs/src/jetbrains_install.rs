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
