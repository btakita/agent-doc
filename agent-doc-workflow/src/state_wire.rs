//! Pure lazily-spec state-wire vocabulary for agent-doc projections.

use serde::{Deserialize, Serialize};

/// The eight agent-doc state node `type_tag`s — the stable cross-language
/// vocabulary plugins address nodes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentDocNodeType {
    /// `agent_doc.document.baseline` — document baseline projection.
    DocumentBaseline,
    /// `agent_doc.queue` — queue document singleton.
    Queue,
    /// `agent_doc.queue.head` — one per queue head node key.
    QueueHead,
    /// `agent_doc.closeout.cycle` — one per closeout cycle.
    CloseoutCycle,
    /// `agent_doc.transport.patch` — one per transport patch.
    TransportPatch,
    /// `agent_doc.supervisor.owner` — one per state owner.
    SupervisorOwner,
    /// `agent_doc.route` — route document singleton.
    Route,
    /// `agent_doc.proof.marker` — one per proof marker.
    ProofMarker,
}

impl AgentDocNodeType {
    /// Wire-stable, versioned `type_tag` string. Never rename without bumping
    /// the lazily-spec schema.
    pub const fn type_tag(self) -> &'static str {
        match self {
            Self::DocumentBaseline => "agent_doc.document.baseline",
            Self::Queue => "agent_doc.queue",
            Self::QueueHead => "agent_doc.queue.head",
            Self::CloseoutCycle => "agent_doc.closeout.cycle",
            Self::TransportPatch => "agent_doc.transport.patch",
            Self::SupervisorOwner => "agent_doc.supervisor.owner",
            Self::Route => "agent_doc.route",
            Self::ProofMarker => "agent_doc.proof.marker",
        }
    }

    /// All node kinds in canonical order for deterministic node walks.
    pub const ALL: [Self; 8] = [
        Self::DocumentBaseline,
        Self::Queue,
        Self::QueueHead,
        Self::CloseoutCycle,
        Self::TransportPatch,
        Self::SupervisorOwner,
        Self::Route,
        Self::ProofMarker,
    ];
}

/// FNV-1a 64-bit over `(document_hash, type_tag, entity_key)`.
///
/// Produces a stable, allocation-free `slot_id` so Rust/Kotlin/JS address the
/// same node without a central allocator.
pub fn slot_id(document_hash: &str, type_tag: &str, entity_key: &str) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    const SEP: u8 = 0xFF;
    let mut hash = FNV_OFFSET;
    let mix = |mut hash: u64, bytes: &[u8]| -> u64 {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    };
    hash = mix(hash, document_hash.as_bytes());
    hash = mix(hash, std::slice::from_ref(&SEP));
    hash = mix(hash, type_tag.as_bytes());
    hash = mix(hash, std::slice::from_ref(&SEP));
    hash = mix(hash, entity_key.as_bytes());
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_tags_are_stable_and_ordered() {
        assert_eq!(
            AgentDocNodeType::ALL.map(AgentDocNodeType::type_tag),
            [
                "agent_doc.document.baseline",
                "agent_doc.queue",
                "agent_doc.queue.head",
                "agent_doc.closeout.cycle",
                "agent_doc.transport.patch",
                "agent_doc.supervisor.owner",
                "agent_doc.route",
                "agent_doc.proof.marker",
            ]
        );
    }

    #[test]
    fn slot_id_changes_with_each_identity_component() {
        let base = slot_id("doc", AgentDocNodeType::Queue.type_tag(), "entity");

        assert_eq!(
            base,
            slot_id("doc", AgentDocNodeType::Queue.type_tag(), "entity")
        );
        assert_ne!(
            base,
            slot_id("other", AgentDocNodeType::Queue.type_tag(), "entity")
        );
        assert_ne!(
            base,
            slot_id("doc", AgentDocNodeType::QueueHead.type_tag(), "entity")
        );
        assert_ne!(
            base,
            slot_id("doc", AgentDocNodeType::Queue.type_tag(), "other")
        );
    }
}
