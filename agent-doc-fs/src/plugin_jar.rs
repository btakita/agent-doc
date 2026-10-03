//! `#pluginbyteidentity` / GH #76: what a live editor process actually has
//! mapped for its plugin jar. Lives in this leaf crate so both preflight
//! warnings and the controller's `editor_route` admission read the same
//! evidence from one implementation.

/// `#pluginbyteidentity`: what a live editor process actually has *mapped* for
/// its plugin jar, as opposed to what it *reports* as its version.
///
/// [`stale_plugin_warnings`] compares version strings, which is blind to the
/// dominant real-world shape: `make install` rewrites the jar at the same path
/// under the same version number, so a running IDE keeps executing the
/// superseded bytes while every version comparison reports "current". Measured
/// 2026-09-20 on this project: IDEA pid 1023909 mapped
/// `agent-doc-jetbrains-0.2.388.jar (deleted)` while disk held a different
/// inode, both labelled `0.2.388`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappedPluginJar {
    /// The mapped jar's backing inode was unlinked. The install replaced the
    /// file and the process still executes the old bytes. Definitive.
    Deleted { path: String },
    /// Mapped, still present, but a different inode than the jar now at that
    /// path — the same replacement seen through a filesystem that reused the
    /// name without unlinking what we mapped.
    Superseded {
        path: String,
        mapped_inode: u64,
        disk_inode: u64,
    },
    /// Mapped and byte-identical to what is on disk.
    Current { path: String, inode: u64 },
    /// Nothing conclusive: no jar mapped, an unreadable `map_files`, or a
    /// platform without `/proc`. Fails open — never manufactures a warning.
    Unknown,
}

impl MappedPluginJar {
    /// Whether the live process is running bytes that are no longer the
    /// installed ones.
    pub fn is_superseded(&self) -> bool {
        matches!(
            self,
            MappedPluginJar::Deleted { .. } | MappedPluginJar::Superseded { .. }
        )
    }
}

/// Pure classifier for one mapped jar.
///
/// `mapped_link` is the raw `readlink` of a `/proc/<pid>/map_files/<range>`
/// entry; the kernel appends `" (deleted)"` when the backing inode is gone,
/// and that suffix is the whole signal — no stat can recover it afterwards.
pub fn classify_mapped_plugin_jar(
    mapped_link: &str,
    mapped_inode: Option<u64>,
    disk_inode: Option<u64>,
) -> MappedPluginJar {
    if let Some(path) = mapped_link.strip_suffix(" (deleted)") {
        return MappedPluginJar::Deleted {
            path: path.to_string(),
        };
    }
    match (mapped_inode, disk_inode) {
        (Some(mapped), Some(disk)) if mapped != disk => MappedPluginJar::Superseded {
            path: mapped_link.to_string(),
            mapped_inode: mapped,
            disk_inode: disk,
        },
        (Some(mapped), Some(_)) => MappedPluginJar::Current {
            path: mapped_link.to_string(),
            inode: mapped,
        },
        // `/proc/<pid>/map_files/<range>` cannot be stat'd without
        // CAP_SYS_ADMIN — even by the owner of the process — so `mapped_inode`
        // is `None` on every ordinary run and the arms above are unreachable
        // outside a privileged probe. The kernel only appends `" (deleted)"`
        // when the mapping's backing inode has been unlinked, and that case
        // returned above; reaching here with the jar still present on disk is
        // therefore positive proof that this mapping is the live generation.
        // Classifying it `Unknown` instead let a lingering `(deleted)` mapping
        // from an already-replaced generation win the fold in
        // `prefer_mapped_plugin_jar`, which is how a healthy IDE was told to
        // reinstall a plugin it was already running.
        (None, Some(disk)) => MappedPluginJar::Current {
            path: mapped_link.to_string(),
            inode: disk,
        },
        // A jar we can stat on neither side, or whose path has since vanished,
        // proves nothing.
        _ => MappedPluginJar::Unknown,
    }
}

pub fn prefer_mapped_plugin_jar(
    best: MappedPluginJar,
    candidate: MappedPluginJar,
) -> MappedPluginJar {
    if matches!(candidate, MappedPluginJar::Current { .. })
        || matches!(best, MappedPluginJar::Unknown) && candidate.is_superseded()
    {
        candidate
    } else {
        best
    }
}

/// Probe one process's mapped plugin jar. Linux-only; every other platform and
/// every IO error yields [`MappedPluginJar::Unknown`].
pub fn probe_mapped_plugin_jar(pid: u32, jar_stem: &str) -> MappedPluginJar {
    let map_files = std::path::PathBuf::from(format!("/proc/{pid}/map_files"));
    let Ok(entries) = std::fs::read_dir(&map_files) else {
        return MappedPluginJar::Unknown;
    };
    let mut best = MappedPluginJar::Unknown;
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let link = target.to_string_lossy().into_owned();
        let stem = link.strip_suffix(" (deleted)").unwrap_or(&link);
        if !std::path::Path::new(stem)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(jar_stem) && name.ends_with(".jar"))
        {
            continue;
        }
        let mapped_inode = inode_of(&entry.path());
        let disk_inode = inode_of(std::path::Path::new(stem));
        let classified = classify_mapped_plugin_jar(&link, mapped_inode, disk_inode);
        best = prefer_mapped_plugin_jar(best, classified);
        if matches!(best, MappedPluginJar::Current { .. }) {
            // Dynamic unload can leave the old classloader's mmap alive until
            // GC while the replacement generation is already active. A mapped
            // current jar proves this process loaded the installed generation;
            // a historical `(deleted)` mapping no longer proves stale execution.
            return best;
        }
    }
    best
}

#[cfg(unix)]
fn inode_of(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|meta| meta.ino())
}

#[cfg(not(unix))]
fn inode_of(_path: &std::path::Path) -> Option<u64> {
    None
}

/// GH #67: file written into the IDE's plugins directory, beside (never inside) the
/// `agent-doc-jetbrains` tree, when an install had to fall back to a file replacement
/// because the live IDE refused the restart-free upgrade. Its body is the refusal
/// reason. It lives outside the plugin tree so the install's byte-identity check
/// never counts it as package content.
pub const PLUGIN_RESTART_REQUIRED_MARKER: &str = ".agent-doc-jetbrains-restart-required";

/// Jar filename prefix for an editor kind. Only editors that load agent-doc as a
/// jar inside their own process can be probed this way.
pub fn plugin_jar_stem(editor_kind: &str) -> Option<&'static str> {
    match editor_kind.to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" => Some("agent-doc-jetbrains-"),
        _ => None,
    }
}

/// The refusal an install recorded next to the jar this process maps, when that
/// record is newer than the process (so it was this process that refused).
pub fn recorded_restart_verdict(pid: u32, mapped: &MappedPluginJar) -> Option<String> {
    let path = match mapped {
        MappedPluginJar::Deleted { path } | MappedPluginJar::Superseded { path, .. } => path,
        _ => return None,
    };
    // <plugins>/agent-doc-jetbrains/lib/<jar>
    let plugins_dir = std::path::Path::new(path.trim_end_matches(" (deleted)"))
        .parent()?
        .parent()?
        .parent()?;
    let marker = plugins_dir.join(PLUGIN_RESTART_REQUIRED_MARKER);
    let recorded_at = std::fs::metadata(&marker).ok()?.modified().ok()?;
    let started_at = std::fs::metadata(format!("/proc/{pid}"))
        .ok()?
        .modified()
        .ok()?;
    if recorded_at < started_at {
        return None;
    }
    let reason = std::fs::read_to_string(&marker).ok()?;
    Some(reason.lines().next().unwrap_or("").to_string()).filter(|r| !r.trim().is_empty())
}

/// What a superseded mapping is, in words: which jar, and how it was replaced.
/// `None` for a mapping that is not superseded.
pub fn superseded_mapping_detail(mapped: &MappedPluginJar) -> Option<String> {
    match mapped {
        MappedPluginJar::Deleted { path } => {
            Some(format!("{path} is mapped but its inode was unlinked"))
        }
        MappedPluginJar::Superseded {
            path,
            mapped_inode,
            disk_inode,
        } => Some(format!(
            "{path} is mapped as inode {mapped_inode} but disk now holds inode {disk_inode}"
        )),
        _ => None,
    }
}

/// GH #67 / #76 / #84: the recovery ladder for an editor running superseded
/// plugin bytes. The first rung is deliberately restart-free (the install's
/// dynamic update, GH #67); a restart is advised only once an install has
/// recorded that THIS process refused that update (`restart_verdict`), because
/// re-running the install there can never converge.
pub fn superseded_editor_remedy(restart_verdict: Option<&str>) -> String {
    match restart_verdict {
        Some(reason) => format!(
            "The last install already recorded that this process cannot take the restart-free \
             upgrade ({}), so another install cannot converge: the plugin files on disk are \
             current. Restart the editor to load them.",
            reason.trim()
        ),
        None => "Re-run the plugin installation once so its dynamic update transaction \
                 can converge. Restart the editor only if that install explicitly reports \
                 a dynamic-unload failure."
            .to_string(),
    }
}

/// One live editor proven to be executing superseded plugin bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersededEditor {
    pub editor_kind: String,
    pub pid: u32,
    pub detail: String,
    pub restart_verdict: Option<String>,
}

impl SupersededEditor {
    pub fn remedy(&self) -> String {
        superseded_editor_remedy(self.restart_verdict.as_deref())
    }
}

/// Probe each `(editor_kind, pid)` once and return the editors proven to run
/// superseded plugin bytes, with any restart verdict an install recorded for
/// them. Inconclusive probes fail open (are omitted).
pub fn probe_superseded_editors(
    editors: impl IntoIterator<Item = (String, u32)>,
) -> Vec<SupersededEditor> {
    let mut probed = std::collections::HashSet::new();
    let mut superseded = Vec::new();
    for (editor_kind, pid) in editors {
        let Some(jar_stem) = plugin_jar_stem(&editor_kind) else {
            continue;
        };
        if pid == 0 || !probed.insert(pid) {
            continue;
        }
        let mapped = probe_mapped_plugin_jar(pid, jar_stem);
        let Some(detail) = superseded_mapping_detail(&mapped) else {
            continue;
        };
        superseded.push(SupersededEditor {
            editor_kind,
            pid,
            detail,
            restart_verdict: recorded_restart_verdict(pid, &mapped),
        });
    }
    superseded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GH #84: without a recorded refusal the ladder starts restart-free; with
    /// one, it advises the restart and names the recorded reason.
    #[test]
    fn superseded_editor_remedy_derives_restart_from_the_recorded_refusal() {
        let restart_free = superseded_editor_remedy(None);
        assert!(restart_free.contains("Re-run the plugin installation once"));
        assert!(!restart_free.contains("Restart the editor to load them"));

        let restart = superseded_editor_remedy(Some("plugin cannot unload dynamically\n"));
        assert!(
            restart.contains("Restart the editor to load them"),
            "{restart}"
        );
        assert!(
            restart.contains("(plugin cannot unload dynamically)"),
            "{restart}"
        );
    }

    #[test]
    fn superseded_mapping_detail_names_only_superseded_mappings() {
        let deleted = MappedPluginJar::Deleted {
            path: "/p/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.455.jar".into(),
        };
        assert!(
            superseded_mapping_detail(&deleted)
                .unwrap()
                .contains("unlinked")
        );
        assert_eq!(superseded_mapping_detail(&MappedPluginJar::Unknown), None);
        let current = MappedPluginJar::Current {
            path: "/p/x.jar".into(),
            inode: 1,
        };
        assert_eq!(superseded_mapping_detail(&current), None);
    }

    #[test]
    fn probe_superseded_editors_skips_non_jar_editors_and_pid_zero() {
        let found = probe_superseded_editors([
            ("vscode".to_string(), std::process::id()),
            ("jetbrains".to_string(), 0),
        ]);
        assert!(found.is_empty());
    }
}
