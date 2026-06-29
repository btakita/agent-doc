//! Compatibility re-export for bounded turn-wait machinery.
//!
//! The implementation lives in `agent-doc-turn`; orchestration owns only the
//! runtime adapters that decide which condition to wait for.

pub use agent_doc_turn::wait_machine::*;
