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
//! - `run(old_path, new_path)` publishes a retained path-transition observation
//!   to the Project Controller. The short-lived CLI never opens state SQLite.
//! - The old path may no longer exist on disk (rename already happened). In
//!   that case, its normalized absolute path is retained in the observation.
//! - Limitation: if the old path contained symlinks, the computed hash may not
//!   match the original because `canonicalize` resolves symlinks but our
//!   fallback does not.
//!
//! ## Agentic Contracts
//!
//! - Filesystem sidecars are write-only crash evidence and are never inputs.
//! - Existing destination ledger state is merged in stable ledger order.
//! - The Project Controller is the only durable SQLite writer.
//! - Registry, actor, live relay, and ACK projections converge as one retained
//!   controller transition.

use anyhow::{Context, Result};
use std::path::Path;

/// Migrate session state after a document rename.
pub fn run(old_path: &Path, new_path: &Path) -> Result<()> {
    if !new_path.exists() {
        anyhow::bail!("new path does not exist: {}", new_path.display());
    }

    let normalized_old = if old_path.exists() {
        old_path.canonicalize()?
    } else if old_path.is_absolute() {
        old_path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to get current directory")?
            .join(old_path)
    };
    let canonical_new = new_path.canonicalize()?;
    if normalized_old == canonical_new {
        eprintln!("[rename] hashes match — nothing to migrate");
        return Ok(());
    }

    let project_root = agent_doc_fs::find_project_root(&canonical_new)
        .context("no .agent-doc/ directory found above new path")?;
    let observation =
        agent_doc_controller_io::project_controller::new_document_path_transition_observation(
            &normalized_old,
            &canonical_new,
            &format!("cli:{}", std::process::id()),
        );
    let receipt = agent_doc_controller_io::project_controller::observe_document_path_transition(
        &project_root,
        &observation,
    )?;
    anyhow::ensure!(
        receipt.converged,
        "document path transition remains {:?}: {}",
        receipt.phase,
        receipt.error.as_deref().unwrap_or("retry pending")
    );

    eprintln!(
        "[rename] converged transition={} attempt={} state_events_rekeyed={} actor_rekeyed={} sessions_rekeyed={} relay_hub_moved={}: {} → {}",
        receipt.transition_id,
        receipt.attempt,
        receipt.state_events_rekeyed,
        receipt.actor_rekeyed,
        receipt.sessions_rekeyed,
        receipt.relay_hub_moved,
        old_path.display(),
        new_path.display()
    );
    Ok(())
}
