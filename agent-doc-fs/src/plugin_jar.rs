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
    /// `#jb263probeblind`: mapped from the installer's private backup tree
    /// (`<plugins>/.agent-doc-jetbrains-install/backup-*/`) while that tree
    /// still exists. The atomic tree swap renamed the live package aside and
    /// moved the replacement in, so the path no longer names the installed
    /// package even though the file is still linked. Definitive.
    Displaced { path: String },
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
            MappedPluginJar::Deleted { .. }
                | MappedPluginJar::Superseded { .. }
                | MappedPluginJar::Displaced { .. }
        )
    }

    /// The mapped path of a superseded mapping. `None` for a current or
    /// inconclusive one.
    pub fn superseded_path(&self) -> Option<&str> {
        match self {
            MappedPluginJar::Deleted { path }
            | MappedPluginJar::Superseded { path, .. }
            | MappedPluginJar::Displaced { path } => Some(path),
            MappedPluginJar::Current { .. } | MappedPluginJar::Unknown => None,
        }
    }
}

/// `#jb263probeblind`: the private work directory the JetBrains installer's
/// atomic tree swap renames the live package into (`backup-*`) before it moves
/// the replacement in and deletes the backup.
pub const JETBRAINS_INSTALL_WORK_DIR: &str = ".agent-doc-jetbrains-install";

/// Whether `path` lies inside an installer backup tree
/// (`<plugins>/.agent-doc-jetbrains-install/backup-<nonce>/...`).
pub fn is_installer_backup_path(path: &std::path::Path) -> bool {
    path.ancestors().any(|dir| {
        dir.file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("backup-"))
            .is_some_and(|nonce| !nonce.is_empty())
            && dir
                .parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|name| name == JETBRAINS_INSTALL_WORK_DIR)
    })
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
    // `#jb263probeblind`: a still-linked mapping under the installer's backup
    // tree is the package the swap renamed aside, never the installed one —
    // even though the file is present, so the inode arms below would call it
    // current.
    if is_installer_backup_path(std::path::Path::new(mapped_link)) {
        return MappedPluginJar::Displaced {
            path: mapped_link.to_string(),
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

/// `#jb262dynupgrade`: versioned plugin-jar filename prefixes a live JetBrains
/// process can map. The classic 242-261 package ships
/// `agent-doc-jetbrains-<v>.jar`; the exact-262 modular package ships its root
/// descriptor jar as `agent.doc-<v>.jar` (its `lib/modules/agent.doc.*.jar`
/// module jars are unversioned and never match a stem — see
/// [`is_jetbrains_plugin_module_jar`]); and the 263 package ships
/// `agent-doc-jetbrains-263-<v>.jar`. A probe missing a package-specific prefix
/// reads that IDE line as "no plugin mapped".
pub const JETBRAINS_PLUGIN_JAR_STEMS: &[&str] = &[
    "agent-doc-jetbrains-263-",
    "agent-doc-jetbrains-",
    "agent.doc-",
];

/// The version a plugin jar filename carries under one of `jar_stems`:
/// `<stem><digits>(.<digits>)*.jar`. Anything else (another plugin, an
/// unversioned module jar, a non-numeric suffix) is not an agent-doc plugin jar.
pub fn plugin_jar_version<'a>(file_name: &'a str, jar_stems: &[&str]) -> Option<&'a str> {
    jar_stems.iter().find_map(|stem| {
        let version = file_name.strip_prefix(stem)?.strip_suffix(".jar")?;
        (!version.is_empty()
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit())))
        .then_some(version)
    })
}

/// The root-jar stem of the modular (Plugin Model v2) package line. Its code
/// lives in unversioned `lib/modules/agent.doc.*.jar` content-module jars.
pub const JETBRAINS_MODULAR_ROOT_JAR_STEM: &str = "agent.doc-";

/// `#jb263probeblind`: whether `path` is one of the modular package's
/// unversioned content-module jars, `<tree>/lib/modules/agent.doc.<module>.jar`
/// (`backend`, `frontend`, `shared`). The exact-263 IDE maps ONLY these — its
/// descriptor-only root jar `agent.doc-<v>.jar` is read, not mapped — so a
/// probe restricted to versioned jar names is blind to that IDE.
pub fn is_jetbrains_plugin_module_jar(path: &std::path::Path) -> bool {
    let Some(module) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("agent.doc."))
        .and_then(|name| name.strip_suffix(".jar"))
    else {
        return false;
    };
    let modules_dir = path.parent();
    !module.is_empty()
        && module
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && modules_dir
            .and_then(|dir| dir.file_name())
            .is_some_and(|name| name == "modules")
        && modules_dir
            .and_then(std::path::Path::parent)
            .and_then(|dir| dir.file_name())
            .is_some_and(|name| name == "lib")
}

/// Pure fold over one process's mapped files: `(readlink target, mapped inode)`
/// per `/proc/<pid>/map_files` entry, with `disk_inode` resolving the inode now
/// at a path.
///
/// Root jars (named by `jar_stems`) fold as they always have: a current mapping
/// wins, because dynamic unload can leave the old classloader's mmap alive until
/// GC while the replacement generation is already active.
///
/// `#jb263probeblind`: when `jar_stems` covers the modular package line, its
/// content-module jars are classified too and fold among themselves the same
/// way. A superseded module verdict outranks the root verdict: the modular
/// package lines have no restart-free upgrade (`declined_by=modular_package`),
/// so no newer module generation can be live in this process, and their root
/// jar is descriptor-only — a current root mapping proves only that something
/// read the installed descriptor, not which code executes. A current module
/// mapping carries no version, so it never replaces the root verdict: on an IDE
/// that maps only current module jars the probe stays `Unknown`, exactly as
/// before, and every version-reading caller is unchanged.
pub fn fold_mapped_plugin_jars(
    mapped_files: impl IntoIterator<Item = (String, Option<u64>)>,
    jar_stems: &[&str],
    disk_inode: impl Fn(&std::path::Path) -> Option<u64>,
) -> MappedPluginJar {
    let scan_modules = jar_stems.contains(&JETBRAINS_MODULAR_ROOT_JAR_STEM);
    let mut root = MappedPluginJar::Unknown;
    let mut modules = MappedPluginJar::Unknown;
    for (link, mapped_inode) in mapped_files {
        let stem = std::path::Path::new(link.strip_suffix(" (deleted)").unwrap_or(&link));
        let is_root = stem
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| plugin_jar_version(name, jar_stems).is_some());
        if is_root {
            if matches!(root, MappedPluginJar::Current { .. }) {
                // The first current root mapping is the verdict.
                continue;
            }
            let classified = classify_mapped_plugin_jar(&link, mapped_inode, disk_inode(stem));
            root = prefer_mapped_plugin_jar(root, classified);
        } else if scan_modules && is_jetbrains_plugin_module_jar(stem) {
            let classified = classify_mapped_plugin_jar(&link, mapped_inode, disk_inode(stem));
            modules = prefer_mapped_plugin_jar(modules, classified);
        }
    }
    if modules.is_superseded() {
        modules
    } else {
        root
    }
}

/// Probe one process's mapped plugin jar. Linux-only; every other platform and
/// every IO error yields [`MappedPluginJar::Unknown`].
pub fn probe_mapped_plugin_jar(pid: u32, jar_stems: &[&str]) -> MappedPluginJar {
    probe_mapped_plugin_jar_at(std::path::Path::new("/proc"), pid, jar_stems)
}

/// Proc-root-parameterized core of [`probe_mapped_plugin_jar`], so a
/// regression can drive the real probe over a `map_files` fixture.
pub fn probe_mapped_plugin_jar_at(
    proc_root: &std::path::Path,
    pid: u32,
    jar_stems: &[&str],
) -> MappedPluginJar {
    let map_files = proc_root.join(pid.to_string()).join("map_files");
    let Ok(entries) = std::fs::read_dir(&map_files) else {
        return MappedPluginJar::Unknown;
    };
    let mapped_files = entries.flatten().filter_map(|entry| {
        let target = std::fs::read_link(entry.path()).ok()?;
        Some((
            target.to_string_lossy().into_owned(),
            inode_of(&entry.path()),
        ))
    });
    fold_mapped_plugin_jars(mapped_files, jar_stems, inode_of)
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

/// Jar filename prefixes for an editor kind. Only editors that load agent-doc as
/// a jar inside their own process can be probed this way.
pub fn plugin_jar_stems(editor_kind: &str) -> Option<&'static [&'static str]> {
    match editor_kind.to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" => Some(JETBRAINS_PLUGIN_JAR_STEMS),
        _ => None,
    }
}

/// The plugins directory that owns one mapped plugin jar. Atomic replacement
/// normally leaves the mapping under the installed tree, but `#jbpluginvanish`
/// moves the old tree to `<plugins>/.agent-doc-jetbrains-install/backup-*/`.
/// Both paths still belong to the same plugins directory and restart marker.
fn plugins_dir_for_mapped_jar(path: &str) -> Option<&std::path::Path> {
    let jar = std::path::Path::new(path.trim_end_matches(" (deleted)"));
    let mut lib = jar.parent()?;
    if is_jetbrains_plugin_module_jar(jar) {
        // `#jb263probeblind`: <tree>/lib/modules/agent.doc.<module>.jar
        lib = lib.parent()?;
    }
    let tree = lib.parent()?;
    let parent = tree.parent()?;
    if tree
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("backup-"))
        && parent
            .file_name()
            .is_some_and(|name| name == JETBRAINS_INSTALL_WORK_DIR)
    {
        parent.parent()
    } else {
        // <plugins>/agent-doc-jetbrains[-263]/lib/<jar>
        Some(parent)
    }
}

/// The refusal an install recorded in `plugins_dir`, when that record is newer
/// than the process (so it was this process that refused).
///
/// The process start time is the kernel's `starttime` (`/proc/<pid>/stat`
/// field 22) plus boot time, never the `/proc/<pid>` directory mtime: procfs
/// stamps that when it instantiates the inode, roughly "now", which made every
/// recorded verdict look older than its process (GH #263). When the start time
/// cannot be read the verdict is withheld (`None`): without proof that the
/// marker post-dates this process, the restart-free ladder is the safe advice.
pub fn recorded_restart_verdict_for_plugins_dir(
    pid: u32,
    plugins_dir: &std::path::Path,
) -> Option<String> {
    recorded_restart_verdict_for_plugins_dir_at(pid, plugins_dir, std::path::Path::new("/proc"))
}

/// Proc-root-parameterized core used by deterministic installer regressions.
pub fn recorded_restart_verdict_for_plugins_dir_at(
    pid: u32,
    plugins_dir: &std::path::Path,
    proc_root: &std::path::Path,
) -> Option<String> {
    let marker = plugins_dir.join(PLUGIN_RESTART_REQUIRED_MARKER);
    let recorded_at = std::fs::metadata(&marker).ok()?.modified().ok()?;
    let started_at = crate::process_start::process_start_time_at(proc_root, pid)?;
    if recorded_at < started_at {
        return None;
    }
    let reason = std::fs::read_to_string(&marker).ok()?;
    Some(reason.lines().next().unwrap_or("").to_string()).filter(|r| !r.trim().is_empty())
}

/// The refusal an install recorded next to the jar this process maps, when that
/// record is newer than the process (so it was this process that refused).
pub fn recorded_restart_verdict(pid: u32, mapped: &MappedPluginJar) -> Option<String> {
    recorded_restart_verdict_at(pid, mapped, std::path::Path::new("/proc"))
}

/// Proc-root-parameterized mapped-jar lookup used by deterministic regressions.
pub fn recorded_restart_verdict_at(
    pid: u32,
    mapped: &MappedPluginJar,
    proc_root: &std::path::Path,
) -> Option<String> {
    let path = mapped.superseded_path()?;
    recorded_restart_verdict_for_plugins_dir_at(pid, plugins_dir_for_mapped_jar(path)?, proc_root)
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
        MappedPluginJar::Displaced { path } => Some(format!(
            "{path} is mapped from the installer's backup tree; the installed package was \
             swapped in under it"
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
    probe_superseded_editors_at(std::path::Path::new("/proc"), editors)
}

/// Proc-root-parameterized core of [`probe_superseded_editors`].
pub fn probe_superseded_editors_at(
    proc_root: &std::path::Path,
    editors: impl IntoIterator<Item = (String, u32)>,
) -> Vec<SupersededEditor> {
    let mut probed = std::collections::HashSet::new();
    let mut superseded = Vec::new();
    for (editor_kind, pid) in editors {
        let Some(jar_stems) = plugin_jar_stems(&editor_kind) else {
            continue;
        };
        if pid == 0 || !probed.insert(pid) {
            continue;
        }
        let mapped = probe_mapped_plugin_jar_at(proc_root, pid, jar_stems);
        let Some(detail) = superseded_mapping_detail(&mapped) else {
            continue;
        };
        superseded.push(SupersededEditor {
            editor_kind,
            pid,
            detail,
            restart_verdict: recorded_restart_verdict_at(pid, &mapped, proc_root),
        });
    }
    superseded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#jb262dynupgrade` / GH #251: live-jar probes match all published
    /// package lines, and nothing else.
    #[test]
    fn plugin_jar_version_matches_all_published_jetbrains_jars() {
        let stems = plugin_jar_stems("jetbrains").unwrap();
        assert_eq!(
            plugin_jar_version("agent-doc-jetbrains-0.2.513.jar", stems),
            Some("0.2.513")
        );
        assert_eq!(
            plugin_jar_version("agent.doc-0.2.514.jar", stems),
            Some("0.2.514")
        );
        assert_eq!(
            plugin_jar_version("agent-doc-jetbrains-263-0.2.515.jar", stems),
            Some("0.2.515")
        );
        for name in [
            "agent.doc.backend.jar",
            "agent.doc.shared.jar",
            "agent.doc-.jar",
            "agent.doc-0.2.514.zip",
            "agent-doc-jetbrains-262-0.2.514.jar",
            "agent-doc-jetbrains-0.2.x.jar",
            "lazily-0.39.0.jar",
            "agent-doc-jetbrains-0..1.jar",
        ] {
            assert_eq!(plugin_jar_version(name, stems), None, "{name}");
        }
    }

    /// GH #257: an atomic plugin swap moves the live jar into the private
    /// backup tree, but the refusal marker remains beside that tree in the IDE
    /// data/plugins directory. Both layouts must find the same verdict.
    #[test]
    fn recorded_restart_verdict_resolves_installed_and_backup_mappings() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("IntelliJIdea2026.3");
        let proc_root = root.path().join("proc");
        // Booted at 1_699_990_000, started 10_000 s later = 1_700_000_000.
        crate::process_start::fixture::write_fake_process_start(
            &proc_root,
            42,
            1_699_990_000,
            crate::process_start::fixture::ticks_after_boot(10_000),
        );
        std::fs::create_dir_all(&plugins).unwrap();
        let marker = plugins.join(PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(&marker, "exact-263 package requires restart\nmetadata\n").unwrap();
        filetime::set_file_mtime(
            &marker,
            filetime::FileTime::from_unix_time(1_700_000_100, 0),
        )
        .unwrap();

        assert_eq!(
            recorded_restart_verdict_for_plugins_dir_at(42, &plugins, &proc_root).as_deref(),
            Some("exact-263 package requires restart")
        );

        let installed = MappedPluginJar::Deleted {
            path: plugins
                .join("agent-doc-jetbrains-263/lib/agent-doc-jetbrains-263-0.2.592.jar")
                .display()
                .to_string(),
        };
        let backup = MappedPluginJar::Deleted {
            path: plugins
                .join(
                    ".agent-doc-jetbrains-install/backup-2095041-1792008622073994402/lib/agent-doc-jetbrains-263-0.2.577.jar",
                )
                .display()
                .to_string(),
        };
        let installed_path = match &installed {
            MappedPluginJar::Deleted { path } => path,
            _ => unreachable!(),
        };
        let backup_path = match &backup {
            MappedPluginJar::Deleted { path } => path,
            _ => unreachable!(),
        };
        assert_eq!(
            plugins_dir_for_mapped_jar(installed_path),
            Some(plugins.as_path())
        );
        assert_eq!(
            plugins_dir_for_mapped_jar(backup_path),
            Some(plugins.as_path())
        );
        assert_eq!(
            recorded_restart_verdict_at(42, &installed, &proc_root).as_deref(),
            Some("exact-263 package requires restart")
        );
        assert_eq!(
            recorded_restart_verdict_at(42, &backup, &proc_root).as_deref(),
            Some("exact-263 package requires restart")
        );

        let unrelated = plugins.join("other/backup-123/lib/agent-doc-jetbrains-0.2.1.jar");
        assert_ne!(
            plugins_dir_for_mapped_jar(&unrelated.display().to_string()),
            Some(plugins.as_path())
        );
    }

    /// GH #263: on Linux the `/proc/<pid>` directory mtime is when procfs made
    /// the inode (seconds before the read), not when the process started. A
    /// marker recorded after the real start (`starttime` + `btime`) must yield
    /// the verdict even though the pid directory looks brand new; a marker from
    /// before the real start must not.
    #[test]
    fn recorded_restart_verdict_uses_kernel_start_time_not_proc_dir_mtime() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("IntelliJIdea2026.3");
        let proc_root = root.path().join("proc");
        std::fs::create_dir_all(&plugins).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // The editor started two hours ago (booted a day before that) ...
        let started = now - 7_200;
        crate::process_start::fixture::write_fake_process_start(
            &proc_root,
            3_575_950,
            started - 86_400,
            crate::process_start::fixture::ticks_after_boot(86_400),
        );
        // ... while its /proc directory carries a fresh, read-time mtime.
        filetime::set_file_mtime(
            proc_root.join("3575950"),
            filetime::FileTime::from_unix_time(now as i64, 0),
        )
        .unwrap();
        let marker = plugins.join(PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(&marker, "modular package has no restart-free upgrade\n").unwrap();
        // The install recorded the refusal an hour ago: after the start, before
        // the procfs mtime.
        filetime::set_file_mtime(
            &marker,
            filetime::FileTime::from_unix_time((now - 3_600) as i64, 0),
        )
        .unwrap();
        let backup = MappedPluginJar::Deleted {
            path: plugins
                .join(".agent-doc-jetbrains-install/backup-1-2/lib/agent.doc-0.2.600.jar")
                .display()
                .to_string(),
        };
        assert_eq!(
            recorded_restart_verdict_at(3_575_950, &backup, &proc_root).as_deref(),
            Some("modular package has no restart-free upgrade")
        );

        // A marker from before the real start belongs to an earlier process.
        filetime::set_file_mtime(
            &marker,
            filetime::FileTime::from_unix_time((started - 60) as i64, 0),
        )
        .unwrap();
        assert_eq!(
            recorded_restart_verdict_at(3_575_950, &backup, &proc_root),
            None
        );

        // An unreadable start time withholds the verdict (restart-free ladder).
        std::fs::write(&marker, "modular package has no restart-free upgrade\n").unwrap();
        std::fs::remove_file(proc_root.join("3575950/stat")).unwrap();
        assert_eq!(
            recorded_restart_verdict_at(3_575_950, &backup, &proc_root),
            None
        );
    }

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

    /// `#jb263probeblind` fixture: a fake `/proc/<pid>/map_files` whose entries
    /// are symlinks to the given `readlink` targets, the same shape the kernel
    /// exposes (a `" (deleted)"` suffix makes the link dangle, as on the real
    /// procfs where the target is gone).
    fn write_map_files(proc_root: &std::path::Path, pid: u32, targets: &[String]) {
        let dir = proc_root.join(pid.to_string()).join("map_files");
        std::fs::create_dir_all(&dir).unwrap();
        for (index, target) in targets.iter().enumerate() {
            let range = format!("7f{index:04x}000-7f{index:04x}fff");
            std::os::unix::fs::symlink(target, dir.join(range)).unwrap();
        }
    }

    /// The exact-263 modular tree as installed: a descriptor-only root jar
    /// that the IDE never maps, and the three content-module jars it does map.
    fn write_modular_tree(tree: &std::path::Path, version: &str) -> Vec<String> {
        std::fs::create_dir_all(tree.join("lib/modules")).unwrap();
        std::fs::write(tree.join(format!("lib/agent.doc-{version}.jar")), version).unwrap();
        ["backend", "frontend", "shared"]
            .iter()
            .map(|module| {
                let jar = tree.join(format!("lib/modules/agent.doc.{module}.jar"));
                std::fs::write(&jar, format!("{module} {version}")).unwrap();
                jar.display().to_string()
            })
            .collect()
    }

    #[test]
    fn module_jar_matcher_names_only_modular_content_modules() {
        use std::path::Path;
        for path in [
            "/p/agent-doc-jetbrains/lib/modules/agent.doc.backend.jar",
            "/p/agent-doc-jetbrains/lib/modules/agent.doc.frontend.jar",
            "/p/.agent-doc-jetbrains-install/backup-1-2/lib/modules/agent.doc.shared.jar",
        ] {
            assert!(is_jetbrains_plugin_module_jar(Path::new(path)), "{path}");
        }
        for path in [
            "/p/agent-doc-jetbrains/lib/agent.doc-0.2.866.jar",
            "/p/agent-doc-jetbrains/lib/agent.doc.backend.jar",
            "/p/agent-doc-jetbrains/modules/agent.doc.backend.jar",
            "/p/agent-doc-jetbrains/lib/modules/agent.doc..jar",
            "/p/agent-doc-jetbrains/lib/modules/agent.doc.backend-1.jar",
            "/p/other/lib/modules/kotlinx.coroutines.jar",
        ] {
            assert!(!is_jetbrains_plugin_module_jar(Path::new(path)), "{path}");
        }
        assert!(is_installer_backup_path(Path::new(
            "/p/.agent-doc-jetbrains-install/backup-1-2/lib/modules/agent.doc.shared.jar"
        )));
        assert!(!is_installer_backup_path(Path::new(
            "/p/agent-doc-jetbrains/lib/modules/agent.doc.shared.jar"
        )));
        assert!(!is_installer_backup_path(Path::new(
            "/p/other/backup-1-2/lib/agent-doc-jetbrains-0.2.1.jar"
        )));
        assert!(!is_installer_backup_path(Path::new(
            "/p/.agent-doc-jetbrains-install/backup-/lib/agent-doc-jetbrains-0.2.1.jar"
        )));
    }

    /// `#jb263probeblind`: the live exact-263 IDE (pid 3639588, 2026-10-27)
    /// mapped only `lib/modules/agent.doc.{backend,frontend,shared}.jar`, never
    /// a versioned jar, so the probe returned `Unknown` and route admission
    /// passed silently while `restart_required pending=0.2.861` was outstanding.
    /// Module jars that the install swapped out must now classify superseded.
    #[test]
    fn exact_263_module_only_maps_classify_swapped_module_jars_superseded() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("IntelliJIdea2026.3");
        let installed = plugins.join("agent-doc-jetbrains");
        write_modular_tree(&installed, "0.2.866");
        let backup_tree = plugins.join(".agent-doc-jetbrains-install/backup-2095041-17920086");
        let stems = plugin_jar_stems("jetbrains").unwrap();
        let unrelated = vec![
            "/tmp/agent-doc-native-3049656/libagent_doc-1793126683196-1205553c.so".to_string(),
            "/opt/idea/lib/app-client.jar".to_string(),
        ];

        // 1. The production shape: the swap renamed the tree into the backup
        //    and then deleted it, so every module mapping reads `(deleted)`.
        let proc_root = root.path().join("proc-deleted");
        let mut targets = unrelated.clone();
        targets.extend(["backend", "frontend", "shared"].iter().map(|module| {
            format!(
                "{}/lib/modules/agent.doc.{module}.jar (deleted)",
                backup_tree.display()
            )
        }));
        write_map_files(&proc_root, 3_639_588, &targets);
        let mapped = probe_mapped_plugin_jar_at(&proc_root, 3_639_588, stems);
        assert!(
            matches!(&mapped, MappedPluginJar::Deleted { path }
                if path.contains("backup-2095041-17920086/lib/modules/agent.doc.")),
            "{mapped:?}"
        );
        assert!(superseded_mapping_detail(&mapped).is_some());
        // The versioned-jar stems alone (the pre-fix probe) are blind to it.
        assert_eq!(
            probe_mapped_plugin_jar_at(&proc_root, 3_639_588, &["agent-doc-jetbrains-"]),
            MappedPluginJar::Unknown
        );

        // 2. The backup tree is still on disk (swap in flight, or its removal
        //    failed): the links resolve, yet they no longer name the package.
        let backup_jars = write_modular_tree(&backup_tree, "0.2.861");
        let proc_root = root.path().join("proc-displaced");
        let mut targets = unrelated.clone();
        targets.extend(backup_jars.iter().cloned());
        write_map_files(&proc_root, 3_639_588, &targets);
        let mapped = probe_mapped_plugin_jar_at(&proc_root, 3_639_588, stems);
        assert!(
            matches!(&mapped, MappedPluginJar::Displaced { path }
                if backup_jars.contains(path)),
            "{mapped:?}"
        );
        assert!(
            superseded_mapping_detail(&mapped)
                .unwrap()
                .contains("installer's backup tree")
        );

        // 3. A module jar rewritten in place under the installed tree.
        let proc_root = root.path().join("proc-in-place");
        let mut targets = unrelated.clone();
        targets.push(format!(
            "{}/lib/modules/agent.doc.backend.jar (deleted)",
            installed.display()
        ));
        write_map_files(&proc_root, 3_639_588, &targets);
        assert!(
            probe_mapped_plugin_jar_at(&proc_root, 3_639_588, stems).is_superseded(),
            "an unlinked installed module jar is superseded"
        );

        // 4. A current descriptor-only root jar read after the install does not
        //    mask superseded module code (no restart-free upgrade exists).
        let proc_root = root.path().join("proc-root-current");
        let mut targets = vec![format!("{}/lib/agent.doc-0.2.866.jar", installed.display())];
        targets.push(format!(
            "{}/lib/modules/agent.doc.backend.jar (deleted)",
            backup_tree.display()
        ));
        write_map_files(&proc_root, 3_639_588, &targets);
        assert!(
            probe_mapped_plugin_jar_at(&proc_root, 3_639_588, stems).is_superseded(),
            "superseded module code outranks a current descriptor mapping"
        );
    }

    /// `#jb263probeblind`: current module jars prove nothing about a version,
    /// so they keep the probe `Unknown` (fail open, no advisory), and the
    /// classic / exact-262 root-jar layouts classify exactly as before.
    #[test]
    fn current_module_jars_stay_unknown_and_root_jar_layouts_are_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("IntelliJIdea2026.3");
        let installed = plugins.join("agent-doc-jetbrains");
        let module_jars = write_modular_tree(&installed, "0.2.866");
        let stems = plugin_jar_stems("jetbrains").unwrap();

        let proc_root = root.path().join("proc-current-263");
        write_map_files(&proc_root, 11, &module_jars);
        let mapped = probe_mapped_plugin_jar_at(&proc_root, 11, stems);
        assert_eq!(mapped, MappedPluginJar::Unknown);
        assert_eq!(superseded_mapping_detail(&mapped), None);
        assert!(
            probe_superseded_editors_at(&proc_root, [("jetbrains".to_string(), 11)]).is_empty()
        );

        // exact-262: the versioned root jar is mapped beside current modules.
        let root_jar = format!("{}/lib/agent.doc-0.2.866.jar", installed.display());
        let proc_root = root.path().join("proc-262");
        let mut targets = vec![root_jar.clone()];
        targets.extend(module_jars.iter().cloned());
        write_map_files(&proc_root, 12, &targets);
        assert!(matches!(probe_mapped_plugin_jar_at(&proc_root, 12, stems),
                MappedPluginJar::Current { path, .. } if path == root_jar));

        // classic: current, unlinked, and a lingering unlinked mapping beside
        // the current generation (dynamic unload) — all as before.
        let classic = plugins.join("classic/agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&classic).unwrap();
        let current = classic.join("agent-doc-jetbrains-0.2.513.jar");
        std::fs::write(&current, "classic").unwrap();
        let current = current.display().to_string();
        let old = format!(
            "{}/agent-doc-jetbrains-0.2.512.jar (deleted)",
            classic.display()
        );
        for (pid, targets, expect_current) in [
            (13, vec![current.clone()], true),
            (14, vec![old.clone()], false),
            (15, vec![old.clone(), current.clone()], true),
        ] {
            let proc_root = root.path().join(format!("proc-classic-{pid}"));
            write_map_files(&proc_root, pid, &targets);
            let mapped = probe_mapped_plugin_jar_at(&proc_root, pid, stems);
            if expect_current {
                assert!(
                    matches!(&mapped, MappedPluginJar::Current { path, .. } if *path == current),
                    "{pid}: {mapped:?}"
                );
            } else {
                assert!(
                    matches!(&mapped, MappedPluginJar::Deleted { .. }),
                    "{pid}: {mapped:?}"
                );
            }
        }

        // No map_files at all stays inconclusive.
        assert_eq!(
            probe_mapped_plugin_jar_at(&root.path().join("proc-none"), 16, stems),
            MappedPluginJar::Unknown
        );
    }

    /// `#jb263probeblind`: the realtime remedy (`superseded_editors=`) counts a
    /// module-only 263 IDE, and finds the restart verdict the 263 install wrote
    /// beside the plugins directory from a `lib/modules` mapping.
    #[test]
    fn probe_superseded_editors_counts_exact_263_module_jars_with_restart_verdict() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("IntelliJIdea2026.3");
        write_modular_tree(&plugins.join("agent-doc-jetbrains"), "0.2.866");
        let proc_root = root.path().join("proc");
        crate::process_start::fixture::write_fake_process_start(
            &proc_root,
            3_639_588,
            1_699_990_000,
            crate::process_start::fixture::ticks_after_boot(10_000),
        );
        write_map_files(
            &proc_root,
            3_639_588,
            &[format!(
                "{}/.agent-doc-jetbrains-install/backup-9-9/lib/modules/agent.doc.backend.jar (deleted)",
                plugins.display()
            )],
        );
        let marker = plugins.join(PLUGIN_RESTART_REQUIRED_MARKER);
        std::fs::write(
            &marker,
            "dynamic upgrade unavailable: exact-263 modular package
",
        )
        .unwrap();
        filetime::set_file_mtime(
            &marker,
            filetime::FileTime::from_unix_time(1_700_000_100, 0),
        )
        .unwrap();

        let found = probe_superseded_editors_at(&proc_root, [("jetbrains".to_string(), 3_639_588)]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].pid, 3_639_588);
        assert!(
            found[0].detail.contains("agent.doc.backend.jar"),
            "{found:?}"
        );
        assert_eq!(
            found[0].restart_verdict.as_deref(),
            Some("dynamic upgrade unavailable: exact-263 modular package")
        );
        assert!(
            found[0]
                .remedy()
                .contains("Restart the editor to load them")
        );
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
