//! Pure queue projection helpers.
//!
//! Queue lifecycle markers are visible projections of active queue heads. They
//! are not queue identity and do not commit or admit a turn by themselves.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use agent_doc_frontmatter::frontmatter::queue_icons;

pub const PRIORITIZED_MARKERS: [&str; 7] = [
    "**prioritized**",
    "__prioritized__",
    "**pin**",
    "__pin__",
    ":pushpin:",
    ":pin:",
    "📌",
];
pub const AGENT_PRIORITIZED_MARKERS: [&str; 6] = [
    "*prioritized*",
    "_prioritized_",
    "*pin*",
    "_pin_",
    ":round_pushpin:",
    "📍",
];
// Canonical pin markers the binary INJECTS use the direct emoji (📌 / 📍) rather
// than the `:pushpin:` / `:round_pushpin:` shortcodes, matching the `🚧`
// in-progress marker. Both shortcode and emoji spellings remain accepted on parse
// (see `PRIORITIZED_MARKERS` / `AGENT_PRIORITIZED_MARKERS`), so existing documents
// that already carry the shortcodes keep working unchanged.
pub const PRIORITIZED_MARKER: &str = "📌";
pub const AGENT_PRIORITIZED_MARKER: &str = "📍";
pub const IN_PROGRESS_MARKER: &str = "🚧";
/// Live queue work that has no current coordinator selection, worker attempt,
/// report receipt, or operator gate. Pending is explicit so removing a stale
/// active marker never leaves the operator wondering whether projection ran.
pub const PENDING_MARKER: &str = "📥";
/// `#queueblocked`: live queue work whose tracked item still waits on open
/// prerequisite work. It is followed by a [`BLOCKED_ANNOTATION_OPEN`]
/// annotation naming the blocking ids, and is held out of drain and dispatch.
pub const BLOCKED_MARKER: &str = "⛔";
/// Opening delimiter of the blocker annotation that follows [`BLOCKED_MARKER`]:
/// `⛔ ⟦blocked by a, b⟧ do [#x]`. The ids carry no `#`, so a scanner that
/// reads the first `#id` of a raw row never mistakes a blocker for the row's
/// own id. The annotation is part of the lifecycle token: every parser that
/// strips the lifecycle marker strips it too, so it is never work identity.
pub const BLOCKED_ANNOTATION_OPEN: &str = "⟦blocked by ";
/// Closing delimiter of the blocker annotation.
pub const BLOCKED_ANNOTATION_CLOSE: &str = "⟧";
/// Claimed for dispatch, but no positive worker-liveness evidence exists yet.
/// `⏳` already means "awaiting operator" in existing documents, so claims use
/// a yellow state marker instead (`#queueclaimstate`).
pub const CLAIMED_MARKER: &str = "🟡";
/// The claimed worker has positive dispatch/liveness evidence.
pub const RUNNING_MARKER: &str = IN_PROGRESS_MARKER;
/// The worker reported and the result is awaiting coordinator integration.
/// `☑️` communicates that the worker's part is complete while the live queue row
/// still records coordinator integration/writeback debt.
pub const REPORTED_MARKER: &str = "☑️";
/// Reported/outbox marker projected by releases before the `☑️` default.
/// Existing documents remain readable and normalize on the next projection.
pub const LEGACY_REPORTED_MARKER: &str = "📫";
/// Older reported/outbox marker projected before the `📫` default.
pub const OLDER_REPORTED_MARKER: &str = "📤";
/// The attempt was lost or expired and is eligible for recovery/re-dispatch.
pub const LOST_MARKER: &str = "⚠️";
/// Durable work evidence exists, but it cannot be bound to a current claim
/// generation. The coordinator must reconcile/close the row; it is not fresh
/// pending work (`#queueworkevidence`).
pub const RECONCILING_MARKER: &str = "🔄";
/// Agent-doc could not read enough durable lifecycle state to name an owner.
/// This is deliberately different from `Lost`: it authorizes diagnosis, not
/// redispatch, and prevents a stale visible marker from claiming certainty.
pub const UNKNOWN_MARKER: &str = "❔";
/// A concrete operator action blocks this queue head. Unlike the mutually
/// exclusive lifecycle markers, this annotation is orthogonal: a worker may
/// still be running (`🚧`) while it waits for the operator's answer. The
/// following `[#id]` links the queue row to the request packet in
/// `agent:exchange` and the operator-owned row in `agent:todo`.
pub const OPERATOR_ACTION_MARKER: &str = "🙋";
/// Delimiters for the canonical owner/generation annotation carried by worker
/// attempt projections. Ordinary pending/coordinator rows omit it.
pub const OWNER_ANNOTATION_OPEN: &str = "[#";
pub const OWNER_ANNOTATION_CLOSE: &str = "]";
/// Older owner/generation annotations used mathematical double brackets. Keep
/// accepting them so existing session documents remain readable.
pub const LEGACY_OWNER_ANNOTATION_OPEN: &str = "⟦";
pub const LEGACY_OWNER_ANNOTATION_CLOSE: &str = "⟧";
const LEGACY_OWNER_ANNOTATION_PREFIX: &str = "owner=";
const LEGACY_OWNER_GENERATION_SEPARATOR: &str = ";generation=";
const OWNER_GENERATION_SEPARATOR: char = '#';
/// Visible projection marker for a queue head the binary has SKIPPED: it was
/// dispatched, came back unconsumed, and the queue advanced past it to a
/// non-dependent drainable head (`#orphanqhead` / `#queueskip`). Like `🚧`, it is
/// a re-derived cosmetic projection — it never changes the head's identity (it is
/// stripped by `strip_in_progress_marker` / `strip_priority_markers`) and clears
/// automatically once the head becomes drainable/consumed again.
pub const SKIP_MARKER: &str = "⏭\u{fe0f}";
/// Visible projection marker for a live queue head whose agent work is finished
/// and which now waits on the operator (`#queueawaitop`): its tracked item is
/// `[operator-verify]` (no inline operator verdict yet) or a gated
/// `agent:review` row, and no worker holds a claim on it. Without it, a head
/// whose subagent finished and released its claim reverted to a bare
/// `do [#id]` that read as "unstarted" (observed 2026-10-11 on
/// `src/haiven-dev/tasks/sdk.md`). Like `🚧`/`⏭️` it is a re-derived cosmetic
/// projection: stripped for identity and cleared automatically once the head is
/// consumed, claimed again, or no longer waits on the operator.
pub const AWAITING_OPERATOR_MARKER: &str = "⏳";

pub fn operator_action_marker() -> &'static str {
    &queue_icons().operator_action
}

pub fn prioritized_marker() -> &'static str {
    &queue_icons().prioritized
}

pub fn agent_prioritized_marker() -> &'static str {
    &queue_icons().agent_prioritized
}

/// Every built-in leading lifecycle marker the binary projects onto or accepts
/// from a queue head. Legacy spellings remain here so they never enter work
/// identity even when a custom palette is installed.
pub const LIFECYCLE_MARKERS: [&str; 12] = [
    PENDING_MARKER,
    BLOCKED_MARKER,
    CLAIMED_MARKER,
    RUNNING_MARKER,
    REPORTED_MARKER,
    LEGACY_REPORTED_MARKER,
    OLDER_REPORTED_MARKER,
    LOST_MARKER,
    RECONCILING_MARKER,
    UNKNOWN_MARKER,
    SKIP_MARKER,
    AWAITING_OPERATOR_MARKER,
];

/// Effective and built-in lifecycle prefixes accepted by parsers that operate
/// on already-rendered queue text.
pub fn lifecycle_markers() -> Vec<&'static str> {
    let mut markers = LIFECYCLE_MARKERS.to_vec();
    for state in ALL_LIFECYCLE_STATES {
        let marker = state.prefix();
        if !markers.contains(&marker) {
            markers.push(marker);
        }
    }
    markers
}

/// Binary-owned visible lifecycle states. These prefixes are projection only:
/// every parser strips them before queue-item identity is computed.
pub use agent_doc_frontmatter::frontmatter::{ALL_LIFECYCLE_STATES, QueueLifecycleMarker};

/// Formal worker-state projection (`#queueworkevidence`). For a live queue row
/// `q`, let `A(q)` be a bound claim attempt, `R(q)` a durable reported-release
/// receipt, and `F(q)` a binary-attested completion cell. Then:
///
/// `Bare(q) <=> !A(q) && !R(q) && !F(q)`.
///
/// A bound attempt has strongest authority; a report receipt means `Reported`;
/// otherwise completion evidence is generation-ambiguous and means
/// `Reconciling`. Callers set `completion_feedback` only when the cell's
/// recorded item is this row's own work (`completion_evidence_matches_row`):
/// a longer `do [#id] …` line shares the tracked id and is not this row.
/// Closure removes the row, so this function applies only while `q` is live.
/// A cosmetic marker alone is not authority.
pub fn worker_evidence_marker(
    bound_attempt: Option<QueueLifecycleMarker>,
    reported_receipt: bool,
    completion_feedback: bool,
) -> Option<QueueLifecycleMarker> {
    if let Some(bound) = bound_attempt {
        return Some(bound);
    }
    if reported_receipt {
        return Some(QueueLifecycleMarker::Reported);
    }
    completion_feedback.then_some(QueueLifecycleMarker::Reconciling)
}

fn strip_lifecycle_prefix(text: &str) -> Option<&str> {
    lifecycle_prefix(text).map(|(_, _, rest)| rest)
}

fn lifecycle_prefix(text: &str) -> Option<(QueueLifecycleMarker, &str, &str)> {
    // `#queueblocked`: a bare blocker annotation (its `⛔` removed by hand or
    // written under another palette) still reads as the blocked lifecycle
    // token, so it never leaks into work identity.
    if let Some(len) = blocked_annotation_len(text) {
        return Some((QueueLifecycleMarker::Blocked, &text[..len], &text[len..]));
    }
    let (state, prefix, rest) = lifecycle_marker_prefix(text)?;
    if state == QueueLifecycleMarker::Blocked {
        let gap = rest.len() - rest.trim_start().len();
        if let Some(len) = blocked_annotation_len(&rest[gap..]) {
            let end = prefix.len() + gap + len;
            return Some((state, &text[..end], &text[end..]));
        }
    }
    Some((state, prefix, rest))
}

/// Byte length of a leading `⟦blocked by ...⟧` annotation on one line.
fn blocked_annotation_len(text: &str) -> Option<usize> {
    let after_open = text.strip_prefix(BLOCKED_ANNOTATION_OPEN)?;
    let close = after_open.find(BLOCKED_ANNOTATION_CLOSE)?;
    if after_open[..close].contains(['\n', '\r']) {
        return None;
    }
    Some(BLOCKED_ANNOTATION_OPEN.len() + close + BLOCKED_ANNOTATION_CLOSE.len())
}

/// The canonical blocker annotation for `blockers` (`⟦blocked by a, b⟧`), or
/// `None` when there is nothing to name. Ids are normalized (lowercase, no
/// `#`) and deduplicated; anything outside `[a-z0-9_-]` is dropped so the
/// annotation can never carry its own delimiter or a newline.
pub fn format_blocked_annotation(blockers: &[String]) -> Option<String> {
    let mut ids: Vec<String> = Vec::new();
    for raw in blockers {
        let id: String = raw
            .trim()
            .trim_start_matches('#')
            .to_ascii_lowercase()
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    (!ids.is_empty()).then(|| {
        format!(
            "{BLOCKED_ANNOTATION_OPEN}{}{BLOCKED_ANNOTATION_CLOSE}",
            ids.join(", ")
        )
    })
}

/// The blocking ids named by a row's leading `⛔ ⟦blocked by a, b⟧` token, in
/// annotation order. Empty when the row is not blocked or names no ids.
pub fn blocked_marker_ids(text: &str) -> Vec<String> {
    let mut rest = text.trim_start();
    loop {
        if let Some((state, token, _)) = lifecycle_prefix(rest) {
            if state != QueueLifecycleMarker::Blocked {
                return Vec::new();
            }
            let Some(open) = token.find(BLOCKED_ANNOTATION_OPEN) else {
                return Vec::new();
            };
            let payload = &token[open + BLOCKED_ANNOTATION_OPEN.len()..];
            let payload = payload
                .strip_suffix(BLOCKED_ANNOTATION_CLOSE)
                .unwrap_or(payload);
            return payload
                .split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect();
        }
        if let Some(after) = strip_operator_action_marker_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = priority_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        return Vec::new();
    }
}

fn lifecycle_marker_prefix(text: &str) -> Option<(QueueLifecycleMarker, &str, &str)> {
    ALL_LIFECYCLE_STATES
        .into_iter()
        .flat_map(|state| {
            [
                (state, state.prefix()),
                (state, state.default_prefix()),
                (
                    state,
                    if state == QueueLifecycleMarker::Reported {
                        LEGACY_REPORTED_MARKER
                    } else {
                        state.default_prefix()
                    },
                ),
                (
                    state,
                    if state == QueueLifecycleMarker::Reported {
                        OLDER_REPORTED_MARKER
                    } else {
                        state.default_prefix()
                    },
                ),
            ]
        })
        .filter_map(|(state, prefix)| text.strip_prefix(prefix).map(|rest| (state, prefix, rest)))
        .max_by_key(|(_, prefix, _)| prefix.len())
}

fn strip_operator_action_marker_prefix(text: &str) -> Option<&str> {
    [operator_action_marker(), OPERATOR_ACTION_MARKER]
        .into_iter()
        .filter_map(|marker| text.strip_prefix(marker).map(|rest| (marker.len(), rest)))
        .max_by_key(|(len, _)| *len)
        .map(|(_, rest)| rest)
}

fn strip_operator_action_prefix(text: &str) -> Option<(&str, &str)> {
    let after_marker = strip_operator_action_marker_prefix(text)?.trim_start();
    let after_open = after_marker.strip_prefix("[#")?;
    let (id, rest) = after_open.split_once(']')?;
    let mut chars = id.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
        return None;
    }
    // `🙋 [#subagent-operator-request-x]: answer` is the canonical marker
    // followed by a local answer row, not the legacy `🙋 [#id] <work>`
    // prefix. Keep the request token and answer together as the work body.
    if is_reserved_operator_action_id(id) && rest.trim_start().starts_with(':') {
        return None;
    }
    Some((id, rest))
}

fn trailing_operator_action_annotation(text: &str) -> Option<(&str, &str, usize)> {
    // A queue prompt may quote an operator-answer row as inline Markdown, for
    // example ``why did `- [#subagent-operator-request-x]: yes` truncate?``.
    // Such evidence is prose, not the prompt's own annotation. Looking only at
    // the final `[#` made `strip_priority_markers` truncate that prompt at the
    // opening backtick, which in turn truncated `queue brief` and its worker
    // authorization. Use the shared Markdown literal ranges and keep looking
    // backwards until an unquoted candidate is found.
    let code_ranges = agent_doc_element::element::find_code_ranges(text);
    for (start, _) in text.rmatch_indices("[#") {
        if code_ranges
            .iter()
            .any(|(range_start, range_end)| start >= *range_start && start < *range_end)
        {
            continue;
        }
        if start > 0
            && !text[..start]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace)
        {
            continue;
        }
        let after_open = &text[start + 2..];
        let Some((id, after)) = after_open.split_once(']') else {
            continue;
        };
        let mut chars = id.chars();
        let Some(first) = chars.next() else {
            continue;
        };
        if !first.is_ascii_alphanumeric()
            || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            || !(after.is_empty() || after.trim_start().starts_with(':'))
        {
            continue;
        }
        return Some((id, after, start));
    }
    None
}

fn is_reserved_operator_action_id(id: &str) -> bool {
    id.to_ascii_lowercase()
        .starts_with("subagent-operator-request-")
}

fn render_projection_parts(parts: &[String]) -> String {
    let mut rendered = String::new();
    for part in parts {
        if !rendered.is_empty()
            && !(rendered == operator_action_marker() && is_lifecycle_prefix(part))
        {
            rendered.push(' ');
        }
        rendered.push_str(part);
    }
    rendered
}

fn priority_prefix(text: &str) -> Option<(&str, &str)> {
    [prioritized_marker(), agent_prioritized_marker()]
        .into_iter()
        .chain(PRIORITIZED_MARKERS)
        .chain(AGENT_PRIORITIZED_MARKERS)
        .filter_map(|marker| text.strip_prefix(marker).map(|rest| (marker, rest)))
        .max_by_key(|(marker, _)| marker.len())
}

fn is_lifecycle_prefix(prefix: &str) -> bool {
    ALL_LIFECYCLE_STATES
        .into_iter()
        .any(|state| prefix == state.prefix() || prefix == state.default_prefix())
        || prefix == LEGACY_REPORTED_MARKER
        || prefix == OLDER_REPORTED_MARKER
        // `#queueblocked`: `⛔ ⟦blocked by a⟧` is one lifecycle token.
        || lifecycle_prefix(prefix).is_some_and(|(_, token, rest)| {
            token.len() == prefix.len() && rest.is_empty()
        })
}

/// Human-readable identity of the exact durable worker attempt governing a
/// queue row. It is projection metadata, never part of queue work identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueOwnerProjection {
    pub owner: String,
    pub generation: u64,
}

/// Parse and remove one leading exact-attempt owner annotation.
///
/// New projections use `[#<owner>#<generation>]`. The earlier compact
/// `⟦<owner>#<generation>⟧` and long
/// `⟦owner=<owner>;generation=<generation>⟧` spellings remain readable so an
/// installed binary can normalize documents written by an older release.
pub fn strip_owner_annotation_prefix(text: &str) -> Option<(QueueOwnerProjection, &str)> {
    let (payload, rest, legacy_delimiters) =
        if let Some(after_open) = text.strip_prefix(OWNER_ANNOTATION_OPEN) {
            let (payload, rest) = after_open.split_once(OWNER_ANNOTATION_CLOSE)?;
            (payload, rest, false)
        } else {
            let after_open = text.strip_prefix(LEGACY_OWNER_ANNOTATION_OPEN)?;
            let (payload, rest) = after_open.split_once(LEGACY_OWNER_ANNOTATION_CLOSE)?;
            (payload, rest, true)
        };
    let (owner, generation) = if legacy_delimiters {
        if let Some(legacy) = payload.strip_prefix(LEGACY_OWNER_ANNOTATION_PREFIX) {
            legacy.rsplit_once(LEGACY_OWNER_GENERATION_SEPARATOR)?
        } else {
            payload.rsplit_once(OWNER_GENERATION_SEPARATOR)?
        }
    } else {
        payload.rsplit_once(OWNER_GENERATION_SEPARATOR)?
    };
    if owner.is_empty() || owner.chars().any(|ch| ch == '\n' || ch == '\r') {
        return None;
    }
    let generation = generation.parse().ok()?;
    Some((
        QueueOwnerProjection {
            owner: owner.to_string(),
            generation,
        },
        rest,
    ))
}

fn format_owner_annotation(owner: &QueueOwnerProjection) -> String {
    let safe_owner = owner
        .owner
        .replace(['\n', '\r'], " ")
        .replace(OWNER_ANNOTATION_CLOSE, "?")
        .replace(LEGACY_OWNER_ANNOTATION_CLOSE, "?");
    format!(
        "{OWNER_ANNOTATION_OPEN}{safe_owner}{OWNER_GENERATION_SEPARATOR}{}{OWNER_ANNOTATION_CLOSE}",
        owner.generation
    )
}

/// Recover the narrow torn projection produced when editor convergence drops
/// the closing `]` between a duplicated owner annotation and lifecycle marker.
/// The exact owner/generation is required as positive durable evidence, so an
/// operator-authored lookalike is never stripped speculatively.
fn strip_torn_duplicate_owner_lifecycle_prefix<'a>(
    text: &'a str,
    owner: &QueueOwnerProjection,
) -> Option<&'a str> {
    let annotation = format_owner_annotation(owner);
    let torn = annotation.strip_suffix(OWNER_ANNOTATION_CLOSE)?;
    let after_owner = text.strip_prefix(torn)?.trim_start();
    strip_lifecycle_prefix(after_owner)
}

/// Read the first exact-attempt owner annotation from a leading projection
/// prefix sequence.
pub fn owner_projection(text: &str) -> Option<QueueOwnerProjection> {
    let mut rest = text.trim_start();
    loop {
        if let Some((owner, _)) = strip_owner_annotation_prefix(rest) {
            return Some(owner);
        }
        if let Some(after) = strip_lifecycle_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = strip_owner_annotation_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = strip_operator_action_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_operator_action_marker_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = priority_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        return None;
    }
}

/// Replace the exact-attempt owner annotation while preserving lifecycle,
/// operator-action, priority, leading, and trailing whitespace. The annotation
/// is placed immediately after a lifecycle marker when one exists.
pub fn apply_owner_projection(text: &str, owner: Option<&QueueOwnerProjection>) -> String {
    let content_start = text.len() - text.trim_start().len();
    let leading = &text[..content_start];
    let (content, trailing) = split_trailing_whitespace(&text[content_start..]);
    let mut rest = content.trim_start();
    let mut prefixes = Vec::<String>::new();
    let mut legacy_operator_ids = Vec::<String>::new();
    let mut observed_owner = None::<QueueOwnerProjection>;
    loop {
        if let Some((projected_owner, after)) = strip_owner_annotation_prefix(rest) {
            observed_owner.get_or_insert(projected_owner);
            rest = after.trim_start();
            continue;
        }
        let repaired = observed_owner
            .as_ref()
            .and_then(|projected_owner| {
                strip_torn_duplicate_owner_lifecycle_prefix(rest, projected_owner)
            })
            .or_else(|| {
                owner.and_then(|projected_owner| {
                    strip_torn_duplicate_owner_lifecycle_prefix(rest, projected_owner)
                })
            });
        if let Some(after) = repaired {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, marker, after)) = lifecycle_prefix(rest) {
            prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        if let Some((id, after)) = strip_operator_action_prefix(rest) {
            prefixes.push(operator_action_marker().to_string());
            legacy_operator_ids.push(id.to_ascii_lowercase());
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_operator_action_marker_prefix(rest) {
            prefixes.push(operator_action_marker().to_string());
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = priority_prefix(rest) {
            prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        break;
    }
    if let Some(index) = prefixes
        .iter()
        .position(|prefix| is_lifecycle_prefix(prefix))
    {
        let lifecycle = prefixes.remove(index);
        let insert_at = prefixes
            .first()
            .is_some_and(|prefix| prefix == operator_action_marker())
            as usize;
        prefixes.insert(insert_at, lifecycle);
    }
    if let Some(owner) = owner {
        let annotation = format_owner_annotation(owner);
        let insert_at = prefixes
            .iter()
            .position(|prefix| is_lifecycle_prefix(prefix))
            .map_or(0, |index| index + 1);
        prefixes.insert(insert_at, annotation);
    }
    if !rest.is_empty() {
        let mut body = rest.to_string();
        for id in legacy_operator_ids {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(&format!("[#{id}]"));
        }
        prefixes.push(body);
    }
    format!("{leading}{}{trailing}", render_projection_parts(&prefixes))
}

/// Request ids carried by canonical trailing `[#id]` annotations or legacy
/// leading `🙋 [#id]` annotations while `🙋` is active.
pub fn operator_action_ids(text: &str) -> Vec<String> {
    // `#nestedsubprompts`: request annotations live on the parent part. A
    // sub-prompt row never lends its parent a request id.
    let full = text;
    let text = agent_doc_element_queue::subprompts::head_without_subprompts(text);
    let mut rest = text.trim_start();
    let mut ids = Vec::new();
    let mut marked = false;
    loop {
        if let Some((id, after)) = strip_operator_action_prefix(rest) {
            marked = true;
            let id = id.to_ascii_lowercase();
            if !ids.contains(&id) {
                ids.push(id);
            }
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_operator_action_marker_prefix(rest) {
            marked = true;
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_lifecycle_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = strip_owner_annotation_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = priority_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        break;
    }
    if let Some((id, after, _)) = trailing_operator_action_annotation(text.trim())
        && (marked
            || (is_reserved_operator_action_id(id)
                && after
                    .trim_start()
                    .strip_prefix(':')
                    .is_some_and(|answer| !answer.trim().is_empty())))
    {
        let id = id.to_ascii_lowercase();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if let Some((id, _)) = nested_operator_answer(full) {
        let id = id.to_ascii_lowercase();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// `#queueoperatoranswer`: the lowercased request id a row answers when the row is
/// nothing but an operator answer: the canonical local answer
/// `[#subagent-operator-request-x]: <answer>` or the nested shape
/// ([`nested_operator_answer`]), with any projection prefix. `None` for a work
/// row, including one that carries a request annotation after its work text.
pub fn operator_answer_row_request_id(text: &str) -> Option<String> {
    if let Some((id, _)) = nested_operator_answer(text) {
        return Some(id.to_ascii_lowercase());
    }
    let head = agent_doc_element_queue::subprompts::head_without_subprompts(text);
    let head = agent_doc_element_queue::strip_attempt_projection_prefix(head).trim();
    let (id, after) = head.strip_prefix("[#")?.split_once(']')?;
    (is_reserved_operator_action_id(id)
        && after
            .trim_start()
            .strip_prefix(':')
            .is_some_and(|answer| !answer.trim().is_empty()))
    .then(|| id.to_ascii_lowercase())
}

/// `#queueoperatoranswer`: lowercased reserved request ids a WORK row carries, marked
/// (`🙋`) or not: the canonical trailing `[#subagent-operator-request-x]`
/// annotation and legacy leading `🙋 [#id]` prefixes. Empty for an answer row
/// ([`operator_answer_row_request_id`]), which answers a request rather than
/// pausing work on it.
pub fn operator_request_annotation_ids(text: &str) -> Vec<String> {
    if operator_answer_row_request_id(text).is_some() {
        return Vec::new();
    }
    let head = agent_doc_element_queue::subprompts::head_without_subprompts(text);
    let mut ids = operator_action_ids(head)
        .into_iter()
        .filter(|id| is_reserved_operator_action_id(id))
        .collect::<Vec<_>>();
    if let Some((id, _, start)) = trailing_operator_action_annotation(head.trim())
        && start > 0
        && is_reserved_operator_action_id(id)
    {
        let id = id.to_ascii_lowercase();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// `#queueoperatoranswer`: the operator's answer written as its own row keyed by the
/// request id, with the answer nested beneath it:
///
/// ```text
/// - [#subagent-operator-request-x]
///   - I want sync to use the latest state.
/// ```
///
/// It means exactly what the canonical local answer row
/// `[#subagent-operator-request-x]: I want sync to use the latest state.`
/// means. Returns the request id (as written) and the answer: the live nested
/// rows' work text, joined by newlines. `None` for any other shape, for a
/// parent that carries work text beside the id, and when no live nested row
/// has text (an unanswered or already-struck block).
pub fn nested_operator_answer(text: &str) -> Option<(&str, String)> {
    use agent_doc_element_queue::subprompts;
    let (head, _) = subprompts::split_subprompt_block(text);
    let head = agent_doc_element_queue::strip_attempt_projection_prefix(head).trim();
    let (id, after) = head.strip_prefix("[#")?.split_once(']')?;
    let after = after.trim();
    if !is_reserved_operator_action_id(id) || !(after.is_empty() || after == ":") {
        return None;
    }
    let answer = subprompts::subprompt_rows(text)
        .into_iter()
        .filter(|row| !subprompts::subprompt_row_is_struck(row))
        .map(|row| {
            agent_doc_element_queue::strip_attempt_projection_prefix(&row)
                .trim()
                .to_string()
        })
        .filter(|row| !row.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!answer.is_empty()).then_some((id, answer))
}

/// Whether an operator request still holds this queue row open
/// (`#operatoractionstrike`, `#answeredrequeststrike`).
///
/// [`operator_action_ids`] also returns the id of a locally ANSWERED canonical
/// row (`[#subagent-operator-request-x]: <answer>`, no `🙋`), because the
/// answer still has to route to its todo. That id is answer provenance, not an
/// open request. The row is held only while it visibly carries `🙋`, or while
/// one of its request ids still names an open todo (`open_todo_ids`,
/// lowercased): the same rule the `🙋` projection uses
/// (`set_prompt_operator_actions`). Without this, an answered `☑️` row whose
/// todo was closed showed no `🙋` yet every completion path vetoed it, so its
/// exact closeout echo never struck it (haiven-dev sdk-demo.md, 2026-10-28).
pub fn operator_action_holds_row(
    text: &str,
    open_todo_ids: &std::collections::HashSet<String>,
) -> bool {
    let ids = operator_action_ids(text);
    if ids.is_empty() {
        return false;
    }
    operator_action_marked(text) || ids.iter().any(|id| open_todo_ids.contains(id))
}

/// True when the row's leading projection prefixes carry the `🙋` marker.
pub fn operator_action_marked(text: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if strip_operator_action_marker_prefix(rest).is_some() {
            return true;
        }
        if let Some(after) = strip_lifecycle_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = strip_owner_annotation_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, after)) = priority_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        return false;
    }
}

/// Whether the operator supplied a non-empty answer beside `request_id`.
///
/// Canonical rows append `[#request-id]`; adding `: <answer>` resolves the
/// request locally. Legacy prefix rows are also recognized when the operator
/// appended `: <answer>` to the original queue item.
pub fn operator_action_is_answered(text: &str, request_id: &str, queue_item: &str) -> bool {
    if !operator_action_ids(text)
        .iter()
        .any(|id| id.eq_ignore_ascii_case(request_id))
    {
        return false;
    }
    if nested_operator_answer(text).is_some_and(|(id, _)| id.eq_ignore_ascii_case(request_id)) {
        return true;
    }
    let text = agent_doc_element_queue::subprompts::head_without_subprompts(text);
    if let Some((id, after, _)) = trailing_operator_action_annotation(text.trim())
        && id.eq_ignore_ascii_case(request_id)
        && after
            .trim_start()
            .strip_prefix(':')
            .is_some_and(|answer| !answer.trim().is_empty())
    {
        return true;
    }
    let bare = strip_priority_markers(text);
    bare.strip_prefix(queue_item)
        .and_then(|tail| tail.trim_start().strip_prefix(':'))
        .is_some_and(|answer| !answer.trim().is_empty())
}

fn strip_operator_action_annotations_preserving_markers(text: &str) -> String {
    let mut rest = text.trim();
    let mut kept_prefixes: Vec<String> = Vec::new();
    let mut legacy_ids: Vec<String> = Vec::new();
    loop {
        let trimmed = rest.trim_start();
        if let Some((id, after)) = strip_operator_action_prefix(trimmed) {
            legacy_ids.push(id.to_ascii_lowercase());
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_operator_action_marker_prefix(trimmed) {
            rest = after.trim_start();
            continue;
        }
        if let Some((_, marker, after)) = lifecycle_prefix(trimmed) {
            kept_prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        if let Some((owner, after)) = strip_owner_annotation_prefix(trimmed) {
            kept_prefixes.push(format_owner_annotation(&owner));
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = priority_prefix(trimmed) {
            kept_prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        rest = trimmed;
        break;
    }
    if !rest.trim().is_empty() {
        kept_prefixes.push(rest.trim().to_string());
    }
    let mut rendered = render_projection_parts(&kept_prefixes);
    for id in legacy_ids {
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        rendered.push_str(&format!("[#{id}]"));
    }
    rendered
}

/// Replace the leading operator-action annotations while preserving worker
/// lifecycle and priority projections. `ids` are normalized and deduplicated.
pub fn apply_operator_action_ids(text: &str, ids: &[String]) -> String {
    // `#nestedsubprompts` / `#queueoperatoranswer`: the annotation belongs to the parent
    // part. Appending a missing `[#id]` to the whole text would land it after
    // the last nested row and re-segment the row.
    if let (head, Some(block)) = agent_doc_element_queue::subprompts::split_subprompt_block(text) {
        return format!("{}\n{block}", apply_operator_action_ids(head, ids));
    }
    let content_start = text.len() - text.trim_start().len();
    let leading = &text[..content_start];
    let (content, trailing) = split_trailing_whitespace(&text[content_start..]);
    let mut normalized = Vec::new();
    for id in ids {
        let id = id.trim().trim_start_matches('#').to_ascii_lowercase();
        let mut chars = id.chars();
        if chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
            && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && !normalized.contains(&id)
        {
            normalized.push(id);
        }
    }
    let bare = strip_operator_action_annotations_preserving_markers(content);
    if normalized.is_empty() {
        return format!("{leading}{bare}{trailing}");
    }
    let mut projected = bare;
    for id in &normalized {
        let already_local = trailing_operator_action_annotation(&projected)
            .is_some_and(|(existing, _, _)| existing.eq_ignore_ascii_case(id));
        if !already_local {
            if !projected.is_empty() {
                projected.push(' ');
            }
            projected.push_str(&format!("[#{id}]"));
        }
    }
    let separator = if lifecycle_prefix(&projected).is_some() {
        ""
    } else {
        " "
    };
    format!(
        "{leading}{}{separator}{projected}{trailing}",
        operator_action_marker()
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuePromptRow {
    pub text: String,
    pub id: Option<String>,
    pub projectable_default: bool,
}

impl QueuePromptRow {
    pub fn new(text: impl Into<String>, id: Option<String>, projectable_default: bool) -> Self {
        Self {
            text: text.into(),
            id,
            projectable_default,
        }
    }

    pub fn unmarked_text(&self) -> String {
        strip_in_progress_marker(&self.text)
    }

    pub fn marked_in_progress(&self) -> bool {
        has_in_progress_marker(&self.text)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveQueuePromptProjection {
    pub prompts: Vec<String>,
    pub retargeted: bool,
    pub missing_dependency_ids: Vec<String>,
}

/// Strip the leading in-progress marker while preserving priority markers.
pub fn strip_in_progress_marker(text: &str) -> String {
    let mut rest = text.trim();
    let mut kept_prefixes: Vec<String> = Vec::new();
    let mut legacy_operator_ids: Vec<String> = Vec::new();
    let mut observed_owner = None::<QueueOwnerProjection>;
    loop {
        let trimmed = rest.trim_start();
        if let Some((id, after_marker)) = strip_operator_action_prefix(trimmed) {
            kept_prefixes.push(operator_action_marker().to_string());
            legacy_operator_ids.push(id.to_ascii_lowercase());
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = strip_operator_action_marker_prefix(trimmed) {
            kept_prefixes.push(operator_action_marker().to_string());
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((owner, after_marker)) = strip_owner_annotation_prefix(trimmed) {
            observed_owner.get_or_insert(owner);
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = observed_owner
            .as_ref()
            .and_then(|owner| strip_torn_duplicate_owner_lifecycle_prefix(trimmed, owner))
        {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = strip_lifecycle_prefix(trimmed) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((marker, after_marker)) = priority_prefix(trimmed) {
            kept_prefixes.push(marker.to_string());
            rest = after_marker.trim_start();
            continue;
        }
        rest = trimmed;
        break;
    }
    let mut out = if kept_prefixes.is_empty() {
        rest.trim().to_string()
    } else {
        let mut out = render_projection_parts(&kept_prefixes);
        if !rest.trim().is_empty() {
            out.push(' ');
            out.push_str(rest.trim());
        }
        out
    };
    for id in legacy_operator_ids {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("[#{id}]"));
    }
    out
}

pub fn apply_in_progress_marker(text: &str) -> String {
    apply_lifecycle_marker(text, QueueLifecycleMarker::Running)
}

/// Replace any existing lifecycle prefix with `marker`, preserving pins and
/// trailing whitespace.
pub fn apply_lifecycle_marker(text: &str, marker: QueueLifecycleMarker) -> String {
    apply_lifecycle_marker_with_blockers(text, marker, &[])
}

/// [`apply_lifecycle_marker`] that also renders the blocker annotation
/// (`⛔ ⟦blocked by a, b⟧`) when `marker` is
/// [`QueueLifecycleMarker::Blocked`] and `blockers` names at least one id
/// (`#queueblocked`). Other markers ignore `blockers`.
pub fn apply_lifecycle_marker_with_blockers(
    text: &str,
    marker: QueueLifecycleMarker,
    blockers: &[String],
) -> String {
    let (content, trailing) = split_trailing_whitespace(text);
    let stripped = strip_in_progress_marker(content);
    let token = match (marker, format_blocked_annotation(blockers)) {
        (QueueLifecycleMarker::Blocked, Some(annotation)) => {
            format!("{} {annotation}", marker.prefix())
        }
        _ => marker.prefix().to_string(),
    };
    if let Some(after_operator) = strip_operator_action_marker_prefix(&stripped) {
        format!(
            "{}{token} {}{trailing}",
            operator_action_marker(),
            after_operator.trim_start()
        )
    } else {
        format!("{token} {stripped}{trailing}")
    }
}

/// Prepend the skip projection marker (`⏭️`) to a queue head, preserving pins and
/// dropping any existing in-progress/skip marker so exactly one lifecycle marker
/// leads the line.
pub fn apply_skip_marker(text: &str) -> String {
    apply_lifecycle_marker(text, QueueLifecycleMarker::Skipped)
}

/// Prepend the awaiting-operator projection marker (`⏳`, `#queueawaitop`),
/// preserving pins and replacing any existing lifecycle marker.
pub fn apply_awaiting_operator_marker(text: &str) -> String {
    apply_lifecycle_marker(text, QueueLifecycleMarker::AwaitingOperator)
}

fn split_trailing_whitespace(text: &str) -> (&str, &str) {
    let content = text.trim_end();
    (&text[..content.len()], &text[content.len()..])
}

pub fn has_in_progress_marker(text: &str) -> bool {
    lifecycle_marker(text) == Some(QueueLifecycleMarker::Running)
}

pub fn lifecycle_marker(text: &str) -> Option<QueueLifecycleMarker> {
    let mut rest = text.trim_start();
    loop {
        if let Some((state, _, _)) = lifecycle_prefix(rest) {
            return Some(state);
        }
        if let Some((_, after_marker)) = strip_operator_action_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = strip_operator_action_marker_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((_, after_marker)) = strip_owner_annotation_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((_, after_marker)) = priority_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        return None;
    }
}

/// True when the head carries a leading skip marker (`⏭️`), past any pins.
pub fn has_skip_marker(text: &str) -> bool {
    lifecycle_marker(text) == Some(QueueLifecycleMarker::Skipped)
}

/// True when the head carries a leading awaiting-operator marker (`⏳`).
pub fn has_awaiting_operator_marker(text: &str) -> bool {
    lifecycle_marker(text) == Some(QueueLifecycleMarker::AwaitingOperator)
}

/// True when the head carries any leading lifecycle marker (`🚧`/`⏭️`/`⏳`).
pub fn has_lifecycle_marker(text: &str) -> bool {
    lifecycle_marker(text).is_some()
}

pub fn strip_in_progress_marker_for_display(text: &str) -> Option<String> {
    if !has_lifecycle_marker(text) {
        return None;
    }
    let (content, trailing) = split_trailing_whitespace(text);
    let mut stripped = strip_in_progress_marker(content);
    stripped.push_str(trailing);
    Some(stripped)
}

pub fn set_in_progress_work_item_markers(
    content: &str,
    active_ids: &HashSet<String>,
) -> Result<(String, bool)> {
    let lifecycle_by_id = active_ids
        .iter()
        .map(|id| (id.clone(), QueueLifecycleMarker::Running))
        .collect();
    set_work_item_lifecycle_markers(content, &lifecycle_by_id)
}

/// Mirror typed queue lifecycle state into linked tracked-work rows. Queue
/// state is the source; stale tracked-work markers are removed in the same
/// projection pass.
pub fn set_work_item_lifecycle_markers(
    content: &str,
    lifecycle_by_id: &HashMap<String, QueueLifecycleMarker>,
) -> Result<(String, bool)> {
    let mut updated = content.to_string();
    let mut changed = false;
    let components = agent_doc_element::element::parse(&updated)?;
    for component in components
        .iter()
        .filter(|component| {
            matches!(
                component.name.as_str(),
                "backlog" | "pending" | "review" | "icebox"
            )
        })
        .rev()
    {
        let (new_body, body_changed) =
            agent_doc_element_backlog::backlog::set_lifecycle_item_markers(
                component.content(&updated),
                lifecycle_by_id,
            );
        if body_changed {
            updated = component.replace_content(&updated, &new_body);
            changed = true;
        }
    }
    Ok((updated, changed))
}

pub fn sync_in_progress_marker_regions(snapshot_content: &str, current_content: &str) -> String {
    let Ok(current_components) = agent_doc_element::element::parse(current_content) else {
        return snapshot_content.to_string();
    };
    let mut updated = snapshot_content.to_string();
    for name in ["queue", "backlog", "pending", "review", "icebox"] {
        let Some(current_component) = current_components
            .iter()
            .find(|component| component.name == name)
        else {
            continue;
        };
        let current_body = current_component.content(current_content);
        let Ok(snapshot_components) = agent_doc_element::element::parse(&updated) else {
            return updated;
        };
        let Some(snapshot_component) = snapshot_components
            .iter()
            .find(|component| component.name == name)
        else {
            continue;
        };
        if snapshot_component.content(&updated) != current_body {
            updated = snapshot_component.replace_content(&updated, current_body);
        }
    }
    updated
}

/// Strip priority and lifecycle (`🚧`/`⏭️`/`⏳`) markers from the visible prompt
/// identity, so a marker- or pin-decorated head compares equal to its bare form.
pub fn strip_priority_markers(text: &str) -> String {
    let mut t = text.trim();
    let mut canonical_operator_action = false;
    let mut observed_owner = None::<QueueOwnerProjection>;
    loop {
        let trimmed = t.trim_start();
        let stripped = strip_lifecycle_prefix(trimmed)
            .or_else(|| strip_operator_action_prefix(trimmed).map(|(_, rest)| rest))
            .or_else(|| {
                strip_operator_action_marker_prefix(trimmed).inspect(|_| {
                    canonical_operator_action = true;
                })
            });
        if let Some(rest) = stripped {
            t = rest.trim_start();
            continue;
        }
        if let Some((owner, rest)) = strip_owner_annotation_prefix(trimmed) {
            observed_owner.get_or_insert(owner);
            t = rest.trim_start();
            continue;
        }
        if let Some(rest) = observed_owner
            .as_ref()
            .and_then(|owner| strip_torn_duplicate_owner_lifecycle_prefix(trimmed, owner))
        {
            t = rest.trim_start();
            continue;
        }
        if let Some((_, rest)) = priority_prefix(trimmed) {
            t = rest.trim_start();
            continue;
        }
        break;
    }
    let mut bare = t.trim();
    if let Some((id, after, start)) = trailing_operator_action_annotation(bare)
        && (canonical_operator_action || is_reserved_operator_action_id(id))
    {
        // A local answer row has no originating work text before the request
        // annotation: `[#subagent-operator-request-x]: answer`. Preserve the
        // reserved request token as its stable identity so the row can receive
        // 📥/🟡/🚧/☑️ lifecycle projection. Ordinary trailing annotations keep
        // keying to the originating work text.
        let identity_end = bare.len().saturating_sub(after.len());
        bare = if start == 0 {
            bare[..identity_end].trim_end()
        } else {
            bare[..start].trim_end()
        };
    }
    bare.to_string()
}

/// True when the operator moved the `🚧` marker onto a head the binary would
/// not have selected. `binary_projected` holds the heads (marker-stripped text)
/// the binary itself last projected `🚧` onto (`#stalemarkerretarget`): a marker
/// on one of those rows is the binary's own projection, not a retarget, even
/// when a baseline rebuilt from HEAD lost it and the diff shows it as added.
/// Otherwise a stale projection outranks heads the operator queued above it.
pub fn in_progress_marker_retarget_requested(
    diff: Option<&str>,
    rows: &[QueuePromptRow],
    binary_projected: &HashSet<String>,
) -> bool {
    if !rows.iter().any(|row| {
        row.marked_in_progress() && !binary_projected.contains(&strip_priority_markers(&row.text))
    }) {
        return false;
    }
    let Some(diff) = diff else {
        return false;
    };
    diff.lines().any(|line| {
        (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
            && (line.contains(QueueLifecycleMarker::Running.prefix())
                || line.contains(IN_PROGRESS_MARKER))
    })
}

pub fn project_active_queue_prompts(
    rows: &[QueuePromptRow],
    after_deps: &HashMap<String, Vec<String>>,
    honor_in_progress_markers: bool,
    skipped_ids: &HashSet<String>,
) -> ActiveQueuePromptProjection {
    let normalized = rows
        .iter()
        .map(|row| {
            (
                row.unmarked_text(),
                row.id.clone(),
                row.marked_in_progress(),
                // `#queueskip`: a head whose id is in the skipped set is not a
                // dispatch target — treat it as non-projectable so selection
                // advances past it. (It stays in the queue with a `⏭️` marker.)
                row.projectable_default
                    && !row.id.as_ref().is_some_and(|id| skipped_ids.contains(id)),
            )
        })
        .collect::<Vec<_>>();

    let marked = if honor_in_progress_markers {
        normalized
            .iter()
            .filter(|(_, _, marked, _)| *marked)
            .map(|(text, id, _, _)| (text.clone(), id.clone()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if marked.is_empty() {
        // `#queueskip`: pick the first projectable head, but SKIP any head whose
        // transitive `after=` closure depends on a skipped head — that follower is
        // NOT "non-dependent", so it must not run ahead of the skipped blocker. If
        // every projectable head depends on a skipped head (or none are
        // projectable), the selection is empty and today's stall-stop applies.
        let mut id_present = HashSet::<String>::new();
        let mut id_deps_local = HashMap::<String, Vec<String>>::new();
        for (text, id, _, _) in &normalized {
            if let Some(id) = id {
                id_present.insert(id.clone());
                let mut merged = after_deps.get(id).cloned().unwrap_or_default();
                for inline in agent_doc_element_backlog::backlog::item_after_deps(text) {
                    if !merged.contains(&inline) {
                        merged.push(inline);
                    }
                }
                if !merged.is_empty() {
                    id_deps_local.entry(id.clone()).or_insert(merged);
                }
            }
        }
        fn depends_on_skipped(
            id: &str,
            id_deps: &HashMap<String, Vec<String>>,
            id_present: &HashSet<String>,
            skipped_ids: &HashSet<String>,
            visiting: &mut HashSet<String>,
        ) -> bool {
            if !visiting.insert(id.to_string()) {
                return false;
            }
            let blocked = id_deps.get(id).is_some_and(|deps| {
                deps.iter().any(|dep| {
                    (id_present.contains(dep) && skipped_ids.contains(dep))
                        || depends_on_skipped(dep, id_deps, id_present, skipped_ids, visiting)
                })
            });
            visiting.remove(id);
            blocked
        }
        let selected = normalized
            .iter()
            .find(|(_, id, _, projectable)| {
                *projectable
                    && id.as_ref().is_none_or(|id| {
                        !depends_on_skipped(
                            id,
                            &id_deps_local,
                            &id_present,
                            skipped_ids,
                            &mut HashSet::new(),
                        )
                    })
            })
            .map(|(text, _, _, _)| text.clone());
        return ActiveQueuePromptProjection {
            prompts: selected.into_iter().collect(),
            retargeted: false,
            missing_dependency_ids: Vec::new(),
        };
    }

    let mut id_to_text = HashMap::<String, String>::new();
    let mut id_deps = HashMap::<String, Vec<String>>::new();
    for (text, id, _, _) in &normalized {
        if let Some(id) = id {
            id_to_text.entry(id.clone()).or_insert_with(|| text.clone());
            let mut merged = after_deps.get(id).cloned().unwrap_or_default();
            for inline in agent_doc_element_backlog::backlog::item_after_deps(text) {
                if !merged.contains(&inline) {
                    merged.push(inline);
                }
            }
            if !merged.is_empty() {
                id_deps.entry(id.clone()).or_insert(merged);
            }
        }
    }

    let mut selected_ids = HashSet::<String>::new();
    let mut selected_id_order = Vec::<String>::new();
    let mut selected_free_texts = HashSet::<String>::new();
    let mut missing_dependency_ids = BTreeSet::<String>::new();

    fn visit(
        id: &str,
        id_to_text: &HashMap<String, String>,
        id_deps: &HashMap<String, Vec<String>>,
        selected_ids: &mut HashSet<String>,
        selected_id_order: &mut Vec<String>,
        visiting: &mut HashSet<String>,
        missing_dependency_ids: &mut BTreeSet<String>,
    ) {
        if selected_ids.contains(id) {
            return;
        }
        if !visiting.insert(id.to_string()) {
            return;
        }
        if let Some(dep_ids) = id_deps.get(id) {
            for dep in dep_ids {
                if id_to_text.contains_key(dep) {
                    visit(
                        dep,
                        id_to_text,
                        id_deps,
                        selected_ids,
                        selected_id_order,
                        visiting,
                        missing_dependency_ids,
                    );
                } else {
                    missing_dependency_ids.insert(dep.clone());
                }
            }
        }
        visiting.remove(id);
        if selected_ids.insert(id.to_string()) {
            selected_id_order.push(id.to_string());
        }
    }

    for (text, id) in marked {
        if let Some(id) = id {
            visit(
                &id,
                &id_to_text,
                &id_deps,
                &mut selected_ids,
                &mut selected_id_order,
                &mut HashSet::new(),
                &mut missing_dependency_ids,
            );
        } else {
            selected_free_texts.insert(strip_priority_markers(&text));
        }
    }

    let mut prompts = Vec::new();
    for id in selected_id_order {
        if let Some(text) = id_to_text.get(&id) {
            prompts.push(text.clone());
        }
    }
    for (text, id, _, _) in &normalized {
        if id.is_none() && selected_free_texts.contains(&strip_priority_markers(text)) {
            prompts.push(text.clone());
        }
    }

    ActiveQueuePromptProjection {
        prompts,
        retargeted: true,
        missing_dependency_ids: missing_dependency_ids.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_identity_preserves_priority_marker() {
        assert_eq!(
            strip_in_progress_marker("🚧 :round_pushpin: do [#alpha]"),
            ":round_pushpin: do [#alpha]"
        );
        assert!(has_in_progress_marker(":round_pushpin: 🚧 do [#alpha]"));
        assert_eq!(
            strip_priority_markers(":round_pushpin: 🚧 do [#alpha]"),
            "do [#alpha]"
        );
    }

    #[test]
    fn every_claim_lifecycle_marker_round_trips_without_changing_identity() {
        let bare = "📌 do [#alpha]";
        for (state, prefix) in [
            (QueueLifecycleMarker::Claimed, CLAIMED_MARKER),
            (QueueLifecycleMarker::Running, RUNNING_MARKER),
            (QueueLifecycleMarker::Reported, REPORTED_MARKER),
            (QueueLifecycleMarker::Lost, LOST_MARKER),
            (QueueLifecycleMarker::Reconciling, RECONCILING_MARKER),
        ] {
            let projected = apply_lifecycle_marker(bare, state);
            assert_eq!(projected, format!("{prefix} {bare}"));
            assert_eq!(strip_in_progress_marker(&projected), bare);
            assert_eq!(strip_priority_markers(&projected), "do [#alpha]");
            assert_eq!(lifecycle_marker(&projected), Some(state));
            assert_eq!(apply_lifecycle_marker(&projected, state), projected);
        }
        assert_eq!(
            strip_in_progress_marker("🚧 do [#alpha]"),
            "do [#alpha]",
            "legacy running marker stays compatible"
        );
        for legacy in [LEGACY_REPORTED_MARKER, OLDER_REPORTED_MARKER] {
            assert_eq!(
                lifecycle_marker(&format!("{legacy} do [#alpha]")),
                Some(QueueLifecycleMarker::Reported),
                "legacy reported marker remains readable"
            );
            assert_eq!(
                apply_lifecycle_marker(
                    &format!("{legacy} do [#alpha]"),
                    QueueLifecycleMarker::Reported
                ),
                "☑️ do [#alpha]",
                "legacy reported input normalizes to the completed-handoff default"
            );
        }
    }

    #[test]
    fn operator_action_annotation_is_linked_and_orthogonal_to_worker_lifecycle() {
        let id = "subagent-operator-request-sdk-route".to_string();
        let annotated = apply_operator_action_ids("🚧 📌 fix sdk demo", std::slice::from_ref(&id));
        assert_eq!(
            annotated,
            "🙋🚧 📌 fix sdk demo [#subagent-operator-request-sdk-route]"
        );
        assert_eq!(operator_action_ids(&annotated), vec![id]);
        assert_eq!(strip_priority_markers(&annotated), "fix sdk demo");
        assert_eq!(
            apply_lifecycle_marker(&annotated, QueueLifecycleMarker::Reported),
            "🙋☑️ 📌 fix sdk demo [#subagent-operator-request-sdk-route]"
        );
        assert_eq!(
            apply_operator_action_ids(&annotated, &[]),
            "🚧 📌 fix sdk demo [#subagent-operator-request-sdk-route]"
        );
        assert!(!operator_action_is_answered(
            &annotated,
            "subagent-operator-request-sdk-route",
            "fix sdk demo"
        ));
        assert!(operator_action_is_answered(
            &format!("{annotated}: coordinator releases"),
            "subagent-operator-request-sdk-route",
            "fix sdk demo"
        ));
        assert!(operator_action_is_answered(
            "🙋 [#subagent-operator-request-sdk-route] 🚧 fix sdk demo: coordinator releases",
            "subagent-operator-request-sdk-route",
            "fix sdk demo"
        ));
        assert_eq!(
            apply_operator_action_ids("  /clear  ", &[]),
            "  /clear  ",
            "a no-op projection preserves literal prompt whitespace"
        );
    }

    /// `#answeredrequeststrike`: an answered request whose todo closed no
    /// longer holds its row; an open todo or a visible `🙋` still does.
    #[test]
    fn operator_action_holds_row_only_while_the_request_is_open() {
        let id = "subagent-operator-request-slack-app-bootstrap";
        let open: std::collections::HashSet<String> = [id.to_string()].into_iter().collect();
        let none = std::collections::HashSet::new();
        let answered = format!(
            "☑️ [#subagent:slack-pr-status#1] Setup the PR status app. [#{id}]: I ran this before in Codex."
        );
        assert_eq!(operator_action_ids(&answered), vec![id.to_string()]);
        assert!(!operator_action_marked(&answered));
        assert!(
            !operator_action_holds_row(&answered, &none),
            "answered, todo closed: the row is ordinary reported work"
        );
        assert!(
            operator_action_holds_row(&answered, &open),
            "answered but the todo is still open: the closeout checks it off first"
        );
        let marked = apply_operator_action_ids("☑️ fix sdk demo", &[id.to_string()]);
        assert!(operator_action_marked(&marked));
        assert!(
            operator_action_holds_row(&marked, &none),
            "a visible 🙋 holds the row until the projection clears it"
        );
        assert!(!operator_action_holds_row("☑️ fix sdk demo", &open));
    }

    /// `#queueoperatoranswer`: the request id as its own row with the answer nested
    /// beneath it means what `[#request-id]: <answer>` means.
    #[test]
    fn nested_operator_answer_row_is_a_local_answer() {
        let id = "subagent-operator-request-undo-agent-writes";
        let answer = format!("[#{id}]\n  - I want sync to use the latest state.");
        assert_eq!(
            nested_operator_answer(&answer),
            Some((id, "I want sync to use the latest state.".to_string()))
        );
        assert_eq!(operator_action_ids(&answer), vec![id.to_string()]);
        assert!(operator_action_is_answered(&answer, id, "do"));
        assert_eq!(
            operator_answer_row_request_id(&answer),
            Some(id.to_string())
        );
        assert!(operator_request_annotation_ids(&answer).is_empty());
        let none = HashSet::new();
        assert!(
            !operator_action_holds_row(&answer, &none),
            "a closed request holds nothing"
        );

        // Projection prefixes and a projected child never hide the answer.
        let projected = format!("🙋📥 [#{id}]\n  - 📥 I want sync.");
        assert_eq!(
            nested_operator_answer(&projected),
            Some((id, "I want sync.".to_string()))
        );
        // The `🙋` annotation lands on the parent part, never after the answer.
        assert_eq!(
            apply_operator_action_ids(&answer, &[id.to_string()]),
            format!("🙋 [#{id}]\n  - I want sync to use the latest state.")
        );
        assert_eq!(
            apply_operator_action_ids(&format!("🙋 [#{id}]\n  - yes"), &[]),
            format!("[#{id}]\n  - yes")
        );

        // Not answers: an empty block, a struck block, a tracked id, and a work
        // row that carries the request annotation.
        assert_eq!(nested_operator_answer(&format!("[#{id}]")), None);
        assert_eq!(
            nested_operator_answer(&format!("[#{id}]\n  - ~~done~~")),
            None
        );
        assert_eq!(nested_operator_answer("[#alpha]\n  - child"), None);
        let work = format!("fix the replay [#{id}]");
        assert_eq!(operator_answer_row_request_id(&work), None);
        assert_eq!(operator_request_annotation_ids(&work), vec![id.to_string()]);
        assert_eq!(
            operator_answer_row_request_id(&format!("[#{id}]: resync")),
            Some(id.to_string())
        );
    }

    #[test]
    fn local_operator_answer_keeps_request_identity_and_inline_examples_are_inert() {
        let id = "subagent-operator-request-publish-ci-pr166";
        let answer = format!("[#{id}]: **Commit and push to PR #166**");
        assert_eq!(operator_action_ids(&answer), vec![id.to_string()]);
        assert_eq!(strip_priority_markers(&answer), format!("[#{id}]"));
        assert!(operator_action_is_answered(&answer, id, "do"));
        assert_eq!(
            apply_operator_action_ids(&answer, &[id.to_string()]),
            format!("🙋 [#{id}]: **Commit and push to PR #166**")
        );
        assert_eq!(
            apply_lifecycle_marker(
                &apply_operator_action_ids(&answer, &[id.to_string()]),
                QueueLifecycleMarker::Running,
            ),
            format!("🙋🚧 [#{id}]: **Commit and push to PR #166**")
        );

        let quoted = format!(
            "In sdk-demo.md, no queue item status despite agent running: `- [#{id}]: **Commit and push to PR #166**`"
        );
        assert!(operator_action_ids(&quoted).is_empty());
        assert_eq!(strip_priority_markers(&quoted), quoted);
    }

    #[test]
    fn owner_generation_annotation_is_orthogonal_and_whitespace_preserving() {
        let owner = QueueOwnerProjection {
            owner: "subagent:sdk-demo".to_string(),
            generation: 7,
        };
        let projected = apply_owner_projection("  🙋 [#choice] 🚧 📌 fix sdk demo  ", Some(&owner));
        assert_eq!(
            projected,
            "  🙋🚧 [#subagent:sdk-demo#7] 📌 fix sdk demo [#choice]  "
        );
        assert_eq!(owner_projection(&projected), Some(owner));
        assert_eq!(strip_priority_markers(&projected), "fix sdk demo");
        assert_eq!(
            strip_in_progress_marker(&projected),
            "🙋 📌 fix sdk demo [#choice]",
            "terminalization strips attempt ownership with lifecycle state"
        );
        assert_eq!(
            apply_owner_projection(&projected, None),
            "  🙋🚧 📌 fix sdk demo [#choice]  "
        );
    }

    #[test]
    fn legacy_owner_generation_annotations_are_read_and_normalized_to_canonical_form() {
        let owner = QueueOwnerProjection {
            owner: "subagent:sdk-demo".to_string(),
            generation: 7,
        };
        for legacy in [
            "🚧 ⟦subagent:sdk-demo#7⟧ fix sdk demo",
            "🚧 ⟦owner=subagent:sdk-demo;generation=7⟧ fix sdk demo",
        ] {
            assert_eq!(owner_projection(legacy), Some(owner.clone()));
            assert_eq!(
                apply_owner_projection(legacy, Some(&owner)),
                "🚧 [#subagent:sdk-demo#7] fix sdk demo"
            );
            assert_eq!(strip_priority_markers(legacy), "fix sdk demo");
        }
    }

    #[test]
    fn torn_duplicate_owner_lifecycle_projection_repairs_only_with_owner_evidence() {
        let owner = QueueOwnerProjection {
            owner: "subagent:queue-state-fix".to_string(),
            generation: 3,
        };
        let prompt = "Several queue items have the wrong state";
        let torn =
            format!("🟡 [#subagent:queue-state-fix#3] [#subagent:queue-state-fix#3📥 {prompt}");

        assert_eq!(strip_in_progress_marker(&torn), prompt);
        assert_eq!(strip_priority_markers(&torn), prompt);
        assert_eq!(
            apply_owner_projection(&torn, Some(&owner)),
            format!("🟡 [#subagent:queue-state-fix#3] {prompt}")
        );

        let unproven = format!("[#subagent:queue-state-fix#3📥 {prompt}");
        assert_eq!(
            apply_owner_projection(&unproven, None),
            unproven,
            "a lookalike without durable or preceding owner evidence is operator text"
        );
        assert_eq!(
            strip_priority_markers(&unproven),
            unproven,
            "identity normalization must not repair a lookalike without owner evidence"
        );
    }

    #[test]
    fn worker_evidence_projection_obeys_bare_iff_no_evidence_truth_table() {
        let bound_states = [
            QueueLifecycleMarker::Claimed,
            QueueLifecycleMarker::Running,
            QueueLifecycleMarker::Reported,
            QueueLifecycleMarker::Lost,
        ];
        for bound in bound_states {
            for receipt in [false, true] {
                for feedback in [false, true] {
                    assert_eq!(
                        worker_evidence_marker(Some(bound), receipt, feedback),
                        Some(bound)
                    );
                }
            }
        }
        assert_eq!(
            worker_evidence_marker(None, true, false),
            Some(QueueLifecycleMarker::Reported)
        );
        assert_eq!(
            worker_evidence_marker(None, false, true),
            Some(QueueLifecycleMarker::Reconciling)
        );
        assert_eq!(worker_evidence_marker(None, false, false), None);
    }

    /// `#queueblocked`: `⛔ ⟦blocked by a, b⟧` is one lifecycle token. It is
    /// rendered with its ids, replaced (never stacked) on repaint, read back by
    /// `blocked_marker_ids`, and stripped from work identity by every parser.
    #[test]
    fn blocked_marker_names_its_blockers_and_is_cosmetic_for_identity() {
        let blockers = vec!["#SBX".to_string(), "deploy".to_string(), "sbx".to_string()];
        let blocked = apply_lifecycle_marker_with_blockers(
            "⏭\u{fe0f} :round_pushpin: do [#stg]",
            QueueLifecycleMarker::Blocked,
            &blockers,
        );
        assert_eq!(blocked, "⛔ ⟦blocked by sbx, deploy⟧ :round_pushpin: do [#stg]");
        assert_eq!(lifecycle_marker(&blocked), Some(QueueLifecycleMarker::Blocked));
        assert_eq!(blocked_marker_ids(&blocked), vec!["sbx", "deploy"]);
        assert_eq!(strip_priority_markers(&blocked), "do [#stg]");
        assert_eq!(strip_in_progress_marker(&blocked), ":round_pushpin: do [#stg]");
        // Repaint replaces the token, including a changed blocker list.
        assert_eq!(
            apply_lifecycle_marker_with_blockers(
                &blocked,
                QueueLifecycleMarker::Blocked,
                &["sbx".to_string()]
            ),
            "⛔ ⟦blocked by sbx⟧ :round_pushpin: do [#stg]"
        );
        assert_eq!(
            apply_lifecycle_marker(&blocked, QueueLifecycleMarker::Pending),
            "📥 :round_pushpin: do [#stg]",
            "unblocking drops the annotation with the marker"
        );
        // Owner/operator-action projections keep the token whole.
        assert_eq!(apply_owner_projection(&blocked, None), blocked);
        assert_eq!(
            apply_operator_action_ids(&blocked, &["subagent-operator-request-x".to_string()]),
            "🙋⛔ ⟦blocked by sbx, deploy⟧ :round_pushpin: do [#stg] [#subagent-operator-request-x]"
        );
        // A bare annotation left behind by a hand edit is still projection.
        assert_eq!(strip_priority_markers("⟦blocked by sbx⟧ do [#stg]"), "do [#stg]");
        // An operator's own prose is never read as the annotation.
        assert_eq!(
            strip_priority_markers("⟦blocked by sbx do [#stg]"),
            "⟦blocked by sbx do [#stg]"
        );
    }

    #[test]
    fn skip_marker_is_cosmetic_for_identity() {
        // `⏭️` behaves like `🚧`: stripped for identity, past pins, apply is
        // idempotent, and it never leaks into `strip_priority_markers` output.
        assert_eq!(apply_skip_marker("do [#sy71]"), "⏭\u{fe0f} do [#sy71]");
        assert_eq!(
            apply_skip_marker("🚧 do [#sy71]"),
            "⏭\u{fe0f} do [#sy71]",
            "apply replaces any existing lifecycle marker"
        );
        assert_eq!(
            apply_skip_marker(":round_pushpin: do [#sy71]"),
            "⏭\u{fe0f} :round_pushpin: do [#sy71]",
            "pins are preserved"
        );
        assert!(has_skip_marker("⏭\u{fe0f} do [#sy71]"));
        assert!(has_skip_marker(":pushpin: ⏭\u{fe0f} do [#sy71]"));
        assert!(!has_skip_marker("🚧 do [#sy71]"));
        assert!(!has_in_progress_marker("⏭\u{fe0f} do [#sy71]"));
        assert_eq!(
            strip_priority_markers("⏭\u{fe0f} :round_pushpin: do [#sy71]"),
            "do [#sy71]"
        );
        assert_eq!(
            strip_in_progress_marker("⏭\u{fe0f} do [#sy71]"),
            "do [#sy71]"
        );
    }

    #[test]
    fn awaiting_operator_marker_is_cosmetic_for_identity() {
        // `#queueawaitop`: `⏳` is a third lifecycle marker with the same
        // identity rules as `🚧`/`⏭️`.
        assert_eq!(
            apply_awaiting_operator_marker("do [#quirkbigint]"),
            "⏳ do [#quirkbigint]"
        );
        assert_eq!(
            apply_awaiting_operator_marker("⏭\u{fe0f} 📍 do [#quirkbigint]  "),
            "⏳ 📍 do [#quirkbigint]  ",
            "replaces the lifecycle marker, keeps pins and trailing whitespace"
        );
        assert!(has_awaiting_operator_marker("📌 ⏳ do [#a]"));
        assert!(!has_awaiting_operator_marker("🚧 do [#a]"));
        assert!(!has_in_progress_marker("⏳ do [#a]"));
        assert!(!has_skip_marker("⏳ do [#a]"));
        assert!(has_lifecycle_marker("⏳ do [#a]"));
        assert_eq!(strip_priority_markers("⏳ 📌 do [#a]"), "do [#a]");
        assert_eq!(strip_in_progress_marker("⏳ 📌 do [#a]"), "📌 do [#a]");
        assert_eq!(
            strip_in_progress_marker_for_display("⏳ do [#a]\n").as_deref(),
            Some("do [#a]\n")
        );
    }

    #[test]
    fn set_in_progress_work_item_markers_updates_tracked_components() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] Alpha\n",
            "- [ ] [#beta] Beta\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:review -->\n",
            "- [ ] [#alpha] Alpha review\n",
            "<!-- /agent:review -->\n",
        );
        let active_ids = HashSet::from(["alpha".to_string()]);

        let (updated, changed) = set_in_progress_work_item_markers(content, &active_ids).unwrap();

        assert!(changed);
        assert!(updated.contains("- [ ] 🚧 [#alpha] Alpha"));
        assert!(updated.contains("- [ ] [#beta] Beta"));
        assert!(updated.contains("- [ ] 🚧 [#alpha] Alpha review"));
    }

    #[test]
    fn set_work_item_lifecycle_markers_mirrors_typed_queue_state() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] 🚧 [#reported] Reported work\n",
            "- [ ] 🚧 [#running] Running work\n",
            "- [ ] 🚧 [#stale] Stale work\n",
            "<!-- /agent:backlog -->\n",
        );
        let lifecycle_by_id = HashMap::from([
            ("reported".to_string(), QueueLifecycleMarker::Reported),
            ("running".to_string(), QueueLifecycleMarker::Running),
        ]);

        let (updated, changed) =
            set_work_item_lifecycle_markers(content, &lifecycle_by_id).unwrap();

        assert!(changed);
        assert!(updated.contains("- [ ] ☑️ [#reported] Reported work"));
        assert!(updated.contains("- [ ] 🚧 [#running] Running work"));
        assert!(updated.contains("- [ ] [#stale] Stale work"));
    }

    #[test]
    fn sync_in_progress_marker_regions_copies_work_regions_from_current() {
        let snapshot = concat!(
            "<!-- agent:queue -->\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] Alpha\n",
            "<!-- /agent:backlog -->\n",
        );
        let current = concat!(
            "<!-- agent:queue -->\n",
            "- 🚧 do [#alpha]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] 🚧 [#alpha] Alpha\n",
            "<!-- /agent:backlog -->\n",
        );

        let synced = sync_in_progress_marker_regions(snapshot, current);

        assert!(synced.contains("- 🚧 do [#alpha]"));
        assert!(synced.contains("- [ ] 🚧 [#alpha] Alpha"));
        assert!(!synced.contains("#old"));
    }

    #[test]
    fn default_projection_uses_first_projectable_row() {
        let rows = vec![
            QueuePromptRow::new("do [#verify]", Some("verify".to_string()), false),
            QueuePromptRow::new("do [#ready]", Some("ready".to_string()), true),
        ];

        let projection =
            project_active_queue_prompts(&rows, &HashMap::new(), false, &HashSet::new());

        assert_eq!(projection.prompts, vec!["do [#ready]"]);
        assert!(!projection.retargeted);
    }

    #[test]
    fn skip_advances_to_next_nondependent_head() {
        // #queueskip: a skipped head is passed over; selection advances to the next
        // projectable head that does not depend on it.
        let rows = vec![
            QueuePromptRow::new("do [#sy71]", Some("sy71".to_string()), true),
            QueuePromptRow::new("do [#hmw9]", Some("hmw9".to_string()), true),
        ];
        let skipped: HashSet<String> = ["sy71".to_string()].into_iter().collect();
        let projection = project_active_queue_prompts(&rows, &HashMap::new(), false, &skipped);
        assert_eq!(projection.prompts, vec!["do [#hmw9]"]);
    }

    #[test]
    fn skip_holds_follower_that_depends_on_skipped_head() {
        // #queueskip: a follower whose `after=` closure includes the skipped head is
        // NOT "non-dependent", so it must not run ahead — selection stalls (empty).
        let rows = vec![
            QueuePromptRow::new("do [#sy71]", Some("sy71".to_string()), true),
            QueuePromptRow::new("do [#dep]", Some("dep".to_string()), true),
        ];
        let deps = HashMap::from([("dep".to_string(), vec!["sy71".to_string()])]);
        let skipped: HashSet<String> = ["sy71".to_string()].into_iter().collect();
        let projection = project_active_queue_prompts(&rows, &deps, false, &skipped);
        assert!(
            projection.prompts.is_empty(),
            "dependent follower must not run ahead of a skipped blocker: {:?}",
            projection.prompts
        );
    }

    #[test]
    fn skip_empty_set_is_unchanged() {
        let rows = vec![QueuePromptRow::new("do [#a]", Some("a".to_string()), true)];
        let projection =
            project_active_queue_prompts(&rows, &HashMap::new(), false, &HashSet::new());
        assert_eq!(projection.prompts, vec!["do [#a]"]);
    }

    #[test]
    fn retarget_projection_expands_auto_dag_dependencies() {
        let rows = vec![
            QueuePromptRow::new("do [#ops]", Some("ops".to_string()), true),
            QueuePromptRow::new("🚧 do [#ship]", Some("ship".to_string()), true),
            QueuePromptRow::new(
                ":round_pushpin: do [#setup]",
                Some("setup".to_string()),
                true,
            ),
        ];
        let deps = HashMap::from([("ship".to_string(), vec!["setup".to_string()])]);
        let projection = project_active_queue_prompts(&rows, &deps, true, &HashSet::new());

        assert_eq!(
            projection.prompts,
            vec![":round_pushpin: do [#setup]", "do [#ship]"]
        );
        assert!(projection.retargeted);
    }

    #[test]
    fn retarget_requires_marker_diff_signal() {
        let rows = vec![QueuePromptRow::new(
            "🚧 do [#ship]",
            Some("ship".to_string()),
            true,
        )];
        let none = HashSet::new();
        assert!(!in_progress_marker_retarget_requested(None, &rows, &none));
        assert!(!in_progress_marker_retarget_requested(
            Some("+- do [#ship]"),
            &rows,
            &none
        ));
        assert!(in_progress_marker_retarget_requested(
            Some("+- 🚧 do [#ship]"),
            &rows,
            &none
        ));
    }

    /// `#stalemarkerretarget`: the binary projected `🚧` onto `release + publish`
    /// while it was the only live head; the operator then queued two `do` heads
    /// above it; a HEAD-rebuilt baseline lost the marker, so the diff showed it as
    /// added. That stale projection must not read as an operator retarget.
    #[test]
    fn binary_projected_marker_is_not_an_operator_retarget() {
        let rows = vec![
            QueuePromptRow::new("do [#a]", Some("a".to_string()), true),
            QueuePromptRow::new("do [#b]", Some("b".to_string()), true),
            QueuePromptRow::new("🚧 release + publish", None, true),
        ];
        let diff = Some("-- release + publish\n+- 🚧 release + publish");
        let projected: HashSet<String> = ["release + publish".to_string()].into_iter().collect();

        assert!(!in_progress_marker_retarget_requested(
            diff, &rows, &projected
        ));
        let projection = project_active_queue_prompts(
            &rows,
            &HashMap::new(),
            in_progress_marker_retarget_requested(diff, &rows, &projected),
            &HashSet::new(),
        );
        assert_eq!(projection.prompts, vec!["do [#a]"]);
        assert!(!projection.retargeted);

        // The operator moving the marker onto a different head is still honored.
        let moved = vec![
            QueuePromptRow::new("do [#a]", Some("a".to_string()), true),
            QueuePromptRow::new("🚧 do [#b]", Some("b".to_string()), true),
            QueuePromptRow::new("release + publish", None, true),
        ];
        let moved_diff = Some("-- do [#b]\n+- 🚧 do [#b]");
        assert!(in_progress_marker_retarget_requested(
            moved_diff, &moved, &projected
        ));
    }
}
