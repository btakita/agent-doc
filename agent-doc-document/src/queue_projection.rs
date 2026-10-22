//! Pure queue projection helpers.
//!
//! Queue lifecycle markers are visible projections of active queue heads. They
//! are not queue identity and do not commit or admit a turn by themselves.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};

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
/// Claimed for dispatch, but no positive worker-liveness evidence exists yet.
/// `⏳` already means "awaiting operator" in existing documents, so claims use
/// a yellow state marker instead (`#queueclaimstate`).
pub const CLAIMED_MARKER: &str = "🟡";
/// The claimed worker has positive dispatch/liveness evidence.
pub const RUNNING_MARKER: &str = IN_PROGRESS_MARKER;
/// The worker reported and the result is awaiting coordinator integration.
pub const REPORTED_MARKER: &str = "📤";
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

/// Every leading lifecycle marker the binary projects onto a queue head.
pub const LIFECYCLE_MARKERS: [&str; 9] = [
    PENDING_MARKER,
    CLAIMED_MARKER,
    RUNNING_MARKER,
    REPORTED_MARKER,
    LOST_MARKER,
    RECONCILING_MARKER,
    UNKNOWN_MARKER,
    SKIP_MARKER,
    AWAITING_OPERATOR_MARKER,
];

/// Binary-owned visible lifecycle states. These prefixes are projection only:
/// every parser strips them before queue-item identity is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueueLifecycleMarker {
    Pending,
    Claimed,
    Running,
    Reported,
    Lost,
    Reconciling,
    Unknown,
    Skipped,
    AwaitingOperator,
}

impl QueueLifecycleMarker {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Pending => PENDING_MARKER,
            Self::Claimed => CLAIMED_MARKER,
            Self::Running => RUNNING_MARKER,
            Self::Reported => REPORTED_MARKER,
            Self::Lost => LOST_MARKER,
            Self::Reconciling => RECONCILING_MARKER,
            Self::Unknown => UNKNOWN_MARKER,
            Self::Skipped => SKIP_MARKER,
            Self::AwaitingOperator => AWAITING_OPERATOR_MARKER,
        }
    }
}

/// Formal worker-state projection (`#queueworkevidence`). For a live queue row
/// `q`, let `A(q)` be a bound claim attempt, `R(q)` a durable reported-release
/// receipt, and `F(q)` a binary-attested completion cell. Then:
///
/// `Bare(q) <=> !A(q) && !R(q) && !F(q)`.
///
/// A bound attempt has strongest authority; a report receipt means `Reported`;
/// otherwise completion evidence is generation-ambiguous and means
/// `Reconciling`. Closure removes the row, so this function applies only while
/// `q` is live. A cosmetic marker alone is not authority.
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
    LIFECYCLE_MARKERS
        .iter()
        .find_map(|marker| text.strip_prefix(marker))
}

fn strip_operator_action_prefix(text: &str) -> Option<(&str, &str)> {
    let after_marker = text.strip_prefix(OPERATOR_ACTION_MARKER)?.trim_start();
    let after_open = after_marker.strip_prefix("[#")?;
    let (id, rest) = after_open.split_once(']')?;
    let mut chars = id.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-') {
        return None;
    }
    Some((id, rest))
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
        if let Some(after) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| rest.strip_prefix(marker))
        {
            rest = after.trim_start();
            continue;
        }
        return None;
    }
}

/// Replace the exact-attempt owner annotation while preserving lifecycle,
/// operator-action, priority, leading, and trailing whitespace. The annotation
/// is placed immediately after a lifecycle marker when one exists.
pub fn apply_owner_projection(
    text: &str,
    owner: Option<&QueueOwnerProjection>,
) -> String {
    let content_start = text.len() - text.trim_start().len();
    let leading = &text[..content_start];
    let (content, trailing) = split_trailing_whitespace(&text[content_start..]);
    let mut rest = content.trim_start();
    let mut prefixes = Vec::<String>::new();
    loop {
        if let Some((_, after)) = strip_owner_annotation_prefix(rest) {
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = LIFECYCLE_MARKERS
            .iter()
            .find_map(|marker| rest.strip_prefix(marker).map(|after| (*marker, after)))
        {
            prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        if let Some((id, after)) = strip_operator_action_prefix(rest) {
            prefixes.push(format!("{OPERATOR_ACTION_MARKER} [#{id}]"));
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| rest.strip_prefix(marker).map(|after| (*marker, after)))
        {
            prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        break;
    }
    if let Some(index) = prefixes
        .iter()
        .position(|prefix| LIFECYCLE_MARKERS.contains(&prefix.as_str()))
    {
        let lifecycle = prefixes.remove(index);
        prefixes.insert(0, lifecycle);
    }
    if let Some(owner) = owner {
        let annotation = format_owner_annotation(owner);
        let insert_at = prefixes
            .first()
            .is_some_and(|prefix| LIFECYCLE_MARKERS.contains(&prefix.as_str())) as usize;
        prefixes.insert(insert_at, annotation);
    }
    if !rest.is_empty() {
        prefixes.push(rest.to_string());
    }
    format!("{leading}{}{trailing}", prefixes.join(" "))
}

/// Request ids carried by leading `🙋 [#id]` queue annotations.
pub fn operator_action_ids(text: &str) -> Vec<String> {
    let mut rest = text.trim_start();
    let mut ids = Vec::new();
    loop {
        if let Some((id, after)) = strip_operator_action_prefix(rest) {
            let id = id.to_ascii_lowercase();
            if !ids.contains(&id) {
                ids.push(id);
            }
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
        if let Some(after) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| rest.strip_prefix(marker))
        {
            rest = after.trim_start();
            continue;
        }
        break;
    }
    ids
}

fn strip_operator_action_annotations_preserving_markers(text: &str) -> String {
    let mut rest = text.trim();
    let mut kept_prefixes: Vec<String> = Vec::new();
    loop {
        let trimmed = rest.trim_start();
        if let Some((_, after)) = strip_operator_action_prefix(trimmed) {
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = LIFECYCLE_MARKERS
            .iter()
            .find_map(|marker| trimmed.strip_prefix(marker).map(|after| (*marker, after)))
        {
            kept_prefixes.push(marker.to_string());
            rest = after.trim_start();
            continue;
        }
        if let Some((owner, after)) = strip_owner_annotation_prefix(trimmed) {
            kept_prefixes.push(format_owner_annotation(&owner));
            rest = after.trim_start();
            continue;
        }
        if let Some((marker, after)) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| trimmed.strip_prefix(marker).map(|after| (*marker, after)))
        {
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
    kept_prefixes.join(" ")
}

/// Replace the leading operator-action annotations while preserving worker
/// lifecycle and priority projections. `ids` are normalized and deduplicated.
pub fn apply_operator_action_ids(text: &str, ids: &[String]) -> String {
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
    let annotations = normalized
        .iter()
        .map(|id| format!("{OPERATOR_ACTION_MARKER} [#{id}]"))
        .collect::<Vec<_>>()
        .join(" ");
    if bare.is_empty() {
        format!("{leading}{annotations}{trailing}")
    } else {
        format!("{leading}{annotations} {bare}{trailing}")
    }
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
    loop {
        let trimmed = rest.trim_start();
        if let Some((id, after_marker)) = strip_operator_action_prefix(trimmed) {
            kept_prefixes.push(format!("{OPERATOR_ACTION_MARKER} [#{id}]"));
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((_, after_marker)) = strip_owner_annotation_prefix(trimmed) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = strip_lifecycle_prefix(trimmed) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((marker, after_marker)) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|marker| trimmed.strip_prefix(marker).map(|after| (*marker, after)))
        {
            kept_prefixes.push(marker.to_string());
            rest = after_marker.trim_start();
            continue;
        }
        rest = trimmed;
        break;
    }
    if kept_prefixes.is_empty() {
        rest.trim().to_string()
    } else {
        let mut out = kept_prefixes.join(" ");
        if !rest.trim().is_empty() {
            out.push(' ');
            out.push_str(rest.trim());
        }
        out
    }
}

pub fn apply_in_progress_marker(text: &str) -> String {
    apply_lifecycle_marker(text, QueueLifecycleMarker::Running)
}

/// Replace any existing lifecycle prefix with `marker`, preserving pins and
/// trailing whitespace.
pub fn apply_lifecycle_marker(text: &str, marker: QueueLifecycleMarker) -> String {
    let (content, trailing) = split_trailing_whitespace(text);
    format!(
        "{} {}{trailing}",
        marker.prefix(),
        strip_in_progress_marker(content)
    )
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
    has_leading_lifecycle_marker(text, IN_PROGRESS_MARKER)
}

pub fn lifecycle_marker(text: &str) -> Option<QueueLifecycleMarker> {
    [
        QueueLifecycleMarker::Pending,
        QueueLifecycleMarker::Claimed,
        QueueLifecycleMarker::Running,
        QueueLifecycleMarker::Reported,
        QueueLifecycleMarker::Lost,
        QueueLifecycleMarker::Reconciling,
        QueueLifecycleMarker::Unknown,
        QueueLifecycleMarker::Skipped,
        QueueLifecycleMarker::AwaitingOperator,
    ]
    .into_iter()
    .find(|marker| has_leading_lifecycle_marker(text, marker.prefix()))
}

/// True when the head carries a leading skip marker (`⏭️`), past any pins.
pub fn has_skip_marker(text: &str) -> bool {
    has_leading_lifecycle_marker(text, SKIP_MARKER)
}

/// True when the head carries a leading awaiting-operator marker (`⏳`).
pub fn has_awaiting_operator_marker(text: &str) -> bool {
    has_leading_lifecycle_marker(text, AWAITING_OPERATOR_MARKER)
}

/// True when the head carries any leading lifecycle marker (`🚧`/`⏭️`/`⏳`).
pub fn has_lifecycle_marker(text: &str) -> bool {
    LIFECYCLE_MARKERS
        .iter()
        .any(|marker| has_leading_lifecycle_marker(text, marker))
}

fn has_leading_lifecycle_marker(text: &str, marker: &str) -> bool {
    let mut rest = text.trim_start();
    loop {
        if rest.starts_with(marker) {
            return true;
        }
        if let Some((_, after_marker)) = strip_operator_action_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some((_, after_marker)) = strip_owner_annotation_prefix(rest) {
            rest = after_marker.trim_start();
            continue;
        }
        if let Some(after_marker) = PRIORITIZED_MARKERS
            .iter()
            .chain(AGENT_PRIORITIZED_MARKERS.iter())
            .find_map(|m| rest.strip_prefix(m))
        {
            rest = after_marker.trim_start();
            continue;
        }
        return false;
    }
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
        let (new_body, body_changed) = agent_doc_element_backlog::backlog::set_in_progress_item_ids(
            component.content(&updated),
            active_ids,
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
    loop {
        let trimmed = t.trim_start();
        let stripped = strip_lifecycle_prefix(trimmed)
            .or_else(|| strip_operator_action_prefix(trimmed).map(|(_, rest)| rest))
            .or_else(|| strip_owner_annotation_prefix(trimmed).map(|(_, rest)| rest))
            .or_else(|| {
                PRIORITIZED_MARKERS
                    .iter()
                    .chain(AGENT_PRIORITIZED_MARKERS.iter())
                    .find_map(|m| trimmed.strip_prefix(m))
            });
        match stripped {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    t.trim().to_string()
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
            && line.contains(IN_PROGRESS_MARKER)
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
    }

    #[test]
    fn operator_action_annotation_is_linked_and_orthogonal_to_worker_lifecycle() {
        let id = "subagent-operator-request-sdk-route".to_string();
        let annotated = apply_operator_action_ids("🚧 📌 fix sdk demo", std::slice::from_ref(&id));
        assert_eq!(
            annotated,
            "🙋 [#subagent-operator-request-sdk-route] 🚧 📌 fix sdk demo"
        );
        assert_eq!(operator_action_ids(&annotated), vec![id]);
        assert_eq!(strip_priority_markers(&annotated), "fix sdk demo");
        assert_eq!(
            apply_lifecycle_marker(&annotated, QueueLifecycleMarker::Reported),
            "📤 🙋 [#subagent-operator-request-sdk-route] 📌 fix sdk demo"
        );
        assert_eq!(
            apply_operator_action_ids(&annotated, &[]),
            "🚧 📌 fix sdk demo"
        );
        assert_eq!(
            apply_operator_action_ids("  /clear  ", &[]),
            "  /clear  ",
            "a no-op projection preserves literal prompt whitespace"
        );
    }

    #[test]
    fn owner_generation_annotation_is_orthogonal_and_whitespace_preserving() {
        let owner = QueueOwnerProjection {
            owner: "subagent:sdk-demo".to_string(),
            generation: 7,
        };
        let projected = apply_owner_projection(
            "  🙋 [#choice] 🚧 📌 fix sdk demo  ",
            Some(&owner),
        );
        assert_eq!(
            projected,
            "  🚧 [#subagent:sdk-demo#7] 🙋 [#choice] 📌 fix sdk demo  "
        );
        assert_eq!(owner_projection(&projected), Some(owner));
        assert_eq!(strip_priority_markers(&projected), "fix sdk demo");
        assert_eq!(
            strip_in_progress_marker(&projected),
            "🙋 [#choice] 📌 fix sdk demo",
            "terminalization strips attempt ownership with lifecycle state"
        );
        assert_eq!(
            apply_owner_projection(&projected, None),
            "  🚧 🙋 [#choice] 📌 fix sdk demo  "
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
