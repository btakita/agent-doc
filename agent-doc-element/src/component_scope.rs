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
    let previous =
        ACTIVE_COMPONENT_WRITE_SCOPE.with(|slot| slot.borrow_mut().replace(scope));
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

/// The scoped component bodies as byte ranges in `doc`, in document order.
///
/// `None` when any scoped occurrence is missing from `doc`, or `doc` does not
/// parse — neither can be bounded by the scope, so callers fall back.
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
    bodies.sort_by_key(|body| body.start);
    Some(bodies)
}

/// `doc` with every range in `bodies` removed, so two documents can be compared
/// for changes *outside* a scope without diffing their component bodies.
pub fn text_outside_bodies(doc: &str, bodies: &[std::ops::Range<usize>]) -> String {
    let mut outside = String::with_capacity(doc.len());
    let mut cursor = 0usize;
    for body in bodies {
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
/// `None` means the change is not expressible as a set of component-body edits —
/// a component was added or removed, the framing text between components moved,
/// or a document did not parse. Callers treat that as "scope unknown" and keep
/// the unscoped path rather than refusing a legitimate write.
pub fn changed_component_scope(before: &str, after: &str) -> Option<ComponentWriteScope> {
    let before_components = crate::element::parse(before).ok()?;
    let after_components = crate::element::parse(after).ok()?;
    if before_components.len() != after_components.len() {
        return None;
    }
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut changed = Vec::new();
    let mut before_bodies = Vec::with_capacity(before_components.len());
    let mut after_bodies = Vec::with_capacity(after_components.len());
    for (before_component, after_component) in before_components.iter().zip(after_components.iter())
    {
        if before_component.name != after_component.name {
            return None;
        }
        let next = seen.entry(before_component.name.as_str()).or_insert(0);
        let occurrence = *next;
        *next += 1;
        let before_body = before_component.open_end..before_component.close_start;
        let after_body = after_component.open_end..after_component.close_start;
        if before[before_body.clone()] != after[after_body.clone()] {
            changed.push(ScopedComponent::new(before_component.name.clone(), occurrence));
        }
        before_bodies.push(before_body);
        after_bodies.push(after_body);
    }
    if text_outside_bodies(before, &before_bodies) != text_outside_bodies(after, &after_bodies) {
        return None;
    }
    Some(ComponentWriteScope::new(changed))
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

    #[test]
    fn pairs_round_trip_through_the_wire_shape() {
        let scope = ComponentWriteScope::from_pairs([
            ("exchange".to_string(), 0),
            ("backlog".to_string(), 2),
        ]);
        assert_eq!(
            ComponentWriteScope::from_pairs(scope.to_pairs()),
            scope
        );
    }
}
