//! Template shim — pure half from `agent_doc_core::template`, `&Path` /
//! `std::fs` wrappers from `crate::template_io` (now in orchestration). The
//! `template_io` re-export overrides core's pure `apply_patches` /
//! `apply_patches_with_overrides` with the file-based wrappers.

pub use crate::template_io::{
    apply_patches, apply_patches_with_context, apply_patches_with_overrides,
    apply_patches_with_overrides_with_context, template_info, template_info_with_context,
};
pub use agent_doc_core::template::*;
