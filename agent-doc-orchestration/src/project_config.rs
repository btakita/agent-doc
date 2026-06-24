//! Project config shim — pure types from `agent_doc_core::project_config`,
//! `&Path` / `std::fs` wrappers from `crate::project_config_io`. Mirrors the
//! main crate's shim so moved modules resolve unchanged.

#[allow(unused_imports)]
pub use crate::project_config_io::{
    agent_doc_bug_target_document_for_doc, clear_project_tmux_session, load_project,
    load_project_for_doc, load_project_from, project_root_for_doc, project_tmux_session,
    save_project, update_project_tmux_session,
};
pub use agent_doc_core::project_config::*;
