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
//! - Otherwise, looks up the UUID in `sessions.json` via `sessions::lookup`; errors
//!   if no entry is found or if the registered pane is dead.
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
//! - `focus_dead_pane` (aspirational): session UUID exists in registry but pane is dead →
//!   error containing "pane … is dead" is returned.
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

fn local_actor_projection_pane_for_document(
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
        tmux.select_pane(&actor_pane)?;
        eprintln!("Focused pane {} ({})", actor_pane, file.display());
        return Ok(());
    }

    let pane = sessions::lookup(&session_id)?;
    match pane {
        Some(pane_id) if tmux.pane_alive(&pane_id) => {
            tmux.select_pane(&pane_id)?;
            eprintln!("Focused pane {} ({})", pane_id, file.display());
            Ok(())
        }
        Some(pane_id) => {
            anyhow::bail!("pane {} is dead for {}", pane_id, file.display());
        }
        None => {
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
        _env_guard: std::sync::MutexGuard<'static, ()>,
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
