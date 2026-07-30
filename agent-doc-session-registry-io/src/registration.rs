//! # Module: sessions
//!
//! Session registration — records document-to-pane ownership and supervisor
//! metadata.
//!
//! Registry lives in `.agent-doc/state.db` relative to the project root.
//! Low-level registry path/load/save/lookup IO lives in
//! `agent-doc-session-registry-io`. Agent-doc-specific registration, rebind
//! logging, and supervisor metadata updates live here. Tmux observation and
//! command effects live in focused tmux crates; callers that need tmux-router
//! types import them from `tmux_router` directly.
//!
//! ## Spec
//! - `register(session_id, pane_id, file)` acquires the lock, calls
//!   `register_with_pid` using the current process ID.
//! - `register_with_pid` queries the pane's window and delegates to `register_full`.
//! - `register_supervisor` records the authoritative supervisor PID +
//!   supervisor instance identity for a live pane owner.
//! - `register_full` enforces single-session-per-pane by evicting stale entries that
//!   share the same pane before inserting the new `SessionEntry`.
//! - When the same session UUID is rebound to a different pane, `register_full`
//!   best-effort appends supervisor session-log rebind events before the new
//!   pane registration lands.
//! ## Agentic Contracts
//! - Registry snapshot IO is not self-locking. Any read-modify-write cycle must
//!   acquire `RegistryLock` first; prefer `tmux_router::with_registry` for safe
//!   cycles.
//! - `register_full` guarantees at most one registry entry per pane ID; pre-existing
//!   entries pointing to the same pane are removed before the new entry is inserted.
//!
//! ## Evals
//! - registry_multiple_sessions_isolated: two entries with distinct pane IDs round-trip
//!   independently without cross-contamination.
//! - registry_overwrite_existing_session: inserting the same session_id twice replaces
//!   the entry; registry length stays at 1.
//! - prune_removes_dead_panes: retain entries whose pane is alive removes fabricated
//!   dead pane IDs, leaving an empty registry.
//! - register_full_deduplicates_pane: seeding two sessions with the same pane then
//!   calling `register_full` with a third session leaves only the third in the registry.
//! - pane_alive_returns_false_for_nonexistent: `pane_alive("%99999")` returns `false`.
//! - tmux_create_session_and_verify: isolated server → `new_session` → pane ID starts
//!   with `%` and `pane_alive` returns `true`.
//! - tmux_auto_start_cascade: auto_start on no-server, no-session, and existing-session
//!   each produce a live pane in the correct session.

use crate as session_registry_io;
use agent_doc_controller::actor::ActorRecord;
use agent_doc_session_registry as session_registry;
use agent_doc_sqlite::state_store;
use agent_doc_supervisor::{
    OwnershipGeneration, OwnershipTransitionEvent, format_transition_event,
};
use anyhow::Result;
use std::io::Write;
use std::path::Path;

#[cfg(test)]
use tmux_router::IsolatedTmux;
use tmux_router::{Registry as SessionRegistry, RegistryEntry as SessionEntry, RegistryLock, Tmux};

// ---------------------------------------------------------------------------
// Free functions — registration operations and env-based checks
// ---------------------------------------------------------------------------

pub fn register(session_id: &str, pane_id: &str, file: &str) -> Result<()> {
    register_with_pid(session_id, pane_id, file, std::process::id())
}

#[allow(dead_code)]
pub fn register_with_pid(session_id: &str, pane_id: &str, file: &str, pid: u32) -> Result<()> {
    register_with_pid_internal(session_id, pane_id, file, pid, "register", "register")
}

fn register_with_pid_internal(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let tmux = Tmux::default_server();
    let window = agent_doc_tmux_io::target_window_id(&tmux, pane_id).unwrap_or_default();
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        &window,
        None,
        None,
        transition_caller,
        transition_reason,
    )
}

/// Like `register_with_pid` but accepts an explicit `cwd` override.
/// Used by `claim` to record the document's nearest git repo root as cwd,
/// avoiding superproject drift when claiming submodule-hosted documents.
pub fn register_with_pid_and_cwd(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    cwd: &str,
) -> Result<()> {
    let tmux = Tmux::default_server();
    let window = agent_doc_tmux_io::target_window_id(&tmux, pane_id).unwrap_or_default();
    register_full_with_cwd_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        &window,
        cwd,
        None,
        "claim",
        "claim_bind",
    )
}

#[allow(dead_code)]
pub fn attach_with_pid_and_cwd_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(base_dir))?;
    let mut registry = session_registry_io::load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "session",
        "manual_attach",
    )
}

pub fn attach_projection_only_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(base_dir))?;
    let mut registry = session_registry_io::load_in(base_dir)?;
    let started = chrono_now();
    let replacement = session_registry::replace_registry_entry(
        base_dir,
        &mut registry,
        session_registry::RegistryEntryFields {
            session_id,
            pane_id,
            file,
            pid,
            cwd,
            started: &started,
            window,
            supervisor_instance_id: "",
        },
    );
    log_stale_registry_keys(&replacement.stale_keys, pane_id);
    session_registry_io::save_in(base_dir, &registry)
}

pub fn register_supervisor(
    session_id: &str,
    pane_id: &str,
    file: &str,
    supervisor_pid: u32,
    supervisor_instance_id: &str,
) -> Result<()> {
    let tmux = Tmux::default_server();
    let window = agent_doc_tmux_io::target_window_id(&tmux, pane_id).unwrap_or_default();
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        supervisor_pid,
        &window,
        None,
        Some(supervisor_instance_id),
        "start",
        "supervisor_register",
    )
}

pub fn register_supervisor_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    supervisor_pid: u32,
    supervisor_instance_id: &str,
) -> Result<()> {
    let tmux = Tmux::default_server();
    let window = agent_doc_tmux_io::target_window_id(&tmux, pane_id).unwrap_or_default();
    register_full_with_cwd_and_instance_in(
        base_dir,
        session_id,
        pane_id,
        file,
        supervisor_pid,
        &window,
        &base_dir.to_string_lossy(),
        supervisor_instance_id,
        "sync",
        "recover_owner",
    )
}

#[allow(clippy::too_many_arguments)]
pub fn register_start_supervisor_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    supervisor_pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: &str,
) -> Result<()> {
    register_full_with_cwd_and_instance_in(
        base_dir,
        session_id,
        pane_id,
        file,
        supervisor_pid,
        window,
        cwd,
        supervisor_instance_id,
        "start",
        "supervisor_register",
    )
}

#[cfg(test)]
#[allow(dead_code)]
pub fn register_full(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
) -> Result<()> {
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        window,
        None,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full` but with an explicit `base_dir` for the registry.
#[allow(dead_code)]
pub fn register_full_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(base_dir))?;
    let mut registry = session_registry_io::load_in(base_dir)?;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        &cwd,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full` but uses the provided `cwd` instead of querying the process cwd.
#[allow(dead_code)]
pub fn register_full_with_cwd(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    register_full_with_cwd_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full_with_cwd` but with an explicit `base_dir` for the registry.
pub fn register_full_with_cwd_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(base_dir))?;
    let mut registry = session_registry_io::load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "route",
        "dispatch_bind",
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_with_cwd_and_instance_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: &str,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(base_dir))?;
    let mut registry = session_registry_io::load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        Some(supervisor_instance_id),
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_internal_call(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: Option<&str>,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let base_dir = std::env::current_dir()?;
    let resolved_cwd = cwd
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| base_dir.to_string_lossy().to_string());
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(&base_dir))?;
    let mut registry = session_registry_io::load_in(&base_dir)?;
    register_full_internal(
        &base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        &resolved_cwd,
        supervisor_instance_id,
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_with_cwd_internal_call(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let base_dir = std::env::current_dir()?;
    let _lock = RegistryLock::acquire(&session_registry_io::registry_path_in(&base_dir))?;
    let mut registry = session_registry_io::load_in(&base_dir)?;
    register_full_internal(
        &base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        supervisor_instance_id,
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_internal(
    base_dir: &Path,
    registry: &mut SessionRegistry,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let started = chrono_now();
    let registry_key = session_registry::canonical_registry_key_in(base_dir, file);
    let supervisor_instance_id = supervisor_instance_id.unwrap_or_default().to_string();

    // Enforce single session per pane: remove stale entries pointing to same pane
    let stale_keys = session_registry::remove_stale_pane_bindings(registry, pane_id, session_id);
    log_stale_registry_keys(&stale_keys, pane_id);
    if !stale_keys.is_empty() {
        session_registry_io::save_in(base_dir, registry)?;
    }

    let mut controller_row_exists = false;
    if let Some(previous) = registry.get(&registry_key).cloned() {
        if transition_caller != "start" {
            let generations = agent_doc_session_actor_io::project_binding_in(
                base_dir,
                file,
                session_id,
                pane_id,
                window,
                transition_caller,
                transition_reason,
            )?;
            log_session_rebind(
                base_dir,
                session_id,
                &previous,
                pane_id,
                window,
                transition_caller,
                transition_reason,
                generations,
            );
            controller_row_exists = true;
        }
    } else if transition_caller != "start" {
        let _ = agent_doc_session_actor_io::project_binding_in(
            base_dir,
            file,
            session_id,
            pane_id,
            window,
            transition_caller,
            transition_reason,
        )?;
        controller_row_exists = true;
    } else if load_actor_record(base_dir, &registry_key)?.is_some() {
        controller_row_exists = true;
    }

    if controller_row_exists {
        persist_registry_metadata_for_actor(
            base_dir,
            &registry_key,
            session_id,
            pane_id,
            file,
            pid,
            window,
            cwd,
            &started,
            &supervisor_instance_id,
        )?;
        return Ok(());
    }

    session_registry::insert_registry_entry(
        base_dir,
        registry,
        session_registry::RegistryEntryFields {
            session_id,
            pane_id,
            file,
            pid,
            cwd,
            started: &started,
            window,
            supervisor_instance_id: &supervisor_instance_id,
        },
    );
    session_registry_io::save_in(base_dir, registry)?;
    Ok(())
}

fn log_stale_registry_keys(stale_keys: &[String], pane_id: &str) {
    for key in stale_keys {
        eprintln!(
            "[registry] removing stale session {} (was pane {})",
            key, pane_id
        );
    }
}

pub fn load_actor_record(project_root: &Path, document_id: &str) -> Result<Option<ActorRecord>> {
    Ok(
        agent_doc_session_actor_io::load_all_records_in(project_root)?
            .get(document_id)
            .cloned(),
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_registry_metadata_for_actor(
    base_dir: &Path,
    document_id: &str,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    started: &str,
    supervisor_instance_id: &str,
) -> Result<()> {
    let actor = load_actor_record(base_dir, document_id)?;
    let entry = session_registry::registry_entry(session_registry::RegistryEntryFields {
        pane_id: actor
            .as_ref()
            .map(|record| record.pane_id.as_str())
            .unwrap_or(pane_id),
        pid,
        cwd,
        started,
        session_id: actor
            .as_ref()
            .map(|record| record.session_id.as_str())
            .unwrap_or(session_id),
        file,
        window: actor
            .as_ref()
            .map(|record| record.window_id.as_str())
            .unwrap_or(window),
        supervisor_instance_id,
    });
    tmux_router::registry::upsert_entry_metadata(
        &session_registry_io::registry_path_in(base_dir),
        document_id,
        &entry,
    )
}

#[allow(clippy::too_many_arguments)]
fn log_session_rebind(
    base_dir: &Path,
    session_id: &str,
    previous: &SessionEntry,
    new_pane: &str,
    new_window: &str,
    transition_caller: &str,
    transition_reason: &str,
    generations: OwnershipGeneration,
) {
    if let Err(err) = append_registry_rebind_session_log(RegistryRebindSessionLog {
        base_dir,
        session_id,
        previous_file: &previous.file,
        previous_pane: &previous.pane,
        previous_window: &previous.window,
        new_pane,
        new_window,
        transition_caller,
        transition_reason,
        generations,
    }) {
        eprintln!(
            "[registry] warning: failed to append registry-rebind session log for {}: {}",
            session_id, err
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct RegistryRebindSessionLog<'a> {
    base_dir: &'a Path,
    session_id: &'a str,
    previous_file: &'a str,
    previous_pane: &'a str,
    previous_window: &'a str,
    new_pane: &'a str,
    new_window: &'a str,
    transition_caller: &'a str,
    transition_reason: &'a str,
    generations: OwnershipGeneration,
}

fn append_registry_rebind_session_log(event: RegistryRebindSessionLog<'_>) -> Result<bool> {
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

fn append_session_log_event(file: &Path, session_id: &str, event: &str) -> Result<bool> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(false);
    };
    let path = root
        .join(".agent-doc/logs")
        .join(format!("{session_id}.log"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let timestamp = agent_doc_log_time::format_log_timestamp(state_store::timestamp_secs());
    writeln!(log, "[{timestamp}] {event}")?;
    Ok(true)
}

fn resolve_relative_file(base_dir: &Path, file: &str) -> std::path::PathBuf {
    let file_path = Path::new(file);
    if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        base_dir.join(file_path)
    }
}

/// Simple UTC timestamp without pulling in chrono.
fn chrono_now() -> String {
    agent_doc_log_time::current_log_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_controller::actor::{ActorLastTransition, ActorState};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn registry_roundtrip() {
        let dir = TempDir::new().unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(
            "test-session".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 12345,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "test-session".to_string(),
                file: "test.md".to_string(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        session_registry_io::save_in(dir.path(), &reg).unwrap();
        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        let key = session_registry::canonical_registry_key_in(dir.path(), "test.md");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&key].pane, "%42");
    }

    #[test]
    fn load_empty_returns_empty_map() {
        let dir = TempDir::new().unwrap();
        let reg = session_registry_io::load_in(dir.path()).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn pane_alive_returns_false_for_nonexistent() {
        assert!(!Tmux::default_server().pane_alive("%99999"));
    }

    #[test]
    fn registry_multiple_sessions_isolated() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%10".to_string(),
                pid: 1000,
                cwd: "/tmp/a".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-a".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%20".to_string(),
                pid: 2000,
                cwd: "/tmp/b".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                session_id: "session-b".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        let json = serde_json::to_string_pretty(&reg).unwrap();
        let loaded: SessionRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["session-a"].pane, "%10");
        assert_eq!(loaded["session-b"].pane, "%20");
        assert_ne!(loaded["session-a"].pane, loaded["session-b"].pane);
        assert_ne!(loaded["session-a"].pid, loaded["session-b"].pid);
    }

    #[test]
    fn registry_overwrite_existing_session() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%old".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-x".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%new".to_string(),
                pid: 200,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:05:00Z".to_string(),
                session_id: "session-x".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        assert_eq!(reg.len(), 1);
        assert_eq!(reg["session-x"].pane, "%new");
        assert_eq!(reg["session-x"].pid, 200);
    }

    #[test]
    fn prune_removes_dead_panes_from_map() {
        let tmux = Tmux::default_server();
        let mut reg = SessionRegistry::new();
        reg.insert(
            "dead-session-1".to_string(),
            SessionEntry {
                pane: "%99998".to_string(),
                pid: 1,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "dead-session-1".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "dead-session-2".to_string(),
            SessionEntry {
                pane: "%99997".to_string(),
                pid: 2,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "dead-session-2".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        let before = reg.len();
        reg.retain(|_, entry| tmux.pane_alive(&entry.pane));
        let removed = before - reg.len();

        assert_eq!(removed, 2);
        assert!(reg.is_empty());
    }

    // -----------------------------------------------------------------------
    // Tmux isolation tests — use `-L` to create independent tmux servers
    // -----------------------------------------------------------------------

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_isolated_server_not_running_initially() {
        let t = IsolatedTmux::new("agent-doc-test-not-running");
        assert!(!t.running());
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_create_session_and_verify() {
        let t = IsolatedTmux::new("agent-doc-test-create-session");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test-session", tmp.path()).unwrap();
        assert!(!pane_id.is_empty(), "pane_id should not be empty");
        assert!(pane_id.starts_with('%'), "pane_id should start with %");

        assert!(t.running());
        assert!(t.session_exists("test-session"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_session_exists_returns_false_for_missing() {
        let t = IsolatedTmux::new("agent-doc-test-session-missing");
        let tmp = TempDir::new().unwrap();

        t.new_session("existing", tmp.path()).unwrap();

        assert!(t.session_exists("existing"));
        assert!(!t.session_exists("nonexistent"));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_new_window_creates_second_pane() {
        let t = IsolatedTmux::new("agent-doc-test-new-window");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("test", tmp.path()).unwrap();
        let pane2 = t.new_window("test", tmp.path()).unwrap();

        assert_ne!(pane1, pane2, "two windows should have different pane IDs");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_send_keys_to_pane() {
        let t = IsolatedTmux::new("agent-doc-test-send-keys");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        t.send_keys(&pane_id, "echo hello").unwrap();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn opencode_submit_uses_named_enter_key() {
        let t = IsolatedTmux::new("agent-doc-test-opencode-enter-key");
        let tmp = TempDir::new().unwrap();
        let pane_id = t.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("input.bin");
        let done_path = tmp.path().join("done");
        let ready_path = tmp.path().join("ready");
        let expected = b"agent-doc plan.md\r";

        let reader = format!(
            "sh -lc 'stty raw -echo; touch \"{}\"; dd bs=1 count={} of=\"{}\" 2>/dev/null; touch \"{}\"'",
            ready_path.display(),
            expected.len(),
            output_path.display(),
            done_path.display()
        );
        let setup_status = t
            .cmd()
            .args(["send-keys", "-t", &pane_id, "-l"])
            .arg(&reader)
            .status()
            .unwrap();
        assert!(setup_status.success(), "raw reader setup command failed");
        t.send_key(&pane_id, "Enter").unwrap();

        for _ in 0..60 {
            if ready_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            ready_path.exists(),
            "expected raw reader to enter raw mode before payload submit"
        );

        agent_doc_tmux_io::send_submitted_text_for_harness_logged(
            &t,
            &pane_id,
            "agent-doc plan.md\n",
            "opencode",
            agent_doc_tmux_io::input_diag::InputDiagSink::new(None, agent_doc_ops_log_io::log_op),
            "sessions.send_submitted_text_for_harness",
        )
        .unwrap();

        for _ in 0..60 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(done_path.exists(), "expected raw reader to finish");
        assert_eq!(std::fs::read(output_path).unwrap(), expected);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_pane_alive_returns_false_after_kill() {
        let t = IsolatedTmux::new("agent-doc-test-pane-kill");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        assert!(t.pane_alive(&pane_id));

        // Create a second window so we can kill the first without killing the session
        let _pane2 = t.new_window("test", tmp.path()).unwrap();

        let _ = t.cmd().args(["kill-pane", "-t", &pane_id]).status();

        assert!(!t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_no_server() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-server");
        let tmp = TempDir::new().unwrap();

        assert!(!t.running());
        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(!pane_id.is_empty());
        assert!(t.running());
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_no_session() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-session");
        let tmp = TempDir::new().unwrap();

        t.new_session("other", tmp.path()).unwrap();
        assert!(t.running());
        assert!(!t.session_exists("claude"));

        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_session_exists() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-exists");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("claude", tmp.path()).unwrap();

        let pane2 = t.auto_start("claude", tmp.path()).unwrap();
        assert_ne!(pane1, pane2, "should be a different pane (new window)");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    // --- #tw4a: register_with_pid_and_cwd tests ---

    #[test]
    fn register_with_pid_and_cwd_stores_explicit_cwd() {
        // register_full_with_cwd_in should store the provided cwd string
        // rather than querying the process cwd.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let explicit_cwd = "/home/user/projects/my-submodule";
        register_full_with_cwd_in(
            dir.path(),
            "session-cwd-test",
            "%77",
            "plan.md",
            9999,
            "@5",
            explicit_cwd,
        )
        .unwrap();

        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        let key = session_registry::canonical_registry_key_in(dir.path(), "plan.md");
        assert!(loaded.contains_key(&key), "session should be registered");
        let entry = &loaded[&key];
        assert_eq!(
            entry.cwd, explicit_cwd,
            "stored cwd must match the explicit value passed in"
        );
        assert_eq!(entry.pane, "%77");
        assert_eq!(entry.file, "plan.md");
        assert_eq!(entry.pid, 9999);
    }

    #[test]
    fn register_full_with_cwd_differs_from_process_cwd() {
        // The cwd stored by register_full_with_cwd_in must be the explicitly passed value,
        // even when it differs from the process cwd.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let explicit_cwd = "/completely/different/path";
        register_full_with_cwd_in(
            dir.path(),
            "s-explicit",
            "%88",
            "doc.md",
            1234,
            "@1",
            explicit_cwd,
        )
        .unwrap();

        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        let key = session_registry::canonical_registry_key_in(dir.path(), "doc.md");
        let entry = &loaded[&key];
        assert_eq!(entry.cwd, explicit_cwd);
        // Verify it differs from the actual process cwd
        let process_cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_ne!(
            entry.cwd, process_cwd,
            "stored cwd should differ from process cwd"
        );
    }

    #[test]
    fn register_start_supervisor_in_uses_explicit_project_root() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("tasks/professional");
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
        let doc = nested.join("sampleportal.md");
        std::fs::write(&doc, "# SP\n").unwrap();
        let cwd = nested.to_string_lossy().to_string();
        let file = doc.to_string_lossy().to_string();

        register_start_supervisor_in(
            dir.path(),
            "efs-session",
            "%26",
            &file,
            4242,
            "@3",
            &cwd,
            "sup-efs",
        )
        .unwrap();

        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        let key = session_registry::canonical_registry_key_in(dir.path(), &file);
        let entry = loaded
            .get(&key)
            .expect("start supervisor should publish into the project registry");
        assert_eq!(entry.session_id, "efs-session");
        assert_eq!(entry.pane, "%26");
        assert_eq!(entry.cwd, cwd);
        assert_eq!(entry.supervisor_instance_id, "sup-efs");

        let nested_registry = session_registry_io::load_in(&nested).unwrap();
        assert!(
            nested_registry.is_empty(),
            "nested cwd registry must not receive start supervisor publication"
        );
    }

    #[test]
    fn register_start_supervisor_persists_stale_pane_prune_before_controller_projection() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let old_doc = dir.path().join("tasks/sampleportal.md");
        let new_doc = dir.path().join("tasks/agent-doc-bugs2.md");
        std::fs::write(&old_doc, "# old\n").unwrap();
        std::fs::write(&new_doc, "# new\n").unwrap();

        let old_file = old_doc.to_string_lossy().to_string();
        let new_file = new_doc.to_string_lossy().to_string();
        let old_key = session_registry::canonical_registry_key_in(dir.path(), &old_file);
        let new_key = session_registry::canonical_registry_key_in(dir.path(), &new_file);

        let mut reg = SessionRegistry::new();
        reg.insert(
            old_key.clone(),
            SessionEntry {
                pane: "%4".to_string(),
                pid: 4242,
                cwd: dir.path().display().to_string(),
                started: "2026-07-08T02:33:00Z".to_string(),
                session_id: "old-session".to_string(),
                file: old_file.clone(),
                window: "@3".to_string(),
                supervisor_instance_id: "old-supervisor".to_string(),
            },
        );
        session_registry_io::save_in(dir.path(), &reg).unwrap();

        let mut conn = state_store::open_state_db(dir.path()).unwrap();
        let record = ActorRecord {
            document_id: new_key.clone(),
            session_id: "new-session".to_string(),
            generation: 7,
            pane_id: "%19".to_string(),
            window_id: "@3".to_string(),
            harness: "codex".to_string(),
            state: ActorState::Ready,
            last_transition: ActorLastTransition {
                caller: "test".to_string(),
                reason: "seed".to_string(),
                timestamp: 1,
                prior_generation: 6,
                new_generation: 7,
            },
        };
        state_store::store_actor_record_tx(&mut conn, None, &record, None, None).unwrap();

        register_start_supervisor_in(
            dir.path(),
            "new-session",
            "%4",
            &new_file,
            4343,
            "@3",
            &dir.path().display().to_string(),
            "new-supervisor",
        )
        .unwrap();

        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        assert!(
            !loaded.contains_key(&old_key),
            "stale pane binding must be persisted before controller projection"
        );
        let entry = loaded
            .get(&new_key)
            .expect("controller projection should keep the existing new-document row");
        assert_eq!(entry.session_id, "new-session");
    }

    #[test]
    fn register_full_deduplicates_pane() {
        // When a new session claims a pane, old sessions pointing to the same pane are removed
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        // Seed registry with two sessions pointing to the same pane
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-a".to_string(),
                file: "old-file.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                session_id: "session-b".to_string(),
                file: "another-old.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        session_registry_io::save_in(dir.path(), &reg).unwrap();

        // Now register session-c with the same pane %42
        register_full_in(dir.path(), "session-c", "%42", "new-file.md", 200, "@1").unwrap();

        let loaded = session_registry_io::load_in(dir.path()).unwrap();
        let new_key = session_registry::canonical_registry_key_in(dir.path(), "new-file.md");
        let old_key_a = session_registry::canonical_registry_key_in(dir.path(), "old-file.md");
        let old_key_b = session_registry::canonical_registry_key_in(dir.path(), "another-old.md");
        // Only session-c should remain for pane %42
        assert!(loaded.contains_key(&new_key), "new session should exist");
        assert!(
            !loaded.contains_key(&old_key_a),
            "old session-a should be removed"
        );
        assert!(
            !loaded.contains_key(&old_key_b),
            "old session-b should be removed"
        );
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&new_key].file, "new-file.md");
    }

    #[test]
    fn register_full_logs_session_rebind_before_overwriting_pane() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("tasks/rebind.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# rebind\n").unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/logs/session-rebind.log"),
            "[1] session_start file=tasks/rebind.md pane=%42 session=session-rebind generation=1\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-rebind".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: dir.path().display().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-rebind".to_string(),
                file: "tasks/rebind.md".to_string(),
                window: "@7".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        session_registry_io::save_in(dir.path(), &reg).unwrap();

        register_full_in(
            dir.path(),
            "session-rebind",
            "%84",
            "tasks/rebind.md",
            200,
            "@9",
        )
        .unwrap();

        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/session-rebind.log")).unwrap();
        assert!(log.contains(
            "ownership_transition caller=test reason=test_register prior_generation=1 new_generation=2 old_pane=%42 new_pane=%84 old_window=@7 new_window=@9"
        ));
        assert!(
            log.contains(
                "session_superseded old_pane=%42 new_pane=%84 old_window=@7 new_window=@9 prior_generation=1 new_generation=2"
            )
        );
        assert!(log.contains(
            "session_end origin=registry_rebind pane=%42 next_pane=%84 generation=1 next_generation=2"
        ));
    }
}
