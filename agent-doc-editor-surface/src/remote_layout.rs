//! Remote Dev split resolution (GH #134).
//!
//! A JetBrains Remote Dev backend exposes **no** `EditorWindow`: the split UI
//! lives in the thin client. The per-client `ClientFileEditorManager` reports
//! only the focused file as "selected", so a detector that requires a client to
//! *select* two files at once never sees a split and answers `unknown` forever
//! (`windows=0 ... reason=no_unique_client_split_set`). The controller is then
//! handed zero columns and tmux can never show more than one pane.
//!
//! The backend does know which client editors are *visible*: the Remote Dev
//! editor tracker records every frontend text editor whose visibility the
//! client reported (one per visible split). Two visible session documents for
//! one client are two splits, because a split shows one tab at a time.
//!
//! This module is the pure fold from that per-client evidence (visible,
//! selected, and open session documents) plus the previous resolution to the
//! column set the plugin publishes. It lives in the shared crate so every
//! editor adapter gets the same answer (`#ffi-first`), and it is a value fold
//! like [`crate::SurfaceTracking`], so the ordering between "read the previous
//! resolution" and "record this one" is owned by one function.
//!
//! Rules, in order:
//!
//! 1. **Detected.** Exactly one distinct client evidence set names two or more
//!    session documents → one column per document. Visible editors outrank
//!    selected files. Column order is stable: documents already in the
//!    previous layout keep their order, new ones follow in reported order.
//! 2. **Retained.** Otherwise the evidence is degenerate (each client names at
//!    most one document, or several clients disagree). A previous layout whose
//!    documents are still open is kept, so a single-selection observation never
//!    collapses a known split to one pane. When the active document is not in
//!    it, the operator switched tabs inside one split: the document takes the
//!    place of the previously focused column (else the rightmost), so the width
//!    is held, never grown — the same rule the controller's ensure merge uses
//!    (GH #120).
//! 3. **Unknown.** No split evidence and no retained layout → no columns. This
//!    stays structurally inert (GH #105): a one-document observation is not a
//!    one-column declaration.

use serde::{Deserialize, Serialize};

use crate::SurfaceColumn;

/// What one Remote Dev client reported, restricted to session documents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteClientEditors {
    /// Session documents whose client editor is currently visible (one per
    /// visible split).
    #[serde(default)]
    pub visible: Vec<String>,
    /// Session documents the client reports as selected. In Remote Dev this is
    /// usually only the focused file.
    #[serde(default)]
    pub selected: Vec<String>,
    /// Session documents open as tabs in this client. Empty means "unknown",
    /// not "nothing open".
    #[serde(default)]
    pub open: Vec<String>,
}

impl RemoteClientEditors {
    /// Visible editors first, then selected files, de-duplicated.
    fn evidence(&self) -> Vec<String> {
        distinct(self.visible.iter().chain(self.selected.iter()))
    }
}

/// One observation of a Remote Dev backend with no local editor windows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutEvidence {
    #[serde(default)]
    pub clients: Vec<RemoteClientEditors>,
    /// Session documents the backend-local `FileEditorManager` reports as
    /// selected (the focused file).
    #[serde(default)]
    pub focused: Vec<String>,
}

/// Where a resolution's columns came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteLayoutSource {
    /// A single client's visible editors named two or more documents.
    RemoteClientVisibleEditors,
    /// A single client's selected files named two or more documents.
    RemoteClientSelectedFiles,
    /// Degenerate evidence; the previous layout was kept (width held).
    RetainedRemoteColumns,
    /// No split evidence and nothing to retain.
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

/// The columns to publish, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutResolution {
    /// Empty only for [`RemoteLayoutSource::Unknown`].
    pub columns: Vec<SurfaceColumn>,
    pub source: RemoteLayoutSource,
    /// Machine-readable explanation for retained/unknown answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The previous resolution a Remote Dev backend folds the next observation
/// against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutMemory {
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
    #[serde(default)]
    pub focused: Option<String>,
}

impl RemoteLayoutMemory {
    /// Fold one observation, returning the next memory and the resolution.
    pub fn advance(&self, evidence: &RemoteLayoutEvidence) -> (Self, RemoteLayoutResolution) {
        let active = active_document(evidence);
        let resolution = self.resolve(evidence, active.as_deref());
        let next = Self {
            columns: resolution.columns.clone(),
            focused: active.or_else(|| self.focused.clone()),
        };
        (next, resolution)
    }

    fn resolve(
        &self,
        evidence: &RemoteLayoutEvidence,
        active: Option<&str>,
    ) -> RemoteLayoutResolution {
        // Rule 1: one client's split evidence.
        let mut candidates: Vec<(Vec<String>, RemoteLayoutSource)> = Vec::new();
        for client in &evidence.clients {
            let files = client.evidence();
            if files.len() < 2 {
                continue;
            }
            let source = if distinct(client.visible.iter()).len() >= 2 {
                RemoteLayoutSource::RemoteClientVisibleEditors
            } else {
                RemoteLayoutSource::RemoteClientSelectedFiles
            };
            if !candidates.iter().any(|(seen, _)| same_set(seen, &files)) {
                candidates.push((files, source));
            }
        }
        let ambiguous = candidates.len() > 1;
        if let [(files, source)] = candidates.as_slice() {
            return RemoteLayoutResolution {
                columns: self.ordered_columns(files),
                source: *source,
                reason: None,
            };
        }

        // Rule 2: retain the previous layout, width held.
        let open = open_documents(evidence);
        let mut retained: Vec<SurfaceColumn> = self
            .columns
            .iter()
            .map(|column| {
                SurfaceColumn::new(
                    column
                        .files
                        .iter()
                        .filter(|file| open.as_ref().is_none_or(|open| open.contains(file)))
                        .cloned(),
                )
            })
            .filter(|column| !column.files.is_empty())
            .collect();
        if !retained.is_empty() {
            let reason = match active {
                Some(active) if !covers(&retained, active) => {
                    let index = self
                        .focused
                        .as_deref()
                        .and_then(|focused| column_of(&retained, focused))
                        .unwrap_or(retained.len() - 1);
                    retained[index] = SurfaceColumn::new([active.to_string()]);
                    "tab_switch_replaced_focus_column"
                }
                _ if ambiguous => "ambiguous_remote_clients",
                _ => "single_selection_within_retained_layout",
            };
            return RemoteLayoutResolution {
                columns: retained,
                source: RemoteLayoutSource::RetainedRemoteColumns,
                reason: Some(reason.to_string()),
            };
        }

        // Rule 3: honest unknown.
        RemoteLayoutResolution {
            columns: Vec::new(),
            source: RemoteLayoutSource::Unknown,
            reason: Some(
                if ambiguous {
                    "ambiguous_remote_clients"
                } else {
                    "no_split_evidence"
                }
                .to_string(),
            ),
        }
    }

    /// One column per file; files from the previous layout keep its order.
    fn ordered_columns(&self, files: &[String]) -> Vec<SurfaceColumn> {
        let previous: Vec<&String> = self.columns.iter().flat_map(|c| c.files.iter()).collect();
        let mut ordered: Vec<&String> = previous
            .iter()
            .copied()
            .filter(|file| files.contains(file))
            .collect();
        ordered.dedup();
        for file in files {
            if !ordered.contains(&file) {
                ordered.push(file);
            }
        }
        ordered
            .into_iter()
            .map(|file| SurfaceColumn::new([file.clone()]))
            .collect()
    }
}

/// The document the operator is on: a lone client evidence file, else the
/// backend-local focused file.
fn active_document(evidence: &RemoteLayoutEvidence) -> Option<String> {
    let singles = distinct(
        evidence
            .clients
            .iter()
            .filter_map(|client| match client.evidence().as_slice() {
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

/// Every open session document, or `None` when no client reported tabs.
fn open_documents(evidence: &RemoteLayoutEvidence) -> Option<Vec<String>> {
    if evidence.clients.iter().all(|client| client.open.is_empty()) {
        return None;
    }
    Some(distinct(evidence.clients.iter().flat_map(|client| {
        client
            .open
            .iter()
            .chain(client.visible.iter())
            .chain(client.selected.iter())
    })))
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

fn same_set(left: &[String], right: &[String]) -> bool {
    left.len() == right.len() && left.iter().all(|file| right.contains(file))
}

fn covers(columns: &[SurfaceColumn], file: &str) -> bool {
    column_of(columns, file).is_some()
}

fn column_of(columns: &[SurfaceColumn], file: &str) -> Option<usize> {
    columns
        .iter()
        .position(|column| column.files.iter().any(|candidate| candidate == file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(visible: &[&str], selected: &[&str], open: &[&str]) -> RemoteClientEditors {
        let owned = |files: &[&str]| files.iter().map(|f| f.to_string()).collect();
        RemoteClientEditors {
            visible: owned(visible),
            selected: owned(selected),
            open: owned(open),
        }
    }

    fn evidence(clients: Vec<RemoteClientEditors>, focused: &[&str]) -> RemoteLayoutEvidence {
        RemoteLayoutEvidence {
            clients,
            focused: focused.iter().map(|f| f.to_string()).collect(),
        }
    }

    fn columns(resolution: &RemoteLayoutResolution) -> Vec<Vec<String>> {
        resolution
            .columns
            .iter()
            .map(|column| column.files.clone())
            .collect()
    }

    const A: &str = "tasks/pmt2/mr/1061.md";
    const B: &str = "tasks/agent-doc/agent-doc.ad.md";
    const C: &str = "tasks/c.md";

    /// GH #134: the live Remote Dev shape — one client, one selected file, no
    /// local windows — with the frontend's two visible splits reported by the
    /// editor tracker yields two columns, not `unknown`.
    #[test]
    fn gh134_remote_client_visible_editors_yield_two_columns() {
        let memory = RemoteLayoutMemory::default();
        let observed = evidence(vec![client(&[A, B], &[A], &[A, B])], &[A]);
        let (_, resolution) = memory.advance(&observed);
        assert_eq!(
            resolution.source,
            RemoteLayoutSource::RemoteClientVisibleEditors
        );
        assert_eq!(
            columns(&resolution),
            vec![vec![A.to_string()], vec![B.to_string()]]
        );
    }

    /// GH #134: the alternating single-selection observations from the issue
    /// (obs=51 A, obs=53 B, obs=55 A ...) never collapse a known split to one
    /// pane and never hand the controller zero columns.
    #[test]
    fn gh134_alternating_single_selections_retain_the_known_split() {
        let (mut memory, first) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[A, B])], &[A]));
        assert_eq!(first.columns.len(), 2);
        for (selected, step) in [(B, 53), (A, 55), (B, 57)] {
            // Visibility evidence degenerate (e.g. a stale tracker), selection alternates.
            let (next, resolution) = memory.advance(&evidence(
                vec![client(&[], &[selected], &[A, B])],
                &[selected],
            ));
            assert_eq!(
                resolution.source,
                RemoteLayoutSource::RetainedRemoteColumns,
                "obs={step}"
            );
            assert_eq!(
                columns(&resolution),
                vec![vec![A.to_string()], vec![B.to_string()]],
                "obs={step}: a single selection inside the split must keep both columns"
            );
            assert_eq!(
                resolution.reason.as_deref(),
                Some("single_selection_within_retained_layout")
            );
            memory = next;
        }
    }

    /// A tab switch to a document outside the retained split replaces the
    /// focused column: width held, never grown (GH #120 invariant).
    #[test]
    fn gh134_tab_switch_outside_split_replaces_focus_column_without_growth() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[B], &[A, B, C])], &[B]));
        let (_, resolution) = memory.advance(&evidence(vec![client(&[], &[C], &[A, B, C])], &[C]));
        assert_eq!(resolution.source, RemoteLayoutSource::RetainedRemoteColumns);
        assert_eq!(
            columns(&resolution),
            vec![vec![A.to_string()], vec![C.to_string()]],
            "C takes the place of B, the previously focused column"
        );
        assert_eq!(
            resolution.reason.as_deref(),
            Some("tab_switch_replaced_focus_column")
        );
    }

    /// A closed tab leaves the retained layout.
    #[test]
    fn gh134_retained_columns_drop_closed_documents() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[A, B])], &[A]));
        let (_, resolution) = memory.advance(&evidence(vec![client(&[], &[A], &[A])], &[A]));
        assert_eq!(columns(&resolution), vec![vec![A.to_string()]]);
    }

    /// The previous selected-files rule still detects a split.
    #[test]
    fn gh134_one_client_selecting_two_files_is_still_detected() {
        let (_, resolution) =
            RemoteLayoutMemory::default().advance(&evidence(vec![client(&[], &[A, B], &[])], &[A]));
        assert_eq!(
            resolution.source,
            RemoteLayoutSource::RemoteClientSelectedFiles
        );
        assert_eq!(resolution.columns.len(), 2);
    }

    /// Cold start with a single document and no split evidence stays inert
    /// (GH #105): no fabricated one-column declaration.
    #[test]
    fn gh134_cold_single_selection_without_split_evidence_stays_unknown() {
        let (memory, resolution) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A], &[A], &[A, B])], &[A]));
        assert_eq!(resolution.source, RemoteLayoutSource::Unknown);
        assert!(resolution.columns.is_empty());
        assert_eq!(resolution.reason.as_deref(), Some("no_split_evidence"));
        assert_eq!(memory.focused.as_deref(), Some(A));
    }

    /// Separate clients' single files are never combined into a split.
    #[test]
    fn gh134_two_clients_with_one_file_each_are_not_a_split() {
        let (_, resolution) = RemoteLayoutMemory::default().advance(&evidence(
            vec![client(&[A], &[A], &[]), client(&[B], &[B], &[])],
            &[A],
        ));
        assert_eq!(resolution.source, RemoteLayoutSource::Unknown);
    }

    /// Two clients disagreeing on their splits is ambiguous; a known layout is
    /// retained rather than picking one client's.
    #[test]
    fn gh134_disagreeing_clients_retain_previous_layout() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[])], &[A]));
        let (_, resolution) = memory.advance(&evidence(
            vec![client(&[A, B], &[A], &[]), client(&[A, C], &[C], &[])],
            &[A],
        ));
        assert_eq!(resolution.source, RemoteLayoutSource::RetainedRemoteColumns);
        assert_eq!(
            resolution.reason.as_deref(),
            Some("ambiguous_remote_clients")
        );
        assert_eq!(
            columns(&resolution),
            vec![vec![A.to_string()], vec![B.to_string()]]
        );
    }

    /// A re-detected split keeps the previous column order, so a frontend that
    /// reports visible editors in a different order does not reorder tmux.
    #[test]
    fn gh134_redetected_split_keeps_previous_column_order() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[])], &[A]));
        let (_, resolution) = memory.advance(&evidence(vec![client(&[C, B, A], &[B], &[])], &[B]));
        assert_eq!(
            columns(&resolution),
            vec![
                vec![A.to_string()],
                vec![B.to_string()],
                vec![C.to_string()]
            ]
        );
    }

    /// End to end through the surface fold: a resolved Remote Dev split drives
    /// a structural two-column `Sync` (the controller is no longer handed zero
    /// columns), exactly like a desktop observation with the same columns; an
    /// unresolved one stays focus-only (GH #105 unchanged).
    #[test]
    fn gh134_resolved_remote_columns_drive_the_same_sync_as_desktop() {
        use crate::{EditorSurface, SurfaceIntent, SurfaceTracking};
        let surface = |columns: Vec<SurfaceColumn>| EditorSurface {
            focused: A.to_string(),
            visible: vec![A.to_string(), B.to_string()],
            open: vec![A.to_string(), B.to_string()],
            columns,
            ..EditorSurface::default()
        };
        let (_, remote) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[A, B])], &[A]));
        let desktop = vec![SurfaceColumn::new([A]), SurfaceColumn::new([B])];
        let (_, remote_intent) = SurfaceTracking::default().advance(&surface(remote.columns), None);
        let (_, desktop_intent) = SurfaceTracking::default().advance(&surface(desktop), None);
        assert_eq!(remote_intent, desktop_intent);
        match remote_intent {
            SurfaceIntent::Sync { columns, .. } => assert_eq!(columns.len(), 2),
            other => panic!("expected a structural sync, got {other:?}"),
        }

        let (_, unknown) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[], &[A], &[A, B])], &[A]));
        let (_, unknown_intent) =
            SurfaceTracking::default().advance(&surface(unknown.columns), None);
        assert!(
            matches!(unknown_intent, SurfaceIntent::Focus { .. }),
            "{unknown_intent:?}"
        );
    }

    #[test]
    fn evidence_roundtrips_plugin_json() {
        let json =
            r#"{"clients":[{"visible":["a.md","b.md"],"selected":["a.md"]}],"focused":["a.md"]}"#;
        let parsed: RemoteLayoutEvidence = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.clients[0].open, Vec::<String>::new());
        let (_, resolution) = RemoteLayoutMemory::default().advance(&parsed);
        let out = serde_json::to_value(&resolution).unwrap();
        assert_eq!(out["source"], "remote_client_visible_editors");
        assert_eq!(out["columns"][1]["files"][0], "b.md");
    }
}
