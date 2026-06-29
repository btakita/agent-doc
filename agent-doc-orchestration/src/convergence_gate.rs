//! Compatibility re-export for the realtime document convergence gate.
//!
//! The pure decision core lives in `agent-doc-document-realtime`; orchestration
//! gathers runtime facts and consumes the decision.

pub use agent_doc_document_realtime::convergence_gate::{
    ConvergenceFacts, ConvergenceGateDecision, convergence_gate_decision, proof,
};
