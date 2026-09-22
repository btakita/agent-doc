use crate::PreflightWarning;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Collect warnings that can be evaluated before preflight mutates document or
/// sidecar state. These are read-only checks over harness selection, live
/// controller/supervisor freshness, and harness-specific config compatibility.
pub fn initial_warnings(
    file: &Path,
    document_agent: Option<&str>,
    active_harness: &str,
    codex_network_access_configured: bool,
) -> Vec<PreflightWarning> {
    let mut warnings = Vec::new();
    if let Some(warning) =
        agent_doc_model_tier::harness_mismatch_warning(document_agent, active_harness)
    {
        warnings.push(PreflightWarning {
            code: warning.code.to_string(),
            message: warning.message,
            document_agent: Some(warning.document_agent),
            active_harness: Some(warning.active_harness),
        });
    }

    // #fccsupwarn: report when the live controller/supervisor hosting this
    // document is serving a stale agent-doc binary. The turn-stage entry point
    // has already requested a CRDT-checkpointed safe-boundary recycle; this
    // warning is diagnostic only. Fail-open: any status/stat error yields no
    // warning and never blocks the cycle.
    if let Some(message) =
        agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
            file,
            "preflight_start",
        )
    {
        warnings.push(PreflightWarning {
            code: "supervisor_binary_stale".to_string(),
            message,
            document_agent: None,
            active_harness: None,
        });
    }

    if let Some(warning) = agent_doc_model_tier::codex_network_access_non_codex_harness_warning(
        &file.display().to_string(),
        document_agent,
        active_harness,
        codex_network_access_configured,
    ) {
        warnings.push(PreflightWarning {
            code: warning.code.to_string(),
            message: warning.message,
            document_agent: warning.document_agent,
            active_harness: Some(warning.active_harness),
        });
    }
    warnings
}

/// Collect late preflight warnings that need the resolved document body and
/// prompt presets after diff/preset resolution has stabilized.
pub fn content_and_staleness_warnings(
    file: &Path,
    content: &str,
    prompt_presets: &IndexMap<String, String>,
) -> Vec<PreflightWarning> {
    let mut warnings = Vec::new();
    if let Some(warning) =
        agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
            &file.display().to_string(),
            content,
            prompt_presets,
        )
        .map(PreflightWarning::from)
    {
        warnings.push(warning);
    }
    if let Some(warning) = agent_doc_workflow::preflight_policy::component_attr_preflight_warning(
        &file.display().to_string(),
        content,
    )
    .map(PreflightWarning::from)
    {
        warnings.push(warning);
    }
    if let Some(warning) =
        agent_doc_workflow::preflight_policy::preset_item_id_collision_warning(content)
            .map(PreflightWarning::from)
    {
        warnings.push(warning);
    }
    if let Ok((git_root, _)) = agent_doc_git_io::dirs::resolve_to_git_root(file)
        && let Some(warning) = stale_install_warning(&git_root)
    {
        warnings.push(warning);
    }
    warnings.extend(stale_plugin_warnings(file));
    warnings.extend(plugin_byte_identity_warnings(file));
    warnings
}

/// Surface semantic memory matches for likely completed work and fail-open
/// retrieval issues as ordinary preflight warnings.
pub fn semantic_completion_warnings(file: &Path) -> Vec<PreflightWarning> {
    match agent_doc_memory_io::session::semantic_completion_matches(file, None, 5) {
        Ok(matches) => matches
            .into_iter()
            .map(|semantic_match| PreflightWarning {
                code: "semantic_completion_match".to_string(),
                message: agent_doc_memory::format_semantic_completion_warning(&semantic_match),
                document_agent: None,
                active_harness: None,
            })
            .collect(),
        Err(err) => vec![PreflightWarning {
            code: "semantic_completion_retrieval_unavailable".to_string(),
            message: format!("semantic completion retrieval unavailable: {err}"),
            document_agent: None,
            active_harness: None,
        }],
    }
}

/// Warn when the installed/built `agent-doc` artifacts predate the latest local
/// source edit, so live sessions (tmux, JetBrains) do not silently run stale code
/// at an unchanged version string (`#install-stale-guard`). Best-effort: only
/// fires when an `agent-doc` source repo is locatable (development / dogfooding)
/// and silently no-ops otherwise (for example a prebuilt or PyPI install with no source).
/// The warning is advisory: the source-tree development/release owner installs,
/// while unrelated document sessions continue without rebuilding.
///
/// `#supstaledetect`: the staleness basis is the newest source-FILE mtime
/// (`newest_crate_source_mtime_secs`, the same signal the supervisor auto-install
/// path uses), NOT the HEAD source-commit timestamp. The dogfood flow is
/// edit -> build -> install -> verify -> THEN commit, so a freshly built binary
/// always predates the commit object that covers it; comparing against the commit
/// timestamp false-positived a fresh binary as stale whenever the build->commit
/// gap exceeded the grace. Unifying onto the source-file mtime keeps this
/// warning in agreement with the auto-install staleness signal.
pub fn stale_install_warning(doc_git_root: &Path) -> Option<PreflightWarning> {
    let repo = agent_doc_fs::install_freshness::locate_agent_doc_source_repo(doc_git_root)?;
    let source_ts = agent_doc_fs::install_freshness::newest_crate_source_mtime_secs(&repo)?;
    let artifacts = agent_doc_fs::install_freshness::agent_doc_install_artifacts(&repo);

    let stale = agent_doc_supervisor::config::classify_stale_install_artifacts(
        source_ts,
        &artifacts,
        agent_doc_supervisor::config::STALE_INSTALL_GRACE_SECS,
    );
    if stale.is_empty() {
        return None;
    }

    // `#autoinstalldeferstale`: distinguish "predates your uncommitted edits"
    // (housekeeping) from "predates COMMITTED work" (a landed fix is not the code
    // running). Both previously read identically, and the second one is why a
    // session crashed on an already-fixed bug.
    let oldest_installed = artifacts.iter().filter_map(|(_, mtime)| *mtime).min();
    let missing = oldest_installed
        .map(|since| commits_since(&repo, since))
        .unwrap_or_default();
    let freshness_detail = missing_commits_note(&missing).unwrap_or_else(|| {
        "No committed agent-doc change is missing; only concurrent source edits are newer."
            .to_string()
    });

    Some(PreflightWarning {
        code: "stale_install".to_string(),
        message: format!(
            "stale agent-doc install (advisory): {} predate the latest local source edit - live sessions (tmux / JetBrains) may run pre-edit code at an unchanged version. {freshness_detail} {} Source repo: {}.",
            stale.join(", "),
            stale_install_guidance(),
            repo.display()
        ),
        document_agent: None,
        active_harness: None,
    })
}

fn stale_install_guidance() -> &'static str {
    "Continue this document session; this warning is not a closeout or queue-drain blocker. Unless this cycle owns development/release of this repository, do not run `make install`: the owning development/release cycle installs and the supervisor performs the safe recycle."
}

/// Name what the running binary is actually missing.
///
/// `#autoinstalldeferstale`: "the install predates local source edits" reads as
/// housekeeping, so an operator reasonably ignores it. But when the source repo
/// has COMMITTED work the binary predates, the same warning means something much
/// sharper — a fix that is already landed is not the code running. Observed
/// 2026-07-20: a supervisor crashed repeatedly on a registry bug whose fix and
/// regression test were already committed in a patched sibling, because
/// auto-install had deferred on an unrelated dirty worktree and only said so in
/// `supervisor-stderr.log`.
///
/// Pure so the wording is testable without a repo.
pub fn missing_commits_note(commits: &[String]) -> Option<String> {
    if commits.is_empty() {
        return None;
    }
    let head = commits.first().map(String::as_str).unwrap_or_default();
    let extra = commits.len().saturating_sub(1);
    let tail = if extra > 0 {
        format!(" (+{extra} more)")
    } else {
        String::new()
    };
    Some(format!(
        "The running binary is MISSING {} committed change(s), newest `{head}`{tail} - a fix you already landed is NOT the code running.",
        commits.len()
    ))
}

/// Commit subjects in `repo` newer than `since` (unix seconds), newest first.
///
/// Best-effort: any git failure yields an empty list, because a missing note is
/// strictly better than blocking or mis-reporting the staleness warning.
fn commits_since(repo: &Path, since: u64) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("log")
        .arg(format!("--since=@{since}"))
        .arg("--format=%h %s")
        .arg("--max-count=20")
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// `#stale-plugin-detect`: the package generation expected by the shared
/// reliable-sync registration authority.
pub fn expected_plugin_version(editor_kind: &str) -> Option<&'static str> {
    agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version(editor_kind)
}

/// Compare two dotted numeric version strings (e.g. `0.2.206`). Returns `true`
/// only when `running` is strictly older than `expected`. A leading `v` and any
/// pre-release/build suffix (after `-` or `+`) are ignored; unparseable input
/// fails open to `false` so a malformed version never manufactures a warning.
pub fn plugin_version_is_older(running: &str, expected: &str) -> bool {
    fn parse(version: &str) -> Option<Vec<u64>> {
        let core = version.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next().unwrap_or(core);
        core.split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    }
    let (Some(run), Some(exp)) = (parse(running), parse(expected)) else {
        return false;
    };
    for index in 0..run.len().max(exp.len()) {
        let run_component = run.get(index).copied().unwrap_or(0);
        let exp_component = exp.get(index).copied().unwrap_or(0);
        if run_component != exp_component {
            return run_component < exp_component;
        }
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePluginGenerationStatus {
    pub editor_id: Option<String>,
    pub kind: String,
    pub running: String,
    pub expected: String,
    pub timestamp_ms: u128,
    pub stale: bool,
}

/// Resolve the latest live registration for each editor instance. A plugin
/// restart may occur after preflight; the newer registration is the authority
/// for replay/ACK recovery and supersedes the turn-start diagnostic.
pub fn live_plugin_generation_statuses_from_registrations(
    registrations: &[agent_doc_reliable_sync_io::liveness::EditorRegistration],
    expected_for_kind: impl Fn(&str) -> Option<&'static str>,
) -> Vec<LivePluginGenerationStatus> {
    let mut latest: HashMap<
        (String, String),
        &agent_doc_reliable_sync_io::liveness::EditorRegistration,
    > = HashMap::new();
    for registration in registrations {
        let kind = registration.editor_kind.as_str();
        let running = registration.editor_version.as_str();
        if expected_for_kind(kind).is_none() {
            continue;
        }
        let key = (kind.to_ascii_lowercase(), registration.editor_id.clone());
        let replace = latest.get(&key).is_none_or(|current| {
            agent_doc_workflow::capture::decide_plugin_generation_refresh(
                agent_doc_workflow::capture::PluginGenerationRefreshEvidence {
                    preflight_generation: u128::from(current.timestamp_ms),
                    live_generation: u128::from(registration.timestamp_ms),
                    live_registration_observed: true,
                },
            ) == agent_doc_workflow::capture::PluginGenerationRefreshDecision::AdoptLive
        });
        if replace {
            latest.insert(key, registration);
        }
        let _ = running;
    }
    let mut statuses = latest
        .into_values()
        .filter_map(|registration| {
            let kind = registration.editor_kind.as_str();
            let running = registration.editor_version.as_str();
            let expected = expected_for_kind(kind)?;
            Some(LivePluginGenerationStatus {
                editor_id: Some(registration.editor_id.clone()),
                kind: kind.to_string(),
                running: running.to_string(),
                expected: expected.to_string(),
                timestamp_ms: u128::from(registration.timestamp_ms),
                // Native effects require exact code-generation identity. A
                // newer plugin paired with an older controller is just as
                // incompatible as an older plugin paired with a newer one.
                stale: running.trim() != expected,
            })
        })
        .collect::<Vec<_>>();
    statuses.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.editor_id.cmp(&right.editor_id))
    });
    statuses
}

pub fn live_plugin_generation_statuses(file: &Path) -> Vec<LivePluginGenerationStatus> {
    let registrations =
        agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file)
            .unwrap_or_default();
    live_plugin_generation_statuses_from_registrations(&registrations, expected_plugin_version)
}

/// Emit replay-time generation evidence. This deliberately runs after preflight
/// so a plugin installed/restarted during the turn is recognized immediately.
pub fn report_live_plugin_generation_refresh(file: &Path) {
    for status in live_plugin_generation_statuses(file) {
        if status.stale {
            eprintln!(
                "[editor] live {} plugin {} does not match expected generation {}; install/update the editor plugin and restart/reload the IDE host. `agent-doc admin reload-lib` refreshes only libagent_doc and cannot change plugin code or its reported version.",
                status.kind, status.running, status.expected,
            );
        } else {
            eprintln!(
                "[editor] live {} plugin {} registered (expected {}); this live generation supersedes any stale plugin warning captured earlier in the turn.",
                status.kind, status.running, status.expected,
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "live_plugin_generation_refresh file={} editor_kind={} editor_id={} running={} expected={} stale={} source=repair_boundary",
                file.display(),
                status.kind,
                status.editor_id.as_deref().unwrap_or("unknown"),
                status.running,
                status.expected,
                status.stale,
            ),
        );
    }
}

/// `#stale-plugin-detect`: detect any live editor plugin reporting a version
/// different from the plugin build this binary ships with, and warn so the
/// operator reloads one exact generation pair.
pub fn stale_plugin_warnings(file: &Path) -> Vec<PreflightWarning> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    live_plugin_generation_statuses(file)
        .into_iter()
        .filter_map(|status| {
            if !status.stale || !seen.insert((status.kind.clone(), status.running.clone())) {
                return None;
            }
            Some(PreflightWarning {
                code: "stale_plugin".to_string(),
                message: format!(
                    "live {} editor plugin {} does not match the {} build shipped with this agent-doc binary; install/update the editor plugin and restart/reload the IDE host before relying on editor delivery ACKs.",
                    status.kind, status.running, status.expected
                ),
                document_agent: None,
                active_harness: None,
            })
        })
        .collect()
}

/// Pure core of [`stale_plugin_warnings`]: given the live per-editor registrations
/// and an expected-version resolver, produce one deduplicated warning per stale
/// (kind, version) pair.
pub fn stale_plugin_warnings_from_registrations(
    registrations: &[agent_doc_reliable_sync_io::liveness::EditorRegistration],
    expected_for_kind: impl Fn(&str) -> Option<&'static str>,
) -> Vec<PreflightWarning> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut warnings = Vec::new();
    for status in
        live_plugin_generation_statuses_from_registrations(registrations, &expected_for_kind)
    {
        if !status.stale {
            continue;
        }
        if !seen.insert((status.kind.clone(), status.running.clone())) {
            continue;
        }
        warnings.push(PreflightWarning {
            code: "stale_plugin".to_string(),
            message: format!(
                "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with. The live editor may run pre-fix IPC/plugin code (a known source of live_prompt_drift / content_ours merge regressions). Install/update the {kind} plugin (JetBrains: update to {expected} and restart/reload the IDE; VS Code: reinstall the extension and reload the window). `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot replace Kotlin/TypeScript plugin code or change the reported plugin version. A later live registration at {expected} or newer supersedes this warning.",
                kind = status.kind,
                running = status.running,
                expected = status.expected,
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    warnings
}

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
        // A jar we cannot stat on either side proves nothing.
        _ => MappedPluginJar::Unknown,
    }
}

/// Pure core: one deduplicated warning per (kind, pid) running superseded bytes.
///
/// The message has to name the version trap explicitly. An operator who reads
/// "plugin 0.2.388 is running" and "0.2.388 is installed" will otherwise
/// conclude the restart already happened, which is exactly how two review items
/// in this project sat gated on a premise that was measurably wrong.
pub fn plugin_byte_identity_warnings_from(
    probes: &[(String, u32, MappedPluginJar)],
) -> Vec<PreflightWarning> {
    let mut seen: HashSet<(String, u32)> = HashSet::new();
    let mut warnings = Vec::new();
    for (kind, pid, mapped) in probes {
        if !mapped.is_superseded() || !seen.insert((kind.clone(), *pid)) {
            continue;
        }
        let detail = match mapped {
            MappedPluginJar::Deleted { path } => {
                format!("{path} is mapped but its inode was unlinked")
            }
            MappedPluginJar::Superseded {
                path,
                mapped_inode,
                disk_inode,
            } => format!(
                "{path} is mapped as inode {mapped_inode} but disk now holds inode {disk_inode}"
            ),
            _ => continue,
        };
        warnings.push(PreflightWarning {
            code: "plugin_bytes_superseded".to_string(),
            message: format!(
                "live {kind} editor pid {pid} is running superseded plugin bytes: {detail}. \
                 The version string cannot show this — an install rewrites the jar under the \
                 same version, so a version check reports the plugin as current while the \
                 process keeps executing the replaced build. Restart the editor (or reopen the \
                 document tab) to pick up the installed jar."
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    warnings
}

/// Probe one process's mapped plugin jar. Linux-only; every other platform and
/// every IO error yields [`MappedPluginJar::Unknown`].
pub fn probe_mapped_plugin_jar(pid: u32, jar_stem: &str) -> MappedPluginJar {
    let map_files = std::path::PathBuf::from(format!("/proc/{pid}/map_files"));
    let Ok(entries) = std::fs::read_dir(&map_files) else {
        return MappedPluginJar::Unknown;
    };
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
        if classified.is_superseded() {
            return classified;
        }
    }
    MappedPluginJar::Unknown
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

/// `#pluginbyteidentity`: warn when a live editor is executing plugin bytes that
/// the installed jar has already replaced.
pub fn plugin_byte_identity_warnings(file: &Path) -> Vec<PreflightWarning> {
    let registrations =
        agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file)
            .unwrap_or_default();
    let mut probes = Vec::new();
    let mut probed: HashSet<u32> = HashSet::new();
    for registration in &registrations {
        let Some(jar_stem) = plugin_jar_stem(&registration.editor_kind) else {
            continue;
        };
        let pid = u32::try_from(registration.pid).unwrap_or_default();
        if pid == 0 || !probed.insert(pid) {
            continue;
        }
        probes.push((
            registration.editor_kind.clone(),
            pid,
            probe_mapped_plugin_jar(pid, jar_stem),
        ));
    }
    plugin_byte_identity_warnings_from(&probes)
}

/// Jar filename prefix for an editor kind. Only editors that load agent-doc as a
/// jar inside their own process can be probed this way.
pub fn plugin_jar_stem(editor_kind: &str) -> Option<&'static str> {
    match editor_kind.to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" => Some("agent-doc-jetbrains-"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MappedPluginJar, classify_mapped_plugin_jar, plugin_byte_identity_warnings_from,
        plugin_jar_stem,
    };

    /// `#pluginbyteidentity`: the kernel's `" (deleted)"` suffix is the only
    /// evidence that survives an install unlinking the jar we are executing.
    /// Nothing can be stat'd back afterwards, so losing this parse loses the case.
    #[test]
    fn deleted_suffix_is_the_definitive_superseded_signal() {
        let jar = "/home/u/.local/share/JetBrains/x/lib/agent-doc-jetbrains-0.2.388.jar";
        let deleted = classify_mapped_plugin_jar(&format!("{jar} (deleted)"), None, None);
        assert_eq!(
            deleted,
            MappedPluginJar::Deleted {
                path: jar.to_string()
            },
            "the (deleted) suffix must classify without needing either inode"
        );
        assert!(deleted.is_superseded());
    }

    /// The discriminator must be BYTES, not the version string. Both sides here
    /// are `0.2.388`; only the inode separates a live IDE running the installed
    /// build from one running a replaced build under the same name.
    #[test]
    fn same_version_string_does_not_suppress_a_byte_mismatch() {
        let jar = "/opt/idea/plugins/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.388.jar";

        let superseded = classify_mapped_plugin_jar(jar, Some(76_585_058), Some(99_000_001));
        assert!(
            superseded.is_superseded(),
            "a differing inode under an identical version must still be superseded: {superseded:?}"
        );

        let current = classify_mapped_plugin_jar(jar, Some(76_585_058), Some(76_585_058));
        assert_eq!(
            current,
            MappedPluginJar::Current {
                path: jar.to_string(),
                inode: 76_585_058
            },
            "an identical inode is the only thing that proves the bytes match"
        );
        assert!(!current.is_superseded());
    }

    /// Fail open. A jar we cannot stat on either side proves nothing, and an
    /// unprovable probe must never manufacture a warning that sends an operator
    /// restarting a healthy IDE.
    #[test]
    fn unstattable_mapping_is_unknown_not_superseded() {
        let jar = "/opt/idea/lib/agent-doc-jetbrains-0.2.388.jar";
        for (mapped, disk) in [(None, Some(1_u64)), (Some(1_u64), None), (None, None)] {
            let classified = classify_mapped_plugin_jar(jar, mapped, disk);
            assert_eq!(
                classified,
                MappedPluginJar::Unknown,
                "missing inode evidence must fail open: mapped={mapped:?} disk={disk:?}"
            );
            assert!(!classified.is_superseded());
        }
    }

    /// The warning has to say why the version string lied, or the operator reads
    /// "0.2.388 running, 0.2.388 installed" and concludes the restart happened.
    #[test]
    fn superseded_warning_names_the_version_trap_and_dedups_per_process() {
        let jar = "/opt/idea/lib/agent-doc-jetbrains-0.2.388.jar";
        let probes = vec![
            (
                "jetbrains".to_string(),
                1023909_u32,
                MappedPluginJar::Deleted {
                    path: jar.to_string(),
                },
            ),
            // same process, probed twice — one warning
            (
                "jetbrains".to_string(),
                1023909_u32,
                MappedPluginJar::Superseded {
                    path: jar.to_string(),
                    mapped_inode: 1,
                    disk_inode: 2,
                },
            ),
            // healthy process contributes nothing
            (
                "jetbrains".to_string(),
                2_u32,
                MappedPluginJar::Current {
                    path: jar.to_string(),
                    inode: 7,
                },
            ),
            ("jetbrains".to_string(), 3_u32, MappedPluginJar::Unknown),
        ];

        let warnings = plugin_byte_identity_warnings_from(&probes);
        assert_eq!(
            warnings.len(),
            1,
            "one warning per superseded process, none for healthy or unknown: {warnings:?}"
        );
        let warning = &warnings[0];
        assert_eq!(warning.code, "plugin_bytes_superseded");
        assert!(
            warning.message.contains("1023909"),
            "the pid is how an operator finds the process: {}",
            warning.message
        );
        assert!(
            warning.message.contains("version string cannot show this"),
            "the message must explain WHY the version comparison missed it, not merely \
             mention versions somewhere: {}",
            warning.message
        );
        assert!(
            warning.message.contains("unlinked"),
            "the deleted case must name its own evidence: {}",
            warning.message
        );
    }

    /// Only editors that load agent-doc as a jar inside their own process can be
    /// probed this way; claiming otherwise would emit a permanently-unknown probe.
    #[test]
    fn jar_stem_is_scoped_to_jvm_hosted_editors() {
        for kind in ["jetbrains", "JetBrains", "intellij", "idea"] {
            assert_eq!(
                plugin_jar_stem(kind),
                Some("agent-doc-jetbrains-"),
                "{kind}"
            );
        }
        for kind in ["vscode", "zed", "neovim", ""] {
            assert_eq!(plugin_jar_stem(kind), None, "{kind}");
        }
    }

    /// `#autoinstalldeferstale`: a stale install that merely predates uncommitted
    /// edits is housekeeping; one that predates COMMITTED work means a landed fix
    /// is not running. Those read identically before this note, which is how a
    /// supervisor crashed repeatedly on an already-fixed bug.
    #[test]
    fn missing_commits_note_names_what_the_binary_is_missing() {
        assert_eq!(
            super::missing_commits_note(&[]),
            None,
            "no committed work past the install is ordinary staleness, not an alarm"
        );

        let one = super::missing_commits_note(&["abc1234 fix the thing".to_string()])
            .expect("committed work must produce a note");
        assert!(one.contains("MISSING 1 committed change"), "{one}");
        assert!(one.contains("abc1234 fix the thing"), "{one}");
        assert!(
            one.contains("NOT the code running"),
            "the note must say why it matters: {one}"
        );
        assert!(
            !one.contains("more"),
            "a single commit needs no overflow: {one}"
        );

        let many = super::missing_commits_note(&[
            "aaa1111 newest".to_string(),
            "bbb2222 older".to_string(),
            "ccc3333 oldest".to_string(),
        ])
        .expect("committed work must produce a note");
        assert!(many.contains("MISSING 3 committed change"), "{many}");
        assert!(
            many.contains("aaa1111 newest"),
            "the NEWEST commit is the most useful identifier: {many}"
        );
        assert!(many.contains("(+2 more)"), "{many}");
    }

    #[test]
    fn stale_install_guidance_never_blocks_an_unrelated_document_session() {
        let message = super::stale_install_guidance();

        assert!(message.contains("Continue this document session"));
        assert!(message.contains("not a closeout or queue-drain blocker"));
        assert!(message.contains("Unless this cycle owns development/release"));
        assert!(!message.contains("Run `make install`"));
    }

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn plugin_version_is_older_compares_numeric_components() {
        assert!(plugin_version_is_older("0.2.205", "0.2.206"));
        assert!(plugin_version_is_older("0.2.6", "0.2.206"));
        assert!(plugin_version_is_older("0.2", "0.2.206"));
        assert!(plugin_version_is_older("v0.2.205", "0.2.206"));
        assert!(plugin_version_is_older("0.2.205-beta", "0.2.206"));
        assert!(!plugin_version_is_older("0.2.206", "0.2.206"));
        assert!(!plugin_version_is_older("0.2.207", "0.2.206"));
        assert!(!plugin_version_is_older("0.3.0", "0.2.206"));
        assert!(!plugin_version_is_older("1.0.0", "0.9.9"));
        assert!(!plugin_version_is_older("garbage", "0.2.206"));
        assert!(!plugin_version_is_older("0.2.206", "unknown"));
    }

    #[test]
    fn expected_plugin_version_maps_known_kinds() {
        assert!(expected_plugin_version("emacs").is_none());
        assert_eq!(
            expected_plugin_version("jetbrains"),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("jetbrains")
        );
        assert_eq!(
            expected_plugin_version("JetBrains"),
            expected_plugin_version("intellij")
        );
        assert_eq!(
            expected_plugin_version("vscode"),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("vscode")
        );
        assert_eq!(
            expected_plugin_version("zed"),
            agent_doc_reliable_sync_io::liveness::expected_editor_plugin_version("zed")
        );
    }

    fn registration_with_editor(
        kind: &str,
        version: &str,
    ) -> agent_doc_reliable_sync_io::liveness::EditorRegistration {
        agent_doc_reliable_sync_io::liveness::EditorRegistration {
            document_hash: "doc".to_string(),
            pid: 1,
            path: "/tmp/doc.md".to_string(),
            editor_id: "editor".to_string(),
            editor_kind: kind.to_string(),
            editor_version: version.to_string(),
            capabilities: Vec::new(),
            timestamp_ms: 0,
        }
    }

    fn registration_with_generation(
        editor_id: &str,
        version: &str,
        timestamp_ms: u64,
    ) -> agent_doc_reliable_sync_io::liveness::EditorRegistration {
        let mut registration = registration_with_editor("jetbrains", version);
        registration.editor_id = editor_id.to_string();
        registration.timestamp_ms = timestamp_ms;
        registration
    }

    #[test]
    fn stale_plugin_warning_flags_older_live_plugin() {
        let registrations = vec![registration_with_editor("jetbrains", "0.2.205")];
        let warnings = stale_plugin_warnings_from_registrations(&registrations, |kind| {
            (kind == "jetbrains").then_some("0.2.206")
        });
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "stale_plugin");
        assert!(warnings[0].message.contains("0.2.205"));
        assert!(warnings[0].message.contains("0.2.206"));
        assert!(warnings[0].message.contains("refreshes only the native"));
        assert!(
            warnings[0]
                .message
                .contains("cannot replace Kotlin/TypeScript")
        );
    }

    #[test]
    fn replay_boundary_uses_latest_live_generation_for_the_same_editor() {
        let registrations = vec![
            registration_with_generation("idea-project", "0.2.261", 10),
            registration_with_generation("idea-project", "0.2.263", 20),
        ];
        let statuses =
            live_plugin_generation_statuses_from_registrations(&registrations, |_| Some("0.2.263"));
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].running, "0.2.263");
        assert!(!statuses[0].stale);
    }

    #[test]
    fn replay_boundary_keeps_independent_editor_generations_separate() {
        let registrations = vec![
            registration_with_generation("idea-a", "0.2.261", 10),
            registration_with_generation("idea-b", "0.2.263", 20),
        ];
        let statuses =
            live_plugin_generation_statuses_from_registrations(&registrations, |_| Some("0.2.263"));
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().any(|status| status.stale));
        assert!(statuses.iter().any(|status| !status.stale));
    }

    #[test]
    fn stale_plugin_warning_requires_exact_generation_or_unknown() {
        let current = vec![registration_with_editor("jetbrains", "0.2.206")];
        assert!(
            stale_plugin_warnings_from_registrations(&current, |_| Some("0.2.206")).is_empty(),
            "a current plugin must not warn"
        );
        let newer = vec![registration_with_editor("jetbrains", "0.2.207")];
        assert!(
            !stale_plugin_warnings_from_registrations(&newer, |_| Some("0.2.206")).is_empty(),
            "a newer plugin paired with an older controller must warn"
        );
        let no_expectation = vec![registration_with_editor("jetbrains", "0.2.100")];
        assert!(
            stale_plugin_warnings_from_registrations(&no_expectation, |_| None).is_empty(),
            "no baked expectation must not warn (fail-open)"
        );
    }

    #[test]
    fn stale_plugin_warning_dedups_identical_kind_version() {
        let registrations = vec![
            registration_with_editor("vscode", "0.2.38"),
            registration_with_editor("vscode", "0.2.38"),
        ];
        let warnings = stale_plugin_warnings_from_registrations(&registrations, |_| Some("0.2.39"));
        assert_eq!(
            warnings.len(),
            1,
            "identical (kind, version) collapses to one warning"
        );
    }

    #[test]
    fn stale_plugin_warning_end_to_end_from_reliable_registration() {
        let Some(_expected) = expected_plugin_version("jetbrains") else {
            return;
        };
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        let editor_id = format!("jetbrains-{}-e2e", std::process::id());
        let ops = vec![
            agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash: document_hash.clone(),
                pid: std::process::id().into(),
                tag: editor_id.clone(),
            },
            agent_doc_reliable_sync_io::liveness::LivenessOp::Register(
                agent_doc_reliable_sync_io::liveness::EditorRegistration {
                    document_hash: document_hash.clone(),
                    pid: std::process::id().into(),
                    path: canonical.to_string_lossy().into_owned(),
                    editor_id,
                    editor_kind: "jetbrains".to_string(),
                    editor_version: "0.2.100".to_string(),
                    capabilities: Vec::new(),
                    timestamp_ms: 1,
                },
            ),
        ];
        agent_doc_sqlite::reliable_sync_inbox::record_remote_frame(
            &agent_doc_sqlite::state_store::state_db_path(dir.path()),
            &document_hash,
            1,
            Some(&serde_json::to_string(&ops).unwrap()),
        )
        .unwrap();

        let warnings = stale_plugin_warnings(&doc);
        assert_eq!(
            warnings.len(),
            1,
            "a live ancient plugin must warn: {warnings:?}"
        );
        assert_eq!(warnings[0].code, "stale_plugin");
        assert!(warnings[0].message.contains("0.2.100"));
    }
}
