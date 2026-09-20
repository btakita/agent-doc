//! Registration-independent JetBrains plugin activation probe
//! (`#pluginactivationprobe`).
//!
//! # Why this exists
//!
//! `stale_plugin` (`agent-doc-preflight-io/src/warnings.rs`) answers "is the
//! plugin the live editor *registered with* older than the build this binary
//! ships?" It reads live editor registrations, so it is silent whenever no
//! editor is attached — which is exactly when the operator's mental model of
//! "which plugin build is actually running" goes stale. Three `agent:review`
//! items carried a hand-recorded `(staged plugin version, live IDE PID, process
//! start time)` triple as their premise and each had to be re-derived by hand,
//! twice, because the triple rots the moment the IDE restarts or exits.
//!
//! This probe asks a different question that needs no registration at all:
//!
//! > **Was the installed plugin artifact written AFTER the IDE process started?**
//!
//! A JVM loads plugin jars at startup. If the newest installed
//! `agent-doc-jetbrains-<version>.jar` has an mtime later than an IDE process's
//! start time, that process cannot be running it — no matter what it registered,
//! and no matter whether it registered at all. If no IDE process is running, the
//! premise is not "stale", it is **dead**: there is nothing to activate.
//!
//! # Platform
//!
//! Process start times are read from `/proc` (Linux). On other platforms
//! [`live_ide_processes`] returns an empty list and the probe reports
//! [`ActivationProbe::unavailable`], which `session doctor` renders as an
//! explicit "not available on this platform" line rather than a false all-clear.

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

/// A live JetBrains IDE process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeProcess {
    pub pid: u32,
    /// Process start time, derived from the OS rather than recorded by hand.
    pub started_at: SystemTime,
    /// A short label for the operator (the IDE's main class or argv[0]).
    pub label: String,
}

/// One IDE process that started before the installed artifact was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleIdeProcess {
    pub pid: u32,
    pub label: String,
    /// How long before the install this process started.
    pub started_before_install: std::time::Duration,
}

/// What the probe found. Deliberately distinguishes "no IDE is running" from
/// "an IDE is running the installed build" — a dead premise and a healthy one
/// are not the same answer, and conflating them is what made the review items
/// need a human every time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationProbe {
    /// No installed plugin artifact was found at all.
    NotInstalled,
    /// No JetBrains IDE process is running; nothing can be activated.
    NoIdeRunning { artifact: InstalledPluginArtifact },
    /// Every live IDE started after the install landed.
    Active {
        artifact: InstalledPluginArtifact,
        pids: Vec<u32>,
    },
    /// At least one live IDE predates the installed artifact and therefore
    /// cannot have loaded it.
    Stale {
        artifact: InstalledPluginArtifact,
        stale: Vec<StaleIdeProcess>,
    },
    /// Process start times are not readable on this platform.
    Unavailable { reason: String },
}

impl ActivationProbe {
    /// The probe result rendered as a `session doctor` line, or `None` when the
    /// state is healthy and needs no operator attention.
    pub fn doctor_issue(&self) -> Option<String> {
        match self {
            Self::Stale { artifact, stale } => {
                let detail = stale
                    .iter()
                    .map(|process| {
                        format!(
                            "pid {} ({}) started {}s before the install",
                            process.pid,
                            process.label,
                            process.started_before_install.as_secs()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                Some(format!(
                    "installed JetBrains plugin {} ({}) postdates {} live IDE process(es), which therefore cannot have loaded it — {detail}. Restart the IDE to activate (`agent-doc admin reload-lib` refreshes only libagent_doc and cannot replace plugin code).",
                    artifact.version,
                    artifact.path.display(),
                    stale.len(),
                ))
            }
            Self::NoIdeRunning { artifact } => Some(format!(
                "installed JetBrains plugin {} is not activated anywhere: no JetBrains IDE process is running. Any review item whose premise is a recorded (plugin version, IDE pid, start time) triple is dead, not merely stale.",
                artifact.version,
            )),
            Self::NotInstalled | Self::Active { .. } | Self::Unavailable { .. } => None,
        }
    }
}

/// Pure core: classify the probe from an artifact and the live IDE processes.
///
/// Kept free of IO so the decision is testable without an IDE, a `/proc`, or an
/// install — which is the whole point of replacing a hand-recorded triple.
pub fn classify_activation(
    artifact: Option<InstalledPluginArtifact>,
    processes: &[IdeProcess],
) -> ActivationProbe {
    let Some(artifact) = artifact else {
        return ActivationProbe::NotInstalled;
    };
    if processes.is_empty() {
        return ActivationProbe::NoIdeRunning { artifact };
    }
    let stale = processes
        .iter()
        .filter_map(|process| {
            artifact
                .modified
                .duration_since(process.started_at)
                .ok()
                .map(|started_before_install| StaleIdeProcess {
                    pid: process.pid,
                    label: process.label.clone(),
                    started_before_install,
                })
        })
        .collect::<Vec<_>>();
    if stale.is_empty() {
        ActivationProbe::Active {
            artifact,
            pids: processes.iter().map(|process| process.pid).collect(),
        }
    } else {
        ActivationProbe::Stale { artifact, stale }
    }
}

/// Newest installed `agent-doc-jetbrains-<version>.jar` across `plugins_dirs`.
pub fn newest_installed_artifact(plugins_dirs: &[PathBuf]) -> Option<InstalledPluginArtifact> {
    plugins_dirs
        .iter()
        .flat_map(|dir| installed_artifacts_in(dir))
        .max_by_key(|artifact| artifact.modified)
}

fn installed_artifacts_in(plugins_dir: &Path) -> Vec<InstalledPluginArtifact> {
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

/// Every live JetBrains IDE process, with an OS-derived start time.
///
/// Linux only: `/proc/<pid>` is created when the process starts, so its
/// directory mtime is the process start time without parsing `stat` field 22 or
/// resolving `btime`.
#[cfg(target_os = "linux")]
pub fn live_ide_processes() -> Vec<IdeProcess> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut processes = entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_string_lossy().parse().ok()?;
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            let label = jetbrains_ide_label(&cmdline)?;
            let started_at = entry.metadata().ok()?.modified().ok()?;
            Some(IdeProcess {
                pid,
                started_at,
                label,
            })
        })
        .collect::<Vec<_>>();
    processes.sort_by_key(|process| process.pid);
    processes
}

#[cfg(not(target_os = "linux"))]
pub fn live_ide_processes() -> Vec<IdeProcess> {
    Vec::new()
}

/// Whether a command line belongs to a JetBrains IDE host, and the label to
/// show for it. Matched on the IDE main class, which every JetBrains launcher
/// passes regardless of how the JVM itself was invoked.
pub fn jetbrains_ide_label(cmdline: &str) -> Option<String> {
    const MAIN_CLASSES: &[&str] = &[
        "com.intellij.idea.Main",
        "com.intellij.idea.MainImpl",
        "com.intellij.platform.ide.bootstrap",
    ];
    if let Some(main_class) = MAIN_CLASSES
        .iter()
        .find(|main_class| cmdline.contains(**main_class))
    {
        return Some((*main_class).to_string());
    }
    // The toolbox launchers exec a product-named binary directly.
    const PRODUCT_BINARIES: &[&str] = &[
        "/idea", "/clion", "/goland", "/phpstorm", "/pycharm", "/rider", "/rubymine", "/rustrover",
        "/webstorm", "/datagrip", "/aqua",
    ];
    let first = cmdline.split_whitespace().next()?;
    PRODUCT_BINARIES
        .iter()
        .find(|binary| first.ends_with(**binary))
        .map(|binary| binary.trim_start_matches('/').to_string())
}

/// The full probe: resolve the installed artifact and the live IDEs, then
/// classify. `plugins_dirs` comes from the same discovery `agent-doc plugin
/// install` uses, so the probe looks exactly where an install would land.
pub fn probe(plugins_dirs: &[PathBuf]) -> ActivationProbe {
    if !cfg!(target_os = "linux") {
        return ActivationProbe::Unavailable {
            reason: "process start times are read from /proc; this probe is Linux-only"
                .to_string(),
        };
    }
    classify_activation(
        newest_installed_artifact(plugins_dirs),
        &live_ide_processes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn artifact(modified: SystemTime) -> InstalledPluginArtifact {
        InstalledPluginArtifact {
            path: PathBuf::from("/plugins/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.35.400.jar"),
            version: "0.35.400".to_string(),
            modified,
        }
    }

    fn process(pid: u32, started_at: SystemTime) -> IdeProcess {
        IdeProcess {
            pid,
            started_at,
            label: "com.intellij.idea.Main".to_string(),
        }
    }

    #[test]
    fn an_ide_started_before_the_install_cannot_have_loaded_it() {
        let install = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let probe = classify_activation(
            Some(artifact(install)),
            &[process(1645748, SystemTime::UNIX_EPOCH + Duration::from_secs(1_400))],
        );
        let ActivationProbe::Stale { stale, .. } = &probe else {
            panic!("an IDE older than the install is stale: {probe:?}");
        };
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].pid, 1645748);
        assert_eq!(stale[0].started_before_install, Duration::from_secs(600));
        assert!(
            probe
                .doctor_issue()
                .is_some_and(|issue| issue.contains("cannot have loaded it")),
            "doctor must surface it: {probe:?}"
        );
    }

    #[test]
    fn an_ide_started_after_the_install_is_active_and_silent() {
        let install = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let probe = classify_activation(
            Some(artifact(install)),
            &[process(7, SystemTime::UNIX_EPOCH + Duration::from_secs(2_001))],
        );
        assert_eq!(
            probe,
            ActivationProbe::Active {
                artifact: artifact(install),
                pids: vec![7],
            }
        );
        assert_eq!(probe.doctor_issue(), None, "a healthy probe says nothing");
    }

    #[test]
    fn one_stale_ide_among_several_still_reports() {
        // Two IDEs, one restarted after the install and one not. The healthy one
        // must not mask the stale one — the old hand-recorded triple named a
        // single pid and silently missed exactly this shape.
        let install = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let probe = classify_activation(
            Some(artifact(install)),
            &[
                process(11, SystemTime::UNIX_EPOCH + Duration::from_secs(2_500)),
                process(22, SystemTime::UNIX_EPOCH + Duration::from_secs(1_999)),
            ],
        );
        let ActivationProbe::Stale { stale, .. } = &probe else {
            panic!("expected stale: {probe:?}");
        };
        assert_eq!(
            stale.iter().map(|entry| entry.pid).collect::<Vec<_>>(),
            vec![22]
        );
    }

    #[test]
    fn no_running_ide_is_a_dead_premise_not_a_stale_one() {
        // The 2026-09-20 refresh recorded IDEA pid 1645748; by 15:20 no
        // IntelliJ or java process existed at all. That is not "stale" — there
        // is nothing to activate, and `stale_plugin` stays silent because it
        // only fires from live editor registrations.
        let install = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000);
        let probe = classify_activation(Some(artifact(install)), &[]);
        assert!(matches!(probe, ActivationProbe::NoIdeRunning { .. }));
        assert!(
            probe
                .doctor_issue()
                .is_some_and(|issue| issue.contains("no JetBrains IDE process is running")),
            "doctor must say the premise is dead: {probe:?}"
        );
    }

    #[test]
    fn nothing_installed_reports_nothing() {
        let probe = classify_activation(None, &[process(1, SystemTime::UNIX_EPOCH)]);
        assert_eq!(probe, ActivationProbe::NotInstalled);
        assert_eq!(probe.doctor_issue(), None);
    }

    #[test]
    fn newest_installed_artifact_picks_the_latest_written_jar() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        for version in ["0.35.399", "0.35.400"] {
            std::fs::write(lib.join(format!("agent-doc-jetbrains-{version}.jar")), "x").unwrap();
        }
        std::fs::write(lib.join("not-ours.jar"), "x").unwrap();

        let artifact = newest_installed_artifact(&[dir.path().to_path_buf()])
            .expect("an installed jar is found");

        assert!(
            artifact.version.starts_with("0.35."),
            "only agent-doc jars are considered: {artifact:?}"
        );
        assert!(artifact.path.ends_with(format!(
            "agent-doc-jetbrains-{}.jar",
            artifact.version
        )));
    }

    #[test]
    fn jetbrains_processes_are_matched_by_main_class_or_product_binary() {
        assert_eq!(
            jetbrains_ide_label("/usr/lib/jvm/java-21/bin/java -cp x com.intellij.idea.Main"),
            Some("com.intellij.idea.Main".to_string())
        );
        assert_eq!(
            jetbrains_ide_label("/opt/idea/bin/idea"),
            Some("idea".to_string())
        );
        assert_eq!(jetbrains_ide_label("/usr/bin/cargo build"), None);
        assert_eq!(jetbrains_ide_label(""), None);
    }
}
