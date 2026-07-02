use std::path::{Path, PathBuf};

use agent_doc_controller::dispatch::{
    DispatchTargetBindFacts, DispatchTargetMatchFacts, classify_dispatch_target_bind,
    classify_dispatch_target_match,
};
use anyhow::{Context, Result};
use tmux_router::registry::find_registry_key_by_session_id;

pub fn canonical_dispatch_file(path: &Path) -> PathBuf {
    let resolved = agent_doc_git_io::dirs::resolve_absolute_file_path(path);
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub fn canonical_registered_file(entry: &tmux_router::RegistryEntry) -> PathBuf {
    let path = Path::new(&entry.file);
    let resolved = if path.is_absolute() || entry.cwd.is_empty() {
        path.to_path_buf()
    } else {
        Path::new(&entry.cwd).join(path)
    };
    std::fs::canonicalize(&resolved).unwrap_or(resolved)
}

pub fn registry_base_dir_for_dispatch(file_path: &str) -> PathBuf {
    let requested = canonical_dispatch_file(Path::new(file_path));
    agent_doc_fs::find_project_root(&requested)
        .or_else(|| requested.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn load_registry_in(base_dir: &Path) -> Result<tmux_router::Registry> {
    crate::load_in(base_dir)
}

pub fn load_dispatch_registry(file_path: &str) -> Result<tmux_router::Registry> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    load_registry_in(&base_dir)
}

pub fn lookup_dispatch_registration(file_path: &str, session_id: &str) -> Result<Option<String>> {
    let registry = load_dispatch_registry(file_path)?;
    Ok(find_registry_key_by_session_id(&registry, session_id)
        .and_then(|key| registry.get(&key).map(|entry| entry.pane.clone())))
}

pub fn deregister_dispatch_registration(file_path: &str, session_id: &str) -> Result<bool> {
    let base_dir = registry_base_dir_for_dispatch(file_path);
    let registry_path = crate::registry_path_in(&base_dir);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = load_registry_in(&base_dir)?;
    let removed_key = registry.iter().find_map(|(key, entry)| {
        ((entry.session_id == session_id) || (entry.session_id.is_empty() && key == session_id))
            .then(|| key.clone())
    });
    let removed = removed_key.and_then(|key| registry.remove(&key)).is_some();
    if removed {
        crate::save_in(&base_dir, &registry)?;
    }
    Ok(removed)
}

pub fn pane_registration_matches_file(
    registry: &tmux_router::Registry,
    pane: &str,
    file_path: &str,
) -> bool {
    let requested = canonical_dispatch_file(Path::new(file_path));
    registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(|entry| canonical_registered_file(entry) == requested)
        .unwrap_or(false)
}

pub fn ensure_dispatch_target_matches_file(pane: &str, file_path: &str) -> Result<()> {
    let registry_base_dir = registry_base_dir_for_dispatch(file_path);
    let registry = load_registry_in(&registry_base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch validation from {}",
            registry_base_dir.display()
        )
    })?;
    let pane_matches_file = pane_registration_matches_file(&registry, pane, file_path);
    let requested = canonical_dispatch_file(Path::new(file_path));
    let requested_display = requested.display().to_string();
    let registered_display = registry
        .values()
        .find(|entry| entry.pane == pane)
        .map(canonical_registered_file)
        .map(|path| path.display().to_string());
    if let Some(refusal) = classify_dispatch_target_match(DispatchTargetMatchFacts {
        pane,
        pane_matches_file,
        registered_file_display: registered_display.as_deref(),
        requested_file_display: &requested_display,
    }) {
        anyhow::bail!(refusal);
    }
    Ok(())
}

pub fn ensure_dispatch_target_can_bind_file(
    base_dir: &Path,
    pane: &str,
    file_path: &str,
    registered_is_live_owner: impl FnOnce(&tmux_router::RegistryEntry, &Path) -> bool,
) -> Result<()> {
    let registry = load_registry_in(base_dir).with_context(|| {
        format!(
            "failed to load route registry before dispatch registration from {}",
            base_dir.display()
        )
    })?;
    let pane_matches_file = pane_registration_matches_file(&registry, pane, file_path);
    let requested = canonical_dispatch_file(Path::new(file_path));
    let requested_display = requested.display().to_string();
    let registered_entry = registry.values().find(|entry| entry.pane == pane);
    let registered = registered_entry.map(canonical_registered_file);
    let registered_display = registered.as_ref().map(|path| path.display().to_string());
    let registered_is_live_owner = match (registered_entry, registered.as_ref()) {
        (Some(entry), Some(registered)) => {
            !entry.session_id.is_empty() && registered_is_live_owner(entry, registered)
        }
        _ => false,
    };
    if let Some(refusal) = classify_dispatch_target_bind(DispatchTargetBindFacts {
        pane,
        pane_matches_file,
        registered_file_display: registered_display.as_deref(),
        requested_file_display: &requested_display,
        registered_is_live_owner,
    }) {
        anyhow::bail!(refusal);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_entry(
        pane: &str,
        cwd: &Path,
        file: &str,
        session_id: &str,
    ) -> tmux_router::RegistryEntry {
        tmux_router::RegistryEntry {
            pane: pane.to_string(),
            pid: std::process::id(),
            cwd: cwd.display().to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: session_id.to_string(),
            file: file.to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: "sup-1".to_string(),
        }
    }

    #[test]
    fn pane_registration_matches_file_resolves_entry_cwd() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let doc = root.join("docs/session.md");
        std::fs::write(&doc, "body").unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            registry_entry("%401", root, "docs/session.md", "session-a"),
        );

        assert!(pane_registration_matches_file(
            &registry,
            "%401",
            &doc.to_string_lossy()
        ));
    }

    #[test]
    fn dispatch_registry_lookup_and_deregister_use_project_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let doc = root.join("docs/session.md");
        std::fs::write(&doc, "body").unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            registry_entry("%401", root, "docs/session.md", "session-a"),
        );
        crate::save_in(root, &registry).unwrap();

        assert_eq!(
            lookup_dispatch_registration(&doc.to_string_lossy(), "session-a").unwrap(),
            Some("%401".to_string())
        );
        assert!(deregister_dispatch_registration(&doc.to_string_lossy(), "session-a").unwrap());
        assert_eq!(
            lookup_dispatch_registration(&doc.to_string_lossy(), "session-a").unwrap(),
            None
        );
    }

    #[test]
    fn ensure_dispatch_target_matches_file_rejects_cross_file_registration() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        let registered = root.join("docs/registered.md");
        let requested = root.join("docs/requested.md");
        std::fs::write(&registered, "one").unwrap();
        std::fs::write(&requested, "two").unwrap();
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "legacy-key".to_string(),
            registry_entry("%401", root, "docs/registered.md", "session-a"),
        );
        crate::save_in(root, &registry).unwrap();

        let err = ensure_dispatch_target_matches_file("%401", &requested.to_string_lossy())
            .expect_err("cross-file dispatch must be rejected");

        assert!(err.to_string().contains("refusing cross-file dispatch"));
    }
}
