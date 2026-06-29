//! Pure template-mode patch parsing, repair, and component mutation policy.
//!
//! This crate owns the template document policy that used to live in
//! `agent-doc-core`. File-backed config and document IO stay in orchestration
//! adapters.

pub mod id {
    pub use agent_doc_element::id::*;
}

mod template;

pub mod replay_guard;

pub use template::*;
