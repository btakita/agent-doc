//! Explicit repair for live queue rows that are already duplicated
//! (`#pruneduplicates`).
//!
//! This is not [`crate::document_queue::dedup_live_prompts`]. That path still
//! leaves intentional free-text duplicates in place and only collapses id
//! directives. The repair here runs when an operator asks: identical live work
//! text inside one queue lineage collapses to one row. A semantic prefix is
//! never inferred to be corrupt from text alone: the operator must select its
//! exact document line before a truncated live fragment whose whole text is a
//! whitespace-separated prefix of one other live row can be removed.
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
    /// A copy of a row the operator named with `--keep-row` (`#dupwrite2`):
    /// the same work text, or a strict prefix of it, at the same place.
    KeptRowCopy,
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
    /// Binary-owned lifecycle/attempt projection, without surrounding whitespace.
    projection: String,
    /// 2 = worker annotation, 1 = any other binary-owned prefix, 0 = bare.
    claim_score: u8,
    has_children: bool,
    /// One-based document line, used only by the explicit prefix selector.
    line: usize,
    /// The row's generated `[#q-…]` id, used only by `--keep-row`.
    prompt_id: Option<String>,
    /// Another row of the queue refers to the row's generated id (a reply,
    /// follow-up or relation): it is never removed (`#dupwrite2`).
    referenced: bool,
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
            DuplicateDropReason::KeptRowCopy => format!(
                "  queue {}: drop copy {}; keep the named row {}",
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
    plan_selected(document, None, &[])
}

/// Plan exact duplicate collapse plus one explicitly selected semantic prefix.
///
/// `line` is the one-based document line of the live queue row the operator
/// has identified as truncated. Prefix relationships on every other line fail
/// closed and remain byte-for-byte intact.
pub fn plan_with_truncated_prefix_line(document: &str, line: usize) -> Result<DuplicatePrunePlan> {
    plan_selected(document, Some(line), &[])
}

/// `#dupwrite2`: [`plan`] plus every live copy of each row the operator names
/// by its generated `[#q-…]` id: a row at the same place whose work text is
/// the named row's, or a strict prefix of it (at least
/// [`crate::stale_typing_copies::STALE_PREFIX_MIN_CHARS`] characters). The
/// operator's naming is the authority; each removed text is contained in
/// the row that stays, so nothing typed is lost. A copy a worker holds, or
/// one with child rows, stays. Each id must name exactly one row, live or
/// struck: a row a worker answered and struck is still the canonical copy
/// (`#dupwrite3`).
pub fn plan_keeping(document: &str, keep_rows: &[String]) -> Result<DuplicatePrunePlan> {
    plan_selected(document, None, keep_rows)
}

/// Both explicit selectors at once: `--truncated-prefix-line` and
/// `--keep-row` (see [`plan_with_truncated_prefix_line`] and
/// [`plan_keeping`]).
pub fn plan_selection(
    document: &str,
    truncated_prefix_line: Option<usize>,
    keep_rows: &[String],
) -> Result<DuplicatePrunePlan> {
    plan_selected(document, truncated_prefix_line, keep_rows)
}

fn normalize_keep_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_start_matches('#')
        .to_ascii_lowercase()
}

fn plan_selected(
    document: &str,
    truncated_prefix_line: Option<usize>,
    keep_rows: &[String],
) -> Result<DuplicatePrunePlan> {
    let keep: Vec<String> = keep_rows.iter().map(|id| normalize_keep_id(id)).collect();
    let mut kept_found: Vec<usize> = vec![0; keep.len()];
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
        let (body_drops, new_body) = plan_body(
            &queue_id,
            document,
            component.open_end,
            body,
            &spans,
            Selection {
                truncated_prefix_line,
                keep: &keep,
            },
            &mut kept_found,
        );
        if new_body != body {
            replacements.push((component.open_end, component.close_start, new_body));
        }
        drops.extend(body_drops);
    }
    for (id, found) in keep.iter().zip(&kept_found) {
        anyhow::ensure!(
            *found == 1,
            "--keep-row {id}: {} queue rows (live or struck) carry [#{id}]; it must name exactly one",
            found
        );
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

/// The operator's explicit selectors.
#[derive(Clone, Copy)]
struct Selection<'a> {
    truncated_prefix_line: Option<usize>,
    /// Normalized generated ids named with `--keep-row`.
    keep: &'a [String],
}

fn plan_body(
    queue_id: &str,
    document: &str,
    body_start: usize,
    body: &str,
    spans: &[(QueueEntry, Range<usize>)],
    selection: Selection<'_>,
    kept_found: &mut [usize],
) -> (Vec<DuplicateDrop>, String) {
    let Selection {
        truncated_prefix_line,
        keep,
    } = selection;
    let mut live = live_rows(document, body_start, spans);
    for row in &mut live {
        row.referenced = row.prompt_id.as_ref().is_some_and(|id| {
            body.to_ascii_lowercase()
                .match_indices(&format!("#{id}"))
                .any(|(at, _)| !row.range.contains(&at))
        });
    }
    let mut dropped = HashSet::new();
    let mut conflicting_projection_rows = HashSet::new();
    let mut keyed: Vec<(usize, DuplicateDrop)> = Vec::new();

    // `#dupwrite2`: copies of a row the operator named go first. The named
    // row may be struck (`#dupwrite3`): a worker answered it after the
    // copies appeared, and it is still the canonical copy.
    let struck = if keep.is_empty() {
        Vec::new()
    } else {
        struck_rows(document, body_start, spans)
    };
    for (slot, id) in keep.iter().enumerate() {
        let named: Vec<&LiveRow> = live
            .iter()
            .chain(&struck)
            .filter(|row| row.prompt_id.as_deref() == Some(id.as_str()))
            .collect();
        kept_found[slot] += named.len();
        let [kept] = named[..] else {
            continue;
        };
        for row in &live {
            let copy = row.work == kept.work
                || (row.work.chars().count() >= crate::stale_typing_copies::STALE_PREFIX_MIN_CHARS
                    && kept.work.len() > row.work.len()
                    && kept.work.starts_with(row.work.as_str()));
            if row.span == kept.span
                || !copy
                || row.parent_span != kept.parent_span
                || row.has_children
                || row.claim_score >= 2
                || (row.referenced && referenced_apart_from(body, spans, row, id))
                || dropped.contains(&row.span)
            {
                continue;
            }
            dropped.insert(row.span);
            keyed.push((
                row.range.start,
                drop_of(queue_id, DuplicateDropReason::KeptRowCopy, row, kept),
            ));
        }
    }

    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, row) in live.iter().enumerate() {
        if dropped.contains(&row.span) {
            continue;
        }
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
        // The pure planner has no durable claim authority. If two otherwise
        // identical rows disagree about their binary-owned lifecycle or
        // owner/generation projection, neither is safe to discard.
        if has_conflicting_projections(&group, &live) {
            conflicting_projection_rows.extend(group.iter().map(|index| live[*index].span));
            continue;
        }
        let keeper = *group
            .iter()
            .min_by_key(|index| {
                let row = &live[**index];
                (
                    std::cmp::Reverse(row.claim_score),
                    std::cmp::Reverse(row.referenced),
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
            if row.has_children || row.referenced {
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
        if dropped.contains(&row.span)
            || conflicting_projection_rows.contains(&row.span)
            || row.has_children
            || truncated_prefix_line != Some(row.line)
        {
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

/// `#dupwrite3`: some reference to `row`'s generated id sits in a queue row
/// tree that does not also name `kept_id`. A row tree naming the kept row
/// and a copy together (the request to collapse them, or the kept row's own
/// response) refers to the duplicate set, not to that one copy; a reply,
/// follow-up or relation to the copy alone still holds it.
fn referenced_apart_from(
    body: &str,
    spans: &[(QueueEntry, Range<usize>)],
    row: &LiveRow,
    kept_id: &str,
) -> bool {
    let Some(id) = row.prompt_id.as_deref() else {
        return false;
    };
    let lower = body.to_ascii_lowercase();
    let kept_marker = format!("#{kept_id}");
    lower
        .match_indices(&format!("#{id}"))
        .filter(|(at, _)| !row.range.contains(at))
        .any(|(at, _)| {
            let tree = root_tree_range(spans, at).unwrap_or(at..at);
            !lower[tree].contains(&kept_marker)
        })
}

/// The byte range of the top-level queue row tree holding byte `at`: its
/// root entry and the entries after it that nest beneath it.
fn root_tree_range(spans: &[(QueueEntry, Range<usize>)], at: usize) -> Option<Range<usize>> {
    let index = spans.iter().position(|(_, range)| range.contains(&at))?;
    let root = root_span(spans, index);
    let end = spans
        .iter()
        .enumerate()
        .skip(root)
        .take_while(|(span, _)| root_span(spans, *span) == root)
        .map(|(_, (_, range))| range.end)
        .max()?;
    Some(spans[root].1.start..end)
}

fn root_span(spans: &[(QueueEntry, Range<usize>)], mut index: usize) -> usize {
    while let Some(parent) = parent_span(spans, index) {
        index = parent;
    }
    index
}

fn has_conflicting_projections(group: &[usize], live: &[LiveRow]) -> bool {
    let mut projection = None;
    for index in group {
        let current = live[*index].projection.as_str();
        if current.is_empty() {
            continue;
        }
        match projection {
            None => projection = Some(current),
            Some(expected) if expected == current => {}
            Some(_) => return true,
        }
    }
    false
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

fn live_rows(
    document: &str,
    body_start: usize,
    spans: &[(QueueEntry, Range<usize>)],
) -> Vec<LiveRow> {
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
            let prompt_id = agent_doc_element_queue::queue_prompt_stable_id(&prompt.text)
                .filter(|id| id.starts_with("q-"));
            Some(LiveRow {
                span,
                parent_span: parent_span(spans, span),
                range: range.clone(),
                claim_score: claim_score(&prompt.text),
                has_children: has_nested_children(spans, span),
                projection: projection_prefix(&prompt.text).trim().to_string(),
                work,
                prompt_id,
                referenced: false,
                line: document[..body_start + range.start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1,
            })
        })
        .collect()
}

/// `#dupwrite3`: struck (completed) rows, as candidates for the row
/// `--keep-row` names. They are never removed or compared otherwise.
fn struck_rows(
    document: &str,
    body_start: usize,
    spans: &[(QueueEntry, Range<usize>)],
) -> Vec<LiveRow> {
    spans
        .iter()
        .enumerate()
        .filter_map(|(span, (entry, range))| {
            let QueueEntry::Completed(prompt) = entry else {
                return None;
            };
            let work = work_text(&prompt.text);
            if work.is_empty() {
                return None;
            }
            let prompt_id = agent_doc_element_queue::queue_prompt_stable_id(&prompt.text)
                .filter(|id| id.starts_with("q-"));
            Some(LiveRow {
                span,
                parent_span: parent_span(spans, span),
                range: range.clone(),
                claim_score: claim_score(&prompt.text),
                has_children: true,
                projection: projection_prefix(&prompt.text).trim().to_string(),
                work,
                prompt_id,
                referenced: false,
                line: document[..body_start + range.start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1,
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

/// Work text with every binary-owned marker removed: the lifecycle/attempt
/// prefix, and (`#dupwrite2`) a generated `[#q-…]` occurrence id and a
/// `⟦effect:N⟧` duplicate ordinal, which a copy is minted on its own.
fn work_text(text: &str) -> String {
    let work = strip_attempt_projection_prefix(text).trim();
    let work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(work);
    let work = crate::document_queue::strip_queue_effect_annotation(&work);
    agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(&work)
        .trim()
        .to_string()
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
        if is_whitespace_boundary_prefix(&subject.work, &row.work)
            && !found.contains(&row.work.as_str())
        {
            found.push(row.work.as_str());
        }
    }
    found
}

fn is_whitespace_boundary_prefix(prefix: &str, candidate: &str) -> bool {
    let Some(suffix) = candidate.strip_prefix(prefix) else {
        return false;
    };
    suffix.chars().next().is_some_and(char::is_whitespace)
}

fn extensions_diverge(extensions: &[&str]) -> bool {
    extensions.iter().any(|left| {
        extensions.iter().any(|right| {
            *left != *right
                && !is_whitespace_boundary_prefix(left, right)
                && !is_whitespace_boundary_prefix(right, left)
        })
    })
}

/// After exact-line authorization, a truncated fragment is a strict prefix of
/// one live sentence (or of nested truncations of that same sentence). A short
/// prefix, a finished sentence, a claimed row, and a prefix shared by diverging
/// rows stay. Callers never reach this predicate for an unselected row.
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

    fn line_of(source: &str, needle: &str) -> usize {
        source[..source.find(needle).expect("fixture row")]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
    }

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
        let plan =
            plan_with_truncated_prefix_line(&source, line_of(&source, "- If the claiming\n"))
                .unwrap();
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
    fn semantic_prefixes_require_an_explicit_document_line() {
        let source = "\
<!-- agent:run id=\"main\" -->
- 📥 Run queue tests
- Run queue tests before publishing the release
<!-- /agent:run -->
";
        let fail_closed = plan(source).unwrap();
        assert!(fail_closed.drops.is_empty(), "{fail_closed:?}");
        assert_eq!(fail_closed.document, source);

        let selected =
            plan_with_truncated_prefix_line(source, line_of(source, "- 📥 Run queue tests\n"))
                .unwrap();
        assert_eq!(selected.drops.len(), 1, "{selected:?}");
        assert_eq!(
            selected.drops[0].reason,
            DuplicateDropReason::TruncatedPrefix
        );
        assert!(!selected.document.contains("- 📥 Run queue tests\n"));
        assert!(
            selected
                .document
                .contains("- Run queue tests before publishing the release\n")
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
    fn keeps_conflicting_attempt_projections_but_prunes_the_same_attempt() {
        let text = "Run the independently reviewed queue repair.";
        let conflicting = format!(
            "\
<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:old#1] {text}
- ☑️ [#subagent:current#2] {text}
<!-- /agent:run -->
"
        );
        let conflicting_plan = plan(&conflicting).unwrap();
        assert!(conflicting_plan.drops.is_empty());
        assert_eq!(conflicting_plan.document, conflicting);
        assert_eq!(
            conflicting_plan.report("session.md", false),
            "session.md: no duplicate live queue rows.\n"
        );
        assert_eq!(
            conflicting_plan.report("session.md", true),
            "session.md: no duplicate live queue rows.\n"
        );

        let same_attempt = format!(
            "\
<!-- agent:run id=\"main\" -->
- 🚧 [#subagent:current#2] {text}
- 🚧 [#subagent:current#2] {text}
<!-- /agent:run -->
"
        );
        let same_attempt_plan = plan(&same_attempt).unwrap();
        assert_eq!(same_attempt_plan.drops.len(), 1);
        assert_eq!(
            same_attempt_plan
                .document
                .matches("🚧 [#subagent:current#2]")
                .count(),
            1
        );
    }

    #[test]
    fn prefix_pruning_keeps_conflicting_lifecycle_projections() {
        let text = "Run the complete queue verification";
        let source = format!(
            "\
<!-- agent:run id=\"main\" -->
- 📥 {text}
- ☑️ {text}
- {text} before release
<!-- /agent:run -->
"
        );
        let plan = plan(&source).unwrap();
        assert!(plan.drops.is_empty(), "{plan:?}");
        assert_eq!(plan.document, source);
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
    fn keeps_a_punctuation_bearing_identifier_prefix_collision() {
        let source = "\
<!-- agent:run id=\"main\" -->
- Compile with C++
- Compile with C++20 support before release
<!-- /agent:run -->
";
        let plan = plan(source).unwrap();
        assert!(plan.drops.is_empty());
        assert_eq!(plan.document, source);
    }

    #[test]
    fn keeps_a_shared_prefix_when_punctuation_bearing_extensions_diverge() {
        let source = "\
<!-- agent:run id=\"main\" -->
- Release build with
- Release build with C++
- Release build with C++20 support before release
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

    // ---- `#dupwrite2` -----------------------------------------------------

    const PIPE: &str = "I do like the pipe operator for commands as well.";
    const LINK: &str = "[Declare lang](https://example.invalid/declare/) looks interesting.";

    /// The live `agent:run` tail of 2026-11-24 once preflight admitted the
    /// copies a retained merge wrote (`#dupwrite`): generated `[#q-…]` ids,
    /// `⟦effect:N⟧` ordinals, and two bare copies fenced in `---`.
    fn admitted_live() -> String {
        format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- ### 📥 {pipe} ⟦effect:1⟧ [#q-8536fadf0285]\n",
                "---\n",
                "📥 {link} [#q-217e769965d7]\n",
                "---\n",
                "- ### 📥 {link} Is this a potential way to declare ⟦effect:1⟧ [#q-0feb5aebf877]\n",
                "- ### 📥 {pipe} ⟦effect:2⟧ [#q-c26e3ea230ac]\n",
                "- ### ~~🗑️ do [#subagentworkpool-8ac1]~~\n",
                "- ### 📥 {link} Is this a potential way to declare guis from agent-doc [#q-140c87b4f232]\n",
                "- ### 📥 {pipe} ⟦effect:3⟧ [#q-158f900202b6]\n",
                "---\n",
                "📥 {link} [#q-12d6f4ea6884]\n",
                "---\n",
                "- ### 📥 {link} Is this a potential way to declare ⟦effect:2⟧ [#q-37cd908220ef]\n",
                "- ### 📥 {pipe} ⟦effect:4⟧ [#q-9e92831a7369]\n",
                "- ### 📥 {link} Is this a potential way to declare guis from agent-doc to present information? [#q-68b3244de244]\n",
                "<!-- /agent:run -->\n",
            ),
            pipe = PIPE,
            link = LINK,
        )
    }

    /// A generated `[#q-…]` and a `⟦effect:N⟧` ordinal are binary-owned:
    /// rows that differ only by them are identical live rows. Measured on the
    /// live document: `prune-duplicates` reported "no duplicate live queue
    /// rows" for four copies of one row.
    #[test]
    fn dupwrite2_rows_differing_only_in_generated_ids_and_ordinals_are_identical() {
        let plan = plan(&admitted_live()).unwrap();
        let pipes = plan
            .drops
            .iter()
            .filter(|drop| drop.work_text == PIPE)
            .count();
        assert_eq!(pipes, 3, "{:?}", plan.drops);
        assert_eq!(plan.document.matches(PIPE).count(), 1, "{}", plan.document);
        // The two fenced bare copies are identical too; each goes with its
        // fence, and the two `… declare` copies collapse.
        assert_eq!(plan.document.matches("[#q-217e769965d7]").count(), 1);
        assert_eq!(plan.document.matches("[#q-12d6f4ea6884]").count(), 0);
        assert_eq!(
            plan.document.matches("---\n").count(),
            2,
            "{}",
            plan.document
        );
        assert_eq!(
            plan.document.matches("potential way to declare ⟦").count(),
            1
        );
        let again = super::plan(&plan.document).unwrap();
        assert!(again.drops.is_empty(), "{:?}", again.drops);
    }

    /// `--keep-row <q-id>`: the operator names the row to keep; every other
    /// live copy of it (identical work text, or a strict prefix of it, at
    /// the same place) goes. Each removed text is contained in the kept row.
    #[test]
    fn dupwrite2_keep_row_removes_every_copy_of_the_named_row() {
        let plan = plan_keeping(&admitted_live(), &["q-68b3244de244".to_string()]).unwrap();
        let expected = format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- ### 📥 {pipe} ⟦effect:1⟧ [#q-8536fadf0285]\n",
                "- ### ~~🗑️ do [#subagentworkpool-8ac1]~~\n",
                "- ### 📥 {link} Is this a potential way to declare guis from agent-doc to present information? [#q-68b3244de244]\n",
                "<!-- /agent:run -->\n",
            ),
            pipe = PIPE,
            link = LINK,
        );
        assert_eq!(plan.document, expected, "{:?}", plan.drops);
        assert_eq!(plan.drops.len(), 8, "{:?}", plan.drops);
        assert_eq!(
            plan.drops
                .iter()
                .filter(|drop| drop.reason == DuplicateDropReason::KeptRowCopy)
                .count(),
            5,
            "{:?}",
            plan.drops
        );
        let report = plan.report("session.md", false);
        assert!(report.contains("keep the named row"), "{report}");
        // Idempotent.
        let again = plan_keeping(&plan.document, &["#q-68b3244de244".to_string()]).unwrap();
        assert!(again.drops.is_empty(), "{:?}", again.drops);
    }

    #[test]
    fn dupwrite2_a_copy_another_row_refers_to_stays_and_keeps() {
        let source = format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- {pipe} [#q-0000000000b1]\n",
                "- {pipe} ⟦effect:2⟧ [#q-0000000000b2]\n",
                "- re [#q-0000000000b2]: yes\n",
                "- {link} [#q-0000000000b3]\n",
                "- re [#q-0000000000b3]: noted\n",
                "- {link} Is this a way [#q-0000000000b4]\n",
                "<!-- /agent:run -->\n",
            ),
            pipe = PIPE,
            link = LINK,
        );
        let plan = plan_keeping(&source, &["q-0000000000b4".to_string()]).unwrap();
        assert_eq!(
            plan.document,
            source.replace(&format!("- {PIPE} [#q-0000000000b1]\n"), ""),
            "{:?}",
            plan.drops
        );
    }

    #[test]
    fn dupwrite2_keep_row_needs_one_live_row_and_spares_workers_and_parents() {
        assert!(plan_keeping(&admitted_live(), &["q-000000000000".to_string()]).is_err());
        let source = format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- 🚧 [#subagent:a#1] {link} Is this\n",
                "- {link} Is this a way\n",
                "  - keep my note\n",
                "- {link}\n",
                "- {link} Is this a way to declare guis [#q-0000000000a1]\n",
                "<!-- /agent:run -->\n",
            ),
            link = LINK,
        );
        let plan = plan_keeping(&source, &["q-0000000000a1".to_string()]).unwrap();
        assert_eq!(
            plan.document,
            source.replace(&format!("- {LINK}\n"), ""),
            "{:?}",
            plan.drops
        );
    }

    // ---- `#dupwrite3` -----------------------------------------------------

    const FINISHED_ROW: &str = "Is this a potential way to declare guis from agent-doc to present information? [#q-68b3244de244]";

    /// [`admitted_live`] once a worker answered the finished row: it is struck
    /// with a response tree, a follow-up row names it, and the coordinator's
    /// claimed row asking for this repair names it and every copy.
    fn answered_live() -> String {
        admitted_live().replace(
            &format!("- ### 📥 {LINK} {FINISHED_ROW}\n"),
            &format!(
                concat!(
                    "- ### ~~{link} {finished}~~\n",
                    "  - **Response [#r-fda0ffecd016]**\n",
                    "    - **Summary:** Declare can draw GUIs.\n",
                    "    - **State:** Complete.\n",
                    "- 🙋 [#q-68b3244de244] follow-up: operator: decide whether to run the experiment. [operator-verify] [#q-ac7ab7b7c892]\n",
                    "- 🚧 [#subagent:dupwrite3#1] [from:coordinator key=dupwrite3] agent: #dupwrite3: `--keep-row q-68b3244de244` refuses once [#q-68b3244de244] is struck, so ([#q-217e769965d7], [#q-12d6f4ea6884], [#q-0feb5aebf877], [#q-37cd908220ef], [#q-140c87b4f232]) stay. Test first.\n",
                ),
                link = LINK,
                finished = FINISHED_ROW,
            ),
        )
    }

    /// Measured 2026-11-24: once a worker answered and struck the row the
    /// operator named, `--keep-row q-68b3244de244` refused ("0 live queue rows
    /// carry") and the five Declare-lang copies stayed live, offered for
    /// dispatch. A struck row is still the canonical copy: every live copy
    /// whose text it contains goes, and the struck row, its response tree and
    /// its follow-up stay byte for byte. A row that names the kept row and
    /// the copies together (the request for this repair) refers to the
    /// duplicate set, not to one copy, so it does not hold a copy.
    #[test]
    fn dupwrite3_keep_row_accepts_a_struck_answered_row() {
        let source = answered_live();
        let plan = plan_keeping(&source, &["q-68b3244de244".to_string()]).unwrap();
        let kept: Vec<&DuplicateDrop> = plan
            .drops
            .iter()
            .filter(|drop| drop.reason == DuplicateDropReason::KeptRowCopy)
            .collect();
        assert_eq!(kept.len(), 5, "{:?}", plan.drops);
        for id in [
            "q-217e769965d7",
            "q-12d6f4ea6884",
            "q-0feb5aebf877",
            "q-37cd908220ef",
            "q-140c87b4f232",
        ] {
            assert_eq!(
                plan.document.matches(&format!("{id}]")).count(),
                1,
                "{id} must survive only in the coordinator row:\n{}",
                plan.document
            );
        }
        let after = &plan.document[plan.document.find("<!-- agent:run").unwrap()..];
        let tail = &source[source.find("- ### ~~[Declare").unwrap()..];
        assert!(after.ends_with(tail), "{after}");
        assert!(!plan.document.contains("---\n"), "{}", plan.document);
        // Idempotent.
        let again = plan_keeping(&plan.document, &["q-68b3244de244".to_string()]).unwrap();
        assert!(again.drops.is_empty(), "{:?}", again.drops);
    }

    /// A struck kept row keeps every guard: a copy another row refers to
    /// (without naming the kept row) stays, and an id carried by a struck and
    /// a live row at once names two rows.
    #[test]
    fn dupwrite3_struck_keep_row_keeps_referenced_copies_and_needs_one_row() {
        let source = answered_live().replace(
            "- 🙋 [#q-68b3244de244]",
            "- re [#q-140c87b4f232]: why is this here?\n- 🙋 [#q-68b3244de244]",
        );
        let plan = plan_keeping(&source, &["q-68b3244de244".to_string()]).unwrap();
        assert!(
            plan.document.contains(&format!(
                "{LINK} Is this a potential way to declare guis from agent-doc [#q-140c87b4f232]"
            )),
            "{}",
            plan.document
        );
        assert_eq!(
            plan.drops
                .iter()
                .filter(|drop| drop.reason == DuplicateDropReason::KeptRowCopy)
                .count(),
            4,
            "{:?}",
            plan.drops
        );

        let twice = format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- ### ~~{link} declare guis [#q-0000000000c1]~~\n",
                "- {link} declare guis [#q-0000000000c1]\n",
                "- {link} [#q-0000000000c2]\n",
                "<!-- /agent:run -->\n",
            ),
            link = LINK,
        );
        let error = plan_keeping(&twice, &["q-0000000000c1".to_string()]).unwrap_err();
        assert!(error.to_string().contains("2 queue rows"), "{error}");
    }
}
