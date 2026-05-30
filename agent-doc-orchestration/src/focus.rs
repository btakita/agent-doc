//! # Module: focus
//!
//! Focus the tmux pane associated with a session document.
//!
//! Usage: `agent-doc focus <file.md>`
//!
//! ## Spec
//! - `run(file, pane)`: entry point; delegates to `run_with_tmux` using the default
//!   tmux server.
//! - `run_with_tmux(file, pane_override, tmux)`: if `pane_override` is `Some`, skips
//!   frontmatter lookup and calls `tmux select-pane` on the supplied pane directly;
//!   errors if the override pane is not alive.
//! - When `pane_override` is `None`, reads the file from disk, parses YAML frontmatter,
//!   and extracts the `agent_doc_session` UUID; errors if the field is absent.
//! - When `.agent-doc/session-actors.json` has a live local actor projection for the
//!   document session, focus prefers that actor-owned pane over a stale
//!   `sessions.json` projection without launching or waiting on the project controller.
//! - Otherwise, looks up the UUID in `sessions.json` via `sessions::lookup`.
//! - Live-owner precedence: a pane resolved from the local actor projection or the
//!   registry is only proof the pane is *alive*, not that it still owns the document.
//!   After a reroute / fresh-restart the session can move to a new pane while the old
//!   pane stays alive with a dead owner. Before selecting, focus reconciles the
//!   candidate against `sync::find_live_owner_pane_quiet`; when a different pane
//!   provably owns the document right now, focus swaps to that live owner and lets
//!   resync repair the registry. This also recovers a dead-registered or
//!   unregistered document whose session is still running in another pane, instead
//!   of failing closed.
//! - On success, calls `tmux select-pane` and logs the focused pane + file path to stderr.
//!
//! ## Agentic Contracts
//! - `run_with_tmux` never modifies `sessions.json` or the document on disk.
//! - A file without `agent_doc_session` in its frontmatter always returns an error with
//!   a message directing the caller to run `claim` first.
//! - A registered pane that is no longer alive returns an error; the caller is responsible
//!   for pruning or re-claiming.
//! - `pane_override` is an escape hatch for callers that already know the pane ID (e.g.
//!   `layout.rs` focusing a resolved pane); it bypasses all registry and frontmatter I/O.
//!
//! ## Evals
//! - `focus_live_pane` (aspirational): file has a valid session UUID and a live pane →
//!   `select-pane` is called and `Ok(())` is returned.
//! - `focus_prefers_local_actor_projection` (aspirational): stale registry pane +
//!   live local actor projection → focus selects the actor-owned pane without an RPC.
//! - `focus_dead_pane` (aspirational): session UUID exists in registry, pane is dead, and
//!   no live owner is provable → error containing "pane … is dead" is returned.
//! - `focus_repairs_stale_registry_to_live_owner` (aspirational): registered pane is alive
//!   but its owner is gone while the document runs in another pane → focus selects the live
//!   owner pane, not the stale registry pane.
//! - `focus_no_session` (aspirational): file frontmatter has no `agent_doc_session` →
//!   error directing caller to run `claim` is returned.
//! - `focus_file_not_found` (aspirational): file path does not exist on disk →
//!   error containing "file not found" is returned.
//! - `focus_pane_override_live` (aspirational): `pane_override` supplied and pane is live →
//!   registry is never read and `select-pane` is called on the override pane.
//! - `focus_pane_override_dead` (aspirational): `pane_override` supplied but pane is dead →
//!   error containing "pane … is dead" is returned without reading frontmatter.

use anyhow::{Context, Result};
use std::path::Path;

use crate::sessions::Tmux;
use crate::{frontmatter, sessions};

/// Outcome of reconciling the pane `focus` was about to select against the
/// document's provably-live owner pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusPaneDecision {
    /// Keep the candidate pane resolved from the local actor projection or the
    /// `sessions.json` registry — it is (or is assumed to be) the live owner.
    UseCandidate,
    /// The candidate pane is stale (its owner process is gone) but the document
    /// is still served by a live agent-doc owner in a different pane — focus
    /// that owner pane instead and let resync repair the registry.
    RepairToLiveOwner(String),
}

/// Pure decision: given the pane `focus` resolved from projection/registry and
/// the document's provably-live owner pane (if one was found), decide whether
/// to keep the candidate or swap to the live owner.
///
/// The live owner wins only when it is a different, non-empty pane. When no
/// live owner is provable, or it matches the candidate, the existing selection
/// is preserved so the happy path is unchanged.
pub fn decide_focus_pane(candidate: &str, live_owner: Option<&str>) -> FocusPaneDecision {
    match live_owner {
        Some(owner) if !owner.is_empty() && owner != candidate => {
            FocusPaneDecision::RepairToLiveOwner(owner.to_string())
        }
        _ => FocusPaneDecision::UseCandidate,
    }
}

/// Resolve the document's live owner pane, but only return it when it is alive
/// and differs from `candidate`. This is the stale-projection / stale-registry
/// repair used by [`run_with_tmux`]: after a reroute or fresh-restart moves the
/// session to a new pane, the registry/projection can still point at the old
/// pane (alive as a pane but with a dead owner), so focus must defer to where
/// the session actually lives.
fn live_owner_override(
    file: &Path,
    session_id: &str,
    candidate: &str,
    tmux: &Tmux,
) -> Option<String> {
    let owner = crate::sync::find_live_owner_pane_quiet(tmux, file, session_id)?;
    match decide_focus_pane(candidate, Some(owner.as_str())) {
        FocusPaneDecision::RepairToLiveOwner(owner) if tmux.pane_alive(&owner) => Some(owner),
        _ => None,
    }
}

pub fn local_actor_projection_pane_for_document(
    file: &Path,
    session_id: &str,
    tmux: &Tmux,
) -> Option<String> {
    let canonical = file
        .canonicalize()
        .ok()
        .unwrap_or_else(|| file.to_path_buf());
    let base_dir = crate::snapshot::find_project_root(&canonical)?;
    let record = crate::session_actor::load_record_in(&base_dir, &canonical.to_string_lossy())
        .ok()
        .flatten()?;
    if record.session_id != session_id
        || matches!(
            record.state,
            crate::session_actor::ActorState::Closed | crate::session_actor::ActorState::Blocked
        )
        || !tmux.pane_alive(&record.pane_id)
    {
        return None;
    }
    Some(record.pane_id)
}

pub fn run(file: &Path, pane: Option<&str>) -> Result<()> {
    run_with_tmux(file, pane, &Tmux::default_server())
}

pub fn run_with_tmux(file: &Path, pane_override: Option<&str>, tmux: &Tmux) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // If an explicit pane was provided, use it directly
    if let Some(p) = pane_override {
        if tmux.pane_alive(p) {
            tmux.select_pane(p)?;
            eprintln!("Focused pane {} ({})", p, file.display());
            return Ok(());
        } else {
            anyhow::bail!("pane {} is dead for {}", p, file.display());
        }
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;
    let session_id = match fm.session {
        Some(id) => id,
        None => anyhow::bail!(
            "no session UUID in {} (use Claim to register)",
            file.display()
        ),
    };

    if let Some(actor_pane) = local_actor_projection_pane_for_document(file, &session_id, tmux) {
        // The local actor projection only proves the pane is *alive*, not that
        // it still owns this document. After a reroute / fresh-restart the
        // session may have moved; defer to the live owner when one exists.
        if let Some(owner) = live_owner_override(file, &session_id, &actor_pane, tmux) {
            tmux.select_pane(&owner)?;
            eprintln!(
                "Focused live-owner pane {} (stale actor projection {}) ({})",
                owner,
                actor_pane,
                file.display()
            );
            return Ok(());
        }
        tmux.select_pane(&actor_pane)?;
        eprintln!("Focused pane {} ({})", actor_pane, file.display());
        return Ok(());
    }

    let pane = sessions::lookup(&session_id)?;
    match pane {
        Some(pane_id) if tmux.pane_alive(&pane_id) => {
            // A registered pane that is alive as a *pane* is not proof it still
            // owns the document — its owner process may be gone while the live
            // session runs in another pane. Prefer the provable live owner.
            if let Some(owner) = live_owner_override(file, &session_id, &pane_id, tmux) {
                tmux.select_pane(&owner)?;
                eprintln!(
                    "Focused live-owner pane {} (stale registry pane {}) ({})",
                    owner,
                    pane_id,
                    file.display()
                );
                return Ok(());
            }
            tmux.select_pane(&pane_id)?;
            eprintln!("Focused pane {} ({})", pane_id, file.display());
            Ok(())
        }
        Some(pane_id) => {
            // The registered pane is dead. Before failing, recover the live
            // owner if the session is still running in a different pane.
            if let Some(owner) = crate::sync::find_live_owner_pane_quiet(tmux, file, &session_id)
                .filter(|owner| tmux.pane_alive(owner))
            {
                tmux.select_pane(&owner)?;
                eprintln!(
                    "Focused live-owner pane {} (registered pane {} is dead) ({})",
                    owner,
                    pane_id,
                    file.display()
                );
                return Ok(());
            }
            anyhow::bail!("pane {} is dead for {}", pane_id, file.display());
        }
        None => {
            // No registry entry, but the session may still be running with a
            // lost registration — recover the live owner pane if provable.
            if let Some(owner) = crate::sync::find_live_owner_pane_quiet(tmux, file, &session_id)
                .filter(|owner| tmux.pane_alive(owner))
            {
                tmux.select_pane(&owner)?;
                eprintln!(
                    "Focused live-owner pane {} (no registry entry) ({})",
                    owner,
                    file.display()
                );
                return Ok(());
            }
            anyhow::bail!(
                "no pane registered for {} (session {})",
                file.display(),
                &session_id[..std::cmp::min(8, session_id.len())]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{self, IsolatedTmux};
    use std::path::{Path, PathBuf};

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

    #[test]
    fn decide_focus_pane_keeps_candidate_when_no_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", None),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn decide_focus_pane_keeps_candidate_when_owner_matches() {
        assert_eq!(
            decide_focus_pane("%8", Some("%8")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn decide_focus_pane_repairs_to_live_owner_when_candidate_is_stale() {
        // The bug: registry/projection points at stale %36 while the live owner
        // runs in %8 — focus must swap to %8.
        assert_eq!(
            decide_focus_pane("%36", Some("%8")),
            FocusPaneDecision::RepairToLiveOwner("%8".to_string())
        );
    }

    #[test]
    fn decide_focus_pane_ignores_empty_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", Some("")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn focus_prefers_local_actor_projection_over_stale_registry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/focus-actor.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: focus-actor\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("focus-authoritative-actor");
        let stale_pane = iso.new_session("test", root).unwrap();
        let actor_pane = iso.split_window(&stale_pane, root, "-dh").unwrap();
        let actor_window = iso.pane_window(&actor_pane).unwrap();

        sessions::register_full_with_cwd(
            "focus-actor",
            &stale_pane,
            &doc.to_string_lossy(),
            1,
            &iso.pane_window(&stale_pane).unwrap(),
            &root.to_string_lossy(),
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "focus-actor",
            &actor_pane,
            &actor_window,
            "sync",
            "focus_test_projection",
        )
        .unwrap();

        run_with_tmux(&doc, None, &iso).unwrap();

        let selected = iso
            .raw_cmd(&["display-message", "-p", "#{pane_id}"])
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(
            selected, actor_pane,
            "focus should select the locally projected actor pane instead of the stale registry pane"
        );
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn focus_ignores_closed_local_actor_projection() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let _cwd = ScopedCurrentDir::set(root);
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();

        let doc = root.join("tasks/focus-closed-actor.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: focus-closed\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

        let iso = IsolatedTmux::new("focus-closed-local-actor");
        let registry_pane = iso.new_session("test", root).unwrap();
        let actor_pane = iso.split_window(&registry_pane, root, "-dh").unwrap();
        let registry_window = iso.pane_window(&registry_pane).unwrap();
        let actor_window = iso.pane_window(&actor_pane).unwrap();

        sessions::register_full_with_cwd(
            "focus-closed",
            &registry_pane,
            &doc.to_string_lossy(),
            1,
            &registry_window,
            &root.to_string_lossy(),
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            root,
            &doc.to_string_lossy(),
            "focus-closed",
            &actor_pane,
            &actor_window,
            "sync",
            "focus_test_projection",
        )
        .unwrap();
        crate::session_actor::transition_state_direct(
            &doc,
            "focus-closed",
            &actor_pane,
            None,
            crate::session_actor::ActorState::Closed,
            "test",
            "closed_projection",
        )
        .unwrap();

        run_with_tmux(&doc, None, &iso).unwrap();

        let selected = iso
            .raw_cmd(&["display-message", "-p", "#{pane_id}"])
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(
            selected, registry_pane,
            "focus should fall back to sessions.json when the local actor projection is closed"
        );
    }
}
