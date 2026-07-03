//! Structured overlay projection for agent-component documents.
//!
//! Persists the parsed agent-component overlay beside snapshots as a stable,
//! versioned projection of the visible markdown. Formerly backed by Yrs maps and
//! arrays; migrated to a magic-prefixed markdown projection (yrs removed —
//! `tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`, Phase 3c).
//!
//! This state is a **projection, not a live CRDT**: it is only ever built fresh
//! from markdown (`from_markdown`), encoded, and later decoded and read back — two
//! overlays are never merged. So the components/items are recovered by re-parsing
//! the stored markdown (`overlay::components`), which is exactly what the old
//! structured schema round-tripped. The document text is the source of truth
//! (rebuild-from-disk): an old/opaque state that no longer decodes migrates by
//! rebuilding from the current visible markdown.

use crate::overlay::{self, Component};

/// Magic prefix identifying a v1 overlay projection state, so a legacy Yrs `.yrs`
/// state (or any foreign bytes) is recognized as "not this schema" and routed
/// through migration instead of mis-decoded.
const OVERLAY_MAGIC: &[u8] = b"ADOVL1\n";
const SCHEMA_VERSION: i64 = 1;

/// Errors returned when decoding or reading the structured overlay projection.
/// Retained for API stability; the projection path only ever produces
/// `DecodeState` / `MissingSchema`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrdtSchemaError {
    DecodeState(String),
    ApplyUpdate(String),
    MissingSchema,
    MissingField(&'static str),
    TypeMismatch(&'static str),
    InvalidKind(String),
    InvalidNumber(&'static str, i64),
}

impl std::fmt::Display for CrdtSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CrdtSchemaError::DecodeState(err) => write!(f, "failed to decode overlay state: {err}"),
            CrdtSchemaError::ApplyUpdate(err) => write!(f, "failed to apply overlay update: {err}"),
            CrdtSchemaError::MissingSchema => {
                write!(f, "state does not contain the structured overlay schema")
            }
            CrdtSchemaError::MissingField(field) => write!(f, "missing overlay field `{field}`"),
            CrdtSchemaError::TypeMismatch(field) => {
                write!(f, "overlay field `{field}` has an unexpected type")
            }
            CrdtSchemaError::InvalidKind(kind) => write!(f, "invalid overlay item kind `{kind}`"),
            CrdtSchemaError::InvalidNumber(field, value) => {
                write!(f, "overlay field `{field}` has invalid numeric value {value}")
            }
        }
    }
}

impl std::error::Error for CrdtSchemaError {}

/// A structured, versioned projection of the parsed agent-component overlay.
pub struct OverlayCrdtDoc {
    markdown: String,
}

impl OverlayCrdtDoc {
    /// Build a structured overlay projection from visible markdown.
    pub fn from_markdown(source: &str) -> Self {
        OverlayCrdtDoc {
            markdown: source.to_string(),
        }
    }

    /// Decode a structured overlay projection state. Returns
    /// [`CrdtSchemaError::MissingSchema`] for a legacy/foreign state (no magic).
    pub fn decode_state(bytes: &[u8]) -> Result<Self, CrdtSchemaError> {
        match bytes.strip_prefix(OVERLAY_MAGIC) {
            Some(md) => {
                let markdown = std::str::from_utf8(md)
                    .map_err(|e| CrdtSchemaError::DecodeState(e.to_string()))?
                    .to_string();
                Ok(OverlayCrdtDoc { markdown })
            }
            None => Err(CrdtSchemaError::MissingSchema),
        }
    }

    /// Decode a structured projection, or migrate a legacy/foreign state by
    /// rebuilding from the current visible markdown (`fallback_markdown`). The old
    /// Yrs-text-extraction migration is gone: the document text is authoritative,
    /// so an undecodable state simply rebuilds from disk.
    pub fn decode_state_or_migrate(
        bytes: &[u8],
        fallback_markdown: Option<&str>,
    ) -> Result<Self, CrdtSchemaError> {
        match Self::decode_state(bytes) {
            Ok(overlay) => Ok(overlay),
            Err(_) => Ok(OverlayCrdtDoc::from_markdown(fallback_markdown.unwrap_or(""))),
        }
    }

    /// Encode the projection for persistence beside snapshots.
    pub fn encode_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(OVERLAY_MAGIC.len() + self.markdown.len());
        out.extend_from_slice(OVERLAY_MAGIC);
        out.extend_from_slice(self.markdown.as_bytes());
        out
    }

    /// Return the visible markdown projection.
    pub fn to_markdown(&self) -> Result<String, CrdtSchemaError> {
        Ok(self.markdown.clone())
    }

    /// Return typed component/item nodes, re-parsed from the stored markdown
    /// (equivalent to the old structured schema, which stored exactly this).
    pub fn to_components(&self) -> Result<Vec<Component>, CrdtSchemaError> {
        Ok(overlay::components(&self.markdown))
    }

    pub fn schema_version(&self) -> Option<i64> {
        Some(SCHEMA_VERSION)
    }

    pub fn has_schema(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::ItemKind;

    const DOC: &str = "\
<!-- agent:queue priority go -->
- :pushpin: do [#alpha]
- ~~:pushpin: do [#beta]~~
- :round_pushpin: Free-text bug report
<!-- /agent:queue -->

<!-- agent:backlog priority queue -->
1. [/] [#alpha] gated task
<!-- /agent:backlog -->
";

    #[test]
    fn projection_state_round_trips_overlay_components() {
        let crdt = OverlayCrdtDoc::from_markdown(DOC);
        let encoded = crdt.encode_state();
        let decoded = OverlayCrdtDoc::decode_state(&encoded).unwrap();

        assert_eq!(decoded.schema_version(), Some(SCHEMA_VERSION));
        assert_eq!(decoded.to_markdown().unwrap(), DOC);
        assert_eq!(decoded.to_components().unwrap(), overlay::components(DOC));
    }

    #[test]
    fn pins_strikes_and_checkbox_state_are_structured_fields() {
        let decoded = OverlayCrdtDoc::from_markdown(DOC).to_components().unwrap();
        let queue = decoded.iter().find(|c| c.name == "queue").unwrap();
        let beta = &queue.items[1];
        let free_text = &queue.items[2];
        let backlog = decoded.iter().find(|c| c.name == "backlog").unwrap();

        assert!(beta.struck);
        assert!(beta.pinned);
        assert_eq!(beta.text, "do [#beta]");
        assert!(!free_text.pinned);
        assert!(free_text.agent_pinned);
        assert_eq!(
            backlog.items[0].kind,
            ItemKind::BacklogTask { checkbox: '/' }
        );
    }

    #[test]
    fn legacy_or_foreign_state_is_not_a_structured_projection() {
        // A legacy Yrs `.yrs` state (arbitrary bytes without the magic) must not
        // decode as a structured projection.
        let foreign = b"\x00\x01yrs-legacy-bytes\xff";
        match OverlayCrdtDoc::decode_state(foreign) {
            Err(err) => assert_eq!(err, CrdtSchemaError::MissingSchema),
            Ok(_) => panic!("foreign state must not decode as a structured projection"),
        }
    }

    #[test]
    fn legacy_migration_rebuilds_from_visible_markdown_fallback() {
        let foreign = b"\x00legacy";
        let migrated = OverlayCrdtDoc::decode_state_or_migrate(foreign, Some(DOC)).unwrap();
        assert_eq!(migrated.to_markdown().unwrap(), DOC);
        assert_eq!(migrated.to_components().unwrap(), overlay::components(DOC));
    }

    #[test]
    fn migration_without_fallback_yields_empty_projection() {
        let foreign = b"\x00legacy";
        let migrated = OverlayCrdtDoc::decode_state_or_migrate(foreign, None).unwrap();
        assert_eq!(migrated.to_markdown().unwrap(), "");
    }
}
