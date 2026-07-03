//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn active_pane_column_index(
    tmux: &Tmux,
    target_session: Option<&str>,
    window: Option<&str>,
    layout_len: usize,
) -> Option<usize> {
    if layout_len < 2 {
        return None;
    }
    let session = target_session?;
    let window = window?;
    let active = tmux.active_pane(session)?;
    let ordered = tmux.list_panes_ordered(window).ok()?;
    if ordered.len() < 2 {
        return None;
    }
    let active_index = ordered.iter().position(|pane| pane == &active)?;
    Some(active_index.min(layout_len.saturating_sub(1)))
}

pub(crate) fn visible_registered_layout(tmux: &Tmux, window: Option<&str>) -> Vec<String> {
    let Some(window) = window else {
        return Vec::new();
    };
    let Ok(ordered_panes) = tmux.list_panes_ordered(window) else {
        return Vec::new();
    };
    if ordered_panes.len() < 2 {
        return Vec::new();
    }
    let registry = agent_doc_session_registry_io::load().unwrap_or_default();
    ordered_panes
        .iter()
        .map(|pane| {
            registry
                .values()
                .find(|entry| entry.pane == *pane && !entry.file.trim().is_empty())
                .map(|entry| entry.file.trim().to_string())
                .unwrap_or_default()
        })
        .collect()
}

pub(crate) fn lookup_registry_entry_for_file_session(
    file: &Path,
    session_id: &str,
) -> Option<tmux_router::RegistryEntry> {
    let (_, _project_root, registry_key) = registry_location_for_file(file)?;
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let registry = rc.session_registry();
    let entry = registry.get(&registry_key)?.clone();
    (entry.session_id == session_id).then_some(entry)
}

#[derive(Debug, Clone)]
pub(crate) struct SyntheticRegistryCandidate {
    pub(crate) session_id: String,
    pub(crate) file_path: PathBuf,
    pub(crate) entry: tmux_router::RegistryEntry,
    pub(crate) live_owner_match: bool,
    pub(crate) pane_root_match: bool,
}

pub(crate) fn filter_duplicate_synthetic_registry_candidates(
    candidates: Vec<SyntheticRegistryCandidate>,
) -> Vec<SyntheticRegistryCandidate> {
    let facts = candidates
        .iter()
        .map(
            |candidate| agent_doc_sync::SyntheticRegistryCandidateFacts {
                pane_id: candidate.entry.pane.clone(),
                file_path: candidate.file_path.display().to_string(),
                live_owner_match: candidate.live_owner_match,
                pane_root_match: candidate.pane_root_match,
            },
        )
        .collect::<Vec<_>>();
    let filter = agent_doc_sync::filter_synthetic_registry_candidate_facts(&facts);
    for resolution in &filter.resolutions {
        match resolution {
            agent_doc_sync::SyntheticRegistryDuplicateResolution::KeepWinner {
                pane_id,
                winner_index,
                duplicate_count,
                basis,
            } => {
                let winner = &candidates[*winner_index];
                eprintln!(
                    "[sync] synthetic tmux-router registry keeps pane {} for {} and drops {} duplicate claimant(s)",
                    pane_id,
                    winner.file_path.display(),
                    duplicate_count
                );
                sync_log(&format!(
                    "router_registry_duplicate_kept pane={} winner={} duplicates={} basis={}",
                    pane_id,
                    winner.file_path.display(),
                    duplicate_count,
                    basis.as_str()
                ));
            }
            agent_doc_sync::SyntheticRegistryDuplicateResolution::DropAmbiguous {
                pane_id,
                files,
            } => {
                let duplicate_files = files.join(", ");
                eprintln!(
                    "[sync] synthetic tmux-router registry dropping ambiguous duplicate pane {} for {}",
                    pane_id, duplicate_files
                );
                sync_log(&format!(
                    "router_registry_duplicate_dropped pane={} files={}",
                    pane_id, duplicate_files
                ));
            }
        }
    }
    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(idx, candidate)| filter.keep[idx].then_some(candidate))
        .collect()
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
    fn recover_existing_associated_pane_reuses_latest_open_session_log_owner() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: associated-session-log\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-associated-session-log-owner");
        let owner_pane = iso.new_session("test", tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/associated-session-log.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=associated-session-log\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

        let recovery = recover_existing_associated_pane(
            &iso,
            &doc,
            "associated-session-log",
            None,
            &RefCell::new(std::collections::HashMap::new()),
        );

        assert!(matches!(
            recovery,
            ExistingAssociatedPaneRecovery::Recovered(ref pane) if pane == &owner_pane
        ));
        let entry = lookup_registry_entry_for_file_session(&doc, "associated-session-log")
            .expect("recovered pane should be registered in the document registry");
        assert_eq!(entry.pane, owner_pane);
        let candidates = find_associated_panes(&iso, &doc, "associated-session-log");
        assert_eq!(candidates.len(), 1);
        assert!(
            candidates[0]
                .sources
                .contains(&AssociatedPaneSource::SessionLog),
            "expected session-log ownership proof: {:?}",
            candidates[0].sources
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn recover_existing_associated_pane_reregisters_supervisor_owned_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: associated-supervisor\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-associated-supervisor");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let pane_pid = agent_doc_tmux_io::pane_pid(&iso, &pane).unwrap();
        let supervisor_instance_id = "instance-1".to_string();

        let _ipc = agent_doc_supervisor_io::ipc::SupervisorIpc::start(
            tmp.path(),
            "associated-supervisor",
            {
                let supervisor_instance_id = supervisor_instance_id.clone();
                move |method| match method {
                    IpcMethod::Pid => IpcResponse::ok(serde_json::json!({
                        "pid": pane_pid
                    })),
                    IpcMethod::State => IpcResponse::ok(serde_json::json!({
                        "supervisor_pid": pane_pid,
                        "supervisor_instance_id": supervisor_instance_id,
                    })),
                    _ => IpcResponse::ok_empty(),
                }
            },
        )
        .unwrap();

        let recovery = recover_existing_associated_pane(
            &iso,
            &doc,
            "associated-supervisor",
            None,
            &RefCell::new(std::collections::HashMap::new()),
        );

        assert!(matches!(
            recovery,
            ExistingAssociatedPaneRecovery::Recovered(_)
        ));
        assert_eq!(
            agent_doc_session_registry_io::lookup("associated-supervisor").unwrap(),
            Some(pane.clone())
        );
        let entry = lookup_registry_entry_for_file_session(&doc, "associated-supervisor")
            .expect("recovered pane should be registered in the document registry");
        assert_eq!(entry.pane, pane);
        assert_eq!(entry.pid, pane_pid);
        assert_eq!(entry.supervisor_instance_id, supervisor_instance_id);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn reregister_recovered_owner_preserves_existing_supervisor_identity_without_socket() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(tmp.path());

        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("tasks").join("owned.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "---\nagent_doc_session: preserved-supervisor\n---\n").unwrap();

        let iso = IsolatedTmux::new("sync-preserve-supervisor-entry");
        let pane = iso.new_session("test", tmp.path()).unwrap();
        let pane_pid = pane_pid_from_tmux(&iso, &pane).unwrap();
        let window = iso.pane_window(&pane).unwrap();

        sessions::register_full_with_cwd_in(
            tmp.path(),
            "preserved-supervisor",
            &pane,
            "tasks/owned.md",
            pane_pid,
            &window,
            &tmp.path().to_string_lossy(),
        )
        .unwrap();
        let mut registry = agent_doc_session_registry_io::load_in(tmp.path()).unwrap();
        let key = tmux_router::registry::canonical_registry_key_in(
            tmp.path(),
            doc.to_string_lossy().as_ref(),
        );
        let entry = registry.get_mut(&key).expect("seeded entry should exist");
        entry.supervisor_instance_id = "instance-preserved".to_string();
        agent_doc_session_registry_io::save_in(tmp.path(), &registry).unwrap();

        reregister_recovered_owner(&iso, &doc, "preserved-supervisor", &pane).unwrap();

        let entry = lookup_registry_entry_for_file_session(&doc, "preserved-supervisor")
            .expect("recovered owner should keep its registry entry");
        assert_eq!(entry.pane, pane);
        assert_eq!(entry.pid, pane_pid);
        assert_eq!(entry.supervisor_instance_id, "instance-preserved");
    }
    #[test]
    fn classify_sync_layout_columns_marks_only_agent_docs() {
        let tmp = tempfile::TempDir::new().unwrap();
        let agent_doc = tmp.path().join("agent.md");
        let plain_doc = tmp.path().join("plain.md");
        std::fs::write(&agent_doc, "---\nagent_doc_session: left\n---\n").unwrap();
        std::fs::write(&plain_doc, "# plain markdown\n").unwrap();

        let agent_doc = agent_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let plain_doc = plain_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let columns = agent_doc_tmux::classify_sync_layout_columns(
            &[plain_doc.clone(), agent_doc.clone()],
            first_agent_doc_in_col,
        );

        assert_eq!(columns[0].raw, plain_doc);
        assert_eq!(columns[0].agent_doc, None);
        assert_eq!(columns[1].raw, agent_doc);
        assert_eq!(columns[1].agent_doc, Some(columns[1].raw.clone()));
    }

    #[test]
    fn focus_only_switch_prefers_existing_focused_column_over_active_tmux_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let left = root.join("tasks/left.md");
        let right = root.join("tasks/right.md");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        std::fs::write(&left, "---\nagent_doc_session: left\n---\n").unwrap();
        std::fs::write(&right, "---\nagent_doc_session: right\n---\n").unwrap();

        let left = left.canonicalize().unwrap().to_string_lossy().to_string();
        let right = right.canonicalize().unwrap().to_string_lossy().to_string();
        let saved_layout = vec![left.clone(), right.clone()];
        let resolved_column = agent_doc_tmux::focused_column_index(&saved_layout, Some(&right))
            .or(Some(0))
            .expect("focused right column should resolve");
        assert_eq!(
            resolved_column, 1,
            "the focused document column should beat the stale active pane column"
        );
        let expanded = agent_doc_tmux::expand_focus_only_columns_for_editor_switch(
            std::slice::from_ref(&right),
            &saved_layout,
            Some(resolved_column),
            agent_doc_tmux::TmuxFocusOnlyExpansionMode::SafePassive,
        );

        assert_eq!(
            expanded.columns,
            vec![left, right],
            "when the focused document is already visible, focus-only sync should select that column instead of replacing the currently active tmux pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn explicit_non_agent_window_preserves_layout_when_session_lacks_agent_doc_window() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/sample-app");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"test\"\n",
        )
        .unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        let non_agent = root.join("tasks/test1.md");
        std::fs::write(
            &non_agent,
            "# plain markdown without agent-doc frontmatter\n",
        )
        .unwrap();
        let child_doc = subroot.join("tasks/sampleorders.md");
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: monster-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-explicit-non-agent-window");
        let root_pane = iso.new_session("test", root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "test:0", "notes"])
            .unwrap();
        let root_window = iso.pane_window(&root_pane).unwrap();

        let child_pane = iso
            .raw_cmd(&[
                "new-window",
                "-t",
                "test:",
                "-n",
                "workspace",
                "-P",
                "-F",
                "#{pane_id}",
                "-c",
                subroot.to_string_lossy().as_ref(),
            ])
            .unwrap()
            .trim()
            .to_string();
        let child_window = iso.pane_window(&child_pane).unwrap();
        assert_ne!(
            child_window, root_window,
            "repro needs a separate child window"
        );

        sessions::register_full_with_cwd_in(
            &subroot,
            "monster-session",
            &child_pane,
            &child_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            &child_window,
            &subroot.to_string_lossy(),
        )
        .unwrap();

        run_with_options_internal(
            &[
                non_agent.to_string_lossy().to_string(),
                child_doc.to_string_lossy().to_string(),
            ],
            Some(root_window.as_str()),
            None,
            AutoStartMode::Full,
            false,
            &iso,
        )
        .unwrap();

        assert_eq!(
            iso.list_panes_ordered(&root_window).unwrap(),
            vec![root_pane.clone()],
            "full sync should preserve the explicit non-agent window instead of reconciling child agent-doc panes onto it"
        );
        assert!(
            iso.pane_alive(&child_pane),
            "the child document pane should stay alive when sync cannot find a named agent-doc window"
        );
        let entry = lookup_registry_entry_for_file_session(&child_doc, "monster-session")
            .expect("child registry entry should remain present");
        assert_eq!(
            entry.pane, child_pane,
            "sync should not replace the child pane when the explicit target window is not an agent-doc window"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn rescue_missing_window_uses_visible_file_registry_not_cwd_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");

        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();
        let _cwd = ScopedCurrentDir::set(&subroot);
        std::fs::write(
            root.join(".agent-doc/config.toml"),
            "tmux_session = \"4\"\n",
        )
        .unwrap();
        std::fs::write(
            subroot.join(".agent-doc/config.toml"),
            "tmux_session = \"1\"\n",
        )
        .unwrap();

        let root_doc = root.join("tasks/agent-doc-bugs2.md");
        let child_doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
        std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-visible-registry-rescue");
        let root_pane = iso.new_session("4", root).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "4:0", "agent-doc"])
            .unwrap();
        let child_pane = iso.new_session("1", subroot.as_path()).unwrap();
        iso.raw_cmd(&["rename-window", "-t", "1:0", "agent-doc"])
            .unwrap();

        let second_root_pane = iso.split_window(&root_pane, root, "-dh").unwrap();
        iso.stash_pane(&root_pane, "4").unwrap();
        iso.stash_pane(&second_root_pane, "4").unwrap();
        iso.raw_cmd(&["select-window", "-t", "4:stash"]).unwrap();

        let root_stash_window = iso.pane_window(&root_pane).unwrap();
        let child_window = iso.pane_window(&child_pane).unwrap();

        sessions::register_full_with_cwd_in(
            root,
            "root-session",
            &root_pane,
            &root_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &root_pane).unwrap(),
            &root_stash_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        sessions::register_full_with_cwd_in(
            &subroot,
            "child-session",
            &child_pane,
            &child_doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            &child_window,
            &subroot.to_string_lossy(),
        )
        .unwrap();

        let root_entry = lookup_registry_entry_for_file_session(&root_doc, "root-session")
            .expect("root document registry should resolve across cwd boundaries");
        assert_eq!(root_entry.pane, root_pane);
        assert!(
            rescue_missing_agent_doc_window_from_candidates(
                &iso,
                "4",
                "agent-doc",
                std::slice::from_ref(&root_pane),
            ),
            "visible-file rescue should recover the missing root agent-doc window even when cwd points at a child project"
        );
        let recreated_window = iso
            .pane_window(&root_pane)
            .expect("rescued pane should remain queryable");
        let rescued_session = iso
            .pane_session(&root_pane)
            .expect("rescued root pane session should be queryable");
        assert_eq!(
            rescued_session, "4",
            "rescued root pane should stay in session 4"
        );
        assert_eq!(
            window_name_for_window_id(&iso, &recreated_window).as_deref(),
            Some("agent-doc"),
            "rescued pane should now live in an agent-doc window"
        );
        let recreated_panes = iso
            .list_window_panes(&recreated_window)
            .expect("recreated window should be queryable");
        assert!(
            recreated_panes.contains(&root_pane) || recreated_panes.contains(&second_root_pane),
            "recreated agent-doc window should contain one of the root session panes, got {:?}",
            recreated_panes
        );
    }
    #[test]
    fn lookup_registry_entry_for_file_session_uses_document_project_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let subroot = root.join("src/session-share");
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(subroot.join("tasks")).unwrap();

        let doc = subroot.join("tasks/claudescore-3.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: child-session\n---\n\n# Child\n",
        )
        .unwrap();

        let mut registry = tmux_router::Registry::new();
        let canonical = doc.canonicalize().unwrap();
        let key = tmux_router::registry::canonical_registry_key_in(
            &subroot,
            canonical.to_string_lossy().as_ref(),
        );
        registry.insert(
            key,
            tmux_router::RegistryEntry {
                pane: "%44".to_string(),
                pid: 2374580,
                cwd: subroot.to_string_lossy().to_string(),
                started: "2026-04-30T21:04:50Z".to_string(),
                session_id: "child-session".to_string(),
                file: "tasks/claudescore-3.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: "instance-1".to_string(),
            },
        );
        agent_doc_session_registry_io::save_in(&subroot, &registry).unwrap();

        let _cwd = ScopedCurrentDir::set(root);
        let entry = lookup_registry_entry_for_file_session(
            Path::new("src/session-share/tasks/claudescore-3.md"),
            "child-session",
        )
        .expect("cross-root registry entry should resolve through child project root");
        assert_eq!(entry.pane, "%44");
        assert_eq!(entry.file, "tasks/claudescore-3.md");
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn register_synced_files_keeps_authoritative_actor_projection() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        let _cwd = ScopedCurrentDir::set(root);

        let doc = root.join("tasks/actor-owned.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: actor-owned\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("sync-register-authoritative-actor");
        let actor_pane = iso.new_session("test", root).unwrap();
        let other_pane = iso.split_window(&actor_pane, root, "-dh").unwrap();
        let actor_window = iso.pane_window(&actor_pane).unwrap();

        sessions::register_full_with_cwd(
            "actor-owned",
            &actor_pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, &actor_pane).unwrap(),
            &actor_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        agent_doc_session_actor_io::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "actor-owned",
            &actor_pane,
            &actor_window,
            "sync",
            "test_actor_projection",
        )
        .unwrap();

        register_synced_files(
            &iso,
            &[("actor-owned".to_string(), doc.clone())],
            &[(doc.clone(), other_pane.clone())],
        );

        let entry = lookup_registry_entry_for_file_session(&doc, "actor-owned")
            .expect("registry entry should remain present");
        assert_eq!(
            entry.pane, actor_pane,
            "sync must keep sessions.json projected onto the authoritative actor pane"
        );
    }
    #[test]
    fn filter_duplicate_synthetic_registry_candidates_drops_ambiguous_same_root_duplicate_pane() {
        let filtered = filter_duplicate_synthetic_registry_candidates(vec![
            synthetic_registry_candidate(
                "claudescore",
                "tasks/claudescore.md",
                "%250",
                false,
                true,
            ),
            synthetic_registry_candidate(
                "claudescore-3",
                "tasks/claudescore-3.md",
                "%250",
                false,
                true,
            ),
        ]);

        assert!(
            filtered.is_empty(),
            "ambiguous same-root duplicate pane claims should be dropped before tmux-router sync"
        );
    }
    #[test]
    fn filter_duplicate_synthetic_registry_candidates_keeps_unique_live_owner() {
        let filtered = filter_duplicate_synthetic_registry_candidates(vec![
            synthetic_registry_candidate(
                "claudescore",
                "tasks/claudescore.md",
                "%250",
                false,
                true,
            ),
            synthetic_registry_candidate(
                "claudescore-3",
                "tasks/claudescore-3.md",
                "%250",
                true,
                true,
            ),
        ]);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session_id, "claudescore-3");
        assert_eq!(filtered[0].entry.pane, "%250");
    }
}
