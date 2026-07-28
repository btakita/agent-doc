use std::path::Path;

use anyhow::Result;
use tmux_router::Tmux;

pub(crate) struct CliFocusEffects;

pub(crate) static FOCUS_EFFECTS: CliFocusEffects = CliFocusEffects;

impl agent_doc_focus_io::FocusEffects for CliFocusEffects {
    fn focus_or_resume_document_via_controller(&self, file: &Path) -> Result<()> {
        let project_root =
            agent_doc_project_root_io::project_root_for_target_or_cwd(None, Some(file))?;
        let receipt =
            agent_doc_controller_io::project_controller::focus_document_pane(&project_root, file)?;
        if !receipt.focused {
            anyhow::bail!(
                "controller could not focus {}: {}",
                file.display(),
                receipt.reason
            );
        }
        eprintln!(
            "Focused pane {} ({})",
            receipt.pane_id.as_deref().unwrap_or("<unknown>"),
            file.display()
        );
        Ok(())
    }

    fn find_live_owner_pane_quiet(
        &self,
        tmux: &Tmux,
        file: &Path,
        session_id: &str,
    ) -> Option<String> {
        agent_doc_sync_io::sync::find_live_owner_pane_quiet(tmux, file, session_id)
    }

    fn local_actor_record_pane_for_document(
        &self,
        file: &Path,
        session_id: &str,
        tmux: &Tmux,
    ) -> Option<String> {
        let canonical = file
            .canonicalize()
            .ok()
            .unwrap_or_else(|| file.to_path_buf());
        let base_dir = agent_doc_project_root_io::project_root_containing(&canonical)?;
        let record =
            agent_doc_session_actor_io::load_record_in(&base_dir, &canonical.to_string_lossy())
                .ok()
                .flatten()?;
        if record.session_id != session_id
            || matches!(
                record.state,
                agent_doc_sqlite::state_store::ActorState::Closed
                    | agent_doc_sqlite::state_store::ActorState::Blocked
            )
            || !tmux.pane_alive(&record.pane_id)
        {
            return None;
        }
        Some(record.pane_id)
    }

    fn pane_in_stash_window(&self, tmux: &Tmux, pane: &str) -> bool {
        agent_doc_sync_io::sync::pane_in_stash_window(tmux, pane)
    }

    fn promote_pane_to_agent_doc_window(&self, tmux: &Tmux, pane: &str) -> Result<bool> {
        agent_doc_sync_io::sync::promote_pane_to_agent_doc_window(tmux, pane)
    }
}
