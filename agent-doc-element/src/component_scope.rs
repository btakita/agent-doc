//! Component write scope for control-plane document writes (`#cpwritecomponentscoped`).
//!
//! A CP write already knows which components it mutated before it builds the new
//! document image, but the relay used to throw that knowledge away: it handed
//! `RelayHub::apply_canonical_replace` a whole-document target and let a text
//! diff rediscover the changed regions. `#exchangetypingrevert` made those
//! derived spans per-region, so an untouched component between two changed ones
//! is no longer tombstoned — but that is still a diff noticing after the fact.
//!
//! Carrying the scope instead makes the write structurally unable to reach
//! another component: the relay diffs only the scoped component bodies, and a
//! target whose text moved outside them is refused rather than published.
//!
//! The scope is ambient because it has to cross several write-path layers
//! (`persist_pending_write` → `converge_or_disk_write` → the CRDT CP write) that
//! every other caller shares. This mirrors the existing `FORCE_DISK_PENDING_WRITE`
//! ambient in the same path.

use std::cell::RefCell;

/// One component occurrence a CP write is allowed to mutate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedComponent {
    /// Component marker name, as it appears in `<!-- agent:name -->`.
    pub name: String,
    /// Zero-based index among components sharing `name`, in document order.
    pub occurrence: usize,
}

impl ScopedComponent {
    pub fn new(name: impl Into<String>, occurrence: usize) -> Self {
        Self {
            name: name.into(),
            occurrence,
        }
    }
}

/// The complete set of component occurrences a single CP write may mutate.
///
/// An empty scope is meaningful: it says the write must not change any component
/// body at all. Callers that do not know their scope pass `None` rather than an
/// empty scope, which keeps the unscoped whole-document path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentWriteScope {
    components: Vec<ScopedComponent>,
}

impl ComponentWriteScope {
    pub fn new(components: impl IntoIterator<Item = ScopedComponent>) -> Self {
        let mut components: Vec<ScopedComponent> = components.into_iter().collect();
        components.sort();
        components.dedup();
        Self { components }
    }

    /// Build from `(name, occurrence)` pairs, the shape the CP write RPC carries.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, usize)>) -> Self {
        Self::new(
            pairs
                .into_iter()
                .map(|(name, occurrence)| ScopedComponent::new(name, occurrence)),
        )
    }

    /// Wire shape for the CP write RPC payload.
    pub fn to_pairs(&self) -> Vec<(String, usize)> {
        self.components
            .iter()
            .map(|component| (component.name.clone(), component.occurrence))
            .collect()
    }

    pub fn components(&self) -> &[ScopedComponent] {
        &self.components
    }

    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    pub fn contains(&self, name: &str, occurrence: usize) -> bool {
        self.components
            .iter()
            .any(|component| component.name == name && component.occurrence == occurrence)
    }
}

thread_local! {
    static ACTIVE_COMPONENT_WRITE_SCOPE: RefCell<Option<ComponentWriteScope>> =
        const { RefCell::new(None) };
}

/// Run `f` with `scope` as the ambient CP write scope, restoring the previous
/// scope afterwards (including on unwind, via the restoring guard).
pub fn with_component_write_scope<T>(scope: ComponentWriteScope, f: impl FnOnce() -> T) -> T {
    let previous = ACTIVE_COMPONENT_WRITE_SCOPE.with(|slot| slot.borrow_mut().replace(scope));
    let _restore = RestoreComponentWriteScope { previous };
    f()
}

/// The ambient CP write scope, if the current write path declared one.
pub fn current_component_write_scope() -> Option<ComponentWriteScope> {
    ACTIVE_COMPONENT_WRITE_SCOPE.with(|slot| slot.borrow().clone())
}

struct RestoreComponentWriteScope {
    previous: Option<ComponentWriteScope>,
}

impl Drop for RestoreComponentWriteScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE_COMPONENT_WRITE_SCOPE.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// Collapse `ranges` to the disjoint, ordered set that covers the same bytes.
///
/// Components nest (`agent:salient-response` lives inside `agent:exchange`), so
/// the body ranges of every parsed component are not disjoint: a nested child's
/// body lies inside its parent's body. A range contained in an earlier range is
/// dropped, and a range that overlaps the previous one is merged into it, so
/// every consumer that slices `doc` between ranges, or applies one edit per
/// range, sees each byte at most once (#264).
pub fn outermost_ranges(
    ranges: impl IntoIterator<Item = std::ops::Range<usize>>,
) -> Vec<std::ops::Range<usize>> {
    let mut ranges: Vec<std::ops::Range<usize>> = ranges.into_iter().collect();
    ranges.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut outermost: Vec<std::ops::Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match outermost.last_mut() {
            Some(last) if range.start < last.end => {
                if range.end > last.end {
                    last.end = range.end;
                }
            }
            _ => outermost.push(range),
        }
    }
    outermost
}

/// Whether each component in `components` (parse order, sorted by
/// `open_start`) is top-level, i.e. not nested inside an earlier component.
fn top_level_flags(components: &[crate::element::Component]) -> Vec<bool> {
    let mut flags = Vec::with_capacity(components.len());
    let mut enclosing_end = 0usize;
    for component in components {
        let top_level = component.open_start >= enclosing_end;
        if top_level {
            enclosing_end = component.close_end;
        }
        flags.push(top_level);
    }
    flags
}

/// The scoped component bodies as byte ranges in `doc`, disjoint and in
/// document order.
///
/// `None` when any scoped occurrence is missing from `doc`, or `doc` does not
/// parse — neither can be bounded by the scope, so callers fall back.
///
/// A scope may name both a component and a component nested inside it. The
/// nested body is already part of its parent's body, so it contributes no
/// separate range: the result is [`outermost_ranges`] of the scoped bodies, and
/// a caller that diffs each returned body never edits the same bytes twice.
pub fn scoped_component_bodies(
    doc: &str,
    scope: &ComponentWriteScope,
) -> Option<Vec<std::ops::Range<usize>>> {
    let components = crate::element::parse(doc).ok()?;
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut bodies = Vec::with_capacity(scope.components().len());
    for component in &components {
        let next = seen.entry(component.name.as_str()).or_insert(0);
        let occurrence = *next;
        *next += 1;
        if scope.contains(&component.name, occurrence) {
            bodies.push(component.open_end..component.close_start);
        }
    }
    if bodies.len() != scope.components().len() {
        return None;
    }
    Some(outermost_ranges(bodies))
}

/// `doc` with every range in `bodies` removed, so two documents can be compared
/// for changes *outside* a scope without diffing their component bodies.
///
/// `bodies` may be unordered, nested, or overlapping: the union of the ranges is
/// removed (see [`outermost_ranges`]). Slicing the raw ranges in sequence would
/// panic on a nested body, whose start precedes the end of its parent (#264).
pub fn text_outside_bodies(doc: &str, bodies: &[std::ops::Range<usize>]) -> String {
    let mut outside = String::with_capacity(doc.len());
    let mut cursor = 0usize;
    for body in outermost_ranges(bodies.iter().cloned()) {
        outside.push_str(&doc[cursor..body.start]);
        cursor = body.end;
    }
    outside.push_str(&doc[cursor..]);
    outside
}

/// The component occurrences whose bodies differ between `before` and `after`.
///
/// This is how a tracked-work mutation declares its own scope: it is computed at
/// the mutation site, against the baseline the mutation itself was planned from,
/// which is a much smaller and earlier comparison than the relay's
/// canonical-versus-target diff. The relay then refuses to widen it.
///
/// Only top-level components are reported. A nested component (for example
/// `agent:salient-response` inside `agent:exchange`) has its body inside its
/// parent's body, so a change to it is a change to the parent's body and is
/// declared as the parent's occurrence. Declaring the child as well would hand
/// consumers overlapping ranges that each get their own edit; the parent alone
/// already bounds the write exactly. Occurrence indexes still count every
/// component with the same name in document order, nested or not, matching
/// [`scoped_component_bodies`].
///
/// `None` means the change is not expressible as a set of component-body edits —
/// a component was added or removed, a component moved into or out of another
/// component, the framing text between top-level components moved, or a
/// document did not parse. Callers treat that as "scope unknown" and keep the
/// unscoped path rather than refusing a legitimate write.
pub fn changed_component_scope(before: &str, after: &str) -> Option<ComponentWriteScope> {
    let before_components = crate::element::parse(before).ok()?;
    let after_components = crate::element::parse(after).ok()?;
    if before_components.len() != after_components.len() {
        return None;
    }
    let before_top_level = top_level_flags(&before_components);
    if before_top_level != top_level_flags(&after_components) {
        return None;
    }
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut changed = Vec::new();
    let mut before_bodies = Vec::with_capacity(before_components.len());
    let mut after_bodies = Vec::with_capacity(after_components.len());
    for ((before_component, after_component), top_level) in before_components
        .iter()
        .zip(after_components.iter())
        .zip(before_top_level)
    {
        if before_component.name != after_component.name {
            return None;
        }
        let next = seen.entry(before_component.name.as_str()).or_insert(0);
        let occurrence = *next;
        *next += 1;
        if !top_level {
            continue;
        }
        let before_body = before_component.open_end..before_component.close_start;
        let after_body = after_component.open_end..after_component.close_start;
        if before[before_body.clone()] != after[after_body.clone()] {
            changed.push(ScopedComponent::new(
                before_component.name.clone(),
                occurrence,
            ));
        }
        before_bodies.push(before_body);
        after_bodies.push(after_body);
    }
    if text_outside_bodies(before, &before_bodies) != text_outside_bodies(after, &after_bodies) {
        return None;
    }
    Some(ComponentWriteScope::new(changed))
}

/// One top-level component occurrence and its body range in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevelComponentBody {
    pub component: ScopedComponent,
    pub body: std::ops::Range<usize>,
}

/// Every top-level component occurrence of `doc` with its body range, in
/// document order. Occurrence indexes count nested components too, matching
/// [`changed_component_scope`] and [`scoped_component_bodies`]. `None` when
/// `doc` does not parse.
pub fn top_level_component_bodies(doc: &str) -> Option<Vec<TopLevelComponentBody>> {
    let components = crate::element::parse(doc).ok()?;
    let top_level = top_level_flags(&components);
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut bodies = Vec::new();
    for (component, top_level) in components.iter().zip(top_level) {
        let next = seen.entry(component.name.as_str()).or_insert(0);
        let occurrence = *next;
        *next += 1;
        if top_level {
            bodies.push(TopLevelComponentBody {
                component: ScopedComponent::new(component.name.clone(), occurrence),
                body: component.open_end..component.close_start,
            });
        }
    }
    Some(bodies)
}

/// `dst` with the body of every top-level component occurrence named in
/// `scope` replaced by that occurrence's body in `src`. Everything else, the
/// framing between components included, stays `dst`'s.
///
/// `None` when either document does not parse, the two do not hold the same
/// top-level component occurrences in the same order, or `scope` names an
/// occurrence that is not top-level in both: the two cannot then be spliced
/// component by component, and callers keep their unspliced value.
pub fn replace_component_bodies(
    dst: &str,
    src: &str,
    scope: &ComponentWriteScope,
) -> Option<String> {
    let dst_bodies = top_level_component_bodies(dst)?;
    let src_bodies = top_level_component_bodies(src)?;
    if dst_bodies.len() != src_bodies.len()
        || dst_bodies
            .iter()
            .zip(&src_bodies)
            .any(|(dst_body, src_body)| dst_body.component != src_body.component)
    {
        return None;
    }
    if scope.components().iter().any(|scoped| {
        !dst_bodies
            .iter()
            .any(|candidate| &candidate.component == scoped)
    }) {
        return None;
    }
    let mut spliced = String::with_capacity(dst.len());
    let mut cursor = 0usize;
    for (dst_body, src_body) in dst_bodies.iter().zip(&src_bodies) {
        if !scope.contains(&dst_body.component.name, dst_body.component.occurrence) {
            continue;
        }
        spliced.push_str(&dst[cursor..dst_body.body.start]);
        spliced.push_str(&src[src_body.body.clone()]);
        cursor = dst_body.body.end;
    }
    spliced.push_str(&dst[cursor..]);
    Some(spliced)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_is_sorted_and_deduplicated() {
        let scope = ComponentWriteScope::new([
            ScopedComponent::new("queue", 0),
            ScopedComponent::new("backlog", 0),
            ScopedComponent::new("queue", 0),
        ]);
        assert_eq!(
            scope.to_pairs(),
            vec![("backlog".to_string(), 0), ("queue".to_string(), 0)]
        );
        assert!(scope.contains("queue", 0));
        assert!(!scope.contains("queue", 1));
    }

    #[test]
    fn ambient_scope_restores_the_previous_value() {
        assert_eq!(current_component_write_scope(), None);
        let outer = ComponentWriteScope::new([ScopedComponent::new("backlog", 0)]);
        with_component_write_scope(outer.clone(), || {
            assert_eq!(current_component_write_scope(), Some(outer.clone()));
            let inner = ComponentWriteScope::new([ScopedComponent::new("queue", 0)]);
            with_component_write_scope(inner.clone(), || {
                assert_eq!(current_component_write_scope(), Some(inner));
            });
            assert_eq!(current_component_write_scope(), Some(outer.clone()));
        });
        assert_eq!(current_component_write_scope(), None);
    }

    #[test]
    fn an_empty_scope_is_distinct_from_no_scope() {
        let empty = ComponentWriteScope::default();
        assert!(empty.is_empty());
        with_component_write_scope(empty, || {
            assert_eq!(
                current_component_write_scope(),
                Some(ComponentWriteScope::default())
            );
        });
    }

    const DOC: &str = concat!(
        "<!-- agent:exchange -->\n",
        "Prompt.\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] one\n",
        "<!-- /agent:backlog -->\n",
        "<!-- agent:queue -->\n",
        "- do [#a]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn a_single_component_edit_declares_only_that_component() {
        let after = DOC.replace("- [ ] [#a] one\n", "- [ ] [#a] one\n- [ ] [#b] two\n");
        assert_eq!(
            changed_component_scope(DOC, &after).map(|scope| scope.to_pairs()),
            Some(vec![("backlog".to_string(), 0)])
        );
    }

    #[test]
    fn an_edit_touching_two_components_declares_both() {
        let after = DOC
            .replace("- [ ] [#a] one\n", "- [ ] [#a] one\n- [ ] [#b] two\n")
            .replace("- do [#a]\n", "- do [#a]\n- do [#b]\n");
        assert_eq!(
            changed_component_scope(DOC, &after).map(|scope| scope.to_pairs()),
            Some(vec![("backlog".to_string(), 0), ("queue".to_string(), 0)])
        );
    }

    #[test]
    fn an_unchanged_document_declares_an_empty_scope() {
        assert_eq!(
            changed_component_scope(DOC, DOC),
            Some(ComponentWriteScope::default())
        );
    }

    #[test]
    fn adding_a_component_is_not_expressible_as_a_scope() {
        let after = DOC.replace(
            "<!-- agent:queue -->\n",
            "<!-- agent:review -->\n<!-- /agent:review -->\n<!-- agent:queue -->\n",
        );
        assert_eq!(changed_component_scope(DOC, &after), None);
    }

    #[test]
    fn changing_text_between_components_is_not_expressible_as_a_scope() {
        let after = DOC.replace(
            "<!-- /agent:backlog -->\n",
            "<!-- /agent:backlog -->\nstray framing line\n",
        );
        assert_eq!(changed_component_scope(DOC, &after), None);
    }

    #[test]
    fn scoped_bodies_are_absent_when_an_occurrence_does_not_exist() {
        let scope = ComponentWriteScope::new([ScopedComponent::new("backlog", 1)]);
        assert_eq!(scoped_component_bodies(DOC, &scope), None);
    }

    /// #264 fixture: `agent:salient-response` is always nested inside
    /// `agent:exchange`, so its body range lies inside the exchange body range.
    const NESTED_DOC: &str = concat!(
        "<!-- agent:exchange patch=append -->\n",
        "Prompt.\n",
        "### Re: answer\n",
        "<!-- agent:salient-response cycle=\"cycle-1\" -->\n",
        "Salient line.\n",
        "<!-- /agent:salient-response -->\n",
        "Trailing response text.\n",
        "<!-- /agent:exchange -->\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] one\n",
        "<!-- /agent:backlog -->\n",
    );

    fn body_of(doc: &str, name: &str) -> std::ops::Range<usize> {
        let component = crate::element::parse(doc)
            .unwrap()
            .into_iter()
            .find(|component| component.name == name)
            .unwrap();
        component.open_end..component.close_start
    }

    #[test]
    fn a_nested_component_in_both_documents_does_not_panic_and_scopes_its_parent() {
        // The write that adds the nested node gets through the count check;
        // every later write, with the node on both sides, used to panic slicing
        // `doc[exchange_body.end..salient_body.start]`.
        let after = NESTED_DOC.replace(
            "Trailing response text.\n",
            "Trailing response text.\nMore response text.\n",
        );
        assert_eq!(
            changed_component_scope(NESTED_DOC, &after).map(|scope| scope.to_pairs()),
            Some(vec![("exchange".to_string(), 0)])
        );
    }

    #[test]
    fn a_change_inside_a_nested_body_is_declared_as_its_top_level_parent() {
        let after = NESTED_DOC.replace("Salient line.\n", "Salient line, revised.\n");
        assert_eq!(
            changed_component_scope(NESTED_DOC, &after).map(|scope| scope.to_pairs()),
            Some(vec![("exchange".to_string(), 0)])
        );
    }

    #[test]
    fn a_nested_document_edit_beside_another_component_declares_both_top_level_components() {
        let after = NESTED_DOC
            .replace("Salient line.\n", "Salient line, revised.\n")
            .replace("- [ ] [#a] one\n", "- [ ] [#a] one\n- [ ] [#b] two\n");
        assert_eq!(
            changed_component_scope(NESTED_DOC, &after).map(|scope| scope.to_pairs()),
            Some(vec![
                ("backlog".to_string(), 0),
                ("exchange".to_string(), 0)
            ])
        );
        assert_eq!(
            changed_component_scope(NESTED_DOC, NESTED_DOC),
            Some(ComponentWriteScope::default())
        );
    }

    #[test]
    fn moving_a_component_out_of_its_parent_is_not_expressible_as_a_scope() {
        let after = NESTED_DOC
            .replace("<!-- /agent:salient-response -->\n", "")
            .replace(
                "<!-- /agent:exchange -->\n",
                "<!-- /agent:exchange -->\n<!-- /agent:salient-response -->\n",
            );
        // `after` is not even well-nested; also check a well-nested move.
        assert_eq!(changed_component_scope(NESTED_DOC, &after), None);
        let moved = NESTED_DOC
            .replace(
                "<!-- agent:salient-response cycle=\"cycle-1\" -->\nSalient line.\n<!-- /agent:salient-response -->\n",
                "",
            )
            .replace(
                "<!-- /agent:exchange -->\n",
                "<!-- /agent:exchange -->\n<!-- agent:salient-response cycle=\"cycle-1\" -->\nSalient line.\n<!-- /agent:salient-response -->\n",
            );
        assert_eq!(changed_component_scope(NESTED_DOC, &moved), None);
    }

    #[test]
    fn scoped_bodies_collapse_a_nested_child_into_its_scoped_parent() {
        let both = ComponentWriteScope::new([
            ScopedComponent::new("exchange", 0),
            ScopedComponent::new("salient-response", 0),
        ]);
        assert_eq!(
            scoped_component_bodies(NESTED_DOC, &both),
            Some(vec![body_of(NESTED_DOC, "exchange")])
        );
        let child_only = ComponentWriteScope::new([ScopedComponent::new("salient-response", 0)]);
        assert_eq!(
            scoped_component_bodies(NESTED_DOC, &child_only),
            Some(vec![body_of(NESTED_DOC, "salient-response")])
        );
        let with_sibling = ComponentWriteScope::new([
            ScopedComponent::new("backlog", 0),
            ScopedComponent::new("exchange", 0),
            ScopedComponent::new("salient-response", 0),
        ]);
        assert_eq!(
            scoped_component_bodies(NESTED_DOC, &with_sibling),
            Some(vec![
                body_of(NESTED_DOC, "exchange"),
                body_of(NESTED_DOC, "backlog")
            ])
        );
    }

    #[test]
    fn text_outside_nested_bodies_removes_their_union() {
        let exchange = body_of(NESTED_DOC, "exchange");
        let salient = body_of(NESTED_DOC, "salient-response");
        let expected = format!(
            "{}{}",
            &NESTED_DOC[..exchange.start],
            &NESTED_DOC[exchange.end..]
        );
        assert_eq!(
            text_outside_bodies(NESTED_DOC, &[exchange.clone(), salient.clone()]),
            expected
        );
        // Order of the input ranges does not matter.
        assert_eq!(
            text_outside_bodies(NESTED_DOC, &[salient, exchange]),
            expected
        );
    }

    #[test]
    fn outermost_ranges_drop_contained_and_merge_overlapping_ranges() {
        assert_eq!(outermost_ranges([5..8, 0..10, 2..3]), vec![0..10]);
        assert_eq!(outermost_ranges([0..4, 2..6, 8..9]), vec![0..6, 8..9]);
        assert_eq!(outermost_ranges([3..3, 0..1]), vec![0..1, 3..3]);
        assert_eq!(outermost_ranges(Vec::new()), Vec::new());
    }

    /// Generated nested markup: every placement of an edit in every body of
    /// a family of nested documents must yield `Some(top-level ancestor)`, and
    /// replaying the declared scope's bodies onto `before` must rebuild `after`
    /// exactly — the contract the relay relies on.
    #[test]
    fn generated_nested_markup_declares_the_top_level_ancestor_of_each_edit() {
        // (name, parent index) — parents precede children in document order.
        type Shape = &'static [(&'static str, Option<usize>)];
        let shapes: &[Shape] = &[
            &[("exchange", None), ("salient-response", Some(0))],
            &[
                ("exchange", None),
                ("salient-response", Some(0)),
                ("backlog", None),
            ],
            &[
                ("backlog", None),
                ("exchange", None),
                ("salient-response", Some(1)),
                ("salient-response", Some(1)),
            ],
            &[("outer", None), ("middle", Some(0)), ("inner", Some(1))],
            &[
                ("exchange", None),
                ("salient-response", Some(0)),
                ("exchange", None),
                ("salient-response", Some(2)),
            ],
        ];

        fn render(shape: Shape, node: usize, edited: Option<usize>, out: &mut String) {
            let name = shape[node].0;
            out.push_str(&format!("<!-- agent:{name} -->\nhead {node}\n"));
            if edited == Some(node) {
                out.push_str("edited\n");
            }
            for child in (0..shape.len()).filter(|child| shape[*child].1 == Some(node)) {
                render(shape, child, edited, out);
                out.push_str(&format!("between {node}/{child}\n"));
            }
            out.push_str(&format!("tail {node}\n<!-- /agent:{name} -->\n"));
        }
        fn document(shape: Shape, edited: Option<usize>) -> String {
            let mut out = String::from("# Doc\n");
            for root in (0..shape.len()).filter(|node| shape[*node].1.is_none()) {
                render(shape, root, edited, &mut out);
                out.push_str("framing\n");
            }
            out
        }
        fn occurrence(shape: Shape, node: usize) -> usize {
            // Document order of the rendered tree (pre-order).
            fn preorder(shape: Shape, node: usize, order: &mut Vec<usize>) {
                order.push(node);
                for child in (0..shape.len()).filter(|child| shape[*child].1 == Some(node)) {
                    preorder(shape, child, order);
                }
            }
            let mut order = Vec::new();
            for root in (0..shape.len()).filter(|node| shape[*node].1.is_none()) {
                preorder(shape, root, &mut order);
            }
            order
                .iter()
                .take_while(|candidate| **candidate != node)
                .filter(|candidate| shape[**candidate].0 == shape[node].0)
                .count()
        }

        for shape in shapes {
            let before = document(shape, None);
            for edited in 0..shape.len() {
                let after = document(shape, Some(edited));
                let mut ancestor = edited;
                while let Some(parent) = shape[ancestor].1 {
                    ancestor = parent;
                }
                let scope = changed_component_scope(&before, &after)
                    .unwrap_or_else(|| panic!("scope for edit {edited} in {shape:?}"));
                assert_eq!(
                    scope.to_pairs(),
                    vec![(shape[ancestor].0.to_string(), occurrence(shape, ancestor))],
                    "edit {edited} in {shape:?}"
                );

                let before_bodies = scoped_component_bodies(&before, &scope).unwrap();
                let after_bodies = scoped_component_bodies(&after, &scope).unwrap();
                assert_eq!(
                    text_outside_bodies(&before, &before_bodies),
                    text_outside_bodies(&after, &after_bodies)
                );
                let mut rebuilt = before.clone();
                for (before_body, after_body) in before_bodies.iter().zip(after_bodies.iter()).rev()
                {
                    rebuilt.replace_range(before_body.clone(), &after[after_body.clone()]);
                }
                assert_eq!(rebuilt, after, "edit {edited} in {shape:?}");

                // A scope naming every component, nested ones included, must
                // still resolve to disjoint ranges.
                let everything = ComponentWriteScope::new(
                    (0..shape.len())
                        .map(|node| ScopedComponent::new(shape[node].0, occurrence(shape, node))),
                );
                let bodies = scoped_component_bodies(&before, &everything).unwrap();
                assert!(bodies.windows(2).all(|pair| pair[0].end <= pair[1].start));
            }
        }
    }

    #[test]
    fn replace_component_bodies_splices_only_the_scoped_occurrences() {
        let src = DOC
            .replace("- [ ] [#a] one\n", "- [ ] [#a] one (src)\n")
            .replace("- do [#a]\n", "- do [#a] (src)\n");
        let dst = DOC
            .replace("Prompt.\n", "Prompt, typed.\n")
            .replace("- do [#a]\n", "- do [#a] (dst)\n");
        let backlog = ComponentWriteScope::new([ScopedComponent::new("backlog", 0)]);
        assert_eq!(
            replace_component_bodies(&dst, &src, &backlog).as_deref(),
            Some(
                dst.replace("- [ ] [#a] one\n", "- [ ] [#a] one (src)\n")
                    .as_str()
            )
        );
        assert_eq!(
            replace_component_bodies(&dst, &src, &ComponentWriteScope::default()).as_deref(),
            Some(dst.as_str())
        );
        // A scoped occurrence missing from either side cannot be spliced.
        let missing = ComponentWriteScope::new([ScopedComponent::new("todo", 0)]);
        assert_eq!(replace_component_bodies(&dst, &src, &missing), None);
        // Different component layouts cannot be spliced either.
        let reshaped = format!("{DOC}<!-- agent:todo -->\n- [ ] t\n<!-- /agent:todo -->\n");
        assert_eq!(replace_component_bodies(&reshaped, &src, &backlog), None);
    }

    #[test]
    fn pairs_round_trip_through_the_wire_shape() {
        let scope = ComponentWriteScope::from_pairs([
            ("exchange".to_string(), 0),
            ("backlog".to_string(), 2),
        ]);
        assert_eq!(ComponentWriteScope::from_pairs(scope.to_pairs()), scope);
    }
}
