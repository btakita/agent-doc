//! Session registry filesystem adapters.
//!
//! Owns the canonical SQLite-backed session registry location, point-in-time
//! lookup helpers, and tmux-router registry normalization for loaded snapshots.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tmux_router::registry::canonical_registry_key_in;
use tmux_router::registry::normalize_registry;
use tmux_router::{Registry, RegistryEntry, RegistryLock};

pub use agent_doc_session_registry::{
    SessionIdentityClaim, SessionIdentityObservation, SessionIdentityOwner,
};

pub mod dispatch_registry;
pub mod registration;

pub const SESSIONS_FILE: &str = ".agent-doc/state.db";

/// Return the path to the sessions registry file relative to CWD.
pub fn registry_path() -> PathBuf {
    PathBuf::from(SESSIONS_FILE)
}

/// Return the path to the sessions registry file under `base_dir`.
pub fn registry_path_in(base_dir: &Path) -> PathBuf {
    base_dir.join(SESSIONS_FILE)
}

/// Load the session registry from durable controller state.
///
/// This is not locked internally. Callers performing read-modify-write must
/// acquire `tmux_router::RegistryLock` first.
pub fn load() -> Result<Registry> {
    load_in(&std::env::current_dir()?)
}

/// Load the session registry from `base_dir/.agent-doc/state.db`.
pub fn load_in(base_dir: &Path) -> Result<Registry> {
    let path = registry_path_in(base_dir);
    tmux_router::registry::load_registry(&path)
        .with_context(|| format!("failed to load registry from {}", path.display()))
}

/// Save the session registry to durable controller state.
///
/// This is not locked internally. Callers must hold `tmux_router::RegistryLock`
/// before saving a read-modify-write update.
pub fn save(registry: &Registry) -> Result<()> {
    save_in(&std::env::current_dir()?, registry)
}

/// Save the session registry to `base_dir/.agent-doc/state.db`.
pub fn save_in(base_dir: &Path, registry: &Registry) -> Result<()> {
    let path = registry_path_in(base_dir);
    let registry = normalize_registry(base_dir, registry.clone());
    tmux_router::registry::save_registry(&path, &registry)
        .with_context(|| format!("failed to save registry to {}", path.display()))
}

/// Look up the pane ID for a session in CWD's registry.
pub fn lookup(session_id: &str) -> Result<Option<String>> {
    let registry = load()?;
    Ok(agent_doc_session_registry::session_pane(
        &registry, session_id,
    ))
}

/// Look up the pane ID for a session in a specific base directory's registry.
pub fn lookup_in(base_dir: &Path, session_id: &str) -> Result<Option<String>> {
    let registry = load_in(base_dir)?;
    Ok(agent_doc_session_registry::session_pane(
        &registry, session_id,
    ))
}

/// Look up a full registry entry in CWD's registry.
pub fn lookup_entry(session_id: &str) -> Result<Option<RegistryEntry>> {
    let registry = load()?;
    Ok(agent_doc_session_registry::session_entry(
        &registry, session_id,
    ))
}

/// Look up a full registry entry in a specific base directory.
pub fn lookup_entry_in(base_dir: &Path, session_id: &str) -> Result<Option<RegistryEntry>> {
    let registry = load_in(base_dir)?;
    Ok(agent_doc_session_registry::session_entry(
        &registry, session_id,
    ))
}

/// Look up the registry entry bound to `file` under `base_dir`.
pub fn lookup_file_entry_in(base_dir: &Path, file: &Path) -> Result<Option<RegistryEntry>> {
    let registry = load_in(base_dir)?;
    let registry_key = canonical_registry_key_in(base_dir, &file.display().to_string());
    Ok(registry.get(&registry_key).cloned())
}

/// Project the durable ownership claim for a document-carried session identity.
///
/// A copied markdown document can carry another document's session UUID. The
/// first durable registration owns that identity; later documents must receive
/// a fresh identity before any pane, actor, or supervisor lookup is attempted.
pub fn session_identity_claim_in(
    base_dir: &Path,
    session_id: &str,
    file: &Path,
) -> Result<agent_doc_session_registry::SessionIdentityClaim> {
    if let Some(claim) = durable_session_identity_claim_in(base_dir, session_id, file)? {
        return Ok(claim);
    }
    let registry = load_in(base_dir)?;
    Ok(agent_doc_session_registry::session_identity_claim(
        base_dir,
        &registry,
        session_id,
        &file.display().to_string(),
    ))
}

/// Project session identity ownership from the immutable typed event stream.
///
/// `None` is the compatibility state for identities first registered before
/// `document_session_identity_observed` existed.
pub fn durable_session_identity_claim_in(
    base_dir: &Path,
    session_id: &str,
    file: &Path,
) -> Result<Option<SessionIdentityClaim>> {
    let conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        base_dir,
        Duration::from_secs(2),
    )?;
    let events = agent_doc_sqlite::state_store::load_state_events_by_fact_type_from_db(
        &conn,
        "document_session_identity_observed",
    )?;
    let mut observations = Vec::with_capacity(events.len());
    for status in events {
        let event: agent_doc_state_backbone::StateEvent =
            serde_json::from_str(&status.payload_json).with_context(|| {
                format!(
                    "invalid document-session identity event {} at sequence {}",
                    status.event_id, status.sequence
                )
            })?;
        if let agent_doc_state_backbone::StateFact::DocumentSessionIdentityObserved {
            document_hash,
            canonical_path,
            session_id,
        } = event.fact
        {
            observations.push(SessionIdentityObservation {
                sequence: status.sequence,
                document_hash,
                file: canonical_path,
                session_id,
            });
        }
    }
    Ok(agent_doc_session_registry::durable_session_identity_claim(
        base_dir,
        &observations,
        session_id,
        &file.display().to_string(),
    ))
}

/// Remove a session from CWD's registry under the registry lock.
pub fn deregister(session_id: &str) -> Result<bool> {
    deregister_in(&std::env::current_dir()?, session_id)
}

/// Remove a session from `base_dir`'s registry under the registry lock.
pub fn deregister_in(base_dir: &Path, session_id: &str) -> Result<bool> {
    let registry_path = registry_path_in(base_dir);
    let _lock = RegistryLock::acquire(&registry_path)?;
    let mut registry = load_in(base_dir)?;
    let removed = agent_doc_session_registry::remove_session_by_id(&mut registry, session_id);
    if removed {
        save_in(base_dir, &registry)?;
    }
    Ok(removed)
}

/// Update registry entries for `session_id` after its document path changes.
///
/// Returns the number of entries rewritten. This is a focused read-modify-write
/// helper because callers must hold the same registry lock used by all other
/// registry mutations.
pub fn update_session_file_in(
    base_dir: &Path,
    session_id: &str,
    file: &Path,
    canonical_file: &Path,
) -> Result<u32> {
    let registry_path = registry_path_in(base_dir);
    if !registry_path.exists() {
        return Ok(0);
    }
    let _lock = RegistryLock::acquire(&registry_path)?;
    let mut registry = load_in(base_dir)?;
    let doc_path = file.to_string_lossy().to_string();
    let canonical_path = canonical_file.to_string_lossy().to_string();
    let destination_key = canonical_registry_key_in(base_dir, &canonical_path);
    let mut updated = 0u32;
    let matching_keys = registry
        .iter()
        .filter_map(|(key, entry)| (entry.session_id == session_id).then_some(key.clone()))
        .collect::<Vec<_>>();
    for key in matching_keys {
        let Some(mut entry) = registry.remove(&key) else {
            continue;
        };
        let changed =
            key != destination_key || (entry.file != doc_path && entry.file != canonical_path);
        entry.file = doc_path.clone();
        if let Some(existing) = registry.get(&destination_key) {
            anyhow::ensure!(
                existing.session_id == session_id,
                "cannot move session {session_id} registry entry to {canonical_path}: destination belongs to session {}",
                existing.session_id,
            );
        }
        registry.insert(destination_key.clone(), entry);
        if changed {
            updated += 1;
        }
    }
    if updated > 0 {
        save_in(base_dir, &registry)?;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tmux_router::registry::canonical_registry_key_in;

    fn entry(session_id: &str, pane: &str, file: &str) -> RegistryEntry {
        RegistryEntry {
            pane: pane.to_string(),
            pid: 12345,
            cwd: "/tmp".to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: session_id.to_string(),
            file: file.to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
    }

    #[test]
    fn load_empty_returns_empty_registry() {
        let dir = TempDir::new().unwrap();
        let registry = load_in(dir.path()).unwrap();
        assert!(registry.is_empty());
    }

    #[test]
    fn save_load_roundtrip_normalizes_file_key() {
        let dir = TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert(
            "test-session".to_string(),
            entry("test-session", "%42", "test.md"),
        );

        save_in(dir.path(), &registry).unwrap();

        let loaded = load_in(dir.path()).unwrap();
        let key = canonical_registry_key_in(dir.path(), "test.md");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&key].pane, "%42");
    }

    #[test]
    fn lookup_finds_pane_by_session_id_after_normalization() {
        let dir = TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            entry("lookup-session", "%77", "lookup.md"),
        );
        save_in(dir.path(), &registry).unwrap();

        assert_eq!(
            lookup_in(dir.path(), "lookup-session").unwrap().as_deref(),
            Some("%77")
        );
    }

    #[test]
    fn lookup_entry_in_returns_the_registered_document_path() {
        let dir = TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            entry("lookup-entry-session", "%78", "old-name.md"),
        );
        save_in(dir.path(), &registry).unwrap();

        let entry = lookup_entry_in(dir.path(), "lookup-entry-session")
            .unwrap()
            .expect("registered session entry");

        assert_eq!(entry.file, "old-name.md");
        assert_eq!(entry.pane, "%78");
    }

    #[test]
    fn lookup_file_entry_finds_canonical_file_owner() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("owned.md");
        std::fs::write(&file, "body").unwrap();
        let mut registry = Registry::new();
        registry.insert(
            file.display().to_string(),
            entry("owner-session", "%79", &file.display().to_string()),
        );
        save_in(dir.path(), &registry).unwrap();

        let entry = lookup_file_entry_in(dir.path(), &file)
            .unwrap()
            .expect("registered owner");
        assert_eq!(entry.session_id, "owner-session");
        assert_eq!(entry.pane, "%79");
    }

    #[test]
    fn update_session_file_rewrites_matching_session_entries() {
        let dir = TempDir::new().unwrap();
        let old_file = dir.path().join("old.md");
        let new_file = dir.path().join("new.md");
        std::fs::write(&old_file, "body").unwrap();
        std::fs::write(&new_file, "body").unwrap();
        let mut registry = Registry::new();
        registry.insert(old_file.display().to_string(), {
            let mut entry = entry("rename-session", "%80", &old_file.display().to_string());
            entry.window = "@12".to_string();
            entry
        });
        save_in(dir.path(), &registry).unwrap();

        let updated =
            update_session_file_in(dir.path(), "rename-session", &new_file, &new_file).unwrap();

        assert_eq!(updated, 1);
        let loaded = load_in(dir.path()).unwrap();
        let destination_key =
            canonical_registry_key_in(dir.path(), &new_file.display().to_string());
        let entry = loaded
            .get(&destination_key)
            .expect("destination registry key");
        assert_eq!(entry.file, new_file.display().to_string());
        assert_eq!(entry.pane, "%80");
        assert_eq!(entry.window, "@12");
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn deregister_removes_session_by_id_under_lock() {
        let dir = TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            entry("remove-session", "%88", "remove.md"),
        );
        save_in(dir.path(), &registry).unwrap();

        assert!(deregister_in(dir.path(), "remove-session").unwrap());
        assert!(!deregister_in(dir.path(), "remove-session").unwrap());
        assert!(load_in(dir.path()).unwrap().is_empty());
    }
}
