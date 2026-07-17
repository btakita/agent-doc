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
