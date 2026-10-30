//! Pure queue schedule and completed-row retention policy (`interval=`,
//! `time=`, `loop`, `keep`).
//!
//! `<!-- agent:loop interval=30m time=09:00,17:30 -->` declares a repeating
//! scheduled run. Legacy `agent:queue loop` / `loop=N` markers remain accepted.
//! Each queue's schedule is independent:
//! its identity, durable state, open window, and loop roster are keyed by the
//! canonical `(document, queue_id)` pair. A scheduled queue does not drain the
//! moment it has a live head; it drains only inside a **window** the project
//! controller's single scheduler loop opens at each due tick. The drain itself
//! is the ordinary one (supervisor idle watch, in-session loop, subagent
//! fan-out): this module only decides *when* a window opens, when the pass it
//! carries is complete, and which heads a closed window holds back.
//!
//! - `interval=<dur>` — the poll interval (`90`, `30s`, `5m`, `1h30m`, `1d`).
//!   Values below [`MIN_SCHEDULE_INTERVAL_SECS`] are clamped to that floor.
//! - `time=HH:MM[,HH:MM…]` — times of day in the controller's local time zone.
//!   `interval` and `time` combine: the next tick is the earliest of either.
//! - `loop` / `loop=N` — the items are a program, not a to-do list: each pass
//!   runs them through the normal drain, then the controller restores them to
//!   the queue so they are still there for the next pass. Bare `loop` repeats
//!   without limit; `loop=N` runs N passes and then holds the items. A `loop`
//!   without `interval`/`time` starts the next pass
//!   [`MIN_SCHEDULE_INTERVAL_SECS`] after the previous one completed.
//! - `keep` — completed rows remain struck in the queue as history; unlike
//!   `loop`, this does not schedule or restore them.
//!
//! # Anti-thrash contract
//!
//! - Ticks are **coalesced**: when the controller wakes after a missed tick
//!   (suspend, hot update, a long pass), it fires the *latest* deadline at or
//!   before now exactly once and schedules the next one strictly after now —
//!   never a catch-up burst.
//! - A tick is fired **at most once**: [`QueueScheduleState::last_fired_due_secs`]
//!   is durable and [`fire`] refuses a deadline at or before it, so a restarted
//!   or hand-over controller cannot refire it.
//! - A pass's number is fixed when its window opens; completing it sets
//!   `passes_completed = max(passes_completed, pass)`, so replaying a completion
//!   never double-counts.
//! - A failed tick backs off exponentially ([`record_failure`]), capped at
//!   [`MAX_SCHEDULE_BACKOFF_SECS`].
//!
//! This module performs no IO and reads no clock: callers pass `now` and a
//! [`LocalOffset`] in.

use std::collections::HashMap;

use agent_doc_element::element;
use agent_doc_element_queue::QueueItemIdentity;
use serde::{Deserialize, Serialize};

use crate::document_queue::{self, QueueEntry};
use crate::queue_graph::QueueGraph;

/// Queue-marker attribute: the poll interval.
pub const QUEUE_INTERVAL_ATTR: &str = "interval";
/// Queue-marker attribute: comma-separated local times of day.
pub const QUEUE_TIME_ATTR: &str = "time";
/// Queue-marker attribute: keep items and re-run them every pass.
pub const QUEUE_LOOP_ATTR: &str = "loop";
/// Queue-marker attribute: retain completed items in the runnable list.
pub const QUEUE_KEEP_ATTR: &str = "keep";

/// The shortest cadence a schedule may run at. A smaller `interval=` is
/// clamped up to this floor (with a preflight warning), and a cadence-less
/// `loop` waits this long between passes.
pub const MIN_SCHEDULE_INTERVAL_SECS: u64 = 60;
/// First retry delay after a failed tick.
pub const BASE_SCHEDULE_BACKOFF_SECS: u64 = 30;
/// Longest retry delay after repeated failed ticks.
pub const MAX_SCHEDULE_BACKOFF_SECS: u64 = 3_600;

const SECS_PER_DAY: i64 = 86_400;

/// How many passes a `loop` queue runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "passes")]
pub enum LoopLimit {
    /// No `loop` attribute: items are consumed as usual.
    Off,
    /// Bare `loop`: repeat without limit.
    Unbounded,
    /// `loop=N`: run exactly N passes.
    Passes(u32),
}

impl LoopLimit {
    pub fn is_loop(self) -> bool {
        !matches!(self, LoopLimit::Off)
    }

    /// Whether `passes_completed` passes exhaust this limit.
    pub fn exhausted_after(self, passes_completed: u32) -> bool {
        matches!(self, LoopLimit::Passes(limit) if passes_completed >= limit)
    }
}

/// A local time of day, minutes after local midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimeOfDay {
    pub minute_of_day: u16,
}

impl TimeOfDay {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let (hours, minutes) = raw
            .split_once(':')
            .ok_or_else(|| format!("`{raw}` is not HH:MM"))?;
        let hours: u16 = hours
            .trim()
            .parse()
            .map_err(|_| format!("`{raw}` is not HH:MM"))?;
        let minutes: u16 = minutes
            .trim()
            .parse()
            .map_err(|_| format!("`{raw}` is not HH:MM"))?;
        if hours > 23 || minutes > 59 {
            return Err(format!("`{raw}` is not a time of day (00:00-23:59)"));
        }
        Ok(Self {
            minute_of_day: hours * 60 + minutes,
        })
    }

    fn offset_secs(self) -> i64 {
        i64::from(self.minute_of_day) * 60
    }

    pub fn render(self) -> String {
        format!(
            "{:02}:{:02}",
            self.minute_of_day / 60,
            self.minute_of_day % 60
        )
    }
}

/// A parsed queue schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueScheduleSpec {
    /// Poll interval, already clamped to [`MIN_SCHEDULE_INTERVAL_SECS`].
    pub interval_secs: Option<u64>,
    /// Sorted, deduplicated local times of day.
    pub times: Vec<TimeOfDay>,
    pub loop_limit: LoopLimit,
}

impl QueueScheduleSpec {
    /// The schedule has a cadence of its own (`interval`/`time`), as opposed to
    /// a bare `loop` that paces passes by completion.
    pub fn has_cadence(&self) -> bool {
        self.interval_secs.is_some() || !self.times.is_empty()
    }

    /// Identity of the cadence. A change re-anchors the schedule; a change to
    /// `loop` alone keeps the anchor and the pass count.
    pub fn cadence_fingerprint(&self) -> String {
        let times = self
            .times
            .iter()
            .map(|time| time.render())
            .collect::<Vec<_>>()
            .join(",");
        match self.interval_secs {
            Some(interval) => format!("interval={interval};time={times}"),
            None => format!("interval=-;time={times}"),
        }
    }
}

/// A queue-schedule attribute problem preflight reports as a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueScheduleAttrIssue {
    pub attr: &'static str,
    pub message: String,
    /// Invalid syntax is fail-closed for this queue. A clamped-but-valid value
    /// is only advisory and therefore leaves the schedule runnable.
    pub fatal: bool,
}

/// Parse a duration: a bare number of seconds, or `<n>d`, `<n>h`, `<n>m`,
/// `<n>s` segments (`1h30m`).
pub fn parse_duration_secs(raw: &str) -> Result<u64, String> {
    let raw = raw.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    if raw.is_empty() {
        return Err("empty duration".to_string());
    }
    if let Ok(secs) = raw.parse::<u64>() {
        return Ok(secs);
    }
    let mut total: u64 = 0;
    let mut digits = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        let unit = match ch.to_ascii_lowercase() {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => {
                return Err(format!(
                    "`{raw}` is not a duration (use e.g. 90, 30s, 5m, 1h30m, 1d)"
                ));
            }
        };
        let value: u64 = digits
            .parse()
            .map_err(|_| format!("`{raw}` is not a duration (use e.g. 90, 30s, 5m, 1h30m, 1d)"))?;
        digits.clear();
        total = total
            .checked_add(value.saturating_mul(unit))
            .ok_or_else(|| format!("`{raw}` is too long"))?;
    }
    if !digits.is_empty() {
        return Err(format!(
            "`{raw}` mixes a unit-less number into a duration (use e.g. 1h30m)"
        ));
    }
    Ok(total)
}

fn attr_value(raw: &str) -> &str {
    raw.trim().trim_matches(|ch| ch == '"' || ch == '\'')
}

/// The schedule declared by an `agent:queue` marker's attributes, plus every
/// attribute problem found. `None` when no schedule attribute is present.
///
/// Invalid values are omitted from the partial spec and reported as fatal.
/// Queue-level callers use those fatal issues to hold that queue fail-closed;
/// valid sibling queues remain runnable. A below-floor interval is clamped and
/// reported as a non-fatal advisory.
pub fn queue_schedule_spec_for_attrs(
    attrs: &HashMap<String, String>,
) -> (Option<QueueScheduleSpec>, Vec<QueueScheduleAttrIssue>) {
    let mut issues = Vec::new();
    let mut interval_secs = None;
    if let Some(raw) = attrs.get(QUEUE_INTERVAL_ATTR) {
        match parse_duration_secs(raw) {
            Ok(0) => issues.push(QueueScheduleAttrIssue {
                attr: QUEUE_INTERVAL_ATTR,
                message: "`interval` must be positive".to_string(),
                fatal: true,
            }),
            Ok(secs) if secs < MIN_SCHEDULE_INTERVAL_SECS => {
                issues.push(QueueScheduleAttrIssue {
                    attr: QUEUE_INTERVAL_ATTR,
                    message: format!(
                        "`interval={}` is below the {MIN_SCHEDULE_INTERVAL_SECS}s floor; using {MIN_SCHEDULE_INTERVAL_SECS}s",
                        attr_value(raw)
                    ),
                    fatal: false,
                });
                interval_secs = Some(MIN_SCHEDULE_INTERVAL_SECS);
            }
            Ok(secs) => interval_secs = Some(secs),
            Err(message) => issues.push(QueueScheduleAttrIssue {
                attr: QUEUE_INTERVAL_ATTR,
                message,
                fatal: true,
            }),
        }
    }
    let mut times = Vec::new();
    if let Some(raw) = attrs.get(QUEUE_TIME_ATTR) {
        let raw = attr_value(raw);
        if raw.is_empty() {
            issues.push(QueueScheduleAttrIssue {
                attr: QUEUE_TIME_ATTR,
                message: "`time` needs HH:MM[,HH:MM...]".to_string(),
                fatal: true,
            });
        }
        for part in raw.split(',').filter(|part| !part.trim().is_empty()) {
            match TimeOfDay::parse(part) {
                Ok(time) => times.push(time),
                Err(message) => issues.push(QueueScheduleAttrIssue {
                    attr: QUEUE_TIME_ATTR,
                    message,
                    fatal: true,
                }),
            }
        }
        times.sort();
        times.dedup();
    }
    let mut loop_limit = LoopLimit::Off;
    if let Some(raw) = attrs.get(QUEUE_LOOP_ATTR) {
        let raw = attr_value(raw);
        if raw.is_empty() || matches!(raw.to_ascii_lowercase().as_str(), "true" | "yes" | "on") {
            loop_limit = LoopLimit::Unbounded;
        } else {
            match raw.parse::<u32>() {
                Ok(0) | Err(_) => issues.push(QueueScheduleAttrIssue {
                    attr: QUEUE_LOOP_ATTR,
                    message: format!(
                        "`loop={raw}` is not a positive pass count (use a bare `loop` to repeat without limit)"
                    ),
                    fatal: true,
                }),
                Ok(passes) => loop_limit = LoopLimit::Passes(passes),
            }
        }
    }
    let spec = QueueScheduleSpec {
        interval_secs,
        times,
        loop_limit,
    };
    if spec.has_cadence() || spec.loop_limit.is_loop() {
        (Some(spec), issues)
    } else {
        (None, issues)
    }
}

/// Whether a drain must keep this queue's struck (`~~item~~`) rows instead of
/// erasing the body when its last live head is consumed.
///
/// Bare `keep` retains completed rows as queue history without making them
/// live again. A `loop` queue implies the same retention because its struck
/// rows are the pass's completion evidence: the
/// scheduler's roster reconciliation ([`observe_with_completed`]) keeps a
/// roster item only while it is live, was seen claimed, or is struck-complete,
/// and the loop restore ([`plan_loop_restore_for_queue`]) is what removes the
/// struck copy. A drain that erases the body when the queue empties (the
/// closeout consume, the preflight drained-queue clear) would leave the item
/// neither live nor struck, so the pass would complete with nothing to
/// restore. Legacy `agent:run loop` / `agent:queue loop` markers retain via the
/// `loop` attribute even when they predate the explicit `keep` projection. Both
/// flags are presence-based, so a malformed value still takes the non-lossy
/// direction.
pub fn queue_retains_struck_heads(attrs: &HashMap<String, String>) -> bool {
    attrs.contains_key(QUEUE_KEEP_ATTR) || attrs.contains_key(QUEUE_LOOP_ATTR)
}

fn declares_schedule(attrs: &HashMap<String, String>) -> bool {
    [QUEUE_INTERVAL_ATTR, QUEUE_TIME_ATTR, QUEUE_LOOP_ATTR]
        .iter()
        .any(|attr| attrs.contains_key(*attr))
}

/// One queue-local schedule declaration. `spec == None` with non-empty issues
/// is an invalid declaration: that queue is held fail-closed, without affecting
/// any sibling queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifiedQueueSchedule {
    pub queue_id: String,
    pub occurrence: usize,
    pub spec: Option<QueueScheduleSpec>,
    pub issues: Vec<QueueScheduleAttrIssue>,
}

impl IdentifiedQueueSchedule {
    pub fn is_valid(&self) -> bool {
        self.spec.is_some() && !self.issues.iter().any(|issue| issue.fatal)
    }
}

/// Every schedule declaration in document order, identified by the same
/// canonical queue IDs used by dependency scheduling. Multiple queues must
/// therefore already have explicit or binary-backfilled IDs.
pub fn identified_queue_schedules(content: &str) -> anyhow::Result<Vec<IdentifiedQueueSchedule>> {
    if !content.contains("agent:queue")
        && !content.contains("agent:run")
        && !content.contains("agent:loop")
    {
        return Ok(Vec::new());
    }
    let components = element::parse(content)?;
    let graph = QueueGraph::parse(content, &components).map_err(anyhow::Error::new)?;
    Ok(graph
        .blocks()
        .iter()
        .filter(|block| declares_schedule(&block.component.attrs))
        .map(|block| {
            let (spec, issues) = queue_schedule_spec_for_attrs(&block.component.attrs);
            IdentifiedQueueSchedule {
                queue_id: block.id.clone(),
                occurrence: block.occurrence,
                spec,
                issues,
            }
        })
        .collect())
}

fn queue_block<'a>(
    content: &str,
    components: &'a [element::Component],
    queue_id: &str,
) -> anyhow::Result<&'a element::Component> {
    QueueGraph::parse(content, components)
        .map_err(anyhow::Error::new)?
        .block(queue_id)
        .map(|block| block.component)
        .ok_or_else(|| anyhow::anyhow!("agent:queue id `{queue_id}` not found"))
}

pub fn live_heads_for_queue(content: &str, queue_id: &str) -> anyhow::Result<Vec<String>> {
    let components = element::parse(content)?;
    let graph = QueueGraph::parse(content, &components).map_err(anyhow::Error::new)?;
    let block = graph
        .block(queue_id)
        .ok_or_else(|| anyhow::anyhow!("agent:queue id `{queue_id}` not found"))?;
    let executable = document_queue::executable_prompt_indexes(&block.entries);
    Ok(block
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match entry {
            QueueEntry::Prompt(prompt) if executable.contains(&index) => Some(prompt.text.clone()),
            QueueEntry::Freeform(line) if !document_queue::is_noise_freeform_line(line) => {
                Some(line.clone())
            }
            _ => None,
        })
        .collect())
}

/// Completed head texts retained in one queue body. The scheduler uses this
/// proof to distinguish a head that ran from a transient editor draft that was
/// replaced before dispatch.
pub fn completed_heads_for_queue(content: &str, queue_id: &str) -> anyhow::Result<Vec<String>> {
    let components = element::parse(content)?;
    let graph = QueueGraph::parse(content, &components).map_err(anyhow::Error::new)?;
    let block = graph
        .block(queue_id)
        .ok_or_else(|| anyhow::anyhow!("agent:queue id `{queue_id}` not found"))?;
    Ok(block
        .entries
        .iter()
        .filter_map(|entry| match entry {
            QueueEntry::Completed(prompt) => Some(prompt.text.clone()),
            _ => None,
        })
        .collect())
}

pub fn queue_held_by_control_for_queue(content: &str, queue_id: &str) -> anyhow::Result<bool> {
    let components = element::parse(content)?;
    let queue = queue_block(content, &components, queue_id)?;
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .map(|(frontmatter, _)| frontmatter);
    let control = frontmatter
        .as_ref()
        .and_then(crate::control_binding::frontmatter_queue_control);
    Ok(!crate::control_binding::queue_control_activation(
        &queue.attrs,
        control,
    ))
}

/// The `agent:queue` block that carries the document's schedule: the first
/// queue block (document order) declaring `interval`, `time`, or `loop`, else
/// the first queue block.
///
/// Deliberately NOT the *selected* queue: selection skips an empty queue, and
/// a scheduled queue is empty between passes (every item consumed, or a
/// `loop` pass waiting for its restore). Reading the schedule from the
/// selected block would unschedule the document — and drop its durable pass
/// count — every time a pass drained it.
pub fn scheduled_queue_component(components: &[element::Component]) -> Option<&element::Component> {
    let mut queues = components
        .iter()
        .filter(|component| component.name == "queue");
    let first = queues.clone().next()?;
    Some(
        queues
            .find(|component| declares_schedule(&component.attrs))
            .unwrap_or(first),
    )
}

/// The schedule of the document's `agent:queue`, if any (see
/// [`scheduled_queue_component`]).
pub fn queue_schedule_spec(content: &str) -> Option<QueueScheduleSpec> {
    identified_queue_schedules(content)
        .ok()?
        .into_iter()
        .next()
        .filter(IdentifiedQueueSchedule::is_valid)?
        .spec
}

/// Whether the document's queue is scheduled.
pub fn content_is_scheduled(content: &str) -> bool {
    identified_queue_schedules(content).is_ok_and(|schedules| !schedules.is_empty())
}

/// Whether the queue's control holds it (`pause`, legacy `stop`, or a legacy
/// inert `queue_active:` flag): a scheduled tick then skips instead of
/// opening a window. No control at all is `go` (`#queuegodefault`).
pub fn queue_held_by_control(content: &str) -> bool {
    identified_queue_schedules(content)
        .ok()
        .and_then(|schedules| schedules.into_iter().next())
        .is_none_or(|schedule| {
            queue_held_by_control_for_queue(content, &schedule.queue_id).unwrap_or(true)
        })
}

/// Live (unstruck, executable) head texts of the selected queue, in order.
pub fn live_heads(content: &str) -> Vec<String> {
    identified_queue_schedules(content)
        .ok()
        .and_then(|schedules| schedules.into_iter().next())
        .and_then(|schedule| live_heads_for_queue(content, &schedule.queue_id).ok())
        .unwrap_or_default()
}

fn identity(text: &str) -> QueueItemIdentity {
    crate::queue_claim::claim_identity(text)
}

/// Lifecycle projections only the drain writes onto a live head: claimed /
/// in progress (`🚧`), fenced subagent attempt (`🟡`, `☑️`, `⚠️`, `⏭️`), and
/// awaiting operator (`⏳`). Pin markers (`📌`, `**pin**`, ...) are operator
/// priority, not drain evidence, and are skipped over.
/// GH #327: whether a live head carries a drain lifecycle marker, i.e. the
/// pass's drain already picked it up. An operator keystroke state never does.
fn head_carries_drain_lifecycle_marker(text: &str) -> bool {
    use agent_doc_document::queue_projection::QueueLifecycleMarker;

    matches!(
        agent_doc_document::queue_projection::lifecycle_marker(text),
        Some(
            QueueLifecycleMarker::Claimed
                | QueueLifecycleMarker::Running
                | QueueLifecycleMarker::Reported
                | QueueLifecycleMarker::Lost
                | QueueLifecycleMarker::Unknown
                | QueueLifecycleMarker::Skipped
                | QueueLifecycleMarker::AwaitingOperator
        )
    ) || !agent_doc_document::queue_projection::operator_action_ids(text).is_empty()
}

/// The local zone's UTC offset at an instant. Injected so the schedule math is
/// deterministic; the controller supplies the platform zone.
pub trait LocalOffset {
    /// Seconds to add to a UTC epoch instant to get local wall-clock seconds.
    fn utc_offset_secs(&self, epoch_secs: i64) -> i64;
}

/// A fixed UTC offset (tests, and platforms without a zone database).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedOffset(pub i64);

impl LocalOffset for FixedOffset {
    fn utc_offset_secs(&self, _epoch_secs: i64) -> i64 {
        self.0
    }
}

/// The UTC instant of local wall-clock `local_secs`, using the offset in
/// effect at that instant (one refinement step covers a DST boundary).
fn local_to_utc(local_secs: i64, tz: &dyn LocalOffset) -> i64 {
    let guess = local_secs - tz.utc_offset_secs(local_secs);
    local_secs - tz.utc_offset_secs(guess)
}

fn local_midnight(utc: i64, tz: &dyn LocalOffset) -> i64 {
    let local = utc + tz.utc_offset_secs(utc);
    local.div_euclid(SECS_PER_DAY) * SECS_PER_DAY
}

/// The first time-of-day instant strictly after `after`.
fn next_time_of_day_after(after: u64, times: &[TimeOfDay], tz: &dyn LocalOffset) -> Option<u64> {
    let after = i64::try_from(after).ok()?;
    let midnight = local_midnight(after, tz);
    (0..=2)
        .flat_map(|day| {
            times.iter().map(move |time| {
                local_to_utc(midnight + day * SECS_PER_DAY + time.offset_secs(), tz)
            })
        })
        .filter(|candidate| *candidate > after)
        .min()
        .and_then(|candidate| u64::try_from(candidate).ok())
}

/// The latest time-of-day instant at or before `at`.
fn latest_time_of_day_at_or_before(
    at: u64,
    times: &[TimeOfDay],
    tz: &dyn LocalOffset,
) -> Option<u64> {
    let at = i64::try_from(at).ok()?;
    let midnight = local_midnight(at, tz);
    (-2..=0)
        .flat_map(|day| {
            times.iter().map(move |time| {
                local_to_utc(midnight + day * SECS_PER_DAY + time.offset_secs(), tz)
            })
        })
        .filter(|candidate| *candidate <= at)
        .max()
        .and_then(|candidate| u64::try_from(candidate).ok())
}

/// Whether a pass is running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ScheduleWindow {
    /// No pass is running; a scheduled queue holds every head.
    #[default]
    Closed,
    /// Pass `pass` is draining. `roster` is every head the pass owns: the live
    /// heads when it opened plus any head that appeared while it ran. `ran`
    /// is the subset of the roster the drain has been observed acting on
    /// (seen claimed or struck-complete); that evidence is sticky for the
    /// rest of the pass, so a drain that later erases the row cannot make
    /// the item look like it never ran (GH #327).
    Open {
        pass: u32,
        opened_secs: u64,
        roster: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        ran: Vec<String>,
    },
}

/// Durable per-`(document, queue_id)` schedule progress
/// (`queue_document_state` kind [`QUEUE_SCHEDULE_STATE_KIND`] for the legacy
/// default queue, or `queue_schedule:<queue_id>` for an identified queue).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueScheduleState {
    /// Queue identity that proved ownership of this state. Older document-wide
    /// rows omit it; the controller binds them only when the successor queue is
    /// unambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_queue_id: Option<String>,
    /// [`QueueScheduleSpec::cadence_fingerprint`] the anchor belongs to.
    pub cadence: String,
    /// When the current cadence was first observed. Interval ticks are
    /// `anchor + k * interval`.
    pub anchor_secs: u64,
    /// The latest deadline already fired. A deadline at or before it is never
    /// fired again.
    #[serde(default)]
    pub last_fired_due_secs: Option<u64>,
    #[serde(default)]
    pub last_fired_at_secs: Option<u64>,
    #[serde(default)]
    pub passes_completed: u32,
    #[serde(default)]
    pub last_pass_completed_secs: Option<u64>,
    #[serde(default)]
    pub window: ScheduleWindow,
    /// A completed `loop` pass whose items are not yet back in the queue.
    #[serde(default)]
    pub restore_pending: Option<Vec<String>>,
    /// Ticks folded into a later one because the controller woke late.
    #[serde(default)]
    pub coalesced_ticks: u64,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub backoff_until_secs: Option<u64>,
}

/// `queue_document_state.state_kind` of the durable schedule row.
pub const QUEUE_SCHEDULE_STATE_KIND: &str = "queue_schedule";

impl QueueScheduleState {
    pub fn new(spec: &QueueScheduleSpec, now: u64) -> Self {
        Self {
            owner_queue_id: None,
            cadence: spec.cadence_fingerprint(),
            anchor_secs: now,
            last_fired_due_secs: None,
            last_fired_at_secs: None,
            passes_completed: 0,
            last_pass_completed_secs: None,
            window: ScheduleWindow::Closed,
            restore_pending: None,
            coalesced_ticks: 0,
            failures: 0,
            backoff_until_secs: None,
        }
    }

    pub fn window_open(&self) -> bool {
        matches!(self.window, ScheduleWindow::Open { .. })
    }

    pub fn current_pass(&self) -> Option<u32> {
        match self.window {
            ScheduleWindow::Open { pass, .. } => Some(pass),
            ScheduleWindow::Closed => None,
        }
    }
}

/// Bring `state` in line with `spec`: create it, or re-anchor it when the
/// cadence changed. A changed cadence counts as already fired at `now`, so
/// editing `interval=` reschedules from now instead of running immediately;
/// a brand-new schedule has never fired and runs its first tick at once.
/// The pass count, window, and pending restore survive every edit.
pub fn reconcile(
    state: Option<QueueScheduleState>,
    spec: &QueueScheduleSpec,
    now: u64,
) -> QueueScheduleState {
    let Some(mut state) = state else {
        return QueueScheduleState::new(spec, now);
    };
    let cadence = spec.cadence_fingerprint();
    if state.cadence != cadence {
        state.cadence = cadence;
        state.anchor_secs = now;
        state.last_fired_due_secs = Some(now);
    }
    state
}

/// The next tick of the cadence, ignoring backoff, windows, and loop limits.
///
/// - interval ticks are `anchor + k * interval`, the first at the anchor;
/// - time-of-day ticks are the configured local times;
/// - a cadence-less `loop` ticks [`MIN_SCHEDULE_INTERVAL_SECS`] after the last
///   pass completed (immediately for the first pass).
pub fn next_deadline(
    state: &QueueScheduleState,
    spec: &QueueScheduleSpec,
    tz: &dyn LocalOffset,
) -> Option<u64> {
    let fired = state.last_fired_due_secs;
    if !spec.has_cadence() {
        if !spec.loop_limit.is_loop() {
            return None;
        }
        return Some(match state.last_pass_completed_secs {
            Some(completed) => completed.saturating_add(MIN_SCHEDULE_INTERVAL_SECS),
            None => state.anchor_secs,
        })
        .map(|deadline| fired.map_or(deadline, |fired| deadline.max(fired.saturating_add(1))));
    }
    let interval_next = spec.interval_secs.map(|interval| match fired {
        None => state.anchor_secs,
        Some(fired) if fired < state.anchor_secs => state.anchor_secs,
        Some(fired) => {
            let steps = (fired - state.anchor_secs) / interval + 1;
            state
                .anchor_secs
                .saturating_add(steps.saturating_mul(interval))
        }
    });
    let times_next = (!spec.times.is_empty())
        .then(|| {
            let after = fired.unwrap_or_else(|| state.anchor_secs.saturating_sub(1));
            next_time_of_day_after(after, &spec.times, tz)
        })
        .flatten();
    match (interval_next, times_next) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The deadline a tick at `now` fires: the latest cadence deadline at or
/// before `now` (coalescing every missed one), never earlier than `next`.
fn coalesced_due(
    state: &QueueScheduleState,
    spec: &QueueScheduleSpec,
    next: u64,
    now: u64,
    tz: &dyn LocalOffset,
) -> u64 {
    let interval_latest = spec.interval_secs.and_then(|interval| {
        (now >= state.anchor_secs)
            .then(|| state.anchor_secs + (now - state.anchor_secs) / interval * interval)
    });
    let times_latest = (!spec.times.is_empty())
        .then(|| latest_time_of_day_at_or_before(now, &spec.times, tz))
        .flatten();
    [Some(next), interval_latest, times_latest]
        .into_iter()
        .flatten()
        .filter(|deadline| *deadline <= now)
        .max()
        .unwrap_or(next)
}

/// When the scheduler must next wake for this document, or `None` when
/// nothing will ever be due (an exhausted loop with nothing to restore).
pub fn wake_deadline(
    state: &QueueScheduleState,
    spec: &QueueScheduleSpec,
    tz: &dyn LocalOffset,
) -> Option<u64> {
    let backoff = state.backoff_until_secs.unwrap_or(0);
    if state.restore_pending.is_some() {
        return Some(backoff);
    }
    if spec.loop_limit.exhausted_after(state.passes_completed) {
        return None;
    }
    next_deadline(state, spec, tz).map(|deadline| deadline.max(backoff))
}

/// What one tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FireDecision {
    /// Nothing was due (a stale wake, or another controller fired it first).
    NotDue,
    /// A pass window opened; the ordinary drain may now dispatch the heads.
    Opened,
    /// An idle loop recovered its struck roster and opened a new pass.
    Recovered,
    /// The tick found no live head; the window stays closed.
    EmptyPoll,
    /// The queue is held (`pause`/`stop`); the tick is skipped, not deferred.
    SkippedPaused,
    /// The previous pass is still running; the tick is folded into it.
    SkippedBusy,
    /// `loop=N` already ran N passes.
    Exhausted,
}

/// Facts the tick reads from the document at fire time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FireFacts {
    pub live_heads: Vec<String>,
    /// Completed rows still present in this queue.
    pub completed_heads: Vec<String>,
    /// The queue control resolves to a hold (`pause`, legacy `stop`).
    pub paused: bool,
}

/// Fire the tick due at `now`, if any. Exactly-once: a deadline at or before
/// [`QueueScheduleState::last_fired_due_secs`] is never fired again, and every
/// missed deadline is coalesced into one.
pub fn fire(
    mut state: QueueScheduleState,
    spec: &QueueScheduleSpec,
    now: u64,
    facts: &FireFacts,
    tz: &dyn LocalOffset,
) -> (QueueScheduleState, FireDecision) {
    if state.backoff_until_secs.is_some_and(|until| now < until) {
        return (state, FireDecision::NotDue);
    }
    if spec.loop_limit.exhausted_after(state.passes_completed) {
        return (state, FireDecision::Exhausted);
    }
    let Some(next) = next_deadline(&state, spec, tz) else {
        return (state, FireDecision::NotDue);
    };
    if next > now {
        return (state, FireDecision::NotDue);
    }
    let due = coalesced_due(&state, spec, next, now, tz);
    if due != next {
        state.coalesced_ticks = state.coalesced_ticks.saturating_add(1);
    }
    state.last_fired_due_secs = Some(due);
    state.last_fired_at_secs = Some(now);
    let decision = if facts.paused {
        FireDecision::SkippedPaused
    } else if state.window_open() {
        FireDecision::SkippedBusy
    } else if facts.live_heads.is_empty()
        && spec.loop_limit.is_loop()
        && !facts.completed_heads.is_empty()
    {
        let roster = dedupe_by_identity(facts.completed_heads.iter().cloned());
        state.restore_pending = Some(roster.clone());
        state.window = ScheduleWindow::Open {
            pass: state.passes_completed.saturating_add(1),
            opened_secs: now,
            roster,
            ran: Vec::new(),
        };
        FireDecision::Recovered
    } else if facts.live_heads.is_empty() {
        if !spec.has_cadence() {
            // A cadence-less loop with nothing queued paces itself by pass
            // completion; an empty poll counts as one so it waits the floor.
            state.last_pass_completed_secs = Some(now);
        }
        FireDecision::EmptyPoll
    } else {
        state.window = ScheduleWindow::Open {
            pass: state.passes_completed.saturating_add(1),
            opened_secs: now,
            roster: dedupe_by_identity(facts.live_heads.iter().cloned()),
            ran: Vec::new(),
        };
        FireDecision::Opened
    };
    (state, decision)
}

fn dedupe_by_identity(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(identity(item)))
        .collect()
}

/// What observing the document's current heads did to an open pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObserveOutcome {
    /// No window, or the pass still has live work.
    Unchanged,
    /// The roster grew (a head appeared while the `loop` pass ran).
    RosterGrew,
    /// Pass `pass` finished; `restore` lists `loop` items to put back.
    Completed { pass: u32, restore: Vec<String> },
}

/// Observe the document's live heads against the open window.
///
/// A non-loop pass completes when the queue has no live head. A `loop` pass
/// completes when none of its roster items is live; heads that appear while
/// it runs join the roster so they loop too. Completion is idempotent:
/// `passes_completed` becomes `max(passes_completed, pass)`.
pub fn observe(
    state: QueueScheduleState,
    spec: &QueueScheduleSpec,
    live_heads: &[String],
    now: u64,
) -> (QueueScheduleState, ObserveOutcome) {
    observe_inner(state, spec, live_heads, None, now)
}

/// Observe a queue with explicit completion evidence. During a loop pass, a
/// roster item that is not live and that the drain was never observed acting
/// on (claimed, or struck-complete in `completed_heads`) never ran; removing
/// it prevents intermediate operator keystroke states (GH #310) from being
/// restored later. Drain evidence is sticky for the pass: an item once seen
/// claimed or struck is restored even after a drain erased its row (GH #327).
pub fn observe_with_completed(
    state: QueueScheduleState,
    spec: &QueueScheduleSpec,
    live_heads: &[String],
    completed_heads: &[String],
    now: u64,
) -> (QueueScheduleState, ObserveOutcome) {
    observe_inner(state, spec, live_heads, Some(completed_heads), now)
}

fn observe_inner(
    mut state: QueueScheduleState,
    spec: &QueueScheduleSpec,
    live_heads: &[String],
    completed_heads: Option<&[String]>,
    now: u64,
) -> (QueueScheduleState, ObserveOutcome) {
    // A recovered pass starts with its old struck roster in the document and
    // keeps that roster in `restore_pending` until the restore is visibly
    // proven. Those struck rows are evidence for the previous/lost restore,
    // not evidence that the newly opened pass ran. Letting observation proceed
    // here immediately completed the recovered pass before any head could be
    // dispatched (GH #348).
    if state.restore_pending.is_some() {
        return (state, ObserveOutcome::Unchanged);
    }
    let ScheduleWindow::Open {
        pass, roster, ran, ..
    } = &mut state.window
    else {
        return (state, ObserveOutcome::Unchanged);
    };
    let pass = *pass;
    let live: std::collections::HashSet<QueueItemIdentity> =
        live_heads.iter().map(|head| identity(head)).collect();
    let mut grew = false;
    if spec.loop_limit.is_loop() {
        if let Some(completed_heads) = completed_heads {
            // Drain evidence observed now: a struck-complete row, or a live
            // head carrying a drain lifecycle marker (claimed / dispatched).
            let evidence: std::collections::HashSet<QueueItemIdentity> = completed_heads
                .iter()
                .map(|head| identity(head))
                .chain(
                    live_heads
                        .iter()
                        .filter(|head| head_carries_drain_lifecycle_marker(head))
                        .map(|head| identity(head)),
                )
                .collect();
            let mut ran_ids: std::collections::HashSet<QueueItemIdentity> =
                ran.iter().map(|item| identity(item)).collect();
            for item in roster.iter() {
                let id = identity(item);
                if evidence.contains(&id) && ran_ids.insert(id) {
                    ran.push(item.clone());
                }
            }
            roster.retain(|item| {
                let item = identity(item);
                live.contains(&item) || ran_ids.contains(&item)
            });
        }
        let known: std::collections::HashSet<QueueItemIdentity> =
            roster.iter().map(|item| identity(item)).collect();
        for head in live_heads {
            if !known.contains(&identity(head)) {
                roster.push(head.clone());
                grew = true;
            }
        }
    }
    let complete = if spec.loop_limit.is_loop() {
        !roster.iter().any(|item| live.contains(&identity(item)))
    } else {
        live.is_empty()
    };
    if !complete {
        let outcome = if grew {
            ObserveOutcome::RosterGrew
        } else {
            ObserveOutcome::Unchanged
        };
        return (state, outcome);
    }
    let roster = std::mem::take(roster);
    state.window = ScheduleWindow::Closed;
    state.passes_completed = state.passes_completed.max(pass);
    state.last_pass_completed_secs = Some(now);
    let restore = if spec.loop_limit.is_loop() {
        match completed_heads {
            // `#loopnoresurrect`: the restore set is the roster items whose
            // struck-complete row is still in THIS loop's body when the pass
            // completes. A roster item that is neither live nor struck here
            // was removed or moved out of the loop (operator delete, a cut to
            // `agent:run`, the done archive); its `ran` evidence keeps it in
            // the roster while the pass runs, but it is never resurrected.
            // On 2026-10-26 a 3-day pass restored 12 long-removed rows after
            // the operator moved the loop's rows into `agent:run`.
            Some(completed_heads) => {
                let struck: std::collections::HashSet<QueueItemIdentity> =
                    completed_heads.iter().map(|head| identity(head)).collect();
                roster
                    .into_iter()
                    .filter(|item| struck.contains(&identity(item)))
                    .collect()
            }
            None => roster,
        }
    } else {
        Vec::new()
    };
    if !restore.is_empty() {
        state.restore_pending = Some(restore.clone());
    }
    (state, ObserveOutcome::Completed { pass, restore })
}

/// Record a failed tick or restore: exponential backoff from
/// [`BASE_SCHEDULE_BACKOFF_SECS`], capped at [`MAX_SCHEDULE_BACKOFF_SECS`].
pub fn record_failure(mut state: QueueScheduleState, now: u64) -> QueueScheduleState {
    state.failures = state.failures.saturating_add(1);
    let exponent = state.failures.saturating_sub(1).min(16);
    let delay = BASE_SCHEDULE_BACKOFF_SECS
        .saturating_mul(1u64 << exponent)
        .min(MAX_SCHEDULE_BACKOFF_SECS);
    state.backoff_until_secs = Some(now.saturating_add(delay));
    state
}

/// A pending `loop` restore landed: the items are back in the queue.
pub fn restore_applied(state: QueueScheduleState) -> QueueScheduleState {
    let mut state = record_success(state);
    state.restore_pending = None;
    state
}

/// Record a successful tick or restore: clear any backoff.
pub fn record_success(mut state: QueueScheduleState) -> QueueScheduleState {
    state.failures = 0;
    state.backoff_until_secs = None;
    state
}

/// Whether a scheduled queue's heads are held from every drain path.
///
/// `spec == None` (unscheduled) holds nothing. A scheduled queue the
/// controller has not admitted yet (`state == None`) holds everything: the
/// schedule is the operator's explicit restriction, so the failure direction
/// is "wait for the scheduler", never "drain off-schedule". An open window
/// holds nothing.
pub fn holds_heads(spec: Option<&QueueScheduleSpec>, state: Option<&QueueScheduleState>) -> bool {
    match (spec, state) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(_), Some(state)) => !state.window_open(),
    }
}

/// The heads of `content` a closed schedule window holds back (empty when the
/// queue is unscheduled or its window is open).
pub fn held_heads(content: &str, state: Option<&QueueScheduleState>) -> Vec<String> {
    let Some(schedule) = identified_queue_schedules(content)
        .ok()
        .and_then(|schedules| schedules.into_iter().next())
    else {
        return Vec::new();
    };
    held_heads_for_queue(content, &schedule, state).unwrap_or_default()
}

/// Queue-local drain gate. Invalid declared syntax is held fail-closed; valid
/// siblings remain governed solely by their own state.
pub fn held_heads_for_queue(
    content: &str,
    schedule: &IdentifiedQueueSchedule,
    state: Option<&QueueScheduleState>,
) -> anyhow::Result<Vec<String>> {
    if !schedule.is_valid() || holds_heads(schedule.spec.as_ref(), state) {
        live_heads_for_queue(content, &schedule.queue_id)
    } else {
        Ok(Vec::new())
    }
}

/// Operator-facing schedule status (`agent-doc queue schedule`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueScheduleStatus {
    pub interval_secs: Option<u64>,
    pub times: Vec<String>,
    pub loop_limit: LoopLimit,
    pub passes_completed: u32,
    pub window_open: bool,
    pub current_pass: Option<u32>,
    pub next_run_secs: Option<u64>,
    pub last_fired_at_secs: Option<u64>,
    pub exhausted: bool,
    pub restore_pending: bool,
    pub failures: u32,
    pub backoff_until_secs: Option<u64>,
    /// The controller has not admitted this schedule yet.
    pub pending_admission: bool,
}

pub fn status(
    spec: &QueueScheduleSpec,
    state: Option<&QueueScheduleState>,
    tz: &dyn LocalOffset,
) -> QueueScheduleStatus {
    let times = spec.times.iter().map(|time| time.render()).collect();
    match state {
        None => QueueScheduleStatus {
            interval_secs: spec.interval_secs,
            times,
            loop_limit: spec.loop_limit,
            passes_completed: 0,
            window_open: false,
            current_pass: None,
            next_run_secs: None,
            last_fired_at_secs: None,
            exhausted: false,
            restore_pending: false,
            failures: 0,
            backoff_until_secs: None,
            pending_admission: true,
        },
        Some(state) => QueueScheduleStatus {
            interval_secs: spec.interval_secs,
            times,
            loop_limit: spec.loop_limit,
            passes_completed: state.passes_completed,
            window_open: state.window_open(),
            current_pass: state.current_pass(),
            next_run_secs: wake_deadline(state, spec, tz),
            last_fired_at_secs: state.last_fired_at_secs,
            exhausted: spec.loop_limit.exhausted_after(state.passes_completed),
            restore_pending: state.restore_pending.is_some(),
            failures: state.failures,
            backoff_until_secs: state.backoff_until_secs,
            pending_admission: false,
        },
    }
}

/// Plan putting a completed `loop` pass's items back into the scheduled queue
/// block ([`scheduled_queue_component`]).
///
/// Each roster item without a live head of the same identity is restored: a
/// struck (`~~…~~`) copy of it is removed and the item is re-appended, in
/// roster order, before the closing marker. Every other byte of the queue is
/// preserved. `None` when every roster item is already live.
pub fn plan_loop_restore(content: &str, roster: &[String]) -> anyhow::Result<Option<String>> {
    let Some(schedule) = identified_queue_schedules(content)?.into_iter().next() else {
        anyhow::bail!("no agent:queue component to restore loop items into");
    };
    plan_loop_restore_for_queue(content, &schedule.queue_id, roster)
}

/// Queue-ID-addressed loop restore. Sibling queue bytes are never candidates
/// for roster removal or insertion.
pub fn plan_loop_restore_for_queue(
    content: &str,
    queue_id: &str,
    roster: &[String],
) -> anyhow::Result<Option<String>> {
    let components = element::parse(content)?;
    let queue = queue_block(content, &components, queue_id)?;
    let body = &content[queue.open_end..queue.close_start];
    let spans = document_queue::parse_spans(body)?;
    let live: std::collections::HashSet<QueueItemIdentity> = spans
        .iter()
        .filter_map(|(entry, _)| match entry {
            QueueEntry::Prompt(prompt) => Some(identity(&prompt.text)),
            _ => None,
        })
        .collect();
    // `#loopnoresurrect`: only a roster item whose struck (`~~…~~`) row is
    // still in this queue's body is restored. An item that is neither live
    // nor struck here was removed or moved out of the loop after the pass
    // completed (or before a retained restore landed); the restore never
    // re-adds it, so done, archived, or relocated rows stay gone.
    let struck: std::collections::HashSet<QueueItemIdentity> = spans
        .iter()
        .filter_map(|(entry, _)| match entry {
            QueueEntry::Completed(prompt) => Some(identity(&prompt.text)),
            _ => None,
        })
        .collect();
    let mut seen_missing = std::collections::HashSet::new();
    let missing: Vec<&String> = roster
        .iter()
        .filter(|item| {
            let id = identity(item);
            !live.contains(&id) && struck.contains(&id) && seen_missing.insert(id)
        })
        .collect();
    if missing.is_empty() {
        return Ok(None);
    }
    let missing_ids: std::collections::HashSet<QueueItemIdentity> =
        missing.iter().map(|item| identity(item)).collect();
    let mut removed: std::collections::HashSet<QueueItemIdentity> =
        std::collections::HashSet::new();
    let mut kept_body = String::with_capacity(body.len());
    let mut cursor = 0;
    for (entry, range) in &spans {
        if let QueueEntry::Completed(prompt) = entry {
            let id = identity(&prompt.text);
            if missing_ids.contains(&id) && removed.insert(id) {
                kept_body.push_str(&body[cursor..range.start]);
                cursor = range.end;
            }
        }
    }
    kept_body.push_str(&body[cursor..]);
    if !kept_body.is_empty() && !kept_body.ends_with('\n') {
        kept_body.push('\n');
    }
    for item in missing {
        let mut lines = item.trim().lines();
        if let Some(first) = lines.next() {
            kept_body.push_str("- ");
            kept_body.push_str(first);
            kept_body.push('\n');
        }
        for line in lines {
            kept_body.push_str("  ");
            kept_body.push_str(line);
            kept_body.push('\n');
        }
    }
    let mut updated = String::with_capacity(content.len() + kept_body.len());
    updated.push_str(&content[..queue.open_end]);
    updated.push_str(&kept_body);
    updated.push_str(&content[queue.close_start..]);
    Ok(Some(updated))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UTC: FixedOffset = FixedOffset(0);

    fn attrs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    fn spec(pairs: &[(&str, &str)]) -> QueueScheduleSpec {
        let (spec, issues) = queue_schedule_spec_for_attrs(&attrs(pairs));
        assert!(issues.is_empty(), "{issues:?}");
        spec.expect("schedule")
    }

    fn doc(marker_attrs: &str, body: &str) -> String {
        format!("# Doc\n\n<!-- agent:queue {marker_attrs} -->\n{body}<!-- /agent:queue -->\n")
    }

    fn heads(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn facts(items: &[&str]) -> FireFacts {
        FireFacts {
            live_heads: heads(items),
            completed_heads: Vec::new(),
            paused: false,
        }
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration_secs("90"), Ok(90));
        assert_eq!(parse_duration_secs("30s"), Ok(30));
        assert_eq!(parse_duration_secs("5m"), Ok(300));
        assert_eq!(parse_duration_secs("1h30m"), Ok(5_400));
        assert_eq!(parse_duration_secs("1d"), Ok(86_400));
        assert_eq!(parse_duration_secs("\"2h\""), Ok(7_200));
        assert!(parse_duration_secs("5x").is_err());
        assert!(parse_duration_secs("1h30").is_err());
        assert!(parse_duration_secs("").is_err());
    }

    #[test]
    fn parses_schedule_attributes() {
        let spec = spec(&[
            ("interval", "30m"),
            ("time", "17:30,09:00,09:00"),
            ("loop", "3"),
        ]);
        assert_eq!(spec.interval_secs, Some(1_800));
        assert_eq!(
            spec.times
                .iter()
                .map(|time| time.render())
                .collect::<Vec<_>>(),
            vec!["09:00", "17:30"]
        );
        assert_eq!(spec.loop_limit, LoopLimit::Passes(3));
        assert_eq!(self::spec(&[("loop", "")]).loop_limit, LoopLimit::Unbounded);
        let (none, issues) = queue_schedule_spec_for_attrs(&attrs(&[("go", "")]));
        assert!(none.is_none() && issues.is_empty());
    }

    #[test]
    fn invalid_values_are_reported_as_fatal() {
        let (spec, issues) =
            queue_schedule_spec_for_attrs(&attrs(&[("interval", "soon"), ("time", "25:00")]));
        assert!(
            spec.is_none(),
            "an all-invalid schedule must not schedule the queue"
        );
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(issues.iter().all(|issue| issue.fatal));
        let (spec, issues) = queue_schedule_spec_for_attrs(&attrs(&[("loop", "0")]));
        assert!(spec.is_none());
        assert!(issues[0].message.contains("positive pass count"));
        assert!(issues[0].fatal);
    }

    #[test]
    fn interval_below_floor_is_clamped_with_a_warning() {
        let (spec, issues) = queue_schedule_spec_for_attrs(&attrs(&[("interval", "5s")]));
        assert_eq!(
            spec.unwrap().interval_secs,
            Some(MIN_SCHEDULE_INTERVAL_SECS)
        );
        assert!(issues[0].message.contains("floor"), "{issues:?}");
        assert!(!issues[0].fatal);
    }

    #[test]
    fn spec_reads_the_selected_queue_marker() {
        let content = doc("interval=5m loop", "- check CI\n");
        let spec = queue_schedule_spec(&content).unwrap();
        assert_eq!(spec.interval_secs, Some(300));
        assert_eq!(spec.loop_limit, LoopLimit::Unbounded);
        assert!(!content_is_scheduled(&doc("go", "- a\n")));
        // A marker quoted in prose is not the queue.
        assert!(!content_is_scheduled(
            "Use `<!-- agent:queue interval=5m -->` to schedule.\n"
        ));
    }

    #[test]
    fn a_drained_queue_stays_scheduled() {
        // Between passes every item may be consumed; the queue is then idle
        // (never *selected*), but its schedule must survive or the durable
        // pass count would be dropped with it.
        let drained = doc("interval=10m loop=3", "- ~~a~~\n");
        let spec =
            queue_schedule_spec(&drained).expect("an empty scheduled queue is still scheduled");
        assert_eq!(spec.loop_limit, LoopLimit::Passes(3));
        assert!(content_is_scheduled(&doc("loop", "")));
        assert!(!queue_held_by_control(&drained));
        let restored = plan_loop_restore(&drained, &heads(&["a"]))
            .unwrap()
            .unwrap();
        assert_eq!(restored, doc("interval=10m loop=3", "- a\n"));
    }

    #[test]
    fn multi_queue_documents_read_the_schedule_from_the_scheduled_block() {
        let content = "# Doc\n\n<!-- agent:queue id=first -->\n- x\n<!-- /agent:queue -->\n\n\
                       <!-- agent:queue id=later interval=1h after=first -->\n- y\n<!-- /agent:queue -->\n";
        let spec = queue_schedule_spec(content).expect("the scheduled block governs the document");
        assert_eq!(spec.interval_secs, Some(3_600));
    }

    #[test]
    fn new_interval_schedule_fires_immediately_then_every_interval() {
        let spec = spec(&[("interval", "10m")]);
        let state = reconcile(None, &spec, 1_000);
        assert_eq!(next_deadline(&state, &spec, &UTC), Some(1_000));
        let (state, decision) = fire(state, &spec, 1_000, &facts(&["a"]), &UTC);
        assert_eq!(decision, FireDecision::Opened);
        assert_eq!(state.current_pass(), Some(1));
        assert_eq!(next_deadline(&state, &spec, &UTC), Some(1_600));
    }

    #[test]
    fn a_tick_fires_exactly_once() {
        let spec = spec(&[("interval", "10m")]);
        let state = reconcile(None, &spec, 1_000);
        let (state, first) = fire(state, &spec, 1_000, &facts(&[]), &UTC);
        assert_eq!(first, FireDecision::EmptyPoll);
        // A second controller (or a restarted one) replays the same wake.
        let replayed: QueueScheduleState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        let (_, second) = fire(replayed, &spec, 1_000, &facts(&["a"]), &UTC);
        assert_eq!(second, FireDecision::NotDue);
    }

    #[test]
    fn an_idle_loop_recovers_its_struck_roster_into_a_new_pass() {
        let spec = spec(&[("interval", "1h"), ("loop", "")]);
        let mut state = reconcile(None, &spec, 1_000);
        state.passes_completed = 9;
        let facts = FireFacts {
            live_heads: Vec::new(),
            completed_heads: heads(&["#upgrade", "check CI", "#upgrade"]),
            paused: false,
        };

        let (state, decision) = fire(state, &spec, 1_000, &facts, &UTC);

        let roster = heads(&["#upgrade", "check CI"]);
        assert_eq!(decision, FireDecision::Recovered);
        assert_eq!(state.restore_pending, Some(roster.clone()));
        assert_eq!(
            state.window,
            ScheduleWindow::Open {
                pass: 10,
                opened_secs: 1_000,
                roster,
                ran: Vec::new(),
            }
        );
    }

    #[test]
    fn missed_ticks_are_coalesced_into_one() {
        let spec = spec(&[("interval", "10m")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&[]), &UTC);
        // Suspended for ten hours: sixty ticks were missed.
        let now = 36_000 + 17;
        let (state, decision) = fire(state, &spec, now, &facts(&[]), &UTC);
        assert_eq!(decision, FireDecision::EmptyPoll);
        assert_eq!(state.last_fired_due_secs, Some(36_000));
        assert_eq!(state.coalesced_ticks, 1);
        let next = next_deadline(&state, &spec, &UTC).unwrap();
        assert!(next > now, "the next tick is strictly after now: {next}");
        let (_, again) = fire(state, &spec, now, &facts(&[]), &UTC);
        assert_eq!(again, FireDecision::NotDue, "no catch-up burst");
    }

    #[test]
    fn time_of_day_uses_the_local_offset() {
        let spec = spec(&[("time", "09:00,17:30")]);
        // UTC-7: local 09:00 is 16:00 UTC.
        let tz = FixedOffset(-7 * 3_600);
        let day = 20 * 86_400;
        let state = reconcile(None, &spec, day + 10 * 3_600);
        assert_eq!(
            next_deadline(&state, &spec, &tz),
            Some(day + 16 * 3_600),
            "first tick is today's local 09:00"
        );
        let (state, _) = fire(state, &spec, day + 16 * 3_600, &facts(&[]), &tz);
        assert_eq!(
            next_deadline(&state, &spec, &tz),
            Some(day + 24 * 3_600 + 30 * 60),
            "then local 17:30 (00:30 UTC next day)"
        );
    }

    #[test]
    fn time_of_day_coalesces_a_multi_day_gap() {
        let spec = spec(&[("time", "09:00")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 9 * 3_600, &facts(&[]), &UTC);
        let now = 5 * 86_400 + 12 * 3_600;
        let (state, decision) = fire(state, &spec, now, &facts(&[]), &UTC);
        assert_eq!(decision, FireDecision::EmptyPoll);
        assert_eq!(state.last_fired_due_secs, Some(5 * 86_400 + 9 * 3_600));
        assert_eq!(
            next_deadline(&state, &spec, &UTC),
            Some(6 * 86_400 + 9 * 3_600)
        );
    }

    #[test]
    fn interval_and_time_combine_to_the_earliest() {
        let spec = spec(&[("interval", "6h"), ("time", "01:00")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&[]), &UTC);
        assert_eq!(next_deadline(&state, &spec, &UTC), Some(3_600));
        let (state, _) = fire(state, &spec, 3_600, &facts(&[]), &UTC);
        assert_eq!(next_deadline(&state, &spec, &UTC), Some(6 * 3_600));
    }

    #[test]
    fn editing_the_cadence_reschedules_from_now_and_keeps_passes() {
        let spec_a = spec(&[("interval", "1h"), ("loop", "")]);
        let mut state = reconcile(None, &spec_a, 0);
        state.passes_completed = 4;
        let spec_b = spec(&[("interval", "10m"), ("loop", "")]);
        let state = reconcile(Some(state), &spec_b, 500);
        assert_eq!(state.passes_completed, 4);
        assert_eq!(state.anchor_secs, 500);
        assert_eq!(next_deadline(&state, &spec_b, &UTC), Some(1_100));
    }

    #[test]
    fn paused_and_busy_ticks_are_skipped_not_deferred() {
        let spec = spec(&[("interval", "10m")]);
        let state = reconcile(None, &spec, 0);
        let paused = FireFacts {
            live_heads: heads(&["a"]),
            completed_heads: Vec::new(),
            paused: true,
        };
        let (state, decision) = fire(state, &spec, 0, &paused, &UTC);
        assert_eq!(decision, FireDecision::SkippedPaused);
        assert!(!state.window_open());
        let (state, decision) = fire(state, &spec, 600, &facts(&["a"]), &UTC);
        assert_eq!(decision, FireDecision::Opened);
        let (state, decision) = fire(state, &spec, 1_200, &facts(&["a"]), &UTC);
        assert_eq!(decision, FireDecision::SkippedBusy);
        assert_eq!(
            state.current_pass(),
            Some(1),
            "the running pass is untouched"
        );
    }

    #[test]
    fn non_loop_pass_completes_when_the_queue_drains() {
        let spec = spec(&[("interval", "10m")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["a", "b"]), &UTC);
        let (state, outcome) = observe(state, &spec, &heads(&["b"]), 10);
        assert_eq!(outcome, ObserveOutcome::Unchanged);
        let (state, outcome) = observe(state, &spec, &[], 20);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: Vec::new()
            }
        );
        assert_eq!(state.passes_completed, 1);
        assert!(state.restore_pending.is_none());
    }

    /// GH #310: realtime refreshes can observe every intermediate state of an
    /// operator-edited line. Only the current live spelling, or a spelling
    /// proven struck-complete, remains owned by the pass.
    #[test]
    fn loop_roster_drops_replaced_typing_states_before_restore() {
        let spec = spec(&[("interval", "10m"), ("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, decision) = fire(state, &spec, 0, &facts(&["#upgrade"]), &UTC);
        assert_eq!(decision, FireDecision::Opened);

        let (state, outcome) =
            observe_with_completed(state, &spec, &heads(&["#upgrade", "refresh prfex"]), &[], 1);
        assert_eq!(outcome, ObserveOutcome::RosterGrew);
        let (state, outcome) = observe_with_completed(
            state,
            &spec,
            &heads(&["#upgrade", "refresh prefix"]),
            &[],
            2,
        );
        assert_eq!(outcome, ObserveOutcome::RosterGrew);
        assert!(matches!(
            &state.window,
            ScheduleWindow::Open { roster, .. }
                if roster == &heads(&["#upgrade", "refresh prefix"])
        ));

        let (state, outcome) = observe_with_completed(
            state,
            &spec,
            &[],
            &heads(&["#upgrade", "refresh prefix"]),
            3,
        );
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: heads(&["#upgrade", "refresh prefix"]),
            }
        );
        assert_eq!(
            state.restore_pending,
            Some(heads(&["#upgrade", "refresh prefix"]))
        );
    }

    /// GH #327 pass 3 is now held by struck-row retention: every binary drain
    /// of a `loop` queue keeps the struck copy ([`queue_retains_struck_heads`]),
    /// so a claimed head that completes leaves `~~#upgrade~~` behind and is
    /// restored from it.
    #[test]
    fn loop_item_seen_claimed_is_restored_from_its_retained_struck_row() {
        let spec = spec(&[("interval", "1h"), ("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, decision) = fire(state, &spec, 0, &facts(&["#upgrade"]), &UTC);
        assert_eq!(decision, FireDecision::Opened);
        let (state, outcome) =
            observe_with_completed(state, &spec, &heads(&["🚧 #upgrade"]), &[], 1);
        assert_eq!(outcome, ObserveOutcome::Unchanged);
        let (state, outcome) = observe_with_completed(state, &spec, &[], &heads(&["#upgrade"]), 2);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: heads(&["#upgrade"]),
            }
        );
        assert_eq!(state.restore_pending, Some(heads(&["#upgrade"])));
    }

    /// `#loopnoresurrect`: a claimed row the operator then deletes (or cuts
    /// into `agent:run`) leaves neither a live nor a struck copy in the loop.
    /// Its `ran` evidence closes the pass but never resurrects the row.
    #[test]
    fn loop_item_seen_claimed_then_removed_is_not_resurrected() {
        let spec = spec(&[("interval", "1h"), ("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["#upgrade"]), &UTC);
        let (state, _) = observe_with_completed(state, &spec, &heads(&["🚧 #upgrade"]), &[], 1);
        let (state, outcome) = observe_with_completed(state, &spec, &[], &[], 2);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: Vec::new(),
            }
        );
        assert_eq!(state.restore_pending, None);
        assert_eq!(state.window, ScheduleWindow::Closed);
    }

    /// GH #327 + `#loopnoresurrect`: a struck copy proves the item ran and
    /// keeps it in the roster, but once that struck row is gone from the loop
    /// (nothing in the binary erases a loop's struck rows, so the operator
    /// removed it) only the items still struck in the loop are restored.
    #[test]
    fn loop_item_whose_struck_row_was_removed_is_not_restored() {
        let spec = spec(&[("interval", "1h"), ("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["#upgrade", "poll"]), &UTC);
        // `#upgrade` is struck while `poll` still runs: the pass stays open.
        let (state, outcome) =
            observe_with_completed(state, &spec, &heads(&["poll"]), &heads(&["#upgrade"]), 1);
        assert_eq!(outcome, ObserveOutcome::Unchanged);
        // The struck row is erased, then `poll` drains.
        let (state, outcome) = observe_with_completed(state, &spec, &heads(&["poll"]), &[], 2);
        assert_eq!(outcome, ObserveOutcome::Unchanged);
        let (_, outcome) = observe_with_completed(state, &spec, &[], &heads(&["poll"]), 3);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: heads(&["poll"]),
            }
        );
    }

    /// GH #310 stays fixed: a pin marker is operator priority, not drain
    /// evidence, so an edited-away pinned spelling is still dropped.
    #[test]
    fn pin_marker_is_not_drain_evidence() {
        assert!(head_carries_drain_lifecycle_marker("🚧 #upgrade"));
        assert!(head_carries_drain_lifecycle_marker("📌 🟡 #upgrade"));
        assert!(head_carries_drain_lifecycle_marker("⏳ answer me"));
        assert!(!head_carries_drain_lifecycle_marker("📌 #upgrade"));
        assert!(!head_carries_drain_lifecycle_marker("#upgrade 🚧 later"));
        let spec = spec(&[("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["📌 refresh prfex"]), &UTC);
        let (state, _) =
            observe_with_completed(state, &spec, &heads(&["📌 refresh prefix"]), &[], 1);
        let (_, outcome) =
            observe_with_completed(state, &spec, &[], &heads(&["📌 refresh prefix"]), 2);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: heads(&["📌 refresh prefix"]),
            }
        );
    }

    #[test]
    fn keep_and_loop_queue_markers_retain_struck_heads() {
        assert!(queue_retains_struck_heads(&attrs(&[("keep", "")])));
        assert!(queue_retains_struck_heads(&attrs(&[
            ("keep", "true"),
            ("interval", "1h")
        ])));
        assert!(queue_retains_struck_heads(&attrs(&[("loop", "")])));
        assert!(queue_retains_struck_heads(&attrs(&[
            ("loop", "3"),
            ("interval", "1h")
        ])));
        assert!(!queue_retains_struck_heads(&attrs(&[("interval", "1h")])));
        assert!(!queue_retains_struck_heads(&attrs(&[])));
    }

    #[test]
    fn loop_three_runs_three_passes_and_restores_items_each_time() {
        let spec = spec(&[("interval", "10m"), ("loop", "3")]);
        let mut state = reconcile(None, &spec, 0);
        let mut now = 0;
        for pass in 1..=3 {
            let (fired, decision) = fire(state, &spec, now, &facts(&["a", "b"]), &UTC);
            assert_eq!(decision, FireDecision::Opened, "pass {pass}");
            let (drained, outcome) = observe(fired, &spec, &[], now + 1);
            assert_eq!(
                outcome,
                ObserveOutcome::Completed {
                    pass,
                    restore: heads(&["a", "b"])
                }
            );
            assert!(drained.restore_pending.is_some());
            // The restore lands; the items are live again and nothing holds
            // the next wake.
            let restored = restore_applied(drained);
            let (restored, outcome) = observe(restored, &spec, &heads(&["a", "b"]), now + 2);
            assert_eq!(outcome, ObserveOutcome::Unchanged);
            state = restored;
            now += 600;
        }
        assert_eq!(state.passes_completed, 3);
        let (state, decision) = fire(state, &spec, now, &facts(&["a", "b"]), &UTC);
        assert_eq!(decision, FireDecision::Exhausted);
        assert_eq!(
            wake_deadline(&state, &spec, &UTC),
            None,
            "an exhausted loop never wakes"
        );
    }

    #[test]
    fn bare_loop_runs_indefinitely_and_keeps_its_items() {
        let spec = spec(&[("interval", "10m"), ("loop", "")]);
        let mut content = doc("interval=10m loop", "- a\n- b\n");
        let mut state = reconcile(None, &spec, 0);
        let mut now = 0;
        for pass in 1..=50u32 {
            let (fired, decision) = fire(state, &spec, now, &facts(&["a", "b"]), &UTC);
            assert_eq!(decision, FireDecision::Opened, "pass {pass}");
            // The drain consumes both items (struck in place).
            let drained = doc("interval=10m loop", "- ~~a~~\n- ~~b~~\n");
            let (done, outcome) = observe(fired, &spec, &live_heads(&drained), now + 30);
            let ObserveOutcome::Completed { restore, .. } = outcome else {
                panic!("pass {pass} did not complete: {outcome:?}");
            };
            content = plan_loop_restore(&drained, &restore).unwrap().unwrap();
            assert_eq!(
                live_heads(&content),
                heads(&["a", "b"]),
                "pass {pass} kept the items"
            );
            state = restore_applied(done);
            assert!(
                wake_deadline(&state, &spec, &UTC).is_some(),
                "an unbounded loop never exhausts"
            );
            now += 600;
        }
        assert_eq!(state.passes_completed, 50);
        assert_eq!(content, doc("interval=10m loop", "- a\n- b\n"));
    }

    #[test]
    fn replaying_a_completion_never_double_counts() {
        let spec = spec(&[("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["a"]), &UTC);
        let open = state.clone();
        let (done, _) = observe(state, &spec, &[], 5);
        assert_eq!(done.passes_completed, 1);
        // A second controller observes the same completion from the same open
        // state it loaded before the first one committed.
        let (replay, _) = observe(open, &spec, &[], 6);
        assert_eq!(replay.passes_completed.max(done.passes_completed), 1);
        let (again, outcome) = observe(done, &spec, &[], 7);
        assert_eq!(outcome, ObserveOutcome::Unchanged);
        assert_eq!(again.passes_completed, 1);
    }

    #[test]
    fn loop_roster_grows_with_heads_added_mid_pass() {
        let spec = spec(&[("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["a"]), &UTC);
        let (state, outcome) = observe(state, &spec, &heads(&["a", "new"]), 1);
        assert_eq!(outcome, ObserveOutcome::RosterGrew);
        let (_, outcome) = observe(state, &spec, &[], 2);
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: heads(&["a", "new"])
            }
        );
    }

    #[test]
    fn cadence_less_loop_waits_the_floor_between_passes() {
        let spec = spec(&[("loop", "")]);
        let state = reconcile(None, &spec, 100);
        assert_eq!(next_deadline(&state, &spec, &UTC), Some(100));
        let (state, _) = fire(state, &spec, 100, &facts(&["a"]), &UTC);
        let (state, _) = observe(state, &spec, &[], 400);
        assert_eq!(
            next_deadline(&state, &spec, &UTC),
            Some(400 + MIN_SCHEDULE_INTERVAL_SECS)
        );
    }

    #[test]
    fn canonical_loop_component_is_an_unbounded_repeating_queue() {
        let content = concat!(
            "<!-- agent:loop id=issues interval=5m -->\n",
            "- poll issues\n",
            "<!-- /agent:loop -->\n",
        );
        let schedules = identified_queue_schedules(content).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].queue_id, "issues");
        assert_eq!(
            schedules[0].spec.as_ref().unwrap().loop_limit,
            LoopLimit::Unbounded
        );
        assert_eq!(
            live_heads_for_queue(content, "issues").unwrap(),
            heads(&["poll issues"])
        );
    }

    #[test]
    fn run_loop_and_queue_loop_compatibility_spellings_match_agent_loop() {
        for content in [
            "<!-- agent:loop id=issues interval=5m -->\n- poll issues\n<!-- /agent:loop -->\n",
            "<!-- agent:run id=issues interval=5m loop -->\n- poll issues\n<!-- /agent:run -->\n",
            "<!-- agent:queue id=issues interval=5m loop -->\n- poll issues\n<!-- /agent:queue -->\n",
        ] {
            let schedule = identified_queue_schedules(content).unwrap().remove(0);
            assert_eq!(schedule.queue_id, "issues");
            assert_eq!(schedule.spec.unwrap().loop_limit, LoopLimit::Unbounded);
            assert_eq!(
                live_heads_for_queue(content, "issues").unwrap(),
                heads(&["poll issues"])
            );
        }
    }

    #[test]
    fn failures_back_off_exponentially_with_a_cap() {
        let spec = spec(&[("interval", "10m")]);
        let mut state = reconcile(None, &spec, 0);
        let mut delays = Vec::new();
        for _ in 0..10 {
            state = record_failure(state, 1_000);
            delays.push(state.backoff_until_secs.unwrap() - 1_000);
        }
        assert_eq!(&delays[..4], &[30, 60, 120, 240]);
        assert_eq!(*delays.last().unwrap(), MAX_SCHEDULE_BACKOFF_SECS);
        assert_eq!(
            wake_deadline(&state, &spec, &UTC),
            Some(1_000 + MAX_SCHEDULE_BACKOFF_SECS)
        );
        let (_, decision) = fire(state.clone(), &spec, 1_001, &facts(&["a"]), &UTC);
        assert_eq!(decision, FireDecision::NotDue, "backoff gates the tick");
        let state = record_success(state);
        assert_eq!(state.failures, 0);
        assert_eq!(state.backoff_until_secs, None);
    }

    #[test]
    fn closed_window_holds_every_head_and_open_window_holds_none() {
        let content = doc("interval=10m", "- a\n- b\n");
        assert_eq!(held_heads(&content, None), heads(&["a", "b"]));
        let spec = queue_schedule_spec(&content).unwrap();
        let state = reconcile(None, &spec, 0);
        assert_eq!(held_heads(&content, Some(&state)), heads(&["a", "b"]));
        let (open, _) = fire(state, &spec, 0, &facts(&["a", "b"]), &UTC);
        assert!(held_heads(&content, Some(&open)).is_empty());
        assert!(
            held_heads(&doc("go", "- a\n"), None).is_empty(),
            "unscheduled"
        );
    }

    #[test]
    fn pause_holds_a_scheduled_queue() {
        assert!(!queue_held_by_control(&doc("interval=5m", "- a\n")));
        assert!(queue_held_by_control(&doc("interval=5m pause", "- a\n")));
        assert!(queue_held_by_control(&format!(
            "---\nqueue: pause\n---\n{}",
            doc("interval=5m", "- a\n")
        )));
    }

    #[test]
    fn restore_reappends_struck_items_and_never_resurrects_removed_ones() {
        // `b` is in the roster but has neither a live nor a struck row: it was
        // removed from the loop, so the restore leaves it gone
        // (`#loopnoresurrect`).
        let content = doc("loop", "- ~~a~~\n- keep me\n");
        let restored = plan_loop_restore(&content, &heads(&["a", "b", "keep me"]))
            .unwrap()
            .unwrap();
        assert_eq!(restored, doc("loop", "- keep me\n- a\n"));
        assert_eq!(
            plan_loop_restore(&restored, &heads(&["a", "b"])).unwrap(),
            None
        );
    }

    #[test]
    fn identified_schedules_are_queue_local_and_restore_cannot_touch_a_sibling() {
        let content = concat!(
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- ~~poll issues~~\n",
            "<!-- /agent:queue -->\n",
        );
        let schedules = identified_queue_schedules(content).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].queue_id, "issues");
        assert_eq!(
            schedules[0].spec.as_ref().unwrap().interval_secs,
            Some(3_600)
        );
        assert_eq!(
            live_heads_for_queue(content, "main").unwrap(),
            heads(&["ship now"])
        );
        assert!(live_heads_for_queue(content, "issues").unwrap().is_empty());

        let restored = plan_loop_restore_for_queue(content, "issues", &heads(&["poll issues"]))
            .unwrap()
            .unwrap();
        assert!(restored.contains("id=main go -->\n- ship now\n"));
        assert!(restored.contains("id=issues interval=1h loop -->\n- poll issues\n"));
    }

    #[test]
    fn canonical_run_marker_is_scheduled() {
        let content =
            "<!-- agent:run id=issues interval=1h loop -->\n- poll issues\n<!-- /agent:run -->\n";
        let schedules = identified_queue_schedules(content).unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].queue_id, "issues");
        assert_eq!(
            schedules[0].spec.as_ref().unwrap().interval_secs,
            Some(3_600)
        );
    }

    #[test]
    fn invalid_schedule_is_fatal_only_for_its_queue() {
        let content = concat!(
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=issues interval='1 hour' -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let schedules = identified_queue_schedules(content).unwrap();
        assert_eq!(schedules.len(), 1);
        assert!(!schedules[0].is_valid());
        assert_eq!(
            held_heads_for_queue(content, &schedules[0], None).unwrap(),
            heads(&["poll issues"])
        );
        assert_eq!(
            live_heads_for_queue(content, "main").unwrap(),
            heads(&["ship now"])
        );
    }

    #[test]
    fn status_reports_next_run_and_pass_state() {
        let spec = spec(&[("interval", "10m"), ("loop", "2")]);
        let pending = status(&spec, None, &UTC);
        assert!(pending.pending_admission);
        let state = reconcile(None, &spec, 0);
        let (state, _) = fire(state, &spec, 0, &facts(&["a"]), &UTC);
        let status = status(&spec, Some(&state), &UTC);
        assert!(status.window_open);
        assert_eq!(status.current_pass, Some(1));
        assert_eq!(status.next_run_secs, Some(600));
        assert_eq!(status.loop_limit, LoopLimit::Passes(2));
    }

    /// `#loopnoresurrect` on the 2026-10-26 `agent-doc-bugs.md` shape: an
    /// `agent:run id="main"` queue above an `agent:loop id=issues` loop. Pass 5
    /// ran for three days and its roster collected one-off rows that had been
    /// struck and removed from the loop long before. The operator then cut the
    /// loop's five rows into `agent:run`. The pass completes, but nothing is
    /// restored: neither the long-removed rows nor the ones just moved to
    /// `agent:run`, and no byte of either queue changes.
    #[test]
    fn two_queue_doc_loop_pass_never_resurrects_removed_or_moved_rows() {
        let before_move = "## Queue: Main\n\
<!-- agent:run id=\"main\" subagents priority go -->\n\
- 🚧 [#subagent:strikeformat#1] The strikethrough is malformed.\n\
<!-- /agent:run -->\n\
\n\
## Run: Issues\n\
<!-- agent:loop id=issues interval=1h priority -->\n\
- 📥 do [#layoutunprovenliveverify]\n\
- 📥 do [#grokqueuesubagents]\n\
- 🔄 release + publish\n\
- 📥 We need to work on making agent-doc easy to onboard.\n\
<!-- /agent:loop -->\n";
        let after_move = "## Queue: Main\n\
<!-- agent:run id=\"main\" subagents priority go -->\n\
- 🚧 [#subagent:strikeformat#1] The strikethrough is malformed.\n\
- 📥 do [#layoutunprovenliveverify]\n\
- 📥 do [#grokqueuesubagents]\n\
- 🔄 release + publish\n\
- 📥 We need to work on making agent-doc easy to onboard.\n\
<!-- /agent:run -->\n\
\n\
## Run: Issues\n\
<!-- agent:loop id=issues interval=1h priority -->\n\
<!-- /agent:loop -->\n";
        let historical = [
            "Watch https://github.com/btakita/agent-doc/issues for new issues.",
            "do [#gh363]",
            "do [#upgradeideatest]",
            "do [#latestbetadownload]",
            "Register agent-doc at https://plugins.jetbrains.com/plugin/add#intellij.",
            "[#coder-remote-install-02805]: done: but still not syncing",
        ];
        let spec = spec(&[("interval", "1h"), ("loop", "")]);
        let state = reconcile(None, &spec, 0);
        let (state, decision) = fire(state, &spec, 0, &facts(&historical), &UTC);
        assert_eq!(decision, FireDecision::Opened);
        // Each historical row ran (struck) and was later removed from the loop;
        // the current rows joined the roster while the pass stayed open.
        let (mut state, _) = observe_with_completed(
            state,
            &spec,
            &live_heads_for_queue(before_move, "issues").unwrap(),
            &heads(&historical),
            1,
        );
        if let ScheduleWindow::Open { roster, .. } = &state.window {
            assert_eq!(roster.len(), historical.len() + 4, "{roster:?}");
        } else {
            panic!("pass closed early: {:?}", state.window);
        }
        state = observe_with_completed(
            state,
            &spec,
            &live_heads_for_queue(before_move, "issues").unwrap(),
            &completed_heads_for_queue(before_move, "issues").unwrap(),
            2,
        )
        .0;
        let (state, outcome) = observe_with_completed(
            state,
            &spec,
            &live_heads_for_queue(after_move, "issues").unwrap(),
            &completed_heads_for_queue(after_move, "issues").unwrap(),
            3,
        );
        assert_eq!(
            outcome,
            ObserveOutcome::Completed {
                pass: 1,
                restore: Vec::new(),
            }
        );
        assert_eq!(state.restore_pending, None);

        // A restore roster already retained in state.db before this fix (the
        // 16-item `restore_pending` of the incident) is a no-op against the
        // document: nothing in it is struck in the loop.
        let mut stale_roster = heads(&historical);
        stale_roster.extend(live_heads_for_queue(before_move, "issues").unwrap());
        assert_eq!(
            plan_loop_restore_for_queue(after_move, "issues", &stale_roster).unwrap(),
            None
        );
        assert_eq!(
            plan_loop_restore_for_queue(before_move, "issues", &stale_roster).unwrap(),
            None
        );
    }

    /// `#loopnoresurrect`: a struck loop row is still restored in place, and the
    /// sibling `agent:run` queue is never a source or a target.
    #[test]
    fn two_queue_doc_loop_restore_reopens_only_its_own_struck_rows() {
        let content = "<!-- agent:run id=\"main\" -->\n\
- ~~poll issues~~\n\
- ship it\n\
<!-- /agent:run -->\n\
<!-- agent:loop id=issues interval=1h -->\n\
- ~~poll issues~~\n\
<!-- /agent:loop -->\n";
        let restored = plan_loop_restore_for_queue(
            content,
            "issues",
            &heads(&["poll issues", "ship it", "do [#gh363]"]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            restored,
            "<!-- agent:run id=\"main\" -->\n\
- ~~poll issues~~\n\
- ship it\n\
<!-- /agent:run -->\n\
<!-- agent:loop id=issues interval=1h -->\n\
- poll issues\n\
<!-- /agent:loop -->\n"
        );
    }

    /// `#componentownclose`: the loop restore rewrites only the loop body that
    /// ends at the loop's OWN close. In the incident shape (loop followed by
    /// todo), it keeps the todo and every other byte outside the loop body
    /// byte-for-byte when the markers pair, and fails closed when the operator
    /// deleted the loop close or deleted through the todo body.
    #[test]
    fn componentownclose_loop_restore_never_rewrites_a_sibling_todo() {
        use agent_doc_element::sibling_span_fixture::{self as fx};
        const STRUCK: &str = "- ~~do [#oncepass]~~\n";
        for style in fx::ALL_STYLES {
            for edit in fx::ALL_EDITS {
                let doc = fx::document(style, edit).replacen(
                    fx::WATCH_ROW,
                    &format!("{}{STRUCK}", fx::WATCH_ROW),
                    1,
                );
                let roster = heads(&["do [#oncepass]"]);
                let label = format!("{style:?}/{edit:?}");
                let planned = plan_loop_restore_for_queue(&doc, "issues", &roster);
                if !fx::edit_is_structurally_valid(edit) {
                    let error = planned.expect_err("a broken loop close must refuse the restore");
                    assert!(
                        format!("{error:#}").contains("agent:loop"),
                        "{label}: {error:#}"
                    );
                    continue;
                }
                let restored = planned
                    .unwrap()
                    .unwrap_or_else(|| panic!("{label}: no restore"));
                assert!(!restored.contains(STRUCK), "{label}");
                assert!(
                    restored.contains("- do [#oncepass]\n<!-- /agent:loop -->\n"),
                    "{label}:\n{restored}"
                );
                let todo = fx::todo_region(style);
                assert_eq!(restored.matches(&todo).count(), 1, "{label}");
                assert!(restored.ends_with(fx::tail_region()), "{label}");
                let close = "<!-- /agent:loop -->\n";
                assert_eq!(
                    &restored[restored.find(close).unwrap()..],
                    &doc[doc.find(close).unwrap()..],
                    "{label}: every byte after the loop close is untouched"
                );
            }
        }
    }
}
