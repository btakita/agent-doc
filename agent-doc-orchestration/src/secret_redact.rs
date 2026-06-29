//! Compatibility re-export for secret redaction.
//!
//! The pure redaction engine lives in `agent-doc-secret-redact`; orchestration
//! applies it at capture, snapshot, route, and stream boundaries.

pub use agent_doc_secret_redact::*;
