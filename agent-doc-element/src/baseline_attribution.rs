//! Attribute a binary-owned component write to the session baseline
//! (`#backlogiceboxreview`).
//!
//! Preflight labels every difference between the stored baseline and the
//! current document as operator input (`[user+]` / `[user-]` / `[user~]`). A
//! standalone `agent-doc backlog|icebox|review` mutation writes the document but
//! never advanced the baseline, so the next preflight presented the binary's own
//! write as an operator edit.
//!
//! Re-snapshotting the whole document would fix that and break something worse:
//! any operator edit that landed since the last baseline (anywhere in the
//! document) would be folded into the baseline and silently disappear from the
//! next diff. So the baseline advances by the *structural* delta of the write
//! instead: the exact component-body change from `before` (the document the
//! command planned against) to `after` (the target it wrote) is replayed onto the
//! stored baseline, and nothing else is.
//!
//! The replay is a line-level three-way merge with `before` as the ancestor:
//! the write's changes (`before` → `after`) and the baseline's own differences
//! (`before` → `baseline`, i.e. operator edits the baseline has not absorbed)
//! must not overlap or touch. When they do, or when the write is not expressible
//! as component-body edits at all, the baseline is left untouched. That is the
//! fail-safe direction: an agent edit shown as an operator edit is noise, an
//! operator edit hidden by the baseline is lost intent.

use std::ops::Range;

use similar::{Algorithm, DiffTag, capture_diff_slices};

use crate::component_scope::{ScopedComponent, changed_component_scope};

/// Outcome of replaying one binary-owned write onto a stored baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineAttribution {
    /// The baseline with exactly the write's component delta applied.
    Patched(String),
    /// The write changed no component body (nothing to attribute).
    NoComponentChange,
    /// Every changed component already carries the write's result in the
    /// baseline (another step adopted it first).
    AlreadyCarried,
    /// The delta could not be replayed without guessing; the baseline must stay
    /// untouched.
    Refused(BaselineAttributionRefusal),
}

/// Why a write's delta was not replayed onto the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineAttributionRefusal {
    /// The write created/removed a component, moved text outside component
    /// bodies, or a document did not parse.
    UnscopedWrite,
    /// A component the write changed does not exist in the baseline.
    ComponentMissingFromBaseline,
    /// The baseline differs from the planned document inside a changed
    /// component in a way that overlaps or touches the write's own change.
    OverlapsUnabsorbedEdit,
}

impl BaselineAttributionRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnscopedWrite => "unscoped_write",
            Self::ComponentMissingFromBaseline => "component_missing_from_baseline",
            Self::OverlapsUnabsorbedEdit => "overlaps_unabsorbed_edit",
        }
    }
}

impl BaselineAttribution {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Patched(_) => "patched",
            Self::NoComponentChange => "no_component_change",
            Self::AlreadyCarried => "already_carried",
            Self::Refused(refusal) => refusal.as_str(),
        }
    }
}

/// Replay the component delta `before` → `after` onto `baseline`.
pub fn attribute_component_write(baseline: &str, before: &str, after: &str) -> BaselineAttribution {
    let Some(scope) = changed_component_scope(before, after) else {
        return BaselineAttribution::Refused(BaselineAttributionRefusal::UnscopedWrite);
    };
    if scope.is_empty() {
        return BaselineAttribution::NoComponentChange;
    }
    let (Some(before_bodies), Some(after_bodies)) = (
        component_bodies(before, scope.components()),
        component_bodies(after, scope.components()),
    ) else {
        return BaselineAttribution::Refused(BaselineAttributionRefusal::UnscopedWrite);
    };
    let Some(baseline_bodies) = component_bodies(baseline, scope.components()) else {
        return BaselineAttribution::Refused(
            BaselineAttributionRefusal::ComponentMissingFromBaseline,
        );
    };

    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    for ((before_body, after_body), baseline_body) in before_bodies
        .iter()
        .zip(after_bodies.iter())
        .zip(baseline_bodies.iter())
    {
        let before_text = &before[before_body.clone()];
        let after_text = &after[after_body.clone()];
        let baseline_text = &baseline[baseline_body.clone()];
        if baseline_text == after_text {
            continue;
        }
        let merged = if baseline_text == before_text {
            after_text.to_string()
        } else {
            match merge_lines(before_text, after_text, baseline_text) {
                Some(merged) => merged,
                None => {
                    return BaselineAttribution::Refused(
                        BaselineAttributionRefusal::OverlapsUnabsorbedEdit,
                    );
                }
            }
        };
        if merged != baseline_text {
            replacements.push((baseline_body.clone(), merged));
        }
    }
    if replacements.is_empty() {
        return BaselineAttribution::AlreadyCarried;
    }
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let mut patched = baseline.to_string();
    for (range, body) in replacements {
        patched.replace_range(range, &body);
    }
    BaselineAttribution::Patched(patched)
}

/// Body ranges of each scoped component occurrence in `doc`, in scope order.
fn component_bodies(doc: &str, scoped: &[ScopedComponent]) -> Option<Vec<Range<usize>>> {
    let components = crate::element::parse(doc).ok()?;
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut located: Vec<(String, usize, Range<usize>)> = Vec::new();
    for component in &components {
        let next = seen.entry(component.name.as_str()).or_insert(0);
        located.push((
            component.name.clone(),
            *next,
            component.open_end..component.close_start,
        ));
        *next += 1;
    }
    scoped
        .iter()
        .map(|wanted| {
            located
                .iter()
                .find(|(name, occurrence, _)| {
                    *name == wanted.name && *occurrence == wanted.occurrence
                })
                .map(|(_, _, range)| range.clone())
        })
        .collect()
}

/// One change against the ancestor's lines: replace `old` with `new`.
struct Hunk<'a> {
    old: Range<usize>,
    new: &'a [&'a str],
}

fn hunks<'a>(ancestor: &[&str], side: &'a [&'a str]) -> Vec<Hunk<'a>> {
    capture_diff_slices(Algorithm::Myers, ancestor, side)
        .into_iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| Hunk {
            old: op.old_range(),
            new: &side[op.new_range()],
        })
        .collect()
}

/// Two hunks conflict when their ancestor ranges overlap or touch. Touching
/// (including two insertions at one position) is treated as a conflict because
/// the relative order of the two sides would be a guess.
fn conflicts(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// Clean three-way line merge of `ours` and `theirs` over `ancestor`, or `None`
/// when any change from one side overlaps or touches a change from the other.
fn merge_lines(ancestor: &str, ours: &str, theirs: &str) -> Option<String> {
    let ancestor_lines: Vec<&str> = ancestor.split_inclusive('\n').collect();
    let our_lines: Vec<&str> = ours.split_inclusive('\n').collect();
    let their_lines: Vec<&str> = theirs.split_inclusive('\n').collect();
    let our_hunks = hunks(&ancestor_lines, &our_lines);
    let their_hunks = hunks(&ancestor_lines, &their_lines);
    if our_hunks.iter().any(|ours| {
        their_hunks
            .iter()
            .any(|theirs| conflicts(&ours.old, &theirs.old))
    }) {
        return None;
    }
    let mut all: Vec<&Hunk<'_>> = our_hunks.iter().chain(their_hunks.iter()).collect();
    all.sort_by_key(|hunk| (hunk.old.start, hunk.old.end));
    let mut merged = String::with_capacity(theirs.len() + ours.len());
    let mut cursor = 0usize;
    for hunk in all {
        for line in &ancestor_lines[cursor..hunk.old.start] {
            merged.push_str(line);
        }
        for line in hunk.new {
            merged.push_str(line);
        }
        cursor = hunk.old.end;
    }
    for line in &ancestor_lines[cursor..] {
        merged.push_str(line);
    }
    Some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(exchange: &str, backlog: &str, queue: &str) -> String {
        format!(
            "---\nagent_doc_session: t\n---\n\n<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n"
        )
    }

    const BACKLOG: &str = "- [ ] [#one] first\n- [ ] [#two] second\n- [ ] [#three] third\n";

    #[test]
    fn a_write_against_an_absorbed_baseline_replays_exactly() {
        let before = doc("Done.\n", BACKLOG, "");
        let after = doc("Done.\n", &format!("- [ ] [#new] added\n{BACKLOG}"), "");
        assert_eq!(
            attribute_component_write(&before, &before, &after),
            BaselineAttribution::Patched(after.clone())
        );
    }

    #[test]
    fn an_operator_edit_in_another_component_stays_out_of_the_baseline() {
        let baseline = doc("Done.\n", BACKLOG, "");
        // The operator typed into the exchange after the last baseline; the
        // command planned against (and wrote over) that live document.
        let before = doc("Done.\n\nplease also check the logs\n", BACKLOG, "");
        let after = doc(
            "Done.\n\nplease also check the logs\n",
            &format!("- [ ] [#new] added\n{BACKLOG}"),
            "",
        );
        let BaselineAttribution::Patched(patched) =
            attribute_component_write(&baseline, &before, &after)
        else {
            panic!("expected a patch");
        };
        assert_eq!(
            patched,
            doc("Done.\n", &format!("- [ ] [#new] added\n{BACKLOG}"), "")
        );
        assert!(!patched.contains("please also check the logs"));
    }

    #[test]
    fn a_non_overlapping_operator_edit_in_the_same_component_stays_visible() {
        let baseline = doc("Done.\n", BACKLOG, "");
        // Operator appended an item at the end; the command removed the first.
        let operator_backlog = format!("{BACKLOG}- [ ] [#op] operator item\n");
        let before = doc("Done.\n", &operator_backlog, "");
        let after = doc(
            "Done.\n",
            &operator_backlog.replace("- [ ] [#one] first\n", ""),
            "",
        );
        let BaselineAttribution::Patched(patched) =
            attribute_component_write(&baseline, &before, &after)
        else {
            panic!("expected a patch");
        };
        assert_eq!(
            patched,
            doc("Done.\n", "- [ ] [#two] second\n- [ ] [#three] third\n", "")
        );
    }

    #[test]
    fn an_overlapping_operator_edit_leaves_the_baseline_untouched() {
        let baseline = doc("Done.\n", BACKLOG, "");
        let before = doc(
            "Done.\n",
            &BACKLOG.replace("[#two] second", "[#two] second (operator note)"),
            "",
        );
        let after = doc(
            "Done.\n",
            &BACKLOG.replace("[#two] second", "[#two] rewritten by edit"),
            "",
        );
        assert_eq!(
            attribute_component_write(&baseline, &before, &after),
            BaselineAttribution::Refused(BaselineAttributionRefusal::OverlapsUnabsorbedEdit)
        );
    }

    #[test]
    fn an_insertion_touching_an_operator_insertion_is_refused() {
        let baseline = doc("Done.\n", BACKLOG, "");
        let before = doc("Done.\n", &format!("- [ ] [#op] operator\n{BACKLOG}"), "");
        let after = doc(
            "Done.\n",
            &format!("- [ ] [#new] agent\n- [ ] [#op] operator\n{BACKLOG}"),
            "",
        );
        assert_eq!(
            attribute_component_write(&baseline, &before, &after),
            BaselineAttribution::Refused(BaselineAttributionRefusal::OverlapsUnabsorbedEdit)
        );
    }

    #[test]
    fn a_change_already_in_the_baseline_is_not_applied_twice() {
        let before = doc("Done.\n", BACKLOG, "");
        let after = doc("Done.\n", &format!("- [ ] [#new] added\n{BACKLOG}"), "");
        assert_eq!(
            attribute_component_write(&after, &before, &after),
            BaselineAttribution::AlreadyCarried
        );
    }

    #[test]
    fn a_write_that_creates_a_component_is_unscoped() {
        let before = "---\nagent_doc_session: t\n---\n\n<!-- agent:exchange -->\nDone.\n<!-- /agent:exchange -->\n";
        let after =
            format!("{before}\n<!-- agent:backlog -->\n- [ ] [#new] x\n<!-- /agent:backlog -->\n");
        assert_eq!(
            attribute_component_write(before, before, &after),
            BaselineAttribution::Refused(BaselineAttributionRefusal::UnscopedWrite)
        );
    }

    #[test]
    fn a_component_missing_from_the_baseline_is_refused() {
        let baseline = "---\nagent_doc_session: t\n---\n\n<!-- agent:exchange -->\nDone.\n<!-- /agent:exchange -->\n";
        let before = doc("Done.\n", BACKLOG, "");
        let after = doc("Done.\n", &format!("- [ ] [#new] added\n{BACKLOG}"), "");
        assert_eq!(
            attribute_component_write(baseline, &before, &after),
            BaselineAttribution::Refused(BaselineAttributionRefusal::ComponentMissingFromBaseline)
        );
    }

    #[test]
    fn an_identical_write_attributes_nothing() {
        let before = doc("Done.\n", BACKLOG, "");
        assert_eq!(
            attribute_component_write(&before, &before, &before),
            BaselineAttribution::NoComponentChange
        );
    }
}
