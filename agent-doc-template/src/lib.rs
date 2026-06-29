//! Pure template-mode patch parsing, repair, and component mutation policy.
//!
//! This crate owns template document policy directly. File-backed config and
//! document IO stay in orchestration adapters.

pub mod id {
    pub use agent_doc_element::id::*;
}

mod template;

pub mod patchback;
pub mod replay_guard;
pub mod response_materialization;
pub mod sanitize;

pub use template::*;
