//! Revision twins: two revisions of ONE authored list item (`typingforkdup`).
//!
//! An item the operator is typing passes through many intermediate texts. Any
//! plane that observes the document mid-burst (a native save on disk, a stale
//! snapshot the agent built on) can carry one of them. When that plane and the
//! live operator text are three-way merged against a baseline that holds
//! neither, BOTH sides look like they "added" an item, and a line-based engine
//! keeps the two texts as two items. Live on 2026-10-11: disk held a 279-byte
//! cut of a queue item ending mid-sentence, the editor authority held the
//! 344-byte revision, and the adopted admission merge carried both.
//!
//! The invariant (`formal/tla/AdmissionSplitMerge.tla`, `NoDuplicateItem`):
//! every item a merge emits is the image of exactly ONE authored item. The
//! decidable form used here: an agent-side item that is a prefix of (or equal
//! to) an item the operator ADDED since the baseline is an earlier revision of
//! that item, so the operator's text is the pair's only image. An in-progress
//! line usually only grows at its end. It is the same item when the operator
//! instead inserts text before one closer they had already typed (`()` becoming
//! `(like a review or a code change)`), and the earlier cut's only unmatched
//! tail is that closer. A backspace is not this shape. The assumption that makes
//! this sound is that an independently authored item is never a prefix, or a
//! prefix plus one trailing closer, of an item the operator added in the same
//! window; the reach config proves genuinely independent agent-side additions
//! still land.
//!
//! Scope: the identity-bearing list components (`queue`, `backlog`), the same
//! ones the admission count-conservation guard covers. Prose components are
//! excluded on purpose: a streamed agent response is legitimately a prefix of
//! its own later checkpoint, so the textual test is unsound there.

use std::collections::HashMap;

use crate::document_cell::{ItemSpan, project_document_spans};

/// Components whose items carry operator-authored identity.
pub const REVISION_TWIN_COMPONENTS: [&str; 2] = ["queue", "backlog"];

/// The agent side of a merge after revision twins of operator additions were
/// projected out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionTwinProjection {
    /// The agent text the merge engine should consume.
    pub agent_text: String,
    /// Number of agent-side item occurrences removed as revision twins.
    pub collapsed: usize,
}

fn twin_key(text: &str) -> &str {
    text.trim_end()
}

fn is_list_item(item: &ItemSpan) -> bool {
    let head = item.value.trim_start();
    REVISION_TWIN_COMPONENTS.contains(&item.component.as_str())
        && (head.starts_with("- ") || head.starts_with("* ") || head.trim_end() == "-")
}

fn list_items(doc: &str) -> Option<Vec<ItemSpan>> {
    Some(
        project_document_spans(doc)?
            .into_iter()
            .filter(is_list_item)
            .collect(),
    )
}

fn counts(items: &[ItemSpan]) -> HashMap<(String, String), usize> {
    let mut counts = HashMap::new();
    for item in items {
        *counts
            .entry((item.component.clone(), twin_key(&item.value).to_string()))
            .or_insert(0) += 1;
    }
    counts
}

/// Closers an operator can type before the clause they belong to.
const TYPING_CLOSERS: &[char] = &[')', ']', '}', '"', '\'', '`'];

/// Shared stem required before a one-closer tail counts as the same line.
/// A stub such as `- see)` must not absorb an unrelated longer item.
const CLOSER_INSERTION_MIN_STEM: usize = 32;

/// True when `earlier` can be an earlier revision of `later` in one typing
/// burst: equal, a prefix of it, or the same line with text inserted before a
/// single closer that was already typed.
pub fn is_revision_twin(earlier: &str, later: &str) -> bool {
    let (earlier, later) = (twin_key(earlier), twin_key(later));
    if earlier.is_empty() {
        return false;
    }
    if later.starts_with(earlier) {
        return true;
    }
    let mut chars = earlier.chars();
    let Some(closer) = chars.next_back() else {
        return false;
    };
    if !TYPING_CLOSERS.contains(&closer) || !later.ends_with(closer) {
        return false;
    }
    let stem = chars.as_str();
    stem.len() >= CLOSER_INSERTION_MIN_STEM
        && later.starts_with(stem)
        && later.len() > earlier.len()
}

/// Remove from `agent` every item occurrence that the agent side added since
/// `base` and that is a revision twin (prefix, equal, or a one-closer insertion)
/// of an item the operator added since `base`. Each operator addition absorbs at
/// most as many agent
/// twins as the operator added copies of it, so the pairing is injective.
///
/// Returns the agent text unchanged when any plane fails to project: a
/// structurally invalid input is rejected by the merge caller, never here.
pub fn project_out_revision_twins(
    base: &str,
    agent: &str,
    operator: &str,
) -> RevisionTwinProjection {
    let unchanged = || RevisionTwinProjection {
        agent_text: agent.to_string(),
        collapsed: 0,
    };
    let (Some(base_items), Some(agent_items), Some(operator_items)) =
        (list_items(base), list_items(agent), list_items(operator))
    else {
        return unchanged();
    };
    let base_counts = counts(&base_items);
    let agent_counts = counts(&agent_items);
    let operator_counts = counts(&operator_items);
    let added = |counts: &HashMap<(String, String), usize>, key: &(String, String)| {
        counts
            .get(key)
            .copied()
            .unwrap_or(0)
            .saturating_sub(base_counts.get(key).copied().unwrap_or(0))
    };

    // Operator additions, in document order, each with its copy multiplicity.
    let mut slots: Vec<((String, String), usize)> = Vec::new();
    for item in &operator_items {
        let key = (item.component.clone(), twin_key(&item.value).to_string());
        if slots.iter().any(|(seen, _)| seen == &key) {
            continue;
        }
        let copies = added(&operator_counts, &key);
        if copies > 0 {
            slots.push((key, copies));
        }
    }
    if slots.is_empty() {
        return unchanged();
    }

    // Agent additions, in document order. Prefer an exact twin, then the
    // shortest operator text the agent item is a prefix of.
    let mut drops: HashMap<(String, String), usize> = HashMap::new();
    let mut visited: Vec<(String, String)> = Vec::new();
    for item in &agent_items {
        let key = (item.component.clone(), twin_key(&item.value).to_string());
        if visited.contains(&key) {
            continue;
        }
        visited.push(key.clone());
        let mut quota = added(&agent_counts, &key);
        while quota > 0 {
            let slot = slots
                .iter_mut()
                .filter(|((component, text), remaining)| {
                    *remaining > 0 && component == &key.0 && is_revision_twin(&key.1, text)
                })
                .min_by_key(|((_, text), _)| (text != &key.1, text.len()));
            let Some((_, remaining)) = slot else {
                break;
            };
            *remaining -= 1;
            *drops.entry(key.clone()).or_insert(0) += 1;
            quota -= 1;
        }
    }
    if drops.is_empty() {
        return unchanged();
    }

    // Remove the LAST occurrences: an added copy follows any baseline copy.
    let mut removals = Vec::new();
    for item in agent_items.iter().rev() {
        let key = (item.component.clone(), twin_key(&item.value).to_string());
        if let Some(remaining) = drops.get_mut(&key)
            && *remaining > 0
        {
            *remaining -= 1;
            removals.push(item.span);
        }
    }
    removals.sort_by_key(|span| std::cmp::Reverse(span.start));
    let mut agent_text = agent.to_string();
    for span in &removals {
        agent_text.replace_range(span.start..span.end, "");
    }
    RevisionTwinProjection {
        agent_text,
        collapsed: removals.len(),
    }
}

/// Number of unordered item pairs, within one list component, where one item is
/// a revision twin of the other. `None` when the document does not project.
pub fn revision_twin_pairs(doc: &str) -> Option<usize> {
    let items = list_items(doc)?;
    let mut pairs = 0;
    for (index, left) in items.iter().enumerate() {
        for right in &items[index + 1..] {
            if left.component == right.component
                && (is_revision_twin(&left.value, &right.value)
                    || is_revision_twin(&right.value, &left.value))
            {
                pairs += 1;
            }
        }
    }
    Some(pairs)
}

/// The twin-pair conservation bound a merge must respect: it may hold no more
/// revision-twin pairs than the operator already authored plus the pairs the
/// agent side added over the baseline. A pair beyond that is two images of one
/// authored item that only the merge put side by side. Returns the
/// `merged>bound` diagnostic on a violation.
pub fn merge_invents_revision_twins(
    base: &str,
    agent: &str,
    operator: &str,
    merged: &str,
) -> Option<String> {
    let (Some(base), Some(agent), Some(operator), Some(merged)) = (
        revision_twin_pairs(base),
        revision_twin_pairs(agent),
        revision_twin_pairs(operator),
        revision_twin_pairs(merged),
    ) else {
        return None;
    };
    let bound = operator + agent.saturating_sub(base);
    (merged > bound).then(|| format!("revision_twin_pairs:{merged}>{bound}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange patch=append -->\nPrior answer.\n<!-- /agent:exchange -->\n\n\
             <!-- agent:queue priority go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    const FULL: &str = "- We also need documentation on all steps. The tooling should be \
                        self-guiding. The agentic layer needs documentation and needs to";
    const CUT: &str = "- We also need documentation on all steps. The tooling should be ";

    /// The 2026-10-11 shape: the baseline holds neither revision, disk holds a
    /// cut of the item (with the trailing space it was saved with), the operator
    /// holds the longer revision. Disk's cut is dropped, nothing else is.
    #[test]
    fn stale_cut_of_an_operator_addition_is_projected_out() {
        let base = doc("- do [#first]\n");
        let disk = doc(&format!("- do [#first]\n{CUT}\n"));
        let operator = doc(&format!("- do [#first]\n{FULL}\n"));
        let projected = project_out_revision_twins(&base, &disk, &operator);
        assert_eq!(projected.collapsed, 1);
        assert_eq!(projected.agent_text, base);
    }

    #[test]
    fn independent_agent_addition_survives() {
        let base = doc("- do [#first]\n");
        let disk = doc("- do [#first]\n- disk branch\n");
        let operator = doc(&format!("- do [#first]\n{FULL}\n"));
        let projected = project_out_revision_twins(&base, &disk, &operator);
        assert_eq!(projected.collapsed, 0);
        assert_eq!(projected.agent_text, disk);
    }

    /// An agent text that EXTENDS the operator's addition is not an earlier
    /// revision of it and is never removed here (a streamed agent response is
    /// legitimately longer than its operator-visible checkpoint).
    #[test]
    fn agent_extension_of_an_operator_addition_is_not_projected() {
        let base = doc("");
        let disk = doc(&format!("{FULL}\n"));
        let operator = doc(&format!("{CUT}\n"));
        let projected = project_out_revision_twins(&base, &disk, &operator);
        assert_eq!(projected.collapsed, 0);
        // ...but a merge that kept both would be caught by the pair bound.
        let merged = doc(&format!("{CUT}\n{FULL}\n"));
        assert_eq!(
            merge_invents_revision_twins(&base, &disk, &operator, &merged).as_deref(),
            Some("revision_twin_pairs:1>0")
        );
    }

    #[test]
    fn agent_addition_unrelated_to_operator_text_survives() {
        let base = doc("- do [#first]\n");
        let disk = doc("- do [#first]\n- zzz\n");
        let operator = doc("- do [#first] more\n");
        assert_eq!(
            project_out_revision_twins(&base, &disk, &operator).collapsed,
            0
        );
    }

    #[test]
    fn each_operator_addition_absorbs_one_twin_per_copy() {
        let base = doc("");
        let disk = doc("- abc\n- ab\n");
        let operator = doc("- abcdef\n");
        let projected = project_out_revision_twins(&base, &disk, &operator);
        assert_eq!(projected.collapsed, 1, "{}", projected.agent_text);
    }

    #[test]
    fn operator_authored_twin_pairs_are_within_bound() {
        let both = doc(&format!("{FULL}\n{CUT}\n"));
        let base = doc("");
        assert_eq!(
            merge_invents_revision_twins(&base, &base, &both, &both),
            None
        );
    }

    /// 2026-10-29: the operator typed `)` and then inserted the clause before it.
    /// The cut is not a string prefix. A one-character stub and a backspace are
    /// not this pair.
    #[test]
    fn insertion_before_a_typed_closer_is_one_revision() {
        let stem = "- I'm running into confusion over published changes (";
        let earlier = format!("{stem})");
        let later = format!("{stem}like a review or a code change)");
        assert!(is_revision_twin(&earlier, &later));
        assert!(!is_revision_twin(&later, &earlier));
        assert!(!is_revision_twin("- see)", &later));
        assert!(!is_revision_twin("which is not f", "which is not to be b"));
        let base = doc("- do [#first]\n");
        let disk = doc(&format!("- do [#first]\n{earlier}\n"));
        let operator = doc(&format!("- do [#first]\n{later}\n"));
        let projected = project_out_revision_twins(&base, &disk, &operator);
        assert_eq!(projected.collapsed, 1, "{}", projected.agent_text);
        assert!(!projected.agent_text.contains("published changes ()"));
        assert!(!projected.agent_text.contains("like a review"));
    }

    #[test]
    fn non_list_components_are_out_of_scope() {
        let base = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let agent = "<!-- agent:exchange patch=append -->\nHello\n<!-- /agent:exchange -->\n";
        let operator =
            "<!-- agent:exchange patch=append -->\nHello world\n<!-- /agent:exchange -->\n";
        assert_eq!(
            project_out_revision_twins(base, agent, operator).collapsed,
            0
        );
    }
}
