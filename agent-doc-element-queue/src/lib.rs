//! Queue element descriptor and local realtime model.

use agent_doc_element::{
    ElementAuthority, ElementCompositionRole, ElementDescriptor, ElementRealtimeModel,
    ElementSchedulingRole, ElementShape, ElementSource, ElementTurnRole, ElementWritePolicy,
};
use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt, ops::Range, str::FromStr};

pub mod nested_claim_steer;
pub mod pipe;
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

/// Worker-reported publication state persisted on a queue claim and rendered
/// as a canonical trailing annotation on the live queue row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueueReportState {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "published")]
    Published,
    #[serde(rename = "published+local")]
    PublishedLocal,
}

impl QueueReportState {
    pub const ALL: [Self; 3] = [Self::Local, Self::Published, Self::PublishedLocal];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Published => "published",
            Self::PublishedLocal => "published+local",
        }
    }

    pub const fn annotation(self) -> &'static str {
        match self {
            Self::Local => "⟦local⟧",
            Self::Published => "⟦published⟧",
            Self::PublishedLocal => "⟦published+local⟧",
        }
    }

    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Local => "✎",
            Self::Published => "⇧",
            // Preserve the state transition in reading order: published first,
            // then a newer local edit.
            Self::PublishedLocal => "⇧✎",
        }
    }
}

impl fmt::Display for QueueReportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for QueueReportState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == value)
            .ok_or_else(|| format!("unknown publication state {value:?}; expected local, published, or published+local"))
    }
}

/// Exact byte range and typed state of a canonical trailing annotation.
pub fn queue_report_state_annotation(text: &str) -> Option<(Range<usize>, QueueReportState)> {
    let trimmed = text.trim_end();
    QueueReportState::ALL.into_iter().find_map(|state| {
        trimmed
            .strip_suffix(state.annotation())
            .filter(|prefix| prefix.ends_with(char::is_whitespace))
            .map(|prefix| (prefix.len()..trimmed.len(), state))
    })
}

/// Remove one canonical trailing report-state annotation without touching a
/// prose occurrence elsewhere in the work text.
pub fn strip_queue_report_state_annotation(text: &str) -> &str {
    queue_report_state_annotation(text)
        .map(|(range, _)| text[..range.start].trim_end())
        .unwrap_or(text)
}

/// Replace any prior report-state suffix with the claim's authoritative state.
pub fn render_queue_report_state_annotation(text: &str, state: Option<QueueReportState>) -> String {
    let (head, tail) = text
        .split_once('\n')
        .map_or((text, None), |(head, tail)| (head, Some(tail)));
    let rendered_head = match state {
        Some(state) => format!(
            "{} {}",
            strip_queue_report_state_annotation(head).trim_end(),
            state.annotation()
        ),
        None => match queue_report_state_annotation(head) {
            Some(_) => strip_queue_report_state_annotation(head).to_string(),
            None => head.to_string(),
        },
    };
    match tail {
        Some(tail) => format!("{rendered_head}\n{tail}"),
        None => rendered_head,
    }
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

/// The keyword after a follow-up row's leading origin reference
/// (`[#<origin>] follow-up: <item>`, `#followupshorttitle`).
pub const FOLLOWUP_REFERENCE_KEYWORD: &str = "follow-up:";

/// Whether `after_close`, the text right after a leading `[#id]` token's `]`,
/// marks that token as a follow-up row's origin reference: whitespace, then
/// [`FOLLOWUP_REFERENCE_KEYWORD`] (any case).
///
/// Every parser that reads a leading `[#id]` as the row's own id (the
/// optional-`do` grammar, the legacy `🙋 [#id] <work>` request prefix, the
/// Markdown-AST node key) checks this first, so a follow-up row never aliases
/// the identity of the row it follows up.
pub fn is_followup_reference_tail(after_close: &str) -> bool {
    if !after_close.starts_with(char::is_whitespace) {
        return false;
    }
    let rest = after_close.trim_start();
    rest.get(..FOLLOWUP_REFERENCE_KEYWORD.len())
        .is_some_and(|keyword| keyword.eq_ignore_ascii_case(FOLLOWUP_REFERENCE_KEYWORD))
}

/// `(origin id, item)` of a follow-up row's work text
/// `[#<origin>] follow-up: <item>` (`#followupshorttitle`). `text` is the
/// row's work text after its lifecycle projection
/// ([`strip_attempt_projection_prefix`]); leading whitespace is ignored. The
/// origin id is returned as written; the item is the rest of the row, owner
/// label (`operator:`, `agent:`, `human (<who>):`) and annotations included.
/// `None` for any other row, including an empty item.
pub fn followup_origin_reference(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start().strip_prefix("[#")?;
    let (id, after) = rest.split_once(']')?;
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        || !is_followup_reference_tail(after)
    {
        return None;
    }
    let item = after.trim_start()[FOLLOWUP_REFERENCE_KEYWORD.len()..].trim_start();
    (!item.trim().is_empty()).then_some((id, item))
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
    // `#followupshorttitle`: `[#origin] follow-up: <item>` references the
    // row it follows up; it is never that row's identity.
    if followup_origin_reference(trimmed).is_some() {
        return None;
    }
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

/// A binary-admitted queue prompt id is a trailing annotation on the first
/// work line (after any effect id, before report state). Keeping it at the tail
/// preserves the row's free-text grammar. The parser also accepts the historic
/// `q-id effect` order so maintenance can repair it without losing identity.
fn generated_prompt_id_annotation(text: &str) -> Option<(&str, Range<usize>)> {
    let first = text.lines().next()?.trim_end();
    let without_report = strip_queue_report_state_annotation(first).trim_end();
    // `effect` predates stable prompt ids. A historical duplicate-effect pass
    // could append its annotation after an admitted q-id, making that id
    // temporarily non-trailing. Recognize and repair that ordering too; the
    // canonical renderer keeps `effect` before q-id going forward.
    let before_id = without_report
        .strip_suffix('⟧')
        .and_then(|prefix| prefix.rsplit_once(" ⟦effect:"))
        .filter(|(_, raw)| raw.parse::<usize>().is_ok())
        .map(|(work, _)| work.trim_end())
        .unwrap_or(without_report);
    let (work, annotation) = before_id.rsplit_once(" [#")?;
    if work.trim().is_empty() {
        return None;
    }
    let id = annotation.strip_suffix(']')?;
    // `#humanfollowupowner`: the id is one bracketed token. A row that quotes
    // an earlier `[#q-…]` (an id-less origin label) and ends in another
    // bracketed tag (`[operator-verify]`) must not read as one id spanning
    // both.
    if !id.starts_with("q-")
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return None;
    }
    let annotation_start = work.len();
    Some((id, annotation_start..annotation_start + id.len() + 4))
}

fn trailing_generated_prompt_id(text: &str) -> Option<&str> {
    generated_prompt_id_annotation(text).map(|(id, _)| id)
}

/// The binary-generated trailing `[#q-…]` a prompt's first line carries,
/// lowercased, whatever its authored head is (`#address [#q-…]`,
/// `parent: #leaf [#q-…]`). `None` when the row has not been admitted one.
/// Pairs with [`strip_generated_queue_prompt_id_annotation`]: the two split a
/// row into its authored text and its occurrence id (`#claimidsurvives`).
pub fn generated_queue_prompt_id(text: &str) -> Option<String> {
    trailing_generated_prompt_id(text).map(str::to_ascii_lowercase)
}

/// `#struckidtrail`: the generated `[#q-…]` id a struck row holds at the end
/// of its strike (`~~work [#q-…]~~`), or `None` when the row's first line does
/// not end with a closing `~~` that wraps a trailing generated id.
///
/// A strike wraps the whole work text, id included, so the id sits before the
/// closing `~~` and [`queue_prompt_stable_id`] (which reads the line's tail)
/// does not see it. A writer that adds the id when it is "missing" must ask
/// this too, or it appends a second copy after the strike, and the row then
/// reads as live (`~~work [#q-x]~~ [#q-x]`).
pub fn struck_generated_prompt_id(text: &str) -> Option<&str> {
    let first = text.lines().next()?.trim_end();
    let inner = first.strip_suffix("~~")?;
    let open = inner.find("~~")?;
    trailing_generated_prompt_id(&inner[open + 2..])
}

/// `#struckidtrail`: `line` without the copies of its struck id that a
/// writer appended after the closing `~~` (`~~work [#q-x]~~ [#q-x]` →
/// `~~work [#q-x]~~`), or `None` when the line has no such trailing copy.
/// Only an exact repeat of the id inside the strike is folded; any other
/// trailing text, or a different id, leaves the line unchanged.
pub fn fold_struck_row_trailing_id(line: &str) -> Option<String> {
    let mut current = line.trim_end();
    let mut copies = Vec::new();
    while !current.ends_with("~~") {
        let (head, annotation) = current.rsplit_once(" [#")?;
        copies.push(annotation.strip_suffix(']')?);
        current = head.trim_end();
    }
    let inside = struck_generated_prompt_id(current)?;
    (!copies.is_empty() && copies.iter().all(|id| id.eq_ignore_ascii_case(inside)))
        .then(|| current.to_string())
}

/// Move a generated queue id that was published ahead of its list marker back
/// to the trailing annotation slot.
///
/// The admitted form is ` [#{id}]` appended to the work line. Inserting that
/// token at the list marker's origin, concurrently with a replacement that
/// inserts a new `-` at the same origin, publishes ` [#q-id]- …` when the
/// annotation's op sorts first. That line no longer starts with a bullet, so
/// the row is absorbed into the previous item. A correct trailing id does not
/// contain `]-` and is left untouched.
pub fn repair_generated_id_before_list_marker<'a>(text: &'a str) -> Cow<'a, str> {
    if !text.contains("]-") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        match repair_generated_id_line(line) {
            Some(repaired) => {
                changed = true;
                out.push_str(&repaired);
            }
            None => out.push_str(line),
        }
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    }
}

fn repair_generated_id_line(line: &str) -> Option<String> {
    let ending = if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    let core = line.trim_end_matches(['\n', '\r']);
    let leading = core.len() - core.trim_start().len();
    if leading == 0 || core.as_bytes().get(leading - 1) != Some(&b' ') {
        return None;
    }
    let trimmed = &core[leading..];
    if !trimmed.starts_with("[#q-") {
        return None;
    }
    let close = trimmed.find("]-")?;
    let id = &trimmed[2..close];
    if !id.starts_with("q-")
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return None;
    }
    let after = &trimmed[close + 2..];
    if !after.is_empty() && !after.starts_with([' ', '\t']) {
        return None;
    }
    let indent = &core[..leading - 1];
    let body = after.trim_start();
    let mut restored = if body.is_empty() {
        format!("{indent}-")
    } else {
        format!("{indent}- {body}")
    };
    let token = format!("[#{id}]");
    if !restored.contains(&token) {
        let insert_at = strip_queue_report_state_annotation(&restored).len();
        restored.insert_str(insert_at, &format!(" {token}"));
    }
    restored.push_str(ending);
    Some(restored)
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
        icons.seen.as_str(),
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
        // `#promptseen`: seen glyph, before prepare emits the prompt.
        "👀",
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
        .or_else(|| strip_waiting_status_token(trimmed))
        .or_else(|| strip_open_count_token(trimmed))
        // `#treetopglyph`: the tree delete/keep glyph is re-derived
        // projection, never work identity.
        .or_else(|| strip_tree_indicator_token(trimmed))
        // `#lifecyclewordfusion`: a state word fused to its glyph
        // (`running🚧`) is one torn lifecycle token, never work identity.
        .or_else(|| {
            agent_doc_queue_icons::fused_lifecycle_word_token(trimmed)
                .map(|(_, len)| &trimmed[len..])
        })
}

/// `#treetopglyph`: which tree glyph a row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TreeIndicator {
    /// `🗑️`: the top-most row of a subtree that may be deleted whole.
    Delete,
    /// `🔒`: the top-level row of a kept tree that holds 🗑️ subtrees.
    Keep,
}

impl TreeIndicator {
    /// The glyph agent-doc renders.
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Delete => agent_doc_queue_icons::TREE_DELETE_GLYPH,
            Self::Keep => agent_doc_queue_icons::TREE_KEEP_GLYPH,
        }
    }
}

/// Parse ONE leading tree glyph (`🗑️`, also without its variation selector,
/// or `🔒`) from `trimmed`: which one, and the text after it.
///
/// The glyph is projection agent-doc re-derives on every sync from the queue
/// tree, never work text and never identity, so every parser strips it with
/// the rest of the leading projection prefix ([`strip_attempt_projection_prefix`],
/// [`strip_priority_markers`]). Inside a strike (`~~🗑️ text~~`) it is the
/// first token of the struck text, which those parsers also read past.
pub fn tree_indicator_token(trimmed: &str) -> Option<(TreeIndicator, &str)> {
    if let Some(rest) = trimmed.strip_prefix(agent_doc_queue_icons::TREE_DELETE_GLYPH) {
        return Some((TreeIndicator::Delete, rest));
    }
    if let Some(rest) = trimmed.strip_prefix(agent_doc_queue_icons::TREE_DELETE_GLYPH_BARE) {
        return Some((TreeIndicator::Delete, rest));
    }
    trimmed
        .strip_prefix(agent_doc_queue_icons::TREE_KEEP_GLYPH)
        .map(|rest| (TreeIndicator::Keep, rest))
}

/// Strip ONE leading tree glyph (see [`tree_indicator_token`]).
pub fn strip_tree_indicator_token(trimmed: &str) -> Option<&str> {
    tree_indicator_token(trimmed).map(|(_, rest)| rest)
}

/// `text` with every leading tree glyph removed (whitespace between them
/// too), and the glyphs it held, in order. Used on the inside of a strike,
/// where the glyph is the first token of the struck text.
pub fn split_leading_tree_indicators(text: &str) -> (Vec<TreeIndicator>, &str) {
    let mut present = Vec::new();
    let mut rest = text;
    while let Some((indicator, after)) = tree_indicator_token(rest.trim_start()) {
        present.push(indicator);
        rest = after;
    }
    if present.is_empty() {
        (present, text)
    } else {
        (present, rest.trim_start())
    }
}

/// The first tree glyph in `text`'s leading projection prefix (a live row's
/// `🙋 🔒 …`), for a pass that re-renders the prefix and must keep the glyph
/// the response-tree sync owns.
pub fn tree_indicator_in_prefix(text: &str) -> Option<TreeIndicator> {
    let trimmed = text.trim_start();
    let work = strip_attempt_projection_prefix(trimmed);
    prefix_without_tree_indicators(&trimmed[..trimmed.len() - work.len()])
        .1
        .first()
        .copied()
}

/// A live row's projection prefix (the part of the row before
/// [`strip_attempt_projection_prefix`]'s remainder) with every tree glyph
/// removed, and the glyphs it held. Every other token keeps its bytes and
/// the whitespace before it.
pub fn prefix_without_tree_indicators(prefix: &str) -> (String, Vec<TreeIndicator>) {
    let mut out = String::with_capacity(prefix.len());
    let mut present = Vec::new();
    let mut rest = prefix;
    loop {
        let trimmed = rest.trim_start();
        let gap = &rest[..rest.len() - trimmed.len()];
        if let Some((indicator, after)) = tree_indicator_token(trimmed) {
            present.push(indicator);
            rest = after;
            continue;
        }
        match strip_operator_action_glyph(trimmed).or_else(|| strip_projection_token(trimmed)) {
            Some(after) if after.len() < trimmed.len() => {
                out.push_str(gap);
                out.push_str(&trimmed[..trimmed.len() - after.len()]);
                rest = after;
            }
            _ => {
                out.push_str(rest);
                break;
            }
        }
    }
    (out, present)
}

/// `#waitingitemstatus`: label words of the binary-owned waiting-item roll-up.
pub const WAITING_STATUS_LABELS: [&str; 3] = ["you", "agent", "done"];

/// Render the binary-owned state and roll-up token of a waiting-item head:
/// `⟦<state> you: N · agent: N · done: N⟧`.
pub fn render_waiting_status_token(state: &str, you: usize, agent: usize, done: usize) -> String {
    if state.is_empty() {
        // `#managedlistnodrift`: the head's own `🙋` gate already shows the
        // state; the token carries only the counts.
        return format!("⟦you: {you} · agent: {agent} · done: {done}⟧");
    }
    format!("⟦{state} you: {you} · agent: {agent} · done: {done}⟧")
}

/// Strip ONE leading waiting-item state and roll-up token
/// (`⟦🙋 you: 1 · agent: 0 · done: 3⟧`). The token is projection recomputed
/// by every closeout, never work text and never identity. Recognition is
/// exact: the three labelled counts in their fixed order, so no operator
/// `⟦...⟧` text is ever mistaken for it.
pub fn strip_waiting_status_token(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('⟦')?;
    let close = rest.find('⟧')?;
    let payload = &rest[..close];
    let counts = match payload.strip_prefix("you: ") {
        // The stateless form, rendered when the head's `🙋` gate shows it.
        Some(counts) => counts,
        None => {
            let (state, counts) = payload.split_once(" you: ")?;
            if state.is_empty() || state.contains(char::is_whitespace) || state.contains('⟦') {
                return None;
            }
            counts
        }
    };
    let (you, rest_counts) = counts.split_once(" · agent: ")?;
    let (agent, done) = rest_counts.split_once(" · done: ")?;
    [you, agent, done]
        .iter()
        .all(|count| !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit()))
        .then(|| &rest[close + '⟧'.len_utf8()..])
}

/// `#looseends`: the groups of the binary-owned open-work count a top-level
/// queue row carries, in render order, as `(singular, plural)` labels. They
/// are the Loose ends digest groups (Decide, Do, Blocked, Running).
pub const OPEN_COUNT_LABELS: [(&str, &str); 4] = [
    ("decision", "decisions"),
    ("action", "actions"),
    ("blocked", "blocked"),
    ("running", "running"),
];

/// Render the open-work count token of a top-level row,
/// `(1 decision, 2 actions, 1 blocked, 1 running)`, from counts in
/// [`OPEN_COUNT_LABELS`] order. Zero groups are left out; `None` when every
/// count is zero (the row then carries no token).
pub fn render_open_count_token(counts: [usize; 4]) -> Option<String> {
    let parts = counts
        .iter()
        .zip(OPEN_COUNT_LABELS)
        .filter(|(count, _)| **count > 0)
        .map(|(count, (one, many))| format!("{count} {}", if *count == 1 { one } else { many }))
        .collect::<Vec<_>>();
    (!parts.is_empty()).then(|| format!("({})", parts.join(", ")))
}

/// Parse ONE leading open-work count token (`(1 decision, 1 running)`) from
/// `trimmed`: its counts in [`OPEN_COUNT_LABELS`] order and the text after it.
///
/// The token is projection the binary re-renders on every sync, never work
/// text and never identity, so every parser strips it with the rest of the
/// leading projection prefix ([`strip_attempt_projection_prefix`],
/// [`strip_priority_markers`]). Recognition is exact so no operator
/// parenthetical is mistaken for it: one or more `<N> <label>` parts, each
/// count a positive number without a leading zero, each label one of
/// [`OPEN_COUNT_LABELS`] in the right number, in that fixed order without
/// repeats, joined by `, `, and followed by whitespace or the end of the text.
pub fn open_count_token(trimmed: &str) -> Option<([usize; 4], &str)> {
    let rest = trimmed.strip_prefix('(')?;
    let close = rest.find(')')?;
    let after = &rest[close + 1..];
    if !(after.is_empty() || after.starts_with(char::is_whitespace)) {
        return None;
    }
    let mut counts = [0usize; 4];
    let mut next = 0;
    for part in rest[..close].split(", ") {
        let (count, label) = part.split_once(' ')?;
        if count.is_empty()
            || count.starts_with('0')
            || !count.chars().all(|ch| ch.is_ascii_digit())
        {
            return None;
        }
        let count = count.parse::<usize>().ok()?;
        let group = (next..OPEN_COUNT_LABELS.len()).find(|&group| {
            let (one, many) = OPEN_COUNT_LABELS[group];
            label == if count == 1 { one } else { many }
        })?;
        counts[group] = count;
        next = group + 1;
    }
    Some((counts, after))
}

/// `#orderedsteps`: byte length of a leading ordered-list marker (`1.`,
/// `12)`) that a space follows, without that space. Every queue list parser
/// reads numbered rows through this one rule, so `N.` and `N)` are the same
/// list shape everywhere.
pub fn ordered_list_marker_len(text: &str) -> Option<usize> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    match text.as_bytes().get(digits..digits + 2) {
        Some([b'.' | b')', b' ']) => Some(digits + 1),
        _ => None,
    }
}

/// `#orderedsteps`: byte width of a leading list marker together with the
/// space after it (`- ` and `* ` and `+ ` are 2, `1. ` is 3, `12) ` is 4),
/// or `None` when `trimmed` does not open a list item. A row's children sit
/// at its indent plus this width, the item's content column.
pub fn list_item_marker_width(trimmed: &str) -> Option<usize> {
    if ["- ", "* ", "+ "]
        .iter()
        .any(|marker| trimmed.starts_with(marker))
    {
        return Some(2);
    }
    ordered_list_marker_len(trimmed).map(|len| len + 1)
}

/// Strip ONE leading open-work count token (see [`open_count_token`]).
pub fn strip_open_count_token(trimmed: &str) -> Option<&str> {
    open_count_token(trimmed).map(|(_, after)| after)
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
    strip_exact_owner_annotation_token(trimmed).or_else(|| {
        // `#lifecyclewordowner`: a state word in front of the annotation
        // (`running [#subagent:x#1]`) is part of the same torn projection.
        agent_doc_queue_icons::lifecycle_word_before_owner_annotation(trimmed)
            .and_then(strip_exact_owner_annotation_token)
    })
}

fn strip_exact_owner_annotation_token(trimmed: &str) -> Option<&str> {
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

/// A Markdown heading token inserted between two copies of binary lifecycle
/// projection. This is projection corruption, not authored work, only when a
/// real projection was already stripped and another projection begins after
/// the heading token. A row whose authored work starts with `###` stays intact.
fn strip_repeated_projection_heading(trimmed: &str) -> Option<&str> {
    let after = trimmed.strip_prefix("###")?;
    if !after.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }
    let candidate = after.trim_start();
    strip_operator_action_glyph(candidate)
        .or_else(|| strip_projection_token(candidate))
        .map(|_| candidate)
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
        icons.seen.as_str(),
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
        "👀",
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
    let mut stripped_projection = false;
    loop {
        if let Some(rest) = strip_operator_action_glyph(t).or_else(|| strip_projection_token(t)) {
            stripped_projection = true;
            t = rest.trim_start();
            continue;
        }
        if stripped_projection && let Some(rest) = strip_repeated_projection_heading(t) {
            t = rest;
            continue;
        }
        break;
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
    let mut stripped_projection = false;
    loop {
        let trimmed = t.trim_start();
        if let Some(after_marker) = strip_operator_action_glyph(trimmed) {
            stripped_projection = true;
            let after_marker = after_marker.trim_start();
            if let Some(after_open) = after_marker.strip_prefix("[#")
                && let Some((_, after)) = after_open.split_once(']')
                && !is_followup_reference_tail(after)
            {
                // Legacy form: the request id was part of the prefix. A
                // follow-up row's leading `[#origin] follow-up:` is its work
                // text, not a request id (`#followupshorttitle`).
                t = after.trim_start();
            } else {
                // Canonical form: the request id is local to the tail.
                canonical_operator_action = true;
                t = after_marker;
            }
            continue;
        }
        match strip_projection_token(trimmed) {
            Some(rest) => {
                stripped_projection = true;
                t = rest.trim_start();
            }
            None if stripped_projection => match strip_repeated_projection_heading(trimmed) {
                Some(rest) => t = rest,
                None => break,
            },
            None => break,
        }
    }
    let mut bare = t.trim();
    // A generated `[#q-…]` is the row's stable id, not an operator request.
    // `🙋` must not strip it (`#opverifyrest`); otherwise preflight drops the
    // id when an `[operator-verify]` follow-up rests as operator wait.
    if let Some((id, after, start)) = trailing_operator_action_annotation(bare)
        && !id.to_ascii_lowercase().starts_with("q-")
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
        // `#dupetags`: a row may carry several request annotations (and, from
        // older binaries, repeated copies of them). Strip the whole run so the
        // work identity does not depend on how many the row carries.
        if start > 0 {
            while let Some((id, after, start)) = trailing_operator_action_annotation(bare) {
                if start == 0
                    || !after.is_empty()
                    || !(id
                        .to_ascii_lowercase()
                        .starts_with("subagent-operator-request-"))
                {
                    break;
                }
                bare = bare[..start].trim_end();
            }
        }
    }
    match bare.split_once('\n') {
        Some((head, tail)) => format!("{}\n{tail}", strip_queue_report_state_annotation(head)),
        None => strip_queue_report_state_annotation(bare).to_string(),
    }
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

/// Durable identity for an executable queue prompt. Authored semantic ids keep
/// their existing meaning; otherwise an admitted trailing `[#q-…]` is the
/// correlation identity. Legacy no-id rows retain the historical text key.
pub fn queue_prompt_stable_identity(text: &str) -> QueueItemIdentity {
    let semantic = queue_head_identity(text);
    if let QueueItemIdentity::Id(id) = &semantic {
        // `#address [#q-…]` strips to `[#q-…]`, which is not tracked work.
        // The q-id correlates the row; the preset token is not a backlog id.
        if id.starts_with("q-") && preset_token_collapsed_to_generated_id(text) {
            return QueueItemIdentity::FreeText(format!("queue-id:{id}"));
        }
        return semantic;
    }
    queue_prompt_stable_id(text)
        .map(|id| QueueItemIdentity::FreeText(format!("queue-id:{id}")))
        .unwrap_or(semantic)
}

/// A leading `#preset` whose remainder is only a generated `[#q-…]`.
fn preset_token_collapsed_to_generated_id(text: &str) -> bool {
    let head = subprompts::head_without_subprompts(text);
    let visible = strip_priority_markers(head.trim());
    let stripped = strip_intent_tags(visible.trim());
    if stripped == visible.trim() {
        return false;
    }
    let Some(rest) = stripped.trim().strip_prefix("[#") else {
        return false;
    };
    let Some((id, tail)) = rest.split_once(']') else {
        return false;
    };
    id.starts_with("q-") && tail.trim().is_empty()
}

/// Authored semantic id or binary-admitted q-id for correlation/navigation.
/// Unlike [`queue_head_identity`], this does not make a q-id a tracked-work id.
pub fn queue_prompt_stable_id(text: &str) -> Option<String> {
    match queue_head_identity(text) {
        QueueItemIdentity::Id(id) => Some(id),
        QueueItemIdentity::FreeText(_) => {
            let work = strip_intent_tags(subprompts::head_without_subprompts(text));
            trailing_generated_prompt_id(&work).map(str::to_ascii_lowercase)
        }
    }
}

/// Remove the binary-owned q-id annotation from a prompt while
/// preserving its authored work text, continuation lines, and report-state
/// annotation. Authored semantic ids are never removed; the historic
/// `q-id effect` order is accepted for repair as well.
pub fn strip_queue_prompt_stable_id_annotation(text: &str) -> String {
    if queue_head_identity(text).is_id_backed() {
        return text.to_string();
    }
    strip_generated_queue_prompt_id_annotation(text)
}

/// Remove only the binary-generated trailing `[#q-*]` occurrence annotation,
/// even when an authored leading intent tag makes the row otherwise id-backed.
/// Claim-repair lineage uses this narrower operation to compare the projected
/// row with its pre-projection parent vector.
pub fn strip_generated_queue_prompt_id_annotation(text: &str) -> String {
    let first_end = text.find('\n').unwrap_or(text.len());
    let first = &text[..first_end];
    let Some((_, annotation)) = generated_prompt_id_annotation(first) else {
        return text.to_string();
    };
    let mut stripped = String::with_capacity(text.len() - annotation.len());
    stripped.push_str(&first[..annotation.start]);
    stripped.push_str(&first[annotation.end..]);
    stripped.push_str(&text[first_end..]);
    stripped
}

/// `text` with leading markers and leading `#tag` / `#tag:` intent tokens
/// removed (see [`queue_head_identity`]). Never strips the last token, so a
/// bare `#id` survives.
///
/// A trailing edge operator (` |` `#pipeprompt`, ` >` `#foldprompt`) is
/// stripped the same way: adding or removing it never changes which work the
/// row names.
pub fn strip_intent_tags(text: &str) -> String {
    let unpiped = pipe::strip_edge_marker(text);
    let mut current = strip_priority_markers(&unpiped);
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

    /// `#looseends`: the open-work count token renders zero-free in its fixed
    /// order and parses back exactly; nothing else parses as one.
    #[test]
    fn open_count_token_round_trips_and_rejects_prose() {
        assert_eq!(render_open_count_token([0, 0, 0, 0]), None);
        assert_eq!(
            render_open_count_token([1, 0, 0, 1]).as_deref(),
            Some("(1 decision, 1 running)")
        );
        assert_eq!(
            render_open_count_token([2, 3, 1, 4]).as_deref(),
            Some("(2 decisions, 3 actions, 1 blocked, 4 running)")
        );
        for counts in [[1, 0, 0, 1], [2, 3, 1, 4], [0, 1, 0, 0], [0, 0, 12, 0]] {
            let token = render_open_count_token(counts).unwrap();
            let text = format!("{token} work");
            assert_eq!(open_count_token(&text), Some((counts, " work")), "{text}");
            assert_eq!(open_count_token(&token), Some((counts, "")));
        }
        for prose in [
            "(see above) work",
            "(1 decisions) work",
            "(2 decision) work",
            "(0 running) work",
            "(01 running) work",
            "(1 running, 1 decision) work",
            "(1 running, 1 running) work",
            "(1 running,1 blocked) work",
            "(1 running)work",
            "( 1 running) work",
            "() work",
            "(1 running",
        ] {
            assert_eq!(open_count_token(prose), None, "{prose}");
        }
    }

    /// The token is leading projection: identity, claim and dedupe keys read
    /// past it exactly like an owner label, wherever it sits in the prefix.
    #[test]
    fn open_count_token_is_stripped_with_the_projection_prefix() {
        for (counted, bare) in [
            (
                "🙋 (1 decision, 1 running) [from:chat key=x] work [#q-0123456789ab]",
                "🙋 [from:chat key=x] work [#q-0123456789ab]",
            ),
            (
                "🚧 [#subagent:x#1] (2 actions) do [#foo]",
                "🚧 [#subagent:x#1] do [#foo]",
            ),
            ("(1 blocked) do [#foo]: note", "do [#foo]: note"),
        ] {
            assert_eq!(
                strip_attempt_projection_prefix(counted),
                strip_attempt_projection_prefix(bare)
            );
            assert_eq!(
                strip_priority_markers(counted),
                strip_priority_markers(bare)
            );
            assert_eq!(queue_head_identity(counted), queue_head_identity(bare));
            assert_eq!(
                queue_prompt_stable_identity(counted),
                queue_prompt_stable_identity(bare)
            );
            assert_eq!(
                queue_prompt_stable_id(counted),
                queue_prompt_stable_id(bare)
            );
            assert_eq!(
                render_struck_row_body(counted),
                render_struck_row_body(bare)
            );
        }
        // Prose that only looks similar stays work text.
        assert_eq!(
            strip_attempt_projection_prefix("🙋 (see above) work"),
            "(see above) work"
        );
    }
    /// `#followupshorttitle`: `[#origin] follow-up: <item>` names the row it
    /// follows up. It is never the row's own identity, under any projection,
    /// and the `🙋` glyph does not read it as a legacy request-id prefix.
    #[test]
    fn a_followup_origin_reference_is_not_the_rows_identity() {
        let row =
            "[#checkadragainst] follow-up: operator: decide whether to keep it [operator-verify]";
        assert_eq!(
            super::followup_origin_reference(row),
            Some((
                "checkadragainst",
                "operator: decide whether to keep it [operator-verify]"
            ))
        );
        for decorated in [
            row.to_string(),
            format!("🙋 {row}"),
            format!("📥 {row}"),
            format!("🚧 [#subagent:x#1] {row}"),
            row.replace("follow-up:", "Follow-Up:"),
        ] {
            assert!(
                !super::queue_head_identity(&decorated).is_id_backed(),
                "{decorated}"
            );
            // The reference stays in the free-text key, so 🙋 and 📥
            // repaints of one row key the same.
            assert!(
                super::strip_priority_markers(&decorated)
                    .to_ascii_lowercase()
                    .starts_with("[#checkadragainst] follow-up:"),
                "{decorated}"
            );
        }
        // Its own identity is the binary-admitted `[#q-…]`.
        let admitted = format!("🙋 {row} [#q-0123456789ab]");
        assert_eq!(
            super::queue_prompt_stable_id(&admitted).as_deref(),
            Some("q-0123456789ab")
        );
        // A follow-up of a follow-up names its immediate origin's q-id.
        let nested = "[#q-0123456789ab] follow-up: agent: rerun CI";
        assert!(!super::queue_head_identity(nested).is_id_backed());
        assert_eq!(super::queue_prompt_stable_id(nested), None);

        // Other leading ids keep their meaning.
        assert!(super::queue_head_identity("[#checkadragainst] check it").is_id_backed());
        assert_eq!(super::followup_origin_reference("[#a]: follow-up: x"), None);
        assert_eq!(super::followup_origin_reference("[#a] follow-up:"), None);
        assert_eq!(super::followup_origin_reference("[#a] follow-upx: y"), None);
        assert_eq!(
            super::strip_priority_markers("🙋 [#subagent-operator-request-x] decide"),
            "decide"
        );
    }

    #[test]
    fn publication_glyphs_read_in_state_transition_order() {
        assert_eq!(super::QueueReportState::Local.glyph(), "✎");
        assert_eq!(super::QueueReportState::Published.glyph(), "⇧");
        assert_eq!(super::QueueReportState::PublishedLocal.glyph(), "⇧✎");
    }

    #[test]
    fn generated_id_ahead_of_a_list_marker_returns_to_the_trailing_slot() {
        let spliced = " [#q-4cc5f6de6590]- 🟡 [#subagent:orphansub#1] orphaned subitems\n";
        let repaired = super::repair_generated_id_before_list_marker(spliced);
        assert_eq!(
            repaired,
            "- 🟡 [#subagent:orphansub#1] orphaned subitems [#q-4cc5f6de6590]\n"
        );
        assert_eq!(
            super::repair_generated_id_before_list_marker(&repaired),
            repaired
        );
        let indented = "   [#q-4cc5f6de6590]- child\n";
        assert_eq!(
            super::repair_generated_id_before_list_marker(indented),
            "  - child [#q-4cc5f6de6590]\n"
        );
        let trailing = "- 🟡 work [#q-4cc5f6de6590]\n";
        assert_eq!(
            super::repair_generated_id_before_list_marker(trailing),
            trailing
        );
        let prose = "- see [#q-4cc5f6de6590]-like note\n";
        assert_eq!(super::repair_generated_id_before_list_marker(prose), prose);
        let reported = " [#q-4cc5f6de6590]- 🟡 work ⟦local⟧\n";
        assert_eq!(
            super::repair_generated_id_before_list_marker(reported),
            "- 🟡 work [#q-4cc5f6de6590] ⟦local⟧\n"
        );
    }

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

    /// `#adnestedclaimedidrow`: an editor/heading merge duplicated the running
    /// projection around `###`; the repeated prefix is metadata, not work.
    #[test]
    fn repeated_projection_around_heading_token_keeps_the_id_work_identity() {
        let row = concat!(
            "🚧 [#subagent:adnestedclaimedidrow#1] ### ",
            "🚧 [#subagent:adnestedclaimedidrow#1] do [#adnestedclaimedidrow]",
        );
        assert_eq!(
            super::strip_attempt_projection_prefix(row),
            "do [#adnestedclaimedidrow]"
        );
        assert_eq!(
            super::strip_priority_markers(row),
            "do [#adnestedclaimedidrow]"
        );
        assert_eq!(
            super::queue_head_identity(row),
            super::QueueItemIdentity::Id("adnestedclaimedidrow".to_string())
        );
        assert_eq!(
            super::render_struck_row_body(row),
            "~~do [#adnestedclaimedidrow]~~"
        );

        assert_eq!(
            super::strip_attempt_projection_prefix("🚧 [#subagent:x#1] ### authored heading"),
            "### authored heading",
            "an authored heading is not swallowed without a repeated projection"
        );
        assert_eq!(
            super::strip_attempt_projection_prefix(
                "### 🚧 [#subagent:x#1] do [#adnestedclaimedidrow]"
            ),
            "### 🚧 [#subagent:x#1] do [#adnestedclaimedidrow]",
            "a leading authored heading is not projection by itself"
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

    /// `#address [#q-…]` is free text. The q-id correlates the row; the preset
    /// token is not a backlog id, and `q-…` is not a `--done` id either.
    #[test]
    fn preset_collapsed_to_generated_id_is_free_text() {
        assert_eq!(
            queue_prompt_stable_identity("#address [#q-3f7fdc3da31f]"),
            QueueItemIdentity::FreeText("queue-id:q-3f7fdc3da31f".into())
        );
        assert_eq!(
            queue_prompt_stable_identity("#upgrade [#q-d20365e4090d]"),
            QueueItemIdentity::FreeText("queue-id:q-d20365e4090d".into())
        );
        assert_ne!(
            queue_prompt_stable_identity("#address [#q-3f7fdc3da31f]"),
            queue_prompt_stable_identity("#address [#q-096acdc5caca]")
        );
        assert_eq!(
            queue_head_identity("#address [#q-3f7fdc3da31f]"),
            QueueItemIdentity::Id("q-3f7fdc3da31f".into()),
            "the tracked-work reader still sees the bracket; stable identity does not"
        );
        assert_eq!(
            queue_prompt_stable_identity("#review [#adr0043-publish]"),
            QueueItemIdentity::Id("adr0043-publish".into())
        );
        assert_eq!(
            queue_prompt_stable_identity("#address"),
            QueueItemIdentity::Id("address".into())
        );
        assert_eq!(
            queue_prompt_stable_identity("do [#q-authored]"),
            QueueItemIdentity::Id("q-authored".into())
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

    /// `#struckidtrail`: the id at the end of a strike is the row's id, and a
    /// copy of it appended after the closing `~~` folds back.
    #[test]
    fn struck_generated_id_reads_inside_the_strike_and_folds_a_trailing_copy() {
        let struck = "- ### ~~check CI run 1001 once it finishes. [#q-0a1b2c3d4e5f]~~";
        assert_eq!(struck_generated_prompt_id(struck), Some("q-0a1b2c3d4e5f"));
        assert_eq!(
            struck_generated_prompt_id("- check it [#q-0a1b2c3d4e5f]"),
            None
        );
        assert_eq!(struck_generated_prompt_id("- ~~check it~~"), None);
        assert_eq!(
            struck_generated_prompt_id("- ~~check [#authored-id]~~"),
            None,
            "only a generated q-id"
        );
        let doubled = format!("{struck} [#q-0a1b2c3d4e5f]");
        assert_eq!(
            fold_struck_row_trailing_id(&doubled).as_deref(),
            Some(struck)
        );
        let tripled = format!("{struck} [#q-0a1b2c3d4e5f] [#Q-0A1B2C3D4E5F]  ");
        assert_eq!(
            fold_struck_row_trailing_id(&tripled).as_deref(),
            Some(struck)
        );
        assert_eq!(fold_struck_row_trailing_id(struck), None);
        assert_eq!(
            fold_struck_row_trailing_id(&format!("{struck} [#q-ffffffffffff]")),
            None,
            "a different id is not a copy"
        );
        assert_eq!(
            fold_struck_row_trailing_id(&format!("{struck} note [#q-0a1b2c3d4e5f]")),
            None,
            "other trailing text is never folded"
        );
        assert_eq!(
            fold_struck_row_trailing_id("- check it [#q-0a1b2c3d4e5f] [#q-0a1b2c3d4e5f]"),
            None,
            "a live row is not touched"
        );
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

    #[test]
    fn report_state_annotations_are_typed_and_not_part_of_work_identity() {
        for state in QueueReportState::ALL {
            let rendered = render_queue_report_state_annotation("do [#ship]", Some(state));
            assert_eq!(queue_report_state_annotation(&rendered).unwrap().1, state);
            assert_eq!(
                queue_head_identity(&rendered),
                queue_head_identity("do [#ship]")
            );
            assert_eq!(
                strip_priority_markers(&format!("worker work {}", state.annotation())),
                "worker work"
            );
        }
        assert_eq!(
            strip_queue_report_state_annotation("explain ⟦local⟧ in prose"),
            "explain ⟦local⟧ in prose"
        );
        assert_eq!(
            render_queue_report_state_annotation(
                "parent\n  - child",
                Some(QueueReportState::Local)
            ),
            "parent ⟦local⟧\n  - child"
        );
    }

    #[test]
    fn stable_prompt_id_stripping_preserves_authored_text_and_other_annotations() {
        assert_eq!(
            strip_queue_prompt_stable_id_annotation("ship it [#q-abc123]"),
            "ship it"
        );
        assert_eq!(
            strip_queue_prompt_stable_id_annotation("ship it [#q-abc123] ⟦published⟧\n  - context"),
            "ship it ⟦published⟧\n  - context"
        );
        assert_eq!(
            strip_queue_prompt_stable_id_annotation("do [#authored]"),
            "do [#authored]"
        );
        assert_eq!(
            strip_queue_prompt_stable_id_annotation("do [#q-authored]"),
            "do [#q-authored]"
        );
        assert_eq!(
            queue_prompt_stable_id("ship it [#q-abc123] ⟦effect:2⟧ ⟦published⟧"),
            Some("q-abc123".to_string())
        );
        assert_eq!(
            strip_queue_prompt_stable_id_annotation("ship it [#q-abc123] ⟦effect:2⟧ ⟦published⟧"),
            "ship it ⟦effect:2⟧ ⟦published⟧"
        );
    }

    /// `#lifecyclewordfusion`: haiven-dev sdk-dev.md 2026-11-03 struck the
    /// row as `~~running🚧 do [#q-c2010d98af04]~~`. The fused state word is a
    /// projection token: identity, priority stripping and the strike body all
    /// read the authored `do [#q-c2010d98af04]`.
    #[test]
    fn fused_state_word_is_projection_for_identity_and_strikes() {
        let fused = "📥 running🚧 do [#q-c2010d98af04]";
        assert_eq!(strip_priority_markers(fused), "do [#q-c2010d98af04]");
        assert_eq!(
            strip_attempt_projection_prefix(fused),
            "do [#q-c2010d98af04]"
        );
        assert_eq!(struck_work_text(fused), "do [#q-c2010d98af04]");
        assert_eq!(render_struck_row_body(fused), "~~do [#q-c2010d98af04]~~");
        assert_eq!(
            queue_head_identity(fused),
            queue_head_identity("do [#q-c2010d98af04]")
        );
        assert_eq!(
            QueueItemIdentity::from_prompt("running🚧 fix the login flow"),
            QueueItemIdentity::from_prompt("fix the login flow")
        );
        assert_eq!(
            strip_priority_markers("running tests on main"),
            "running tests on main"
        );
    }

    /// `#lifecyclewordowner`: agent-loop agent-doc-bugs.md 2026-11-21 carried
    /// `🟡 running [#subagent:lzeffectvoid#1] amend core-v1` and, after
    /// closeout, `~~running [#subagent:lzeffectvoid#1] amend core-v1~~`. The
    /// state word plus owner annotation is projection: identity, priority
    /// stripping and the strike body all read the authored `amend core-v1`.
    #[test]
    fn state_word_before_owner_annotation_is_projection_for_identity_and_strikes() {
        let leaked = "🟡 running [#subagent:lzeffectvoid#1] amend core-v1";
        assert_eq!(strip_priority_markers(leaked), "amend core-v1");
        assert_eq!(strip_attempt_projection_prefix(leaked), "amend core-v1");
        assert_eq!(struck_work_text(leaked), "amend core-v1");
        assert_eq!(render_struck_row_body(leaked), "~~amend core-v1~~");
        assert_eq!(
            queue_head_identity(leaked),
            queue_head_identity("amend core-v1")
        );
        assert_eq!(
            queue_head_identity(
                "🚧 [#subagent:lzeffectvoid#1] running [#subagent:lzeffectvoid#1] amend core-v1"
            ),
            queue_head_identity("amend core-v1")
        );
        assert_eq!(
            QueueItemIdentity::from_prompt("running [#subagent:lzeffectvoid#1] amend core-v1"),
            QueueItemIdentity::from_prompt("amend core-v1")
        );
        assert_eq!(
            strip_priority_markers("running [#fix-parser] tests"),
            "running [#fix-parser] tests"
        );
    }
}
