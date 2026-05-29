//! Template-mode support — pure half moved to `agent_doc_core::template`
//! (Wave 3 of #adcr); `&Path` / `std::fs` wrappers (apply_patches,
//! apply_patches_with_overrides, template_info) live in [`agent_doc_orchestration::template_io`]
//! (Wave 5 / `#ckv3`).
//!
//! Thin re-export so `crate::template::*` call sites in run.rs, stream.rs,
//! write.rs, preflight.rs, repair.rs, etc. continue to resolve. The
//! `template_io` re-export overrides core's pure `apply_patches` /
//! `apply_patches_with_overrides` with the file-based wrappers so existing
//! `template::apply_patches(doc, patches, "", &file)` call sites compile
//! unchanged.

pub use agent_doc_core::template::*;
pub use agent_doc_orchestration::template_io::{apply_patches, apply_patches_with_overrides, template_info};
