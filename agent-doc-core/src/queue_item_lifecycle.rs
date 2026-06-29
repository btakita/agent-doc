//! Compatibility re-export for the queue-item lifecycle lattice.
//!
//! The implementation lives in `agent-doc-element-queue`, because queue
//! lifecycle is the queue element's local realtime model. This module keeps the
//! old `agent_doc_core::queue_item_lifecycle::QueueItemLifecycle` path stable
//! while callers move to the element crate.

pub use agent_doc_element_queue::QueueItemLifecycle;
