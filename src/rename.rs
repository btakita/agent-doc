//! # Module: rename
//!
//! Migrate session state after a document file rename/move.
//!
//! When a document is renamed, its typed state ledger is rekeyed because the
//! document identity is derived from the canonical path. Crash sidecars remain
//! immutable evidence under their crash-time identity and are never read or
//! migrated by this command.
//!
//! ## Spec
//!
//! - `run(old_path, new_path)` transactionally rekeys typed state events from
//!   the old path hash to the new path hash. It then updates registry entries
//!   whose `file` field matches the old path.
//! - The old path may no longer exist on disk (rename already happened). In
//!   that case, `agent_doc_fs::document_state_hash_from_str` is used with the
//!   absolute path string instead of `agent_doc_fs::document_state_hash` (which
//!   requires `canonicalize`).
//! - Limitation: if the old path contained symlinks, the computed hash may not
//!   match the original because `canonicalize` resolves symlinks but our
//!   fallback does not.
//!
//! ## Agentic Contracts
//!
//! - Filesystem sidecars are write-only crash evidence and are never inputs.
//! - Existing destination ledger state causes an error (prevents split history).
//! - Registry updates are performed under `RegistryLock`.

use anyhow::{Context, Result};
use std::path::Path;

/// Migrate session state after a document rename.
pub fn run(old_path: &Path, new_path: &Path) -> Result<()> {
    // new_path must exist
    if !new_path.exists() {
        anyhow::bail!("new path does not exist: {}", new_path.display());
    }

    // Compute old hash
    let old_hash = if old_path.exists() {
        agent_doc_fs::document_state_hash(old_path)?
    } else {
        // Old path no longer exists — resolve to absolute without canonicalize
        let abs = if old_path.is_absolute() {
            old_path.to_string_lossy().to_string()
        } else {
            let cwd = std::env::current_dir().context("failed to get current directory")?;
            cwd.join(old_path).to_string_lossy().to_string()
        };
        agent_doc_fs::document_state_hash_from_str(&abs)
    };

    // Compute new hash (file exists, canonicalize works)
    let new_hash = agent_doc_fs::document_state_hash(new_path)?;

    if old_hash == new_hash {
        eprintln!("[rename] hashes match — nothing to migrate");
        return Ok(());
    }

    // Find project root from the new path
    let canonical_new = new_path.canonicalize()?;
    let project_root = agent_doc_fs::find_project_root(&canonical_new)
        .context("no .agent-doc/ directory found above new path")?;

    let conn = agent_doc_sqlite::state_store::open_state_db(&project_root)?;
    let state_report =
        agent_doc_sqlite::state_store::rekey_document_state_in_db(&conn, &old_hash, &new_hash)?;

    // Update sessions registry
    let old_path_str = old_path.to_string_lossy().to_string();
    let new_path_str = new_path.to_string_lossy().to_string();
    let old_key = tmux_router::registry::canonical_registry_key_in(&project_root, &old_path_str);
    let new_key = tmux_router::registry::canonical_registry_key_in(&project_root, &new_path_str);

    let registry_path = agent_doc_session_registry_io::registry_path_in(&project_root);
    let _lock = tmux_router::RegistryLock::acquire(&registry_path)?;
    let mut registry = agent_doc_session_registry_io::load_in(&project_root)?;
    let mut updated_sessions = 0u32;
    if let Some(mut entry) = registry.remove(&old_key) {
        entry.file = new_path_str.clone();
        registry.insert(new_key, entry);
        updated_sessions += 1;
    } else {
        for entry in registry.values_mut() {
            if entry.file == old_path_str {
                entry.file = new_path_str.clone();
                updated_sessions += 1;
            }
        }
    }
    if updated_sessions > 0 {
        agent_doc_session_registry_io::save_in(&project_root, &registry)?;
    }

    eprintln!(
        "[rename] rekeyed {} typed event(s), retired {} peer acknowledgement(s), updated {} session(s): {} → {}",
        state_report.state_events_rekeyed,
        state_report.peer_acknowledgements_retired,
        updated_sessions,
        old_path.display(),
        new_path.display()
    );
    Ok(())
}
