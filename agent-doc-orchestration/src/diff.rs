//! Diff — pure half moved to `agent_doc_core::diff` (Wave 4 of #adcr per
//! `#rtx6` Option 1). The snapshot-backed I/O half lives in [`crate::diff_io`]
//! and is re-exported here so callers can use `diff::compute(file)` etc.
//! unchanged.

pub use crate::diff_io::*;
pub use agent_doc_core::diff::*;
