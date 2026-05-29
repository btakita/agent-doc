//! Project config — pure types in `agent_doc_core::project_config` (Wave 2
//! of #adcr); `&Path` / `std::fs` wrappers (load_project, save_project,
//! load_project_for_doc, project_root_for_doc, load_project_from,
//! save_project_to, project_tmux_session, update_project_tmux_session,
//! clear_project_tmux_session) live in [`agent_doc_orchestration::project_config_io`]
//! (Wave 5 / `#bjrv`).
//!
//! Thin re-export shim. All `crate::project_config::*` call sites continue to resolve.

pub use agent_doc_core::project_config::*;
// The tmux-session helpers are consumed via `agent_doc_orchestration::config::*` (which now
// re-exports them from orchestration) in non-test code and via
// `crate::project_config::*` in tests, so the non-test bin build sees this
// compat re-export as unused.
#[allow(unused_imports)]
pub use agent_doc_orchestration::project_config_io::{
    clear_project_tmux_session, load_project, load_project_for_doc, load_project_from,
    project_root_for_doc, project_tmux_session, save_project, update_project_tmux_session,
};
