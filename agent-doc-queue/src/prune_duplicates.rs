//! Explicit repair for live queue rows that are already duplicated
//! (`#pruneduplicates`).
//!
//! This is not [`crate::document_queue::dedup_live_prompts`]. That path still
//! leaves intentional free-text duplicates in place and only collapses id
//! directives. The repair here runs when an operator asks: identical live work
//! text inside one queue lineage collapses to one row, and a truncated live
//! fragment whose whole text is a token-boundary prefix of one other live row
//! in that lineage is removed.
//! Struck history is not a live row. Nothing in this module releases a claim
//! or rewrites a row it keeps.

use std::collections::HashSet;
use std::ops::Range;

use agent_doc_element_queue::strip_attempt_projection_prefix;
use anyhow::Result;

use crate::document_queue::{QueueEntry, parse_spans};

/// Why a live row is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateDropReason {
    /// Another live row in the same queue lineage has the same work text.
    IdenticalLiveHead,
    /// The row's whole work text is a truncated prefix of one other live row.
    TruncatedPrefix,
}

/// One live row the repair removes. The keeper stays byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateDrop {
    pub queue_id: String,
    pub reason: DuplicateDropReason,
    /// Work text of the removed row, after binary-owned markers are stripped.
    pub work_text: String,
    /// Work text of the row that justified keeping a copy.
    pub kept_work_text: String,
    /// The keeper carries a `[#owner#generation]` (or legacy `⟦…⟧`) marker.
    pub kept_has_worker_marker: bool,
}

/// Dry-run plan. [`Self::document`] is the successor; the caller writes it
/// only on `--apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicatePrunePlan {
    pub drops: Vec<DuplicateDrop>,
    pub document: String,
}

struct LiveRow {
    span: usize,
    /// Direct parent entry, which uniquely identifies the row's ancestor path.
    parent_span: Option<usize>,
    range: Range<usize>,
    work: String,
    /// 2 = worker annotation, 1 = any other binary-owned prefix, 0 = bare.
    claim_score: u8,
    has_children: bool,
}

impl DuplicateDrop {
    fn detail(&self) -> String {
        match self.reason {
            DuplicateDropReason::IdenticalLiveHead => format!(
                "  queue {}: drop identical live row; keep {}{}",
                self.queue_id,
                if self.kept_has_worker_marker {
                    "the claimed row: "
                } else {
                    "one row: "
                },
                preview(&self.kept_work_text)
            ),
            DuplicateDropReason::TruncatedPrefix => format!(
                "  queue {}: drop truncated prefix {}; it is a strict prefix of {}",
                self.queue_id,
                preview(&self.work_text),
                preview(&self.kept_work_text)
            ),
        }
    }
}

impl DuplicatePrunePlan {
    /// Operator report. `applied` is false for the default dry run.
    pub fn report(&self, file: &str, applied: bool) -> String {
        if self.drops.is_empty() {
            return format!("{file}: no duplicate live queue rows.\n");
        }
        let mut out = String::new();
        if applied {
            out.push_str(&format!(
                "{file}: removed {} duplicate live queue row(s). Claims were not released.\n",
                self.drops.len()
            ));
        } else {
            out.push_str(&format!(
                "{file}: dry run, {} duplicate live row(s) would be removed. Pass --apply to write. Nothing was changed.\n",
                self.drops.len()
            ));
        }
        for drop in &self.drops {
            out.push_str(&drop.detail());
            out.push('\n');
        }
        out
    }
}

/// Plan the repair for a whole document. Queues are independent: the same
/// work text in two queues is not a duplicate. An unparsable document is an
/// error and [`DuplicatePrunePlan::document`] is not a partial edit.
pub fn plan(document: &str) -> Result<DuplicatePrunePlan> {
    let components = agent_doc_element::element::parse(document)?;
    let mut drops = Vec::new();
    let mut replacements = Vec::new();
    for (ordinal, component) in components
        .iter()
        .filter(|component| component.name == "queue")
        .enumerate()
    {
        let queue_id = component
            .attrs
            .get("id")
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("queue-{}", ordinal + 1));
        let body = component.content(document);
        let spans = parse_spans(body)?;
        let (body_drops, new_body) = plan_body(&queue_id, body, &spans);
        if new_body != body {
            replacements.push((component.open_end, component.close_start, new_body));
        }
        drops.extend(body_drops);
    }
    let mut rewritten = document.to_string();
    for (start, end, new_body) in replacements.into_iter().rev() {
        rewritten.replace_range(start..end, &new_body);
    }
    Ok(DuplicatePrunePlan {
        drops,
        document: rewritten,
    })
}

fn plan_body(
    queue_id: &str,
    body: &str,
    spans: &[(QueueEntry, Range<usize>)],
) -> (Vec<DuplicateDrop>, String) {
    let live = live_rows(spans);
    let mut dropped = HashSet::new();
    let mut keyed: Vec<(usize, DuplicateDrop)> = Vec::new();

    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, row) in live.iter().enumerate() {
        if let Some(group) = groups.iter_mut().find(|group| {
            live[group[0]].parent_span == row.parent_span && live[group[0]].work == row.work
        }) {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let keeper = *group
            .iter()
            .min_by_key(|index| {
                let row = &live[**index];
                (
                    std::cmp::Reverse(row.claim_score),
                    std::cmp::Reverse(row.has_children),
                    row.span,
                )
            })
            .expect("group is non-empty");
        let kept = &live[keeper];
        for index in group {
            if index == keeper {
                continue;
            }
            let row = &live[index];
            if row.has_children {
                continue;
            }
            dropped.insert(row.span);
            keyed.push((
                row.range.start,
                drop_of(queue_id, DuplicateDropReason::IdenticalLiveHead, row, kept),
            ));
        }
    }

    for row in &live {
        if dropped.contains(&row.span) || row.has_children {
            continue;
        }
        let extensions = distinct_extensions(row, &live, &dropped);
        if !should_drop_prefix(row, &extensions) {
            continue;
        }
        let Some(kept) = keeper_of_extensions(row, &extensions, &live, &dropped) else {
            continue;
        };
        dropped.insert(row.span);
        keyed.push((
            row.range.start,
            drop_of(queue_id, DuplicateDropReason::TruncatedPrefix, row, kept),
        ));
    }

    if keyed.is_empty() {
        return (Vec::new(), body.to_string());
    }
    keyed.sort_by_key(|(at, _)| *at);
    let drops = keyed.into_iter().map(|(_, drop)| drop).collect();
    let mut ranges: Vec<Range<usize>> = live
        .iter()
        .filter(|row| dropped.contains(&row.span))
        .map(|row| row.range.clone())
        .collect();
    ranges.sort_by_key(|range| range.start);
    (drops, excise(body, &ranges))
}

fn drop_of(
    queue_id: &str,
    reason: DuplicateDropReason,
    row: &LiveRow,
    kept: &LiveRow,
) -> DuplicateDrop {
    DuplicateDrop {
        queue_id: queue_id.to_string(),
        reason,
        work_text: row.work.clone(),
        kept_work_text: kept.work.clone(),
        kept_has_worker_marker: kept.claim_score >= 2,
    }
}

fn live_rows(spans: &[(QueueEntry, Range<usize>)]) -> Vec<LiveRow> {
    spans
        .iter()
        .enumerate()
        .filter_map(|(span, (entry, range))| {
            let QueueEntry::Prompt(prompt) = entry else {
                return None;
            };
            let work = work_text(&prompt.text);
            if work.is_empty() {
                return None;
            }
            Some(LiveRow {
                span,
                parent_span: parent_span(spans, span),
                range: range.clone(),
                claim_score: claim_score(&prompt.text),
                has_children: has_nested_children(spans, span),
                work: work.to_string(),
            })
        })
        .collect()
}

fn parent_span(spans: &[(QueueEntry, Range<usize>)], index: usize) -> Option<usize> {
    let indent = entry_indent(&spans[index].0)?;
    spans[..index]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(span, (entry, _))| (entry_indent(entry)? < indent).then_some(span))
}

fn entry_indent(entry: &QueueEntry) -> Option<usize> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt.indent),
        _ => None,
    }
}

fn work_text(text: &str) -> &str {
    strip_attempt_projection_prefix(text).trim()
}

fn projection_prefix(text: &str) -> &str {
    let work = strip_attempt_projection_prefix(text);
    let split = text.len().saturating_sub(work.len());
    &text[..split]
}

fn claim_score(text: &str) -> u8 {
    let prefix = projection_prefix(text);
    if has_worker_marker(prefix) {
        2
    } else if prefix.trim().is_empty() {
        0
    } else {
        1
    }
}

fn has_worker_marker(prefix: &str) -> bool {
    bracket_owner(prefix) || legacy_owner(prefix)
}

fn bracket_owner(prefix: &str) -> bool {
    let mut rest = prefix;
    while let Some(start) = rest.find("[#") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(']') else {
            return false;
        };
        let payload = &rest[..end];
        if let Some((owner, generation)) = payload.rsplit_once('#')
            && owner_and_generation(owner, generation)
        {
            return true;
        }
        rest = &rest[end + 1..];
    }
    false
}

fn legacy_owner(prefix: &str) -> bool {
    let mut rest = prefix;
    while let Some(start) = rest.find('⟦') {
        rest = &rest[start + '⟦'.len_utf8()..];
        let Some(end) = rest.find('⟧') else {
            return false;
        };
        let payload = &rest[..end];
        let matches = if let Some(legacy) = payload.strip_prefix("owner=") {
            legacy
                .rsplit_once(";generation=")
                .is_some_and(|(owner, generation)| owner_and_generation(owner, generation))
        } else {
            payload
                .rsplit_once('#')
                .is_some_and(|(owner, generation)| owner_and_generation(owner, generation))
        };
        if matches {
            return true;
        }
        rest = &rest[end + '⟧'.len_utf8()..];
    }
    false
}

fn owner_and_generation(owner: &str, generation: &str) -> bool {
    !owner.is_empty()
        && !owner.chars().any(char::is_whitespace)
        && generation.parse::<u64>().is_ok()
}

fn has_nested_children(spans: &[(QueueEntry, Range<usize>)], index: usize) -> bool {
    let QueueEntry::Prompt(prompt) = &spans[index].0 else {
        return false;
    };
    for (entry, _) in spans.iter().skip(index + 1) {
        match entry {
            QueueEntry::Prompt(next) | QueueEntry::Completed(next) => {
                return next.indent > prompt.indent;
            }
            _ => continue,
        }
    }
    false
}

fn distinct_extensions<'a>(
    subject: &LiveRow,
    rows: &'a [LiveRow],
    dropped: &HashSet<usize>,
) -> Vec<&'a str> {
    let mut found = Vec::new();
    for row in rows {
        if dropped.contains(&row.span)
            || row.parent_span != subject.parent_span
            || row.work == subject.work
        {
            continue;
        }
        if is_token_boundary_prefix(&subject.work, &row.work) && !found.contains(&row.work.as_str())
        {
            found.push(row.work.as_str());
        }
    }
    found
}

fn is_token_boundary_prefix(prefix: &str, candidate: &str) -> bool {
    let Some(suffix) = candidate.strip_prefix(prefix) else {
        return false;
    };
    let Some(next) = suffix.chars().next() else {
        return false;
    };
    let Some(previous) = prefix.chars().last() else {
        return false;
    };
    !is_word_character(previous) || !is_word_character(next)
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn extensions_diverge(extensions: &[&str]) -> bool {
    extensions.iter().any(|left| {
        extensions
            .iter()
            .any(|right| *left != *right && !left.starts_with(*right) && !right.starts_with(*left))
    })
}

/// A truncated fragment is a strict prefix of one live sentence (or of nested
/// truncations of that same sentence). A short prefix, a finished sentence,
/// a claimed row, and a prefix shared by diverging rows stay.
fn should_drop_prefix(row: &LiveRow, extensions: &[&str]) -> bool {
    if extensions.is_empty() || extensions_diverge(extensions) {
        return false;
    }
    if row.claim_score >= 2 || row.has_children || ends_sentence(&row.work) || is_short(&row.work) {
        return false;
    }
    true
}

fn is_short(work: &str) -> bool {
    work.split_whitespace().count() < 3 || work.chars().count() < 12
}

fn ends_sentence(work: &str) -> bool {
    matches!(work.chars().last(), Some('.' | '!' | '?' | ':'))
}

fn keeper_of_extensions<'a>(
    subject: &LiveRow,
    extensions: &[&str],
    rows: &'a [LiveRow],
    dropped: &HashSet<usize>,
) -> Option<&'a LiveRow> {
    rows.iter()
        .filter(|row| {
            !dropped.contains(&row.span)
                && row.parent_span == subject.parent_span
                && extensions.contains(&row.work.as_str())
        })
        .max_by_key(|row| (row.work.len(), row.claim_score, std::cmp::Reverse(row.span)))
}

fn excise(body: &str, ranges: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut cursor = 0usize;
    for range in ranges {
        debug_assert!(range.start >= cursor && range.end <= body.len());
        out.push_str(&body[cursor..range.start]);
        cursor = range.end;
    }
    out.push_str(&body[cursor..]);
    out
}

fn preview(text: &str) -> String {
    let flat = text.replace('\n', " ");
    let mut end = flat.len().min(80);
    while end > 0 && !flat.is_char_boundary(end) {
        end -= 1;
    }
    if flat.len() > end {
        format!("{:?}…", &flat[..end])
    } else {
        format!("{flat:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNC: &str = "On this machine, tmux auto-sync opened an in-editor terminal that errorred when linking to tmux.";
    const SHELL: &str = "If the claiming tmux pane for a document is in shell (no agent-doc start running), the supervisor should run `agent-doc start` in all cases.";

    fn session() -> String {
        format!(
            "\
# Session

<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:sync#1] {SYNC}
- 🚧 [#subagent:shell#1] {SHELL}
- 🚧 [#subagent:sync#1] {SYNC}
- If the claiming
- 🚧 [#subagent:shell#1] {SHELL}
- ~~{SHELL}~~
- Document the prune command
<!-- /agent:run -->

<!-- agent:run id=\"other\" -->
- 🚧 [#subagent:sync#1] {SYNC}
<!-- /agent:run -->
"
        )
    }

    #[test]
    fn collapses_identical_live_heads_and_a_truncated_prefix() {
        let source = session();
        let plan = plan(&source).unwrap();
        let expected = format!(
            "\
# Session

<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:sync#1] {SYNC}
- 🚧 [#subagent:shell#1] {SHELL}
- ~~{SHELL}~~
- Document the prune command
<!-- /agent:run -->

<!-- agent:run id=\"other\" -->
- 🚧 [#subagent:sync#1] {SYNC}
<!-- /agent:run -->
"
        );
        assert_eq!(plan.document, expected);
        assert_eq!(plan.drops.len(), 3);
        assert!(plan.drops.iter().all(|drop| drop.queue_id == "main"));
        assert_eq!(
            plan.drops
                .iter()
                .filter(|drop| drop.reason == DuplicateDropReason::IdenticalLiveHead)
                .count(),
            2
        );
        let fragment = plan
            .drops
            .iter()
            .find(|drop| drop.reason == DuplicateDropReason::TruncatedPrefix)
            .unwrap();
        assert_eq!(fragment.work_text, "If the claiming");
        assert_eq!(fragment.kept_work_text, SHELL);
        assert!(fragment.kept_has_worker_marker);
        assert!(plan.drops.iter().all(|drop| drop.kept_has_worker_marker));
        assert_eq!(plan.document.matches("[#subagent:sync#1]").count(), 2);
        assert_eq!(plan.document.matches("[#subagent:shell#1]").count(), 1);
        let again = super::plan(&plan.document).unwrap();
        assert!(again.drops.is_empty());
        assert_eq!(again.document, plan.document);
        let dry = plan.report("session.md", false);
        assert!(dry.contains("dry run"));
        assert!(dry.contains("Nothing was changed"));
        assert!(dry.contains("If the claiming"));
        assert!(
            plan.report("session.md", true)
                .contains("Claims were not released")
        );
    }

    #[test]
    fn keeps_the_claimed_copy_when_the_earlier_copy_is_bare() {
        let source = format!(
            "\
<!-- agent:run id=\"main\" -->
- {SHELL}
- 🚧 [#subagent:shell#1] {SHELL}
<!-- /agent:run -->
"
        );
        let plan = plan(&source).unwrap();
        assert_eq!(plan.drops.len(), 1);
        assert_eq!(plan.drops[0].reason, DuplicateDropReason::IdenticalLiveHead);
        assert!(plan.drops[0].kept_has_worker_marker);
        assert!(plan.document.contains("[#subagent:shell#1]"));
        assert!(!plan.document.contains(&format!("- {SHELL}\n")));
    }

    #[test]
    fn keeps_a_lifecycle_marker_over_a_bare_duplicate() {
        let text = "Record the queue repair in the operator report and leave the claim in place.";
        let source = format!(
            "\
<!-- agent:run id=\"main\" -->
- {text}
- 🚧 {text}
<!-- /agent:run -->
"
        );
        let plan = plan(&source).unwrap();
        assert_eq!(plan.drops.len(), 1);
        assert!(!plan.drops[0].kept_has_worker_marker);
        assert!(plan.document.contains(&format!("- 🚧 {text}\n")));
        assert!(!plan.document.contains(&format!("- {text}\n")));
    }

    #[test]
    fn keeps_a_short_prefix_and_a_prefix_shared_by_diverging_rows() {
        let short = "\
<!-- agent:run id=\"main\" -->
- Fix parser
- Fix parser crash when the queue body is empty and the tests fail
<!-- /agent:run -->
";
        let shared = "\
<!-- agent:run id=\"main\" -->
- Fix
- Fix the parser crash when the queue body is empty
- Fix the release script when the tag already exists
<!-- /agent:run -->
";
        let diverging = "\
<!-- agent:run id=\"main\" -->
- Update the queue repair command so it
- Update the queue repair command so it collapses identical heads
- Update the queue repair command so it leaves struck history in place
<!-- /agent:run -->
";
        for source in [short, shared, diverging] {
            let plan = plan(source).unwrap();
            assert!(plan.drops.is_empty(), "{source}\n{plan:?}");
            assert_eq!(plan.document, source);
        }
    }

    #[test]
    fn keeps_a_finished_sentence_that_prefixes_a_longer_row() {
        let source = "\
<!-- agent:run id=\"main\" -->
- Ship the notes.
- Ship the notes. Then tag the release after the tests pass.
<!-- /agent:run -->
";
        let plan = plan(source).unwrap();
        assert!(plan.drops.is_empty());
        assert_eq!(plan.document, source);
    }

    #[test]
    fn keeps_a_mid_token_prefix_collision() {
        let source = "\
<!-- agent:run id=\"main\" -->
- Audit the queue map
- Audit the queue mapper before applying the repair
<!-- /agent:run -->
";
        let plan = plan(source).unwrap();
        assert!(plan.drops.is_empty());
        assert_eq!(plan.document, source);
    }

    #[test]
    fn keeps_a_prefix_of_only_struck_history_and_a_claimed_fragment() {
        let struck_only = format!(
            "\
<!-- agent:run id=\"main\" -->
- If the claiming
- ~~{SHELL}~~
<!-- /agent:run -->
"
        );
        let claimed_fragment = format!(
            "\
<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:fragment#1] If the claiming
- {SHELL}
<!-- /agent:run -->
"
        );
        for source in [struck_only, claimed_fragment] {
            let plan = plan(&source).unwrap();
            assert!(plan.drops.is_empty(), "{plan:?}");
            assert_eq!(plan.document, source);
        }
    }

    #[test]
    fn drops_the_childless_duplicate_and_keeps_the_parent() {
        let source = format!(
            "\
<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:sync#1] {SYNC}
- 🚧 [#subagent:sync#1] {SYNC}
  - nested note that stays under the kept parent
<!-- /agent:run -->
"
        );
        let plan = plan(&source).unwrap();
        assert_eq!(plan.drops.len(), 1);
        assert!(
            plan.document
                .contains("nested note that stays under the kept parent")
        );
        assert_eq!(plan.document.matches("[#subagent:sync#1]").count(), 1);
    }

    #[test]
    fn keeps_identical_children_under_different_parents() {
        let source = "\
<!-- agent:run id=\"main\" -->
- Parent alpha
  - Run the focused queue regression suite
- Parent beta
  - Run the focused queue regression suite
<!-- /agent:run -->
";
        let plan = plan(source).unwrap();
        assert!(plan.drops.is_empty());
        assert_eq!(plan.document, source);
    }

    #[test]
    fn does_not_collapse_the_same_text_across_queues() {
        let source = format!(
            "\
<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:sync#1] {SYNC}
<!-- /agent:run -->
<!-- agent:run id=\"other\" -->
- 🚧 [#subagent:sync#1] {SYNC}
<!-- /agent:run -->
"
        );
        let plan = plan(&source).unwrap();
        assert!(plan.drops.is_empty());
        assert_eq!(plan.document, source);
    }
}
