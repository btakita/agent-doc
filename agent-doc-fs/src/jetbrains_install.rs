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
            let (source, _) = parse_unzip_line(line)?;
            staged_zip_version(&source)
        })
        .collect()
}

/// GH #115: the plugin version a staged agent-doc package names. Staged copies
/// are `agent-doc-jetbrains-<version>+<nonce>.zip` (unique per staging, so one
/// staging's `delete:<zip>` can never remove another's `unzip:` source); older
/// generations staged the fixed `agent-doc-jetbrains-<version>.zip`.
pub fn staged_zip_version(zip: &Path) -> Option<String> {
    let name = zip.file_name()?.to_str()?;
    let stem = name
        .strip_prefix("agent-doc-jetbrains-")?
        .strip_suffix(".zip")?;
    let version = stem.split('+').next().unwrap_or(stem).trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// `unzip:<source zip>:<destination dir>` -> (source, destination). The source
/// ends in `.zip`, so the split point is the first `.zip:`; a `:` inside either
/// path is preserved.
fn parse_unzip_line(line: &str) -> Option<(PathBuf, PathBuf)> {
    let rest = line.trim().strip_prefix("unzip:")?;
    let split = rest.find(".zip:")? + ".zip".len();
    let (source, destination) = rest.split_at(split);
    Some((
        PathBuf::from(source),
        PathBuf::from(destination.strip_prefix(':')?),
    ))
}

/// GH #115: one agent-doc package that an IDE's pending-install queue
/// (`action.script`) will unzip into a given plugins directory at its next start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingStaging {
    /// The `action.script` holding the entry.
    pub script: PathBuf,
    /// The staged package the `unzip:` entry reads.
    pub zip: PathBuf,
    pub version: String,
    /// Whether that package still exists. A pending `unzip:` whose source is gone
    /// is doomed: the paired `delete:<plugin dir>` still runs, the unzip cannot.
    pub zip_present: bool,
}

fn same_dir(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        let text = path.to_string_lossy();
        let trimmed = text.trim_end_matches('/');
        PathBuf::from(if trimmed.is_empty() { "/" } else { trimmed })
    };
    normalize(left) == normalize(right)
}

/// GH #115: every pending agent-doc staging targeting `plugins_dir` across the
/// IDE system roots' `<IDE>/plugins/action.script` files. Only the text script
/// format (current IDEs) is readable here; a serialized script yields nothing.
pub fn pending_stagings_for(plugins_dir: &Path, system_roots: &[PathBuf]) -> Vec<PendingStaging> {
    let mut pending = Vec::new();
    for root in system_roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !is_jetbrains_ide_data_dir(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let script = entry.path().join("plugins/action.script");
            let Ok(bytes) = std::fs::read(&script) else {
                continue;
            };
            for line in String::from_utf8_lossy(&bytes).lines() {
                let Some((zip, destination)) = parse_unzip_line(line) else {
                    continue;
                };
                let Some(version) = staged_zip_version(&zip) else {
                    continue;
                };
                if same_dir(&destination, plugins_dir) {
                    pending.push(PendingStaging {
                        script: script.clone(),
                        zip_present: zip.is_file(),
                        zip,
                        version,
                    });
                }
            }
        }
    }
    pending
}

/// `#jbpluginvanish`: pending agent-doc stagings removed from one IDE's
/// pending-install script by [`purge_pending_stagings_for`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgedStagings {
    /// The `action.script` that was rewritten.
    pub script: PathBuf,
    /// The staged packages whose `unzip:` entries were removed.
    pub zips: Vec<PathBuf>,
    /// How many script lines were removed (deletes of the plugin dir included).
    pub removed_lines: usize,
}

/// Which pending stagings [`purge_pending_stagings_for`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingPurge {
    /// Every pending agent-doc staging targeting the plugins dir. Used once the
    /// plugin tree holds a freshly installed generation: a leftover staging can
    /// then only downgrade it (package present) or destroy it (package gone).
    All,
    /// Only stagings whose package is gone ([`StagedInstallFailure::Doomed`]):
    /// their `delete:` would remove the plugin and their `unzip:` cannot run.
    Doomed,
}

/// Plain-text pending-install command prefixes this module can rewrite. A
/// script holding anything else (a serialized/binary script, an unknown
/// command) is left untouched: rewriting what cannot be parsed is guesswork.
const ACTION_SCRIPT_TEXT_COMMANDS: [&str; 3] = ["delete:", "unzip:", "copy:"];

/// `#jbpluginvanish`: remove pending agent-doc stagings targeting `plugins_dir`
/// from every IDE's `<system>/<IDE>/plugins/action.script`.
///
/// IntelliJ runs a staging's `delete:<plugins>/agent-doc-jetbrains` before its
/// `unzip:<package>:<plugins>`; when the unzip fails (package deleted, disk
/// full) the restart leaves NO plugin. A staging also outlives a later
/// restart-free upgrade or direct replacement, so the next IDE start would run
/// it against the freshly installed generation. On 2026-10-04 13:48 an IDE
/// restart ran a leftover `0.2.468` staging whose package was gone ("Source
/// file missing") over a dynamically installed 0.2.488 and removed the plugin.
///
/// The plugin-dir `delete:` lines are removed only when no agent-doc `unzip:`
/// into `plugins_dir` remains in that script, so a surviving viable staging
/// keeps its own delete. The script is rewritten via a sibling temp file and
/// rename; an emptied script is removed. Packages of purged stagings are
/// deleted best-effort.
pub fn purge_pending_stagings_for(
    plugins_dir: &Path,
    system_roots: &[PathBuf],
    mode: StagingPurge,
) -> anyhow::Result<Vec<PurgedStagings>> {
    use anyhow::Context as _;
    let plugin_dir = plugins_dir.join("agent-doc-jetbrains");
    let mut purged = Vec::new();
    for root in system_roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !is_jetbrains_ide_data_dir(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let script = entry.path().join("plugins/action.script");
            let Ok(bytes) = std::fs::read(&script) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let lines: Vec<&str> = text.lines().collect();
            if lines.iter().any(|line| {
                let line = line.trim();
                !line.is_empty()
                    && !ACTION_SCRIPT_TEXT_COMMANDS
                        .iter()
                        .any(|prefix| line.starts_with(prefix))
            }) {
                continue;
            }
            // Agent-doc unzips into this plugins dir: (line index, zip).
            let unzips: Vec<(usize, PathBuf)> = lines
                .iter()
                .enumerate()
                .filter_map(|(index, line)| {
                    let (zip, destination) = parse_unzip_line(line)?;
                    staged_zip_version(&zip)?;
                    same_dir(&destination, plugins_dir).then_some((index, zip))
                })
                .collect();
            let doomed: Vec<&(usize, PathBuf)> = unzips
                .iter()
                .filter(|(_, zip)| mode == StagingPurge::All || !zip.is_file())
                .collect();
            if doomed.is_empty() {
                continue;
            }
            let zips: Vec<PathBuf> = doomed.iter().map(|(_, zip)| zip.clone()).collect();
            let any_unzip_survives = unzips.len() > doomed.len();
            let mut kept = Vec::with_capacity(lines.len());
            let mut removed_lines = 0;
            for (index, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                let remove = if doomed.iter().any(|(at, _)| *at == index) {
                    true
                } else if let Some(path) = trimmed.strip_prefix("delete:") {
                    let path = Path::new(path);
                    zips.iter().any(|zip| same_dir(path, zip))
                        || (!any_unzip_survives && same_dir(path, &plugin_dir))
                } else {
                    false
                };
                if remove {
                    removed_lines += 1;
                } else {
                    kept.push(*line);
                }
            }
            if kept.iter().all(|line| line.trim().is_empty()) {
                std::fs::remove_file(&script)
                    .with_context(|| format!("Failed to remove {}", script.display()))?;
            } else {
                let mut body = kept.join("\n");
                body.push('\n');
                let temp = script.with_file_name(format!(
                    "action.script.agent-doc-purge.{}.tmp",
                    std::process::id()
                ));
                let written = std::fs::write(&temp, body.as_bytes())
                    .and_then(|()| std::fs::File::open(&temp)?.sync_all())
                    .and_then(|()| std::fs::rename(&temp, &script));
                if let Err(error) = written {
                    let _ = std::fs::remove_file(&temp);
                    return Err(error)
                        .with_context(|| format!("Failed to rewrite {}", script.display()));
                }
            }
            for zip in &zips {
                let _ = std::fs::remove_file(zip);
            }
            purged.push(PurgedStagings {
                script,
                zips,
                removed_lines,
            });
        }
    }
    Ok(purged)
}

/// GH #115: marker line recording the plugin version the plugins directory held
/// when the staging was recorded, so a post-restart check can tell applied,
/// not-applied and destroyed apart.
pub const PREVIOUS_VERSION_MARKER_PREFIX: &str = "previous_version=";

/// GH #115: the pre-staging version a restart-required marker recorded.
pub fn previous_version_from_restart_marker(marker: &str) -> Option<String> {
    marker.lines().find_map(|line| {
        line.trim()
            .strip_prefix(PREVIOUS_VERSION_MARKER_PREFIX)
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(str::to_string)
    })
}

/// GH #115: a staged JetBrains upgrade that removed, or will remove, the plugin
/// without installing its replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedInstallFailure {
    /// The marker says `staged` was queued, no pending staging remains (the IDE
    /// consumed its script), and the plugins directory holds no agent-doc jar:
    /// the restart ran the `delete:` and not the `unzip:`.
    Destroyed {
        staged: String,
        previous: Option<String>,
    },
    /// A pending staging targets this directory but every staged package it
    /// would unzip is gone, so the next IDE start deletes the plugin and
    /// installs nothing.
    Doomed { staged: String, zip: PathBuf },
}

/// GH #115: classify `plugins_dir`'s staged upgrade. `None` when nothing is
/// staged, the staging is still viable, or it was applied.
pub fn staged_install_failure(
    plugins_dir: &Path,
    system_roots: &[PathBuf],
) -> Option<StagedInstallFailure> {
    let pending = pending_stagings_for(plugins_dir, system_roots);
    if !pending.is_empty() {
        if pending.iter().any(|staging| staging.zip_present) {
            return None;
        }
        let doomed = pending.into_iter().next_back()?;
        return Some(StagedInstallFailure::Doomed {
            staged: doomed.version,
            zip: doomed.zip,
        });
    }
    let marker = std::fs::read_to_string(
        plugins_dir.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
    )
    .ok()?;
    let staged = staged_version_from_restart_marker(&marker)?;
    if !installed_artifacts_in(plugins_dir).is_empty() {
        return None;
    }
    Some(StagedInstallFailure::Destroyed {
        staged,
        previous: previous_version_from_restart_marker(&marker),
    })
}

/// GH #115: the operator-facing description of a [`StagedInstallFailure`],
/// remedy included.
pub fn staged_install_failure_message(
    plugins_dir: &Path,
    failure: &StagedInstallFailure,
) -> String {
    let remedy = format!(
        "Reinstall it now with `agent-doc plugin update jetbrains --plugins-dir {}` (or `agent-doc plugin install jetbrains --plugins-dir {}`), then restart the IDE so it loads the plugin.",
        plugins_dir.display(),
        plugins_dir.display()
    );
    match failure {
        StagedInstallFailure::Destroyed { staged, previous } => format!(
            "JetBrains plugin install DESTROYED in {}: agent-doc staged v{staged} for the next IDE start{}, the IDE has since consumed its pending-install script, and the plugin directory now holds no agent-doc-jetbrains jar. The restart deleted the plugin without installing its replacement, so this IDE has NO agent-doc plugin (no actions, no shortcuts, no editor sync). {remedy}",
            plugins_dir.display(),
            previous
                .as_deref()
                .map(|version| format!(" over v{version}"))
                .unwrap_or_default(),
        ),
        StagedInstallFailure::Doomed { staged, zip } => format!(
            "JetBrains staged upgrade to v{staged} in {} is DOOMED: the IDE's pending-install script will delete the plugin at its next start, but the package it would unzip ({}) is gone, so the restart would leave NO agent-doc plugin. Do not restart yet. {remedy}",
            plugins_dir.display(),
            zip.display()
        ),
    }
}

/// GH #115: every known JetBrains plugins directory whose staged upgrade failed.
pub fn jetbrains_staged_install_failures() -> Vec<(PathBuf, StagedInstallFailure)> {
    let roots = jetbrains_system_roots();
    jetbrains_plugin_dirs()
        .into_iter()
        .filter_map(|dir| staged_install_failure(&dir, &roots).map(|failure| (dir, failure)))
        .collect()
}

/// GH #115: lock file beside the restart-required marker that serializes every
/// agent-doc install/staging into one plugins directory (`agent-doc upgrade`
/// and a manual `plugin update` used to stage the same package concurrently).
pub const INSTALL_LOCK_FILE: &str = ".agent-doc-jetbrains-install.lock";

/// GH #115: take the exclusive install lock for `plugins_dir`, blocking while
/// another agent-doc process installs into it. Released when the returned file
/// is dropped (the kernel also releases it if the process dies).
pub fn lock_jetbrains_install(plugins_dir: &Path) -> anyhow::Result<std::fs::File> {
    use anyhow::Context as _;
    use fs2::FileExt as _;
    std::fs::create_dir_all(plugins_dir)
        .with_context(|| format!("Failed to create {}", plugins_dir.display()))?;
    let path = plugins_dir.join(INSTALL_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("Failed to open JetBrains install lock {}", path.display()))?;
    if let Err(error) = file.try_lock_exclusive() {
        if error.kind() != std::io::ErrorKind::WouldBlock
            && error.raw_os_error() != fs2::lock_contended_error().raw_os_error()
        {
            return Err(error).with_context(|| {
                format!("Failed to lock JetBrains install lock {}", path.display())
            });
        }
        eprintln!(
            "[plugin] another agent-doc install into {} is running; waiting for it to finish",
            plugins_dir.display()
        );
        file.lock_exclusive()
            .with_context(|| format!("Failed to lock JetBrains install lock {}", path.display()))?;
    }
    Ok(file)
}

/// JetBrains system (cache) roots holding each IDE's `plugins/action.script`.
pub fn jetbrains_system_roots() -> Vec<PathBuf> {
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
/// GH #115: a plugins directory with no agent-doc jar at all is never a receipt,
/// so a staging whose restart deleted the plugin without installing it keeps its
/// marker -- that marker is the evidence [`staged_install_failure`] reports.
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
    // GH #115: a marker whose staging destroyed the plugin is not "staged":
    // a restart is no longer the remedy there, a reinstall is.
    let marker_staged = plugins_dirs.iter().any(|dir| {
        staged_install_failure(dir, system_roots).is_none()
            && std::fs::read_to_string(dir.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER))
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
        std::fs::write(
            &marker,
            "staged for restart: pid 1\nstaged_version=0.2.480\n",
        )
        .unwrap();

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
        assert!(!jetbrains_plugin_staged_in(
            &[plugins.clone()],
            &[],
            "0.2.480"
        ));
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

        // The staging IDE still runs the old generation from the plugin tree.
        let lib = plugins.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("agent-doc-jetbrains-0.2.458.jar"), b"live").unwrap();
        std::fs::write(
            plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
            "staged for restart\nstaged_version=0.2.460\n",
        )
        .unwrap();
        assert!(jetbrains_plugin_staged_in(&dirs, &roots, "0.2.460"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// GH #115: a staged copy is unique per staging; its version still parses.
    #[test]
    fn unique_staged_zip_names_parse_to_their_version() {
        assert_eq!(
            staged_zip_version(Path::new("/c/agent-doc-jetbrains-0.2.481+1a2b3c.zip")).as_deref(),
            Some("0.2.481")
        );
        assert_eq!(
            staged_zip_version(Path::new("/c/agent-doc-jetbrains-0.2.481.zip")).as_deref(),
            Some("0.2.481")
        );
        assert_eq!(staged_zip_version(Path::new("/c/other-1.0.zip")), None);
        let script = "unzip:/c/agent-doc-jetbrains-0.2.481+ff00.zip:/d/IntelliJIdea2026.3\n";
        assert_eq!(staged_versions_from_action_script(script), vec!["0.2.481"]);
    }

    fn gh115_fixture(tag: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("adoc-gh115-{tag}-{}", std::process::id()));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp).unwrap();
        }
        let plugins = tmp.join("data/IntelliJIdea2026.3");
        let system = tmp.join("cache");
        let script_dir = system.join("IntelliJIdea2026.3/plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(&script_dir).unwrap();
        (tmp, plugins, system, script_dir)
    }

    /// GH #115: marker staged, script consumed, no plugin dir -> destroyed. The
    /// GH #108 retirement must keep the marker, and the version no longer reads
    /// as "staged, just restart".
    #[test]
    fn destroyed_staged_install_is_reported_and_its_marker_kept() {
        let (tmp, plugins, system, _) = gh115_fixture("destroyed");
        let marker = plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(
            &marker,
            "dynamic upgrade unavailable, staged for restart: pid 9\nstaged_version=0.2.481\nprevious_version=0.2.480\n",
        )
        .unwrap();
        let roots = vec![system.clone()];

        let failure = staged_install_failure(&plugins, &roots);
        assert_eq!(
            failure,
            Some(StagedInstallFailure::Destroyed {
                staged: "0.2.481".to_string(),
                previous: Some("0.2.480".to_string()),
            })
        );
        let message = staged_install_failure_message(&plugins, failure.as_ref().unwrap());
        assert!(message.contains("DESTROYED"), "{message}");
        assert!(message.contains("over v0.2.480"), "{message}");
        assert!(
            message.contains(&format!("--plugins-dir {}", plugins.display())),
            "{message}"
        );

        assert!(!retire_satisfied_restart_marker(&plugins));
        assert!(!jetbrains_plugin_staged_in(
            &[plugins.clone()],
            &roots,
            "0.2.481"
        ));
        assert!(marker.exists(), "a destroyed staging must keep its marker");

        // An empty plugin directory (no jar) is the same state.
        std::fs::create_dir_all(plugins.join("agent-doc-jetbrains/lib")).unwrap();
        assert!(matches!(
            staged_install_failure(&plugins, &roots),
            Some(StagedInstallFailure::Destroyed { .. })
        ));
        assert!(marker.exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// GH #115: the restart has not happened yet: a pending staging with its
    /// package present is fine; one whose package vanished is doomed.
    #[test]
    fn pending_staging_is_viable_only_while_its_package_exists() {
        let (tmp, plugins, system, script_dir) = gh115_fixture("pending");
        let roots = vec![system.clone()];
        let lib = plugins.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("agent-doc-jetbrains-0.2.480.jar"), b"live").unwrap();
        std::fs::write(
            plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
            "staged for restart\nstaged_version=0.2.481\n",
        )
        .unwrap();
        let zip = script_dir.join("agent-doc-jetbrains-0.2.481+abc.zip");
        std::fs::write(
            script_dir.join("action.script"),
            format!(
                "delete:{p}/agent-doc-jetbrains\ndelete:{p}/agent-doc-jetbrains\nunzip:{z}:{p}/\ndelete:{z}\n",
                p = plugins.display(),
                z = zip.display()
            ),
        )
        .unwrap();

        // Package missing: the restart would delete and not unzip.
        assert_eq!(
            staged_install_failure(&plugins, &roots),
            Some(StagedInstallFailure::Doomed {
                staged: "0.2.481".to_string(),
                zip: zip.clone(),
            })
        );

        std::fs::write(&zip, b"pkg").unwrap();
        assert_eq!(staged_install_failure(&plugins, &roots), None);
        let pending = pending_stagings_for(&plugins, &roots);
        assert_eq!(pending.len(), 1);
        assert!(pending[0].zip_present);
        assert_eq!(pending[0].version, "0.2.481");
        assert!(jetbrains_plugin_staged_in(
            &[plugins.clone()],
            &roots,
            "0.2.481"
        ));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// GH #115: a staging that was applied still retires its marker (GH #108)
    /// and is never classified as a failure.
    #[test]
    fn applied_staging_retires_its_marker_and_is_no_failure() {
        let (tmp, plugins, system, _) = gh115_fixture("applied");
        let marker = plugins.join(crate::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(
            &marker,
            "staged\nstaged_version=0.2.481\nprevious_version=0.2.480\n",
        )
        .unwrap();
        let lib = plugins.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        let jar = lib.join("agent-doc-jetbrains-0.2.481.jar");
        std::fs::write(&jar, b"new").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&jar)
            .unwrap()
            .set_modified(SystemTime::now() + std::time::Duration::from_secs(1))
            .unwrap();
        let roots = vec![system.clone()];
        assert_eq!(staged_install_failure(&plugins, &roots), None);
        assert!(!jetbrains_plugin_staged_in(
            &[plugins.clone()],
            &roots,
            "0.2.481"
        ));
        assert!(!marker.exists(), "an applied staging retires its marker");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// `#jbpluginvanish`: the 2026-10-04 13:48 incident. A leftover staging of
    /// 0.2.468 (fixed package name, package gone) survived a restart-free
    /// upgrade to 0.2.488; the next IDE start ran its `delete:` and failed its
    /// `unzip:` ("Source file missing"), leaving no plugin. Once the tree holds
    /// a fresh generation, every pending agent-doc staging is purged, and other
    /// plugins' commands are kept verbatim.
    #[test]
    fn purge_all_removes_a_leftover_staging_after_a_fresh_install() {
        let (tmp, plugins, system, script_dir) = gh115_fixture("purge-all");
        let roots = vec![system.clone()];
        let lib = plugins.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("agent-doc-jetbrains-0.2.488.jar"), b"live").unwrap();
        let zip = script_dir.join("agent-doc-jetbrains-0.2.468.zip");
        let other = script_dir.join("IdeaVIM.zip");
        let script = script_dir.join("action.script");
        std::fs::write(
            &script,
            format!(
                "delete:{p}/IdeaVIM\nunzip:{o}:{p}\ndelete:{p}/agent-doc-jetbrains\nunzip:{z}:{p}\ndelete:{z}\n",
                p = plugins.display(),
                o = other.display(),
                z = zip.display()
            ),
        )
        .unwrap();
        assert!(matches!(
            staged_install_failure(&plugins, &roots),
            Some(StagedInstallFailure::Doomed { .. })
        ));

        let purged = purge_pending_stagings_for(&plugins, &roots, StagingPurge::All).unwrap();
        assert_eq!(purged.len(), 1);
        assert_eq!(purged[0].zips, vec![zip.clone()]);
        assert_eq!(purged[0].removed_lines, 3);
        assert_eq!(
            std::fs::read_to_string(&script).unwrap(),
            format!(
                "delete:{p}/IdeaVIM\nunzip:{o}:{p}\n",
                p = plugins.display(),
                o = other.display()
            )
        );
        assert!(pending_stagings_for(&plugins, &roots).is_empty());
        assert_eq!(staged_install_failure(&plugins, &roots), None);
        assert!(lib.join("agent-doc-jetbrains-0.2.488.jar").is_file());

        // Nothing left to purge: a second pass is a no-op.
        assert!(
            purge_pending_stagings_for(&plugins, &roots, StagingPurge::All)
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// `#jbpluginvanish`: the doomed-only purge keeps a viable staging and its
    /// plugin-dir delete, removes the doomed one, and drops an emptied script.
    #[test]
    fn purge_doomed_keeps_a_viable_staging() {
        let (tmp, plugins, system, script_dir) = gh115_fixture("purge-doomed");
        let roots = vec![system.clone()];
        let script = script_dir.join("action.script");
        let gone = script_dir.join("agent-doc-jetbrains-0.2.480+aa.zip");
        let viable = script_dir.join("agent-doc-jetbrains-0.2.481+bb.zip");
        std::fs::write(&viable, b"pkg").unwrap();
        let p = plugins.display();
        std::fs::write(
            &script,
            format!(
                "delete:{p}/agent-doc-jetbrains\nunzip:{g}:{p}\ndelete:{g}\ndelete:{p}/agent-doc-jetbrains\nunzip:{v}:{p}\ndelete:{v}\n",
                g = gone.display(),
                v = viable.display()
            ),
        )
        .unwrap();
        let purged = purge_pending_stagings_for(&plugins, &roots, StagingPurge::Doomed).unwrap();
        assert_eq!(purged.len(), 1);
        assert_eq!(purged[0].zips, vec![gone.clone()]);
        let left = std::fs::read_to_string(&script).unwrap();
        assert_eq!(
            left,
            format!(
                "delete:{p}/agent-doc-jetbrains\ndelete:{p}/agent-doc-jetbrains\nunzip:{v}:{p}\ndelete:{v}\n",
                v = viable.display()
            )
        );
        assert!(viable.is_file(), "a viable staging keeps its package");
        assert_eq!(staged_install_failure(&plugins, &roots), None);

        // Its package vanishes too: the whole script goes.
        std::fs::remove_file(&viable).unwrap();
        purge_pending_stagings_for(&plugins, &roots, StagingPurge::Doomed).unwrap();
        assert!(!script.exists(), "an emptied script is removed");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// `#jbpluginvanish`: a script this module cannot fully parse is never
    /// rewritten.
    #[test]
    fn purge_leaves_an_unparseable_script_alone() {
        let (tmp, plugins, system, script_dir) = gh115_fixture("purge-opaque");
        let roots = vec![system.clone()];
        let script = script_dir.join("action.script");
        let zip = script_dir.join("agent-doc-jetbrains-0.2.468.zip");
        let body = format!(
            "delete:{p}/agent-doc-jetbrains\nunzip:{z}:{p}\nfuture-command:x\n",
            p = plugins.display(),
            z = zip.display()
        );
        std::fs::write(&script, &body).unwrap();
        assert!(
            purge_pending_stagings_for(&plugins, &roots, StagingPurge::All)
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read_to_string(&script).unwrap(), body);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    /// GH #115: two installs into one plugins directory serialize.
    #[test]
    fn concurrent_installs_into_one_plugins_dir_serialize() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (tmp, plugins, _, _) = gh115_fixture("lock");
        let first = lock_jetbrains_install(&plugins).unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let waiter = {
            let plugins = plugins.clone();
            let released = Arc::clone(&released);
            std::thread::spawn(move || {
                let _second = lock_jetbrains_install(&plugins).unwrap();
                released.load(Ordering::SeqCst)
            })
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(
            !waiter.is_finished(),
            "second install must wait for the first"
        );
        released.store(true, Ordering::SeqCst);
        drop(first);
        assert!(
            waiter.join().unwrap(),
            "second install acquired the lock only after the first released it"
        );
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
