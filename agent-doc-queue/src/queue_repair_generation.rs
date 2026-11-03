//! Monotonic document-local generation for queue reset/projection repair.
//!
//! The generation fences derived marker writers across a clean-slate reset.
//! A projection that began before the reset may finish parsing afterward, but
//! it cannot write against or retire the newer generation.

use serde::{Deserialize, Serialize};

pub const QUEUE_REPAIR_GENERATION_STATE_KIND: &str = "queue_repair_generation";
pub const QUEUE_PROJECTION_REPAIR_REQUEST_STATE_KIND: &str = "queue_projection_repair_request";
pub const QUEUE_PROJECTION_REPAIR_SEQUENCE_STATE_KIND: &str = "queue_projection_repair_sequence";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRepairGeneration {
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueProjectionRepairRequest {
    /// Queue clean-slate generation that fences the document write.
    pub generation: u64,
    /// Monotonic identity of this one-shot repair request.
    ///
    /// Older persisted requests predate this field and deserialize as zero;
    /// the next recorder upgrades them without losing the repair obligation.
    #[serde(default)]
    pub request_generation: u64,
    pub reason: String,
    pub requested_at_secs: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueProjectionRepairSequence {
    pub generation: u64,
}
