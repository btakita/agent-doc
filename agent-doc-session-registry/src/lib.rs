//! Pure session registry mutation and lookup policy.
//!
//! This crate owns key selection, session-id lookup, stale pane-binding
//! removal, and `tmux_router::RegistryEntry` construction. It performs no file
//! IO, tmux queries, controller projection, or log writes.

use std::path::Path;

use tmux_router::registry::{
    canonical_registry_key_in as tmux_canonical_registry_key_in, entry_session_id,
};
use tmux_router::{Registry, RegistryEntry};

#[derive(Debug, Clone, Copy)]
pub struct RegistryEntryFields<'a> {
    pub session_id: &'a str,
    pub pane_id: &'a str,
    pub file: &'a str,
    pub pid: u32,
    pub cwd: &'a str,
    pub started: &'a str,
    pub window: &'a str,
    pub supervisor_instance_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryReplacement {
    pub registry_key: String,
    pub stale_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentityOwner {
    pub registry_key: String,
    pub file: String,
    pub pane: String,
    pub started: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdentityClaim {
    Unclaimed,
    OwnedByDocument(SessionIdentityOwner),
    Conflicting(SessionIdentityOwner),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionIdentityObservation {
    pub sequence: u64,
    pub document_hash: String,
    pub file: String,
    pub session_id: String,
}

pub fn canonical_registry_key_in(base_dir: &Path, file: &str) -> String {
    tmux_canonical_registry_key_in(base_dir, file)
}

pub fn session_key(registry: &Registry, session_id: &str) -> Option<String> {
    session_identity_owner(registry, session_id).map(|owner| owner.registry_key)
}

pub fn session_identity_owner(
    registry: &Registry,
    session_id: &str,
) -> Option<SessionIdentityOwner> {
    registry
        .iter()
        .filter(|(key, entry)| entry_session_id(key, entry) == session_id)
        .min_by(|(left_key, left), (right_key, right)| {
            left.started
                .cmp(&right.started)
                .then_with(|| left_key.cmp(right_key))
        })
        .map(|(registry_key, entry)| SessionIdentityOwner {
            registry_key: registry_key.clone(),
            file: entry.file.clone(),
            pane: entry.pane.clone(),
            started: entry.started.clone(),
        })
}

pub fn session_identity_claim(
    base_dir: &Path,
    registry: &Registry,
    session_id: &str,
    file: &str,
) -> SessionIdentityClaim {
    let Some(owner) = session_identity_owner(registry, session_id) else {
        return SessionIdentityClaim::Unclaimed;
    };
    let document_key = canonical_registry_key_in(base_dir, file);
    let owner_file_key = if owner.file.is_empty() {
        owner.registry_key.clone()
    } else {
        canonical_registry_key_in(base_dir, &owner.file)
    };
    if owner.registry_key == document_key || owner_file_key == document_key {
        SessionIdentityClaim::OwnedByDocument(owner)
    } else {
        SessionIdentityClaim::Conflicting(owner)
    }
}

/// Project session identity ownership from immutable controller event order.
///
/// `None` means this identity predates the typed event stream and callers may
/// use the registry only as a one-time compatibility bootstrap. Once any
/// observation exists, registry pruning and supervisor recycling are irrelevant.
pub fn durable_session_identity_claim(
    base_dir: &Path,
    observations: &[SessionIdentityObservation],
    session_id: &str,
    file: &str,
) -> Option<SessionIdentityClaim> {
    let owner = observations
        .iter()
        .filter(|observation| observation.session_id == session_id)
        .min_by(|left, right| {
            left.sequence
                .cmp(&right.sequence)
                .then_with(|| left.file.cmp(&right.file))
        })?;
    let owner = SessionIdentityOwner {
        registry_key: canonical_registry_key_in(base_dir, &owner.file),
        file: owner.file.clone(),
        pane: String::new(),
        started: format!("state-event:{}", owner.sequence),
    };
    let document_key = canonical_registry_key_in(base_dir, file);
    if owner.registry_key == document_key {
        Some(SessionIdentityClaim::OwnedByDocument(owner))
    } else {
        Some(SessionIdentityClaim::Conflicting(owner))
    }
}

pub fn session_pane(registry: &Registry, session_id: &str) -> Option<String> {
    session_key(registry, session_id)
        .and_then(|key| registry.get(&key).map(|entry| entry.pane.clone()))
}

pub fn session_entry(registry: &Registry, session_id: &str) -> Option<RegistryEntry> {
    session_key(registry, session_id).and_then(|key| registry.get(&key).cloned())
}

pub fn remove_session_by_id(registry: &mut Registry, session_id: &str) -> bool {
    session_key(registry, session_id)
        .and_then(|key| registry.remove(&key))
        .is_some()
}

pub fn stale_pane_keys(registry: &Registry, pane_id: &str, session_id: &str) -> Vec<String> {
    registry
        .iter()
        .filter(|(key, entry)| entry.pane == pane_id && entry_session_id(key, entry) != session_id)
        .map(|(key, _)| key.clone())
        .collect()
}

pub fn remove_stale_pane_bindings(
    registry: &mut Registry,
    pane_id: &str,
    session_id: &str,
) -> Vec<String> {
    let stale_keys = stale_pane_keys(registry, pane_id, session_id);
    for key in &stale_keys {
        registry.remove(key);
    }
    stale_keys
}

pub fn registry_entry(fields: RegistryEntryFields<'_>) -> RegistryEntry {
    RegistryEntry {
        pane: fields.pane_id.to_string(),
        pid: fields.pid,
        cwd: fields.cwd.to_string(),
        started: fields.started.to_string(),
        session_id: fields.session_id.to_string(),
        file: fields.file.to_string(),
        window: fields.window.to_string(),
        supervisor_instance_id: fields.supervisor_instance_id.to_string(),
    }
}

pub fn insert_registry_entry(
    base_dir: &Path,
    registry: &mut Registry,
    fields: RegistryEntryFields<'_>,
) -> String {
    let registry_key = canonical_registry_key_in(base_dir, fields.file);
    registry.insert(registry_key.clone(), registry_entry(fields));
    registry_key
}

pub fn replace_registry_entry(
    base_dir: &Path,
    registry: &mut Registry,
    fields: RegistryEntryFields<'_>,
) -> RegistryReplacement {
    let stale_keys = remove_stale_pane_bindings(registry, fields.pane_id, fields.session_id);
    let registry_key = insert_registry_entry(base_dir, registry, fields);
    RegistryReplacement {
        registry_key,
        stale_keys,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(session_id: &str, pane_id: &str, file: &str) -> RegistryEntry {
        registry_entry(RegistryEntryFields {
            session_id,
            pane_id,
            file,
            pid: 42,
            cwd: "/tmp",
            started: "2026-01-01T00:00:00Z",
            window: "@1",
            supervisor_instance_id: "",
        })
    }

    #[test]
    fn session_lookup_uses_entry_session_id_fallback() {
        let mut registry = Registry::new();
        registry.insert("legacy-key".to_string(), entry("session-a", "%1", "a.md"));

        assert_eq!(
            session_key(&registry, "session-a").as_deref(),
            Some("legacy-key")
        );
        assert_eq!(session_pane(&registry, "session-a").as_deref(), Some("%1"));
        assert_eq!(
            session_entry(&registry, "session-a")
                .as_ref()
                .map(|entry| entry.file.as_str()),
            Some("a.md")
        );
    }

    #[test]
    fn session_identity_owner_is_first_durable_registration_not_map_order() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut registry = Registry::new();
        let mut later = entry("copied-session", "%9", "z-copy.md");
        later.started = "2026-08-02T01:30:02Z".to_string();
        registry.insert(canonical_registry_key_in(dir.path(), "z-copy.md"), later);
        let mut original = entry("copied-session", "%8", "a-original.md");
        original.started = "2026-08-02T01:29:54Z".to_string();
        registry.insert(
            canonical_registry_key_in(dir.path(), "a-original.md"),
            original,
        );

        let owner = session_identity_owner(&registry, "copied-session").unwrap();

        assert_eq!(owner.file, "a-original.md");
        assert_eq!(owner.pane, "%8");
        assert_eq!(
            session_key(&registry, "copied-session").as_deref(),
            Some(owner.registry_key.as_str())
        );
    }

    #[test]
    fn session_identity_claim_distinguishes_owner_from_copied_document() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert(
            canonical_registry_key_in(dir.path(), "original.md"),
            entry("copied-session", "%8", "original.md"),
        );

        assert!(matches!(
            session_identity_claim(dir.path(), &registry, "copied-session", "original.md"),
            SessionIdentityClaim::OwnedByDocument(_)
        ));
        assert!(matches!(
            session_identity_claim(dir.path(), &registry, "copied-session", "copy.md"),
            SessionIdentityClaim::Conflicting(SessionIdentityOwner { file, .. })
                if file == "original.md"
        ));
        assert_eq!(
            session_identity_claim(dir.path(), &registry, "new-session", "copy.md"),
            SessionIdentityClaim::Unclaimed
        );
    }

    #[test]
    fn durable_session_identity_claim_survives_registry_recycling() {
        let dir = tempfile::TempDir::new().unwrap();
        let observations = vec![
            SessionIdentityObservation {
                sequence: 41,
                document_hash: "owner-hash".to_string(),
                file: "original.md".to_string(),
                session_id: "copied-session".to_string(),
            },
            SessionIdentityObservation {
                sequence: 42,
                document_hash: "copy-hash".to_string(),
                file: "copy.md".to_string(),
                session_id: "copied-session".to_string(),
            },
        ];

        assert!(matches!(
            durable_session_identity_claim(
                dir.path(),
                &observations,
                "copied-session",
                "original.md"
            ),
            Some(SessionIdentityClaim::OwnedByDocument(_))
        ));
        assert!(matches!(
            durable_session_identity_claim(
                dir.path(),
                &observations,
                "copied-session",
                "copy.md"
            ),
            Some(SessionIdentityClaim::Conflicting(SessionIdentityOwner {
                file,
                started,
                ..
            })) if file == "original.md" && started == "state-event:41"
        ));
        assert_eq!(
            durable_session_identity_claim(
                dir.path(),
                &observations,
                "unobserved-session",
                "copy.md"
            ),
            None
        );
    }

    #[test]
    fn stale_pane_removal_keeps_current_session_binding() {
        let mut registry = Registry::new();
        registry.insert("current".to_string(), entry("session-a", "%1", "a.md"));
        registry.insert("stale".to_string(), entry("session-b", "%1", "b.md"));
        registry.insert("other-pane".to_string(), entry("session-c", "%2", "c.md"));

        let removed = remove_stale_pane_bindings(&mut registry, "%1", "session-a");

        assert_eq!(removed, vec!["stale".to_string()]);
        assert!(registry.contains_key("current"));
        assert!(!registry.contains_key("stale"));
        assert!(registry.contains_key("other-pane"));
    }

    #[test]
    fn replace_registry_entry_removes_stale_bindings_and_inserts_canonical_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut registry = Registry::new();
        registry.insert("stale".to_string(), entry("old-session", "%9", "old.md"));

        let replacement = replace_registry_entry(
            dir.path(),
            &mut registry,
            RegistryEntryFields {
                session_id: "new-session",
                pane_id: "%9",
                file: "doc.md",
                pid: 99,
                cwd: "/work",
                started: "2026-01-01T00:00:00Z",
                window: "@2",
                supervisor_instance_id: "sup-1",
            },
        );

        assert_eq!(replacement.stale_keys, vec!["stale".to_string()]);
        assert!(!registry.contains_key("stale"));
        let inserted = registry.get(&replacement.registry_key).unwrap();
        assert_eq!(inserted.session_id, "new-session");
        assert_eq!(inserted.pane, "%9");
        assert_eq!(inserted.supervisor_instance_id, "sup-1");
    }
}
