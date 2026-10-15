//! `#queuecutexclusive`: a component both admission planes changed takes ONE
//! plane's version, never a body built from both.
//!
//! Preflight's admission merge reconciles three revisions of a live document:
//! the recorded baseline `B`, the live editor authority `A`, and the durable
//! disk file `D`. While an editor holds the document, agent-doc never writes
//! disk behind the buffer, so `A` and `D` are two cuts of ONE editor history:
//! the editor's own op stream and its native save. A three-way merge that
//! unions them treats one history as two writers. Every admission duplicate
//! since the merge replaced the refusal (2026-09-30 `infra.md`, GH #198,
//! `typingforkdup`, `opcutahead`, and the 2026-10-15 `#gh-dix` -> `#gh-fix`
//! correction) has the same rung sequence, `Cell=equals_authority
//! Semantic=adopted`: the identity rung kept the authority and the line rung
//! unioned the two cuts.
//!
//! The union is wrong whichever cut is newer. A STALE disk cut resurrects a
//! revision the operator already replaced. A NEWER disk cut is worse: the merge
//! writes disk's lines into the CRDT as new items, and the editor's in-flight
//! ops then edit or insert the originals, so the same text lands twice once the
//! ops arrive.
//!
//! The earlier fixes each added a classifier for "is this disk line a revision
//! of that authority line?" (a count bound, a textual prefix test, an op-ledger
//! lineage proof). Each was sound for the shape it was written from and blind
//! to the next. This rule needs no classifier:
//!
//! > CutExclusivity: for every component `C` with `A[C] != B[C]` and
//! > `D[C] != B[C]`, the adopted revision's `C` equals `A[C]`.
//!
//! The authority is the plane the editor's own ops roll forward exactly once,
//! which is the codebase-wide editor-is-truth orientation the identity rung
//! already applies per node. Disk-only components still merge, so independent
//! disk work lands. `formal/tla/AdmissionCutExclusivity.tla` checks the rule,
//! a wedge for the pre-fix ladder with all three heuristic guards on, a reach
//! obligation, and the stated boundary (a disk-only component carrying the
//! editor's in-flight typing, which no text-only rule can tell from
//! independent work).
//!
//! Entry points:
//! - [`project_disk_for_cut_exclusivity`]: the input projection. Disk's body of
//!   every concurrently changed component reverts to the baseline before any
//!   merge rung runs, so no rung can see two versions.
//! - [`merge_mixes_component_versions`]: the fail-closed guard every candidate
//!   must clear. It refuses a candidate whose concurrently changed component is
//!   neither the authority's nor disk's body.
//! - [`project_disk_for_editor_provenance`] and its guard
//!   [`merge_changes_editor_attributed`] (`#keylogrebase`): with a correctly
//!   based, editor-attested op ledger, a component only DISK changed whose body
//!   is a state of that ledger's replay is the editor's in-flight typing, and
//!   keeps the authority's body too. This closes the boundary above whenever
//!   the ledger covers the native save.
//!
//! Bodies are compared in the DURABLE domain (`#admissionmarkersplit`):
//! transient agent-doc markers are transport state, not a writer's text.

use crate::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_element::element::{Component, parse};
use std::collections::HashMap;
use std::ops::Range;

/// A top-level component identified by name and its ordinal among top-level
/// components of the same name (a document may hold several `agent:queue`s).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentKey {
    pub name: String,
    pub ordinal: usize,
}

impl std::fmt::Display for ComponentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.ordinal)
    }
}

/// Result of [`project_disk_for_cut_exclusivity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutExclusiveProjection {
    /// Disk with every concurrently changed component's body reverted to the
    /// baseline body. Equal to the input when nothing was concurrent.
    pub disk_text: String,
    /// The components whose authority version was kept, in document order.
    pub authority_kept: Vec<ComponentKey>,
}

/// Top-level component bodies keyed by (name, ordinal). `None` when the
/// document is not structurally parseable.
fn top_level_bodies(doc: &str) -> Option<HashMap<ComponentKey, Range<usize>>> {
    let components: Vec<Component> = parse(doc).ok()?;
    let mut bodies = HashMap::new();
    let mut ordinals = HashMap::<String, usize>::new();
    let mut enclosing_end = 0usize;
    for component in components {
        // `parse` returns components sorted by `open_start`; one that starts
        // before the previous top-level component closed is nested inside it
        // and is covered by that component's body.
        if component.open_start < enclosing_end {
            continue;
        }
        enclosing_end = component.close_end;
        let ordinal = ordinals.entry(component.name.clone()).or_insert(0);
        bodies.insert(
            ComponentKey {
                name: component.name.clone(),
                ordinal: *ordinal,
            },
            component.open_end..component.close_start,
        );
        *ordinal += 1;
    }
    Some(bodies)
}

fn durable_body(doc: &str, range: &Range<usize>) -> String {
    normalize_transient_agent_doc_markers(&doc[range.clone()])
}

/// Number of top-level components per name.
fn name_counts(bodies: &HashMap<ComponentKey, Range<usize>>) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for key in bodies.keys() {
        *counts.entry(key.name.as_str()).or_insert(0) += 1;
    }
    counts
}

/// A component both planes changed since the baseline, with each plane's body.
struct Concurrent {
    key: ComponentKey,
    authority_durable: String,
    disk_durable: String,
    disk_range: Range<usize>,
    baseline_range: Range<usize>,
}

/// The components that are aligned across all three planes (same name, same
/// top-level count) and that BOTH the authority and disk changed in the
/// durable domain. Components whose occurrence count differs between planes
/// (one was added or removed) cannot be aligned by ordinal and are left to the
/// merge; that is structural work, not two cuts of one body.
fn concurrent_components(baseline: &str, authority: &str, disk: &str) -> Option<Vec<Concurrent>> {
    let base = top_level_bodies(baseline)?;
    let auth = top_level_bodies(authority)?;
    let disk_bodies = top_level_bodies(disk)?;
    let (base_counts, auth_counts, disk_counts) = (
        name_counts(&base),
        name_counts(&auth),
        name_counts(&disk_bodies),
    );
    let mut keys: Vec<&ComponentKey> = base.keys().collect();
    keys.sort();
    let mut concurrent = Vec::new();
    for key in keys {
        let name = key.name.as_str();
        let count = base_counts.get(name).copied();
        if auth_counts.get(name).copied() != count || disk_counts.get(name).copied() != count {
            continue;
        }
        let (Some(base_range), Some(auth_range), Some(disk_range)) =
            (base.get(key), auth.get(key), disk_bodies.get(key))
        else {
            continue;
        };
        let base_durable = durable_body(baseline, base_range);
        let authority_durable = durable_body(authority, auth_range);
        let disk_durable = durable_body(disk, disk_range);
        if authority_durable != base_durable && disk_durable != base_durable {
            concurrent.push(Concurrent {
                key: key.clone(),
                authority_durable,
                disk_durable,
                disk_range: disk_range.clone(),
                baseline_range: base_range.clone(),
            });
        }
    }
    Some(concurrent)
}

/// Input projection: revert disk's body of every component both planes changed
/// to the baseline body, so every merge rung sees only the authority's change
/// there and keeps it. A component both planes changed IDENTICALLY is left
/// alone (there is nothing to choose between).
///
/// Unparseable input is returned unchanged; the fail-closed guard
/// [`merge_mixes_component_versions`] refuses every candidate in that case.
pub fn project_disk_for_cut_exclusivity(
    baseline: &str,
    authority: &str,
    disk: &str,
) -> CutExclusiveProjection {
    let Some(mut concurrent) = concurrent_components(baseline, authority, disk) else {
        return CutExclusiveProjection {
            disk_text: disk.to_string(),
            authority_kept: Vec::new(),
        };
    };
    concurrent.retain(|c| c.authority_durable != c.disk_durable);
    // Splice from the end so earlier byte ranges stay valid.
    concurrent.sort_by_key(|c| std::cmp::Reverse(c.disk_range.start));
    let mut disk_text = disk.to_string();
    for c in &concurrent {
        disk_text.replace_range(c.disk_range.clone(), &baseline[c.baseline_range.clone()]);
    }
    let mut authority_kept: Vec<ComponentKey> = concurrent.into_iter().map(|c| c.key).collect();
    authority_kept.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then(a.name.cmp(&b.name)));
    CutExclusiveProjection {
        disk_text,
        authority_kept,
    }
}

/// Fail-closed guard: `Some(reason)` when `merged` holds a component both
/// planes changed whose body is neither the authority's nor disk's (in the
/// durable domain) -- a body built from both cuts. `disk` is the ORIGINAL disk
/// text, before any projection. Unparseable input or a candidate that lost an
/// aligned component is refused too: the guard cannot prove single-version
/// there, and refusing a rung only means admission proceeds on the authority.
pub fn merge_mixes_component_versions(
    baseline: &str,
    authority: &str,
    disk: &str,
    merged: &str,
) -> Option<String> {
    let Some(concurrent) = concurrent_components(baseline, authority, disk) else {
        return Some("unparseable_input".to_string());
    };
    if concurrent.is_empty() {
        return None;
    }
    let Some(merged_bodies) = top_level_bodies(merged) else {
        return Some("unparseable_candidate".to_string());
    };
    let mixed: Vec<String> = concurrent
        .iter()
        .filter_map(|c| {
            let Some(range) = merged_bodies.get(&c.key) else {
                return Some(format!("{}=missing", c.key));
            };
            let body = durable_body(merged, range);
            (body != c.authority_durable && body != c.disk_durable).then(|| c.key.to_string())
        })
        .collect();
    (!mixed.is_empty()).then(|| mixed.join(","))
}

/// Result of [`project_disk_for_editor_provenance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorProvenanceProjection {
    /// Disk with every provenance-attributed component's body reverted to the
    /// baseline body. Equal to the input when nothing was attributed.
    pub disk_text: String,
    /// Disk-only components whose disk body is a state of the editor's own
    /// verified op lineage, in document order.
    pub editor_attributed: Vec<ComponentKey>,
}

/// A component only disk changed since the baseline.
struct DiskOnly {
    key: ComponentKey,
    disk_raw: String,
    disk_durable: String,
    disk_range: Range<usize>,
    baseline_range: Range<usize>,
}

/// Components aligned across the three planes that disk changed and the
/// authority did not (in the durable domain).
fn disk_only_components(baseline: &str, authority: &str, disk: &str) -> Option<Vec<DiskOnly>> {
    let base = top_level_bodies(baseline)?;
    let auth = top_level_bodies(authority)?;
    let disk_bodies = top_level_bodies(disk)?;
    let (base_counts, auth_counts, disk_counts) = (
        name_counts(&base),
        name_counts(&auth),
        name_counts(&disk_bodies),
    );
    let mut keys: Vec<&ComponentKey> = base.keys().collect();
    keys.sort();
    let mut disk_only = Vec::new();
    for key in keys {
        let name = key.name.as_str();
        let count = base_counts.get(name).copied();
        if auth_counts.get(name).copied() != count || disk_counts.get(name).copied() != count {
            continue;
        }
        let (Some(base_range), Some(auth_range), Some(disk_range)) =
            (base.get(key), auth.get(key), disk_bodies.get(key))
        else {
            continue;
        };
        let base_durable = durable_body(baseline, base_range);
        let disk_durable = durable_body(disk, disk_range);
        if durable_body(authority, auth_range) == base_durable && disk_durable != base_durable {
            disk_only.push(DiskOnly {
                key: key.clone(),
                disk_raw: disk[disk_range.clone()].to_string(),
                disk_durable,
                disk_range: disk_range.clone(),
                baseline_range: base_range.clone(),
            });
        }
    }
    Some(disk_only)
}

/// Input projection with provenance (`#keylogrebase`): revert disk's body of a
/// component only DISK changed when that body is a state the editor's own
/// verified op lineage passed through.
///
/// [`project_disk_for_cut_exclusivity`] cannot touch such a component: with
/// text alone, the editor's in-flight typing on disk (the native save is ahead
/// of the authority's op stream) is indistinguishable from an independent disk
/// write, and merging it lets the in-flight ops land it a second time. A
/// correctly based op ledger is the provenance that tells them apart. The
/// caller feeds `visit_lineage` every replay state of the editor's ops from
/// the baseline (the baseline itself included); a component whose disk body
/// equals the same component in one of those states is the editor's own text,
/// so the authority's (unchanged) body is kept and the editor's ops roll it
/// forward exactly once.
///
/// Callers must only pass a lineage that is proven: the ops are based on the
/// baseline, attested end to end by the editor, and pass through the
/// authority's cut. A component mixing in-flight typing with independent work
/// matches no lineage state and still merges (the stated residual).
pub fn project_disk_for_editor_provenance(
    baseline: &str,
    authority: &str,
    disk: &str,
    visit_lineage: impl FnOnce(&mut dyn FnMut(&str) -> bool),
) -> EditorProvenanceProjection {
    let unchanged = || EditorProvenanceProjection {
        disk_text: disk.to_string(),
        editor_attributed: Vec::new(),
    };
    let Some(candidates) = disk_only_components(baseline, authority, disk) else {
        return unchanged();
    };
    if candidates.is_empty() {
        return unchanged();
    }
    let baseline_counts = top_level_bodies(baseline).map(|bodies| {
        name_counts(&bodies)
            .into_iter()
            .map(|(name, count)| (name.to_string(), count))
            .collect::<HashMap<String, usize>>()
    });
    let Some(baseline_counts) = baseline_counts else {
        return unchanged();
    };
    let mut matched = vec![false; candidates.len()];
    visit_lineage(&mut |state: &str| {
        // Cheap prefilter: a state can only match a body it contains verbatim.
        let open: Vec<usize> = (0..candidates.len())
            .filter(|&i| !matched[i] && state.contains(candidates[i].disk_raw.as_str()))
            .collect();
        if !open.is_empty()
            && let Some(bodies) = top_level_bodies(state)
        {
            let counts = name_counts(&bodies);
            for i in open {
                let candidate = &candidates[i];
                let name = candidate.key.name.as_str();
                if counts.get(name).copied() != baseline_counts.get(name).copied() {
                    continue;
                }
                if bodies
                    .get(&candidate.key)
                    .is_some_and(|range| durable_body(state, range) == candidate.disk_durable)
                {
                    matched[i] = true;
                }
            }
        }
        // Keep visiting while any candidate is still unattributed.
        matched.iter().any(|m| !m)
    });
    let mut attributed: Vec<&DiskOnly> = candidates
        .iter()
        .zip(&matched)
        .filter_map(|(candidate, matched)| matched.then_some(candidate))
        .collect();
    if attributed.is_empty() {
        return unchanged();
    }
    attributed.sort_by_key(|c| std::cmp::Reverse(c.disk_range.start));
    let mut disk_text = disk.to_string();
    for c in &attributed {
        disk_text.replace_range(c.disk_range.clone(), &baseline[c.baseline_range.clone()]);
    }
    let mut editor_attributed: Vec<ComponentKey> =
        attributed.into_iter().map(|c| c.key.clone()).collect();
    editor_attributed.sort_by(|a, b| a.ordinal.cmp(&b.ordinal).then(a.name.cmp(&b.name)));
    EditorProvenanceProjection {
        disk_text,
        editor_attributed,
    }
}

/// Fail-closed guard for [`project_disk_for_editor_provenance`]: `Some(reason)`
/// when `merged` does not hold the authority's body for every component the
/// provenance projection attributed to the editor's in-flight typing. Such a
/// component must reach the authority only through the editor's own ops.
pub fn merge_changes_editor_attributed(
    authority: &str,
    merged: &str,
    attributed: &[ComponentKey],
) -> Option<String> {
    if attributed.is_empty() {
        return None;
    }
    let Some(auth) = top_level_bodies(authority) else {
        return Some("unparseable_authority".to_string());
    };
    let Some(merged_bodies) = top_level_bodies(merged) else {
        return Some("unparseable_candidate".to_string());
    };
    let changed: Vec<String> = attributed
        .iter()
        .filter(|key| match (auth.get(*key), merged_bodies.get(*key)) {
            (Some(a), Some(m)) => durable_body(authority, a) != durable_body(merged, m),
            _ => true,
        })
        .map(ToString::to_string)
        .collect();
    (!changed.is_empty()).then(|| changed.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str, backlog: &str) -> String {
        format!(
            "---\nagent_doc_session: t\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nhi\n\
             <!-- agent:boundary:abcd1234 -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue subagents priority go -->\n{queue}<!-- /agent:queue -->\n\n\
             <!-- agent:backlog priority queue -->\n{backlog}<!-- /agent:backlog -->\n"
        )
    }

    const DIX: &str = "- #gh-dixhttps://github.com/btakita/agent-doc/issues/268\n\
                       - #gh-dixhttps://github.com/btakita/agent-doc/issues/269\n";
    const FIX: &str = "- #gh-fix https://github.com/btakita/agent-doc/issues/268\n\
                       - #gh-fix https://github.com/btakita/agent-doc/issues/269\n";

    #[test]
    fn a_concurrently_changed_queue_reverts_to_the_baseline_on_disk() {
        let (b, a, d) = (doc("", ""), doc(DIX, ""), doc(FIX, ""));
        let projection = project_disk_for_cut_exclusivity(&b, &a, &d);
        assert_eq!(projection.disk_text, b);
        assert_eq!(
            projection.authority_kept,
            vec![ComponentKey {
                name: "queue".into(),
                ordinal: 0
            }]
        );
    }

    #[test]
    fn a_disk_only_component_is_not_projected() {
        // The authority changed the queue; disk changed only the backlog.
        let (b, a) = (doc("", ""), doc(DIX, ""));
        let d = doc("", "- [ ] [#ind] independent\n");
        let projection = project_disk_for_cut_exclusivity(&b, &a, &d);
        assert_eq!(projection.disk_text, d);
        assert!(projection.authority_kept.is_empty());
    }

    #[test]
    fn an_identical_change_on_both_planes_is_not_a_conflict() {
        let (b, a, d) = (doc("", ""), doc(FIX, ""), doc(FIX, ""));
        let projection = project_disk_for_cut_exclusivity(&b, &a, &d);
        assert_eq!(projection.disk_text, d);
        assert!(projection.authority_kept.is_empty());
        assert_eq!(merge_mixes_component_versions(&b, &a, &d, &a), None);
    }

    #[test]
    fn the_guard_refuses_a_union_of_two_cuts_and_admits_either_cut() {
        let (b, a, d) = (doc("", ""), doc(DIX, ""), doc(FIX, ""));
        let union = doc(&format!("{DIX}{FIX}"), "");
        assert_eq!(
            merge_mixes_component_versions(&b, &a, &d, &union).as_deref(),
            Some("queue:0")
        );
        assert_eq!(merge_mixes_component_versions(&b, &a, &d, &a), None);
        assert_eq!(merge_mixes_component_versions(&b, &a, &d, &d), None);
    }

    #[test]
    fn the_guard_ignores_transient_markers() {
        let (b, a, d) = (doc("", ""), doc(DIX, ""), doc(FIX, ""));
        let marked = a.replace(
            "<!-- agent:boundary:abcd1234 -->\n",
            "<!-- agent:boundary:ffff0000 -->\n",
        );
        assert_eq!(merge_mixes_component_versions(&b, &a, &d, &marked), None);
    }

    #[test]
    fn the_guard_fails_closed_on_unparseable_input() {
        let (b, a) = (doc("", ""), doc(DIX, ""));
        let broken = "<!-- agent:queue -->\n- x\n";
        assert_eq!(
            merge_mixes_component_versions(&b, &a, broken, &a).as_deref(),
            Some("unparseable_input")
        );
    }

    fn history_visitor<'a>(states: &'a [String]) -> impl FnOnce(&mut dyn FnMut(&str) -> bool) + 'a {
        move |visit: &mut dyn FnMut(&str) -> bool| {
            for state in states {
                if !visit(state) {
                    break;
                }
            }
        }
    }

    /// `#keylogrebase` residual: the authority has not received the typing
    /// yet, the native save has. A disk-only component whose body is a state
    /// of the editor's verified lineage reverts to the baseline, so the
    /// in-flight ops land it exactly once.
    #[test]
    fn a_disk_only_component_on_the_editor_lineage_reverts_to_the_baseline() {
        let b = doc("", "");
        let typed = "- [ ] [#gh-fix] typed in the backlog\n";
        let lineage = vec![b.clone(), doc("", "- [ ] [#gh-f"), doc("", typed)];
        let a = b.clone(); // the authority's cut predates the typing
        let d = doc("", typed);
        let projection = project_disk_for_editor_provenance(&b, &a, &d, history_visitor(&lineage));
        assert_eq!(projection.disk_text, b);
        assert_eq!(
            projection.editor_attributed,
            vec![ComponentKey {
                name: "backlog".into(),
                ordinal: 0
            }]
        );
    }

    /// Reach: independent disk work is not on the lineage and still merges.
    #[test]
    fn independent_disk_work_is_not_attributed_to_the_editor() {
        let b = doc("", "");
        let lineage = vec![b.clone(), doc(DIX, "")];
        let d = doc("", "- [ ] [#ind] independent\n");
        let projection = project_disk_for_editor_provenance(&b, &b, &d, history_visitor(&lineage));
        assert_eq!(projection.disk_text, d);
        assert!(projection.editor_attributed.is_empty());
    }

    /// The stated residual: in-flight typing and independent work in ONE
    /// component match no lineage state, so the component is left to the merge.
    #[test]
    fn a_component_mixing_typing_and_independent_work_is_left_to_the_merge() {
        let b = doc("", "");
        let typed = "- [ ] [#gh-fix] typed\n";
        let lineage = vec![b.clone(), doc("", typed)];
        let d = doc("", &format!("{typed}- [ ] [#ind] independent\n"));
        let projection = project_disk_for_editor_provenance(&b, &b, &d, history_visitor(&lineage));
        assert_eq!(projection.disk_text, d);
        assert!(projection.editor_attributed.is_empty());
    }

    /// A component the authority changed is cut exclusivity's, never provenance's.
    #[test]
    fn provenance_never_touches_a_component_the_authority_changed() {
        let (b, a, d) = (doc("", ""), doc(DIX, ""), doc(FIX, ""));
        let lineage = vec![b.clone(), a.clone(), d.clone()];
        let projection = project_disk_for_editor_provenance(&b, &a, &d, history_visitor(&lineage));
        assert_eq!(projection.disk_text, d);
        assert!(projection.editor_attributed.is_empty());
    }

    /// Exhaustive check over two cuts of one editor history: every pair of
    /// distinct cuts that both changed the queue. The projected disk never
    /// carries a queue body other than the baseline's, and the guard refuses
    /// the line union of the two cuts whenever it differs from both.
    #[test]
    fn every_pair_of_cuts_of_one_history_resolves_to_one_version() {
        let history = [
            "",
            "- https://x/issues/268\n",
            "- #gh-dixhttps://x/issues/268\n",
            "- #gh-fixhttps://x/issues/268\n",
            "- #gh-fix https://x/issues/268\n",
            "- #gh-fix https://x/issues/268\n- release + publish\n",
        ];
        let b = doc(history[0], "");
        for (i, auth_q) in history.iter().enumerate().skip(1) {
            for (j, disk_q) in history.iter().enumerate().skip(1) {
                if i == j || auth_q == disk_q {
                    continue;
                }
                let (a, d) = (doc(auth_q, ""), doc(disk_q, ""));
                let projection = project_disk_for_cut_exclusivity(&b, &a, &d);
                assert_eq!(projection.disk_text, b, "cuts {i}/{j}");
                let union = doc(&format!("{auth_q}{disk_q}"), "");
                assert!(
                    merge_mixes_component_versions(&b, &a, &d, &union).is_some(),
                    "cuts {i}/{j}: the union of two cuts must be refused"
                );
            }
        }
    }
}
