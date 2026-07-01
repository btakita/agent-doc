use super::*;

pub(crate) fn cycle_phase_label(file: &Path) -> Option<String> {
    let state = crate::cycle_state::load(file).ok().flatten()?;
    let label = match state.phase {
        agent_doc_turn::CyclePhase::PreflightStarted => "preflight_started",
        agent_doc_turn::CyclePhase::ResponseCaptured => "response_captured",
        agent_doc_turn::CyclePhase::WriteApplied => "write_applied",
        agent_doc_turn::CyclePhase::Committed => "committed",
        agent_doc_turn::CyclePhase::Abandoned => "abandoned",
    };
    Some(label.to_string())
}

pub(crate) fn repair_outcome_label(outcome: crate::repair::RepairOutcome) -> &'static str {
    match outcome {
        crate::repair::RepairOutcome::Noop => "noop",
        crate::repair::RepairOutcome::ReplayedResponse => "replayed_response",
        crate::repair::RepairOutcome::AlreadyApplied => "already_applied",
        crate::repair::RepairOutcome::ManualTailRemovalRespected => "manual_tail_removal_respected",
        crate::repair::RepairOutcome::StaleCaptureRetired => "stale_capture_retired",
        crate::repair::RepairOutcome::StalePreflightLockRepaired => "stale_preflight_lock_repaired",
        crate::repair::RepairOutcome::StalePreflightCycleAbandoned => {
            "stale_preflight_cycle_abandoned"
        }
        crate::repair::RepairOutcome::CommitBoundaryRecovered => "commit_boundary_recovered",
        crate::repair::RepairOutcome::TemplateNormalized => "template_normalized",
        crate::repair::RepairOutcome::CompletedBacklogReaped => "completed_backlog_reaped",
    }
}

pub(crate) fn canonicalize_sync_file(file: &Path) -> Option<PathBuf> {
    let candidate = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(file)
    };
    Some(candidate.canonicalize().unwrap_or(candidate))
}

pub(crate) fn registry_location_for_file(file: &Path) -> Option<(PathBuf, PathBuf, String)> {
    let canonical = canonicalize_sync_file(file)?;
    let project_root = agent_doc_fs::find_project_root(&canonical)?;
    let registry_key = tmux_router::registry::canonical_registry_key_in(
        &project_root,
        canonical.to_string_lossy().as_ref(),
    );
    Some((canonical, project_root, registry_key))
}

pub(crate) fn first_agent_doc_in_col(col: &str) -> Option<String> {
    col.split(',').find_map(|f| {
        let f = f.trim();
        if f.is_empty() {
            return None;
        }
        if let Ok(content) = std::fs::read_to_string(f)
            && let Ok((fm, _)) = frontmatter::parse(&content)
            && fm.session.is_some()
        {
            return Some(f.to_string());
        }
        None
    })
}

pub(crate) fn build_tmux_router_sync_registry(
    tmux: &Tmux,
    col_args: &[String],
    proof_cache: &SyncProofCache,
) -> Result<Option<NamedTempFile>> {
    let mut candidates = Vec::new();

    for file_path in col_args
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(file_path);
        if !path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let (fm, _) = match frontmatter::parse(&content) {
            Ok(parsed) => parsed,
            Err(_) => continue,
        };
        let Some(session_id) = fm.session else {
            continue;
        };
        let Some(entry) = lookup_registry_entry_for_file_session(path, &session_id) else {
            continue;
        };
        let Some((_, project_root, _)) = registry_location_for_file(path) else {
            continue;
        };
        let live_owner_match = sync_actor_or_live_owner_matches_cached(
            tmux,
            path,
            &session_id,
            &entry.pane,
            proof_cache,
        );
        let pane_root_match =
            pane_assignment_matches_document_root(tmux, &entry.pane, &project_root);
        candidates.push(SyntheticRegistryCandidate {
            session_id,
            file_path: path.to_path_buf(),
            entry,
            live_owner_match,
            pane_root_match,
        });
    }

    let mut registry = tmux_router::Registry::new();
    for candidate in filter_duplicate_synthetic_registry_candidates(candidates) {
        registry.insert(candidate.session_id, candidate.entry);
    }

    if registry.is_empty() {
        return Ok(None);
    }

    // Snapshot the synthetic registry under an absolute path so later cwd
    // drift in other parallel tests cannot make tmux-router read the wrong
    // registry file for this sync cycle.
    let temp_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".agent-doc/router-sync");
    std::fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "failed to create synthetic tmux-router registry dir {}",
            temp_dir.display()
        )
    })?;
    let temp_file = NamedTempFile::new_in(&temp_dir).with_context(|| {
        format!(
            "failed to create synthetic tmux-router registry in {}",
            temp_dir.display()
        )
    })?;
    tmux_router::registry::save_registry(temp_file.path(), &registry).with_context(|| {
        format!(
            "failed to save synthetic tmux-router registry {}",
            temp_file.path().display()
        )
    })?;
    Ok(Some(temp_file))
}

pub(crate) fn claimed_sync_pane_owner(
    claimed_panes: &RefCell<std::collections::HashMap<String, PathBuf>>,
    pane_id: &str,
    file_path: &Path,
) -> Option<PathBuf> {
    let claimed = claimed_panes.borrow();
    let owner = claimed.get(pane_id)?;
    (owner != file_path).then_some(owner.clone())
}

pub(crate) fn reserve_sync_pane(
    claimed_panes: &RefCell<std::collections::HashMap<String, PathBuf>>,
    pane_id: &str,
    file_path: &Path,
) {
    claimed_panes
        .borrow_mut()
        .insert(pane_id.to_string(), file_path.to_path_buf());
}
