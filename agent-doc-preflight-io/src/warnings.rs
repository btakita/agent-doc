use crate::PreflightWarning;
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    // `#presetshape` (GH #69 §1): a structured or unsupported preset value is a
    // warning with the supported shape named, never a fatal parse error.
    if let Ok((fm, _)) = agent_doc_frontmatter::frontmatter::parse(content) {
        warnings.extend(fm.prompt_presets.diagnostics().into_iter().map(|message| {
            PreflightWarning {
                code: "prompt_preset_shape".to_string(),
                message: format!("{}: {message}", file.display()),
                document_agent: None,
                active_harness: None,
            }
        }));
    }
    if let Ok((git_root, _)) = agent_doc_git_io::dirs::resolve_to_git_root(file)
        && let Some(warning) = stale_install_warning(&git_root)
    {
        warnings.push(warning);
    }
    warnings.extend(stale_plugin_warnings(file));
    warnings.extend(plugin_byte_identity_warnings(file));
    warnings.extend(jetbrains_staged_install_failure_warnings());
    if let Some(age_secs) =
        agent_doc_controller_io::project_controller::undrained_supervisor_drain_handoff_age(
            file, content,
        )
    {
        warnings.push(supervisor_drain_handoff_undrained_warning(file, age_secs));
    }
    warnings
}

/// `#supdrainyieldfalsifiable` (GH #73 §3): a `[focused-cycle]` head that
/// session-check handed to the supervisor is still the supervisor head long
/// after the hand-off. Without this the same head was silently re-presented as
/// if nothing had been promised.
pub fn supervisor_drain_handoff_undrained_warning(file: &Path, age_secs: u64) -> PreflightWarning {
    PreflightWarning {
        code: "supervisor_drain_handoff_undrained".to_string(),
        message: format!(
            "the [focused-cycle] queue head of {} was handed to the supervisor {} min ago and is \
             still undrained — the supervisor clear-and-continue drain did not happen. Treat it \
             as a stalled queue: refresh the supervisor (`agent-doc admin recycle` after this \
             turn closes, or `agent-doc session restart-supervisor <FILE>`) or drain the head \
             in a fresh session.",
            file.display(),
            age_secs / 60
        ),
        document_agent: None,
        active_harness: None,
    }
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

/// Which half of a generation mismatch is behind.
///
/// `stale` is symmetric — exact generation identity is what native effects
/// require — but the *remedy* is not. Printing "install/update the editor
/// plugin" when the live plugin is the NEWER half sends the operator to
/// reinstall a build that is already running, and leaves the actually-stale
/// half (this binary) untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleGenerationSide {
    /// The live plugin is older than the build this binary ships.
    Plugin,
    /// The live plugin is newer: this agent-doc binary is the stale half.
    Binary,
}

/// Resolve which half is behind, or `None` when neither version parses as a
/// dotted numeric generation and no direction can be claimed.
pub fn stale_generation_side(running: &str, expected: &str) -> Option<StaleGenerationSide> {
    if plugin_version_is_older(running, expected) {
        Some(StaleGenerationSide::Plugin)
    } else if plugin_version_is_older(expected, running) {
        Some(StaleGenerationSide::Binary)
    } else {
        None
    }
}

/// The single `stale_plugin` message body, shared by the preflight warning and
/// the replay-boundary console line so an operator cannot be handed two
/// different remedies for one mismatch.
///
/// `installed` is the on-disk installed plugin version, when it is known
/// (`#gh76secondary`). A restart only loads what is on disk, so "update to
/// {expected} and restart" is unreachable advice when the disk holds an older
/// build, and redundant install advice when the disk already holds {expected}.
///
/// `staged_for_restart` (GH #87) is true when {expected} is already queued for the
/// next IDE start (IntelliJ's `action.script`, or agent-doc's own staging record).
/// The unpacked install still holds the old build in that state, so without this
/// the message took the "install first" branch against an install that is done.
pub fn stale_plugin_message(
    kind: &str,
    running: &str,
    expected: &str,
    installed: Option<&str>,
    staged_for_restart: bool,
) -> String {
    stale_plugin_message_for_platform(kind, running, expected, installed, staged_for_restart, None)
}

/// [`stale_plugin_message`] with the live IDE's proven platform build when it
/// lies outside every published package range (GH #233). Such an IDE cannot
/// receive {expected} — the classic package is capped at 261 and the modular
/// one at 262 — so "install {expected} first" is unreachable advice there; the
/// message names the build, the supported ranges and what stays installed.
pub fn stale_plugin_message_for_platform(
    kind: &str,
    running: &str,
    expected: &str,
    installed: Option<&str>,
    staged_for_restart: bool,
    unsupported_platform_build: Option<u32>,
) -> String {
    match stale_generation_side(running, expected) {
        Some(StaleGenerationSide::Plugin) if let Some(build) = unsupported_platform_build => {
            let kept = installed.unwrap_or(running);
            format!(
                "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with, but this IDE runs platform build {build}, outside every published agent-doc-jetbrains range ({ranges}). No {expected} package can be installed into it, so plugin {kept} stays installed and no install, upgrade or restart reaches parity on this IDE until an agent-doc-jetbrains package declares build {build}. This warning is ADVISORY: continue the current document task; editor delivery may run the older plugin code. `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot change the reported plugin version. A later live registration at {expected} or newer supersedes this warning.",
                ranges = agent_doc_fs::jetbrains_install::JETBRAINS_SUPPORTED_RANGES,
            )
        }
        Some(StaleGenerationSide::Plugin)
            if staged_for_restart && installed.is_none_or(|v| v.trim() != expected) =>
        {
            format!(
                "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with. The {expected} build is ALREADY STAGED for the next IDE start (the restart-free upgrade did not run in that IDE, so the install was queued instead), so do not reinstall: restart the IDE and it installs {expected} as it starts. `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot replace Kotlin/TypeScript plugin code or change the reported plugin version. A later live registration at {expected} or newer supersedes this warning."
            )
        }
        Some(StaleGenerationSide::Plugin) if installed.is_some_and(|v| v.trim() == expected) => {
            format!(
                "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with. The {expected} build is ALREADY installed on disk, so do not reinstall: restart/reload the IDE so the live process loads it (a long-lived JVM keeps the old plugin bytes mapped). `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot replace Kotlin/TypeScript plugin code or change the reported plugin version. A later live registration at {expected} or newer supersedes this warning."
            )
        }
        Some(StaleGenerationSide::Plugin) if let Some(on_disk) = installed => format!(
            "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with, and the on-disk install is {on_disk}, not {expected} — so a restart alone cannot reach parity. Install {expected} first (`agent-doc plugin install {kind}`, or `--local` from a build of this checkout), then restart/reload the IDE. `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot replace Kotlin/TypeScript plugin code or change the reported plugin version. A later live registration at {expected} or newer supersedes this warning."
        ),
        Some(StaleGenerationSide::Plugin) => format!(
            "stale editor plugin: a live {kind} plugin reports version {running}, older than the {expected} build this agent-doc binary ships with. The live editor may run pre-fix IPC/plugin code (a known source of live_prompt_drift / content_ours merge regressions). Install/update the {kind} plugin (JetBrains: update to {expected} and restart/reload the IDE; VS Code: reinstall the extension and reload the window). `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot replace Kotlin/TypeScript plugin code or change the reported plugin version. A later live registration at {expected} or newer supersedes this warning."
        ),
        // `#staleinstallremedyowner`: this warning reaches EVERY document session,
        // most of which must not run `make install` — a mid-session install against
        // a live supervisor strands attached editor replicas, and only the cycle
        // that owns agent-doc development/release owns the rebuild. The remedy used
        // to read as an unconditional instruction, so sessions doing unrelated work
        // had to reason their way out of it every turn. Name the owner instead.
        Some(StaleGenerationSide::Binary) => format!(
            "stale agent-doc binary: a live {kind} plugin reports version {running}, newer than the {expected} build this agent-doc binary ships with. The plugin is NOT the stale half — do not reinstall or restart it, that would reinstall a build that is already live. This warning is ADVISORY: continue the current document task on this binary. The rebuild (`make install`, so the controller ships generation {running}) belongs to the cycle that owns agent-doc development/release — if this session's work is not that, do NOT install from here, because a mid-session install against a live supervisor strands attached editor replicas. `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib; it cannot change which plugin build this binary expects. A rebuilt binary shipping {running} supersedes this warning."
        ),
        None => format!(
            "editor plugin generation mismatch: a live {kind} plugin reports version {running} while this agent-doc binary ships {expected}, and neither is a dotted numeric generation, so which half is behind cannot be determined. Reconcile the pair by hand before relying on editor delivery ACKs; `agent-doc admin reload-lib` refreshes only the native libagent_doc cdylib and changes neither side."
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePluginGenerationStatus {
    pub editor_id: Option<String>,
    pub pid: u32,
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
                pid: u32::try_from(registration.pid).unwrap_or_default(),
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
            // GH #233: an IDE on an unsupported platform build must hear the
            // same no-install remedy here as in the preflight warning.
            let unsupported_platform_build =
                live_plugin_install_state(&status).unsupported_platform_build;
            eprintln!(
                "[editor] {}",
                stale_plugin_message_for_platform(
                    &status.kind,
                    &status.running,
                    &status.expected,
                    installed_plugin_version(&status.kind).as_deref(),
                    plugin_staged_for_restart(&status.kind, &status.expected),
                    unsupported_platform_build,
                ),
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
    // Delegate to the pure core rather than re-deriving the message here: the
    // two bodies had drifted, so every message assertion in this module tested
    // text production never emitted.
    stale_plugin_warnings_from_statuses(
        live_plugin_generation_statuses(file),
        live_plugin_install_state,
    )
}

/// GH #115: a staged JetBrains upgrade that deleted (or, at the next start, will
/// delete) the plugin without installing its replacement. It needs no live
/// registration: a destroyed install has no plugin left to register, which is
/// exactly why `stale_plugin` stayed silent while the IDE had no agent-doc plugin.
///
/// `#jbstagebackup`: doomed stagings are repaired (purged) first rather than only
/// reported, so a restart between this preflight and the next install cannot
/// delete the plugin; each repair surfaces once as `jetbrains_plugin_staging_repaired`.
pub fn jetbrains_staged_install_failure_warnings() -> Vec<PreflightWarning> {
    let mut warnings = staging_repair_warnings_from(
        agent_doc_fs::jetbrains_install::repair_all_jetbrains_stagings(),
    );
    warnings.extend(staged_install_failure_warnings_from(
        agent_doc_fs::jetbrains_install::jetbrains_staged_install_failures(),
    ));
    warnings
}

/// Pure core of the `#jbstagebackup` repair warnings. Only a purged doomed
/// staging is operator-relevant (its upgrade must be staged again); removed
/// leftovers are housekeeping and stay silent.
pub fn staging_repair_warnings_from(
    repairs: Vec<agent_doc_fs::jetbrains_install::StagingRepair>,
) -> Vec<PreflightWarning> {
    use agent_doc_fs::jetbrains_install::{StagingRepair, staging_repair_message};
    repairs
        .into_iter()
        .filter(|repair| matches!(repair, StagingRepair::PurgedDoomed { .. }))
        .map(|repair| PreflightWarning {
            code: "jetbrains_plugin_staging_repaired".to_string(),
            message: staging_repair_message(&repair),
            document_agent: None,
            active_harness: None,
        })
        .collect()
}

/// Pure core of [`jetbrains_staged_install_failure_warnings`].
pub fn staged_install_failure_warnings_from(
    failures: Vec<(
        std::path::PathBuf,
        agent_doc_fs::jetbrains_install::StagedInstallFailure,
    )>,
) -> Vec<PreflightWarning> {
    use agent_doc_fs::jetbrains_install::{StagedInstallFailure, staged_install_failure_message};
    failures
        .into_iter()
        .map(|(dir, failure)| PreflightWarning {
            code: match failure {
                StagedInstallFailure::Destroyed { .. } => "jetbrains_plugin_install_destroyed",
                StagedInstallFailure::Doomed { .. } => "jetbrains_plugin_staging_doomed",
            }
            .to_string(),
            message: staged_install_failure_message(&dir, &failure),
            document_agent: None,
            active_harness: None,
        })
        .collect()
}

/// GH #87: is `expected` already staged for `kind`'s next IDE start? Only
/// JetBrains has a pending-install queue this side can read.
pub fn plugin_staged_for_restart(kind: &str, expected: &str) -> bool {
    kind.eq_ignore_ascii_case("jetbrains")
        && agent_doc_fs::jetbrains_install::jetbrains_plugin_staged_for_restart(expected)
}

/// On-disk installed plugin version for `kind`; only JetBrains installs are
/// discoverable from this side.
pub fn installed_plugin_version(kind: &str) -> Option<String> {
    kind.eq_ignore_ascii_case("jetbrains")
        .then(agent_doc_fs::jetbrains_install::installed_jetbrains_plugin_version)
        .flatten()
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PluginInstallState {
    installed: Option<String>,
    staged_for_restart: bool,
    /// GH #233: the live IDE's proven platform build, when no published
    /// package range declares it.
    unsupported_platform_build: Option<u32>,
}

/// Resolve install evidence from the plugin tree used by this live editor, not
/// from whichever JetBrains data directory happened to receive the newest jar.
/// A missing `/proc` mapping fails open to the generic update remedy instead of
/// borrowing another IDE's install or staging state.
fn live_plugin_install_state(status: &LivePluginGenerationStatus) -> PluginInstallState {
    if !status.kind.eq_ignore_ascii_case("jetbrains") || status.pid == 0 {
        return PluginInstallState::default();
    }
    let Some(jar_stems) = plugin_jar_stems(&status.kind) else {
        return PluginInstallState::default();
    };
    plugin_install_state_for_mapped_jar(
        &status.kind,
        &status.expected,
        &probe_mapped_plugin_jar(status.pid, jar_stems),
        &agent_doc_fs::jetbrains_install::jetbrains_system_roots(),
    )
}

fn plugin_install_state_for_mapped_jar(
    kind: &str,
    expected: &str,
    mapped: &MappedPluginJar,
    system_roots: &[PathBuf],
) -> PluginInstallState {
    if !kind.eq_ignore_ascii_case("jetbrains") {
        return PluginInstallState::default();
    }
    let Some(plugins_dir) = jetbrains_plugins_dir_from_mapped_jar(mapped) else {
        return PluginInstallState::default();
    };
    let installed = agent_doc_fs::jetbrains_install::newest_installed_artifact(
        std::slice::from_ref(&plugins_dir),
    )
    .map(|artifact| artifact.version);
    let staged_for_restart = agent_doc_fs::jetbrains_install::jetbrains_plugin_staged_in(
        std::slice::from_ref(&plugins_dir),
        system_roots,
        expected,
    );
    let unsupported_platform_build =
        agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&plugins_dir);
    PluginInstallState {
        installed,
        staged_for_restart,
        unsupported_platform_build,
    }
}

fn jetbrains_plugins_dir_from_mapped_jar(mapped: &MappedPluginJar) -> Option<PathBuf> {
    let path = match mapped {
        MappedPluginJar::Deleted { path }
        | MappedPluginJar::Superseded { path, .. }
        | MappedPluginJar::Current { path, .. } => Path::new(path),
        MappedPluginJar::Unknown => return None,
    };
    let lib_dir = path.parent()?;
    let plugin_dir = lib_dir.parent()?;
    // `#jb262dynupgrade`: both lines install into the canonical tree; an older
    // 262 install may still sit in its legacy `agent-doc-jetbrains-262/` tree.
    if lib_dir.file_name()? != "lib"
        || !matches!(
            plugin_dir.file_name()?.to_str()?,
            "agent-doc-jetbrains" | "agent-doc-jetbrains-262"
        )
    {
        return None;
    }
    plugin_dir.parent().map(Path::to_path_buf)
}

/// Shared tail of both `stale_plugin` entry points: one deduplicated warning per
/// distinct message. Two live IDEs with the same running version may need
/// different remedies when only one has the expected package staged.
fn stale_plugin_warnings_from_statuses(
    statuses: impl IntoIterator<Item = LivePluginGenerationStatus>,
    install_state_for_status: impl Fn(&LivePluginGenerationStatus) -> PluginInstallState,
) -> Vec<PreflightWarning> {
    let mut seen: HashSet<String> = HashSet::new();
    statuses
        .into_iter()
        .filter_map(|status| {
            if !status.stale {
                return None;
            }
            let install_state = install_state_for_status(&status);
            let message = stale_plugin_message_for_platform(
                &status.kind,
                &status.running,
                &status.expected,
                install_state.installed.as_deref(),
                install_state.staged_for_restart,
                install_state.unsupported_platform_build,
            );
            if !seen.insert(message.clone()) {
                return None;
            }
            Some(PreflightWarning {
                code: "stale_plugin".to_string(),
                message,
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
    stale_plugin_warnings_from_statuses(
        live_plugin_generation_statuses_from_registrations(registrations, &expected_for_kind),
        |_| PluginInstallState::default(),
    )
}

pub use agent_doc_fs::plugin_jar::{
    MappedPluginJar, classify_mapped_plugin_jar, prefer_mapped_plugin_jar, probe_mapped_plugin_jar,
};

/// Pure core: one deduplicated warning per (kind, pid) running superseded bytes.
///
/// The message has to name the version trap explicitly. An operator who reads
/// "plugin 0.2.388 is running" and "0.2.388 is installed" will otherwise
/// conclude the restart already happened, which is exactly how two review items
/// in this project sat gated on a premise that was measurably wrong.
pub fn plugin_byte_identity_warnings_from(
    probes: &[(String, u32, MappedPluginJar)],
) -> Vec<PreflightWarning> {
    plugin_byte_identity_warnings_with_restart_verdicts(probes, &HashMap::new())
}

pub use agent_doc_fs::plugin_jar::{PLUGIN_RESTART_REQUIRED_MARKER, plugin_jar_stems};

/// Like [`plugin_byte_identity_warnings_from`], but a process whose last install
/// already recorded a refused restart-free upgrade (`restart_verdicts[pid]`) is told
/// to restart. Re-running the install there can never converge: it downloads,
/// replaces another live jar, and re-learns the same refusal every cycle.
pub fn plugin_byte_identity_warnings_with_restart_verdicts(
    probes: &[(String, u32, MappedPluginJar)],
    restart_verdicts: &HashMap<u32, String>,
) -> Vec<PreflightWarning> {
    let mut seen: HashSet<(String, u32)> = HashSet::new();
    let mut warnings = Vec::new();
    for (kind, pid, mapped) in probes {
        if !mapped.is_superseded() || !seen.insert((kind.clone(), *pid)) {
            continue;
        }
        let Some(detail) = agent_doc_fs::plugin_jar::superseded_mapping_detail(mapped) else {
            continue;
        };
        let remedy = agent_doc_fs::plugin_jar::superseded_editor_remedy(
            restart_verdicts.get(pid).map(String::as_str),
        );
        warnings.push(PreflightWarning {
            code: "plugin_bytes_superseded".to_string(),
            message: format!(
                "live {kind} editor pid {pid} is running superseded plugin bytes: {detail}. \
                 The version string cannot show this — an install rewrites the jar under the \
                 same version, so a version check reports the plugin as current while the \
                 process keeps executing the replaced build. {remedy} Do not repeat status \
                 checks or reopen document tabs; neither changes the loaded bytes."
            ),
            document_agent: None,
            active_harness: None,
        });
    }
    warnings
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
        let Some(jar_stems) = plugin_jar_stems(&registration.editor_kind) else {
            continue;
        };
        let pid = u32::try_from(registration.pid).unwrap_or_default();
        if pid == 0 || !probed.insert(pid) {
            continue;
        }
        probes.push((
            registration.editor_kind.clone(),
            pid,
            probe_mapped_plugin_jar(pid, jar_stems),
        ));
    }
    let restart_verdicts = probes
        .iter()
        .filter_map(|(_, pid, mapped)| {
            agent_doc_fs::plugin_jar::recorded_restart_verdict(*pid, mapped)
                .map(|reason| (*pid, reason))
        })
        .collect::<HashMap<_, _>>();
    plugin_byte_identity_warnings_with_restart_verdicts(&probes, &restart_verdicts)
}

#[cfg(test)]
mod tests {
    use super::{
        MappedPluginJar, classify_mapped_plugin_jar, plugin_byte_identity_warnings_from,
        plugin_install_state_for_mapped_jar, plugin_jar_stems, prefer_mapped_plugin_jar,
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

    /// `#jbstagebackup`: a purged doomed staging surfaces once with the restage
    /// remedy; housekeeping removals stay silent.
    #[test]
    fn staging_repairs_warn_only_for_a_purged_doomed_staging() {
        use agent_doc_fs::jetbrains_install::{PurgedStagings, StagingRepair};
        let dir = std::path::PathBuf::from("/h/.local/share/JetBrains/IntelliJIdea2026.3");
        let warnings = super::staging_repair_warnings_from(vec![
            StagingRepair::PurgedDoomed {
                plugins_dir: dir.clone(),
                purged: PurgedStagings {
                    script: "/c/IntelliJIdea2026.3/plugins/action.script".into(),
                    zips: vec![
                        "/c/IntelliJIdea2026.3/plugins/agent-doc-jetbrains-0.2.493+a.zip".into(),
                    ],
                    removed_lines: 5,
                },
            },
            StagingRepair::RemovedProbeDir {
                dir: "/h/.local/share/JetBrains/.agent-doc-jetbrains-probe-0.2.493+a".into(),
            },
            StagingRepair::RemovedOrphanedPackage {
                zip: "/c/IntelliJIdea2026.3/plugins/agent-doc-jetbrains-0.2.469.zip".into(),
            },
        ]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "jetbrains_plugin_staging_repaired");
        assert!(warnings[0].message.contains("DOOMED"));
        assert!(
            warnings[0]
                .message
                .contains("agent-doc plugin update jetbrains")
        );
    }

    /// GH #115: each staged-install failure gets its own warning code and the
    /// reinstall remedy for its plugins directory.
    #[test]
    fn staged_install_failures_surface_distinct_codes_with_the_remedy() {
        use agent_doc_fs::jetbrains_install::StagedInstallFailure;
        let dir = std::path::PathBuf::from("/h/.local/share/JetBrains/IntelliJIdea2026.3");
        let warnings = super::staged_install_failure_warnings_from(vec![
            (
                dir.clone(),
                StagedInstallFailure::Destroyed {
                    staged: "0.2.481".to_string(),
                    previous: Some("0.2.480".to_string()),
                },
            ),
            (
                dir.clone(),
                StagedInstallFailure::Doomed {
                    staged: "0.2.482".to_string(),
                    zip: "/c/agent-doc-jetbrains-0.2.482+aa.zip".into(),
                },
            ),
        ]);
        assert_eq!(warnings[0].code, "jetbrains_plugin_install_destroyed");
        assert_eq!(warnings[1].code, "jetbrains_plugin_staging_doomed");
        for warning in &warnings {
            assert!(
                warning.message.contains(
                    "agent-doc plugin update jetbrains --plugins-dir /h/.local/share/JetBrains/IntelliJIdea2026.3"
                ),
                "{}",
                warning.message
            );
        }
        assert!(warnings[0].message.contains("NO agent-doc plugin"));
        assert!(warnings[1].message.contains("Do not restart yet"));
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

    #[test]
    fn current_generation_wins_over_lingering_deleted_dynamic_unload_mapping() {
        let old = MappedPluginJar::Deleted {
            path: "/plugins/agent-doc-jetbrains-0.2.397.jar".to_string(),
        };
        let current = MappedPluginJar::Current {
            path: "/plugins/agent-doc-jetbrains-0.2.408.jar".to_string(),
            inode: 42,
        };

        assert_eq!(prefer_mapped_plugin_jar(old, current.clone()), current);
    }

    /// Fail open. A jar we cannot stat on either side proves nothing, and an
    /// unprovable probe must never manufacture a warning that sends an operator
    /// restarting a healthy IDE.
    #[test]
    fn unstattable_mapping_is_unknown_not_superseded() {
        let jar = "/opt/idea/lib/agent-doc-jetbrains-0.2.388.jar";
        for (mapped, disk) in [(Some(1_u64), None), (None, None)] {
            let classified = classify_mapped_plugin_jar(jar, mapped, disk);
            assert_eq!(
                classified,
                MappedPluginJar::Unknown,
                "missing inode evidence must fail open: mapped={mapped:?} disk={disk:?}"
            );
            assert!(!classified.is_superseded());
        }
    }

    /// `#pluginmapfileseperm`: the shape every real Linux probe produces.
    /// `/proc/<pid>/map_files/<range>` is EPERM to `stat()` without
    /// CAP_SYS_ADMIN, so the mapped inode is never readable and the only
    /// inode we ever hold is the disk one. If that is classified `Unknown`,
    /// `MappedPluginJar::Current` is unreachable in production and the
    /// `(deleted)` residue of superseded generations decides the verdict.
    #[test]
    fn non_deleted_mapping_is_current_when_only_the_disk_inode_is_readable() {
        let jar = "/home/u/.local/share/JetBrains/x/lib/agent-doc-jetbrains-0.2.426.jar";
        let classified = classify_mapped_plugin_jar(jar, None, Some(79_464_992));
        assert_eq!(
            classified,
            MappedPluginJar::Current {
                path: jar.to_string(),
                inode: 79_464_992
            },
            "an absent (deleted) suffix over a jar still on disk is itself proof \
             the mapping is live; map_files is unstattable so no mapped inode exists"
        );
        assert!(!classified.is_superseded());
    }

    /// Regression for the live shape measured on IDEA pid 3129637
    /// (2026-09-25): six `(deleted)` mappings left behind by successive
    /// dynamic reloads plus one live mapping of the installed jar. The fold
    /// must land on the live generation and emit no warning, whatever order
    /// `read_dir` hands the entries over in.
    #[test]
    fn lingering_deleted_reload_generations_do_not_outvote_the_live_jar() {
        let dir = "/home/u/.local/share/JetBrains/x/lib";
        let live = format!("{dir}/agent-doc-jetbrains-0.2.426.jar");
        let residue: Vec<String> = ["0.2.419", "0.2.420", "0.2.421", "0.2.423", "0.2.424"]
            .iter()
            .map(|version| format!("{dir}/agent-doc-jetbrains-{version}.jar (deleted)"))
            .collect();

        for live_first in [true, false] {
            let mut links: Vec<&str> = residue.iter().map(String::as_str).collect();
            if live_first {
                links.insert(0, live.as_str());
            } else {
                links.push(live.as_str());
            }

            let mut best = MappedPluginJar::Unknown;
            for link in links {
                // Every probe sees `mapped_inode: None`; only the live jar
                // still resolves to a disk inode.
                let disk = (!link.ends_with(" (deleted)")).then_some(79_464_992_u64);
                best = prefer_mapped_plugin_jar(best, classify_mapped_plugin_jar(link, None, disk));
            }

            assert!(
                !best.is_superseded(),
                "live_first={live_first}: the installed generation is mapped and must win: {best:?}"
            );
            assert!(
                plugin_byte_identity_warnings_from(&[(
                    "jetbrains".to_string(),
                    3_129_637_u32,
                    best.clone()
                )])
                .is_empty(),
                "live_first={live_first}: a healthy IDE must not be told to reinstall"
            );
        }
    }

    /// GH #67: once an install recorded that this process refused the restart-free
    /// upgrade, prescribing another install can never converge; the advice is restart.
    #[test]
    fn supervisor_drain_handoff_undrained_warning_names_age_and_remedy() {
        let warning = super::supervisor_drain_handoff_undrained_warning(
            std::path::Path::new("tasks/doc.md"),
            1_260,
        );
        assert_eq!(warning.code, "supervisor_drain_handoff_undrained");
        assert!(
            warning.message.contains("21 min ago"),
            "{}",
            warning.message
        );
        assert!(
            warning.message.contains("still undrained"),
            "{}",
            warning.message
        );
        assert!(
            warning.message.contains("restart-supervisor"),
            "{}",
            warning.message
        );
    }

    #[test]
    fn a_recorded_restart_verdict_turns_the_remedy_into_restart() {
        let probes = vec![(
            "jetbrains".to_string(),
            1_506_046_u32,
            MappedPluginJar::Deleted {
                path: "/p/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.392.jar".to_string(),
            },
        )];
        let verdicts = std::collections::HashMap::from([(
            1_506_046_u32,
            "dynamic upgrade unavailable: plugin cannot unload dynamically".to_string(),
        )]);
        let warnings =
            super::plugin_byte_identity_warnings_with_restart_verdicts(&probes, &verdicts);
        let message = &warnings[0].message;
        assert!(
            message.contains("Restart the editor to load them"),
            "{message}"
        );
        assert!(
            message.contains("plugin cannot unload dynamically"),
            "{message}"
        );
        assert!(
            !message.contains("Re-run the plugin installation"),
            "{message}"
        );

        let without = plugin_byte_identity_warnings_from(&probes);
        assert!(
            without[0]
                .message
                .contains("Re-run the plugin installation once")
        );
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
        assert!(
            warning
                .message
                .contains("Re-run the plugin installation once"),
            "the first recovery must invoke the dynamic replacement path: {}",
            warning.message
        );
        assert!(
            warning
                .message
                .contains("Do not repeat status checks or reopen document tabs"),
            "passive checks cannot repair loaded bytes: {}",
            warning.message
        );
        assert!(
            warning
                .message
                .contains("only if that install explicitly reports a dynamic-unload failure"),
            "restart must be the explicit unload-failure fallback: {}",
            warning.message
        );
    }

    /// Only editors that load agent-doc as a jar inside their own process can be
    /// probed this way; claiming otherwise would emit a permanently-unknown probe.
    #[test]
    fn jar_stem_is_scoped_to_jvm_hosted_editors() {
        for kind in ["jetbrains", "JetBrains", "intellij", "idea"] {
            assert_eq!(
                plugin_jar_stems(kind),
                Some(&["agent-doc-jetbrains-", "agent.doc-"][..]),
                "{kind}"
            );
        }
        for kind in ["vscode", "zed", "neovim", ""] {
            assert_eq!(plugin_jar_stems(kind), None, "{kind}");
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

    /// `#stalepluginremedydirection`: measured live 2026-09-25 — plugin 0.2.426
    /// against a binary shipping 0.2.422. The mismatch is real, but the plugin
    /// is the NEWER half, so "install/update the editor plugin" is the one
    /// remedy that cannot work and hides the half that is actually behind.
    #[test]
    fn a_newer_live_plugin_blames_the_binary_not_the_plugin() {
        let newer = vec![registration_with_editor("jetbrains", "0.2.426")];
        let warnings = stale_plugin_warnings_from_registrations(&newer, |_| Some("0.2.422"));
        assert_eq!(warnings.len(), 1);
        let message = &warnings[0].message;
        assert!(
            message.contains("newer than the 0.2.422 build"),
            "the direction must be stated as measured: {message}"
        );
        assert!(
            message.contains("`make install`"),
            "the remedy must target the stale half — the CLI rebuild: {message}"
        );
        assert!(
            !message.contains("Install/update the jetbrains plugin"),
            "the newer half must never be sent for reinstallation: {message}"
        );
        // `#staleinstallremedyowner`: naming the stale half is not the same as
        // ordering THIS session to rebuild it. The warning reaches every document
        // session, and a mid-session install against a live supervisor strands
        // attached editor replicas, so the remedy must name its owner and say the
        // warning is advisory for everyone else.
        assert!(
            message.contains("ADVISORY"),
            "a session doing unrelated work must be told it can continue: {message}"
        );
        assert!(
            message.contains("owns agent-doc development/release"),
            "the rebuild must name its owning cycle: {message}"
        );
        assert!(
            message.contains("strands attached editor replicas"),
            "the reason a non-owner must not install must be stated: {message}"
        );
        assert_eq!(
            stale_generation_side("0.2.426", "0.2.422"),
            Some(StaleGenerationSide::Binary)
        );
        assert_eq!(
            stale_generation_side("0.2.422", "0.2.426"),
            Some(StaleGenerationSide::Plugin)
        );
        assert_eq!(
            stale_generation_side("nightly", "0.2.426"),
            None,
            "an unparseable generation must not manufacture a direction"
        );
    }

    /// The doc comment called `stale_plugin_warnings_from_registrations` the
    /// "pure core" of `stale_plugin_warnings` while the two built different
    /// messages, so every message assertion above tested text production never
    /// emitted. Pin them to one body.
    #[test]
    fn both_stale_plugin_entry_points_emit_one_shared_message() {
        let registrations = vec![registration_with_editor("jetbrains", "0.2.205")];
        let core = stale_plugin_warnings_from_registrations(&registrations, |_| Some("0.2.206"));
        let statuses =
            live_plugin_generation_statuses_from_registrations(&registrations, |_| Some("0.2.206"));
        let shared =
            stale_plugin_warnings_from_statuses(statuses, |_| PluginInstallState::default());
        assert_eq!(core.len(), 1);
        assert_eq!(shared.len(), 1);
        assert_eq!(
            core[0].message, shared[0].message,
            "the production path and the tested core must not drift apart again"
        );
        assert_eq!(
            core[0].message,
            stale_plugin_message("jetbrains", "0.2.205", "0.2.206", None, false)
        );
    }

    /// `#jb262dynupgrade`: a live exact-262 IDE maps the modular root jar
    /// `agent.doc-<v>.jar`; its install state resolves from the same plugin
    /// tree as a classic mapping, canonical or legacy `agent-doc-jetbrains-262`.
    #[test]
    fn stale_plugin_install_state_reads_a_modular_262_mapped_jar() {
        for plugin_dir in ["agent-doc-jetbrains", "agent-doc-jetbrains-262"] {
            let tmp = TempDir::new().unwrap();
            let live = tmp.path().join("data/IntelliJIdea2026.2");
            let lib = live.join(plugin_dir).join("lib");
            std::fs::create_dir_all(&lib).unwrap();
            let live_jar = lib.join("agent.doc-0.2.514.jar");
            std::fs::write(&live_jar, b"live").unwrap();
            let mapped = MappedPluginJar::Current {
                path: live_jar.to_string_lossy().into_owned(),
                inode: 1,
            };
            let state = plugin_install_state_for_mapped_jar("jetbrains", "0.2.515", &mapped, &[]);
            assert_eq!(state.installed.as_deref(), Some("0.2.514"), "{plugin_dir}");
            assert!(!state.staged_for_restart, "{plugin_dir}");
        }
    }

    /// GH #233: a live IDE whose platform build no published package declares
    /// (263, past the classic 261 cap and the modular 262 range) must not be
    /// told to "install {expected} first": no package can be installed there.
    #[test]
    fn stale_plugin_on_unsupported_platform_build_names_no_unreachable_install() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("data/IntelliJIdea2026.3");
        let lib = live.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&lib).unwrap();
        let live_jar = lib.join("agent-doc-jetbrains-0.2.508.jar");
        std::fs::write(&live_jar, b"live").unwrap();
        let mapped = MappedPluginJar::Current {
            path: live_jar.to_string_lossy().into_owned(),
            inode: 1,
        };
        let state = plugin_install_state_for_mapped_jar("jetbrains", "0.2.517", &mapped, &[]);
        assert_eq!(state.installed.as_deref(), Some("0.2.508"));
        assert_eq!(state.unsupported_platform_build, Some(263));

        let status = LivePluginGenerationStatus {
            editor_id: Some("e".to_string()),
            pid: 1,
            kind: "jetbrains".to_string(),
            running: "0.2.508".to_string(),
            expected: "0.2.517".to_string(),
            timestamp_ms: 1,
            stale: true,
        };
        let warnings = stale_plugin_warnings_from_statuses(vec![status], |_| state.clone());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "stale_plugin");
        let message = &warnings[0].message;
        for needle in [
            "platform build 263",
            "242-261 (agent-doc-jetbrains-<version>.zip)",
            "262 (agent-doc-jetbrains-262-<version>.zip)",
            "plugin 0.2.508 stays installed",
            "ADVISORY",
        ] {
            assert!(message.contains(needle), "missing {needle:?}: {message}");
        }
        for remedy in [
            "Install 0.2.517 first",
            "agent-doc plugin install",
            "--local",
        ] {
            assert!(!message.contains(remedy), "offers {remedy:?}: {message}");
        }

        // A supported 262 IDE with the same skew keeps the install-first remedy.
        let supported = stale_plugin_message_for_platform(
            "jetbrains",
            "0.2.508",
            "0.2.517",
            Some("0.2.508"),
            false,
            None,
        );
        assert!(supported.contains("Install 0.2.517 first"), "{supported}");
        // The binary-is-stale branch is unaffected by the platform build.
        let binary = stale_plugin_message_for_platform(
            "jetbrains",
            "0.2.520",
            "0.2.517",
            Some("0.2.520"),
            false,
            Some(263),
        );
        assert!(binary.starts_with("stale agent-doc binary"), "{binary}");
    }

    /// GH #180: an idle IDE data directory can hold a newer jar or a pending
    /// staging. Neither is evidence about the live IDE whose mapped jar names a
    /// different plugin tree.
    #[test]
    fn stale_plugin_install_state_is_scoped_to_the_live_mapped_jar() {
        let tmp = TempDir::new().unwrap();
        let live = tmp.path().join("data/IntelliJIdea2026.3");
        let idle = tmp.path().join("data/IntelliJIdea2026.2");
        let live_lib = live.join("agent-doc-jetbrains/lib");
        let idle_lib = idle.join("agent-doc-jetbrains/lib");
        std::fs::create_dir_all(&live_lib).unwrap();
        std::fs::create_dir_all(&idle_lib).unwrap();
        let live_jar = live_lib.join("agent-doc-jetbrains-0.2.502.jar");
        std::fs::write(&live_jar, b"live").unwrap();
        std::fs::write(idle_lib.join("agent-doc-jetbrains-0.2.503.jar"), b"idle").unwrap();

        let mapped = MappedPluginJar::Current {
            path: live_jar.to_string_lossy().into_owned(),
            inode: 1,
        };
        std::fs::write(
            live.join(agent_doc_fs::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
            "agent-doc declined the restart-free upgrade\nstaged_version=0.2.503\n",
        )
        .unwrap();
        let staged = plugin_install_state_for_mapped_jar("jetbrains", "0.2.503", &mapped, &[]);
        assert_eq!(staged.installed.as_deref(), Some("0.2.502"));
        assert!(staged.staged_for_restart);
        let staged_message = stale_plugin_message(
            "jetbrains",
            "0.2.502",
            "0.2.503",
            staged.installed.as_deref(),
            staged.staged_for_restart,
        );
        assert!(
            staged_message.contains("ALREADY STAGED"),
            "{staged_message}"
        );
        assert!(
            !staged_message.contains("ALREADY installed on disk"),
            "{staged_message}"
        );

        std::fs::remove_file(live.join(agent_doc_fs::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER))
            .unwrap();
        std::fs::write(
            idle.join(agent_doc_fs::plugin_jar::PLUGIN_RESTART_REQUIRED_MARKER),
            "agent-doc declined the restart-free upgrade\nstaged_version=0.2.503\n",
        )
        .unwrap();
        let not_staged = plugin_install_state_for_mapped_jar("jetbrains", "0.2.503", &mapped, &[]);
        assert_eq!(not_staged.installed.as_deref(), Some("0.2.502"));
        assert!(!not_staged.staged_for_restart);
        let update_message = stale_plugin_message(
            "jetbrains",
            "0.2.502",
            "0.2.503",
            not_staged.installed.as_deref(),
            not_staged.staged_for_restart,
        );
        assert!(
            update_message.contains("on-disk install is 0.2.502"),
            "{update_message}"
        );
        assert!(
            !update_message.contains("ALREADY STAGED"),
            "{update_message}"
        );
    }

    #[test]
    fn stale_plugin_remedy_follows_the_on_disk_install() {
        // `#gh76secondary`: the warning said "update to 0.2.453" while disk held
        // 0.2.451, so following it could not reach parity even after a restart.
        let behind =
            stale_plugin_message("jetbrains", "0.2.392", "0.2.453", Some("0.2.451"), false);
        assert!(behind.contains("on-disk install is 0.2.451"), "{behind}");
        assert!(
            behind.contains("agent-doc plugin install jetbrains"),
            "{behind}"
        );
        assert!(
            behind.contains("restart alone cannot reach parity"),
            "{behind}"
        );

        // Disk already current: restart, never reinstall.
        let current =
            stale_plugin_message("jetbrains", "0.2.392", "0.2.453", Some("0.2.453"), false);
        assert!(current.contains("ALREADY installed on disk"), "{current}");
        assert!(current.contains("do not reinstall"), "{current}");

        // Unknown install falls back to the generic remedy.
        let unknown = stale_plugin_message("jetbrains", "0.2.392", "0.2.453", None, false);
        assert!(unknown.contains("update to 0.2.453"), "{unknown}");

        // The binary-stale side ignores the on-disk plugin.
        assert_eq!(
            stale_plugin_message("jetbrains", "0.2.454", "0.2.453", Some("0.2.451"), false),
            stale_plugin_message("jetbrains", "0.2.454", "0.2.453", None, false),
        );

        // The production core threads the lookup into the message.
        let statuses = vec![LivePluginGenerationStatus {
            editor_id: Some("e".to_string()),
            pid: 1,
            kind: "jetbrains".to_string(),
            running: "0.2.392".to_string(),
            expected: "0.2.453".to_string(),
            timestamp_ms: 1,
            stale: true,
        }];
        let warnings =
            stale_plugin_warnings_from_statuses(statuses.clone(), |_| PluginInstallState {
                installed: Some("0.2.451".into()),
                staged_for_restart: false,
                unsupported_platform_build: None,
            });
        assert!(warnings[0].message.contains("on-disk install is 0.2.451"));

        // GH #87: the same on-disk state with {expected} staged for the next IDE
        // start is restart-only, through the production core too.
        let staged = stale_plugin_warnings_from_statuses(statuses, |_| PluginInstallState {
            installed: Some("0.2.451".into()),
            staged_for_restart: true,
            unsupported_platform_build: None,
        });
        assert!(
            staged[0].message.contains("ALREADY STAGED"),
            "{}",
            staged[0].message
        );
    }

    #[test]
    fn staged_install_is_restart_only_never_install_first() {
        // GH #87: disk still holds 0.2.455 because the IDE applies the staged
        // 0.2.459 package only as it starts.
        let staged = stale_plugin_message("jetbrains", "0.2.455", "0.2.459", Some("0.2.455"), true);
        assert!(staged.contains("ALREADY STAGED"), "{staged}");
        assert!(staged.contains("restart the IDE"), "{staged}");
        assert!(staged.contains("do not reinstall"), "{staged}");
        assert!(!staged.contains("cannot reach parity"), "{staged}");
        assert!(!staged.contains("agent-doc plugin install"), "{staged}");

        // Unknown on-disk version + staged: still restart-only.
        let unknown = stale_plugin_message("jetbrains", "0.2.455", "0.2.459", None, true);
        assert!(unknown.contains("ALREADY STAGED"), "{unknown}");

        // Disk already current keeps the installed-on-disk wording.
        let current =
            stale_plugin_message("jetbrains", "0.2.455", "0.2.459", Some("0.2.459"), true);
        assert!(current.contains("ALREADY installed on disk"), "{current}");

        // A staged record never changes the binary-stale remedy.
        assert_eq!(
            stale_plugin_message("jetbrains", "0.2.460", "0.2.459", Some("0.2.455"), true),
            stale_plugin_message("jetbrains", "0.2.460", "0.2.459", Some("0.2.455"), false),
        );
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
