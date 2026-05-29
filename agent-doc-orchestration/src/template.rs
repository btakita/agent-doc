//! Template shim — pure half from `agent_doc_core::template`, `&Path` /
//! `std::fs` wrappers from `crate::template_io` (now in orchestration). The
//! `template_io` re-export overrides core's pure `apply_patches` /
//! `apply_patches_with_overrides` with the file-based wrappers.

pub use agent_doc_core::template::*;
pub use crate::template_io::{apply_patches, apply_patches_with_overrides, template_info};
