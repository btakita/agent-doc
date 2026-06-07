//! Structured CRDT schema for the agent-component overlay.
//!
//! Phase 3 of `#md-ast-document-model`: persist the overlay as Yrs maps and
//! arrays instead of storing only the visible markdown in a single `Text`.
//! Current callers can still keep the markdown projection, while later phases
//! mutate queue/backlog/exchange nodes by stable id.

use crate::overlay::{self, Component, Item, ItemKind};
use std::convert::TryFrom;
use std::fmt;
use yrs::updates::decoder::Decode;
use yrs::{
    Array, ArrayPrelim, ArrayRef, Doc, GetString, In, Map, MapPrelim, MapRef, ReadTxn, TextRef,
    Transact, Update,
};

const ROOT_KEY: &str = "agent_doc_overlay";
const LEGACY_TEXT_KEY: &str = "content";
const SCHEMA_VERSION: i64 = 1;

const FIELD_SCHEMA_VERSION: &str = "schema_version";
const FIELD_MARKDOWN: &str = "markdown";
const FIELD_COMPONENTS: &str = "components";
const FIELD_NAME: &str = "name";
const FIELD_ATTRS: &str = "attrs";
const FIELD_START_BYTE: &str = "start_byte";
const FIELD_END_BYTE: &str = "end_byte";
const FIELD_ITEMS: &str = "items";
const FIELD_ID: &str = "id";
const FIELD_TEXT: &str = "text";
const FIELD_RAW: &str = "raw";
const FIELD_STRUCK: &str = "struck";
const FIELD_PINNED: &str = "pinned";
const FIELD_AGENT_PINNED: &str = "agent_pinned";
const FIELD_KIND: &str = "kind";
const FIELD_TYPE: &str = "type";
const FIELD_CHECKBOX: &str = "checkbox";

/// Errors returned when decoding or reading the structured overlay CRDT.
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

impl fmt::Display for CrdtSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CrdtSchemaError::DecodeState(err) => write!(f, "failed to decode CRDT state: {err}"),
            CrdtSchemaError::ApplyUpdate(err) => write!(f, "failed to apply CRDT update: {err}"),
            CrdtSchemaError::MissingSchema => {
                write!(
                    f,
                    "CRDT state does not contain the structured overlay schema"
                )
            }
            CrdtSchemaError::MissingField(field) => write!(f, "missing CRDT field `{field}`"),
            CrdtSchemaError::TypeMismatch(field) => {
                write!(f, "CRDT field `{field}` has an unexpected type")
            }
            CrdtSchemaError::InvalidKind(kind) => write!(f, "invalid overlay item kind `{kind}`"),
            CrdtSchemaError::InvalidNumber(field, value) => {
                write!(f, "CRDT field `{field}` has invalid numeric value {value}")
            }
        }
    }
}

impl std::error::Error for CrdtSchemaError {}

/// A Yrs-backed structured representation of the parsed agent-component overlay.
pub struct OverlayCrdtDoc {
    doc: Doc,
}

impl OverlayCrdtDoc {
    /// Build a structured overlay state from visible markdown.
    pub fn from_markdown(source: &str) -> Self {
        let doc = Doc::new();
        let root = doc.get_or_insert_map(ROOT_KEY);
        let components = overlay::components(source);
        {
            let mut txn = doc.transact_mut();
            root.insert(&mut txn, FIELD_SCHEMA_VERSION, SCHEMA_VERSION);
            root.insert(&mut txn, FIELD_MARKDOWN, source);
            root.insert(
                &mut txn,
                FIELD_COMPONENTS,
                ArrayPrelim::from(components.iter().map(component_prelim)),
            );
        }
        OverlayCrdtDoc { doc }
    }

    /// Decode a structured overlay state.
    pub fn decode_state(bytes: &[u8]) -> Result<Self, CrdtSchemaError> {
        let doc = decode_doc(bytes)?;
        let overlay = OverlayCrdtDoc { doc };
        if overlay.has_schema() {
            Ok(overlay)
        } else {
            Err(CrdtSchemaError::MissingSchema)
        }
    }

    /// Decode a structured state, or migrate an old `.yrs` state that stored
    /// the visible markdown under the legacy `content` text key.
    pub fn decode_state_or_migrate(
        bytes: &[u8],
        fallback_markdown: Option<&str>,
    ) -> Result<Self, CrdtSchemaError> {
        let doc = decode_doc(bytes)?;
        let overlay = OverlayCrdtDoc { doc };
        if overlay.has_schema() {
            return Ok(overlay);
        }
        let legacy = overlay.legacy_markdown_projection();
        let source = if legacy.is_empty() {
            fallback_markdown.unwrap_or("")
        } else {
            &legacy
        };
        Ok(OverlayCrdtDoc::from_markdown(source))
    }

    /// Encode the full Yrs update for persistence beside snapshots.
    pub fn encode_state(&self) -> Vec<u8> {
        let txn = self.doc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    }

    /// Return the visible markdown projection stored in the structured root.
    pub fn to_markdown(&self) -> Result<String, CrdtSchemaError> {
        let root = self.doc.get_or_insert_map(ROOT_KEY);
        let txn = self.doc.transact();
        get_string(&root, &txn, FIELD_MARKDOWN)
    }

    /// Return typed component/item nodes from the structured CRDT.
    pub fn to_components(&self) -> Result<Vec<Component>, CrdtSchemaError> {
        let root = self.doc.get_or_insert_map(ROOT_KEY);
        let txn = self.doc.transact();
        let components = get_array(&root, &txn, FIELD_COMPONENTS)?;
        components
            .iter(&txn)
            .map(|value| {
                let map = value
                    .cast::<MapRef>()
                    .map_err(|_| CrdtSchemaError::TypeMismatch(FIELD_COMPONENTS))?;
                component_from_map(&map, &txn)
            })
            .collect()
    }

    pub fn schema_version(&self) -> Option<i64> {
        let root = self.doc.get_or_insert_map(ROOT_KEY);
        let txn = self.doc.transact();
        root.get(&txn, FIELD_SCHEMA_VERSION)
            .and_then(|value| value.cast::<i64>().ok())
    }

    pub fn has_schema(&self) -> bool {
        self.schema_version() == Some(SCHEMA_VERSION)
    }

    fn legacy_markdown_projection(&self) -> String {
        let text: TextRef = self.doc.get_or_insert_text(LEGACY_TEXT_KEY);
        let txn = self.doc.transact();
        text.get_string(&txn)
    }
}

fn decode_doc(bytes: &[u8]) -> Result<Doc, CrdtSchemaError> {
    let doc = Doc::new();
    let update =
        Update::decode_v1(bytes).map_err(|err| CrdtSchemaError::DecodeState(err.to_string()))?;
    {
        let mut txn = doc.transact_mut();
        txn.apply_update(update)
            .map_err(|err| CrdtSchemaError::ApplyUpdate(err.to_string()))?;
    }
    Ok(doc)
}

fn component_prelim(component: &Component) -> In {
    In::from(MapPrelim::from([
        (FIELD_NAME, In::from(component.name.clone())),
        (FIELD_ATTRS, In::from(component.attrs.clone())),
        (FIELD_START_BYTE, In::from(component.start_byte as i64)),
        (FIELD_END_BYTE, In::from(component.end_byte as i64)),
        (
            FIELD_ITEMS,
            In::from(ArrayPrelim::from(component.items.iter().map(item_prelim))),
        ),
    ]))
}

fn item_prelim(item: &Item) -> In {
    In::from(MapPrelim::from([
        (FIELD_ID, In::from(item.id.clone())),
        (FIELD_TEXT, In::from(item.text.clone())),
        (FIELD_RAW, In::from(item.raw.clone())),
        (FIELD_START_BYTE, In::from(item.start_byte as i64)),
        (FIELD_END_BYTE, In::from(item.end_byte as i64)),
        (FIELD_STRUCK, In::from(item.struck)),
        (FIELD_PINNED, In::from(item.pinned)),
        (FIELD_AGENT_PINNED, In::from(item.agent_pinned)),
        (FIELD_KIND, In::from(item_kind_prelim(&item.kind))),
    ]))
}

fn item_kind_prelim(kind: &ItemKind) -> MapPrelim {
    match kind {
        ItemKind::Do => MapPrelim::from([(FIELD_TYPE, In::from("do"))]),
        ItemKind::Reference => MapPrelim::from([(FIELD_TYPE, In::from("reference"))]),
        ItemKind::FreeText => MapPrelim::from([(FIELD_TYPE, In::from("free_text"))]),
        ItemKind::BacklogTask { checkbox } => MapPrelim::from([
            (FIELD_TYPE, In::from("backlog_task")),
            (FIELD_CHECKBOX, In::from(checkbox.to_string())),
        ]),
    }
}

fn component_from_map<T: ReadTxn>(map: &MapRef, txn: &T) -> Result<Component, CrdtSchemaError> {
    let items = get_array(map, txn, FIELD_ITEMS)?;
    let items = items
        .iter(txn)
        .map(|value| {
            let map = value
                .cast::<MapRef>()
                .map_err(|_| CrdtSchemaError::TypeMismatch(FIELD_ITEMS))?;
            item_from_map(&map, txn)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Component {
        name: get_string(map, txn, FIELD_NAME)?,
        attrs: get_string(map, txn, FIELD_ATTRS)?,
        start_byte: get_usize(map, txn, FIELD_START_BYTE)?,
        end_byte: get_usize(map, txn, FIELD_END_BYTE)?,
        items,
    })
}

fn item_from_map<T: ReadTxn>(map: &MapRef, txn: &T) -> Result<Item, CrdtSchemaError> {
    Ok(Item {
        id: get_string(map, txn, FIELD_ID)?,
        text: get_string(map, txn, FIELD_TEXT)?,
        raw: get_string(map, txn, FIELD_RAW)?,
        start_byte: get_usize(map, txn, FIELD_START_BYTE)?,
        end_byte: get_usize(map, txn, FIELD_END_BYTE)?,
        struck: get_bool(map, txn, FIELD_STRUCK)?,
        pinned: get_bool(map, txn, FIELD_PINNED)?,
        agent_pinned: get_bool(map, txn, FIELD_AGENT_PINNED)?,
        kind: item_kind_from_map(&get_map(map, txn, FIELD_KIND)?, txn)?,
    })
}

fn item_kind_from_map<T: ReadTxn>(map: &MapRef, txn: &T) -> Result<ItemKind, CrdtSchemaError> {
    match get_string(map, txn, FIELD_TYPE)?.as_str() {
        "do" => Ok(ItemKind::Do),
        "reference" => Ok(ItemKind::Reference),
        "free_text" => Ok(ItemKind::FreeText),
        "backlog_task" => {
            let checkbox = get_string(map, txn, FIELD_CHECKBOX)?
                .chars()
                .next()
                .unwrap_or(' ');
            Ok(ItemKind::BacklogTask { checkbox })
        }
        other => Err(CrdtSchemaError::InvalidKind(other.to_string())),
    }
}

fn get_string<T: ReadTxn>(
    map: &MapRef,
    txn: &T,
    field: &'static str,
) -> Result<String, CrdtSchemaError> {
    map.get(txn, field)
        .ok_or(CrdtSchemaError::MissingField(field))?
        .cast::<String>()
        .map_err(|_| CrdtSchemaError::TypeMismatch(field))
}

fn get_bool<T: ReadTxn>(
    map: &MapRef,
    txn: &T,
    field: &'static str,
) -> Result<bool, CrdtSchemaError> {
    map.get(txn, field)
        .ok_or(CrdtSchemaError::MissingField(field))?
        .cast::<bool>()
        .map_err(|_| CrdtSchemaError::TypeMismatch(field))
}

fn get_usize<T: ReadTxn>(
    map: &MapRef,
    txn: &T,
    field: &'static str,
) -> Result<usize, CrdtSchemaError> {
    let value = map
        .get(txn, field)
        .ok_or(CrdtSchemaError::MissingField(field))?
        .cast::<i64>()
        .map_err(|_| CrdtSchemaError::TypeMismatch(field))?;
    usize::try_from(value).map_err(|_| CrdtSchemaError::InvalidNumber(field, value))
}

fn get_map<T: ReadTxn>(
    map: &MapRef,
    txn: &T,
    field: &'static str,
) -> Result<MapRef, CrdtSchemaError> {
    map.get(txn, field)
        .ok_or(CrdtSchemaError::MissingField(field))?
        .cast::<MapRef>()
        .map_err(|_| CrdtSchemaError::TypeMismatch(field))
}

fn get_array<T: ReadTxn>(
    map: &MapRef,
    txn: &T,
    field: &'static str,
) -> Result<ArrayRef, CrdtSchemaError> {
    map.get(txn, field)
        .ok_or(CrdtSchemaError::MissingField(field))?
        .cast::<ArrayRef>()
        .map_err(|_| CrdtSchemaError::TypeMismatch(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::{Text, Transact};

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
    fn structured_state_round_trips_overlay_components() {
        let crdt = OverlayCrdtDoc::from_markdown(DOC);
        let encoded = crdt.encode_state();
        let decoded = OverlayCrdtDoc::decode_state(&encoded).unwrap();

        assert_eq!(decoded.schema_version(), Some(SCHEMA_VERSION));
        assert_eq!(decoded.to_markdown().unwrap(), DOC);
        assert_eq!(decoded.to_components().unwrap(), overlay::components(DOC));
    }

    #[test]
    fn schema_uses_arrays_and_maps_for_components_and_items() {
        let crdt = OverlayCrdtDoc::from_markdown(DOC);
        let root = crdt.doc.get_or_insert_map(ROOT_KEY);
        let txn = crdt.doc.transact();
        let components = get_array(&root, &txn, FIELD_COMPONENTS).unwrap();
        let first_component = components.get(&txn, 0).unwrap().cast::<MapRef>().unwrap();
        let items = get_array(&first_component, &txn, FIELD_ITEMS).unwrap();
        let first_item = items.get(&txn, 0).unwrap().cast::<MapRef>().unwrap();
        let kind = get_map(&first_item, &txn, FIELD_KIND).unwrap();

        assert_eq!(
            get_string(&first_component, &txn, FIELD_NAME).unwrap(),
            "queue"
        );
        assert_eq!(get_string(&first_item, &txn, FIELD_ID).unwrap(), "alpha");
        assert_eq!(get_string(&kind, &txn, FIELD_TYPE).unwrap(), "do");
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
    fn legacy_text_state_migrates_to_structured_schema() {
        let legacy = Doc::new();
        let text = legacy.get_or_insert_text(LEGACY_TEXT_KEY);
        {
            let mut txn = legacy.transact_mut();
            text.insert(&mut txn, 0, DOC);
        }
        let legacy_state = {
            let txn = legacy.transact();
            txn.encode_state_as_update_v1(&yrs::StateVector::default())
        };

        match OverlayCrdtDoc::decode_state(&legacy_state) {
            Err(err) => assert_eq!(err, CrdtSchemaError::MissingSchema),
            Ok(_) => panic!("legacy text state must not decode as structured schema"),
        }

        let migrated = OverlayCrdtDoc::decode_state_or_migrate(&legacy_state, None).unwrap();
        assert!(migrated.has_schema());
        assert_eq!(migrated.to_markdown().unwrap(), DOC);
        assert_eq!(migrated.to_components().unwrap(), overlay::components(DOC));
    }

    #[test]
    fn legacy_migration_can_use_visible_markdown_fallback() {
        let legacy = Doc::new();
        let legacy_state = {
            let txn = legacy.transact();
            txn.encode_state_as_update_v1(&yrs::StateVector::default())
        };

        let migrated = OverlayCrdtDoc::decode_state_or_migrate(&legacy_state, Some(DOC)).unwrap();

        assert_eq!(migrated.to_markdown().unwrap(), DOC);
        assert_eq!(migrated.to_components().unwrap(), overlay::components(DOC));
    }

    #[test]
    fn legacy_migration_prefers_non_empty_legacy_text_over_fallback() {
        let fallback = "<!-- agent:queue -->\n- do [#fallback]\n<!-- /agent:queue -->\n";
        let legacy = Doc::new();
        let text = legacy.get_or_insert_text(LEGACY_TEXT_KEY);
        {
            let mut txn = legacy.transact_mut();
            text.insert(&mut txn, 0, DOC);
        }
        let legacy_state = {
            let txn = legacy.transact();
            txn.encode_state_as_update_v1(&yrs::StateVector::default())
        };

        let migrated =
            OverlayCrdtDoc::decode_state_or_migrate(&legacy_state, Some(fallback)).unwrap();

        assert_eq!(migrated.to_markdown().unwrap(), DOC);
        assert_eq!(migrated.to_components().unwrap(), overlay::components(DOC));
    }
}
