//! Shared binary/plugin vocabulary for Lazily document authority.
//!
//! These names are transport capabilities, not alternate state stores. The
//! Rust core and editor plugins use the same tokens at the Lazily seam so the
//! implementation cannot silently drift into a second live-buffer model.

/// The editor supplies complete operator text through Lazily current state.
pub const OPERATOR_TEXT_AUTHORITY_CAPABILITY: &str = "operator_text_authority_v1";

/// The editor reports Lazily delivery receipts for visible-write proof.
pub const LAZILY_TRANSPORT_RECEIPTS_CAPABILITY: &str = "lazily_transport_receipts_v1";

/// The editor and core can exchange the lossless semantic cell tree.
pub const LOSSLESS_TREE_CRDT_CAPABILITY: &str = "lossless_tree_crdt_v1";

/// `#ctrlkillreregister` Tier 3 — the editor asks `agent_doc_peer_replicas_missing`
/// about itself on startup and on reconnect, and rebuilds whatever it names.
///
/// This is a **retirement condition**, not a feature flag. A peer advertising it
/// repairs itself from replicated state, so the controller's Tier 1 restart fan-out
/// must skip it: that push exists only for plugins predating the pull, and every
/// push is a delivery that can fail to reach its endpoint (`reload-lib reached 1/4
/// endpoints`). Retiring per-peer off the converged registration set means neither
/// side needs to be upgraded first and there is no flag day.
pub const PEER_REPLICA_PULL_CAPABILITY: &str = "peer_replica_pull_v1";

/// Required capabilities for a plugin that participates in live authority.
pub const REQUIRED_LAZILY_EDITOR_CAPABILITIES: [&str; 2] = [
    OPERATOR_TEXT_AUTHORITY_CAPABILITY,
    LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
];

pub fn has_capability(capabilities: &[String], required: &str) -> bool {
    capabilities.iter().any(|capability| capability == required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_capabilities_use_the_lazily_contract_vocabulary() {
        assert_eq!(
            REQUIRED_LAZILY_EDITOR_CAPABILITIES,
            ["operator_text_authority_v1", "lazily_transport_receipts_v1"]
        );
    }
}
