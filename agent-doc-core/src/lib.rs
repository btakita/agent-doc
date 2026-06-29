//! agent-doc-core — document data layer for agent-doc.
//!
//! See `tasks/agent-doc/prd-crate-decomposition.md` for the current extraction
//! plan.
//! `agent-doc-core` is a compatibility/data facade during extraction, not the
//! home for new domain policy. New document semantics should land in focused
//! crates such as `agent-doc-element-*`, `agent-doc-document`,
//! `agent-doc-merge`, or a purpose-built pure crate.

pub mod cell_doc;
pub mod crdt;
pub mod crdt_sync;
pub mod diff;
pub mod ffi;
pub mod frontmatter;
pub mod gate_verify;
pub mod heuristics;
pub mod id;
pub mod log_time;
pub mod op_log;
pub mod pending;
pub mod project_config;
pub mod queue_item_lifecycle;
pub mod replay_guard;
pub mod syntax;
pub mod template;
pub mod topic;
pub mod turn_scope;

pub use agent_doc_element::element::Component;
pub use agent_doc_model_tier as model_tier;
pub use crdt::CrdtDoc;
pub use diff::{
    DiffClassification, DiffType, OrchestrationRequest, OrchestrationRequestMode,
    ParsedSlashCommands, PromptBearingChange, PromptBearingChangeKind,
};
pub use id::{
    BOUNDARY_ID_LEN, format_boundary_marker, new_boundary_id, new_boundary_id_with_summary,
};
pub use template::{ComponentInfo, PatchBlock, TemplateInfo};
