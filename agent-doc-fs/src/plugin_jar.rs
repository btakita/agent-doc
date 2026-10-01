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

pub fn prefer_mapped_plugin_jar(best: MappedPluginJar, candidate: MappedPluginJar) -> MappedPluginJar {
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
