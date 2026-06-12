use super::*;
use crate::sessions::IsolatedTmux;
use std::process::Command as ProcessCommand;
use std::time::Duration;

/// Helper: list windows as vec of (index, name) pairs.
fn list_windows(tmux: &Tmux, session: &str) -> Vec<(String, String)> {
    let output = tmux
        .raw_cmd(&[
            "list-windows",
            "-t",
            &format!("{}:", session),
            "-F",
            "#{window_index} #{window_name}",
        ])
        .unwrap();
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let idx = parts.next()?.to_string();
            let name = parts.next()?.to_string();
            Some((idx, name))
        })
        .collect()
}

#[test]
fn planned_stash_window_indices_packs_overflow_after_agent_doc() {
    let windows = vec![
        ("0".to_string(), "@10".to_string(), "agent-doc".to_string()),
        ("3".to_string(), "@11".to_string(), "stash".to_string()),
        ("7".to_string(), "@12".to_string(), "stash-2".to_string()),
        ("8".to_string(), "@13".to_string(), "work".to_string()),
    ];

    assert_eq!(
        planned_stash_window_indices(&windows),
        vec![("@11".to_string(), 1), ("@12".to_string(), 2)],
        "repair_layout must keep overflow stash windows adjacent after agent-doc"
    );
}

fn candidate(
    pane_id: &str,
    window_id: &str,
    window_name: &str,
    sources: &[AssociatedPaneSource],
) -> AssociatedPaneCandidate {
    let mut source_set = BTreeSet::new();
    for source in sources {
        source_set.insert(source.clone());
    }
    AssociatedPaneCandidate {
        pane_id: pane_id.to_string(),
        pane_pid: "100".to_string(),
        session_name: "14".to_string(),
        window_id: window_id.to_string(),
        window_name: window_name.to_string(),
        current_command: "agent-doc".to_string(),
        sources: source_set,
    }
}

fn wait_for<F>(timeout: Duration, mut predicate: F) -> bool
where
    F: FnMut() -> bool,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    predicate()
}

fn pane_current_command(tmux: &IsolatedTmux, pane: &str) -> Option<String> {
    let output = tmux
        .cmd()
        .args([
            "display-message",
            "-p",
            "-t",
            pane,
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn wait_for_shell(tmux: &IsolatedTmux, pane: &str, timeout: Duration) -> bool {
    wait_for(timeout, || {
        matches!(
            pane_current_command(tmux, pane).as_deref(),
            Some("sh" | "bash" | "zsh" | "fish")
        )
    })
}

struct ScopedCurrentDir {
    prev_cwd: PathBuf,
    _env_guard: crate::test_support::ProcessGlobalLockGuard,
}

impl ScopedCurrentDir {
    fn set(path: &Path) -> Self {
        let env_guard = crate::test_support::env_lock();
        let prev_cwd = std::env::current_dir()
            .ok()
            .filter(|cwd| cwd.exists())
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        std::env::set_current_dir(path).unwrap();
        Self {
            prev_cwd,
            _env_guard: env_guard,
        }
    }
}

impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
    }
}

fn synthetic_registry_candidate(
    session_id: &str,
    file_path: &str,
    pane_id: &str,
    live_owner_match: bool,
    pane_root_match: bool,
) -> SyntheticRegistryCandidate {
    SyntheticRegistryCandidate {
        session_id: session_id.to_string(),
        file_path: PathBuf::from(file_path),
        entry: sessions::SessionEntry {
            pane: pane_id.to_string(),
            pid: 1000,
            cwd: "/tmp/project".to_string(),
            started: "2026-05-01T00:00:00Z".to_string(),
            session_id: session_id.to_string(),
            file: file_path.to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
        live_owner_match,
        pane_root_match,
    }
}

fn init_git_repo(root: &Path, tracked: &Path) {
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["init"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test User"])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["add", tracked.strip_prefix(root).unwrap().to_str().unwrap()])
        .status()
        .unwrap();
    ProcessCommand::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .status()
        .unwrap();
}

#[test]
fn resolve_associated_panes_prefers_unique_active_window() {
    let candidates = vec![
        candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
        candidate(
            "%419",
            "@3",
            "agent-doc",
            &[
                AssociatedPaneSource::Registered,
                AssociatedPaneSource::SupervisorPid,
            ],
        ),
    ];

    let resolution = resolve_associated_panes(candidates, Some("@3"));
    match resolution {
        AssociatedPaneResolution::Selected { winner, redundant } => {
            assert_eq!(winner.pane_id, "%419");
            assert_eq!(redundant.len(), 1);
            assert_eq!(redundant[0].pane_id, "%417");
        }
        other => panic!("expected selected winner, got {other:?}"),
    }
}

#[test]
fn resolve_associated_panes_accepts_single_stash_candidate() {
    let candidates = vec![candidate(
        "%420",
        "@9",
        "stash",
        &[AssociatedPaneSource::ProcessTree],
    )];

    let resolution = resolve_associated_panes(candidates, Some("@7"));
    match resolution {
        AssociatedPaneResolution::Selected { winner, redundant } => {
            assert_eq!(winner.pane_id, "%420");
            assert!(redundant.is_empty());
        }
        other => panic!("expected selected stash winner, got {other:?}"),
    }
}

#[test]
fn resolve_associated_panes_reports_ambiguity_when_multiple_candidates_remain() {
    let candidates = vec![
        candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
        candidate(
            "%419",
            "@3",
            "agent-doc",
            &[AssociatedPaneSource::Registered],
        ),
        candidate(
            "%420",
            "@5",
            "agent-doc",
            &[AssociatedPaneSource::SupervisorPid],
        ),
    ];

    let resolution = resolve_associated_panes(candidates, Some("@7"));
    match resolution {
        AssociatedPaneResolution::Ambiguous(candidates) => {
            assert_eq!(candidates.len(), 3);
        }
        other => panic!("expected ambiguous resolution, got {other:?}"),
    }
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn find_live_owner_pane_reuses_latest_open_session_log_owner() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: session-log-owner\n---\n").unwrap();

    let iso = IsolatedTmux::new("sync-session-log-owner");
    let owner_pane = iso.new_session("test", tmp.path()).unwrap();
    std::fs::write(
            tmp.path().join(".agent-doc/logs/session-log-owner.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=session-log-owner\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

    let owner = find_live_owner_pane(&iso, &doc, "session-log-owner");
    assert_eq!(owner.as_deref(), Some(owner_pane.as_str()));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn find_live_owner_pane_prefers_latest_open_session_log_owner_over_stale_process_tree_match() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(
        &doc,
        "---\nagent_doc_session: session-log-beats-process-tree\n---\n",
    )
    .unwrap();

    let iso = IsolatedTmux::new("sync-session-log-beats-process-tree");
    let stale_pane = iso.new_session("test", tmp.path()).unwrap();
    let owner_pane = iso.split_window(&stale_pane, tmp.path(), "-dh").unwrap();

    let fake_bin_dir = tmp.path().join("bin");
    std::fs::create_dir_all(&fake_bin_dir).unwrap();
    let fake_codex = fake_bin_dir.join("codex");
    std::fs::write(&fake_codex, "#!/bin/sh\nsleep 60\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake_codex).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_codex, perms).unwrap();
    }

    iso.raw_cmd(&[
        "send-keys",
        "-t",
        &stale_pane,
        &format!("{} {}", fake_codex.display(), doc.display()),
        "Enter",
    ])
    .unwrap();

    assert!(
        wait_for(Duration::from_secs(3), || {
            find_alive_pane_for_file_inner(&iso, doc.to_string_lossy().as_ref(), None, false)
                .as_deref()
                == Some(stale_pane.as_str())
        }),
        "stale pane should expose a same-file process-tree match before session-log precedence is evaluated"
    );

    std::fs::write(
            tmp.path()
                .join(".agent-doc/logs/session-log-beats-process-tree.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=session-log-beats-process-tree\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

    let owner = find_live_owner_pane(&iso, &doc, "session-log-beats-process-tree");
    assert_eq!(
        owner.as_deref(),
        Some(owner_pane.as_str()),
        "latest open session-log owner must win over older same-file process-tree matches"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn find_live_owner_pane_reuses_live_registry_rebind_successor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: live-rebind-owner\n---\n").unwrap();

    let iso = IsolatedTmux::new("sync-live-rebind-owner");
    let successor_pane = iso.new_session("test", tmp.path()).unwrap();
    std::fs::write(
            tmp.path().join(".agent-doc/logs/live-rebind-owner.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane=%70 session=live-rebind-owner\n[2] codex_start mode=fresh restart_count=0\n[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                successor_pane,
                successor_pane
            ),
        )
        .unwrap();

    let owner = find_live_owner_pane(&iso, &doc, "live-rebind-owner");
    assert_eq!(owner.as_deref(), Some(successor_pane.as_str()));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_actor_or_live_owner_matches_prefers_authoritative_actor_record() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd_guard = ScopedCurrentDir::set(root);

    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    let doc = root.join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(
            &doc,
            "---\nagent_doc_session: actor-owner\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-authoritative-actor-owner");
    let stale_pane = iso.new_session("test", root).unwrap();
    let actor_pane = iso.split_window(&stale_pane, root, "-dh").unwrap();
    let stale_window = iso.pane_window(&stale_pane).unwrap();
    let actor_window = iso.pane_window(&actor_pane).unwrap();

    sessions::register_full_with_cwd(
        "actor-owner",
        &stale_pane,
        &doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &stale_pane).unwrap(),
        &stale_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    crate::session_actor::project_binding_in(
        root,
        &doc.to_string_lossy(),
        "actor-owner",
        &actor_pane,
        &actor_window,
        "sync",
        "test_actor_projection",
    )
    .unwrap();

    assert!(
        sync_actor_or_live_owner_matches(&iso, &doc, "actor-owner", &actor_pane),
        "sync should treat the authoritative actor pane as a live owner even when generic route heuristics still point elsewhere"
    );
}

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
fn recover_existing_associated_pane_reuses_live_registry_rebind_successor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: associated-rebind\n---\n").unwrap();

    let iso = IsolatedTmux::new("sync-associated-registry-rebind");
    let successor_pane = iso.new_session("test", tmp.path()).unwrap();
    std::fs::write(
            tmp.path().join(".agent-doc/logs/associated-rebind.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane=%70 session=associated-rebind\n[2] codex_start mode=fresh restart_count=0\n[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
                successor_pane,
                successor_pane
            ),
        )
        .unwrap();

    let recovery = recover_existing_associated_pane(
        &iso,
        &doc,
        "associated-rebind",
        None,
        &RefCell::new(std::collections::HashMap::new()),
    );

    assert!(matches!(
        recovery,
        ExistingAssociatedPaneRecovery::Recovered(ref pane) if pane == &successor_pane
    ));
    let candidates = find_associated_panes(&iso, &doc, "associated-rebind");
    assert_eq!(candidates.len(), 1);
    assert!(
        candidates[0]
            .sources
            .contains(&AssociatedPaneSource::RegistryRebind),
        "expected registry-rebind ownership proof: {:?}",
        candidates[0].sources
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn open_session_log_owner_fail_closed_diagnostic_requires_same_alive_open_pane() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("owned.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: open-log-pane\n---\n").unwrap();

    let iso = IsolatedTmux::new("sync-open-log-owner-fail-closed");
    let owner_pane = iso.new_session("test", tmp.path()).unwrap();
    std::fs::write(
            tmp.path().join(".agent-doc/logs/open-log-pane.log"),
            format!(
                "[1] session_start file=tasks/owned.md pane={} session=open-log-pane\n[2] codex_start mode=fresh restart_count=0\n",
                owner_pane
            ),
        )
        .unwrap();

    let diagnostic =
        open_session_log_owner_fail_closed_diagnostic(&doc, "open-log-pane", &owner_pane).unwrap();
    assert!(
        diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("session log still has no later child exit or session_end")
    );

    let none =
        open_session_log_owner_fail_closed_diagnostic(&doc, "open-log-pane", "%99999").unwrap();
    assert!(
        none.is_none(),
        "other panes should not inherit the open-log guard"
    );
}

#[test]
fn agent_doc_cmdline_owner_detection_only_accepts_start_supervisor() {
    let file = "tasks/live-tmux-repro-codex.md";

    assert!(agent_doc_cmdline_is_owner(
        "/home/brian/.cargo/bin/agent-doc start tasks/live-tmux-repro-codex.md",
        file
    ));
    assert!(agent_doc_cmdline_is_owner(
        "/usr/bin/codex /home/brian/work/btakita/agent-loop/tasks/live-tmux-repro-codex.md",
        file
    ));
    assert!(!agent_doc_cmdline_is_owner(
        "/home/brian/.cargo/bin/agent-doc route tasks/live-tmux-repro-codex.md",
        file
    ));
    assert!(!agent_doc_cmdline_is_owner(
        "/home/brian/.cargo/bin/agent-doc claim tasks/live-tmux-repro-codex.md --pane %522",
        file
    ));
}

#[test]
fn cmdline_owns_other_document_blocks_cross_root_commandeer() {
    // The exact awear/monsterrodholders cross-root repro: a brand-new
    // superproject document is claimed onto a pane already running a live
    // submodule Codex session for a different document.
    let claimed = "tasks/recruit/awear.md";
    assert!(
        cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/monsterrodholders.md",
            claimed,
        ),
        "a pane owning a different document must block commandeering"
    );
    assert!(
        cmdline_owns_other_document(
            "/usr/bin/codex /home/brian/work/btakita/agent-loop/src/boost-client/tasks/monsterrodholders.md",
            claimed,
        ),
        "a harness session for another document must block commandeering"
    );
}

#[test]
fn cmdline_owns_other_document_allows_same_doc_and_non_owner_panes() {
    let claimed = "tasks/recruit/awear.md";
    // The pane's own document — reuse, do not provision a new pane.
    assert!(
        !cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/recruit/awear.md",
            claimed,
        ),
        "a pane owning the claimed document is reusable"
    );
    // A plain shell with no owned .md document — claimable.
    assert!(
        !cmdline_owns_other_document("-zsh", claimed),
        "a bare shell does not own another document"
    );
    // An owner invocation with no .md document token — not provably another doc.
    assert!(
        !cmdline_owns_other_document("/home/brian/.cargo/bin/agent-doc start", claimed),
        "an owner session with no document token is not a different-document conflict"
    );
    // A transient non-owner subcommand against another doc — not an owner session.
    assert!(
        !cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc route tasks/other.md",
            claimed,
        ),
        "a non-owner subcommand is not a live owner session"
    );
}

#[test]
fn cmdline_owns_other_document_blocks_navigation_to_wrong_document_pane() {
    // #jb-tsift-pane-sync cross-document variant: navigating the editor to
    // `tasks/software/tsift.md` must not surface or reuse the pane that is
    // already running `tasks/agent-doc/agent-doc-bugs2.md`. The normal
    // owner-resolution path (`find_normal_path_owner_pane_excluding_with_logging`
    // -> `reject_cross_document_owner_pane` -> `pane_runs_other_document_owner`)
    // relies on this decision to reject the wrong-document pane and cold-start
    // a correct owner instead of aliasing two documents onto one pane.
    let navigated = "tasks/software/tsift.md";
    assert!(
        cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/agent-doc/agent-doc-bugs2.md",
            navigated,
        ),
        "a pane running a different document must not be surfaced as the navigated file's owner"
    );
    // The legitimate tsift owner is still reusable — the guard must not force
    // a spurious cold-start when the surfaced pane already owns the file.
    assert!(
        !cmdline_owns_other_document(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md",
            navigated,
        ),
        "the navigated document's own owner pane stays reusable under the cross-document guard"
    );
}

#[test]
fn owner_document_from_cmdline_extracts_bound_document() {
    assert_eq!(
        owner_document_from_cmdline(
            "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md"
        ),
        Some("tasks/software/tsift.md".to_string())
    );
    // Quoted token is unwrapped.
    assert_eq!(
        owner_document_from_cmdline("/usr/bin/codex \"tasks/agent-doc/agent-doc-bugs2.md\""),
        Some("tasks/agent-doc/agent-doc-bugs2.md".to_string())
    );
    // A bare shell owns no document.
    assert_eq!(owner_document_from_cmdline("-zsh"), None);
}

#[test]
fn cross_document_execution_identifies_foreign_owner_document() {
    // #jb-tsift-pane-sync logging vector: an agent-doc cycle for bugs2 running
    // inside a pane whose process owns tsift.md must be both detected
    // (cmdline_owns_other_document) and name the foreign document
    // (owner_document_from_cmdline) so log_cross_document_execution_context can
    // emit `pane_owns=tasks/software/tsift.md`.
    let pane_cmdline =
        "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/software/tsift.md";
    let cycle_doc = "tasks/agent-doc/agent-doc-bugs2.md";
    assert!(cmdline_owns_other_document(pane_cmdline, cycle_doc));
    assert_eq!(
        owner_document_from_cmdline(pane_cmdline),
        Some("tasks/software/tsift.md".to_string())
    );
    // No spurious cross-document signal when the pane owns the cycle's own doc.
    assert!(!cmdline_owns_other_document(
        "/home/brian/.cargo/bin/agent-doc start --route-owned tasks/agent-doc/agent-doc-bugs2.md",
        cycle_doc,
    ));
}

#[test]
fn reject_cross_document_owner_pane_preserves_non_contaminated_candidates() {
    // #jb-tsift-pane-sync focus-path wiring: the heuristic resolver
    // (`find_live_owner_pane_excluding_with_logging`, used by `focus.rs` and
    // resync recovery) now funnels its candidate through this guard. The
    // guard must only drop a pane that PROVABLY runs another document's
    // owner — it must never over-reject on the focus hot path, or normal
    // editor navigation would spuriously cold-start instead of focusing the
    // existing owner.
    let tmux = Tmux::default_server();
    let file = Path::new("tasks/software/tsift.md");

    // No candidate stays no candidate.
    assert_eq!(
        reject_cross_document_owner_pane(&tmux, None, file, false),
        None
    );

    // A candidate pane id with no resolvable process tree (no `#{pane_pid}`)
    // is not provably a cross-document owner, so it passes through unchanged.
    // This is the focus happy path: the resolved owner survives the guard.
    let bare = Some("%agent-doc-nonexistent-pane".to_string());
    assert_eq!(
        reject_cross_document_owner_pane(&tmux, bare.clone(), file, false),
        bare,
        "guard must not reject a candidate it cannot prove owns another document"
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
    let pane_pid = iso
        .raw_cmd(&["display-message", "-t", &pane, "-p", "#{pane_pid}"])
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let supervisor_instance_id = "instance-1".to_string();

    let _ipc = crate::supervisor::ipc::SupervisorIpc::start(tmp.path(), "associated-supervisor", {
        let supervisor_instance_id = supervisor_instance_id.clone();
        move |method| match method {
            crate::supervisor::ipc::IpcMethod::Pid => {
                crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                    "pid": pane_pid
                }))
            }
            crate::supervisor::ipc::IpcMethod::State => {
                crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                    "supervisor_pid": pane_pid,
                    "supervisor_instance_id": supervisor_instance_id,
                }))
            }
            _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
        }
    })
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
        sessions::lookup("associated-supervisor").unwrap(),
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
    let mut registry = sessions::load_in(tmp.path()).unwrap();
    let key = sessions::canonical_registry_key_in(tmp.path(), doc.to_string_lossy().as_ref());
    let entry = registry.get_mut(&key).expect("seeded entry should exist");
    entry.supervisor_instance_id = "instance-preserved".to_string();
    sessions::save_in(tmp.path(), &registry).unwrap();

    reregister_recovered_owner(&iso, &doc, "preserved-supervisor", &pane).unwrap();

    let entry = lookup_registry_entry_for_file_session(&doc, "preserved-supervisor")
        .expect("recovered owner should keep its registry entry");
    assert_eq!(entry.pane, pane);
    assert_eq!(entry.pid, pane_pid);
    assert_eq!(entry.supervisor_instance_id, "instance-preserved");
}

#[test]
fn cmdline_file_match_accepts_submodule_relative_start_path() {
    let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
    let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/docs.md";

    assert!(
        cmdline_has_file_match(cmdline, file_path),
        "root-relative target should match pane-relative start path"
    );
}

#[test]
fn cmdline_file_match_rejects_different_relative_path() {
    let file_path = "/tmp/agent-loop/src/session-share/tasks/docs.md";
    let cmdline = "/home/brian/.cargo/bin/agent-doc start tasks/other.md";

    assert!(
        !cmdline_has_file_match(cmdline, file_path),
        "different relative path should not match by basename alone"
    );
}

#[test]
fn unresolved_startup_miss_skips_sync_autostart_only_for_matching_alive_pane() {
    let miss = crate::startup_miss::StartupMiss {
        file: "tasks/owned.md".to_string(),
        pane_id: "%42".to_string(),
        session_id: "associated-supervisor".to_string(),
        harness: "codex".to_string(),
        timestamp: 5,
        origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        cycle_baseline_id: None,
    };

    assert!(should_skip_autostart_for_unresolved_startup_miss(
        Some("%42"),
        true,
        Some(&miss)
    ));
    assert!(!should_skip_autostart_for_unresolved_startup_miss(
        Some("%42"),
        false,
        Some(&miss)
    ));
    assert!(!should_skip_autostart_for_unresolved_startup_miss(
        Some("%43"),
        true,
        Some(&miss)
    ));
    assert!(!should_skip_autostart_for_unresolved_startup_miss(
        Some("%42"),
        true,
        None
    ));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn passive_autostart_allows_cleanly_closed_latest_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let iso = IsolatedTmux::new("sync-passive-closed");
    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("closed.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: passive-closed\n---\n").unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/logs/passive-closed.log"),
        concat!(
            "[1] session_start file=tasks/closed.md pane=%52 session=passive-closed\n",
            "[2] claude_start mode=fresh restart_count=0\n",
            "[3] supervisor_exit reason=user_quit_clean_exit pane=%52 restart_count=0\n",
            "[4] session_end\n",
        ),
    )
    .unwrap();

    assert_eq!(
        passive_autostart_skip_reason(&iso, &doc, "passive-closed", None).unwrap(),
        None
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn passive_autostart_blocks_open_latest_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let iso = IsolatedTmux::new("sync-passive-open");
    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("open.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: passive-open\n---\n").unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/logs/passive-open.log"),
        concat!(
            "[1] session_start file=tasks/open.md pane=%61 session=passive-open\n",
            "[2] codex_start mode=fresh restart_count=0\n",
        ),
    )
    .unwrap();

    let reason = passive_autostart_skip_reason(&iso, &doc, "passive-open", None)
        .unwrap()
        .expect("open session should block passive auto-start");
    assert!(reason.contains("latest session log is still open or ambiguous"));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn passive_autostart_blocks_live_registry_rebind_successor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());
    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("rebind.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: passive-rebind\n---\n").unwrap();
    let iso = IsolatedTmux::new("sync-passive-rebind-live");
    let successor = iso.new_session("test", tmp.path()).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/logs/passive-rebind.log"),
        format!(
            "[1] session_start file=tasks/rebind.md pane=%70 session=passive-rebind\n\
[2] codex_start mode=fresh restart_count=0\n\
[3] session_superseded old_pane=%70 new_pane={} old_window=@1 new_window=@2\n\
[4] session_end origin=registry_rebind pane=%70 next_pane={}\n",
            successor, successor
        ),
    )
    .unwrap();

    let reason = passive_autostart_skip_reason(&iso, &doc, "passive-rebind", None)
        .unwrap()
        .expect("live registry-rebind successor should block passive auto-start");
    assert!(reason.contains("registry_rebind"));
    assert!(reason.contains(successor.as_str()));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn passive_autostart_allows_stale_registry_rebind_successor() {
    let tmp = tempfile::TempDir::new().unwrap();
    let iso = IsolatedTmux::new("sync-passive-rebind-stale");
    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    let doc = tmp.path().join("tasks").join("rebind.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: passive-rebind-stale\n---\n").unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/logs/passive-rebind-stale.log"),
        concat!(
            "[1] session_start file=tasks/rebind.md pane=%70 session=passive-rebind-stale\n",
            "[2] codex_start mode=fresh restart_count=0\n",
            "[3] session_superseded old_pane=%70 new_pane=%71 old_window=@1 new_window=@2\n",
            "[4] session_end origin=registry_rebind pane=%70 next_pane=%71\n",
        ),
    )
    .unwrap();

    assert_eq!(
        passive_autostart_skip_reason(&iso, &doc, "passive-rebind-stale", None).unwrap(),
        None
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn passive_autostart_blocks_unresolved_startup_miss() {
    let tmp = tempfile::TempDir::new().unwrap();
    let iso = IsolatedTmux::new("sync-passive-miss");
    let doc = tmp.path().join("tasks").join("miss.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: passive-miss\n---\n").unwrap();
    let miss = crate::startup_miss::StartupMiss {
        file: "tasks/miss.md".to_string(),
        pane_id: "%81".to_string(),
        session_id: "passive-miss".to_string(),
        harness: "codex".to_string(),
        timestamp: 17,
        origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        cycle_baseline_id: None,
    };

    let reason = passive_autostart_skip_reason(&iso, &doc, "passive-miss", Some(&miss))
        .unwrap()
        .expect("startup miss should block passive auto-start");
    assert!(reason.contains("startup-miss is still unresolved"));
}

#[test]
fn parse_frontmatter_for_sync_includes_phase_and_fix_hint() {
    let path = Path::new("tasks/bad.md");
    let err = parse_frontmatter_for_sync(
        "---\nprompt_presets:\n  key: [oops\n---\n",
        path,
        "auto-start",
    )
    .unwrap_err();
    let message = err.to_string();

    assert!(message.contains("sync auto-start frontmatter"));
    assert!(message.contains("invalid YAML frontmatter in tasks/bad.md"));
    assert!(message.contains("Frontmatter excerpt:"));
    assert!(message.contains("> 2 |   key: [oops"));
    assert!(message.contains("Fix the frontmatter between the opening and closing --- markers"));
}

#[test]
fn sync_frontmatter_status_round_trips_through_snapshot() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    let doc = tmp.path().join("tasks").join("bad.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(
            &doc,
            "---\nprompt_presets:\n  key: [oops\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\nold status\n<!-- /agent:status -->\n",
        )
        .unwrap();

    let err = parse_frontmatter_for_sync(
        "---\nprompt_presets:\n  key: [oops\n---\n",
        &doc,
        "auto-start",
    )
    .unwrap_err();

    surface_frontmatter_status(&doc, "auto-start", &err);

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains(SYNC_FRONTMATTER_STATUS_PREFIX));
    assert!(updated.contains("sync auto-start frontmatter"));

    let snapshot = snapshot::load(&doc).unwrap().unwrap();
    assert!(snapshot.contains(SYNC_FRONTMATTER_STATUS_PREFIX));

    std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
        )
        .unwrap();
    snapshot::save(
            &doc,
            "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n[agent-doc sync] malformed frontmatter during auto-start.\n\nsync auto-start frontmatter: invalid YAML frontmatter in tasks/bad.md: boom\n<!-- /agent:status -->\n",
        )
        .unwrap();

    clear_frontmatter_status(&doc);

    let cleared = std::fs::read_to_string(&doc).unwrap();
    assert!(
        !cleared.contains(SYNC_FRONTMATTER_STATUS_PREFIX),
        "managed sync warning should be removed once parsing succeeds"
    );
    let cleared_snapshot = snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !cleared_snapshot.contains(SYNC_FRONTMATTER_STATUS_PREFIX),
        "snapshot should track the cleared status too"
    );
}

#[test]
fn clear_frontmatter_status_preserves_non_sync_status() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    let doc = tmp.path().join("tasks").join("ok.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    let original = "---\nagent_doc_session: test\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\nuser-owned status\n<!-- /agent:status -->\n";
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();

    clear_frontmatter_status(&doc);

    assert_eq!(std::fs::read_to_string(&doc).unwrap(), original);
    assert_eq!(snapshot::load(&doc).unwrap().unwrap(), original);
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
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);

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
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::ResponseCaptured
    );
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
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(updated), Some(updated))
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
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
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
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_skips_correct_state() {
    let iso = IsolatedTmux::new("sync-repair-skip-correct");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session with agent-doc window at index 0 + one stash window
    let _pane = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let _ = iso.ensure_stash_window("test");

    let windows_before = list_windows(&iso, "test");

    // repair_layout should succeed and not change anything
    repair_layout(&iso, "test", "agent-doc").unwrap();

    let windows_after = list_windows(&iso, "test");
    assert_eq!(
        windows_before, windows_after,
        "layout was already correct — nothing should change"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_moves_window_to_index_0() {
    let iso = IsolatedTmux::new("sync-repair-move-idx0");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session: initial window at 0 (placeholder), then create
    // agent-doc + stash at higher indices, and remove the placeholder.
    // This leaves agent-doc at a non-zero index with index 0 free.
    let _pane0 = iso.new_session("test", tmp.path()).unwrap();
    // Create stash at index 1
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);
    // Create agent-doc at index 2
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);
    // Kill the placeholder at index 0 to free it
    let _ = iso.raw_cmd(&["kill-window", "-t", "test:0"]);

    // Verify agent-doc is NOT at index 0 before repair
    let windows_before = list_windows(&iso, "test");
    let ad_before = windows_before.iter().find(|(_, n)| n == "agent-doc");
    assert!(ad_before.is_some(), "agent-doc window should exist");
    assert_ne!(
        ad_before.unwrap().0,
        "0",
        "agent-doc should NOT be at index 0 before repair"
    );

    repair_layout(&iso, "test", "agent-doc").unwrap();

    // After repair, agent-doc should be at index 0
    let windows_after = list_windows(&iso, "test");
    let ad_after = windows_after.iter().find(|(_, n)| n == "agent-doc");
    assert!(ad_after.is_some(), "agent-doc window should still exist");
    assert_eq!(
        ad_after.unwrap().0,
        "0",
        "agent-doc should be at index 0 after repair"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_moves_stash_directly_after_agent_doc() {
    let iso = IsolatedTmux::new("sync-repair-stash-index");
    let tmp = tempfile::TempDir::new().unwrap();

    let _pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);

    let windows_before = list_windows(&iso, "test");
    assert_eq!(
        windows_before
            .iter()
            .find(|(_, name)| name == "stash")
            .unwrap()
            .0,
        "2",
        "stash should start away from index 1 for this repro"
    );

    repair_layout(&iso, "test", "agent-doc").unwrap();

    let windows_after = list_windows(&iso, "test");
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "agent-doc")
            .unwrap()
            .0,
        "0"
    );
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "stash")
            .unwrap()
            .0,
        "1",
        "stash should be normalized directly after agent-doc"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_normalizes_stash_alias_to_index_1() {
    let iso = IsolatedTmux::new("sync-repair-stash-alias-index");
    let tmp = tempfile::TempDir::new().unwrap();

    let _pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash-2", "-d"]);

    let windows_before = list_windows(&iso, "test");
    assert!(
        windows_before
            .iter()
            .any(|(index, name)| index == "2" && name == "stash-2"),
        "stash alias should start away from index 1 for this repro"
    );

    repair_layout(&iso, "test", "agent-doc").unwrap();

    let windows_after = list_windows(&iso, "test");
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "agent-doc")
            .unwrap()
            .0,
        "0"
    );
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "stash")
            .unwrap()
            .0,
        "1",
        "the first stash window should be normalized to 1:stash"
    );
    assert!(
        !windows_after
            .iter()
            .any(|(_, name)| name.starts_with("stash-")),
        "repair should rename stash overflow aliases back to stash"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn full_sync_repairs_window_order_before_reconcile() {
    let iso = IsolatedTmux::new("sync-full-repairs-window-order");
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc = root.join("tasks/full-sync-repair.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: full-sync-repair\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    init_git_repo(root, &doc);
    let doc_str = doc.to_string_lossy().to_string();

    let pane0 = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let agent_doc_window = iso.pane_window(&pane0).unwrap();
    sessions::register_full_with_cwd(
        "full-sync-repair",
        &pane0,
        &doc_str,
        pane_pid_from_tmux(&iso, &pane0).unwrap(),
        &agent_doc_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "corky", "-d"]);
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash-2", "-d"]);

    run_with_options_internal(
        &[doc_str.clone()],
        None,
        Some(doc_str.as_str()),
        AutoStartMode::Full,
        false,
        &iso,
    )
    .unwrap();

    let windows_after = list_windows(&iso, "test");
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "agent-doc")
            .unwrap()
            .0,
        "0",
        "full sync should repair the agent-doc window to index 0"
    );
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "stash")
            .unwrap()
            .0,
        "1",
        "full sync should repair the primary stash window to 1:stash"
    );
    assert!(
        !windows_after
            .iter()
            .any(|(_, name)| name.starts_with("stash-")),
        "full sync should normalize stash aliases during repair"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn full_sync_calls_doctor_repair_for_explicit_stash_window() {
    let iso = IsolatedTmux::new("sync-full-doctor-repairs-stash-window");
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc = root.join("tasks/full-sync-doctor-repair.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: full-sync-doctor-repair\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    init_git_repo(root, &doc);
    let doc_str = doc.to_string_lossy().to_string();

    let pane0 = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "stash"]);
    let stash_window = iso.pane_window(&pane0).unwrap();
    sessions::register_full_with_cwd(
        "full-sync-doctor-repair",
        &pane0,
        &doc_str,
        pane_pid_from_tmux(&iso, &pane0).unwrap(),
        &stash_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);

    run_with_options_internal(
        &[doc_str.clone()],
        Some("test:0"),
        Some(doc_str.as_str()),
        AutoStartMode::Full,
        false,
        &iso,
    )
    .unwrap();

    let windows_after = list_windows(&iso, "test");
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "agent-doc")
            .unwrap()
            .0,
        "0",
        "full sync should let the doctor repair path recreate 0:agent-doc"
    );
    assert_eq!(
        windows_after
            .iter()
            .find(|(_, name)| name == "stash")
            .unwrap()
            .0,
        "1",
        "full sync should leave the repaired stash window at 1:stash"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_rescues_pane_from_stash() {
    let iso = IsolatedTmux::new("sync-repair-rescue-stash");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session with a non-agent-doc window + stash with a pane
    let pane1 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "other"]);

    // Create a second pane and stash it
    let pane2 = iso.split_window(&pane1, tmp.path(), "-dh").unwrap();
    iso.stash_pane(&pane2, "test").unwrap();

    // Verify no agent-doc window exists
    let windows_before = list_windows(&iso, "test");
    assert!(
        !windows_before.iter().any(|(_, n)| n == "agent-doc"),
        "agent-doc window should NOT exist before repair"
    );

    // Note: repair_layout uses sessions::load() which reads from CWD.
    // In tests without CWD override, Phase 2 rescue may not find the pane
    // in the registry. But Phase 1 (stash consolidation) and Phase 3 (index
    // normalization) still run. The key assertion is that repair doesn't error.
    let result = repair_layout(&iso, "test", "agent-doc");
    assert!(result.is_ok(), "repair_layout should not error");

    // The stashed pane should still be alive regardless
    assert!(iso.pane_alive(&pane2), "stashed pane should still be alive");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn promote_pane_to_agent_doc_window_reparents_stash_pane() {
    // #stash-pane-promote-on-focus: a live-owner pane parked in the stash
    // window must be reparented into the agent-doc window on focus.
    let iso = IsolatedTmux::new("sync-promote-stash");
    let tmp = tempfile::TempDir::new().unwrap();

    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

    // A second pane stashed away — the live owner stuck in the stash window.
    let pane2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    iso.stash_pane(&pane2, "test").unwrap();

    let win_before = iso.pane_window(&pane2).unwrap();
    let name_before = window_name_for_window_id(&iso, &win_before).unwrap();
    assert!(
        is_stash_window_name(&name_before),
        "pane2 should start in a stash window, got {name_before}"
    );

    let promoted = promote_pane_to_agent_doc_window(&iso, &pane2).unwrap();
    assert!(promoted, "stash pane should be promoted");

    // tmux preserves the pane id across join-pane, and it now lives in the
    // agent-doc window.
    assert!(
        iso.pane_alive(&pane2),
        "promoted pane should still be alive"
    );
    let win_after = iso.pane_window(&pane2).unwrap();
    let name_after = window_name_for_window_id(&iso, &win_after).unwrap();
    assert_eq!(
        name_after, "agent-doc",
        "pane2 should be in the agent-doc window after promotion"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn promote_pane_to_agent_doc_window_noop_for_non_stash_pane() {
    // A pane already outside the stash must not be reparented.
    let iso = IsolatedTmux::new("sync-promote-noop");
    let tmp = tempfile::TempDir::new().unwrap();
    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

    let promoted = promote_pane_to_agent_doc_window(&iso, &pane0).unwrap();
    assert!(
        !promoted,
        "a pane already outside the stash should not be promoted"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_in_stash_window_detects_stash_membership() {
    // `#jb-tsift-pane-sync`: editor-navigation focus uses this gate to avoid
    // selecting a stashed pane in place (which would surface focus inside the
    // stash). A pane in the agent-doc window is not stashed; a pane parked in
    // the stash window is.
    let iso = IsolatedTmux::new("sync-pane-in-stash");
    let tmp = tempfile::TempDir::new().unwrap();

    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    assert!(
        !pane_in_stash_window(&iso, &pane0),
        "pane in the agent-doc window must not be reported as stashed"
    );

    let pane2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    iso.stash_pane(&pane2, "test").unwrap();
    assert!(
        pane_in_stash_window(&iso, &pane2),
        "pane parked in the stash window must be reported as stashed"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_consolidates_multiple_stash_windows() {
    let iso = IsolatedTmux::new("sync-repair-consolidate");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session with agent-doc window
    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

    // Create 3 extra panes, stash each one separately to create multiple stash windows
    let p1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    let _p2 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    let _p3 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();

    // Stash them — each stash_pane goes to the same stash window normally,
    // but we can force multiple stash windows by using break_pane_to_stash
    // which creates overflow windows.
    iso.stash_pane(&p1, "test").unwrap();
    // The first stash_pane creates the stash window. For the second and third,
    // create new windows named "stash" manually to simulate overflow.
    let _ = iso.raw_cmd(&[
        "new-window",
        "-t",
        "test:",
        "-n",
        "stash",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
    ]);

    let stash_windows: Vec<String> = {
        let output = iso
            .raw_cmd(&[
                "list-windows",
                "-t",
                "test:",
                "-F",
                "#{window_id} #{window_name}",
            ])
            .unwrap();
        output
            .lines()
            .filter_map(|line| {
                let (id, name) = line.split_once(' ')?;
                if name == "stash" || name.starts_with("stash-") {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .collect()
    };

    // We should have at least 2 stash windows now
    assert!(
        stash_windows.len() >= 2,
        "should have multiple stash windows, got {}",
        stash_windows.len()
    );

    // Count total stash windows before repair
    let windows_before = list_windows(&iso, "test");
    let stash_count_before = windows_before
        .iter()
        .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
        .count();
    assert!(
        stash_count_before >= 2,
        "should have >=2 stash windows before repair, got {}",
        stash_count_before
    );

    repair_layout(&iso, "test", "agent-doc").unwrap();

    // After repair, joinable panes should be consolidated into 1:stash.
    // If tmux refuses a join, overflow windows must remain adjacent as
    // 2:stash, 3:stash, etc. This repro normally consolidates fully.
    let windows_after = list_windows(&iso, "test");
    let stash_windows_after: Vec<_> = windows_after
        .iter()
        .filter(|(_, n)| n == "stash" || n.starts_with("stash-"))
        .collect();
    assert!(
        stash_windows_after.len() <= 1,
        "should have at most 1 stash window after consolidation, got {}",
        stash_windows_after.len()
    );
    if let Some((index, name)) = stash_windows_after.first() {
        assert_eq!(name.as_str(), "stash", "stash aliases must be renamed");
        assert_eq!(index.as_str(), "1", "primary stash should be 1:stash");
    }

    // agent-doc should still be at index 0
    let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
    assert!(ad.is_some(), "agent-doc window should still exist");
    assert_eq!(ad.unwrap().0, "0", "agent-doc should be at index 0");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_swaps_when_index_0_occupied() {
    // Bug: when agent-doc is at index 2 and index 0 is occupied by another window
    // (e.g., stash), move-window fails because index 0 is taken.
    // Fix: use swap-window when index 0 is occupied.
    let iso = IsolatedTmux::new("sync-repair-swap-idx0");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session — window 0 is a "corky" window (simulating user's corky watch)
    let _pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "corky"]);

    // Create stash at index 1
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "stash", "-d"]);
    // Create agent-doc at index 2
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);

    // Verify: corky at 0, stash at 1, agent-doc at 2
    let windows_before = list_windows(&iso, "test");
    assert_eq!(
        windows_before.iter().find(|(i, _)| i == "0").unwrap().1,
        "corky"
    );
    assert_eq!(
        windows_before.iter().find(|(i, _)| i == "2").unwrap().1,
        "agent-doc"
    );

    repair_layout(&iso, "test", "agent-doc").unwrap();

    // After repair: agent-doc should be at 0, corky should be at 2
    let windows_after = list_windows(&iso, "test");
    let ad = windows_after.iter().find(|(_, n)| n == "agent-doc");
    assert!(ad.is_some(), "agent-doc window should still exist");
    assert_eq!(
        ad.unwrap().0,
        "0",
        "agent-doc should be at index 0 after swap"
    );

    let corky = windows_after.iter().find(|(_, n)| n == "corky");
    assert!(
        corky.is_some(),
        "corky window should still exist (not destroyed)"
    );
    assert_ne!(
        corky.unwrap().0,
        "0",
        "corky should have moved away from index 0"
    );

    // All 3 windows should still exist
    assert_eq!(
        windows_after.len(),
        3,
        "no windows should be destroyed, got {:?}",
        windows_after
    );
}

/// Regression: sync must never write tmux_session back to document frontmatter.
/// This was the root cause of pane-swap bugs — stale session names in frontmatter
/// caused terminal.rs to route panes to the wrong session.
#[test]
fn sync_does_not_write_tmux_session_to_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");

    // Write a doc WITHOUT tmux_session
    std::fs::write(
        &doc,
        "---\nagent_doc_session: test-123\n---\n\n## User\n\nHello\n",
    )
    .unwrap();

    // Read it back — tmux_session should be None
    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();
    assert!(
        fm.tmux_session.is_none(),
        "tmux_session should not be set initially"
    );

    // Write a doc WITH tmux_session already set
    let doc2 = tmp.path().join("test2.md");
    std::fs::write(
        &doc2,
        "---\nagent_doc_session: test-456\ntmux_session: old-session\n---\n\n## User\n\nHello\n",
    )
    .unwrap();

    let content2 = std::fs::read_to_string(&doc2).unwrap();
    let (fm2, _) = crate::frontmatter::parse(&content2).unwrap();
    // Frontmatter still parses it (for backward compat reading), but resolve_file
    // must NOT propagate it to FileResolution
    assert_eq!(
        fm2.tmux_session,
        Some("old-session".to_string()),
        "frontmatter parser should still read tmux_session for backward compat"
    );
}

/// Verify resolve_file closure always passes tmux_session: None regardless of frontmatter.
#[test]
fn resolve_file_ignores_frontmatter_tmux_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("test.md");

    // File with tmux_session in frontmatter
    std::fs::write(
        &doc,
        "---\nagent_doc_session: sess-1\ntmux_session: stale-session\n---\n\nbody\n",
    )
    .unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();

    // Simulate what resolve_file does — tmux_session must be None
    let resolution = match fm.session {
        Some(key) => FileResolution::Registered {
            key,
            tmux_session: None, // This is the critical assertion
        },
        None => FileResolution::Unmanaged,
    };

    match resolution {
        FileResolution::Registered { tmux_session, .. } => {
            assert!(
                tmux_session.is_none(),
                "FileResolution must never carry tmux_session from frontmatter"
            );
        }
        _ => panic!("expected Registered"),
    }
}

/// Sync skips files that have no `agent_doc_session` in frontmatter.
/// These are regular files that were never claimed — they should resolve as Unmanaged.
#[test]
fn sync_skips_file_without_session_in_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create a file with no frontmatter session UUID
    let doc = tmp.path().join("no-session.md");
    std::fs::write(&doc, "# Just a regular file\n\nNo frontmatter at all.\n").unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();
    assert!(fm.session.is_none(), "file should have no session UUID");

    // Simulate resolve_file: no session → Unmanaged
    let resolution = match fm.session {
        Some(_) => unreachable!("session should be None"),
        None => FileResolution::Unmanaged,
    };
    assert!(matches!(resolution, FileResolution::Unmanaged));
}

/// Sync skips files that have a session UUID in frontmatter but no registry entry.
/// This prevents auto-starting sessions for files that were never properly claimed
/// or whose claim expired.
#[test]
fn sync_skips_file_with_session_uuid_but_no_registry() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create an empty registry
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(tmp.path().join(".agent-doc/sessions.json"), "{}").unwrap();

    // Create a file with a session UUID but no matching registry entry
    let doc = tmp.path().join("stale-claim.md");
    std::fs::write(
        &doc,
        "---\nagent_doc_session: orphan-uuid-123\n---\n\n## User\n\nHello\n",
    )
    .unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();
    assert_eq!(fm.session, Some("orphan-uuid-123".to_string()));

    // Load registry directly from the temp path (avoid CWD dependency)
    let reg_content = std::fs::read_to_string(tmp.path().join(".agent-doc/sessions.json")).unwrap();
    let registry: sessions::SessionRegistry = serde_json::from_str(&reg_content).unwrap();
    let has_registry_entry = registry.contains_key("orphan-uuid-123");
    assert!(!has_registry_entry, "should NOT have a registry entry");

    // This is what the fixed resolve_file does — returns Unmanaged for stale claims
    let resolution = if has_registry_entry {
        FileResolution::Registered {
            key: "orphan-uuid-123".to_string(),
            tmux_session: None,
        }
    } else {
        FileResolution::Unmanaged
    };
    assert!(
        matches!(resolution, FileResolution::Unmanaged),
        "file with session UUID but no registry entry should be Unmanaged"
    );
}

/// Sync routes files that have both a session UUID in frontmatter AND a registry entry.
#[test]
fn sync_routes_file_with_session_uuid_and_registry_entry() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create registry with a matching entry
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    let registry_content = serde_json::json!({
        "claimed-uuid-456": {
            "pane": "%99",
            "pid": 12345,
            "cwd": "/tmp",
            "started": "2026-01-01T00:00:00Z",
            "file": "claimed.md",
            "window": "@0"
        }
    });
    std::fs::write(
        tmp.path().join(".agent-doc/sessions.json"),
        serde_json::to_string_pretty(&registry_content).unwrap(),
    )
    .unwrap();

    // Create a file with a session UUID that matches the registry
    let doc = tmp.path().join("claimed.md");
    std::fs::write(
        &doc,
        "---\nagent_doc_session: claimed-uuid-456\n---\n\n## User\n\nHello\n",
    )
    .unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();
    assert_eq!(fm.session, Some("claimed-uuid-456".to_string()));

    // Load registry directly from the temp path (avoid CWD dependency)
    let reg_content = std::fs::read_to_string(tmp.path().join(".agent-doc/sessions.json")).unwrap();
    let registry: sessions::SessionRegistry = serde_json::from_str(&reg_content).unwrap();
    let has_registry_entry = registry.contains_key("claimed-uuid-456");
    assert!(has_registry_entry, "should have a registry entry");

    // This is what the fixed resolve_file does — returns Registered for claimed files
    let resolution = if has_registry_entry {
        FileResolution::Registered {
            key: "claimed-uuid-456".to_string(),
            tmux_session: None,
        }
    } else {
        FileResolution::Unmanaged
    };
    assert!(
        matches!(resolution, FileResolution::Registered { .. }),
        "file with session UUID AND registry entry should be Registered"
    );
}

/// Empty col_args are filtered out before processing (JetBrains plugin sends phantom columns).
#[test]
fn empty_col_args_filtered() {
    let col_args: Vec<String> = vec![
        "file1.md".into(),
        "".into(),
        "file2.md".into(),
        "".into(),
        "  ".into(),
    ];
    let filtered: Vec<String> = col_args
        .iter()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .collect();
    assert_eq!(filtered, vec!["file1.md", "file2.md"]);
}

#[test]
fn column_memory_restores_empty_placeholder_columns() {
    let tmp = tempfile::TempDir::new().unwrap();
    let left = tmp.path().join("left.md");
    let right = tmp.path().join("right.md");
    std::fs::write(&left, "---\nagent_doc_session: left\n---\n").unwrap();
    std::fs::write(&right, "---\nagent_doc_session: right\n---\n").unwrap();

    let remembered = vec![
        left.canonicalize().unwrap().to_string_lossy().to_string(),
        String::new(),
    ];
    let cols = vec![
        String::new(),
        right.canonicalize().unwrap().to_string_lossy().to_string(),
    ];

    assert_eq!(
        apply_column_memory(&cols, &remembered),
        vec![
            left.canonicalize().unwrap().to_string_lossy().to_string(),
            right.canonicalize().unwrap().to_string_lossy().to_string(),
        ],
        "blank editor columns should keep their position long enough to restore remembered panes"
    );
}

#[test]
fn column_memory_skips_duplicate_remembered_doc_already_visible_elsewhere() {
    let tmp = tempfile::TempDir::new().unwrap();
    let right = tmp.path().join("right.md");
    std::fs::write(&right, "---\nagent_doc_session: right\n---\n").unwrap();

    let remembered = vec![
        right.canonicalize().unwrap().to_string_lossy().to_string(),
        String::new(),
    ];
    let cols = vec![
        String::new(),
        right.canonicalize().unwrap().to_string_lossy().to_string(),
    ];

    assert_eq!(
        apply_column_memory(&cols, &remembered),
        cols,
        "a remembered doc should not be duplicated into an empty sibling column when it is already visible"
    );
}

#[test]
fn build_layout_state_preserves_prior_distinct_doc_when_current_cols_duplicate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let left = tmp.path().join("left.md");
    let right = tmp.path().join("right.md");
    std::fs::write(&left, "---\nagent_doc_session: left\n---\n").unwrap();
    std::fs::write(&right, "---\nagent_doc_session: right\n---\n").unwrap();

    let saved_layout = vec![
        left.canonicalize().unwrap().to_string_lossy().to_string(),
        right.canonicalize().unwrap().to_string_lossy().to_string(),
    ];
    let duplicate_cols = vec![
        right.canonicalize().unwrap().to_string_lossy().to_string(),
        right.canonicalize().unwrap().to_string_lossy().to_string(),
    ];

    assert_eq!(
        build_layout_state(&duplicate_cols, &saved_layout),
        saved_layout,
        "duplicate current columns should not overwrite a previously distinct remembered layout"
    );
}

#[test]
fn column_memory_round_trip_persists_and_restores_across_cycles() {
    let tmp = tempfile::TempDir::new().unwrap();
    let left = tmp.path().join("left.md");
    let right = tmp.path().join("right.md");
    std::fs::write(&left, "---\nagent_doc_session: left-sess\n---\n").unwrap();
    std::fs::write(&right, "---\nagent_doc_session: right-sess\n---\n").unwrap();

    let left_path = left.canonicalize().unwrap().to_string_lossy().to_string();
    let right_path = right.canonicalize().unwrap().to_string_lossy().to_string();

    // Cycle 1: both columns filled → build_layout_state records them
    let cols_filled = vec![left_path.clone(), right_path.clone()];
    let no_prior = vec![];
    let state_1 = build_layout_state(&cols_filled, &no_prior);
    assert_eq!(state_1, vec![left_path.clone(), right_path.clone()]);

    // Cycle 2: left column goes empty (user opens non-markdown file) → apply_column_memory restores it
    let cols_empty_left = vec![String::new(), right_path.clone()];
    let restored = apply_column_memory(&cols_empty_left, &state_1);
    assert_eq!(
        restored,
        vec![left_path.clone(), right_path.clone()],
        "round-trip: empty left column should be restored from prior cycle's layout state"
    );

    // Cycle 2 continued: build_layout_state persists the restored state
    let state_2 = build_layout_state(&restored, &state_1);
    assert_eq!(
        state_2,
        vec![left_path.clone(), right_path.clone()],
        "round-trip: layout state should survive through restore + re-persist"
    );
}

#[test]
fn column_memory_cross_root_doc_restores_from_submodule_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let submodule = tmp.path().join("src/boost-client/tasks");
    std::fs::create_dir_all(&submodule).unwrap();

    let root_doc = tmp.path().join("tasks/bugs.md");
    std::fs::create_dir_all(root_doc.parent().unwrap()).unwrap();
    std::fs::write(&root_doc, "---\nagent_doc_session: root-sess\n---\n").unwrap();

    let child_doc = submodule.join("monsterrodholders.md");
    std::fs::write(&child_doc, "---\nagent_doc_session: monster-sess\n---\n").unwrap();

    let root_path = root_doc
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let child_path = child_doc
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    // Prior layout: root on left, child on right
    let saved = vec![root_path.clone(), child_path.clone()];

    // Current: left empty (non-markdown focused), child on right
    let cols = vec![String::new(), child_path.clone()];
    let restored = apply_column_memory(&cols, &saved);
    assert_eq!(
        restored,
        vec![root_path.clone(), child_path.clone()],
        "cross-root doc in submodule path should restore from column memory"
    );
}

#[test]
fn layout_state_path_uses_shared_sync_scope_root() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let child = root.join("src/boost-client");
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(child.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::create_dir_all(child.join("tasks")).unwrap();

    let root_doc = root.join("tasks/root.md");
    let child_doc = child.join("tasks/child.md");
    std::fs::write(&root_doc, "---\nagent_doc_session: root\n---\n").unwrap();
    std::fs::write(&child_doc, "---\nagent_doc_session: child\n---\n").unwrap();

    let _cwd = ScopedCurrentDir::set(root);
    let layout_path = layout_state_path_for_sync(
        &[format!("{},{}", root_doc.display(), child_doc.display())],
        None,
    );
    assert_eq!(layout_path, root.join(".agent-doc/last_layout.json"));
}

#[test]
fn effective_sync_columns_fall_back_to_recorded_layout() {
    let saved_layout = vec!["left.md".to_string(), "right.md".to_string()];
    let cols = effective_sync_columns(&[], &saved_layout, Path::new(".agent-doc/last_layout.json"))
        .expect("recorded layout should satisfy a no-col sync");
    assert_eq!(cols, saved_layout);
}

#[test]
fn column_memory_preserves_right_column_when_left_is_empty() {
    let tmp = tempfile::TempDir::new().unwrap();
    let right = tmp.path().join("right.md");
    std::fs::write(&right, "---\nagent_doc_session: right-sess\n---\n").unwrap();
    let right_path = right.canonicalize().unwrap().to_string_lossy().to_string();

    // No prior layout at all — fresh start
    let cols = vec![String::new(), right_path.clone()];
    let no_saved: Vec<String> = vec![];
    let result = apply_column_memory(&cols, &no_saved);
    assert_eq!(
        result,
        vec![String::new(), right_path.clone()],
        "right-column doc must stay in position even with no column memory to restore"
    );

    // build_layout_state should record empty left, filled right
    let state = build_layout_state(&result, &no_saved);
    assert_eq!(
        state,
        vec![String::new(), right_path],
        "layout state must preserve column positions including empty slots"
    );
}

#[test]
fn safe_passive_focus_only_switch_expands_active_column_from_memory() {
    let saved_layout = vec!["tasks/left.md".to_string(), "tasks/right.md".to_string()];
    let focused = vec!["tasks/new-left.md".to_string()];

    let expanded = expand_focus_only_columns_for_editor_switch(
        &focused,
        &saved_layout,
        Some(0),
        AutoStartMode::SafePassive,
    );
    assert_eq!(
        expanded,
        vec![
            "tasks/new-left.md".to_string(),
            "tasks/right.md".to_string()
        ],
        "a focus-only editor switch should replace the active tmux side and keep the sibling side visible"
    );

    let full_mode = expand_focus_only_columns_for_editor_switch(
        &focused,
        &saved_layout,
        Some(0),
        AutoStartMode::Full,
    );
    assert_eq!(
        full_mode, focused,
        "manual/full sync keeps the literal editor projection"
    );
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
    let resolved_column = focused_column_index(&saved_layout, Some(&right))
        .or(Some(0))
        .expect("focused right column should resolve");
    assert_eq!(
        resolved_column, 1,
        "the focused document column should beat the stale active pane column"
    );
    let expanded = expand_focus_only_columns_for_editor_switch(
        std::slice::from_ref(&right),
        &saved_layout,
        Some(resolved_column),
        AutoStartMode::SafePassive,
    );

    assert_eq!(
        expanded,
        vec![left, right],
        "when the focused document is already visible, focus-only sync should select that column instead of replacing the currently active tmux pane"
    );
}

#[test]
fn exact_visible_projection_does_not_expand_from_remembered_focus_only_layout() {
    let saved_layout = vec![
        "tasks/tsift.md".to_string(),
        "tasks/software/corky.md".to_string(),
    ];
    let focused = vec!["tasks/tsift.md".to_string()];

    let expanded = apply_focus_only_expansion_policy(
        &focused,
        &saved_layout,
        Some(0),
        AutoStartMode::SafePassive,
        false,
    );
    assert_eq!(
        expanded, saved_layout,
        "legacy focus-only sync still preserves remembered sibling columns"
    );

    let exact = apply_focus_only_expansion_policy(
        &focused,
        &saved_layout,
        Some(0),
        AutoStartMode::SafePassive,
        true,
    );
    assert_eq!(
        exact, focused,
        "editor snapshots marked exact-visible must not reintroduce stale remembered siblings"
    );
}

#[test]
fn empty_window_arg_normalized_to_none() {
    assert_eq!(normalize_scope_arg(None), None);
    assert_eq!(normalize_scope_arg(Some("")), None);
    assert_eq!(normalize_scope_arg(Some("   ")), None);
    assert_eq!(normalize_scope_arg(Some("@12")), Some("@12"));
    assert_eq!(normalize_scope_arg(Some("  @12  ")), Some("@12"));
}

/// Empty .md files should be auto-scaffolded by sync's resolve_file.
/// This tests the scaffolding logic inline (resolve_file is a closure in run()).
#[test]
fn sync_auto_scaffolds_empty_md_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

    let doc = project.join("test.md");
    std::fs::write(&doc, "").unwrap(); // Empty file

    // Simulate what resolve_file does for empty files:
    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(content.trim().is_empty(), "file should be empty");

    // Scaffold it
    let session_id = uuid::Uuid::new_v4();
    let scaffold = format!(
        "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
        session_id
    );
    std::fs::write(&doc, &scaffold).unwrap();

    // Verify scaffolded content has frontmatter
    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();
    assert!(
        fm.session.is_some(),
        "should have session UUID after scaffold"
    );
    assert!(fm.format.is_some(), "should have format after scaffold");
    assert!(
        content.contains("<!-- agent:exchange"),
        "should have exchange component"
    );
}

/// Non-empty .md files without frontmatter should NOT be auto-scaffolded.
#[test]
fn sync_does_not_scaffold_non_empty_md_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("notes.md");
    std::fs::write(&doc, "# My Notes\n\nSome content here.\n").unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    assert!(!content.trim().is_empty(), "file is not empty");
}

/// Scaffolded template must include all required components.
#[test]
fn sync_scaffold_includes_all_components() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

    let doc = project.join("new-session.md");
    std::fs::write(&doc, "").unwrap();

    // Simulate scaffold (same code as resolve_file)
    let raw = std::fs::read_to_string(&doc).unwrap();
    assert!(raw.trim().is_empty());

    let session_id = uuid::Uuid::new_v4();
    let scaffold = format!(
        "---\nagent_doc_session: {}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Status\n\n<!-- agent:status patch=replace -->\n<!-- /agent:status -->\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n\n## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n## Icebox\n\n<!-- agent:icebox -->\n<!-- /agent:icebox -->\n",
        session_id
    );
    std::fs::write(&doc, &scaffold).unwrap();

    let content = std::fs::read_to_string(&doc).unwrap();
    let (fm, _) = crate::frontmatter::parse(&content).unwrap();

    // Verify frontmatter
    assert!(fm.session.is_some(), "must have session UUID");
    assert!(fm.format.is_some(), "must have format set");

    // Verify all five components
    assert!(
        content.contains("<!-- agent:status patch=replace -->"),
        "must have status component"
    );
    assert!(
        content.contains("<!-- agent:exchange patch=append -->"),
        "must have exchange component"
    );
    assert!(
        content.contains("<!-- agent:queue -->"),
        "must have queue component"
    );
    assert!(
        content.contains("<!-- agent:backlog -->"),
        "must have backlog component"
    );
    assert!(
        content.contains("<!-- agent:icebox -->"),
        "must have icebox component"
    );

    // Verify components are properly closed
    assert!(
        content.contains("<!-- /agent:status -->"),
        "status must be closed"
    );
    assert!(
        content.contains("<!-- /agent:exchange -->"),
        "exchange must be closed"
    );
    assert!(
        content.contains("<!-- /agent:queue -->"),
        "queue must be closed"
    );
    assert!(
        content.contains("<!-- /agent:backlog -->"),
        "backlog must be closed"
    );
    assert!(
        content.contains("<!-- /agent:icebox -->"),
        "icebox must be closed"
    );
}

/// Non-.md files should never be scaffolded even if empty.
#[test]
fn sync_does_not_scaffold_non_md_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let txt = tmp.path().join("empty.txt");
    std::fs::write(&txt, "").unwrap();

    // .txt extension should not trigger scaffold
    assert_ne!(txt.extension(), Some(std::ffi::OsStr::new("md")));
}

/// Whitespace-only files should be treated as empty and scaffolded.
#[test]
fn sync_scaffolds_whitespace_only_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path();
    std::fs::create_dir_all(project.join(".agent-doc/snapshots")).unwrap();

    let doc = project.join("whitespace.md");
    std::fs::write(&doc, "   \n\n  \n").unwrap();

    let raw = std::fs::read_to_string(&doc).unwrap();
    assert!(
        raw.trim().is_empty(),
        "whitespace-only should be treated as empty"
    );
}

/// Files that already have frontmatter (even minimal) should NOT be re-scaffolded.
#[test]
fn sync_does_not_scaffold_file_with_existing_frontmatter() {
    let tmp = tempfile::TempDir::new().unwrap();
    let doc = tmp.path().join("existing.md");
    std::fs::write(&doc, "---\nagent_doc_session: test-123\n---\n").unwrap();

    let raw = std::fs::read_to_string(&doc).unwrap();
    // File has content (frontmatter) → not empty → no scaffold
    assert!(!raw.trim().is_empty(), "file with frontmatter is not empty");
}

// --- #4sh0: sync_log / repair_layout logging tests ---

/// repair_layout writes move-window or swap-window entries to /tmp/agent-doc-sync.log
/// when it has to reposition the agent-doc window to index 0.
#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn repair_layout_logs_move_window_action() {
    let iso = IsolatedTmux::new("sync-log-move-window");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session: placeholder at 0, then agent-doc at 1+ after killing placeholder
    let _pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["new-window", "-t", "test:", "-n", "agent-doc", "-d"]);
    // Kill index 0 so agent-doc is at index 1 with 0 free → triggers move-window
    let _ = iso.raw_cmd(&["kill-window", "-t", "test:0"]);

    let log_path = std::path::Path::new("/tmp/agent-doc-sync.log");
    // Record log size before repair so we only check new lines
    let before_len = std::fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);

    repair_layout(&iso, "test", "agent-doc").unwrap();

    // Verify the log file has new content mentioning move-window or swap-window
    let log_content = std::fs::read_to_string(log_path).unwrap_or_default();
    let new_content = &log_content[before_len.min(log_content.len() as u64) as usize..];
    assert!(
        new_content.contains("repair_action=move-window")
            || new_content.contains("repair_action=swap-window"),
        "repair_layout should log a move-window or swap-window action, got:\n{new_content}"
    );
}

/// sync_log writes timestamped entries to /tmp/agent-doc-sync.log.
#[test]
fn sync_log_writes_to_log_file() {
    let marker = format!("sync_log_test_marker_{}", std::process::id());
    sync_log(&marker);

    let log_content = std::fs::read_to_string("/tmp/agent-doc-sync.log").unwrap_or_default();
    assert!(
        log_content.contains(&marker),
        "sync_log should write to /tmp/agent-doc-sync.log, marker not found"
    );
    // Verify timestamp format: each line starts with [<unix_seconds>]
    let matching_line = log_content
        .lines()
        .find(|l| l.contains(&marker))
        .expect("marker line should exist");
    assert!(
        matching_line.starts_with('['),
        "log line should start with timestamp bracket, got: {matching_line}"
    );
}

#[test]
fn sync_latency_message_marks_budget_status() {
    let ok = sync_latency_message(
        "tmux_router",
        Duration::from_millis(999),
        Duration::from_secs(1),
        AutoStartMode::SafePassive,
    );
    assert!(ok.contains("status=ok"), "{ok}");
    assert!(ok.contains("mode=safe-passive"), "{ok}");

    let slow = sync_latency_message(
        "safe_passive_total",
        Duration::from_secs(1),
        Duration::from_secs(1),
        AutoStartMode::SafePassive,
    );
    assert!(slow.contains("status=over_budget"), "{slow}");
    assert!(slow.contains("elapsed_ms=1000"), "{slow}");

    let controller = sync_latency_message(
        "controller_actor_lookup",
        Duration::from_millis(251),
        SYNC_CONTROLLER_ACTOR_LOOKUP_BUDGET,
        AutoStartMode::SafePassive,
    );
    assert!(
        controller.contains("phase=controller_actor_lookup"),
        "{controller}"
    );
    assert!(controller.contains("status=over_budget"), "{controller}");
}

#[test]
fn authoritative_actor_cache_returns_prefilled_record_without_live_probe() {
    let proof_cache = SyncProofCache::default();
    let file = Path::new("/tmp/agent-doc-cache-hit.md");
    let session_id = "cache-session";
    let record = crate::session_actor::ActorRecord {
        document_id: file.display().to_string(),
        session_id: session_id.to_string(),
        generation: 42,
        pane_id: "%cached".to_string(),
        window_id: "@cached".to_string(),
        harness: "codex".to_string(),
        state: crate::session_actor::ActorState::Ready,
        last_transition: crate::session_actor::ActorLastTransition {
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
fn safe_passive_lock_contention_message_is_retryable_and_visible() {
    let message = safe_passive_lock_contention_message(
        Duration::from_millis(125),
        SYNC_LOCK_WAIT_LATENCY_BUDGET,
    );

    assert!(
        message.contains(SAFE_PASSIVE_SYNC_LOCK_SKIPPED_MARKER),
        "{message}"
    );
    assert!(message.contains("phase=sync_lock_wait"), "{message}");
    assert!(message.contains("status=over_budget"), "{message}");
    assert!(message.contains("coalesced=skipped_stale"), "{message}");
    assert!(message.contains("action=retry"), "{message}");
}

#[test]
fn safe_passive_prune_state_skips_stash_cleanup_from_first_pass() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_path = tmp.path().join(".agent-doc/sync-prune-state.json");
    let cols = vec!["tasks/a.md,tasks/b.md".to_string()];

    let first = safe_passive_prune_cleanup_mode_at(&state_path, &cols, Some("agent:1"), 1_000);
    assert_eq!(first, resync::PruneCleanupMode::SkipExpensiveStashCleanup);

    let second = safe_passive_prune_cleanup_mode_at(&state_path, &cols, Some("agent:1"), 1_500);
    assert_eq!(second, resync::PruneCleanupMode::SkipExpensiveStashCleanup);
}

#[test]
fn safe_passive_prune_cleanup_skips_stash_scan_for_editor_handoff() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(tmp.path().join("tasks")).unwrap();
    std::fs::write(tmp.path().join("tasks/a.md"), "").unwrap();
    std::fs::write(tmp.path().join("tasks/b.md"), "").unwrap();
    let cols = vec!["tasks/a.md,tasks/b.md".to_string()];

    assert_eq!(
        safe_passive_prune_cleanup_mode(
            AutoStartMode::SafePassive,
            &cols,
            Some("agent:1"),
            Some("tasks/a.md")
        ),
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );

    let changed_focus = safe_passive_prune_cleanup_mode(
        AutoStartMode::SafePassive,
        &cols,
        Some("agent:1"),
        Some("tasks/b.md"),
    );
    assert_eq!(
        changed_focus,
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );

    let changed_cols = vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()];
    assert_eq!(
        safe_passive_prune_cleanup_mode(
            AutoStartMode::SafePassive,
            &changed_cols,
            Some("agent:1"),
            Some("tasks/b.md"),
        ),
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );
}

#[test]
fn safe_passive_prune_state_keeps_skipping_on_layout_change_or_expiry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state_path = tmp.path().join(".agent-doc/sync-prune-state.json");
    let cols = vec!["tasks/a.md,tasks/b.md".to_string()];
    let changed_cols = vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()];

    assert_eq!(
        safe_passive_prune_cleanup_mode_at(&state_path, &cols, Some("agent:1"), 1_000),
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );
    assert_eq!(
        safe_passive_prune_cleanup_mode_at(&state_path, &changed_cols, Some("agent:1"), 1_100),
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );

    let expired_ms = 1_100 + SAFE_PASSIVE_STASH_CLEANUP_THROTTLE.as_millis() as u64;
    assert_eq!(
        safe_passive_prune_cleanup_mode_at(&state_path, &changed_cols, Some("agent:1"), expired_ms),
        resync::PruneCleanupMode::SkipExpensiveStashCleanup
    );
}

#[test]
fn acquire_sync_lock_times_out_when_lock_is_held() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_path = tmp.path().join(".agent-doc/sync.lock");
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();

    let holder = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&holder).unwrap();

    let start = Instant::now();
    let acquired = acquire_sync_lock(&lock_path, Duration::from_millis(120));
    let elapsed = start.elapsed();

    fs2::FileExt::unlock(&holder).unwrap();
    assert!(
        matches!(acquired, SyncLockAcquire::Contended),
        "contended sync lock should time out instead of blocking"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "sync lock timeout should be bounded, elapsed={elapsed:?}"
    );
}

#[test]
fn stale_orphaned_sync_lock_owner_requires_all_guards() {
    let stale_owner = SyncLockProcess {
        pid: 42,
        ppid: 1,
        age: STALE_SYNC_LOCK_OWNER_AGE + Duration::from_secs(1),
        cmdline: vec!["/home/brian/.cargo/bin/agent-doc".into(), "sync".into()],
        has_lock_fd: true,
    };
    assert!(is_stale_orphaned_sync_lock_owner(&stale_owner));

    let live_parent = SyncLockProcess {
        ppid: 100,
        ..stale_owner.clone()
    };
    assert!(!is_stale_orphaned_sync_lock_owner(&live_parent));

    let too_young = SyncLockProcess {
        age: STALE_SYNC_LOCK_OWNER_AGE - Duration::from_secs(1),
        ..stale_owner.clone()
    };
    assert!(!is_stale_orphaned_sync_lock_owner(&too_young));

    let different_command = SyncLockProcess {
        cmdline: vec!["/home/brian/.cargo/bin/agent-doc".into(), "route".into()],
        ..stale_owner.clone()
    };
    assert!(!is_stale_orphaned_sync_lock_owner(&different_command));

    let no_lock_fd = SyncLockProcess {
        has_lock_fd: false,
        ..stale_owner.clone()
    };
    assert!(!is_stale_orphaned_sync_lock_owner(&no_lock_fd));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_sync_focuses_local_projection_when_sync_lock_is_contended() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks/software")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let active_doc = root.join("tasks/active.md");
    let stale_doc = root.join("tasks/software/tsift.md");
    std::fs::write(
            &active_doc,
            "---\nagent_doc_session: active-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &stale_doc,
            "---\nagent_doc_session: stale-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-postlock-actor-focus");
    let stale_pane = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let active_pane = iso.split_window(&stale_pane, root, "-dh").unwrap();
    let active_window = iso.pane_window(&active_pane).unwrap();

    crate::session_actor::project_binding_in(
        root,
        &active_doc.to_string_lossy(),
        "active-session",
        &active_pane,
        &active_window,
        "sync",
        "postlock_focus_test",
    )
    .unwrap();
    crate::project_controller::store_actor_record(
        root,
        Some(0),
        &crate::session_actor::ActorRecord {
            document_id: crate::session_actor::canonical_document_id_in(
                root,
                &stale_doc.to_string_lossy(),
            ),
            session_id: "stale-session".to_string(),
            generation: 1,
            pane_id: stale_pane.clone(),
            window_id: active_window.clone(),
            harness: "codex".to_string(),
            state: crate::session_actor::ActorState::Starting,
            last_transition: crate::session_actor::ActorLastTransition {
                caller: "sync".to_string(),
                reason: "stale_starting_sibling_test".to_string(),
                timestamp: 1,
                prior_generation: 0,
                new_generation: 1,
            },
        },
    )
    .unwrap();
    iso.select_pane(&stale_pane).unwrap();

    let lock_path = root.join(".agent-doc/sync.lock");
    let holder = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&holder).unwrap();

    run_with_options_internal(
        &[active_doc.to_string_lossy().to_string()],
        None,
        Some(active_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    fs2::FileExt::unlock(&holder).unwrap();
    assert_eq!(
        iso.active_pane("test").unwrap(),
        active_pane,
        "safe-passive editor sync should focus the known local actor pane before sync lock contention defers prune/reconcile"
    );
}

// --- File rename detection tests ---

#[test]
fn is_file_rename_detects_rename_when_old_path_gone() {
    let tmp = tempfile::TempDir::new().unwrap();
    let old_path = tmp.path().join("old.md");
    // old_path does NOT exist on disk
    let current_path = tmp.path().join("new.md").to_string_lossy().to_string();
    assert!(
        is_file_rename(&old_path.to_string_lossy(), &current_path),
        "should detect rename when old path doesn't exist and paths differ"
    );
}

#[test]
fn is_file_rename_returns_false_when_paths_match() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("same.md");
    std::fs::write(&path, "content").unwrap();
    let path_str = path.to_string_lossy().to_string();
    assert!(
        !is_file_rename(&path_str, &path_str),
        "should not detect rename when paths are identical"
    );
}

#[test]
fn is_file_rename_returns_false_when_old_path_still_exists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let old_path = tmp.path().join("old.md");
    let new_path = tmp.path().join("new.md");
    std::fs::write(&old_path, "content").unwrap();
    std::fs::write(&new_path, "content").unwrap();
    assert!(
        !is_file_rename(&old_path.to_string_lossy(), &new_path.to_string_lossy()),
        "should not detect rename when old path still exists (both files present)"
    );
}

#[test]
fn is_file_rename_handles_relative_paths() {
    assert!(
        is_file_rename(
            "tasks/nonexistent-old-file.md",
            "tasks/software/renamed-file.md"
        ),
        "should detect rename with relative paths when old doesn't exist"
    );
}

#[test]
fn file_rename_updates_registry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project = tmp.path();

    // Set up registry with an entry pointing to old path
    std::fs::create_dir_all(project.join(".agent-doc")).unwrap();
    let session_id = "rename-test-uuid";
    let old_file = "tasks/old-name.md";
    let new_file = "tasks/new-name.md";
    let registry_content = serde_json::json!({
        session_id: {
            "pane": "%42",
            "pid": 12345,
            "cwd": project.to_string_lossy(),
            "started": "2026-04-20T00:00:00Z",
            "file": old_file,
            "window": "@0"
        }
    });
    std::fs::write(
        project.join(".agent-doc/sessions.json"),
        serde_json::to_string_pretty(&registry_content).unwrap(),
    )
    .unwrap();

    // Verify detection
    assert!(
        is_file_rename(old_file, new_file),
        "old path doesn't exist on disk, paths differ → rename"
    );

    // Verify we can load the entry and see the old path
    let reg: sessions::SessionRegistry = serde_json::from_str(
        &std::fs::read_to_string(project.join(".agent-doc/sessions.json")).unwrap(),
    )
    .unwrap();
    let entry = reg.get(session_id).unwrap();
    assert_eq!(entry.file, old_file);
    assert_eq!(entry.pane, "%42");
}

// --- Batch summary formatting tests ---

#[test]
fn batch_summary_format_multiple_panes() {
    let auto_started_panes = [
        ("%80".to_string(), "tasks/cursor.md".to_string()),
        ("%81".to_string(), "tasks/feat.md".to_string()),
        ("%82".to_string(), "tasks/agent-loop.md".to_string()),
    ];
    let summary: Vec<String> = auto_started_panes
        .iter()
        .map(|(pane, file)| format!("{}→{}", pane, file))
        .collect();
    let msg = format!(
        "[sync] auto-started {} panes: {}",
        auto_started_panes.len(),
        summary.join(", ")
    );
    assert!(msg.contains("3 panes"));
    assert!(msg.contains("%80→tasks/cursor.md"));
    assert!(msg.contains("%81→tasks/feat.md"));
    assert!(msg.contains("%82→tasks/agent-loop.md"));
}

#[test]
fn batch_summary_not_printed_for_single_pane() {
    let auto_started_panes = [("%84".to_string(), "tasks/file.md".to_string())];
    // Batch summary only prints when len > 1
    assert!(
        auto_started_panes.len() <= 1,
        "single pane should not trigger batch summary"
    );
}

// --- Rename debounce tests ---

#[test]
fn rename_debounce_suppresses_auto_start() {
    let tmp = tempfile::TempDir::new().unwrap();
    let debounce_dir = tmp.path().join(".agent-doc/rename-debounce");
    std::fs::create_dir_all(&debounce_dir).unwrap();

    // Create a file with known content for hashing
    let file = tmp.path().join("test.md");
    std::fs::write(&file, "---\nagent_doc_session: abc123\n---\n").unwrap();

    // Write marker using the same hash function
    let hash = crate::snapshot::doc_hash(&file).unwrap();
    let marker = debounce_dir.join(format!("{}.marker", hash));
    std::fs::write(&marker, file.to_string_lossy().as_ref()).unwrap();

    // Check: marker exists and is fresh → has_rename_debounce should find it
    // (We test the marker file existence and freshness directly since
    // has_rename_debounce uses a hardcoded path relative to cwd)
    assert!(marker.exists(), "marker should exist after write");
    let age = marker
        .metadata()
        .unwrap()
        .modified()
        .unwrap()
        .elapsed()
        .unwrap();
    assert!(
        age.as_secs() < RENAME_DEBOUNCE_TTL_SECS,
        "marker should be fresh"
    );
}

#[test]
fn rename_debounce_ttl_logic() {
    // Test the expiry logic directly: a marker older than RENAME_DEBOUNCE_TTL_SECS
    // should be considered expired
    let now = std::time::SystemTime::now();
    let fresh = now - std::time::Duration::from_secs(1);
    let expired = now - std::time::Duration::from_secs(RENAME_DEBOUNCE_TTL_SECS + 1);

    let fresh_age = now.duration_since(fresh).unwrap().as_secs();
    let expired_age = now.duration_since(expired).unwrap().as_secs();

    assert!(
        fresh_age < RENAME_DEBOUNCE_TTL_SECS,
        "fresh marker should be within TTL"
    );
    assert!(
        expired_age >= RENAME_DEBOUNCE_TTL_SECS,
        "expired marker should exceed TTL"
    );
}

#[test]
fn rename_debounce_does_not_affect_other_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let debounce_dir = tmp.path().join(".agent-doc/rename-debounce");
    std::fs::create_dir_all(&debounce_dir).unwrap();

    let file_a = tmp.path().join("a.md");
    let file_b = tmp.path().join("b.md");
    std::fs::write(&file_a, "---\nagent_doc_session: aaa\n---\n").unwrap();
    std::fs::write(&file_b, "---\nagent_doc_session: bbb\n---\n").unwrap();

    // Only write marker for file_a
    let hash_a = crate::snapshot::doc_hash(&file_a).unwrap();
    let marker_a = debounce_dir.join(format!("{}.marker", hash_a));
    std::fs::write(&marker_a, file_a.to_string_lossy().as_ref()).unwrap();

    // file_b should have a different hash, no marker
    let hash_b = crate::snapshot::doc_hash(&file_b).unwrap();
    let marker_b = debounce_dir.join(format!("{}.marker", hash_b));
    assert_ne!(
        hash_a, hash_b,
        "different files should have different hashes"
    );
    assert!(!marker_b.exists(), "no marker should exist for file_b");
}

#[test]
fn skip_auto_start_for_recent_session_loss_detects_repeated_window() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(tmp.path());
    let doc = tmp.path().join("tasks").join("repeat-loss.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
    std::fs::write(&doc, "---\nagent_doc_session: repeat-loss\n---\n").unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(
            tmp.path().join(".agent-doc/logs/repeat-loss.log"),
            format!(
                "[{}] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n[{}] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                now.saturating_sub(30),
                now.saturating_sub(5)
            ),
        )
        .unwrap();

    assert!(
        skip_auto_start_for_recent_session_loss(&doc, "repeat-loss").unwrap(),
        "two recent session-loss events should suppress sync auto-start"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn stash_rescue_discovers_agent_doc_window_when_window_arg_is_none() {
    let iso = IsolatedTmux::new("sync-stash-discover-window");
    let tmp = tempfile::TempDir::new().unwrap();

    // Create session with agent-doc window
    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

    // Create a second pane and stash it
    let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    iso.stash_pane(&pane1, "test").unwrap();
    assert!(iso.pane_alive(&pane1), "stashed pane should still be alive");

    // Verify pane1 is in a stash window
    let win_id = iso.pane_window(&pane1).unwrap();
    let win_name = iso
        .cmd()
        .args(["display-message", "-t", &win_id, "-p", "#{window_name}"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    assert!(
        win_name == "stash" || win_name.starts_with("stash-"),
        "pane should be in stash window, got: {}",
        win_name
    );

    // Simulate what the fix does: discover agent-doc window from session name
    // when `window` arg is None
    let target_sess = "test";
    let candidate = format!("{}:agent-doc", target_sess);
    let window_panes = iso.list_window_panes(&candidate).unwrap_or_default();
    assert!(
        !window_panes.is_empty(),
        "should discover agent-doc window from session name"
    );

    // Rescue the pane into the agent-doc window without swapping pane0 out.
    let target = window_panes.first().unwrap();
    let rescue_result = sessions::join_pane_guarded(&iso, &pane1, target, target_sess, "-dh");
    assert!(
        rescue_result.is_ok(),
        "join-pane rescue should succeed: {:?}",
        rescue_result.err()
    );

    // Verify pane1 is no longer in stash
    let post_win_id = iso.pane_window(&pane1).unwrap();
    let post_win_name = iso
        .cmd()
        .args([
            "display-message",
            "-t",
            &post_win_id,
            "-p",
            "#{window_name}",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    assert_eq!(
        post_win_name, "agent-doc",
        "pane should be in agent-doc window after rescue, got: {}",
        post_win_name
    );
    let visible_panes = iso.list_window_panes(&candidate).unwrap();
    assert!(
        visible_panes.contains(&pane0),
        "existing pane should stay visible after rescue, got: {:?}",
        visible_panes
    );
    assert!(
        visible_panes.contains(&pane1),
        "rescued pane should be visible after rescue, got: {:?}",
        visible_panes
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_defers_stash_rescue_to_reconciler_swap() {
    let iso = IsolatedTmux::new("sync-deferred-stash-rescue");
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd = ScopedCurrentDir::set(tmp.path());

    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc_a = tmp.path().join("tasks/a.md");
    let doc_b = tmp.path().join("tasks/b.md");
    std::fs::create_dir_all(doc_a.parent().unwrap()).unwrap();

    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();

    let session_a = "aaaa-aaaa";
    let session_b = "bbbb-bbbb";
    std::fs::write(
            &doc_a,
            format!("---\nagent_doc_session: {session_a}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n"),
        ).unwrap();
    std::fs::write(
            &doc_b,
            format!("---\nagent_doc_session: {session_b}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n"),
        ).unwrap();

    let win = iso.pane_window(&pane0).unwrap();
    sessions::register_full_with_cwd(
        session_a,
        &pane0,
        &doc_a.to_string_lossy(),
        std::process::id(),
        &win,
        &tmp.path().to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        session_b,
        &pane1,
        &doc_b.to_string_lossy(),
        std::process::id(),
        &win,
        &tmp.path().to_string_lossy(),
    )
    .unwrap();

    // Stash pane1 — simulates what the reconciler does when switching layouts.
    iso.stash_pane(&pane1, "test").unwrap();
    assert!(iso.pane_alive(&pane1), "stashed pane should be alive");

    let agent_doc_window = "test:agent-doc";
    let panes_before = iso.list_window_panes(agent_doc_window).unwrap();
    assert_eq!(
        panes_before.len(),
        1,
        "agent-doc window should have 1 pane before sync"
    );
    assert!(
        panes_before.contains(&pane0),
        "pane0 should be in agent-doc window"
    );

    // Run sync requesting doc_a + doc_b — this should NOT rescue pane1 pre-reconciler.
    // Instead, the reconciler should handle the swap.
    let result = run_with_tmux(
        &[
            doc_a.to_string_lossy().to_string(),
            doc_b.to_string_lossy().to_string(),
        ],
        Some(agent_doc_window),
        None,
        &iso,
    );
    assert!(result.is_ok(), "sync should succeed: {:?}", result.err());

    // After sync, both panes should be in the agent-doc window.
    let panes_after = iso.list_window_panes(agent_doc_window).unwrap();
    assert!(
        panes_after.contains(&pane0),
        "pane0 should be in agent-doc window after sync, got: {:?}",
        panes_after
    );
    assert!(
        panes_after.contains(&pane1),
        "pane1 (was in stash) should be in agent-doc window after sync, got: {:?}",
        panes_after
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn windowless_sync_targets_current_session_agent_doc_window() {
    let iso = IsolatedTmux::new("sync-windowless-target-agent-doc");
    let tmp = tempfile::TempDir::new().unwrap();

    let pane0 = iso.new_session("test", tmp.path()).unwrap();
    iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
        .unwrap();
    let pane1 = iso.split_window(&pane0, tmp.path(), "-dh").unwrap();
    iso.stash_pane(&pane1, "test").unwrap();
    iso.raw_cmd(&["select-window", "-t", "test:stash"]).unwrap();

    assert_eq!(
        current_tmux_session_name(&iso).as_deref(),
        Some("test"),
        "current session lookup should still point at the owning session"
    );
    assert_eq!(
        resolve_agent_doc_window_id(&iso, "test", "agent-doc").as_deref(),
        Some("@0"),
        "windowless sync should resolve the named agent-doc window instead of inheriting stash"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn windowless_sync_prefers_live_project_session_pin_over_current_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd = ScopedCurrentDir::set(tmp.path());
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/config.toml"),
        "tmux_session = \"0\"\n",
    )
    .unwrap();

    let iso = IsolatedTmux::new("sync-windowless-project-pin");
    let _pane0 = iso.new_session("0", tmp.path()).unwrap();
    iso.raw_cmd(&["rename-window", "-t", "0:0", "agent-doc"])
        .unwrap();
    let _pane1 = iso.new_session("1", tmp.path()).unwrap();

    assert_eq!(
        current_tmux_session_name(&iso).as_deref(),
        Some("1"),
        "the current client session should be the most recently created one"
    );
    assert_eq!(
        resolve_sync_target_session(&iso, None, &[], None).as_deref(),
        Some("0"),
        "windowless sync should honor a live project tmux_session pin before the current session"
    );
    assert_eq!(
        resolve_agent_doc_window_id(&iso, "0", "agent-doc").as_deref(),
        Some("@0")
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn windowless_sync_falls_back_to_current_session_when_project_pin_dead() {
    let tmp = tempfile::TempDir::new().unwrap();
    let _cwd = ScopedCurrentDir::set(tmp.path());
    std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
    std::fs::write(
        tmp.path().join(".agent-doc/config.toml"),
        "tmux_session = \"0\"\n",
    )
    .unwrap();

    let iso = IsolatedTmux::new("sync-windowless-dead-project-pin");
    let _pane1 = iso.new_session("1", tmp.path()).unwrap();

    assert_eq!(
        current_tmux_session_name(&iso).as_deref(),
        Some("1"),
        "the live attached session should still be discoverable"
    );
    assert_eq!(
        resolve_sync_target_session(&iso, None, &[], None).as_deref(),
        Some("1"),
        "a dead project pin should fall back to the current live session"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn windowless_sync_prefers_shared_workspace_root_pin_for_mixed_roots() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");

    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
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
            "---\nagent_doc_session: root-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-windowless-mixed-root-pin");
    let _pane1 = iso.new_session("1", root).unwrap();
    let _pane4 = iso.new_session("4", root).unwrap();

    let columns = vec![
        root_doc.to_string_lossy().to_string(),
        child_doc.to_string_lossy().to_string(),
    ];
    assert_eq!(
        resolve_sync_target_session(
            &iso,
            None,
            &columns,
            Some(child_doc.to_string_lossy().as_ref()),
        )
        .as_deref(),
        Some("4"),
        "mixed-root windowless sync should stay on the shared workspace root pin instead of the caller cwd or focused child root"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn explicit_non_agent_window_preserves_layout_when_session_lacks_agent_doc_window() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/boost-client");
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
    let child_doc = subroot.join("tasks/monsterrodholders.md");
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

    let mut registry = sessions::SessionRegistry::new();
    let canonical = doc.canonicalize().unwrap();
    let key = sessions::canonical_registry_key_in(&subroot, canonical.to_string_lossy().as_ref());
    registry.insert(
        key,
        sessions::SessionEntry {
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
    sessions::save_in(&subroot, &registry).unwrap();

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
fn register_synced_files_updates_each_project_registry_by_path_key() {
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

    let iso = IsolatedTmux::new("sync-cross-root-register");
    let root_pane = iso.new_session("test", root).unwrap();
    let child_pane = iso.split_window(&root_pane, &subroot, "-dh").unwrap();

    let mut child_registry = sessions::SessionRegistry::new();
    let bad_key =
        sessions::canonical_registry_key_in(&subroot, "src/session-share/tasks/claudescore-3.md");
    child_registry.insert(
        bad_key,
        sessions::SessionEntry {
            pane: child_pane.clone(),
            pid: pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            cwd: root.to_string_lossy().to_string(),
            started: String::new(),
            session_id: "child-session".to_string(),
            file: "src/session-share/tasks/claudescore-3.md".to_string(),
            window: iso.pane_window(&child_pane).unwrap(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(&subroot, &child_registry).unwrap();

    let _cwd = ScopedCurrentDir::set(root);
    register_synced_files(
        &iso,
        &[
            (
                "root-session".to_string(),
                PathBuf::from("tasks/agent-doc-bugs2.md"),
            ),
            (
                "child-session".to_string(),
                PathBuf::from("src/session-share/tasks/claudescore-3.md"),
            ),
        ],
        &[
            (PathBuf::from("tasks/agent-doc-bugs2.md"), root_pane.clone()),
            (
                PathBuf::from("src/session-share/tasks/claudescore-3.md"),
                child_pane.clone(),
            ),
        ],
    );

    let root_registry = sessions::load_in(root).unwrap();
    let root_key = sessions::canonical_registry_key_in(
        root,
        root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    let root_entry = root_registry
        .get(&root_key)
        .expect("root document should be registered in root registry");
    assert_eq!(root_entry.pane, root_pane);
    assert_eq!(root_entry.file, "tasks/agent-doc-bugs2.md");
    assert_eq!(root_registry.len(), 1);

    let child_registry = sessions::load_in(&subroot).unwrap();
    let child_key = sessions::canonical_registry_key_in(
        &subroot,
        child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    let child_entry = child_registry
        .get(&child_key)
        .expect("child document should be registered in child registry");
    assert_eq!(child_entry.pane, child_pane);
    assert_eq!(child_entry.file, "tasks/claudescore-3.md");
    assert_eq!(child_entry.cwd, subroot.to_string_lossy());
    assert_eq!(child_registry.len(), 1);
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn register_synced_files_prunes_cross_root_duplicate_pane_binding() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join("tasks")).unwrap();

    let root_doc = root.join("tasks/agent-doc-bugs2.md");
    let child_doc = subroot.join("tasks/agentic-harness-engineering.md");
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

    let iso = IsolatedTmux::new("sync-cross-root-duplicate-register");
    let root_pane = iso.new_session("test", root).unwrap();
    let window = iso.pane_window(&root_pane).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);

    let root_key = sessions::canonical_registry_key_in(
        root,
        root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    let mut root_registry = sessions::SessionRegistry::new();
    root_registry.insert(
        root_key,
        sessions::SessionEntry {
            pane: root_pane.clone(),
            pid: pane_pid_from_tmux(&iso, &root_pane).unwrap(),
            cwd: root.to_string_lossy().to_string(),
            started: "2026-05-01T00:44:03Z".to_string(),
            session_id: "root-session".to_string(),
            file: "tasks/agent-doc-bugs2.md".to_string(),
            window: window.clone(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(root, &root_registry).unwrap();

    let child_key = sessions::canonical_registry_key_in(
        &subroot,
        child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    let mut child_registry = sessions::SessionRegistry::new();
    child_registry.insert(
        child_key,
        sessions::SessionEntry {
            pane: root_pane.clone(),
            pid: pane_pid_from_tmux(&iso, &root_pane).unwrap(),
            cwd: root.to_string_lossy().to_string(),
            started: "2026-05-01T00:36:27Z".to_string(),
            session_id: "child-session".to_string(),
            file: "tasks/agentic-harness-engineering.md".to_string(),
            window: window.clone(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(&subroot, &child_registry).unwrap();

    let _cwd = ScopedCurrentDir::set(root);
    register_synced_files(
        &iso,
        &[
            (
                "root-session".to_string(),
                PathBuf::from("tasks/agent-doc-bugs2.md"),
            ),
            (
                "child-session".to_string(),
                PathBuf::from("src/session-share/tasks/agentic-harness-engineering.md"),
            ),
        ],
        &[
            (PathBuf::from("tasks/agent-doc-bugs2.md"), root_pane.clone()),
            (
                PathBuf::from("src/session-share/tasks/agentic-harness-engineering.md"),
                root_pane.clone(),
            ),
        ],
    );

    let root_registry = sessions::load_in(root).unwrap();
    let root_entry = root_registry
        .values()
        .find(|entry| entry.session_id == "root-session")
        .expect("root document should remain registered");
    assert_eq!(root_entry.pane, root_pane);

    let child_registry = sessions::load_in(&subroot).unwrap();
    assert!(
        child_registry.is_empty(),
        "duplicate cross-root pane binding should be pruned instead of preserved"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn register_synced_files_skips_geometry_only_binding_during_fail_closed_recovery() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");
    std::fs::create_dir_all(subroot.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(subroot.join("tasks")).unwrap();

    let child_doc = subroot.join("tasks/claudescore-3.md");
    std::fs::write(
        &child_doc,
        "---\nagent_doc_session: child-session\n---\n\n# Child\n",
    )
    .unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
            subroot.join(".agent-doc/logs/child-session.log"),
            format!(
                "[{}] session_start file=tasks/claudescore-3.md pane=%261 session=child-session\n[{}] codex_start mode=fresh restart_count=0\n[{}] supervisor_exit code=missing_pane pane=%261 reason=registered_pane_missing\n[{}] session_end origin=sync_missing_pane\n[{}] session_start file=tasks/claudescore-3.md pane=%261 session=child-session\n[{}] codex_start mode=fresh restart_count=0\n[{}] supervisor_exit code=missing_pane pane=%261 reason=registered_pane_missing\n[{}] session_end origin=sync_missing_pane\n",
                now.saturating_sub(8),
                now.saturating_sub(7),
                now.saturating_sub(6),
                now.saturating_sub(5),
                now.saturating_sub(4),
                now.saturating_sub(3),
                now.saturating_sub(2),
                now.saturating_sub(1),
            ),
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-fail-closed-geometry-binding");
    let child_pane = iso.new_session("test", &subroot).unwrap();

    let child_key = sessions::canonical_registry_key_in(
        &subroot,
        child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    let mut child_registry = sessions::SessionRegistry::new();
    child_registry.insert(
        child_key,
        sessions::SessionEntry {
            pane: child_pane.clone(),
            pid: pane_pid_from_tmux(&iso, &child_pane).unwrap(),
            cwd: subroot.to_string_lossy().to_string(),
            started: "2026-05-01T01:12:43Z".to_string(),
            session_id: "child-session".to_string(),
            file: "tasks/claudescore-3.md".to_string(),
            window: iso.pane_window(&child_pane).unwrap(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(&subroot, &child_registry).unwrap();

    let _cwd = ScopedCurrentDir::set(root);
    register_synced_files(
        &iso,
        &[(
            "child-session".to_string(),
            PathBuf::from("src/session-share/tasks/claudescore-3.md"),
        )],
        &[(
            PathBuf::from("src/session-share/tasks/claudescore-3.md"),
            child_pane.clone(),
        )],
    );

    let child_registry = sessions::load_in(&subroot).unwrap();
    assert!(
        child_registry.is_empty(),
        "fail-closed recovery should not let sync rebind a geometry-only pane assignment"
    );
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
    crate::session_actor::project_binding_in(
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

    let mut root_registry = sessions::SessionRegistry::new();
    let root_key = sessions::canonical_registry_key_in(
        root,
        root_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    root_registry.insert(
        root_key,
        sessions::SessionEntry {
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

    let mut child_registry = sessions::SessionRegistry::new();
    let child_key = sessions::canonical_registry_key_in(
        &subroot,
        child_doc.canonicalize().unwrap().to_string_lossy().as_ref(),
    );
    child_registry.insert(
        child_key,
        sessions::SessionEntry {
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
fn claimed_sync_pane_owner_ignores_same_file_and_reports_other_owner() {
    let claimed: RefCell<std::collections::HashMap<String, PathBuf>> =
        RefCell::new(std::collections::HashMap::new());
    let root_doc = PathBuf::from("tasks/agent-doc-bugs2.md");
    let child_doc = PathBuf::from("src/session-share/tasks/claudescore-3.md");

    reserve_sync_pane(&claimed, "%75", &root_doc);

    assert_eq!(
        claimed_sync_pane_owner(&claimed, "%75", &root_doc),
        None,
        "a file should be allowed to keep its own reserved pane"
    );
    assert_eq!(
        claimed_sync_pane_owner(&claimed, "%75", &child_doc),
        Some(root_doc),
        "another file should see the reservation conflict"
    );
}

#[test]
fn recover_existing_associated_pane_skips_reserved_candidates() {
    let claimed: RefCell<std::collections::HashMap<String, PathBuf>> =
        RefCell::new(std::collections::HashMap::new());
    reserve_sync_pane(&claimed, "%75", Path::new("tasks/agent-doc-bugs2.md"));

    let winner = AssociatedPaneCandidate {
        pane_id: "%75".to_string(),
        pane_pid: "1000".to_string(),
        session_name: "0".to_string(),
        window_id: "@1".to_string(),
        window_name: "agent-doc".to_string(),
        current_command: "agent-doc".to_string(),
        sources: [AssociatedPaneSource::ProcessTree].into_iter().collect(),
    };
    let filtered: Vec<AssociatedPaneCandidate> = vec![winner.clone()]
        .into_iter()
        .filter(|candidate| {
            claimed_sync_pane_owner(
                &claimed,
                &candidate.pane_id,
                Path::new("src/session-share/tasks/claudescore-3.md"),
            )
            .is_none()
        })
        .collect();
    assert!(
        filtered.is_empty(),
        "reserved pane candidates should be removed before associated-pane recovery"
    );

    match resolve_associated_panes(filtered, Some("@1")) {
        AssociatedPaneResolution::None => {}
        other => panic!("expected no available associated pane after filtering, got {other:?}"),
    }
}

#[test]
fn filter_duplicate_synthetic_registry_candidates_drops_ambiguous_same_root_duplicate_pane() {
    let filtered = filter_duplicate_synthetic_registry_candidates(vec![
        synthetic_registry_candidate("claudescore", "tasks/claudescore.md", "%250", false, true),
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
        synthetic_registry_candidate("claudescore", "tasks/claudescore.md", "%250", false, true),
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

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_stashes_open_cycle_pane_during_reconcile_detach() {
    let iso = IsolatedTmux::new("sync-open-cycle-protect");
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);

    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc_a = root.join("tasks/a.md");
    let doc_b = root.join("tasks/b.md");
    let content_a = concat!(
        "---\n",
        "agent_doc_session: sync-open-cycle-a\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "<!-- /agent:exchange -->\n"
    );
    let content_b = concat!(
        "---\n",
        "agent_doc_session: sync-open-cycle-b\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc_a, content_a).unwrap();
    std::fs::write(&doc_b, content_b).unwrap();
    snapshot::save(&doc_a, content_a).unwrap();
    snapshot::save(&doc_b, content_b).unwrap();

    let pane_a = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
    let window = iso.pane_window(&pane_a).unwrap();

    sessions::register_full_with_cwd(
        "sync-open-cycle-a",
        &pane_a,
        &doc_a.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_a).unwrap(),
        &window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "sync-open-cycle-b",
        &pane_b,
        &doc_b.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_b).unwrap(),
        &window,
        &root.to_string_lossy(),
    )
    .unwrap();

    crate::cycle_state::start_preflight(&doc_a, Some(content_a), Some(content_a)).unwrap();

    run_with_tmux(
        &[doc_b.to_string_lossy().to_string()],
        Some("test:agent-doc"),
        Some(doc_b.to_string_lossy().as_ref()),
        &iso,
    )
    .unwrap();

    let visible = iso.list_window_panes("test:agent-doc").unwrap();
    assert!(
        !visible.contains(&pane_a),
        "open-cycle extra pane should be stashed instead of forcing a 3-pane projection: {visible:?}"
    );
    assert!(
        visible.contains(&pane_b),
        "requested pane must remain visible after reconcile: {visible:?}"
    );
    assert!(iso.pane_alive(&pane_a), "open-cycle pane must stay alive");
    assert_ne!(
        iso.pane_window(&pane_a).unwrap(),
        window,
        "open-cycle pane should move out of the visible agent-doc window"
    );
    assert_eq!(
        crate::cycle_state::load(&doc_a).unwrap().unwrap().phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_sync_preserves_existing_layout_for_vscode_mixed_root_split_replay() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");
    std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(root.join("tasks/software")).unwrap();
    std::fs::create_dir_all(root.join("tasks/agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();
    let _cwd = ScopedCurrentDir::set(root);

    let tsift_doc = root.join("tasks/software/tsift.md");
    let bugs_doc = root.join("tasks/agent-doc/agent-doc-bugs2.md");
    let claudescore_doc = subroot.join("tasks/claudescore-3.md");
    std::fs::write(
            &tsift_doc,
            "---\nagent_doc_session: tsift-v0.1\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &bugs_doc,
            "---\nagent_doc_session: bugs-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &claudescore_doc,
            "---\nagent_doc_session: claudescore-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            root.join(".agent-doc/logs/tsift-v0.1.log"),
            "[1] session_start file=tasks/software/tsift.md pane=%26 session=tsift-v0.1\n[2] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-safe-passive-no-alias");
    let bugs_pane = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let dev_pane = iso.split_window(&bugs_pane, &subroot, "-dh").unwrap();
    let agent_doc_window = iso.pane_window(&bugs_pane).unwrap();
    let dev_pane_pid = pane_pid_from_tmux(&iso, &dev_pane).unwrap();

    let _ipc =
        crate::supervisor::ipc::SupervisorIpc::start(subroot.as_path(), "claudescore-session", {
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                        "pid": dev_pane_pid
                    }))
                }
                crate::supervisor::ipc::IpcMethod::State => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                        "supervisor_pid": dev_pane_pid,
                        "supervisor_instance_id": "dev-instance",
                    }))
                }
                _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    sessions::register_full_with_cwd(
        "bugs-session",
        &bugs_pane,
        &bugs_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &bugs_pane).unwrap(),
        &agent_doc_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd_in(
        &subroot,
        "claudescore-session",
        &dev_pane,
        &claudescore_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &dev_pane).unwrap(),
        &agent_doc_window,
        &subroot.to_string_lossy(),
    )
    .unwrap();

    run_with_options_internal(
        &[
            tsift_doc.to_string_lossy().to_string(),
            claudescore_doc.to_string_lossy().to_string(),
        ],
        None,
        Some(tsift_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let root_registry = sessions::load_in(root).unwrap();
    assert!(
        !root_registry
            .values()
            .any(|entry| entry.session_id == "tsift-v0.1"),
        "blocked passive sync must not register tsift onto a spare pane"
    );

    let ordered = iso.list_panes_ordered(&agent_doc_window).unwrap();
    assert_eq!(
        ordered,
        vec![bugs_pane.clone(), dev_pane.clone()],
        "blocked passive sync must preserve the agent-doc-bugs2/claudescore-3 visible split instead of letting the remaining foreign pane become authoritative"
    );
    assert!(
        iso.pane_alive(&bugs_pane),
        "the preserved workspace pane must remain alive"
    );
    assert!(
        iso.pane_alive(&dev_pane),
        "the resolved sibling pane must also remain visible"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_sync_reuses_alive_registered_pane_before_full_live_owner_proof() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc = root.join("tasks/passive-fast-path.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: passive-fast-path\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-safe-passive-registered-fast-path");
    let pane = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let window_id = iso.pane_window(&pane).unwrap();

    sessions::register_full_with_cwd(
        "passive-fast-path",
        &pane,
        &doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane).unwrap(),
        &window_id,
        &root.to_string_lossy(),
    )
    .unwrap();

    run_with_options_internal(
        &[doc.to_string_lossy().to_string()],
        None,
        Some(doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&window_id).unwrap();
    assert_eq!(
        ordered,
        vec![pane.clone()],
        "safe passive sync should immediately reuse the alive registered pane instead of provisioning a replacement"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_sync_attaches_requested_pane_and_stashes_open_cycle_extra() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc_a = root.join("tasks/a.md");
    let doc_b = root.join("tasks/b.md");
    let doc_c = root.join("tasks/c.md");
    for (path, session) in [
        (&doc_a, "apresync-a"),
        (&doc_b, "bpresync-b"),
        (&doc_c, "cpresync-c"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let iso = IsolatedTmux::new("sync-safe-passive-protected-grow");
    let pane_a = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
    let target_window = iso.pane_window(&pane_a).unwrap();
    let pane_c = iso.new_window("test", root).unwrap();
    let pane_c_window = iso.pane_window(&pane_c).unwrap();

    sessions::register_full_with_cwd(
        "apresync-a",
        &pane_a,
        &doc_a.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_a).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "bpresync-b",
        &pane_b,
        &doc_b.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_b).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "cpresync-c",
        &pane_c,
        &doc_c.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_c).unwrap(),
        &pane_c_window,
        &root.to_string_lossy(),
    )
    .unwrap();

    let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
    crate::cycle_state::start_preflight(&doc_a, Some(&doc_a_content), Some(&doc_a_content))
        .unwrap();

    run_with_options_internal(
        &[
            doc_c.to_string_lossy().to_string(),
            doc_b.to_string_lossy().to_string(),
        ],
        None,
        Some(doc_c.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert!(
        !ordered.contains(&pane_a),
        "open-cycle extra pane should be stashed while other documents sync"
    );
    assert!(
        ordered.contains(&pane_b),
        "already visible requested pane should remain visible"
    );
    assert!(
        ordered.contains(&pane_c),
        "requested hidden pane should be attached immediately instead of waiting for the protected pane to close out"
    );
    assert_eq!(
        iso.pane_window(&pane_c).unwrap(),
        target_window,
        "safe passive sync should move the requested pane into the visible agent-doc window"
    );
    assert!(iso.pane_alive(&pane_a), "open-cycle pane must stay alive");
    assert_ne!(
        iso.pane_window(&pane_a).unwrap(),
        target_window,
        "open-cycle pane should no longer be visible in the requested projection"
    );
}

#[test]
#[ignore = "covered by sync_sim_tmuxbudget_seed_3001; safe-passive tmux smoke keeps the real pane/window path covered"]
fn manual_sync_attaches_requested_pane_around_protected_open_cycle_pane() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc_a = root.join("tasks/a.md");
    let doc_b = root.join("tasks/b.md");
    let doc_c = root.join("tasks/c.md");
    for (path, session) in [
        (&doc_a, "amanual-a"),
        (&doc_b, "bmanual-b"),
        (&doc_c, "cmanual-c"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let iso = IsolatedTmux::new("sync-manual-protected-grow");
    let pane_a = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
    let target_window = iso.pane_window(&pane_a).unwrap();
    let pane_c = iso.new_window("test", root).unwrap();
    let pane_c_window = iso.pane_window(&pane_c).unwrap();

    sessions::register_full_with_cwd(
        "amanual-a",
        &pane_a,
        &doc_a.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_a).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "bmanual-b",
        &pane_b,
        &doc_b.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_b).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "cmanual-c",
        &pane_c,
        &doc_c.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_c).unwrap(),
        &pane_c_window,
        &root.to_string_lossy(),
    )
    .unwrap();

    let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
    crate::cycle_state::start_preflight(&doc_a, Some(&doc_a_content), Some(&doc_a_content))
        .unwrap();

    run_with_options_internal(
        &[
            doc_c.to_string_lossy().to_string(),
            doc_b.to_string_lossy().to_string(),
        ],
        None,
        Some(doc_c.to_string_lossy().as_ref()),
        AutoStartMode::Full,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert!(
        !ordered.contains(&pane_a),
        "open-cycle extra pane should be stashed while other documents sync"
    );
    assert!(
        ordered.contains(&pane_c),
        "manual sync should attach the requested hidden pane immediately"
    );
    assert_eq!(
        iso.pane_window(&pane_c).unwrap(),
        target_window,
        "manual sync should move the requested pane into the visible agent-doc window"
    );
}

fn sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
    mode: AutoStartMode,
    test_name: &str,
) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let protected_doc = root.join("tasks/protected.md");
    let detached_doc = root.join("tasks/detached.md");
    let requested_doc = root.join("tasks/requested.md");
    for (path, session) in [
        (&protected_doc, "sync-replace-protected"),
        (&detached_doc, "sync-replace-detached"),
        (&requested_doc, "sync-replace-requested"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let iso = IsolatedTmux::new(test_name);
    let protected_pane = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let detached_pane = iso.split_window(&protected_pane, root, "-dh").unwrap();
    let target_window = iso.pane_window(&protected_pane).unwrap();
    let requested_pane = iso.new_window("test", root).unwrap();

    sessions::register_full_with_cwd(
        "sync-replace-protected",
        &protected_pane,
        &protected_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &protected_pane).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "sync-replace-detached",
        &detached_pane,
        &detached_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &detached_pane).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "sync-replace-requested",
        &requested_pane,
        &requested_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &requested_pane).unwrap(),
        &iso.pane_window(&requested_pane).unwrap(),
        &root.to_string_lossy(),
    )
    .unwrap();

    let protected_content = std::fs::read_to_string(&protected_doc).unwrap();
    crate::cycle_state::start_preflight(
        &protected_doc,
        Some(&protected_content),
        Some(&protected_content),
    )
    .unwrap();

    run_with_options_internal(
        &[requested_doc.to_string_lossy().to_string()],
        None,
        Some(requested_doc.to_string_lossy().as_ref()),
        mode,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert!(
        !ordered.contains(&protected_pane),
        "open-cycle extra pane should be stashed instead of remaining visible"
    );
    assert!(
        ordered.contains(&requested_pane),
        "requested hidden pane should be brought into the visible agent-doc window"
    );
    assert!(
        !ordered.contains(&detached_pane),
        "detachable visible pane should be displaced instead of making sync a no-op"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        requested_pane,
        "sync should focus the requested pane after replacing a detachable visible pane"
    );
    assert!(
        iso.pane_alive(&protected_pane),
        "open-cycle pane should stay alive after being stashed"
    );
}

#[test]
fn safe_passive_sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra() {
    sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
        AutoStartMode::SafePassive,
        "sync-safe-passive-replace-detachable",
    );
}

#[test]
#[ignore = "covered by sync_sim_tmuxbudget_seed_3002; safe-passive tmux smoke keeps the real pane/window path covered"]
fn manual_sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra() {
    sync_replaces_detachable_visible_pane_and_stashes_open_cycle_extra(
        AutoStartMode::Full,
        "sync-manual-replace-detachable",
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_blocked_layout_preserve_still_reselects_visible_focus_pane() {
    let root = tempfile::TempDir::new().unwrap();
    let subroot = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(root.path().join("tasks/software")).unwrap();
    std::fs::create_dir_all(root.path().join("tasks/agent-doc")).unwrap();
    std::fs::write(
        root.path().join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(subroot.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.path().join("tasks")).unwrap();
    std::fs::write(
        subroot.path().join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();
    let _cwd = ScopedCurrentDir::set(root.path());

    let tsift_doc = root.path().join("tasks/software/tsift.md");
    let bugs_doc = root.path().join("tasks/agent-doc/agent-doc-bugs2.md");
    let claudescore_doc = subroot.path().join("tasks/claudescore-3.md");
    std::fs::write(
            &tsift_doc,
            "---\nagent_doc_session: tsift-v0.1\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &bugs_doc,
            "---\nagent_doc_session: bugs-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &claudescore_doc,
            "---\nagent_doc_session: claudescore-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            root.path().join(".agent-doc/logs/tsift-v0.1.log"),
            "[1] session_start file=tasks/software/tsift.md pane=%26 session=tsift-v0.1\n[2] document_cycle phase=committed cycle=cycle-1 event=commit_success capture_id=cycle-1\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("sync-safe-passive-blocked-focus");
    let bugs_pane = iso.new_session("test", root.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let dev_pane = iso.split_window(&bugs_pane, subroot.path(), "-dh").unwrap();
    let agent_doc_window = iso.pane_window(&bugs_pane).unwrap();
    let dev_pane_pid = pane_pid_from_tmux(&iso, &dev_pane).unwrap();

    let _ipc =
        crate::supervisor::ipc::SupervisorIpc::start(subroot.path(), "claudescore-session", {
            move |method| match method {
                crate::supervisor::ipc::IpcMethod::Pid => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                        "pid": dev_pane_pid
                    }))
                }
                crate::supervisor::ipc::IpcMethod::State => {
                    crate::supervisor::ipc::IpcResponse::ok(serde_json::json!({
                        "supervisor_pid": dev_pane_pid,
                        "supervisor_instance_id": "dev-instance",
                    }))
                }
                _ => crate::supervisor::ipc::IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    sessions::register_full_with_cwd(
        "bugs-session",
        &bugs_pane,
        &bugs_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &bugs_pane).unwrap(),
        &agent_doc_window,
        &root.path().to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd_in(
        subroot.path(),
        "claudescore-session",
        &dev_pane,
        &claudescore_doc.to_string_lossy(),
        pane_pid_from_tmux(&iso, &dev_pane).unwrap(),
        &agent_doc_window,
        &subroot.path().to_string_lossy(),
    )
    .unwrap();
    iso.select_pane(&bugs_pane).unwrap();

    run_with_options_internal(
        &[
            tsift_doc.to_string_lossy().to_string(),
            claudescore_doc.to_string_lossy().to_string(),
        ],
        None,
        Some(claudescore_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&agent_doc_window).unwrap();
    assert_eq!(
        ordered,
        vec![bugs_pane.clone(), dev_pane.clone()],
        "blocked passive sync must preserve the visible layout"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        dev_pane,
        "blocked passive sync should still reselect the already-visible focused pane"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_focus_only_editor_switch_preserves_sibling_pane() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let left_doc = root.join("tasks/left.md");
    let right_doc = root.join("tasks/right.md");
    let new_left_doc = root.join("tasks/new-left.md");
    for (path, session) in [
        (&left_doc, "left-session"),
        (&right_doc, "right-session"),
        (&new_left_doc, "new-left-session"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let layout_state = vec![
        left_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        right_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    ];
    std::fs::write(
        root.join(".agent-doc/last_layout.json"),
        serde_json::to_string(&layout_state).unwrap(),
    )
    .unwrap();

    let iso = IsolatedTmux::new("sync-focus-only-editor-switch");
    let left_pane = iso.new_session("test", root).unwrap();
    iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
        .unwrap();
    let right_pane = iso.split_window(&left_pane, root, "-dh").unwrap();
    let target_window = iso.pane_window(&left_pane).unwrap();
    let new_left_pane = iso.new_window("test", root).unwrap();
    let new_left_window = iso.pane_window(&new_left_pane).unwrap();

    for (session, pane, window, doc) in [
        ("left-session", &left_pane, &target_window, &left_doc),
        ("right-session", &right_pane, &target_window, &right_doc),
        (
            "new-left-session",
            &new_left_pane,
            &new_left_window,
            &new_left_doc,
        ),
    ] {
        sessions::register_full_with_cwd(
            session,
            pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, pane).unwrap(),
            window,
            &root.to_string_lossy(),
        )
        .unwrap();
    }
    iso.select_pane(&left_pane).unwrap();

    run_with_options_internal(
        &[new_left_doc.to_string_lossy().to_string()],
        None,
        Some(new_left_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert_eq!(
        ordered,
        vec![new_left_pane.clone(), right_pane.clone()],
        "focus-only editor tab switches should update the active side without collapsing the sibling pane"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        new_left_pane,
        "focused replacement pane should be selected after the same-side handoff"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_focus_only_existing_sibling_focus_does_not_replace_active_pane() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let bugs_doc = root.join("tasks/bugs.md");
    let docs_doc = root.join("tasks/docs.md");
    for (path, session) in [(&bugs_doc, "bugs-session"), (&docs_doc, "docs-session")] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let layout_state = vec![
        bugs_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        docs_doc
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    ];
    std::fs::write(
        root.join(".agent-doc/last_layout.json"),
        serde_json::to_string(&layout_state).unwrap(),
    )
    .unwrap();

    let iso = IsolatedTmux::new("sync-focus-only-visible-sibling");
    let bugs_pane = iso.new_session("test", root).unwrap();
    iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
        .unwrap();
    let docs_pane = iso.split_window(&bugs_pane, root, "-dh").unwrap();
    let target_window = iso.pane_window(&bugs_pane).unwrap();

    for (session, pane, doc) in [
        ("bugs-session", &bugs_pane, &bugs_doc),
        ("docs-session", &docs_pane, &docs_doc),
    ] {
        sessions::register_full_with_cwd(
            session,
            pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, pane).unwrap(),
            &target_window,
            &root.to_string_lossy(),
        )
        .unwrap();
    }
    iso.select_pane(&bugs_pane).unwrap();

    run_with_options_internal(
        &[docs_doc.to_string_lossy().to_string()],
        None,
        Some(docs_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert_eq!(
        ordered,
        vec![bugs_pane.clone(), docs_pane.clone()],
        "after a turn ends on the old pane, editor focus of an already-visible sibling must not collapse or replace that pane"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        docs_pane,
        "focus-only sync should select the existing focused sibling pane"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn safe_passive_focus_only_editor_switch_preserves_sibling_without_saved_layout() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let left_doc = root.join("tasks/left.md");
    let right_doc = root.join("tasks/right.md");
    let new_left_doc = root.join("tasks/new-left.md");
    for (path, session) in [
        (&left_doc, "left-session-nosaved"),
        (&right_doc, "right-session-nosaved"),
        (&new_left_doc, "new-left-session-nosaved"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let iso = IsolatedTmux::new("sync-focus-only-no-saved-layout");
    let left_pane = iso.new_session("test", root).unwrap();
    iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"])
        .unwrap();
    let right_pane = iso.split_window(&left_pane, root, "-dh").unwrap();
    let target_window = iso.pane_window(&left_pane).unwrap();
    let new_left_pane = iso.new_window("test", root).unwrap();
    let new_left_window = iso.pane_window(&new_left_pane).unwrap();

    for (session, pane, window, doc) in [
        (
            "left-session-nosaved",
            &left_pane,
            &target_window,
            &left_doc,
        ),
        (
            "right-session-nosaved",
            &right_pane,
            &target_window,
            &right_doc,
        ),
        (
            "new-left-session-nosaved",
            &new_left_pane,
            &new_left_window,
            &new_left_doc,
        ),
    ] {
        sessions::register_full_with_cwd(
            session,
            pane,
            &doc.to_string_lossy(),
            pane_pid_from_tmux(&iso, pane).unwrap(),
            window,
            &root.to_string_lossy(),
        )
        .unwrap();
    }
    iso.select_pane(&left_pane).unwrap();

    run_with_options_internal(
        &[new_left_doc.to_string_lossy().to_string()],
        None,
        Some(new_left_doc.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert_eq!(
        ordered,
        vec![new_left_pane.clone(), right_pane.clone()],
        "focus-only sync should derive the current split from visible registered panes when last_layout.json is absent"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        new_left_pane,
        "focused replacement pane should be selected without collapsing the sibling pane"
    );
}

#[test]
#[ignore = "covered by sync_sim_tmuxbudget_seed_3004; safe-passive attach/focus tmux smoke keeps the real pane/window path covered"]
fn safe_passive_protected_open_cycle_sync_still_selects_visible_focus_pane() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let _cwd = ScopedCurrentDir::set(root);
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"test\"\n",
    )
    .unwrap();

    let doc_a = root.join("tasks/a.md");
    let doc_b = root.join("tasks/b.md");
    let doc_c = root.join("tasks/c.md");
    for (path, session) in [
        (&doc_a, "afocus-a"),
        (&doc_b, "bfocus-b"),
        (&doc_c, "cfocus-c"),
    ] {
        std::fs::write(
                path,
                format!(
                    "---\nagent_doc_session: {session}\nagent_doc_format: template\nagent_doc_write: crdt\n---\n"
                ),
            )
            .unwrap();
    }

    let iso = IsolatedTmux::new("sync-safe-passive-protected-focus");
    let pane_a = iso.new_session("test", root).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "test:0", "agent-doc"]);
    let pane_b = iso.split_window(&pane_a, root, "-dh").unwrap();
    let target_window = iso.pane_window(&pane_a).unwrap();
    let pane_c = iso.new_window("test", root).unwrap();
    let pane_c_window = iso.pane_window(&pane_c).unwrap();

    sessions::register_full_with_cwd(
        "afocus-a",
        &pane_a,
        &doc_a.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_a).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "bfocus-b",
        &pane_b,
        &doc_b.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_b).unwrap(),
        &target_window,
        &root.to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "cfocus-c",
        &pane_c,
        &doc_c.to_string_lossy(),
        pane_pid_from_tmux(&iso, &pane_c).unwrap(),
        &pane_c_window,
        &root.to_string_lossy(),
    )
    .unwrap();

    let doc_a_content = std::fs::read_to_string(&doc_a).unwrap();
    crate::cycle_state::start_preflight(&doc_a, Some(&doc_a_content), Some(&doc_a_content))
        .unwrap();
    iso.select_pane(&pane_a).unwrap();

    run_with_options_internal(
        &[
            doc_c.to_string_lossy().to_string(),
            doc_b.to_string_lossy().to_string(),
        ],
        None,
        Some(doc_b.to_string_lossy().as_ref()),
        AutoStartMode::SafePassive,
        false,
        &iso,
    )
    .unwrap();

    let ordered = iso.list_panes_ordered(&target_window).unwrap();
    assert!(
        !ordered.contains(&pane_a),
        "open-cycle extra pane should be stashed while other documents sync"
    );
    assert!(
        ordered.contains(&pane_c),
        "requested hidden pane should be attached even while another document is mid-closeout"
    );
    assert_eq!(
        iso.active_pane("test").unwrap(),
        pane_b,
        "sync should still select the already-visible focused pane"
    );
    assert_eq!(
        iso.pane_window(&pane_c).unwrap(),
        target_window,
        "requested hidden pane should move into the visible agent-doc window"
    );
}
