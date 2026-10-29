//! Queue element descriptor and local realtime model.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole, ElementWritePolicy,
};
use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod nested_claim_steer;
pub mod queue_identity;
pub mod subprompts;

pub const DESCRIPTOR: ElementDescriptor = ElementDescriptor {
    name: "run",
    aliases: &["queue", "loop"],
    source: ElementSource::BuiltIn,
    shape: ElementShape::Component,
    authority: ElementAuthority::DerivedProjection,
    write_policy: ElementWritePolicy::ProjectionOnly,
    scheduling_role: ElementSchedulingRole::ActiveQueue,
    turn_role: ElementTurnRole::Trigger,
    realtime_model: ElementRealtimeModel::Queue,
    composition_role: ElementCompositionRole::Consumer,
    realtime: true,
};

pub fn descriptor() -> ElementDescriptor {
    DESCRIPTOR
}

/// Visible lifecycle state of a single rendered queue item, in ascending lawful
/// order. This is the coarse lattice the CRDT convergence joins over; richer
/// orchestration queue state machines refine this state but must map back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemLifecycle {
    /// The item is a live, un-struck queue head and is still actionable.
    Live,
    /// The item is struck through (`~~...~~`) and visibly retired. A stale live
    /// re-emit must never regress past this.
    Struck,
}

impl QueueItemLifecycle {
    /// Position in the total lifecycle order (`Live` = 0, `Struck` = 1).
    pub fn rank(self) -> u8 {
        match self {
            QueueItemLifecycle::Live => 0,
            QueueItemLifecycle::Struck => 1,
        }
    }

    /// The join (`max`) of two lifecycle states. Monotonic: a `Live` re-emit can
    /// never regress a `Struck` item.
    pub fn join(self, other: QueueItemLifecycle) -> QueueItemLifecycle {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }

    /// Classify a rendered queue item's first line. A markdown strike (`~~`)
    /// anywhere on the identity line marks the item answered-and-retired.
    pub fn classify(item: &str) -> QueueItemLifecycle {
        let first = item.lines().next().unwrap_or("");
        if first.contains("~~") {
            QueueItemLifecycle::Struck
        } else {
            QueueItemLifecycle::Live
        }
    }
}

/// Durable identity of a queue head.
///
/// Two heads share an identity iff they refer to the same work, regardless of
/// cosmetic pin / prefix variation (`:pushpin: do [#abcd]`, `do [#abcd]`, and
/// `[#abcd]` are one identity). Free-text heads key on whitespace-collapsed,
/// lowercased text so a verbatim re-emit collapses.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "key")]
pub enum QueueItemIdentity {
    Id(String),
    FreeText(String),
}

impl QueueItemIdentity {
    /// Resolve the durable identity of a queue head prompt. Id-backed heads key
    /// on the extracted id; everything else keys on normalized free text.
    pub fn from_prompt(prompt: &str) -> Self {
        match extract_head_id(prompt) {
            Some(id) => QueueItemIdentity::Id(id),
            // Strip lifecycle/pin markers before keying free text so a
            // marker-decorated re-emit (`🚧 <text>`, `📌 <text>`) keys to the
            // same identity as the operator's bare `<text>` line. Without this
            // the convergence pass treats them as distinct free-text heads and
            // cannot collapse a resurrected copy against the bare original
            // (`#qdedup-directive-twin` free-text variant).
            None => QueueItemIdentity::FreeText(normalize_multiline_dedup_text(
                &strip_priority_markers(prompt),
            )),
        }
    }

    /// `true` for an id-backed identity.
    pub fn is_id_backed(&self) -> bool {
        matches!(self, QueueItemIdentity::Id(_))
    }
}

/// Lifecycle state of a single queue item, in ascending lawful order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueItemState {
    Authored,
    Mirrored,
    InProgress,
    Answered,
    Struck,
    Reaped,
}

impl QueueItemState {
    pub fn rank(self) -> u8 {
        match self {
            QueueItemState::Authored => 0,
            QueueItemState::Mirrored => 1,
            QueueItemState::InProgress => 2,
            QueueItemState::Answered => 3,
            QueueItemState::Struck => 4,
            QueueItemState::Reaped => 5,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, QueueItemState::Reaped)
    }

    /// Project this fine-grained lifecycle state onto the coarse rendered queue
    /// lattice the CRDT convergence joins over.
    pub fn lifecycle(self) -> QueueItemLifecycle {
        match self {
            QueueItemState::Authored
            | QueueItemState::Mirrored
            | QueueItemState::InProgress
            | QueueItemState::Answered => QueueItemLifecycle::Live,
            QueueItemState::Struck | QueueItemState::Reaped => QueueItemLifecycle::Struck,
        }
    }
}

/// Events driving a queue item's lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueItemEvent {
    OperatorAuthored,
    BacklogMirrored,
    DrainStarted,
    ExchangeAnswered,
    StruckThrough,
    Reaped,
}

impl QueueItemEvent {
    /// The lifecycle state this event drives the item toward.
    pub fn target(self) -> QueueItemState {
        match self {
            QueueItemEvent::OperatorAuthored => QueueItemState::Authored,
            QueueItemEvent::BacklogMirrored => QueueItemState::Mirrored,
            QueueItemEvent::DrainStarted => QueueItemState::InProgress,
            QueueItemEvent::ExchangeAnswered => QueueItemState::Answered,
            QueueItemEvent::StruckThrough => QueueItemState::Struck,
            QueueItemEvent::Reaped => QueueItemState::Reaped,
        }
    }
}

/// Pure transition: the join (`max`) of the current state and the state the
/// event drives toward.
pub fn transition_queue_item(
    current: &QueueItemState,
    event: &QueueItemEvent,
) -> Option<QueueItemState> {
    let target = event.target();
    Some(if target.rank() > current.rank() {
        target
    } else {
        *current
    })
}

/// Thread-safe lifecycle machine for one queue identity.
pub struct QueueItemMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<QueueItemState, QueueItemEvent>,
}

impl QueueItemMachine {
    /// Join this machine to `scope`'s graph (`#stategraphjoin`).
    ///
    /// A queue identity outlives any single turn — it is authored in one and consumed in a later one — so it belongs to the document.
    ///
    /// The scope owns the context, so dropping the scope drops this machine's cells —
    /// teardown is the scope's lifetime, not a separate deregistration step.
    pub fn new_in(scope: &agent_doc_state_scope::DocumentScope, initial: QueueItemState) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_item);
        Self { ctx, machine }
    }

    /// Standalone machine in a private context — a pure-transition helper for unit
    /// tests only.
    ///
    /// A long-lived owner must use [`Self::new_in`] instead: nothing outside a private
    /// context can derive from its cells and invalidation never crosses it, so a
    /// `Computed` built over one is Computed in name only.
    pub fn new(initial: QueueItemState) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_queue_item);
        Self { ctx, machine }
    }

    pub fn send(&self, event: QueueItemEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> QueueItemState {
        self.machine.state(&self.ctx)
    }

    pub fn transition(initial: QueueItemState, event: QueueItemEvent) -> Option<QueueItemState> {
        let machine = Self::new(initial);
        if machine.send(event) {
            Some(machine.state())
        } else {
            None
        }
    }
}

pub fn normalize_multiline_dedup_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn extract_head_id(prompt: &str) -> Option<String> {
    let stripped = strip_priority_markers(prompt);
    let trimmed = stripped.trim();
    let lower = trimmed.to_ascii_lowercase();
    let explicit_do = lower.starts_with("do ");
    let topic = lower.strip_prefix("do ").unwrap_or(&lower).trim();
    let raw = topic
        .strip_prefix("[#")
        .and_then(|rest| {
            let (id, tail) = rest.split_once(']')?;
            // Under the optional-`do` grammar, a colon immediately following a
            // bare `[#id]` makes the line prose (`[#id]: note`). An explicit
            // `do [#id]: note` remains an id-backed annotated directive.
            (!tail.starts_with(':') || explicit_do).then_some(id)
        })
        .or_else(|| {
            let rest = topic.strip_prefix('#')?;
            let id = id_prefix(rest);
            // `#presetargdedup`: a bare, un-`do`'d `#word <more text>` line is a
            // prompt-preset invocation with arguments (`#gh-fix <issue url>`),
            // not a reference to tracked item `#word`. Keying it by `word`
            // gave every line sharing the preset ONE identity, so convergence
            // collapsed the siblings -- and once the first was struck, dropped
            // the rest as stale re-emits of a finished item. Only a bare `#id`
            // (or `do #id ...` / `[#id]`) is id-backed; the rest keys as text.
            if lower.starts_with("do ") || rest[id.len()..].trim().is_empty() {
                Some(id)
            } else {
                None
            }
        })?;
    if raw.is_empty()
        || !raw
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        None
    } else {
        Some(raw.to_ascii_lowercase())
    }
}

fn id_prefix(text: &str) -> &str {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
        .next()
        .unwrap_or("")
}

fn trailing_operator_action_annotation(text: &str) -> Option<(&str, &str, usize)> {
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
        if id.is_empty()
            || !id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            || !(after.is_empty() || after.trim_start().starts_with(':'))
        {
            continue;
        }
        return Some((id, after, start));
    }
    None
}

/// Strip the bare operator-action glyph (`🙋`, or its configured spelling)
/// from the front of `trimmed`, leaving whatever follows it untouched.
fn strip_operator_action_glyph(trimmed: &str) -> Option<&str> {
    let icons = agent_doc_queue_icons::queue_icons();
    [icons.operator_action.as_str(), "🙋"]
        .into_iter()
        .filter(|marker| !marker.is_empty())
        .filter_map(|marker| {
            trimmed
                .strip_prefix(marker)
                .map(|rest| (marker.len(), rest))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, rest)| rest)
}

/// Strip ONE leading binary-owned projection token from `trimmed`: a
/// lifecycle/priority marker (configured or default spelling) or an
/// exact-attempt owner annotation (`[#<owner>#<generation>]`, or the legacy
/// `⟦<owner>#<generation>⟧` / `⟦owner=<owner>;generation=<generation>⟧`
/// spellings). This is the single grammar both [`strip_priority_markers`] and
/// [`strip_attempt_projection_prefix`] consume.
fn strip_projection_token(trimmed: &str) -> Option<&str> {
    let icons = agent_doc_queue_icons::queue_icons();
    let configured = [
        icons.prioritized.as_str(),
        icons.agent_prioritized.as_str(),
        icons.claimed.as_str(),
        icons.running.as_str(),
        icons.pending.as_str(),
        icons.blocked.as_str(),
        icons.reported.as_str(),
        icons.lost.as_str(),
        icons.reconciling.as_str(),
        icons.unknown.as_str(),
        icons.skipped.as_str(),
        icons.awaiting_operator.as_str(),
    ];
    let defaults = [
        "**prioritized**",
        "__prioritized__",
        "**pin**",
        "__pin__",
        ":pushpin:",
        ":pin:",
        "📌",
        "*prioritized*",
        "_prioritized_",
        "*pin*",
        "_pin_",
        ":round_pushpin:",
        "📍",
        // Fenced subagent-attempt lifecycle projections.
        "🟡",
        "🚧",
        "📥",
        // `#queueblocked`: blocked lifecycle projection.
        "⛔",
        "☑️",
        // Former reported/outbox spellings; accepted for compatibility.
        "📫",
        "📤",
        "⚠️",
        "🔄",
        "❔",
        "⏭\u{fe0f}",
        // `#queueawaitop`: awaiting-operator lifecycle projection.
        "⏳",
    ];
    configured
        .iter()
        .chain(defaults.iter())
        // An empty configured spelling would match every line and never
        // advance the caller's strip loop.
        .filter(|marker| !marker.is_empty())
        .filter_map(|marker| {
            trimmed
                .strip_prefix(marker)
                .map(|rest| (marker.len(), rest))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, rest)| rest)
        // `#queueblocked`: the blocker annotation that follows `⛔`
        // (`⛔ ⟦blocked by a, b⟧`) is projection, never work identity.
        .or_else(|| strip_blocked_annotation_token(trimmed))
        .or_else(|| strip_owner_annotation_token(trimmed))
}

/// A leading `⟦blocked by ...⟧` annotation, the blocker list that follows `⛔`.
fn strip_blocked_annotation_token(trimmed: &str) -> Option<&str> {
    let after_open = trimmed.strip_prefix("⟦blocked by ")?;
    let close = after_open.find('⟧')?;
    (!after_open[..close].contains(['\n', '\r'])).then(|| &after_open[close + '⟧'.len_utf8()..])
}

/// A leading exact-attempt owner annotation (`[#<owner>#<generation>]` or the
/// legacy `⟦…⟧` spellings). A bare `[#id]` is work identity, not an attempt.
fn strip_owner_annotation_token(trimmed: &str) -> Option<&str> {
    let (payload, after, legacy_delimiters) = if let Some(rest) = trimmed.strip_prefix("[#") {
        let (payload, after) = rest.split_once(']')?;
        (payload, after, false)
    } else {
        let rest = trimmed.strip_prefix('⟦')?;
        let (payload, after) = rest.split_once('⟧')?;
        (payload, after, true)
    };
    let (owner, generation) = if legacy_delimiters {
        if let Some(legacy) = payload.strip_prefix("owner=") {
            legacy.rsplit_once(";generation=")?
        } else {
            payload.rsplit_once('#')?
        }
    } else {
        payload.rsplit_once('#')?
    };
    (!owner.is_empty()
        && !owner.chars().any(|ch| ch == '\n' || ch == '\r')
        && generation.parse::<u64>().is_ok())
    .then_some(after)
}

fn longest_marker_prefix<'a>(trimmed: &'a str, markers: &[&str]) -> Option<(&'a str, &'a str)> {
    markers
        .iter()
        .filter(|marker| !marker.is_empty())
        .filter_map(|marker| {
            trimmed
                .strip_prefix(marker)
                .map(|rest| (&trimmed[..marker.len()], rest))
        })
        .max_by_key(|(marker, _)| marker.len())
}

/// Operator pin spellings. A pin on a sub-prompt child is the operator's, not
/// a lifecycle projection (`#nestedsubprompts`).
fn leading_pin(trimmed: &str) -> Option<(&str, &str)> {
    let icons = agent_doc_queue_icons::queue_icons();
    let configured = [icons.prioritized.as_str(), icons.agent_prioritized.as_str()];
    let defaults = [
        "**prioritized**",
        "__prioritized__",
        "**pin**",
        "__pin__",
        ":pushpin:",
        ":pin:",
        "📌",
        "*prioritized*",
        "_prioritized_",
        "*pin*",
        "_pin_",
        ":round_pushpin:",
        "📍",
    ];
    let markers = configured.into_iter().chain(defaults).collect::<Vec<_>>();
    longest_marker_prefix(trimmed, &markers)
}

/// One leading lifecycle marker, not a pin and not `🙋`.
fn strip_lifecycle_token(trimmed: &str) -> Option<&str> {
    let icons = agent_doc_queue_icons::queue_icons();
    let configured = [
        icons.claimed.as_str(),
        icons.running.as_str(),
        icons.pending.as_str(),
        icons.blocked.as_str(),
        icons.reported.as_str(),
        icons.lost.as_str(),
        icons.reconciling.as_str(),
        icons.unknown.as_str(),
        icons.skipped.as_str(),
        icons.awaiting_operator.as_str(),
    ];
    let defaults = [
        "🟡",
        "🚧",
        "📥",
        "⛔",
        "☑️",
        "📫",
        "📤",
        "⚠️",
        "🔄",
        "❔",
        "⏭\u{fe0f}",
        "⏳",
    ];
    let markers = configured.into_iter().chain(defaults).collect::<Vec<_>>();
    longest_marker_prefix(trimmed, &markers).map(|(_, rest)| rest)
}

/// Lifecycle projection on a sub-prompt child, with operator pins kept
/// (`#nestedsubprompts`).
///
/// A child is not its own head, so a lifecycle marker, `🙋`, an owner
/// annotation, or a blocker annotation on it is binary-owned whether or not
/// an owner annotation follows the marker. A pin stays, including one written
/// before or after the marker. `None` when nothing was removed, or when
/// removal would leave the child empty.
pub(crate) fn without_child_lifecycle_projection(text: &str) -> Option<String> {
    let mut rest = text.trim_start();
    let mut pins: Vec<&str> = Vec::new();
    let mut dropped = false;
    loop {
        if let Some((pin, after)) = leading_pin(rest) {
            pins.push(pin);
            rest = after.trim_start();
            continue;
        }
        if let Some(after) = strip_operator_action_glyph(rest)
            .or_else(|| strip_lifecycle_token(rest))
            .or_else(|| strip_blocked_annotation_token(rest))
            .or_else(|| strip_owner_annotation_token(rest))
        {
            dropped = true;
            rest = after.trim_start();
            continue;
        }
        break;
    }
    if !dropped || rest.is_empty() {
        return None;
    }
    if pins.is_empty() {
        return Some(rest.to_string());
    }
    let mut out = String::new();
    for (index, pin) in pins.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(pin);
    }
    out.push(' ');
    out.push_str(rest);
    Some(out)
}

/// `text` with every leading binary-owned *attempt projection* token removed:
/// lifecycle/priority markers (`☑️`, `🔄`, `📥`, `📌`, …), exact-attempt owner
/// annotations (`[#subagent:x#1]`, legacy `⟦…⟧`), and the bare operator-action
/// glyph `🙋` (`#queuerowworkidentity`).
///
/// It returns a suffix of `text`, so callers that need the row's work
/// identity can key on it without re-rendering the line. Unlike
/// [`strip_priority_markers`] it keeps everything after the projection prefix
/// verbatim: a `[#id]` following `🙋` is the row's own id (the canonical
/// `🙋 [#subagent-operator-request-x]: answer` row, or the legacy
/// `🙋 [#id] <work>` prefix), and trailing operator-request annotations stay.
/// The Markdown-AST overlay keys queue rows on this remainder, so a claimed
/// `☑️ [#subagent:x#1] do [#foo]`, its `📥`/`🔄` re-projections, and its struck
/// `~~do [#foo]~~` copy are one node.
pub fn strip_attempt_projection_prefix(text: &str) -> &str {
    let mut t = text.trim_start();
    while let Some(rest) = strip_operator_action_glyph(t).or_else(|| strip_projection_token(t)) {
        t = rest.trim_start();
    }
    t
}

/// The work text a strike keeps: `text` (trimmed) without its leading attempt
/// projection prefix ([`strip_attempt_projection_prefix`]): lifecycle and pin
/// markers, exact-attempt owner annotations, and the bare `🙋` glyph. A struck
/// row is completed work, so none of that projection survives inside the
/// strike. A row that is nothing but projection keeps its trimmed text, so a
/// strike never wraps an empty body.
pub fn struck_work_text(text: &str) -> &str {
    let work = strip_attempt_projection_prefix(text).trim();
    if work.is_empty() { text.trim() } else { work }
}

/// The one single-line strike formatter: `~~<work text>~~` for a queue row
/// body (without its `- ` list marker). Every single-line strike path renders
/// through this, so a strike drops the lifecycle marker and owner annotation
/// and wraps only the work text, byte for byte. Under GFM the result is one
/// strikethrough span over the whole work text, including inline code,
/// links, and images inside it.
///
/// A head with nested sub-prompts (`#nestedsubprompts`) strikes its parent
/// part and keeps the sub-prompt block beneath it as a Markdown list
/// (`~~parent~~` with its children below). Closing the parent closes the
/// children, so every live child is struck too (`  - ~~child~~`,
/// `#struckparentkids`); an already-struck parent (a reopened block) is not
/// struck twice. The queue enumerators read that shape back as one completed
/// item.
pub fn render_struck_row_body(text: &str) -> String {
    let (head, block) = subprompts::split_subprompt_block(text);
    match block {
        Some(block) => format!(
            "~~{}~~\n{}",
            subprompts::unwrap_struck_parent(struck_work_text(head)),
            subprompts::strike_subprompt_block(block)
        ),
        None => format!("~~{}~~", struck_work_text(text)),
    }
}

/// Strip all leading lifecycle/priority markers (`:pushpin:`/`📌`/`📍`/`🟡`/`🚧`/`📥`/`☑️`/`⚠️`/`⏭️`/`⏳`
/// and the `**pin**`/`**prioritized**` word forms) from a queue head so a
/// marker-decorated line keys to the same durable identity as the bare line.
/// Shared with the CRDT cell-merge free-text keyer so merge, convergence, and
/// dedup all normalize the same marker set.
pub fn strip_priority_markers(text: &str) -> String {
    let mut t = text.trim();
    let mut canonical_operator_action = false;
    loop {
        let trimmed = t.trim_start();
        if let Some(after_marker) = strip_operator_action_glyph(trimmed) {
            let after_marker = after_marker.trim_start();
            if let Some(after_open) = after_marker.strip_prefix("[#")
                && let Some((_, after)) = after_open.split_once(']')
            {
                // Legacy form: the request id was part of the prefix.
                t = after.trim_start();
            } else {
                // Canonical form: the request id is local to the tail.
                canonical_operator_action = true;
                t = after_marker;
            }
            continue;
        }
        match strip_projection_token(trimmed) {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    let mut bare = t.trim();
    if let Some((id, after, start)) = trailing_operator_action_annotation(bare)
        && (canonical_operator_action
            || id
                .to_ascii_lowercase()
                .starts_with("subagent-operator-request-"))
    {
        let identity_end = bare.len().saturating_sub(after.len());
        bare = if start == 0 {
            bare[..identity_end].trim_end()
        } else {
            bare[..start].trim_end()
        };
    }
    bare.to_string()
}

/// The **work identity** of a live queue head: the one key queue claims,
/// subagent dispatch, mid-turn steering alignment, in-session drainability
/// and preflight head selection all agree on (`#claimdispatchidentity`).
///
/// It answers "which piece of work is this line about?", so it ignores
/// everything on the line that only says *how* to run that work:
///
/// * lifecycle / priority markers (`🟡`, `🚧`, `📥`, `⚠️`, `⏭️`, `📌`, `**pin**`, ...), via
///   [`strip_priority_markers`];
/// * leading intent/preset tags: `#subagents`, `#subagent:`, `#gh-fix`,
///   `#bug:` — any leading `#tag` / `#tag:` token that starts with a letter and
///   is followed by more text. Tags route the work (inline vs subagent, which
///   preset expands it); they do not change what the work is. A lone `#tag`
///   with nothing after it is kept, so a bare `#id` head stays id-backed.
///
/// What remains is keyed through [`QueueItemIdentity::from_prompt`]: an
/// id-backed remainder (`do [#a]`, `[#a]`, `do #a`, a bare `#a`) keys on the
/// id, anything else on its whitespace-collapsed, lowercased text — so a URL
/// or the operator's prose is the key.
///
/// The retarget boundary follows from that: an edit that only adds, removes or
/// swaps tags/markers is the SAME work (a claim follows it), while an edit to
/// the remainder — a different `#id`, a different URL, different prose — is a
/// different task and gets a fresh identity.
///
/// [`QueueItemIdentity::from_prompt`] itself stays tag-sensitive: CRDT
/// convergence and dedup decide whether two *lines of text* are one line, and
/// collapsing `#gh-fix <url>` with `#review <url>` there would delete operator
/// text. This key only decides which in-flight work a line names.
///
/// A head's nested sub-prompts (`#nestedsubprompts`) are steering for the
/// work, never part of its identity: the key is computed from
/// [`subprompts::head_without_subprompts`], so editing, adding or removing a
/// child row keeps the parent's claim and reaches its owner as an edit.
pub fn queue_head_identity(text: &str) -> QueueItemIdentity {
    QueueItemIdentity::from_prompt(&strip_intent_tags(subprompts::head_without_subprompts(
        text,
    )))
}

/// `text` with leading markers and leading `#tag` / `#tag:` intent tokens
/// removed (see [`queue_head_identity`]). Never strips the last token, so a
/// bare `#id` survives.
pub fn strip_intent_tags(text: &str) -> String {
    let mut current = strip_priority_markers(text);
    loop {
        let trimmed = current.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            break;
        };
        if !rest
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            break;
        }
        let tag_len = id_prefix(rest).len();
        let after = &rest[tag_len..];
        let after = after.strip_prefix(':').unwrap_or(after);
        // A tag must be a whole token: `#gh-fix:` / `#gh-fix ` qualify,
        // `#gh-fix/x` or `#tag]` do not.
        if !after.is_empty() && !after.starts_with(char::is_whitespace) {
            break;
        }
        let remainder = strip_priority_markers(after);
        if remainder.is_empty() {
            break;
        }
        current = remainder;
    }
    current.trim().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn descriptor_recognizes_run_and_loop_component_spellings() {
        assert!(super::DESCRIPTOR.matches_name("queue"));
        assert!(super::DESCRIPTOR.matches_name("run"));
        assert!(super::DESCRIPTOR.matches_name("loop"));
    }

    /// `#queueblocked`: the blocked marker and its blocker annotation are
    /// projection, so identity, strike text and the attempt-prefix strip all
    /// see only the work.
    #[test]
    fn blocked_marker_and_annotation_are_projection() {
        let row = "⛔ ⟦blocked by sbx, deploy⟧ 📍 do [#stg]";
        assert_eq!(super::strip_priority_markers(row), "do [#stg]");
        assert_eq!(super::strip_attempt_projection_prefix(row), "do [#stg]");
        assert_eq!(super::render_struck_row_body(row), "~~do [#stg]~~");
        assert_eq!(
            super::queue_head_identity(row),
            super::queue_head_identity("do [#stg]")
        );
        // An unterminated lookalike is the operator's text.
        assert_eq!(
            super::strip_priority_markers("⟦blocked by sbx do [#stg]"),
            "⟦blocked by sbx do [#stg]"
        );
    }

    /// `#claimdispatchidentity`: tags and markers are routing, not work.
    #[test]
    fn head_identity_ignores_intent_tags_and_markers_but_not_the_work() {
        let url = "https://github.com/btakita/agent-doc/issues/120";
        let claimed = queue_head_identity(&format!("#subagents: {url}"));
        for edited in [
            format!("#subagents: #gh-fix {url}"),
            format!("🟡 #subagents: #gh-fix {url}"),
            format!("🚧 #subagents: #gh-fix {url}"),
            format!("📥 #subagents: #gh-fix {url}"),
            format!("⚠️ #subagents: #gh-fix {url}"),
            format!("🟡 [#subagent:worker#2] #subagents: #gh-fix {url}"),
            format!("🟡 ⟦subagent:worker#2⟧ #subagents: #gh-fix {url}"),
            format!("🟡 ⟦owner=subagent:worker;generation=2⟧ #subagents: #gh-fix {url}"),
            format!("#gh-fix {url}"),
            format!("#subagent: {url}"),
            format!("⏭\u{fe0f} #subagents 📌 {url}"),
            url.to_string(),
        ] {
            assert_eq!(queue_head_identity(&edited), claimed, "{edited:?}");
        }
        assert_eq!(
            claimed,
            QueueItemIdentity::FreeText(url.to_ascii_lowercase())
        );
        // A different URL is a different task.
        assert_ne!(
            queue_head_identity("#subagents: https://github.com/btakita/agent-doc/issues/121"),
            claimed
        );
        // Id-backed: tags around a tracked id key on the id.
        assert_eq!(
            queue_head_identity("#subagents do [#a]"),
            QueueItemIdentity::Id("a".into())
        );
        assert_eq!(
            queue_head_identity("#subagents #a"),
            QueueItemIdentity::Id("a".into())
        );
        assert_eq!(queue_head_identity("#a"), QueueItemIdentity::Id("a".into()));
        assert_ne!(
            queue_head_identity("#subagents do [#b]"),
            queue_head_identity("do [#a]")
        );
        // Prose is the key for a tagged free-text head.
        assert_eq!(
            queue_head_identity("#bug: The tmux panes are swapped."),
            queue_head_identity("#subagents #bug: the tmux panes are swapped.")
        );
        // Numeric `#124` is a reference, not a tag; non-token `#x/y` is kept.
        assert_ne!(
            queue_head_identity("#124 fix it"),
            queue_head_identity("#125 fix it")
        );
        assert_ne!(queue_head_identity("#x/y fix"), queue_head_identity("fix"));
        // Convergence identity is unchanged: still tag-sensitive.
        assert_ne!(
            QueueItemIdentity::from_prompt(&format!("#gh-fix {url}")),
            QueueItemIdentity::from_prompt(&format!("#review {url}"))
        );
    }

    /// `#presetargdedup`: sibling preset invocations that differ only in their
    /// argument are distinct queue items; a bare `#id` stays id-backed.
    #[test]
    fn preset_invocations_with_arguments_key_by_full_text() {
        let a = QueueItemIdentity::from_prompt("#gh-fix https://x/issues/69");
        let b = QueueItemIdentity::from_prompt("#gh-fix https://x/issues/70");
        assert!(!a.is_id_backed(), "{a:?}");
        assert_ne!(a, b);
        assert_eq!(
            QueueItemIdentity::from_prompt("#abc123"),
            QueueItemIdentity::Id("abc123".to_string())
        );
        assert_eq!(
            QueueItemIdentity::from_prompt("do #abc123 then more"),
            QueueItemIdentity::Id("abc123".to_string())
        );
        assert_eq!(
            QueueItemIdentity::from_prompt("[#abc123] described"),
            QueueItemIdentity::Id("abc123".to_string())
        );
    }

    use super::*;

    const ALL_STATES: [QueueItemState; 6] = [
        QueueItemState::Authored,
        QueueItemState::Mirrored,
        QueueItemState::InProgress,
        QueueItemState::Answered,
        QueueItemState::Struck,
        QueueItemState::Reaped,
    ];

    const ALL_EVENTS: [QueueItemEvent; 6] = [
        QueueItemEvent::OperatorAuthored,
        QueueItemEvent::BacklogMirrored,
        QueueItemEvent::DrainStarted,
        QueueItemEvent::ExchangeAnswered,
        QueueItemEvent::StruckThrough,
        QueueItemEvent::Reaped,
    ];

    fn t(state: QueueItemState, event: QueueItemEvent) -> QueueItemState {
        transition_queue_item(&state, &event).expect("transition is total")
    }

    #[test]
    fn rank_orders_live_before_struck() {
        assert!(QueueItemLifecycle::Live.rank() < QueueItemLifecycle::Struck.rank());
        assert!(QueueItemLifecycle::Live < QueueItemLifecycle::Struck);
    }

    #[test]
    fn free_text_identity_is_marker_invariant() {
        // A marker-decorated free-text re-emit must key to the SAME identity as
        // the operator's bare line so convergence can collapse the resurrected
        // copy (#qdedup-directive-twin free-text variant). Covers the new emoji
        // set (📌/📍/🚧/⏭️) plus shortcodes and word forms.
        let bare = QueueItemIdentity::from_prompt("fix the flaky test");
        for decorated in [
            "🚧 fix the flaky test",
            "📌 fix the flaky test",
            "📍 fix the flaky test",
            "⏭\u{fe0f} fix the flaky test",
            "⏳ fix the flaky test",
            "🙋🚧 fix the flaky test [#subagent-operator-request-flaky]",
            "🚧 fix the flaky test [#subagent-operator-request-flaky]: coordinator releases",
            ":pushpin: fix the flaky test",
            "**pin** fix the flaky test",
        ] {
            assert_eq!(
                QueueItemIdentity::from_prompt(decorated),
                bare,
                "marker-decorated free text must share the bare line's identity: {decorated:?}"
            );
        }
        assert_eq!(
            bare,
            QueueItemIdentity::FreeText("fix the flaky test".to_string())
        );
    }

    #[test]
    fn local_operator_answer_is_keyed_by_reserved_request_id() {
        let id = "subagent-operator-request-publish-ci-pr166";
        assert_eq!(
            queue_head_identity(&format!("[#{id}]: **Commit and push to PR #166**")),
            QueueItemIdentity::Id(id.to_string())
        );
        let quoted = format!(
            "In sdk-demo.md, no queue item status despite agent running: `- [#{id}]: **Commit and push to PR #166**`"
        );
        assert_eq!(strip_priority_markers(&quoted), quoted);
    }

    #[test]
    fn join_is_monotonic_max() {
        use QueueItemLifecycle::*;
        assert_eq!(Live.join(Live), Live);
        assert_eq!(Live.join(Struck), Struck);
        assert_eq!(Struck.join(Live), Struck, "struck never regresses to live");
        assert_eq!(Struck.join(Struck), Struck);
    }

    #[test]
    fn join_is_idempotent_and_commutative() {
        use QueueItemLifecycle::*;
        for &a in &[Live, Struck] {
            assert_eq!(a.join(a), a, "idempotent");
            for &b in &[Live, Struck] {
                assert_eq!(a.join(b), b.join(a), "commutative");
            }
        }
    }

    #[test]
    fn classify_detects_strike_on_head_line() {
        assert_eq!(
            QueueItemLifecycle::classify("- do [#abcd] fix"),
            QueueItemLifecycle::Live
        );
        assert_eq!(
            QueueItemLifecycle::classify("- ~~do [#abcd] fix~~"),
            QueueItemLifecycle::Struck
        );
        assert_eq!(
            QueueItemLifecycle::classify("- ~~do [#abcd] fix~~\n  note line"),
            QueueItemLifecycle::Struck
        );
        assert_eq!(
            QueueItemLifecycle::classify("- do [#abcd] fix\n  ~~old note~~"),
            QueueItemLifecycle::Live
        );
    }

    #[test]
    fn queue_item_state_rank_is_strictly_ascending_lifecycle_order() {
        for pair in ALL_STATES.windows(2) {
            assert!(pair[0].rank() < pair[1].rank());
            assert!(pair[0] < pair[1]);
        }
    }

    #[test]
    fn queue_item_transition_is_total_monotonic_and_idempotent() {
        for &state in &ALL_STATES {
            for &event in &ALL_EVENTS {
                let once = t(state, event);
                let twice = t(once, event);
                assert!(once.rank() >= state.rank());
                assert_eq!(once, twice);
                if event.target().rank() > state.rank() {
                    assert_eq!(once, event.target());
                } else {
                    assert_eq!(once, state);
                }
            }
        }
    }

    #[test]
    fn queue_item_reaped_is_absorbing() {
        for &event in &ALL_EVENTS {
            assert_eq!(t(QueueItemState::Reaped, event), QueueItemState::Reaped);
        }
        assert!(QueueItemState::Reaped.is_terminal());
    }

    #[test]
    fn queue_item_machine_drives_canonical_lifecycle() {
        let machine = QueueItemMachine::new(QueueItemState::Authored);
        assert_eq!(machine.state(), QueueItemState::Authored);

        assert!(machine.send(QueueItemEvent::BacklogMirrored));
        assert_eq!(machine.state(), QueueItemState::Mirrored);
        assert!(machine.send(QueueItemEvent::DrainStarted));
        assert_eq!(machine.state(), QueueItemState::InProgress);
        assert!(machine.send(QueueItemEvent::ExchangeAnswered));
        assert_eq!(machine.state(), QueueItemState::Answered);
        assert!(machine.send(QueueItemEvent::StruckThrough));
        assert_eq!(machine.state(), QueueItemState::Struck);
        assert!(machine.send(QueueItemEvent::Reaped));
        assert_eq!(machine.state(), QueueItemState::Reaped);
    }

    #[test]
    fn queue_item_stale_reemit_storm_never_resurrects_an_advanced_item() {
        let machine = QueueItemMachine::new(QueueItemState::Authored);
        machine.send(QueueItemEvent::BacklogMirrored);
        machine.send(QueueItemEvent::DrainStarted);
        machine.send(QueueItemEvent::ExchangeAnswered);
        assert_eq!(machine.state(), QueueItemState::Answered);

        for _ in 0..10 {
            machine.send(QueueItemEvent::OperatorAuthored);
            machine.send(QueueItemEvent::BacklogMirrored);
        }
        assert_eq!(machine.state(), QueueItemState::Answered);
    }

    #[test]
    fn queue_item_lifecycle_projection_commutes_with_join() {
        for &a in &ALL_STATES {
            for &b in &ALL_STATES {
                let joined_state = if a.rank() >= b.rank() { a } else { b };
                assert_eq!(joined_state.lifecycle(), a.lifecycle().join(b.lifecycle()));
            }
        }
    }

    #[test]
    fn queue_item_identity_collapses_pin_and_prefix_variants() {
        let bracketed = QueueItemIdentity::from_prompt(":pushpin: do [#gww8] thing");
        let bare_prefix = QueueItemIdentity::from_prompt("do [#gww8] thing");
        let plain = QueueItemIdentity::from_prompt("[#gww8]");
        assert_eq!(bracketed, QueueItemIdentity::Id("gww8".into()));
        assert_eq!(bracketed, bare_prefix);
        assert_eq!(bracketed, plain);
        assert!(bracketed.is_id_backed());
    }

    #[test]
    fn queue_item_free_text_identity_keys_on_normalized_text() {
        let a = QueueItemIdentity::from_prompt("Still getting JB File Cache Conflict dialogs.");
        let b = QueueItemIdentity::from_prompt("still getting   jb file cache conflict dialogs.");
        let other = QueueItemIdentity::from_prompt("Opencode reverse-orders numeric lists.");
        assert_eq!(a, b);
        assert_ne!(a, other);
        assert!(!a.is_id_backed());
    }

    #[test]
    fn queue_item_identity_keeps_bare_bracketed_id_colon_notes_as_free_text() {
        let note = QueueItemIdentity::from_prompt("[#todo-a]: note");
        let pinned_note = QueueItemIdentity::from_prompt(":pushpin: [#todo-a]: note");
        let explicit_directive = QueueItemIdentity::from_prompt("do [#todo-a]: note");

        assert_eq!(note, QueueItemIdentity::FreeText("[#todo-a]: note".into()));
        assert_eq!(pinned_note, note);
        assert_eq!(explicit_directive, QueueItemIdentity::Id("todo-a".into()));
    }
}
