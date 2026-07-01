//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn auto_start_candidate_files(col_args: &[String]) -> Vec<PathBuf> {
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut out: Vec<PathBuf> = Vec::new();
    for path in col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(|s| PathBuf::from(s.trim()))
    {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}

#[derive(Default)]
pub(crate) struct SyncProofCache {
    pub(crate) actor_records:
        RefCell<HashMap<(PathBuf, String), Option<agent_doc_sqlite::state_store::ActorRecord>>>,
    pub(crate) live_owner_matches: RefCell<HashMap<(PathBuf, String, String), bool>>,
}

pub(crate) fn sync_proof_file_key(file: &Path) -> PathBuf {
    file.canonicalize().unwrap_or_else(|_| file.to_path_buf())
}

pub(crate) fn registered_pane_proves_live_owner(
    tmux: &Tmux,
    file_path: &Path,
    session_id: &str,
    pane_id: &str,
    proof_cache: &SyncProofCache,
) -> bool {
    if !tmux.pane_alive(pane_id) {
        return false;
    }
    sync_actor_or_live_owner_matches_cached(tmux, file_path, session_id, pane_id, proof_cache)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProtectedRegisteredPaneState {
    pub(crate) reason: String,
    pub(crate) last_visible_excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenCycleProtectedPaneState {
    pub(crate) file: PathBuf,
    pub(crate) phase: &'static str,
}

pub(crate) fn resolve_harness_for_sync(file: &Path) -> agent_doc_harness::HarnessConfig {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    rc.set_doc_content(content);
    let fm = rc.frontmatter();
    let global_config = rc.global_config();
    agent_doc_harness::HarnessConfig::from_context(&fm, &global_config)
}

pub(crate) fn protected_registered_pane_state(
    tmux: &Tmux,
    file: &Path,
    pane_id: &str,
) -> Option<ProtectedRegisteredPaneState> {
    if !tmux.pane_alive(pane_id) {
        return None;
    }

    let capture = sessions::capture_pane(tmux, pane_id).ok()?;
    protected_registered_pane_state_from_capture(file, &capture)
}

pub(crate) fn protected_registered_pane_state_from_capture(
    file: &Path,
    capture: &str,
) -> Option<ProtectedRegisteredPaneState> {
    let harness = resolve_harness_for_sync(file);
    let reason = harness.protected_prompt_input_reason(capture)?;
    if reason == "active permission prompt" {
        crate::input_diag::log_prompt_detection(
            Some(file),
            "sync.protected_registered_pane",
            "registered_pane",
            &harness.binary,
            &reason,
            "active",
        );
    }
    Some(ProtectedRegisteredPaneState {
        reason,
        last_visible_excerpt: last_visible_excerpt(capture),
    })
}

pub(crate) fn open_cycle_protected_file_state(file: &Path) -> Option<OpenCycleProtectedPaneState> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    let phase = match state.phase {
        agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
        agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
        agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
        agent_doc_turn::CyclePhase::Committed | agent_doc_turn::CyclePhase::Abandoned => {
            return None;
        }
    };
    Some(OpenCycleProtectedPaneState {
        file: file.to_path_buf(),
        phase,
    })
}

pub(crate) fn registered_file_for_pane(tmux: &Tmux, pane_id: &str) -> Option<PathBuf> {
    let project_root = pane_project_root(tmux, pane_id)?;
    let registry = sessions::load_in(&project_root).ok()?;
    let entry = registry
        .values()
        .find(|entry| entry.pane == pane_id && !entry.file.is_empty())?;
    let file = Path::new(&entry.file);
    Some(if file.is_absolute() {
        file.to_path_buf()
    } else {
        project_root.join(file)
    })
}

pub(crate) fn open_cycle_protected_pane_state(
    tmux: &Tmux,
    pane_id: &str,
) -> Option<OpenCycleProtectedPaneState> {
    let file = registered_file_for_pane(tmux, pane_id)?;
    open_cycle_protected_file_state(&file)
}

pub(crate) fn projected_sync_pane_count(col_args: &[String]) -> usize {
    col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim)
        .filter(|file| !file.is_empty())
        .map(|file| {
            let path = PathBuf::from(file);
            path.canonicalize().unwrap_or(path)
        })
        .collect::<HashSet<_>>()
        .len()
}

pub(crate) fn select_visible_focus_pane_if_present(
    tmux: &Tmux,
    window: &str,
    focus: Option<&str>,
) -> Option<String> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    let focus_path = PathBuf::from(focus);
    let canonical_focus = focus_path.canonicalize().unwrap_or(focus_path);
    for pane in tmux.list_window_panes(window).unwrap_or_default() {
        let Some(file) = registered_file_for_pane(tmux, &pane) else {
            continue;
        };
        let canonical_file = file.canonicalize().unwrap_or(file);
        if canonical_file != canonical_focus || !tmux.pane_alive(&pane) {
            continue;
        }
        if let Err(err) = tmux.select_pane(&pane) {
            let warning = format!(
                "[sync] warning: failed to reselect visible focus pane {} for {} while preserving layout: {}",
                pane,
                canonical_focus.display(),
                err
            );
            eprintln!("{}", warning);
            sync_log(&warning);
            return None;
        }
        return Some(pane);
    }
    None
}

pub(crate) fn emit_preserved_layout_focus_marker(pane: &str, reason: &str) {
    let marker = format!(
        "[sync] safe_passive_layout_preserved_reselected_focus pane={} reason={}",
        pane, reason
    );
    eprintln!("{}", marker);
    sync_log(&marker);
}

pub(crate) fn persist_dead_pane_capture(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    tail: &str,
) -> Option<PathBuf> {
    if tail.trim().is_empty() {
        return None;
    }
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)?;
    let dir = root.join(".agent-doc/logs/dead-panes");
    std::fs::create_dir_all(&dir).ok()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pane_token = pane_id.trim_start_matches('%');
    let path = dir.join(format!("{session_id}-{timestamp}-pane-{pane_token}.log"));
    std::fs::write(&path, tail).ok()?;
    Some(path)
}

pub(crate) fn capture_dead_pane_diagnostics(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    last_known_window: Option<&str>,
) -> Result<Option<DeadPaneDiagnostics>> {
    if !tmux.pane_dead(pane_id) {
        return Ok(None);
    }

    let dead_status = tmux.pane_dead_status(pane_id)?;
    let observed_window = tmux
        .pane_window(pane_id)
        .ok()
        .or_else(|| last_known_window.map(ToOwned::to_owned));
    let cycle_phase = cycle_phase_label(file);
    let tail = tmux.capture_pane(pane_id, Some(80)).unwrap_or_default();
    let capture_path = persist_dead_pane_capture(file, session_id, pane_id, &tail);
    let last_visible_excerpt = last_visible_excerpt(&tail);
    let mut event = format!(
        "pane_death_detected pane={pane_id} status={} cycle_phase={}",
        dead_status.as_deref().unwrap_or("unknown"),
        cycle_phase.as_deref().unwrap_or("none")
    );
    if let Some(window_id) = observed_window.as_deref() {
        event.push_str(&format!(" window={window_id}"));
    }
    if let Some(path) = capture_path.as_ref() {
        event.push_str(&format!(" capture={}", path.display()));
    }
    if let Some(excerpt) = last_visible_excerpt.as_deref() {
        event.push_str(&format!(" last_visible_excerpt={excerpt}"));
    }
    let _ = crate::startup_miss::append_session_log_event(file, session_id, &event);

    let _ = crate::startup_miss::append_session_log_event(
        file,
        session_id,
        &format!(
            "pane_death_cleanup pane={pane_id} action=keep_dead policy=normal_sync_never_kills"
        ),
    );

    Ok(Some(DeadPaneDiagnostics {
        observed_window,
        dead_status,
        cycle_phase,
        capture_path,
        last_visible_excerpt,
        pane_killed: false,
    }))
}

pub(crate) fn recover_missing_pane_closeout(
    file: &Path,
    session_id: &str,
    pane_id: &str,
) -> (
    Option<String>,
    Option<crate::repair::RepairOutcome>,
    Option<String>,
) {
    let state = match crate::cycle_state::load(file) {
        Ok(state) => state,
        Err(err) => {
            return (
                None,
                None,
                sanitize_excerpt(&format!("failed to load cycle state: {err}")),
            );
        }
    };
    let Some(state) = state else {
        return (None, None, None);
    };
    let phase = match state.phase {
        agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
        agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
        _ => return (None, None, None),
    };
    let capture_present = crate::capture::load_active(file).ok().flatten().is_some();
    let _ = crate::startup_miss::append_session_log_event(
        file,
        session_id,
        &format!(
            "sync_missing_pane_closeout_recovery_start pane={pane_id} cycle={} phase={phase} durable_capture={capture_present}",
            state.cycle_id
        ),
    );
    match crate::repair::repair(file) {
        Ok(outcome) => {
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!(
                    "sync_missing_pane_closeout_recovery_result pane={pane_id} cycle={} phase={phase} outcome={}",
                    state.cycle_id,
                    repair_outcome_label(outcome)
                ),
            );
            (Some(phase.to_string()), Some(outcome), None)
        }
        Err(err) => {
            let detail =
                sanitize_excerpt(&err.to_string()).unwrap_or_else(|| "unknown".to_string());
            let _ = crate::startup_miss::append_session_log_event(
                file,
                session_id,
                &format!(
                    "sync_missing_pane_closeout_recovery_failed pane={pane_id} cycle={} phase={phase} reason={detail}",
                    state.cycle_id
                ),
            );
            (Some(phase.to_string()), None, Some(detail))
        }
    }
}

pub(crate) fn pending_missing_pane_repair_phase(file: &Path) -> Option<&'static str> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    match state.phase {
        agent_doc_turn::CyclePhase::PreflightStarted => Some("preflight_started"),
        agent_doc_turn::CyclePhase::ResponseCaptured => Some("response_captured"),
        agent_doc_turn::CyclePhase::WriteApplied => Some("write_applied"),
        agent_doc_turn::CyclePhase::Committed | agent_doc_turn::CyclePhase::Abandoned => None,
    }
}

pub(crate) fn missing_pane_manual_repair_reason(file: &Path, phase: &str) -> String {
    if let Ok(Some(message)) = crate::session_check::detect_uncommitted_closeout_drift(file) {
        return message;
    }
    let detail = match phase {
        "preflight_started" => "stale preflight state is still open",
        "response_captured" => "a captured response still needs explicit closeout recovery",
        "write_applied" => "the write reached disk but the commit boundary is still open",
        _ => "manual repair is still required",
    };
    format!(
        "normal sync will not auto-repair {phase} for {} ({}). Run `agent-doc repair {}` or `agent-doc session doctor {} --repair` before syncing again",
        file.display(),
        detail,
        file.display(),
        file.display()
    )
}

pub(crate) fn missing_pane_closeout_block_reason(
    file: &Path,
    phase: &str,
    error: Option<&str>,
) -> String {
    if let Ok(Some(message)) = crate::session_check::detect_uncommitted_closeout_drift(file) {
        return message;
    }
    let detail = error.unwrap_or("unknown");
    format!(
        "closeout recovery for {phase} failed and needs manual repair ({detail}). Re-run `agent-doc repair {}` before syncing again",
        file.display()
    )
}

pub(crate) fn repair_missing_registered_pane(
    tmux: &Tmux,
    file: &Path,
    session_id: &str,
    pane_id: &str,
    last_known_window: Option<&str>,
    mode: MissingRegisteredPaneRepairMode,
) -> Result<MissingRegisteredPaneRepair> {
    let dead_pane =
        capture_dead_pane_diagnostics(tmux, file, session_id, pane_id, last_known_window)?;
    let (closeout_recovery_phase, closeout_recovery_outcome, closeout_recovery_error) = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => (
            pending_missing_pane_repair_phase(file).map(str::to_string),
            None,
            None,
        ),
        MissingRegisteredPaneRepairMode::ExplicitRepair => {
            recover_missing_pane_closeout(file, session_id, pane_id)
        }
    };
    let recorded_session_loss = crate::startup_miss::record_session_loss(
        file,
        session_id,
        pane_id,
        if dead_pane.is_some() {
            "registered_pane_dead"
        } else {
            "registered_pane_missing"
        },
        dead_pane
            .as_ref()
            .and_then(|diag| diag.observed_window.as_deref())
            .or(last_known_window),
    )?;
    let repaired_stale_preflight = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => false,
        MissingRegisteredPaneRepairMode::ExplicitRepair if closeout_recovery_phase.is_none() => {
            matches!(
                crate::repair::repair_stale_preflight_started_cycle(file)?,
                crate::repair::RepairOutcome::StalePreflightLockRepaired
            )
        }
        MissingRegisteredPaneRepairMode::ExplicitRepair => false,
    };
    let block_auto_start_reason = match mode {
        MissingRegisteredPaneRepairMode::InspectOnly => closeout_recovery_phase
            .as_deref()
            .map(|phase| missing_pane_manual_repair_reason(file, phase)),
        MissingRegisteredPaneRepairMode::ExplicitRepair
            if closeout_recovery_phase.is_some() && closeout_recovery_outcome.is_none() =>
        {
            Some(missing_pane_closeout_block_reason(
                file,
                closeout_recovery_phase.as_deref().unwrap_or("unknown"),
                closeout_recovery_error.as_deref(),
            ))
        }
        MissingRegisteredPaneRepairMode::ExplicitRepair => None,
    };
    Ok(MissingRegisteredPaneRepair {
        dead_pane,
        recorded_session_loss,
        repaired_stale_preflight,
        closeout_recovery_phase,
        closeout_recovery_outcome,
        closeout_recovery_error,
        block_auto_start_reason,
    })
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::process::Command as ProcessCommand;
    use std::time::Duration;
    use tmux_router::IsolatedTmux;
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn sync_proof_cache_reuses_actor_lookup_within_one_sync_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd_guard = ScopedCurrentDir::set(root);

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: cached-actor\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-proof-cache-actor");
        let first_pane = iso.new_session("test", root).unwrap();
        let second_pane = iso.split_window(&first_pane, root, "-dh").unwrap();
        let first_window = iso.pane_window(&first_pane).unwrap();
        let second_window = iso.pane_window(&second_pane).unwrap();

        crate::session_actor::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "cached-actor",
            &first_pane,
            &first_window,
            "sync",
            "first_actor_projection",
        )
        .unwrap();

        let proof_cache = SyncProofCache::default();
        assert!(
            sync_actor_or_live_owner_matches_cached(
                &iso,
                &doc,
                "cached-actor",
                &first_pane,
                &proof_cache,
            ),
            "the first lookup should populate the per-sync proof cache"
        );

        crate::session_actor::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "cached-actor",
            &second_pane,
            &second_window,
            "sync",
            "second_actor_projection",
        )
        .unwrap();

        assert_eq!(
            authoritative_actor_pane_for_document(&iso, &doc, "cached-actor").as_deref(),
            Some(second_pane.as_str()),
            "the uncached actor lookup should see the later projection"
        );
        assert!(
            sync_actor_or_live_owner_matches_cached(
                &iso,
                &doc,
                "cached-actor",
                &first_pane,
                &proof_cache,
            ),
            "one sync cycle should reuse already-proven actor facts instead of re-querying the controller/session store"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn safe_passive_authoritative_actor_binding_prefers_local_projection() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd_guard = ScopedCurrentDir::set(root);

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: local-projection\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-safe-passive-local-actor");
        let pane = iso.new_session("test", root).unwrap();
        let window = iso.pane_window(&pane).unwrap();
        crate::session_actor::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "local-projection",
            &pane,
            &window,
            "sync",
            "local_actor_projection",
        )
        .unwrap();

        let proof_cache = SyncProofCache::default();
        let resolved = project_authoritative_actor_binding(
            &iso,
            &doc,
            "local-projection",
            Some(&doc.to_string_lossy()),
            AutoStartMode::SafePassive,
            &proof_cache,
        );

        assert_eq!(resolved.as_deref(), Some(pane.as_str()));
    }
    #[test]
    fn repair_missing_registered_pane_records_loss_and_closes_stale_preflight() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("lost-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: session-lost-pane\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("session-lost-pane.log"),
            "[1] session_start file=tasks/lost-pane.md pane=%422 session=session-lost-pane\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-lost-pane",
            "%422",
            Some("@17"),
            MissingRegisteredPaneRepairMode::ExplicitRepair,
        )
        .unwrap();
        assert!(repair.recorded_session_loss);
        assert!(repair.repaired_stale_preflight);
        assert!(repair.dead_pane.is_none());

        let state = crate::cycle_state::load(&doc)
            .unwrap()
            .expect("cycle state should exist");
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);

        let status = crate::startup_miss::session_log_status(&doc, "session-lost-pane")
            .unwrap()
            .expect("session log should be readable");
        assert!(status.latest_session_closed());
    }
    #[test]
    fn inspect_only_missing_registered_pane_blocks_manual_closeout_repair() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp
            .path()
            .join("tasks")
            .join("captured-pane-loss-inspect.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-captured-pane-loss-inspect\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(tmp.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let response = "<!-- patch:exchange -->\n### Re: topic — gpt-5\nRecovered body.\n<!-- /patch:exchange -->\n";
        crate::repair::save_pending(&doc, response).unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-captured-pane-loss-inspect",
            "%422",
            Some("@17"),
            MissingRegisteredPaneRepairMode::InspectOnly,
        )
        .unwrap();
        assert_eq!(
            repair.closeout_recovery_phase.as_deref(),
            Some("response_captured")
        );
        assert!(repair.closeout_recovery_outcome.is_none());
        assert!(repair.closeout_recovery_error.is_none());
        let block_reason = repair
            .block_auto_start_reason
            .as_deref()
            .expect("inspect-only mode should block until explicit repair runs");
        assert!(block_reason.contains("agent-doc repair"));
        assert!(block_reason.contains("session doctor"));
        assert!(!repair.repaired_stale_preflight);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::ResponseCaptured);
        assert!(snapshot::pending_path_for(&doc).unwrap().exists());
    }
    #[test]
    fn repair_missing_registered_pane_recovers_response_captured_closeout() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("captured-pane-loss.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-captured-pane-loss\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(tmp.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let response = "<!-- patch:exchange -->\n### Re: topic — gpt-5\nRecovered body.\n<!-- /patch:exchange -->\n";
        crate::repair::save_pending(&doc, response).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("session-captured-pane-loss.log"),
            "[1] session_start file=tasks/captured-pane-loss.md pane=%422 session=session-captured-pane-loss\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-captured-pane-loss",
            "%422",
            Some("@17"),
            MissingRegisteredPaneRepairMode::ExplicitRepair,
        )
        .unwrap();
        assert!(repair.recorded_session_loss);
        assert_eq!(
            repair.closeout_recovery_phase.as_deref(),
            Some("response_captured")
        );
        assert_eq!(
            repair.closeout_recovery_outcome,
            Some(crate::repair::RepairOutcome::ReplayedResponse)
        );
        assert!(repair.closeout_recovery_error.is_none());
        assert!(repair.block_auto_start_reason.is_none());
        assert!(!repair.repaired_stale_preflight);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::Committed
        );
        assert!(!snapshot::pending_path_for(&doc).unwrap().exists());
        assert!(
            std::fs::read_to_string(&doc)
                .unwrap()
                .contains("### Re: topic — gpt-5")
        );
    }
    #[test]
    fn repair_missing_registered_pane_recovers_write_applied_commit_boundary() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("write-applied-pane-loss.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-write-applied-pane-loss\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(tmp.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let response = "<!-- patch:exchange -->\n### Re: topic — gpt-5\nRecovered body.\n<!-- /patch:exchange -->\n";
        crate::repair::save_pending(&doc, response).unwrap();

        let updated = concat!(
            "---\n",
            "agent_doc_session: session-write-applied-pane-loss\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, updated).unwrap();
        snapshot::save(&doc, updated).unwrap();
        crate::cycle_state::mark_write_applied(
            &doc,
            "write_template",
            Some(updated),
            Some(updated),
        )
        .unwrap();
        crate::capture::mark_write_applied(&doc).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("session-write-applied-pane-loss.log"),
            "[1] session_start file=tasks/write-applied-pane-loss.md pane=%423 session=session-write-applied-pane-loss\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-write-applied-pane-loss",
            "%423",
            Some("@17"),
            MissingRegisteredPaneRepairMode::ExplicitRepair,
        )
        .unwrap();
        assert!(repair.recorded_session_loss);
        assert_eq!(
            repair.closeout_recovery_phase.as_deref(),
            Some("write_applied")
        );
        assert_eq!(
            repair.closeout_recovery_outcome,
            Some(crate::repair::RepairOutcome::AlreadyApplied)
        );
        assert!(repair.closeout_recovery_error.is_none());
        assert!(repair.block_auto_start_reason.is_none());
        assert!(!repair.repaired_stale_preflight);

        let state = crate::cycle_state::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::Committed
        );
        assert!(!snapshot::pending_path_for(&doc).unwrap().exists());
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn repair_missing_registered_pane_captures_retained_dead_pane_diagnostics() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("dead-pane.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = "---\nagent_doc_session: dead-pane-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("dead-pane-session.log"),
            "[1] session_start file=tasks/dead-pane.md pane=%501 session=dead-pane-session\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-dead-pane-diagnostics");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        iso.enable_remain_on_exit(&pane).unwrap();
        assert!(
            wait_for_shell(&iso, &pane, Duration::from_secs(3)),
            "pane shell should be ready before sending the diagnostic exit command"
        );
        iso.send_keys(&pane, "printf 'assistant tail\\n'; exit 9")
            .unwrap();
        assert!(
            wait_for(Duration::from_secs(10), || iso.pane_dead(&pane)),
            "pane should be retained as dead for diagnostics; alive={} dead={} current_command={:?} capture={:?}",
            iso.pane_alive(&pane),
            iso.pane_dead(&pane),
            pane_current_command(&iso, &pane),
            iso.capture_pane(&pane, Some(20)).ok()
        );
        let repair = repair_missing_registered_pane(
            &iso,
            &doc,
            "dead-pane-session",
            &pane,
            Some("@17"),
            MissingRegisteredPaneRepairMode::ExplicitRepair,
        )
        .unwrap();
        let dead = repair
            .dead_pane
            .as_ref()
            .expect("retained dead pane should be captured");
        let capture_path = dead
            .capture_path
            .as_ref()
            .expect("dead pane tail should be persisted for provenance");
        if let Some(status) = dead.dead_status.as_deref() {
            assert_eq!(status, "9");
        }
        assert_eq!(dead.cycle_phase.as_deref(), Some("preflight_started"));
        assert!(capture_path.exists(), "dead pane tail should exist");
        let capture = std::fs::read_to_string(capture_path).unwrap();
        assert!(
            capture.contains("assistant tail"),
            "persisted dead pane tail should contain the last visible assistant output: {capture}"
        );
        assert!(dead.last_visible_excerpt.is_some());
        assert!(repair.recorded_session_loss);
        assert!(repair.repaired_stale_preflight);
        assert!(!iso.pane_alive(&pane));
        assert!(
            iso.pane_dead(&pane),
            "normal sync should retain the dead pane"
        );
        assert!(
            !dead.pane_killed,
            "normal sync should record the no-kill policy for dead panes"
        );
    }
    #[test]
    fn repair_missing_registered_pane_blocks_auto_start_when_closeout_recovery_fails() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp
            .path()
            .join("tasks")
            .join("captured-pane-loss-invalid-backlog.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        let content = concat!(
            "---\n",
            "agent_doc_session: session-captured-pane-loss-invalid-backlog\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please reply\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep] existing item\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();
        init_git_repo(tmp.path(), &doc);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — gpt-5\n",
            "Recovered body.\n",
            "<!-- /patch:exchange -->\n",
            "<!-- patch:backlog -->\n",
            "not-a-list\n",
            "<!-- /patch:backlog -->\n"
        );
        crate::capture::capture_response(&doc, response).unwrap();
        let pending_path = snapshot::pending_path_for(&doc).unwrap();
        std::fs::create_dir_all(pending_path.parent().unwrap()).unwrap();
        std::fs::write(&pending_path, response).unwrap();

        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(
            log_dir.join("session-captured-pane-loss-invalid-backlog.log"),
            "[1] session_start file=tasks/captured-pane-loss-invalid-backlog.md pane=%424 session=session-captured-pane-loss-invalid-backlog\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let repair = repair_missing_registered_pane(
            &Tmux::default_server(),
            &doc,
            "session-captured-pane-loss-invalid-backlog",
            "%424",
            Some("@17"),
            MissingRegisteredPaneRepairMode::ExplicitRepair,
        )
        .unwrap();
        assert!(repair.recorded_session_loss);
        assert_eq!(
            repair.closeout_recovery_phase.as_deref(),
            Some("response_captured")
        );
        assert!(repair.closeout_recovery_outcome.is_none());
        assert!(
            repair
                .closeout_recovery_error
                .as_deref()
                .unwrap_or_default()
                .contains("pending/backlog patch changed non-list content")
        );
        let block_reason = repair
            .block_auto_start_reason
            .as_deref()
            .expect("failed closeout recovery should block replacement auto-start");
        assert!(block_reason.contains("agent-doc repair"));
        assert!(!block_reason.contains("auto-starting session"));
        assert!(!repair.repaired_stale_preflight);
    }
    #[test]
    fn authoritative_actor_cache_returns_prefilled_record_without_live_probe() {
        let proof_cache = SyncProofCache::default();
        let file = Path::new("/tmp/agent-doc-cache-hit.md");
        let session_id = "cache-session";
        let record = agent_doc_sqlite::state_store::ActorRecord {
            document_id: file.display().to_string(),
            session_id: session_id.to_string(),
            generation: 42,
            pane_id: "%cached".to_string(),
            window_id: "@cached".to_string(),
            harness: "codex".to_string(),
            state: agent_doc_sqlite::state_store::ActorState::Ready,
            last_transition: agent_doc_sqlite::state_store::ActorLastTransition {
                caller: "test".to_string(),
                reason: "prefilled_cache".to_string(),
                timestamp: 0,
                prior_generation: 41,
                new_generation: 42,
            },
        };
        proof_cache.actor_records.borrow_mut().insert(
            (sync_proof_file_key(file), session_id.to_string()),
            Some(record),
        );

        let cached = load_live_authoritative_actor_record_cached(
            &Tmux::default_server(),
            file,
            session_id,
            &proof_cache,
        )
        .expect("prefilled cache hit should not require a live tmux pane");

        assert_eq!(cached.pane_id, "%cached");
        assert_eq!(cached.generation, 42);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_router_sync_keeps_cross_root_columns_stable_when_focus_moves() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\n---\n\n# Root\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\n---\n\n# Child\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-cross-root-focus-stability");
        let root_pane = iso.new_session("test", root).unwrap();
        let window = iso.pane_window(&root_pane).unwrap();
        let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
        let child_pane = iso.split_window(&root_pane, &subroot, "-dh").unwrap();

        let mut root_registry = tmux_router::Registry::new();
        let root_key = sessions::canonical_registry_key_in(
            root,
            root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        root_registry.insert(
            root_key,
            tmux_router::RegistryEntry {
                pane: root_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &root_pane).unwrap(),
                cwd: root.to_string_lossy().to_string(),
                started: "2026-04-30T23:31:02Z".to_string(),
                session_id: "root-session".to_string(),
                file: "tasks/agent-doc-bugs2.md".to_string(),
                window: window.clone(),
                supervisor_instance_id: String::new(),
            },
        );
        sessions::save_in(root, &root_registry).unwrap();

        let mut child_registry = tmux_router::Registry::new();
        let child_key = sessions::canonical_registry_key_in(
            &subroot,
            child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
        );
        child_registry.insert(
            child_key,
            tmux_router::RegistryEntry {
                pane: child_pane.clone(),
                pid: pane_pid_from_tmux(&iso, &child_pane).unwrap(),
                cwd: subroot.to_string_lossy().to_string(),
                started: "2026-04-30T23:29:49Z".to_string(),
                session_id: "child-session".to_string(),
                file: "tasks/claudescore-3.md".to_string(),
                window: window.clone(),
                supervisor_instance_id: "instance-1".to_string(),
            },
        );
        sessions::save_in(&subroot, &child_registry).unwrap();

        let _cwd = ScopedCurrentDir::set(root);
        let root_col = root_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let child_col = child_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let cols = vec![root_col.clone(), child_col.clone()];
        let proof_cache = SyncProofCache::default();
        let synthetic_registry = build_tmux_router_sync_registry(&iso, &cols, &proof_cache)
            .unwrap()
            .expect("cross-root sync should synthesize a router registry");
        let resolve_file = |path: &Path| {
            let content = std::fs::read_to_string(path).ok()?;
            let (fm, _) = frontmatter::parse(&content).ok()?;
            Some(FileResolution::Registered {
                key: fm.session?,
                tmux_session: None,
            })
        };
        tmux_router::sync(
            &cols,
            Some(window.as_str()),
            Some(child_col.as_str()),
            &iso,
            synthetic_registry.path(),
            &resolve_file,
        )
        .unwrap();

        let ordered = iso.list_panes_ordered(&window).unwrap();
        assert_eq!(
            ordered,
            vec![root_pane, child_pane],
            "focusing the child document must not invert cross-root pane ownership"
        );
    }
    #[test]
    fn auto_start_candidate_files_dedupes_repeated_documents_preserving_order() {
        // A document requested in more than one column must yield a single
        // auto-start candidate so the pre-sync pass cannot start two editor
        // panes for it ("3 tmux panes with 2 editor panes" regression).
        let col_args = vec![
            "editor.md,notes.md".to_string(),
            "editor.md".to_string(), // same document requested again in a second column
            "other.md, notes.md".to_string(), // whitespace + repeat
        ];
        let files = auto_start_candidate_files(&col_args);
        assert_eq!(
            files,
            vec![
                PathBuf::from("editor.md"),
                PathBuf::from("notes.md"),
                PathBuf::from("other.md"),
            ],
            "duplicate document requests must collapse to one first-seen auto-start candidate"
        );
    }
    #[test]
    fn auto_start_candidate_files_keeps_distinct_documents() {
        let col_args = vec!["a.md".to_string(), "b.md".to_string(), "c.md".to_string()];
        let files = auto_start_candidate_files(&col_args);
        assert_eq!(
            files,
            vec![
                PathBuf::from("a.md"),
                PathBuf::from("b.md"),
                PathBuf::from("c.md"),
            ],
            "distinct documents must each remain an auto-start candidate"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn registered_pane_proves_live_owner_rejects_unowned_alive_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tasks")).unwrap();

        let doc = tmp.path().join("tasks/owned.md");
        std::fs::write(&doc, "---\nagent_doc_session: owned-session\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-live-owner-proof");
        let pane = iso.new_session("test", tmp.path()).unwrap();

        assert!(
            !registered_pane_proves_live_owner(
                &iso,
                &doc,
                "owned-session",
                &pane,
                &SyncProofCache::default(),
            ),
            "a merely alive pane should not count as a live owner without ownership proof"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn registered_pane_proves_live_owner_rejects_live_registry_rebind_successor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd = ScopedCurrentDir::set(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(tmp.path().join("tasks")).unwrap();

        let doc = tmp.path().join("tasks/owned.md");
        std::fs::write(&doc, "---\nagent_doc_session: owned-rebind-session\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-owned-rebind-proof");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/owned-rebind-session.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane=%70 session=owned-rebind-session\n[2] codex_start mode=fresh restart_count=0\n[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                pane, pane
            ),
        )
        .unwrap();

        assert!(
            !registered_pane_proves_live_owner(
                &iso,
                &doc,
                "owned-rebind-session",
                &pane,
                &SyncProofCache::default(),
            ),
            "a live registry-rebind successor should not count as normal-path ownership proof without an authoritative actor record or supervisor-backed registry binding"
        );
    }
    #[test]
    fn protected_registered_pane_state_detects_protected_codex_queue_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/protected.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: protected-session\nagent: codex\n---\n",
        )
        .unwrap();

        let capture = "\
Starting codex...
›
tab to queue message
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
        let protected = protected_registered_pane_state_from_capture(&doc, capture)
            .expect("protected queue-state prompt detected");
        assert_eq!(protected.reason, "queued draft in composer");
        assert!(
            protected
                .last_visible_excerpt
                .as_deref()
                .unwrap_or_default()
                .contains("Context 31% used")
        );
    }
    #[test]
    fn protected_registered_pane_state_ignores_idle_codex_placeholder() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/idle.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: idle-session\nagent: codex\n---\n",
        )
        .unwrap();
        let capture = "\
Starting codex...
› Explain this module in @filename
gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used
";
        assert_eq!(
            protected_registered_pane_state_from_capture(&doc, capture),
            None
        );
    }
    #[test]
    fn open_cycle_protected_file_state_detects_open_closeout_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/open-cycle.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: open-cycle-session\n",
            "agent_doc_format: template\n",
            "agent_doc_write: crdt\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        snapshot::save(&doc, content).unwrap();

        assert_eq!(open_cycle_protected_file_state(&doc), None);

        crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let protected =
            open_cycle_protected_file_state(&doc).expect("preflight_started should protect file");
        assert_eq!(protected.file, doc);
        assert_eq!(protected.phase, "preflight_started");
    }
}
