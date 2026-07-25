//! Startup-miss state and supervisor session-log I/O helpers.

use std::io::Write;
use std::path::{Path, PathBuf};

use agent_doc_state_backbone::{StateEvent, StateFact};
use anyhow::Result;

use agent_doc_supervisor::startup_miss::{
    RecentSessionLossWindow, SessionLogStatus, StartupMiss, StartupMissOrigin,
    StartupMissSupersession, registered_start_supersedes_miss,
};
use agent_doc_supervisor::{
    OwnershipGeneration, OwnershipTransitionEvent, format_transition_event,
};

const SUPERVISOR_LOG_DIR: &str = ".agent-doc/logs";

/// Registered startup owner facts supplied by a session-registry lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredStartupOwner {
    pub pane: String,
    pub session_id: String,
}

/// Registry lookup boundary for startup-miss supersession checks.
pub trait StartupMissRegistryLookup {
    fn registered_startup_owner(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<RegisteredStartupOwner>>;
}

/// Session-registry-backed startup-miss owner lookup.
pub struct SessionRegistryStartupMissLookup;

const SESSION_REGISTRY_LOOKUP: SessionRegistryStartupMissLookup = SessionRegistryStartupMissLookup;

pub fn session_registry_lookup() -> &'static SessionRegistryStartupMissLookup {
    &SESSION_REGISTRY_LOOKUP
}

impl StartupMissRegistryLookup for SessionRegistryStartupMissLookup {
    fn registered_startup_owner(
        &self,
        project_root: &Path,
        file: &Path,
    ) -> Result<Option<RegisteredStartupOwner>> {
        Ok(
            agent_doc_session_registry_io::lookup_file_entry_in(project_root, file)?.map(|entry| {
                RegisteredStartupOwner {
                    pane: entry.pane,
                    session_id: entry.session_id,
                }
            }),
        )
    }
}

fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Return the project root used for startup-miss state for `file`.
pub fn startup_miss_project_root(file: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    agent_doc_fs::find_project_root(&canonical)
}

/// Compute `.agent-doc/logs/<session_id>.log` for `file`'s project.
pub fn supervisor_session_log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = agent_doc_fs::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(SUPERVISOR_LOG_DIR)
            .join(format!("{session_id}.log")),
    ))
}

pub fn append_session_log_event(file: &Path, session_id: &str, event: &str) -> Result<bool> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let timestamp = agent_doc_log_time::format_log_timestamp(current_epoch_secs());
    writeln!(log, "[{timestamp}] {event}")?;
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
pub struct RegistryRebindSessionLog<'a> {
    pub base_dir: &'a Path,
    pub session_id: &'a str,
    pub previous_file: &'a str,
    pub previous_pane: &'a str,
    pub previous_window: &'a str,
    pub new_pane: &'a str,
    pub new_window: &'a str,
    pub transition_caller: &'a str,
    pub transition_reason: &'a str,
    pub generations: OwnershipGeneration,
}

pub fn append_registry_rebind_session_log(event: RegistryRebindSessionLog<'_>) -> Result<bool> {
    if event.previous_pane == event.new_pane {
        return Ok(false);
    }

    let log_file = resolve_relative_file(event.base_dir, event.previous_file);
    let old_window = if event.previous_window.is_empty() {
        "unknown"
    } else {
        event.previous_window
    };
    let new_window = if event.new_window.is_empty() {
        "unknown"
    } else {
        event.new_window
    };

    let transition = format_transition_event(OwnershipTransitionEvent {
        caller: event.transition_caller,
        reason: event.transition_reason,
        prior_generation: event.generations.prior_generation,
        new_generation: event.generations.new_generation,
        old_pane: Some(event.previous_pane),
        new_pane: event.new_pane,
        old_window: Some(old_window),
        new_window: Some(new_window),
    });
    append_session_log_event(&log_file, event.session_id, &transition)?;

    let superseded = format!(
        "session_superseded old_pane={} new_pane={} old_window={} new_window={} prior_generation={} new_generation={}",
        event.previous_pane,
        event.new_pane,
        old_window,
        new_window,
        event.generations.prior_generation,
        event.generations.new_generation
    );
    append_session_log_event(&log_file, event.session_id, &superseded)?;

    append_session_log_event(
        &log_file,
        event.session_id,
        &format!(
            "session_end origin=registry_rebind pane={} next_pane={} generation={} next_generation={}",
            event.previous_pane,
            event.new_pane,
            event.generations.prior_generation,
            event.generations.new_generation
        ),
    )?;
    Ok(true)
}

fn resolve_relative_file(base_dir: &Path, file: &str) -> PathBuf {
    let file_path = Path::new(file);
    if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        base_dir.join(file_path)
    }
}

pub fn record_startup_miss(
    file: &Path,
    pane_id: &str,
    session_id: &str,
    harness: &str,
    origin: StartupMissOrigin,
    cycle_baseline_id: Option<&str>,
) -> Result<StartupMiss> {
    let timestamp = current_epoch_secs();
    let Some(identity) = crate::state_events::document_state_identity(file)? else {
        return Ok(StartupMiss {
            file: file.display().to_string(),
            pane_id: pane_id.to_string(),
            session_id: session_id.to_string(),
            harness: harness.to_string(),
            timestamp,
            origin,
            cycle_baseline_id: cycle_baseline_id.map(|s| s.to_string()),
        });
    };
    let marker = StartupMiss {
        file: identity.canonical_file.display().to_string(),
        pane_id: pane_id.to_string(),
        session_id: session_id.to_string(),
        harness: harness.to_string(),
        timestamp,
        origin,
        cycle_baseline_id: cycle_baseline_id.map(|s| s.to_string()),
    };
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )?;
    let next_epoch = ledger
        .document_epoch(&identity.document_hash)
        .saturating_add(1);
    let event = StateEvent::new(
        format!(
            "startup-miss-recorded:{}:epoch-{next_epoch}",
            identity.document_hash
        ),
        StateFact::StartupMissRecorded {
            document_hash: identity.document_hash,
            file: marker.file.clone(),
            pane_id: marker.pane_id.clone(),
            session_id: marker.session_id.clone(),
            harness: marker.harness.clone(),
            timestamp: marker.timestamp,
            origin: startup_miss_origin_token(&marker.origin).to_string(),
            cycle_baseline_id: marker.cycle_baseline_id.clone(),
        },
    );
    crate::state_events::append_event(&identity.project_root, &event)?;
    Ok(marker)
}

pub fn load_startup_miss(file: &Path) -> Result<Option<StartupMiss>> {
    let Some(identity) = crate::state_events::document_state_identity(file)? else {
        return Ok(None);
    };
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )?;
    let Some(miss) = ledger
        .project_document(&identity.document_hash)
        .and_then(|projection| projection.supervisor.startup_miss)
    else {
        return Ok(None);
    };
    Ok(Some(StartupMiss {
        file: miss.file,
        pane_id: miss.pane_id,
        session_id: miss.session_id,
        harness: miss.harness,
        timestamp: miss.timestamp,
        origin: startup_miss_origin_from_token(&miss.origin)?,
        cycle_baseline_id: miss.cycle_baseline_id,
    }))
}

pub fn clear_startup_miss(file: &Path) -> Result<()> {
    let Some(identity) = crate::state_events::document_state_identity(file)? else {
        return Ok(());
    };
    let ledger = crate::state_events::load_document_ledger_shared(
        &identity.project_root,
        &identity.document_hash,
    )?;
    let Some(miss) = ledger
        .project_document(&identity.document_hash)
        .and_then(|projection| projection.supervisor.startup_miss)
    else {
        return Ok(());
    };
    let next_epoch = ledger
        .document_epoch(&identity.document_hash)
        .saturating_add(1);
    let event = StateEvent::new(
        format!(
            "startup-miss-cleared:{}:epoch-{next_epoch}",
            identity.document_hash
        ),
        StateFact::StartupMissCleared {
            document_hash: identity.document_hash,
            pane_id: miss.pane_id,
            session_id: miss.session_id,
            timestamp: miss.timestamp,
        },
    );
    crate::state_events::append_event(&identity.project_root, &event)?;
    Ok(())
}

fn startup_miss_origin_token(origin: &StartupMissOrigin) -> &'static str {
    match origin {
        StartupMissOrigin::FreshStart => "fresh_start",
        StartupMissOrigin::RoutedTrigger => "routed_trigger",
    }
}

fn startup_miss_origin_from_token(token: &str) -> Result<StartupMissOrigin> {
    match token {
        "fresh_start" => Ok(StartupMissOrigin::FreshStart),
        "routed_trigger" => Ok(StartupMissOrigin::RoutedTrigger),
        other => anyhow::bail!("unknown startup-miss origin in state.db: {other}"),
    }
}

pub fn superseded_by_newer_registered_start(
    registry: &impl StartupMissRegistryLookup,
    file: &Path,
    miss: &StartupMiss,
) -> Result<Option<StartupMissSupersession>> {
    let Some(root) = startup_miss_project_root(file) else {
        return Ok(None);
    };
    let Some(registered_entry) = registry.registered_startup_owner(&root, file)? else {
        return Ok(None);
    };
    let status = session_log_status(file, &registered_entry.session_id)?;
    Ok(registered_start_supersedes_miss(
        miss,
        &registered_entry.pane,
        status.as_ref(),
    ))
}

pub fn take_superseded_startup_miss(
    registry: &impl StartupMissRegistryLookup,
    file: &Path,
) -> Result<Option<(StartupMiss, StartupMissSupersession)>> {
    let Some(miss) = load_startup_miss(file)? else {
        return Ok(None);
    };
    let Some(supersession) = superseded_by_newer_registered_start(registry, file, &miss)? else {
        return Ok(None);
    };
    clear_startup_miss(file)?;
    Ok(Some((miss, supersession)))
}

#[allow(dead_code)]
pub fn is_startup_miss_pane(file: &Path, pane_id: &str) -> bool {
    load_startup_miss(file)
        .ok()
        .flatten()
        .is_some_and(|m| m.pane_id == pane_id)
}

pub fn session_log_status(file: &Path, session_id: &str) -> Result<Option<SessionLogStatus>> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    Ok(agent_doc_supervisor::startup_miss::session_log_status_from_content(&content))
}

pub fn session_log_has_event_after_latest_start(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
) -> Result<bool> {
    session_log_has_event_after_latest_start_matching(file, session_id, event_prefix, |_| true)
}

pub fn session_log_has_event_after_latest_start_containing(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
    required_fragment: &str,
) -> Result<bool> {
    session_log_has_event_after_latest_start_matching(file, session_id, event_prefix, |event| {
        event.contains(required_fragment)
    })
}

fn session_log_has_event_after_latest_start_matching(
    file: &Path,
    session_id: &str,
    event_prefix: &str,
    matches_event: impl Fn(&str) -> bool,
) -> Result<bool> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(false);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(false);
    };
    Ok(
        agent_doc_supervisor::startup_miss::session_log_has_event_after_latest_start(
            &content,
            event_prefix,
            matches_event,
        ),
    )
}

pub fn session_log_diagnostic(file: &Path, session_id: &str) -> Result<Option<String>> {
    let Some(status) = session_log_status(file, session_id)? else {
        return Ok(None);
    };
    Ok(Some(
        agent_doc_supervisor::startup_miss::session_log_diagnostic(&status),
    ))
}

pub fn recent_session_loss_window(
    file: &Path,
    session_id: &str,
) -> Result<Option<RecentSessionLossWindow>> {
    recent_session_loss_window_at(file, session_id, current_epoch_secs())
}

pub fn recent_session_loss_window_at(
    file: &Path,
    session_id: &str,
    now_epoch_secs: u64,
) -> Result<Option<RecentSessionLossWindow>> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(None);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(None);
    };
    Ok(agent_doc_supervisor::startup_miss::recent_session_loss_window_at(&content, now_epoch_secs))
}

/// Count session-loss events in the supervisor session log for `file`/`session_id`
/// within `window_secs` of now. Reads the same ledger the route auto-start
/// fallback consults (`recent_session_loss_window`) so the controller supervisor
/// watchdog (`#supresilience` Part B) shares one crash-window ledger rather than a
/// competing counter. Missing log ⇒ `Ok(0)`.
pub fn count_recent_session_loss_events(
    file: &Path,
    session_id: &str,
    window_secs: u64,
) -> Result<usize> {
    count_recent_session_loss_events_at(file, session_id, window_secs, current_epoch_secs())
}

pub fn count_recent_session_loss_events_at(
    file: &Path,
    session_id: &str,
    window_secs: u64,
    now_epoch_secs: u64,
) -> Result<usize> {
    let Some(path) = supervisor_session_log_path(file, session_id)? else {
        return Ok(0);
    };
    let Some(content) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(
        agent_doc_supervisor::startup_miss::count_session_loss_events_within(
            &content,
            now_epoch_secs,
            window_secs,
        ),
    )
}

pub fn record_session_loss(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    reason: &str,
    last_known_window: Option<&str>,
) -> Result<bool> {
    let status = session_log_status(file, session_id)?;
    let Some((exit_event, session_end_event)) =
        agent_doc_supervisor::startup_miss::missing_pane_session_loss_events(
            status.as_ref(),
            pane_id,
            reason,
            last_known_window,
        )
    else {
        return Ok(false);
    };
    append_session_log_event(file, session_id, &exit_event)?;
    append_session_log_event(file, session_id, &session_end_event)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeRegistryLookup {
        owner: RefCell<Option<RegisteredStartupOwner>>,
    }

    impl StartupMissRegistryLookup for FakeRegistryLookup {
        fn registered_startup_owner(
            &self,
            _project_root: &Path,
            _file: &Path,
        ) -> Result<Option<RegisteredStartupOwner>> {
            Ok(self.owner.borrow().clone())
        }
    }

    fn setup_project(tmp: &Path) -> PathBuf {
        std::fs::create_dir_all(tmp.join(".agent-doc")).unwrap();
        let doc = tmp.join("nested").join("test.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# test\n").unwrap();
        doc
    }

    fn temp_dir_without_agent_doc_ancestor() -> Option<tempfile::TempDir> {
        for base in [
            PathBuf::from("/var/tmp"),
            PathBuf::from("/dev/shm"),
            std::env::temp_dir(),
        ] {
            if !base.is_dir() || has_agent_doc_ancestor(&base) {
                continue;
            }
            if let Ok(dir) = tempfile::Builder::new()
                .prefix("agent-doc-supervisor-io-no-root")
                .tempdir_in(base)
            {
                return Some(dir);
            }
        }
        None
    }

    fn has_agent_doc_ancestor(path: &Path) -> bool {
        let Ok(mut current) = path.canonicalize() else {
            return false;
        };
        loop {
            if current.join(".agent-doc").is_dir() {
                return true;
            }
            if !current.pop() {
                return false;
            }
        }
    }

    #[test]
    fn supervisor_session_log_path_uses_project_root_and_session_id() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());

        assert_eq!(
            supervisor_session_log_path(&doc, "session-123").unwrap(),
            Some(tmp.path().join(".agent-doc/logs/session-123.log"))
        );
    }

    #[test]
    fn registry_rebind_session_log_appends_transition_superseded_and_end_events() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());

        let appended = append_registry_rebind_session_log(RegistryRebindSessionLog {
            base_dir: tmp.path(),
            session_id: "session-123",
            previous_file: "nested/test.md",
            previous_pane: "%1",
            previous_window: "@1",
            new_pane: "%2",
            new_window: "@2",
            transition_caller: "sync",
            transition_reason: "recover_owner",
            generations: OwnershipGeneration {
                prior_generation: 1,
                new_generation: 2,
            },
        })
        .unwrap();

        assert!(appended);
        let log =
            std::fs::read_to_string(tmp.path().join(".agent-doc/logs/session-123.log")).unwrap();
        assert!(log.contains("ownership_transition"));
        assert!(log.contains("caller=sync"));
        assert!(log.contains("reason=recover_owner"));
        assert!(log.contains("session_superseded old_pane=%1 new_pane=%2"));
        assert!(log.contains("session_end origin=registry_rebind pane=%1 next_pane=%2"));

        let skipped = append_registry_rebind_session_log(RegistryRebindSessionLog {
            base_dir: tmp.path(),
            session_id: "session-123",
            previous_file: doc.to_str().unwrap(),
            previous_pane: "%2",
            previous_window: "@2",
            new_pane: "%2",
            new_window: "@2",
            transition_caller: "sync",
            transition_reason: "recover_owner",
            generations: OwnershipGeneration {
                prior_generation: 2,
                new_generation: 3,
            },
        })
        .unwrap();
        assert!(!skipped);
    }

    #[test]
    fn path_helpers_return_none_without_project_root() {
        let Some(tmp) = temp_dir_without_agent_doc_ancestor() else {
            return;
        };
        let doc = tmp.path().join("test.md");
        std::fs::write(&doc, "# test\n").unwrap();

        assert!(
            crate::state_events::document_state_identity(&doc)
                .unwrap()
                .is_none()
        );
        assert!(
            supervisor_session_log_path(&doc, "session-123")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn startup_miss_project_root_handles_missing_document_path() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("missing.md");

        assert_eq!(
            startup_miss_project_root(&doc),
            Some(tmp.path().to_path_buf())
        );
    }

    #[test]
    fn record_persists_startup_miss() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record_startup_miss(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::FreshStart,
            Some("cycle-abc"),
        )
        .unwrap();
        let loaded = load_startup_miss(&doc)
            .unwrap()
            .expect("should have marker");
        assert_eq!(loaded.pane_id, "%42");
        assert_eq!(loaded.session_id, "session-123");
        assert_eq!(loaded.harness, "claude");
        assert_eq!(loaded.origin, StartupMissOrigin::FreshStart);
        assert_eq!(loaded.cycle_baseline_id.as_deref(), Some("cycle-abc"));
    }

    #[test]
    fn load_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        assert!(load_startup_miss(&doc).unwrap().is_none());
    }

    #[test]
    fn clear_removes_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record_startup_miss(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::RoutedTrigger,
            None,
        )
        .unwrap();
        assert!(load_startup_miss(&doc).unwrap().is_some());
        clear_startup_miss(&doc).unwrap();
        assert!(load_startup_miss(&doc).unwrap().is_none());
    }

    #[test]
    fn superseded_by_newer_registered_start_detects_stale_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            concat!(
                "[1] session_start file=test.md pane=%401 session=session-123\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[10] session_start file=test.md pane=%408 session=session-123\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();
        let registry = FakeRegistryLookup {
            owner: RefCell::new(Some(RegisteredStartupOwner {
                pane: "%408".to_string(),
                session_id: "session-123".to_string(),
            })),
        };
        let miss = StartupMiss {
            file: doc.display().to_string(),
            pane_id: "%401".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 5,
            origin: StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: None,
        };

        let supersession = superseded_by_newer_registered_start(&registry, &doc, &miss)
            .unwrap()
            .expect("stale marker should be superseded");
        assert_eq!(supersession.registered_pane, "%408");
        assert_eq!(supersession.latest_start_pane, "%408");
        assert_eq!(supersession.latest_start_timestamp, 10);
    }

    #[test]
    fn take_superseded_startup_miss_clears_marker_and_returns_supersession() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            concat!(
                "[1] session_start file=test.md pane=%401 session=session-123\n",
                "[2] codex_start mode=fresh restart_count=0\n",
                "[10] session_start file=test.md pane=%408 session=session-123\n",
                "[11] codex_start mode=fresh restart_count=0\n",
            ),
        )
        .unwrap();
        let marker = record_startup_miss(
            &doc,
            "%401",
            "session-123",
            "codex",
            StartupMissOrigin::RoutedTrigger,
            None,
        )
        .unwrap();
        let newer_timestamp = marker.timestamp.saturating_add(1);
        std::fs::write(
            tmp.path().join(".agent-doc/logs/session-123.log"),
            format!(
                "[{}] session_start file=test.md pane=%401 session=session-123\n[{}] codex_start mode=fresh restart_count=0\n[{newer_timestamp}] session_start file=test.md pane=%408 session=session-123\n[{newer_timestamp}] codex_start mode=fresh restart_count=0\n",
                marker.timestamp.saturating_sub(1),
                marker.timestamp
            ),
        )
        .unwrap();
        let registry = FakeRegistryLookup {
            owner: RefCell::new(Some(RegisteredStartupOwner {
                pane: "%408".to_string(),
                session_id: "session-123".to_string(),
            })),
        };

        let (miss, supersession) = take_superseded_startup_miss(&registry, &doc)
            .unwrap()
            .expect("stale marker should be taken");
        assert_eq!(miss.pane_id, "%401");
        assert_eq!(supersession.registered_pane, "%408");
        assert!(load_startup_miss(&doc).unwrap().is_none());
    }

    #[test]
    fn session_registry_lookup_finds_canonical_file_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            doc.display().to_string(),
            tmux_router::RegistryEntry {
                pane: "%408".to_string(),
                pid: 1,
                cwd: tmp.path().display().to_string(),
                started: "2026-04-29T00:00:00Z".to_string(),
                session_id: "session-456".to_string(),
                file: doc.display().to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        agent_doc_session_registry_io::save_in(tmp.path(), &registry).unwrap();

        let owner = session_registry_lookup()
            .registered_startup_owner(tmp.path(), &doc)
            .unwrap()
            .expect("registered owner");
        assert_eq!(owner.pane, "%408");
        assert_eq!(owner.session_id, "session-456");
    }

    #[test]
    fn is_startup_miss_pane_matches() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        record_startup_miss(
            &doc,
            "%42",
            "session-123",
            "claude",
            StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        assert!(is_startup_miss_pane(&doc, "%42"));
        assert!(!is_startup_miss_pane(&doc, "%99"));
    }

    #[test]
    fn session_log_status_reports_open_latest_session() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(
            logs_dir.join("session-123.log"),
            "[1] session_start file=test.md pane=%41 session=session-123 generation=1\n[2] ipc_started project_root=/tmp/project\n[3] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let status = session_log_status(&doc, "session-123")
            .unwrap()
            .expect("session log status");
        assert_eq!(status.latest_start_pane.as_deref(), Some("%41"));
        assert_eq!(status.latest_run_timestamp, Some(3));
        assert_eq!(
            status.latest_run_event.as_deref(),
            Some("codex_start mode=fresh restart_count=0")
        );
        assert!(!status.saw_committed_cycle_after_latest_run);
        assert!(status.latest_session_open());
        assert!(!status.latest_session_closed());
        assert_eq!(
            session_log_diagnostic(&doc, "session-123").unwrap(),
            Some(
                "latest harness run `codex_start mode=fresh restart_count=0` on pane=%41; session log still has no later child exit or session_end"
                    .to_string()
            )
        );
    }

    #[test]
    fn session_log_event_after_latest_start_resets_on_new_start() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(
            logs_dir.join("session-123.log"),
            "[1] session_start file=test.md pane=%41 session=session-123 generation=1\n\
             [2] codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1\n\
             [3] session_start file=test.md pane=%42 session=session-123 generation=2\n",
        )
        .unwrap();

        assert!(
            !session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven",
            )
            .unwrap()
        );

        append_session_log_event(
            &doc,
            "session-123",
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=1",
        )
        .unwrap();
        assert!(
            session_log_has_event_after_latest_start(
                &doc,
                "session-123",
                "codex_capability_proof status=proven",
            )
            .unwrap()
        );
    }

    #[test]
    fn record_session_loss_closes_open_latest_session() {
        let tmp = tempfile::TempDir::new().unwrap();
        let doc = setup_project(tmp.path());
        let log_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("session-loss.log");
        std::fs::write(
            &log_path,
            "[1] session_start file=test.md pane=%61 session=session-loss generation=1\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let recorded = record_session_loss(
            &doc,
            "session-loss",
            "%61",
            "registered_pane_missing",
            Some("@9"),
        )
        .unwrap();
        assert!(recorded, "open sessions should record a loss event");

        let status = session_log_status(&doc, "session-loss")
            .unwrap()
            .expect("status should remain readable");
        assert!(status.latest_session_closed());

        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("supervisor_exit code=missing_pane"));
        assert!(log.contains("reason=registered_pane_missing"));
        assert!(log.contains("last_known_window=@9"));
    }

    #[test]
    fn recent_session_loss_window_requires_multiple_recent_losses() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(
            logs_dir.join("session-loss-window.log"),
            concat!(
                "[100] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n",
                "[200] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                "[250] pane_death_detected pane=%42 status=9 cycle_phase=preflight_started\n",
                "[900] supervisor_exit code=missing_pane pane=%43 reason=registered_pane_missing\n",
            ),
        )
        .unwrap();

        let recent = recent_session_loss_window_at(&doc, "session-loss-window", 260)
            .unwrap()
            .expect("two recent session losses should trip the guard");
        assert_eq!(recent.count, 2);
        assert_eq!(recent.first_timestamp, 100);
        assert_eq!(recent.last_timestamp, 200);
        assert_eq!(
            recent.latest_reason.as_deref(),
            Some("registered_pane_dead")
        );

        assert!(
            recent_session_loss_window_at(&doc, "session-loss-window", 1000)
                .unwrap()
                .is_none(),
            "old session-loss events outside the guard window should not trip it"
        );
    }

    #[test]
    fn count_recent_session_loss_events_reads_shared_ledger() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = setup_project(tmp.path());
        let logs_dir = tmp.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(
            logs_dir.join("watchdog-window.log"),
            concat!(
                "[100] supervisor_exit code=missing_pane pane=%41 reason=watchdog_dead_supervisor\n",
                "[200] supervisor_exit code=missing_pane pane=%41 reason=watchdog_dead_supervisor\n",
                "[900] supervisor_exit code=missing_pane pane=%41 reason=watchdog_dead_supervisor\n",
            ),
        )
        .unwrap();

        // 300s window ending at 350 sees the two recent losses (unlike the guard
        // there is no minimum-count threshold).
        assert_eq!(
            count_recent_session_loss_events_at(&doc, "watchdog-window", 300, 350).unwrap(),
            2
        );
        assert_eq!(
            count_recent_session_loss_events_at(&doc, "watchdog-window", 300, 950).unwrap(),
            1
        );
        // Missing session log reads as zero.
        assert_eq!(
            count_recent_session_loss_events_at(&doc, "no-such-session", 300, 350).unwrap(),
            0
        );
    }
}
