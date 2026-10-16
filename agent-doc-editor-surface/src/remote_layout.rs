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
//! editor tracker records frontend text editors whose visibility the client
//! reported. Usually that is one per visible split, but compound editors such
//! as JetBrains' "Editor and Preview" can leave the hidden tab's text editor in
//! that set after a tab switch (GH #175). The globally selected file therefore
//! identifies focus, not another split, and cannot make an established layout
//! wider by itself.
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
//! 1. **Detected.** Exactly one distinct client visible set names two or more
//!    session documents → one column per document. A multi-file selected set
//!    remains a compatibility fallback, but selected is never unioned into a
//!    visible set. Column order is stable: documents already in the previous
//!    layout keep their order, new ones follow in reported order. If the only
//!    added visible document is the globally selected one, an established
//!    width is held because that observation is equally explained by a hidden
//!    compound-editor tab (GH #175).
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
    /// Structural split evidence. Visible and selected are deliberately not
    /// unioned: Remote Dev selected files report focus, and adding that focus
    /// to a lagging/stale visible set fabricates a column on a tab switch.
    fn split_evidence(&self) -> Option<(Vec<String>, RemoteLayoutSource)> {
        let visible = distinct(self.visible.iter());
        if visible.len() >= 2 {
            return Some((visible, RemoteLayoutSource::RemoteClientVisibleEditors));
        }
        let selected = distinct(self.selected.iter());
        (selected.len() >= 2).then_some((selected, RemoteLayoutSource::RemoteClientSelectedFiles))
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

/// How much left-to-right authority the Remote Dev fold has for its columns.
///
/// Remote client collections expose membership and tab-open order, not split
/// geometry. A cold observation is therefore `Unknown`; once the fold preserves
/// surviving columns or replaces a document in a remembered slot, the resulting
/// order is `Retained`.
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

/// The previous resolution a Remote Dev backend folds the next observation
/// against.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLayoutMemory {
    #[serde(default)]
    pub columns: Vec<SurfaceColumn>,
    #[serde(default)]
    pub focused: Option<String>,
    /// Columns from before a selection-only observation guessed which split
    /// changed. The next structural visible-set observation folds against this
    /// base instead of treating the guess as authoritative geometry (GH #250).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisional_base_columns: Option<Vec<SurfaceColumn>>,
    /// The visible set of the immediately preceding selection-only
    /// observation while a guess is outstanding. The SAME set observed again
    /// at once has persisted, so it is a real split (GH #253), not a lagging
    /// tracker; any other visible set is judged against the confirmed layout
    /// (`#tmuxpanesreversed`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisional_visible: Option<Vec<String>>,
    /// The left-to-right order of `columns` was never anchored to the order
    /// authority (`#tmuxpanesreversed`). A cold fold (empty memory after a
    /// native reload, an IDE restart, or a first start) takes the visible
    /// list's TAB-OPEN order, which is not geometry. Every later fold keeps
    /// survivors in that order, so its output inherits the same lack of
    /// authority: while this is set, every resolution is labelled
    /// [`RemoteLayoutColumnOrder::Unknown`] (the controller then orders it
    /// against the layout tmux already shows) instead of upgrading a guessed
    /// order to `Retained`. [`RemoteLayoutMemory::anchor`] clears it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unanchored: bool,
}

impl RemoteLayoutMemory {
    /// Fold one observation, returning the next memory and the resolution.
    pub fn advance(&self, evidence: &RemoteLayoutEvidence) -> (Self, RemoteLayoutResolution) {
        let active = active_document(evidence);
        let mut resolution = self.resolve(evidence, active.as_deref());
        // Order provenance never upgrades (`#tmuxpanesreversed`): a fold over
        // an unanchored memory, or a cold fold with no survivor, has only
        // tab-open order to offer.
        let unanchored = if resolution.columns.is_empty() {
            self.unanchored
        } else {
            self.unanchored
                || (resolution.column_order == RemoteLayoutColumnOrder::Unknown
                    && resolution.columns.len() >= 2)
        };
        if unanchored && !resolution.columns.is_empty() {
            resolution.column_order = RemoteLayoutColumnOrder::Unknown;
        }
        let provisional_base_columns = match (resolution.source, resolution.reason.as_deref()) {
            (
                RemoteLayoutSource::RetainedRemoteColumns,
                Some(
                    "selected_visible_change_replaced_focus_column"
                    | "tab_switch_replaced_focus_column",
                ),
            ) => self
                .provisional_base_columns
                .clone()
                .or_else(|| Some(self.columns.clone())),
            (
                RemoteLayoutSource::RemoteClientVisibleEditors
                | RemoteLayoutSource::RemoteClientSelectedFiles,
                _,
            ) => None,
            _ => self.provisional_base_columns.clone(),
        };
        // Only the IMMEDIATELY preceding selection-only observation counts:
        // GH #253's "the same evidence again" is consecutive persistence.
        let provisional_visible = match resolution.reason.as_deref() {
            _ if provisional_base_columns.is_none() => None,
            Some(
                "selected_visible_change_replaced_focus_column"
                | "selected_visible_change_width_held",
            ) => single_split_candidate(evidence),
            _ => None,
        };
        let next = Self {
            columns: resolution.columns.clone(),
            focused: active.or_else(|| self.focused.clone()),
            provisional_base_columns,
            provisional_visible,
            unanchored,
        };
        (next, resolution)
    }

    /// Anchor this memory's order to the order authority (`#tmuxpanesreversed`).
    ///
    /// `authority` is the layout the Project Controller retains and tmux
    /// shows, as column file lists in this memory's path space. Remote Dev
    /// has no geometry, so that layout is the only order authority; the fold
    /// keeps membership and its slot logic, but must never hold the shared
    /// documents in a different relative order than the authority.
    ///
    /// - Empty memory: seeded with the authority's columns, so the next
    ///   observation is a slot fold against what tmux shows instead of a
    ///   cold tab-open order.
    /// - Otherwise: the slots that hold authority documents are refilled
    ///   with those documents in authority order (a slot-preserving
    ///   permutation; other slots and the membership are unchanged), for the
    ///   current columns and the provisional base alike.
    ///
    /// The result is anchored (`unanchored == false`) when every memory
    /// document appears in the authority or the memory was seeded. An empty
    /// authority changes nothing. Pure and idempotent.
    pub fn anchor(&self, authority: &[SurfaceColumn]) -> Self {
        let authority_files = distinct(authority.iter().flat_map(|column| column.files.iter()));
        if authority_files.is_empty() {
            return self.clone();
        }
        if self.columns.is_empty() {
            return Self {
                columns: authority
                    .iter()
                    .filter(|column| !column.files.is_empty())
                    .cloned()
                    .collect(),
                focused: self.focused.clone(),
                provisional_base_columns: None,
                provisional_visible: None,
                unanchored: false,
            };
        }
        let permute = |columns: &[SurfaceColumn]| -> Vec<SurfaceColumn> {
            let rank = |column: &SurfaceColumn| {
                column
                    .files
                    .iter()
                    .filter_map(|file| authority_files.iter().position(|known| known == file))
                    .min()
            };
            let mut anchored: Vec<&SurfaceColumn> = columns
                .iter()
                .filter(|column| rank(column).is_some())
                .collect();
            anchored.sort_by_key(|column| rank(column));
            let mut refill = anchored.into_iter();
            columns
                .iter()
                .map(|column| match rank(column) {
                    Some(_) => refill.next().cloned().unwrap_or_else(|| column.clone()),
                    None => column.clone(),
                })
                .collect()
        };
        let covered = self
            .columns
            .iter()
            .flat_map(|column| column.files.iter())
            .all(|file| authority_files.contains(file));
        Self {
            columns: permute(&self.columns),
            focused: self.focused.clone(),
            provisional_base_columns: self.provisional_base_columns.as_deref().map(permute),
            provisional_visible: self.provisional_visible.clone(),
            unanchored: self.unanchored && !covered,
        }
    }

    fn resolve(
        &self,
        evidence: &RemoteLayoutEvidence,
        active: Option<&str>,
    ) -> RemoteLayoutResolution {
        // Rule 1: one client's split evidence.
        let mut candidates: Vec<(Vec<String>, RemoteLayoutSource)> = Vec::new();
        for client in &evidence.clients {
            let Some((files, source)) = client.split_evidence() else {
                continue;
            };
            if !candidates.iter().any(|(seen, _)| same_set(seen, &files)) {
                candidates.push((files, source));
            }
        }
        let ambiguous = candidates.len() > 1;
        match candidates.as_slice() {
            [(files, source)] if !self.selection_only_change(files, evidence) => {
                let (columns, column_order) = self.ordered_columns(files);
                return RemoteLayoutResolution {
                    columns,
                    source: *source,
                    column_order,
                    reason: None,
                };
            }
            _ => {}
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
            let selection_only_change = !ambiguous
                && candidates
                    .first()
                    .is_some_and(|(files, _)| self.selection_only_change(files, evidence));
            let reason = match active {
                Some(active) if !covers(&retained, active) => {
                    let index = self
                        .focused
                        .as_deref()
                        .and_then(|focused| column_of(&retained, focused))
                        .unwrap_or(retained.len() - 1);
                    retained[index] = SurfaceColumn::new([active.to_string()]);
                    if selection_only_change {
                        "selected_visible_change_replaced_focus_column"
                    } else {
                        "tab_switch_replaced_focus_column"
                    }
                }
                _ if selection_only_change => "selected_visible_change_width_held",
                _ if ambiguous => "ambiguous_remote_clients",
                _ => "single_selection_within_retained_layout",
            };
            return RemoteLayoutResolution {
                columns: retained,
                source: RemoteLayoutSource::RetainedRemoteColumns,
                column_order: RemoteLayoutColumnOrder::Retained,
                reason: Some(reason.to_string()),
            };
        }

        // Rule 3: honest unknown.
        RemoteLayoutResolution {
            columns: Vec::new(),
            source: RemoteLayoutSource::Unknown,
            column_order: RemoteLayoutColumnOrder::Unknown,
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

    /// One column per file, folded against the previous layout (GH #185, GH #234).
    ///
    /// Remote Dev proves membership, not geometry, so the previous columns are
    /// the only order authority. With `P` the previous files, `V` the new
    /// visible set, survivors `S = P ∩ V`, dropped `D = P \ V` and added
    /// `A = V \ P` (in visible order), the result `O` satisfies:
    ///
    /// 1. `O` holds exactly `V`, one file per column.
    /// 2. Survivors keep their previous relative order.
    /// 3. Added files take the slots dropped files vacated, left to right.
    ///    At the same width every survivor therefore keeps its exact index.
    /// 4. A survivor never crosses the split on a width change: if `P[0]`
    ///    survives it stays first, and if `P` had two or more columns and its
    ///    last file survives, that file stays last. So growth beyond the
    ///    vacated slots inserts the extra added files just before a surviving
    ///    rightmost column, and otherwise appends them; shrinking removes the
    ///    vacated slots that no added file filled.
    /// 5. Added files keep their visible order among themselves.
    /// 6. With no survivors the order is the visible order, reported as
    ///    `Unknown`; otherwise the order is memory-derived and `Retained`.
    fn ordered_columns(&self, files: &[String]) -> (Vec<SurfaceColumn>, RemoteLayoutColumnOrder) {
        // A selection-only observation can identify the active document before
        // Remote Dev reports which visible split changed. Its replacement is a
        // provisional width-preserving guess, not geometry authority. When the
        // structural visible set catches up, recover the slots from before that
        // guess (GH #250).
        let previous_columns = self.confirmed_columns();
        let previous = distinct(
            previous_columns
                .iter()
                .flat_map(|column| column.files.iter()),
        );
        let files = distinct(files.iter());
        let mut added = files
            .iter()
            .filter(|file| !previous.contains(file))
            .cloned();

        // Survivors stay in place; each dropped slot takes the next added
        // file, or stays vacant (`None`) when the added files run out.
        let mut slots: Vec<Option<String>> = previous
            .iter()
            .map(|file| {
                if files.contains(file) {
                    Some(file.clone())
                } else {
                    added.next()
                }
            })
            .collect();
        let survived = slots
            .iter()
            .zip(&previous)
            .any(|(slot, file)| slot.as_ref() == Some(file));
        let extra: Vec<String> = added.collect();
        if !extra.is_empty() {
            let last_survives = previous.len() >= 2
                && previous
                    .last()
                    .is_some_and(|file| slots.last() == Some(&Some(file.clone())));
            let at = if last_survives {
                slots.len() - 1
            } else {
                slots.len()
            };
            slots.splice(at..at, extra.into_iter().map(Some));
        }
        let ordered: Vec<String> = slots.into_iter().flatten().collect();
        (
            ordered
                .into_iter()
                .map(|file| SurfaceColumn::new([file]))
                .collect(),
            if survived {
                RemoteLayoutColumnOrder::Retained
            } else {
                RemoteLayoutColumnOrder::Unknown
            },
        )
    }

    /// GH #175: `EditorTracker.activeEditors` may retain the hidden text half
    /// of an Editor/Preview tab. If a known layout's only new visible member is
    /// also the one globally selected file, the observation proves a focus
    /// change but not a new split. Route it through rule 2 so the focused
    /// column is replaced and width is held.
    fn selection_only_change(&self, files: &[String], evidence: &RemoteLayoutEvidence) -> bool {
        // `#tmuxpanesreversed`: judge against the last CONFIRMED layout, never
        // a provisional guess. A guess is a conclusion, not a premise: judging
        // a lagging observation against it let a stale hidden editor (GH #175)
        // read as a real extra split, consume the pre-guess base, and leave
        // the corrected document on the wrong side.
        let confirmed = self.confirmed_columns();
        if confirmed.is_empty() {
            return false;
        }
        let selected = distinct(
            evidence
                .clients
                .iter()
                .flat_map(|client| client.selected.iter()),
        );
        let [selected] = selected.as_slice() else {
            return false;
        };
        // The tracker has not caught up: the only selected file is absent from
        // its otherwise-authoritative visible set.
        if !files.contains(selected) {
            return true;
        }
        // GH #253: the visible set that produced the outstanding guess has now
        // been observed again. Evidence that persists is a real split.
        if self
            .provisional_visible
            .as_ref()
            .is_some_and(|seen| same_set(seen, files))
        {
            return false;
        }
        if files.len() <= confirmed.len() {
            return false;
        }
        let added: Vec<&String> = files
            .iter()
            .filter(|file| !covers(confirmed, file))
            .collect();
        added.as_slice() == [selected]
    }

    /// The last layout a structural visible-set observation confirmed: the
    /// pre-guess base while a provisional guess is outstanding, else the
    /// current columns. Every inference folds against this, never a guess.
    fn confirmed_columns(&self) -> &[SurfaceColumn] {
        self.provisional_base_columns
            .as_deref()
            .unwrap_or(&self.columns)
    }
}

/// The one distinct client split-evidence set, if exactly one client offers
/// split evidence (the rule 1 candidate).
fn single_split_candidate(evidence: &RemoteLayoutEvidence) -> Option<Vec<String>> {
    let mut candidates: Vec<Vec<String>> = Vec::new();
    for client in &evidence.clients {
        if let Some((files, _)) = client.split_evidence()
            && !candidates.iter().any(|seen| same_set(seen, &files))
        {
            candidates.push(files);
        }
    }
    match <[Vec<String>; 1]>::try_from(candidates) {
        Ok([only]) => Some(only),
        Err(_) => None,
    }
}

/// The document the operator is on: one selected client file, else the
/// backend-local focused file. Visible is structural evidence, not focus.
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

    /// GH #278 Ask 2: a no-survivor guess stays `Unknown`. From empty memory,
    /// folding `visible={x, y}` twice must not turn the tab-open order into
    /// `Retained` on the second (identical) fold: the memory carries the label.
    #[test]
    fn gh278_an_identical_refold_of_a_no_survivor_guess_stays_unknown() {
        let observed = evidence(vec![client(&[B, C], &[C], &[A, B, C])], &[C]);
        let (memory, first) = RemoteLayoutMemory::default().advance(&observed);
        assert_eq!(first.column_order, RemoteLayoutColumnOrder::Unknown);
        let (memory, second) = memory.advance(&observed);
        assert_eq!(second.column_order, RemoteLayoutColumnOrder::Unknown);
        assert_eq!(columns(&second), columns(&first));
        // GH #278's own shape: a one-column memory holding neither visible
        // document (`[<doc-d>]`), then the claim's two identical folds.
        let held = RemoteLayoutMemory {
            columns: vec![SurfaceColumn::new(["tasks/d.md"])],
            focused: Some("tasks/d.md".to_string()),
            ..RemoteLayoutMemory::default()
        };
        let (held, first) = held.advance(&observed);
        assert_eq!(first.column_order, RemoteLayoutColumnOrder::Unknown);
        let (_, second) = held.advance(&observed);
        assert_eq!(second.column_order, RemoteLayoutColumnOrder::Unknown);
        // Anchored to the controller's retained layout, it is an order again.
        let (anchored, _) = memory
            .anchor(&[SurfaceColumn::new([C]), SurfaceColumn::new([B])])
            .advance(&observed);
        assert!(!anchored.unanchored);
        assert_eq!(
            anchored.columns,
            vec![SurfaceColumn::new([C]), SurfaceColumn::new([B])]
        );
    }

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
        assert_eq!(resolution.column_order, RemoteLayoutColumnOrder::Unknown);
        assert_eq!(
            columns(&resolution),
            vec![vec![A.to_string()], vec![B.to_string()]]
        );
    }

    /// GH #185: Remote Dev exposes membership but not split geometry. When one
    /// visible document replaces another at the same retained width, the new
    /// document inherits the dropped document's slot instead of appending.
    #[test]
    fn gh185_same_width_replacement_inherits_the_dropped_column_slot() {
        let memory = RemoteLayoutMemory {
            columns: vec![SurfaceColumn::new([C]), SurfaceColumn::new([B])],
            focused: Some(C.to_string()),
            provisional_base_columns: None,
            provisional_visible: None,
            unanchored: false,
        };
        let (_, resolution) = memory.advance(&evidence(vec![client(&[A, B], &[A], &[A, B])], &[A]));

        assert_eq!(
            resolution.source,
            RemoteLayoutSource::RemoteClientVisibleEditors
        );
        assert_eq!(resolution.column_order, RemoteLayoutColumnOrder::Retained);
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

    /// GH #175: selected is focus evidence, not another split. While the
    /// tracker still names the old two visible tabs, selecting an already-open
    /// third tab replaces the focused column instead of appending a third.
    #[test]
    fn gh175_selected_file_does_not_extend_visible_split_set() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[A, B, C])], &[A]));
        let (memory, resolution) =
            memory.advance(&evidence(vec![client(&[A, B], &[C], &[A, B, C])], &[C]));
        assert_eq!(resolution.source, RemoteLayoutSource::RetainedRemoteColumns);
        assert_eq!(resolution.columns.len(), 2);
        assert_eq!(
            columns(&resolution),
            vec![vec![C.to_string()], vec![B.to_string()]]
        );
        assert_eq!(
            resolution.reason.as_deref(),
            Some("selected_visible_change_replaced_focus_column")
        );

        // GH #250: selection arrives before Remote Dev's visible set. The
        // replacement above guessed that C replaced A, but the next structural
        // observation proves that C replaced B. Fold that correction against
        // the pre-guess [A, B] layout, not the provisional [C, B] result.
        let (memory, resolution) =
            memory.advance(&evidence(vec![client(&[A, C], &[C], &[A, B, C])], &[C]));
        assert_eq!(
            resolution.source,
            RemoteLayoutSource::RemoteClientVisibleEditors
        );
        assert_eq!(
            columns(&resolution),
            vec![vec![A.to_string()], vec![C.to_string()]]
        );
        assert!(memory.provisional_base_columns.is_none());
    }

    /// GH #175 reproduction: one established split has an Editor/Preview tab
    /// A. After switching that split to B, Remote Dev can transiently report
    /// both text editors as visible. B is selected, so this is a tab switch,
    /// not proof of a second split; the established width remains one.
    #[test]
    fn gh175_editor_preview_tab_switch_holds_one_column() {
        let memory = RemoteLayoutMemory {
            columns: vec![SurfaceColumn::new([A])],
            focused: Some(A.to_string()),
            provisional_base_columns: None,
            provisional_visible: None,
            unanchored: false,
        };
        let (_, resolution) = memory.advance(&evidence(vec![client(&[A, B], &[B], &[A, B])], &[B]));
        assert_eq!(resolution.source, RemoteLayoutSource::RetainedRemoteColumns);
        assert_eq!(columns(&resolution), vec![vec![B.to_string()]]);
        assert_eq!(
            resolution.reason.as_deref(),
            Some("selected_visible_change_replaced_focus_column")
        );
    }

    /// GH #253: the GH #175 one-column replacement is provisional too. If the
    /// same two-file visible set arrives again as structural evidence, order it
    /// against the pre-guess [A] ancestry instead of the provisional [B].
    #[test]
    fn gh253_repeated_visible_set_restores_one_column_provisional_base_order() {
        let memory = RemoteLayoutMemory {
            columns: vec![SurfaceColumn::new([A])],
            focused: Some(A.to_string()),
            provisional_base_columns: None,
            provisional_visible: None,
            unanchored: false,
        };

        let (memory, provisional) =
            memory.advance(&evidence(vec![client(&[A, B], &[B], &[A, B])], &[B]));
        assert_eq!(columns(&provisional), vec![vec![B.to_string()]]);
        assert_eq!(
            memory.provisional_base_columns,
            Some(vec![SurfaceColumn::new([A])])
        );

        let (memory, corrected) =
            memory.advance(&evidence(vec![client(&[A, B], &[B], &[A, B])], &[B]));
        assert_eq!(
            corrected.source,
            RemoteLayoutSource::RemoteClientVisibleEditors
        );
        assert_eq!(
            columns(&corrected),
            vec![vec![A.to_string()], vec![B.to_string()]]
        );
        assert!(memory.provisional_base_columns.is_none());
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
    /// The new column opens between the survivors: the previous rightmost
    /// column `B` stays rightmost (GH #234).
    #[test]
    fn gh134_redetected_split_keeps_previous_column_order() {
        let (memory, _) = RemoteLayoutMemory::default()
            .advance(&evidence(vec![client(&[A, B], &[A], &[])], &[A]));
        let (_, resolution) = memory.advance(&evidence(vec![client(&[C, B, A], &[B], &[])], &[B]));
        assert_eq!(
            columns(&resolution),
            vec![
                vec![A.to_string()],
                vec![C.to_string()],
                vec![B.to_string()]
            ]
        );
    }

    /// GH #234: the issue's `idea.log` trace. A 2->3->2 width change used to
    /// put the lone survivor `b` (the right split) in slot 0, and later
    /// slot-keeping replacements carried the swap forward until the IDE's
    /// `[a | b]` was published as `[b | a]`. `b` must stay rightmost.
    #[test]
    fn gh234_width_change_keeps_the_right_survivor_right() {
        let steps: [(&[&str], &[&str]); 8] = [
            (&["a", "b"], &["a", "b"]),
            (&["a", "b"], &["a", "b"]),
            (&["b", "c"], &["c", "b"]),
            (&["d", "b", "e"], &["d", "e", "b"]),
            (&["d", "f", "b"], &["d", "f", "b"]),
            (&["g", "b"], &["g", "b"]),
            (&["d", "b"], &["d", "b"]),
            (&["a", "b"], &["a", "b"]),
        ];
        let mut memory = RemoteLayoutMemory::default();
        for (step, (visible, expected)) in steps.into_iter().enumerate() {
            let (next, resolution) =
                memory.advance(&evidence(vec![client(visible, &["b"], visible)], &["b"]));
            assert_eq!(
                resolution.source,
                RemoteLayoutSource::RemoteClientVisibleEditors,
                "step {step}"
            );
            assert_eq!(
                columns(&resolution),
                expected
                    .iter()
                    .map(|file| vec![file.to_string()])
                    .collect::<Vec<_>>(),
                "step {step}: visible={visible:?}"
            );
            memory = next;
        }
    }

    mod gh234_properties {
        use super::super::*;
        use proptest::prelude::*;

        fn fold(previous: &[String], visible: &[String]) -> (Vec<String>, RemoteLayoutColumnOrder) {
            let memory = RemoteLayoutMemory {
                columns: previous
                    .iter()
                    .map(|file| SurfaceColumn::new([file.clone()]))
                    .collect(),
                focused: None,
                provisional_base_columns: None,
                provisional_visible: None,
                unanchored: false,
            };
            let (columns, order) = memory.ordered_columns(visible);
            (
                columns
                    .into_iter()
                    .map(|column| {
                        let [file]: [String; 1] = column.files.try_into().unwrap();
                        file
                    })
                    .collect(),
                order,
            )
        }

        /// Distinct file names drawn from a small universe so survivors,
        /// departures and arrivals all occur often.
        fn layout(max: usize) -> impl Strategy<Value = Vec<String>> {
            proptest::sample::subsequence((0..8).collect::<Vec<u8>>(), 0..=max)
                .prop_shuffle()
                .prop_map(|ids| ids.into_iter().map(|id| format!("doc-{id}.md")).collect())
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(2048))]

            /// The `ordered_columns` invariant over arbitrary membership
            /// changes, `(previous columns, new visible set) -> columns`.
            #[test]
            fn survivors_keep_relative_order_and_sides(
                previous in layout(5),
                visible in layout(5),
            ) {
                let (ordered, order) = fold(&previous, &visible);
                let survivors: Vec<&String> =
                    previous.iter().filter(|file| visible.contains(file)).collect();
                let added: Vec<&String> =
                    visible.iter().filter(|file| !previous.contains(file)).collect();

                // 1. Exactly the visible set, one file per column.
                prop_assert_eq!(ordered.len(), visible.len());
                prop_assert!(visible.iter().all(|file| ordered.contains(file)));

                // 2. Survivors keep their previous relative order.
                let kept: Vec<&String> =
                    ordered.iter().filter(|file| previous.contains(file)).collect();
                prop_assert_eq!(&kept, &survivors);

                // 3. Same width: every survivor keeps its exact slot.
                if previous.len() == visible.len() {
                    for (index, file) in previous.iter().enumerate() {
                        if visible.contains(file) {
                            prop_assert_eq!(&ordered[index], file);
                        }
                    }
                }

                // 4. No survivor crosses the split on a width change.
                if let Some(first) = previous.first().filter(|f| visible.contains(f)) {
                    prop_assert_eq!(ordered.first(), Some(first));
                }
                if previous.len() >= 2
                    && let Some(last) = previous.last().filter(|f| visible.contains(f))
                {
                    prop_assert_eq!(ordered.last(), Some(last));
                }

                // 5. Added files keep their visible order.
                let arrivals: Vec<&String> =
                    ordered.iter().filter(|file| !previous.contains(file)).collect();
                prop_assert_eq!(&arrivals, &added);

                // 6. Order authority: memory-derived iff something survived.
                if survivors.is_empty() {
                    prop_assert_eq!(order, RemoteLayoutColumnOrder::Unknown);
                    prop_assert_eq!(&ordered, &visible);
                } else {
                    prop_assert_eq!(order, RemoteLayoutColumnOrder::Retained);
                }
            }

            /// Across a whole sequence of split observations, a survivor's
            /// side never flips: not relative to another survivor, and not
            /// relative to the split's edges (the GH #234 swap kept pairwise
            /// order but moved the right survivor to slot 0).
            #[test]
            fn survivor_pairs_never_swap_across_a_sequence(
                steps in proptest::collection::vec(layout(4), 1..8),
            ) {
                let mut previous: Vec<String> = Vec::new();
                for visible in steps {
                    if visible.len() < 2 {
                        continue;
                    }
                    let (ordered, _) = fold(&previous, &visible);
                    let position =
                        |list: &[String], file: &String| list.iter().position(|f| f == file);
                    for left in &previous {
                        for right in &previous {
                            if let (Some(pl), Some(pr), Some(ol), Some(or)) = (
                                position(&previous, left),
                                position(&previous, right),
                                position(&ordered, left),
                                position(&ordered, right),
                            ) {
                                prop_assert_eq!(pl < pr, ol < or);
                            }
                        }
                    }
                    if let Some(first) = previous.first().filter(|f| visible.contains(f)) {
                        prop_assert_eq!(ordered.first(), Some(first));
                    }
                    if previous.len() >= 2
                        && let Some(last) = previous.last().filter(|f| visible.contains(f))
                    {
                        prop_assert_eq!(ordered.last(), Some(last));
                    }
                    previous = ordered;
                }
            }
        }
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
