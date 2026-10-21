//! Legacy Remote Dev focus evidence (GH #134, GH #343).
//!
//! Visible, selected, and open file lists do not contain split geometry or a
//! left-to-right column vector. They are therefore focus evidence only. This
//! fold never derives pane membership or order from those lists, regardless of
//! how often or in which order they arrive.
//!
//! A retained `columns` value is an exact frontend-authoritative snapshot
//! cached by a caller that has such a snapshot. Flat legacy observations may
//! update focus while preserving that value byte-for-byte. With no cached
//! snapshot the answer is `Unknown`, and downstream code must defer structural
//! work. Exact schema-3 columns and schema-2 measured geometry bypass this
//! legacy fold through their typed layout paths.

use serde::{Deserialize, Serialize};

use crate::SurfaceColumn;

/// What one Remote Dev client reported, restricted to session documents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteClientEditors {
    /// Session documents whose client editor is currently visible.
    /// This flat set does not identify frontend columns.
    #[serde(default)]
    pub visible: Vec<String>,
    /// Session documents the client reports as selected. In Remote Dev this is
    /// normally only the focused file.
    #[serde(default)]
    pub selected: Vec<String>,
    /// Session documents open as tabs in this client. Empty means "unknown".
    #[serde(default)]
    pub open: Vec<String>,
}

/// One legacy observation of a Remote Dev backend with no local editor windows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutEvidence {
    #[serde(default)]
    pub clients: Vec<RemoteClientEditors>,
    /// Backend-local focus evidence.
    #[serde(default)]
    pub focused: Vec<String>,
}

/// Where a resolution's columns came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteLayoutSource {
    /// Legacy serialized value. Flat visible-file evidence no longer produces
    /// this source.
    RemoteClientVisibleEditors,
    /// Legacy serialized value. Selection evidence is focus-only.
    RemoteClientSelectedFiles,
    /// A cached exact frontend snapshot was preserved unchanged.
    RetainedRemoteColumns,
    /// No exact frontend snapshot is available.
    Unknown,
}

impl RemoteLayoutSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RemoteClientVisibleEditors => "remote_client_visible_editors",
            Self::RemoteClientSelectedFiles => "remote_client_selected_files",
            Self::RetainedRemoteColumns => "retained_remote_columns",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether the answer preserves an exact cached frontend column vector.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteLayoutColumnOrder {
    #[default]
    Unknown,
    Retained,
}

/// The columns to publish, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutResolution {
    /// Empty only for [`RemoteLayoutSource::Unknown`].
    pub columns: Vec<SurfaceColumn>,
    pub source: RemoteLayoutSource,
    #[serde(default)]
    pub column_order: RemoteLayoutColumnOrder,
    /// Machine-readable explanation for retained/unknown answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Compatibility state for the legacy flat-observation resolver. Production starts empty and
/// [`RemoteLayoutMemory::advance`] never populates or edits `columns`; non-empty columns are
/// accepted only for legacy deserialization and explicit synthetic tests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutMemory {
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
    #[serde(default)]
    pub focused: Option<String>,
}

impl RemoteLayoutMemory {
    /// Classify one legacy flat observation.
    ///
    /// The observation can update focus only. It cannot add, remove, replace,
    /// group, or reorder any column. Repetition is still the same incomplete
    /// evidence and confers no additional authority.
    pub fn advance(&self, evidence: &RemoteLayoutEvidence) -> (Self, RemoteLayoutResolution) {
        let active = active_document(evidence);
        let next = Self {
            columns: self.columns.clone(),
            focused: active.or_else(|| self.focused.clone()),
        };
        let resolution = if self.columns.is_empty() {
            RemoteLayoutResolution {
                columns: Vec::new(),
                source: RemoteLayoutSource::Unknown,
                column_order: RemoteLayoutColumnOrder::Unknown,
                reason: Some("flat_evidence_has_no_column_geometry".to_string()),
            }
        } else {
            RemoteLayoutResolution {
                columns: self.columns.clone(),
                source: RemoteLayoutSource::RetainedRemoteColumns,
                column_order: RemoteLayoutColumnOrder::Retained,
                reason: Some("focus_only_preserved_authoritative_snapshot".to_string()),
            }
        };
        (next, resolution)
    }
}

/// The document the operator is on: one selected client file, else the
/// backend-local focused file. Visible and open lists are never focus fallbacks
/// because their order is not authoritative either.
fn active_document(evidence: &RemoteLayoutEvidence) -> Option<String> {
    let singles = distinct(
        evidence
            .clients
            .iter()
            .filter_map(|client| match distinct(client.selected.iter()).as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .iter(),
    );
    if let [only] = singles.as_slice() {
        return Some(only.clone());
    }
    evidence
        .focused
        .iter()
        .find(|file| !file.is_empty())
        .cloned()
}

fn distinct<'a>(files: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for file in files {
        if !file.is_empty() && !seen.contains(file) {
            seen.push(file.clone());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(visible: &[&str], selected: &[&str], open: &[&str]) -> RemoteLayoutEvidence {
        let owned = |files: &[&str]| files.iter().map(|file| (*file).to_string()).collect();
        RemoteLayoutEvidence {
            clients: vec![RemoteClientEditors {
                visible: owned(visible),
                selected: owned(selected),
                open: owned(open),
            }],
            focused: selected
                .first()
                .map(|file| (*file).to_string())
                .into_iter()
                .collect(),
        }
    }

    fn authoritative() -> RemoteLayoutMemory {
        RemoteLayoutMemory {
            columns: vec![
                SurfaceColumn::new(["left.md"]),
                SurfaceColumn::new(["right.md"]),
            ],
            focused: Some("left.md".to_string()),
        }
    }

    #[test]
    fn arbitrarily_many_repeated_flat_observations_never_create_structure() {
        let observed = evidence(&["a.md", "b.md"], &["b.md"], &["a.md", "b.md"]);
        let mut memory = RemoteLayoutMemory::default();
        for _ in 0..10_000 {
            let (next, resolution) = memory.advance(&observed);
            assert!(resolution.columns.is_empty());
            assert_eq!(resolution.source, RemoteLayoutSource::Unknown);
            assert_eq!(
                resolution.reason.as_deref(),
                Some("flat_evidence_has_no_column_geometry")
            );
            memory = next;
        }
        assert_eq!(memory.focused.as_deref(), Some("b.md"));
    }

    #[test]
    fn flat_unknown_partial_and_selection_only_inputs_preserve_exact_snapshot() {
        let expected = authoritative().columns;
        let observations = [
            evidence(&[], &[], &[]),
            evidence(&["left.md"], &["left.md"], &["left.md"]),
            evidence(&["new.md", "left.md"], &["new.md"], &["new.md"]),
            evidence(&["right.md", "left.md"], &["right.md"], &[]),
            evidence(&["x.md", "y.md", "z.md"], &["z.md"], &[]),
        ];
        let mut memory = authoritative();
        for observation in observations.iter().cycle().take(1_000) {
            let (next, resolution) = memory.advance(observation);
            assert_eq!(resolution.columns, expected);
            assert_eq!(next.columns, expected);
            assert_eq!(resolution.source, RemoteLayoutSource::RetainedRemoteColumns);
            memory = next;
        }
    }

    #[test]
    fn evidence_roundtrips_without_turning_flat_files_into_columns() {
        let json =
            r#"{"clients":[{"visible":["a.md","b.md"],"selected":["a.md"]}],"focused":["a.md"]}"#;
        let parsed: RemoteLayoutEvidence = serde_json::from_str(json).unwrap();
        let (_, resolution) = RemoteLayoutMemory::default().advance(&parsed);
        assert_eq!(resolution.source, RemoteLayoutSource::Unknown);
        assert!(resolution.columns.is_empty());
    }
}
