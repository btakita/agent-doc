//! Pure template-mode patch parsing, repair, and component mutation policy.
//!
//! This crate owns template document policy directly. File-backed config and
//! document IO stay in orchestration adapters.

mod template;

pub mod patchback;
pub mod replay_guard;
pub mod response_materialization;
pub mod sanitize;
pub mod stale_baseline;
pub mod structure_guard;
pub mod todo_patch_guard;

pub use template::*;
