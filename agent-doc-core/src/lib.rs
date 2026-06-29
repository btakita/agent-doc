//! agent-doc-core — shared identifiers and small data contracts for agent-doc.
//!
//! See `tasks/agent-doc/prd-crate-decomposition.md` for the current extraction
//! plan.
//! New document semantics should land in focused crates such as
//! `agent-doc-element-*`, `agent-doc-template`, `agent-doc-diff`,
//! `agent-doc-frontmatter`, `agent-doc-document`, `agent-doc-merge`, or a
//! purpose-built pure crate.

pub mod id;
pub mod queue_item_lifecycle;

pub use agent_doc_element::element::Component;
pub use agent_doc_model_tier as model_tier;
pub use id::{
    BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary,
};
