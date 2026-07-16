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

/// Build the warning companion for semantic-merge acks carried from the prior
/// cycle into this preflight output.
pub fn document_cell_merge_ack_warning(
    document_cell_merge_acks: &[agent_doc_cycle_state_io::PendingSemanticMergeAck],
) -> Option<PreflightWarning> {
    if document_cell_merge_acks.is_empty() {
        return None;
    }
    let summary = document_cell_merge_acks
        .iter()
        .map(|ack| format!("{}:{} ({})", ack.component, ack.id, ack.reason))
        .collect::<Vec<_>>()
        .join(", ");
    Some(PreflightWarning {
        code: "document_cell_merge_ack_pending".to_string(),
        message: format!(
            "{} node-keyed semantic-merge ack(s) from the prior cycle: {summary}. The operator's concurrent edit won these node(s); acknowledge the non-applied agent change(s) in an exchange turn this cycle.",
            document_cell_merge_acks.len()
        ),
        document_agent: None,
        active_harness: None,
    })
}

/// Warn when the installed/built `agent-doc` artifacts predate the latest local
/// source edit, so live sessions (tmux, JetBrains) do not silently run stale code
/// at an unchanged version string (`#install-stale-guard`). Best-effort: only
/// fires when an `agent-doc` source repo is locatable (development / dogfooding)
/// and silently no-ops otherwise (for example a prebuilt or PyPI install with no source).
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

    Some(PreflightWarning {
        code: "stale_install".to_string(),
        message: format!(
            "stale agent-doc install: {} predate the latest local source edit - live sessions (tmux / JetBrains) may run pre-edit code at an unchanged version. Run `make install` in {} to rebuild the binary + cdylib.",
            stale.join(", "),
            repo.display()
        ),
        document_agent: None,
        active_harness: None,
    })
}

/// `#stale-plugin-detect`: the expected editor-plugin version this binary ships
/// with, baked at build time from `editors/{jetbrains/gradle.properties,
/// vscode/package.json}` (see `agent-doc-preflight-io/build.rs`). `None` when
/// the editor sources were absent at build time.
pub fn expected_plugin_version(editor_kind: &str) -> Option<&'static str> {
    match editor_kind.trim().to_ascii_lowercase().as_str() {
        "jetbrains" | "intellij" | "idea" | "jb" => {
            option_env!("AGENT_DOC_EXPECTED_JETBRAINS_PLUGIN_VERSION")
        }
        "vscode" | "vs-code" | "code" => option_env!("AGENT_DOC_EXPECTED_VSCODE_PLUGIN_VERSION"),
        _ => None,
    }
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
pub fn live_plugin_generation_statuses_from_snapshots(
    snapshots: &[agent_doc_debounce::LiveBufferSnapshot],
    expected_for_kind: impl Fn(&str) -> Option<&'static str>,
) -> Vec<LivePluginGenerationStatus> {
    let mut latest: HashMap<(String, String), &agent_doc_debounce::LiveBufferSnapshot> =
        HashMap::new();
    for snapshot in snapshots {
        let (Some(kind), Some(running)) = (
            snapshot.editor_kind.as_deref(),
            snapshot.editor_version.as_deref(),
        ) else {
            continue;
        };
        if expected_for_kind(kind).is_none() {
            continue;
        }
        let key = (
            kind.to_ascii_lowercase(),
            snapshot.editor_id.clone().unwrap_or_default(),
        );
        let replace = latest.get(&key).is_none_or(|current| {
            agent_doc_workflow::capture::decide_plugin_generation_refresh(
                agent_doc_workflow::capture::PluginGenerationRefreshEvidence {
                    preflight_generation: current.timestamp_ms,
                    live_generation: snapshot.timestamp_ms,
                    live_registration_observed: true,
                },
            ) == agent_doc_workflow::capture::PluginGenerationRefreshDecision::AdoptLive
        });
        if replace {
            latest.insert(key, snapshot);
        }
        let _ = running;
    }
    let mut statuses = latest
        .into_values()
        .filter_map(|snapshot| {
            let kind = snapshot.editor_kind.as_deref()?;
            let running = snapshot.editor_version.as_deref()?;
            let expected = expected_for_kind(kind)?;
            Some(LivePluginGenerationStatus {
                editor_id: snapshot.editor_id.clone(),
                kind: kind.to_string(),
                running: running.to_string(),
                expected: expected.to_string(),
                timestamp_ms: snapshot.timestamp_ms,
                stale: plugin_version_is_older(running, expected),
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
    agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|registration| {
            let expected = expected_plugin_version(&registration.editor_kind)?;
            Some(LivePluginGenerationStatus {
                editor_id: Some(registration.editor_id),
                kind: registration.editor_kind,
                stale: plugin_version_is_older(&registration.editor_version, expected),
                running: registration.editor_version,
                expected: expected.to_string(),
                timestamp_ms: u128::from(registration.timestamp_ms),
            })
        })
        .collect()
}

/// Emit replay-time generation evidence. This deliberately runs after preflight
/// so a plugin installed/restarted during the turn is recognized immediately.
pub fn report_live_plugin_generation_refresh(file: &Path) {
    for status in live_plugin_generation_statuses(file) {
        if status.stale {
            eprintln!(
                "[editor] live {} plugin {} is older than expected {}; install/update the editor plugin and restart/reload the IDE host. `agent-doc admin reload-lib` refreshes only libagent_doc and cannot change plugin code or its reported version.",
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
/// older than the plugin build this binary ships with, and warn so the operator
/// reinstalls it.
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
                    "live {} editor plugin {} is older than the {} build shipped with this agent-doc binary; install/update the editor plugin and restart/reload the IDE host before relying on editor delivery ACKs.",
                    status.kind, status.running, status.expected
                ),
                document_agent: None,
                active_harness: None,
            })
        })
        .collect()
}

/// Pure core of [`stale_plugin_warnings`]: given the live per-editor snapshots
/// and an expected-version resolver, produce one deduplicated warning per stale
/// (kind, version) pair.
pub fn stale_plugin_warnings_from_snapshots(
    snapshots: &[agent_doc_debounce::LiveBufferSnapshot],
    expected_for_kind: impl Fn(&str) -> Option<&'static str>,
) -> Vec<PreflightWarning> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut warnings = Vec::new();
    for status in live_plugin_generation_statuses_from_snapshots(snapshots, &expected_for_kind) {
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

#[cfg(test)]
mod tests {
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
            option_env!("AGENT_DOC_EXPECTED_JETBRAINS_PLUGIN_VERSION")
        );
        assert_eq!(
            expected_plugin_version("JetBrains"),
            expected_plugin_version("intellij")
        );
        assert_eq!(
            expected_plugin_version("vscode"),
            option_env!("AGENT_DOC_EXPECTED_VSCODE_PLUGIN_VERSION")
        );
    }

    fn snapshot_with_editor(kind: &str, version: &str) -> agent_doc_debounce::LiveBufferSnapshot {
        agent_doc_debounce::LiveBufferSnapshot {
            path: "/tmp/doc.md".to_string(),
            len: 0,
            hash: String::new(),
            timestamp_ms: 0,
            edit_epoch: 0,
            last_synced_epoch: 0,
            state_vector_b64: None,
            editor_id: None,
            editor_kind: Some(kind.to_string()),
            editor_version: Some(version.to_string()),
            capabilities: Vec::new(),
            content: None,
            no_unsaved_operator_edits: false,
        }
    }

    fn snapshot_with_generation(
        editor_id: &str,
        version: &str,
        timestamp_ms: u128,
    ) -> agent_doc_debounce::LiveBufferSnapshot {
        let mut snapshot = snapshot_with_editor("jetbrains", version);
        snapshot.editor_id = Some(editor_id.to_string());
        snapshot.timestamp_ms = timestamp_ms;
        snapshot
    }

    #[test]
    fn stale_plugin_warning_flags_older_live_plugin() {
        let snapshots = vec![snapshot_with_editor("jetbrains", "0.2.205")];
        let warnings = stale_plugin_warnings_from_snapshots(&snapshots, |kind| {
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
        let snapshots = vec![
            snapshot_with_generation("idea-project", "0.2.261", 10),
            snapshot_with_generation("idea-project", "0.2.263", 20),
        ];
        let statuses =
            live_plugin_generation_statuses_from_snapshots(&snapshots, |_| Some("0.2.263"));
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].running, "0.2.263");
        assert!(!statuses[0].stale);
    }

    #[test]
    fn replay_boundary_keeps_independent_editor_generations_separate() {
        let snapshots = vec![
            snapshot_with_generation("idea-a", "0.2.261", 10),
            snapshot_with_generation("idea-b", "0.2.263", 20),
        ];
        let statuses =
            live_plugin_generation_statuses_from_snapshots(&snapshots, |_| Some("0.2.263"));
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().any(|status| status.stale));
        assert!(statuses.iter().any(|status| !status.stale));
    }

    #[test]
    fn stale_plugin_warning_silent_for_current_or_unknown() {
        let current = vec![snapshot_with_editor("jetbrains", "0.2.206")];
        assert!(
            stale_plugin_warnings_from_snapshots(&current, |_| Some("0.2.206")).is_empty(),
            "a current plugin must not warn"
        );
        let newer = vec![snapshot_with_editor("jetbrains", "0.2.207")];
        assert!(
            stale_plugin_warnings_from_snapshots(&newer, |_| Some("0.2.206")).is_empty(),
            "a newer plugin must not warn"
        );
        let no_expectation = vec![snapshot_with_editor("jetbrains", "0.2.100")];
        assert!(
            stale_plugin_warnings_from_snapshots(&no_expectation, |_| None).is_empty(),
            "no baked expectation must not warn (fail-open)"
        );
    }

    #[test]
    fn stale_plugin_warning_dedups_identical_kind_version() {
        let snapshots = vec![
            snapshot_with_editor("vscode", "0.2.38"),
            snapshot_with_editor("vscode", "0.2.38"),
        ];
        let warnings = stale_plugin_warnings_from_snapshots(&snapshots, |_| Some("0.2.39"));
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
            &dir.path().join(".agent-doc/reliable_sync_outbox.db"),
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
