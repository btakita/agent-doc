//! agent-doc-core — document data layer for agent-doc.
//!
//! See `tasks/agent-doc/plan-agent-doc-core-extraction.md` for the wave plan.
//! Currently extracted: Wave 1 (id, crdt, component, model_tier, pending),
//! Wave 2 (frontmatter, project_config), Wave 3 (template). Some functions in
//! `frontmatter`, `project_config`, and `template` take `&Path` and touch the
//! filesystem — they live here for the convenience of a single move and may
//! be split into orchestration wrappers in a follow-up tidy.

pub mod component;
pub mod crdt;
pub mod frontmatter;
pub mod id;
pub mod model_tier;
pub mod pending;
pub mod project_config;
pub mod template;

pub use component::Component;
pub use crdt::CrdtDoc;
pub use id::{BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary};
pub use template::{ComponentInfo, PatchBlock, TemplateInfo};
