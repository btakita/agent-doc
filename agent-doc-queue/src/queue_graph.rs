//! Identified multi-queue parsing and dependency-aware scheduling.
//!
//! Queue blocks are independent scheduling nodes. Every block's id comes from
//! the one shared rule in [`agent_doc_element_queue::queue_identity`]
//! (`#idlessqueueidentity`): an explicit `id=` wins; the legacy single unkeyed
//! block is the `default` queue; in a multi-queue document an id-less block
//! that is the document's one `agent:run` or one `agent:loop` is named by its
//! kind (`run` / `loop`). Only kind-ambiguous omissions (two id-less runs) get
//! preflight's persisted `queue-N` surrogates before this graph is read.
//! Identified blocks may wait for all predecessors with `after=a,b`.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element::{self, Component};
use anyhow::Result;

use crate::document_queue::{self, QueueEntry};
use agent_doc_element_queue::queue_identity::{
    AmbiguousQueueId, QueueIdentityInput, QueueKind, ResolvedQueueId, resolve_queue_ids,
};

pub const DEFAULT_QUEUE_ID: &str = agent_doc_element_queue::queue_identity::LEGACY_SINGLE_QUEUE_ID;
pub const SURROGATE_QUEUE_ID_PREFIX: &str = "queue";

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(String);

impl QueueId {
    pub fn parse(value: impl Into<String>) -> std::result::Result<Self, QueueGraphError> {
        let value = value.into();
        validate_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for QueueId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for QueueId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueGraphError {
    /// A queue component whose canonical id cannot be resolved
    /// (`#idlessqueueidentity`): see
    /// [`agent_doc_element_queue::queue_identity`].
    MissingId {
        occurrence: usize,
        /// The component's kind spelling (`run` / `loop`).
        kind: &'static str,
        /// 1-based line of the component's opening marker.
        line: usize,
        reason: MissingIdReason,
    },
    InvalidId {
        value: String,
    },
    DuplicateId {
        id: String,
        occurrences: Vec<usize>,
    },
    EmptyAfter {
        id: String,
    },
    SelfEdge {
        id: String,
    },
    MissingPredecessor {
        id: String,
        predecessor: String,
    },
    Cycle {
        path: Vec<String>,
    },
    InvalidBody {
        id: String,
        message: String,
    },
}

/// Why [`QueueGraphError::MissingId`] could not resolve a queue id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingIdReason {
    /// A bare `id`, `id=`, or `id=true` in a multi-queue document.
    MalformedId,
    /// Several queues share the kind (1-based marker lines of all of them).
    SharedKind { lines: Vec<usize> },
    /// The kind id is an explicit `id=` of the queue at `owner_line`.
    KindIdTaken { owner_line: usize },
}

impl std::fmt::Display for QueueGraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingId {
                occurrence,
                kind,
                line,
                reason,
            } => match reason {
                MissingIdReason::MalformedId => write!(
                    formatter,
                    "agent:{kind} queue (occurrence {occurrence}, line {line}) has a bare or empty `id`; \
                     write a nonempty `id=<name>`, or delete the attribute so the queue is named by its kind"
                ),
                MissingIdReason::SharedKind { lines } => write!(
                    formatter,
                    "agent:{kind} queue (occurrence {occurrence}, line {line}) has no `id=`, and the document has {} `{kind}` queues (lines {}), so `{kind}` cannot name it. \
                     Give each `{kind}` queue a distinct `id=` (for example `<!-- agent:{kind} id=release -->`); \
                     an id-less queue is named by its kind only when it is the document's one `agent:run` or one `agent:loop`",
                    lines.len(),
                    lines
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                MissingIdReason::KindIdTaken { owner_line } => write!(
                    formatter,
                    "agent:{kind} queue (occurrence {occurrence}, line {line}) has no `id=`, and its kind id `{kind}` is already the explicit `id={kind}` of the queue at line {owner_line}. \
                     Give this queue its own `id=`, or rename the other queue's id"
                ),
            },
            Self::InvalidId { value } => write!(
                formatter,
                "invalid agent:queue id `{value}`; use an ASCII letter/digit followed by letters, digits, `.`, `_`, or `-`"
            ),
            Self::DuplicateId { id, occurrences } => write!(
                formatter,
                "duplicate agent:queue id `{id}` at occurrences {occurrences:?}; every queue component needs a unique `id=`"
            ),
            Self::EmptyAfter { id } => write!(
                formatter,
                "agent:queue id `{id}` has bare or empty `after`; use `after=queue-id`"
            ),
            Self::SelfEdge { id } => {
                write!(formatter, "agent:queue id `{id}` cannot be after itself")
            }
            Self::MissingPredecessor { id, predecessor } => write!(
                formatter,
                "agent:queue id `{id}` is after missing queue id `{predecessor}`"
            ),
            Self::Cycle { path } => write!(
                formatter,
                "agent:queue dependency cycle: {}",
                path.join(" -> ")
            ),
            Self::InvalidBody { id, message } => {
                write!(
                    formatter,
                    "failed to parse agent:queue id `{id}`: {message}"
                )
            }
        }
    }
}

impl std::error::Error for QueueGraphError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueNodeState {
    Empty,
    Ready,
    Paused,
    Deferred,
    InFlight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedQueue {
    pub id: String,
    pub waiting_for: Vec<String>,
    pub state: QueueNodeState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueScheduleDecision {
    Idle,
    Blocked { queues: Vec<BlockedQueue> },
    Ready { queue_id: String, occurrence: usize },
    InFlight { queue_id: String, occurrence: usize },
}

#[derive(Debug, Clone)]
pub struct QueueBlock<'a> {
    pub id: String,
    pub dependencies: Vec<String>,
    pub occurrence: usize,
    pub component: &'a Component,
    pub entries: Vec<QueueEntry>,
}

impl QueueBlock<'_> {
    pub fn live_prompt_count(&self) -> usize {
        document_queue::prompts(&self.entries).len()
            + self
                .entries
                .iter()
                .filter(|entry| {
                    matches!(entry, QueueEntry::Freeform(line) if !document_queue::is_noise_freeform_line(line))
                })
                .count()
    }

    pub fn is_drained(&self) -> bool {
        self.live_prompt_count() == 0
    }

    pub fn state(&self) -> QueueNodeState {
        if self.is_drained() {
            QueueNodeState::Empty
        } else {
            QueueNodeState::Ready
        }
    }

    /// Owner-relative readiness. A claimed prompt does not keep the queue
    /// ready; a non-noise freeform line does. `Paused` still has live work,
    /// so it is not `Empty` and an `after=` dependent keeps waiting.
    pub fn state_excluding_claimed(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
    ) -> QueueNodeState {
        self.state_excluding_claimed_with_control(claimed, None)
    }

    /// [`Self::state_excluding_claimed`] that also honors a `pause` control.
    ///
    /// A marker `pause` or legacy `stop` holds the queue. Frontmatter
    /// `queue: pause` holds it when the marker has no token; a marker `go`
    /// or `start` overrides that. An empty queue stays `Empty`, so `after=`
    /// does not wait on a drained predecessor. A held queue with live heads
    /// stays `Paused`, not `Empty`.
    pub fn state_excluding_claimed_with_control(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> QueueNodeState {
        if self.is_drained() {
            return QueueNodeState::Empty;
        }
        if crate::control_binding::explicit_queue_pause_mode(
            &self.component.attrs,
            frontmatter_queue,
        ) {
            return QueueNodeState::Paused;
        }
        // `#nestedclaimsteer`: a child split out of a claimed parent is its own
        // head. Judge availability on that view when a steer guard is installed.
        // `#nestlistlineage`: a nested leaf's claim key is its nest vector, so
        // the match reads the full list before any row is dropped.
        let entries = crate::nested_claim_steer::expand_slice_if_installed(&self.entries);
        let agent_available = document_queue::executable_prompt_indexes(entries.as_ref())
            .into_iter()
            .any(|index| {
                let QueueEntry::Prompt(prompt) = &entries[index] else {
                    return false;
                };
                !claimed.claims_at(entries.as_ref(), index)
                    && agent_doc_document::queue_projection::operator_action_ids(&prompt.text)
                        .is_empty()
            })
            || entries.iter().any(|entry| {
                matches!(entry, QueueEntry::Freeform(line) if !document_queue::is_noise_freeform_line(line))
            });
        if agent_available {
            QueueNodeState::Ready
        } else {
            // Claimed, operator-gated, or group-label work stays live for
            // `after=` but must not monopolize selection ahead of an
            // independent queue that still has runnable heads.
            QueueNodeState::Paused
        }
    }

    /// Every agent-available head is subagent work, so an in-session drain
    /// has nothing to run here. Opt-out tags (`[inline]`, `[operator-verify]`)
    /// and a queue without subagent intent keep the block inline.
    fn is_delegated_only(&self, claimed: &crate::queue_claim::ClaimedQueueItems) -> bool {
        let entries = crate::nested_claim_steer::expand_slice_if_installed(&self.entries);
        let available = document_queue::executable_prompt_indexes(entries.as_ref())
            .into_iter()
            .filter_map(|index| {
                let QueueEntry::Prompt(prompt) = &entries[index] else {
                    return None;
                };
                if claimed.claims_at(entries.as_ref(), index)
                    || !agent_doc_document::queue_projection::operator_action_ids(&prompt.text)
                        .is_empty()
                {
                    return None;
                }
                Some(prompt)
            })
            .collect::<Vec<_>>();
        if available.is_empty() {
            return false;
        }
        let queue_subagents =
            crate::subagent_intent::queue_subagents_mode(&self.component.attrs).is_some();
        available.iter().all(|prompt| {
            if queue_subagents {
                !crate::subagent_intent::opts_out_of_queue_subagents(&prompt.text)
            } else {
                crate::subagent_intent::carries_subagent_intent_tag(&prompt.text)
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct QueueGraph<'a> {
    blocks: Vec<QueueBlock<'a>>,
    by_id: HashMap<String, usize>,
}

pub type QueueSet<'a> = QueueGraph<'a>;

impl<'a> QueueGraph<'a> {
    pub fn parse(
        content: &str,
        components: &'a [Component],
    ) -> std::result::Result<Self, QueueGraphError> {
        let queue_components = components
            .iter()
            .filter(|component| component.name == "queue")
            .collect::<Vec<_>>();
        let resolved = resolve_component_queue_ids(&queue_components);
        let mut blocks: Vec<QueueBlock<'a>> = Vec::new();
        let mut by_id = HashMap::new();
        for (occurrence, component) in queue_components.iter().copied().enumerate() {
            let id = match resolved[occurrence].id() {
                Some(id) => id.to_string(),
                None => {
                    return Err(missing_id_error(
                        content,
                        &queue_components,
                        occurrence,
                        &resolved[occurrence],
                    ));
                }
            };
            validate_id(&id)?;
            if let Some(first) = by_id.insert(id.clone(), blocks.len()) {
                return Err(QueueGraphError::DuplicateId {
                    id,
                    occurrences: vec![blocks[first].occurrence, occurrence],
                });
            }
            let dependencies = parse_dependencies(component.attrs.get("after"), &id)?;
            let entries = document_queue::parse(component.content(content)).map_err(|error| {
                QueueGraphError::InvalidBody {
                    id: id.clone(),
                    message: error.to_string(),
                }
            })?;
            blocks.push(QueueBlock {
                id,
                dependencies,
                occurrence,
                component,
                entries,
            });
        }

        for block in &blocks {
            for dependency in &block.dependencies {
                if !by_id.contains_key(dependency) {
                    return Err(QueueGraphError::MissingPredecessor {
                        id: block.id.clone(),
                        predecessor: dependency.clone(),
                    });
                }
            }
        }
        validate_acyclic(&blocks, &by_id)?;
        Ok(Self { blocks, by_id })
    }

    pub fn from_document(
        content: &'a str,
        components: &'a [Component],
    ) -> std::result::Result<Self, QueueGraphError> {
        Self::parse(content, components)
    }

    pub fn blocks(&self) -> &[QueueBlock<'a>] {
        &self.blocks
    }

    pub fn block(&self, id: &str) -> Option<&QueueBlock<'a>> {
        self.by_id.get(id).map(|index| &self.blocks[*index])
    }

    pub fn selected(&self) -> Option<&QueueBlock<'a>> {
        match self.schedule() {
            QueueScheduleDecision::Ready { queue_id, .. } => self.block(&queue_id),
            QueueScheduleDecision::Idle
            | QueueScheduleDecision::Blocked { .. }
            | QueueScheduleDecision::InFlight { .. } => None,
        }
    }

    /// Select the first queue with work available to this drain owner.
    ///
    /// A queue whose control is `pause` (a legacy marker `stop` included), or
    /// whose every executable head is claimed, operator-gated (`🙋`), or only
    /// a non-executable group label, is paused, not empty: independent later
    /// queues remain selectable, while an `after=` dependent still waits for
    /// the externally owned work to finish.
    pub fn selected_excluding_claimed(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
    ) -> Option<&QueueBlock<'a>> {
        self.selected_excluding_claimed_with_control(claimed, None)
    }

    /// [`Self::selected_excluding_claimed`] with the document's frontmatter
    /// `queue:` control. A marker token still overrides that control.
    pub fn selected_excluding_claimed_with_control(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> Option<&QueueBlock<'a>> {
        match self.schedule_excluding_claimed_with_control(claimed, frontmatter_queue) {
            QueueScheduleDecision::Ready { queue_id, .. } => self.block(&queue_id),
            QueueScheduleDecision::Idle
            | QueueScheduleDecision::Blocked { .. }
            | QueueScheduleDecision::InFlight { .. } => None,
        }
    }

    pub fn schedule_excluding_claimed(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
    ) -> QueueScheduleDecision {
        self.schedule_excluding_claimed_with_control(claimed, None)
    }

    pub fn schedule_excluding_claimed_with_control(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> QueueScheduleDecision {
        let runtime_states = self
            .blocks
            .iter()
            .map(|block| {
                (
                    block.id.clone(),
                    block.state_excluding_claimed_with_control(claimed, frontmatter_queue),
                )
            })
            .collect();
        self.schedule_with_states(&runtime_states)
    }

    /// In-session selection. A queue whose remaining heads are all
    /// subagent-delegated stays the dispatch owner ([`Self::selected_excluding_claimed`])
    /// but is paused when a later queue has inline work, so an open scheduled
    /// loop is not hidden behind workers that are already running. A later
    /// queue that is itself only delegated does not count: one cycle does not
    /// skip from the first subagent run to the next. `after=` still waits.
    pub fn selected_reaching_inline_work(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
    ) -> Option<&QueueBlock<'a>> {
        self.selected_reaching_inline_work_with_control(claimed, None)
    }

    /// [`Self::selected_reaching_inline_work`] with the document's frontmatter
    /// `queue:` control. A marker token still overrides that control.
    pub fn selected_reaching_inline_work_with_control(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> Option<&QueueBlock<'a>> {
        match self.schedule_reaching_inline_work_with_control(claimed, frontmatter_queue) {
            QueueScheduleDecision::Ready { queue_id, .. } => self.block(&queue_id),
            QueueScheduleDecision::Idle
            | QueueScheduleDecision::Blocked { .. }
            | QueueScheduleDecision::InFlight { .. } => None,
        }
    }

    pub fn schedule_reaching_inline_work(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
    ) -> QueueScheduleDecision {
        self.schedule_reaching_inline_work_with_control(claimed, None)
    }

    pub fn schedule_reaching_inline_work_with_control(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> QueueScheduleDecision {
        self.schedule_with_states(&self.inline_work_states(claimed, frontmatter_queue))
    }

    fn inline_work_states(
        &self,
        claimed: &crate::queue_claim::ClaimedQueueItems,
        frontmatter_queue: Option<&str>,
    ) -> HashMap<String, QueueNodeState> {
        let annotated = self
            .blocks
            .iter()
            .map(|block| {
                let state =
                    block.state_excluding_claimed_with_control(claimed, frontmatter_queue);
                let delegated = state == QueueNodeState::Ready && block.is_delegated_only(claimed);
                (block.id.clone(), state, delegated)
            })
            .collect::<Vec<_>>();
        annotated
            .iter()
            .enumerate()
            .map(|(index, (id, state, delegated))| {
                let later_inline = annotated[index + 1..].iter().any(
                    |(_, later_state, later_delegated)| {
                        *later_state == QueueNodeState::Ready && !*later_delegated
                    },
                );
                let state = if *delegated && later_inline {
                    QueueNodeState::Paused
                } else {
                    state.clone()
                };
                (id.clone(), state)
            })
            .collect()
    }

    pub fn schedule(&self) -> QueueScheduleDecision {
        self.schedule_with_states(&HashMap::new())
    }

    /// Combine graph readiness with live pause/defer/claim evidence. Dependency
    /// edges are satisfied only by `Empty`; paused or deferred work remains
    /// live, and an already-dispatched head stays sticky across predecessor
    /// refills until its exact receipt retires the in-flight state.
    pub fn schedule_with_states(
        &self,
        runtime_states: &HashMap<String, QueueNodeState>,
    ) -> QueueScheduleDecision {
        let state_for = |block: &QueueBlock<'_>| {
            runtime_states
                .get(&block.id)
                .cloned()
                .unwrap_or_else(|| block.state())
        };

        if let Some(block) = self
            .blocks
            .iter()
            .find(|block| state_for(block) == QueueNodeState::InFlight)
        {
            return QueueScheduleDecision::InFlight {
                queue_id: block.id.clone(),
                occurrence: block.occurrence,
            };
        }

        let mut blocked = Vec::new();
        for block in &self.blocks {
            let state = state_for(block);
            if state == QueueNodeState::Empty {
                continue;
            }
            let waiting_for = block
                .dependencies
                .iter()
                .filter(|dependency| {
                    self.block(dependency)
                        .is_some_and(|predecessor| state_for(predecessor) != QueueNodeState::Empty)
                })
                .cloned()
                .collect::<Vec<_>>();
            if waiting_for.is_empty() && state == QueueNodeState::Ready {
                return QueueScheduleDecision::Ready {
                    queue_id: block.id.clone(),
                    occurrence: block.occurrence,
                };
            }
            blocked.push(BlockedQueue {
                id: block.id.clone(),
                waiting_for,
                state,
            });
        }
        if blocked.is_empty() {
            QueueScheduleDecision::Idle
        } else {
            QueueScheduleDecision::Blocked { queues: blocked }
        }
    }

    pub fn all_drained(&self) -> bool {
        self.blocks.iter().all(QueueBlock::is_drained)
    }

    pub fn live_prompt_count(&self) -> usize {
        self.blocks.iter().map(QueueBlock::live_prompt_count).sum()
    }
}

/// What a queue component is (`#idlessqueueidentity`): a repeating
/// `agent:loop` (or legacy `agent:run loop` / `agent:queue loop`), else a
/// one-shot `agent:run`.
pub fn queue_kind(component: &Component) -> QueueKind {
    QueueKind::from_loop_flag(is_loop_queue(component))
}

/// Every queue component's canonical identity, index-aligned with
/// `queue_components`. The single resolver every queue-id reader shares.
fn resolve_component_queue_ids(queue_components: &[&Component]) -> Vec<ResolvedQueueId> {
    let inputs = queue_components
        .iter()
        .map(|component| QueueIdentityInput {
            explicit_id: component.attrs.get("id").map(String::as_str),
            kind: queue_kind(component),
        })
        .collect::<Vec<_>>();
    resolve_queue_ids(&inputs)
}

/// 1-based line of `component`'s opening marker.
fn marker_line(content: &str, component: &Component) -> usize {
    content
        .get(..component.open_start)
        .map_or(0, |prefix| prefix.matches('\n').count())
        + 1
}

fn missing_id_error(
    content: &str,
    queue_components: &[&Component],
    occurrence: usize,
    resolved: &ResolvedQueueId,
) -> QueueGraphError {
    let component = queue_components[occurrence];
    let reason = match resolved {
        ResolvedQueueId::Ambiguous(AmbiguousQueueId::SharedKind { siblings }) => {
            MissingIdReason::SharedKind {
                lines: siblings
                    .iter()
                    .map(|index| marker_line(content, queue_components[*index]))
                    .collect(),
            }
        }
        ResolvedQueueId::Ambiguous(AmbiguousQueueId::KindIdTaken { owner }) => {
            MissingIdReason::KindIdTaken {
                owner_line: marker_line(content, queue_components[*owner]),
            }
        }
        _ => MissingIdReason::MalformedId,
    };
    QueueGraphError::MissingId {
        occurrence,
        kind: queue_kind(component).as_str(),
        line: marker_line(content, component),
        reason,
    }
}

/// Backfill omitted queue ids before a multi-queue document enters the queue
/// graph. A lone omitted id remains the legacy `default` queue, and an omitted
/// id whose component is the document's one `run` or one `loop` is named by
/// its kind and left id-free (`#idlessqueueidentity`); only the remaining,
/// kind-ambiguous omissions get surrogates.
///
/// Surrogates are based on one-based document order (`queue-1`, `queue-2`, …).
/// If that name is already explicit or was generated earlier in the same pass,
/// deterministic `-2`, `-3`, … suffix probing finds the first unused name.
/// Once this returned document is persisted, later insertions and reorderings
/// preserve every existing id because normalization only touches markers whose
/// `id` attribute is absent.
///
/// Bare (`id`) and empty (`id=`) attributes are explicit malformed ids, not
/// omissions. They continue to fail closed, as do invalid and duplicate
/// explicit ids.
pub fn backfill_missing_queue_ids(
    content: &str,
    components: &[Component],
) -> std::result::Result<Option<String>, QueueGraphError> {
    let queue_components = components
        .iter()
        .filter(|component| component.name == "queue")
        .collect::<Vec<_>>();
    if queue_components.len() <= 1 {
        return Ok(None);
    }

    let resolved = resolve_component_queue_ids(&queue_components);
    let mut used = HashMap::<String, usize>::new();
    let mut missing = Vec::new();
    for (occurrence, component) in queue_components.iter().enumerate() {
        let Some(raw_id) = component.attrs.get("id") else {
            match &resolved[occurrence] {
                // `#idlessqueueidentity`: a queue that is its document's one
                // `run` or one `loop` is named by its kind and keeps its
                // id-free marker. Only kind-ambiguous omissions get surrogates.
                ResolvedQueueId::Derived(kind) => {
                    used.insert(kind.as_str().to_string(), occurrence);
                }
                _ => missing.push((occurrence, *component)),
            }
            continue;
        };
        let id = raw_id.trim();
        if id.is_empty() || id == "true" {
            return Err(QueueGraphError::InvalidId {
                value: raw_id.clone(),
            });
        }
        validate_id(id)?;
        if let Some(first) = used.insert(id.to_string(), occurrence) {
            return Err(QueueGraphError::DuplicateId {
                id: id.to_string(),
                occurrences: vec![first, occurrence],
            });
        }
    }
    if missing.is_empty() {
        return Ok(None);
    }

    let mut generated = HashMap::<usize, String>::new();
    for (occurrence, component) in missing {
        let base = format!("{SURROGATE_QUEUE_ID_PREFIX}-{}", occurrence + 1);
        let mut candidate = base.clone();
        let mut probe = 2usize;
        while used.contains_key(&candidate) {
            candidate = format!("{base}-{probe}");
            probe += 1;
        }
        used.insert(candidate.clone(), occurrence);
        generated.insert(component.open_start, candidate);
    }

    let mut normalized = String::with_capacity(content.len() + generated.len() * 16);
    let mut cursor = 0usize;
    for component in queue_components {
        let Some(id) = generated.get(&component.open_start) else {
            continue;
        };
        normalized.push_str(&content[cursor..component.open_start]);
        normalized.push_str(&insert_marker_attribute_preserving_bytes(
            &content[component.open_start..component.open_end],
            "id",
            id,
        ));
        cursor = component.open_end;
    }
    normalized.push_str(&content[cursor..]);
    Ok(Some(normalized))
}

fn insert_marker_attribute_preserving_bytes(marker: &str, key: &str, value: &str) -> String {
    let Some(close) = marker.rfind("-->") else {
        return marker.to_string();
    };
    let before_close = &marker[..close];
    let separator = if before_close.chars().last().is_some_and(char::is_whitespace) {
        ""
    } else {
        " "
    };
    format!(
        "{before_close}{separator}{key}={value} {}",
        &marker[close..]
    )
}

pub fn parse_document(content: &str) -> Result<(Vec<Component>, Option<usize>)> {
    let components = element::parse(content)?;
    let selected_occurrence = QueueSet::parse(content, &components)?
        .selected()
        .map(|block| block.occurrence);
    Ok((components, selected_occurrence))
}

pub fn selected_component<'a>(
    content: &str,
    components: &'a [Component],
) -> Result<Option<&'a Component>> {
    Ok(QueueSet::parse(content, components)?
        .selected()
        .map(|block| block.component))
}

/// Whether `component` is a repeating queue (`agent:loop`, or a legacy
/// `agent:run loop` / `agent:queue loop`). Loop rows are operator-authored
/// recurring work; binary-originated items never land in one
/// (`#loopnoinsert`).
pub fn is_loop_queue(component: &Component) -> bool {
    component.name == "queue"
        && component
            .attrs
            .contains_key(crate::queue_schedule::QUEUE_LOOP_ATTR)
}

/// `#loopnoinsert`: the queue a binary-originated insertion (queue add,
/// worker follow-ups, backlog mirror, reopen/requeue) lands in.
///
/// The selected queue when it is a non-repeating `agent:run`; otherwise the
/// first non-repeating queue in document order. `None` when the document has
/// only loops (or no queue): callers refuse rather than writing into a loop,
/// because a loop pass keeps every row it ran as recurring work.
pub fn insertion_component<'a>(
    content: &str,
    components: &'a [Component],
) -> Result<Option<&'a Component>> {
    let queues = QueueSet::parse(content, components)?;
    if let Some(selected) = queues.selected()
        && !is_loop_queue(selected.component)
    {
        return Ok(Some(selected.component));
    }
    Ok(queues
        .blocks()
        .iter()
        .map(|block| block.component)
        .find(|component| !is_loop_queue(component)))
}

/// [`insertion_component`]'s queue id.
pub fn insertion_queue_id(content: &str, components: &[Component]) -> Result<Option<String>> {
    let queues = QueueSet::parse(content, components)?;
    if let Some(selected) = queues.selected()
        && !is_loop_queue(selected.component)
    {
        return Ok(Some(selected.id.clone()));
    }
    Ok(queues
        .blocks()
        .iter()
        .find(|block| !is_loop_queue(block.component))
        .map(|block| block.id.clone()))
}

pub fn component_for_id<'a>(
    content: &str,
    components: &'a [Component],
    id: &str,
) -> Result<Option<&'a Component>> {
    Ok(QueueSet::parse(content, components)?
        .block(id)
        .map(|block| block.component))
}

fn validate_id(id: &str) -> std::result::Result<(), QueueGraphError> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err(QueueGraphError::InvalidId {
            value: id.to_string(),
        });
    };
    if !first.is_ascii_alphanumeric()
        || !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(QueueGraphError::InvalidId {
            value: id.to_string(),
        });
    }
    Ok(())
}

fn parse_dependencies(
    raw: Option<&String>,
    owner: &str,
) -> std::result::Result<Vec<String>, QueueGraphError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    if raw.trim().is_empty() || raw == "true" {
        return Err(QueueGraphError::EmptyAfter {
            id: owner.to_string(),
        });
    }
    let mut seen = HashSet::new();
    let mut dependencies = Vec::new();
    for dependency in raw.split(',').map(str::trim) {
        if dependency.is_empty() {
            return Err(QueueGraphError::EmptyAfter {
                id: owner.to_string(),
            });
        }
        validate_id(dependency)?;
        if dependency == owner {
            return Err(QueueGraphError::SelfEdge {
                id: owner.to_string(),
            });
        }
        if seen.insert(dependency.to_string()) {
            dependencies.push(dependency.to_string());
        }
    }
    if dependencies.is_empty() {
        return Err(QueueGraphError::EmptyAfter {
            id: owner.to_string(),
        });
    }
    Ok(dependencies)
}

fn validate_acyclic(
    blocks: &[QueueBlock<'_>],
    by_id: &HashMap<String, usize>,
) -> std::result::Result<(), QueueGraphError> {
    fn visit(
        index: usize,
        blocks: &[QueueBlock<'_>],
        by_id: &HashMap<String, usize>,
        stack: &mut Vec<usize>,
        visited: &mut HashSet<usize>,
    ) -> std::result::Result<(), QueueGraphError> {
        if visited.contains(&index) {
            return Ok(());
        }
        if let Some(cycle_start) = stack.iter().position(|candidate| *candidate == index) {
            let mut path = stack[cycle_start..]
                .iter()
                .map(|candidate| blocks[*candidate].id.clone())
                .collect::<Vec<_>>();
            path.push(blocks[index].id.clone());
            return Err(QueueGraphError::Cycle { path });
        }
        stack.push(index);
        for dependency in &blocks[index].dependencies {
            visit(by_id[dependency], blocks, by_id, stack, visited)?;
        }
        stack.pop();
        visited.insert(index);
        Ok(())
    }

    let mut stack = Vec::new();
    let mut visited = HashSet::new();
    for index in 0..blocks.len() {
        visit(index, blocks, by_id, &mut stack, &mut visited)?;
    }
    Ok(())
}

/// One generation-fenced queue mutation. Producers use a batch so a recurring
/// sequence cannot expose `B` before the same run's `A` refill is visible.
#[derive(Debug, Clone)]
pub struct QueueAppendBatch {
    pub expected_generation: u64,
    pub appends: Vec<QueueAppend>,
}

#[derive(Debug, Clone)]
pub struct QueueAppend {
    pub queue_id: Option<QueueId>,
    pub create: Option<CreateQueue>,
    pub items: Vec<QueueEntry>,
}

#[derive(Debug, Clone)]
pub struct CreateQueue {
    pub id: QueueId,
    pub after: Vec<QueueId>,
    /// Queue-local attributes such as `preset` and `subagents`. `id` and
    /// `after` are reserved and rejected here so they have one typed owner.
    pub attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueAppendOutcome {
    pub generation: u64,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAppendError {
    StaleGeneration { expected: u64, actual: u64 },
    InvalidExistingGraph(QueueGraphError),
    InvalidProposedGraph(QueueGraphError),
    AmbiguousTarget { known_ids: Vec<String> },
    UnknownQueueId { id: String, known_ids: Vec<String> },
    CreateIdMismatch { target: String, create: String },
    DuplicateCreate { id: String },
    ExistingQueueCreate { id: String },
    ReservedCreateAttribute { id: String, attribute: String },
    Parse(String),
}

impl std::fmt::Display for QueueAppendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleGeneration { expected, actual } => write!(
                formatter,
                "stale queue append generation: expected {expected}, current {actual}"
            ),
            Self::InvalidExistingGraph(error) => {
                write!(formatter, "existing queue graph is invalid: {error}")
            }
            Self::InvalidProposedGraph(error) => {
                write!(formatter, "proposed queue graph is invalid: {error}")
            }
            Self::AmbiguousTarget { known_ids } => write!(
                formatter,
                "a keyed or multi-queue append requires `queue_id`; known IDs: {}",
                known_ids.join(", ")
            ),
            Self::UnknownQueueId { id, known_ids } => write!(
                formatter,
                "unknown queue id `{id}`; known IDs: {}",
                known_ids.join(", ")
            ),
            Self::CreateIdMismatch { target, create } => write!(
                formatter,
                "queue append target `{target}` does not match create id `{create}`"
            ),
            Self::DuplicateCreate { id } => {
                write!(
                    formatter,
                    "queue id `{id}` is created more than once in one batch"
                )
            }
            Self::ExistingQueueCreate { id } => {
                write!(
                    formatter,
                    "queue id `{id}` already exists and cannot be created"
                )
            }
            Self::ReservedCreateAttribute { id, attribute } => write!(
                formatter,
                "queue id `{id}` create attrs must not override reserved `{attribute}`"
            ),
            Self::Parse(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for QueueAppendError {}

/// Apply every append to one in-memory proposal, validate the complete graph,
/// then return one next-generation document. On error the input is untouched.
pub fn apply_append_batch(
    content: &str,
    current_generation: u64,
    batch: &QueueAppendBatch,
) -> std::result::Result<QueueAppendOutcome, QueueAppendError> {
    if batch.expected_generation != current_generation {
        return Err(QueueAppendError::StaleGeneration {
            expected: batch.expected_generation,
            actual: current_generation,
        });
    }

    let components =
        element::parse(content).map_err(|error| QueueAppendError::Parse(error.to_string()))?;
    let graph =
        QueueGraph::parse(content, &components).map_err(QueueAppendError::InvalidExistingGraph)?;
    let known_ids = graph
        .blocks()
        .iter()
        .map(|block| block.id.clone())
        .collect::<Vec<_>>();
    let legacy_single =
        graph.blocks().len() == 1 && !graph.blocks()[0].component.attrs.contains_key("id");

    let mut existing_items: HashMap<String, Vec<QueueEntry>> = HashMap::new();
    let mut creates: Vec<(CreateQueue, Vec<QueueEntry>)> = Vec::new();
    let mut create_ids = HashSet::new();

    for append in &batch.appends {
        let target = match append.queue_id.as_ref() {
            Some(id) => id.as_str().to_string(),
            None if legacy_single => DEFAULT_QUEUE_ID.to_string(),
            None => {
                return Err(QueueAppendError::AmbiguousTarget {
                    known_ids: known_ids.clone(),
                });
            }
        };

        if graph.block(&target).is_some() {
            if append.create.is_some() {
                return Err(QueueAppendError::ExistingQueueCreate { id: target });
            }
            existing_items
                .entry(target)
                .or_default()
                .extend(append.items.clone());
            continue;
        }

        let Some(create) = append.create.as_ref() else {
            return Err(QueueAppendError::UnknownQueueId {
                id: target,
                known_ids: known_ids.clone(),
            });
        };
        if create.id.as_str() != target {
            return Err(QueueAppendError::CreateIdMismatch {
                target,
                create: create.id.to_string(),
            });
        }
        if !create_ids.insert(target.clone()) {
            return Err(QueueAppendError::DuplicateCreate { id: target });
        }
        for (attribute, _) in &create.attrs {
            if matches!(attribute.as_str(), "id" | "after") {
                return Err(QueueAppendError::ReservedCreateAttribute {
                    id: target,
                    attribute: attribute.clone(),
                });
            }
        }
        creates.push((create.clone(), append.items.clone()));
    }

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (id, items) in existing_items {
        if items.is_empty() {
            continue;
        }
        let block = graph.block(&id).expect("resolved existing queue block");
        let body = block.component.content(content);
        let mut addition = document_queue::render(&items);
        if !body.is_empty() && !body.ends_with('\n') && !addition.is_empty() {
            addition.insert(0, '\n');
        }
        edits.push((
            block.component.close_start,
            block.component.close_start,
            addition,
        ));
    }

    if legacy_single && !creates.is_empty() {
        let component = graph.blocks()[0].component;
        let opener = &content[component.open_start..component.open_end];
        let migrated = add_marker_attribute(opener, "id", DEFAULT_QUEUE_ID);
        edits.push((component.open_start, component.open_end, migrated));
    }

    edits.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut proposed = content.to_string();
    for (start, end, replacement) in edits {
        proposed.replace_range(start..end, &replacement);
    }

    for (create, items) in creates {
        if !proposed.is_empty() && !proposed.ends_with('\n') {
            proposed.push('\n');
        }
        let mut attrs = vec![("id".to_string(), create.id.to_string())];
        if !create.after.is_empty() {
            attrs.push((
                "after".to_string(),
                create
                    .after
                    .iter()
                    .map(QueueId::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            ));
        }
        attrs.extend(create.attrs);
        proposed.push_str("<!-- agent:run");
        for (key, value) in attrs {
            proposed.push(' ');
            proposed.push_str(&key);
            proposed.push('=');
            proposed.push_str(&value);
        }
        proposed.push_str(" -->\n");
        proposed.push_str(&document_queue::render(&items));
        proposed.push_str("<!-- /agent:run -->\n");
    }

    let proposed_components =
        element::parse(&proposed).map_err(|error| QueueAppendError::Parse(error.to_string()))?;
    QueueGraph::parse(&proposed, &proposed_components)
        .map_err(QueueAppendError::InvalidProposedGraph)?;

    Ok(QueueAppendOutcome {
        generation: current_generation.saturating_add(1),
        content: proposed,
    })
}

fn add_marker_attribute(marker: &str, key: &str, value: &str) -> String {
    let Some(close) = marker.rfind("-->") else {
        return marker.to_string();
    };
    let prefix = marker[..close].trim_end();
    format!("{prefix} {key}={value} -->")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_single_unkeyed_queue_is_default() {
        let doc = "<!-- agent:queue -->\n- first\n<!-- /agent:queue -->\n";
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        let selected = queues.selected().unwrap();
        assert_eq!(selected.id, DEFAULT_QUEUE_ID);
        assert_eq!(selected.occurrence, 0);
    }

    #[test]
    fn selects_first_ready_id_and_preserves_scoped_attrs() {
        let doc = concat!(
            "<!-- agent:queue id=review subagents=2 -->\n",
            "- review now\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release preset=release after=review -->\n",
            "- publish later\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "review");
        assert_eq!(
            queues.block("review").unwrap().component.attrs["subagents"],
            "2"
        );
        assert_eq!(
            queues.block("release").unwrap().component.attrs["preset"],
            "release"
        );
    }

    #[test]
    fn dependent_release_queue_unblocks_after_predecessor_drains() {
        let doc = concat!(
            "<!-- agent:queue id=release-a -->\n",
            "~~- shipped A~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b after=release-a -->\n",
            "- organize B\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "release-b");
        assert_eq!(queues.selected().unwrap().occurrence, 1);
    }

    #[test]
    fn unowned_bare_hash_head_keeps_queue_runnable() {
        let doc = "<!-- agent:queue -->\n- #unknown-preset\n<!-- /agent:queue -->\n";
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.blocks()[0].live_prompt_count(), 1);
        assert!(queues.selected().is_some());
    }

    #[test]
    fn nonempty_dependent_queue_stays_blocked() {
        let doc = concat!(
            "<!-- agent:queue id=release-a -->\n",
            "- finish A\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b after=release-a -->\n",
            "- organize B\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let queues = QueueSet::parse(doc, &components).unwrap();
        assert_eq!(queues.selected().unwrap().id, "release-a");
    }

    #[test]
    fn rejects_duplicate_ids_missing_dependencies_and_cycles() {
        for (doc, expected) in [
            (
                concat!(
                    "<!-- agent:queue id=a -->\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue id=a -->\n<!-- /agent:queue -->\n",
                ),
                "duplicate agent:queue id",
            ),
            (
                "<!-- agent:queue id=b after=a -->\n- B\n<!-- /agent:queue -->\n",
                "is after missing queue id",
            ),
            (
                concat!(
                    "<!-- agent:queue id=a after=b -->\n- A\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue id=b after=a -->\n- B\n<!-- /agent:queue -->\n",
                ),
                "dependency cycle: a -> b -> a",
            ),
        ] {
            let components = element::parse(doc).unwrap();
            let error = QueueSet::parse(doc, &components).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn multiple_queues_require_explicit_nonempty_ids() {
        for doc in [
            concat!(
                "<!-- agent:queue id=a -->\n- A\n<!-- /agent:queue -->\n",
                "<!-- agent:queue -->\n- B\n<!-- /agent:queue -->\n",
            ),
            concat!(
                "<!-- agent:queue id=a -->\n- A\n<!-- /agent:queue -->\n",
                "<!-- agent:queue id -->\n- B\n<!-- /agent:queue -->\n",
            ),
            concat!(
                "<!-- agent:queue id=a -->\n- A\n<!-- /agent:queue -->\n",
                "<!-- agent:queue id= -->\n- B\n<!-- /agent:queue -->\n",
            ),
        ] {
            let components = element::parse(doc).unwrap();
            assert!(matches!(
                QueueGraph::parse(doc, &components),
                Err(QueueGraphError::MissingId { occurrence: 1, .. })
            ));
        }
    }

    #[test]
    fn missing_multi_queue_ids_are_backfilled_without_touching_explicit_markers() {
        let doc = concat!(
            "<!-- agent:queue   subagents -->\n- first\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-1 preset=#keep-byte-exact -->\n- second\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-1-2 -->\n- third\n<!-- /agent:queue -->\n",
            "<!-- agent:queue -->\n- fourth\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let normalized = backfill_missing_queue_ids(doc, &components)
            .unwrap()
            .expect("multi-queue omissions need persisted ids");

        assert!(normalized.contains("<!-- agent:queue   subagents id=queue-1-3 -->"));
        assert!(normalized.contains("<!-- agent:queue id=queue-1 preset=#keep-byte-exact -->"));
        assert!(normalized.contains("<!-- agent:queue id=queue-1-2 -->"));
        assert!(normalized.contains("<!-- agent:queue id=queue-4 -->"));

        let normalized_components = element::parse(&normalized).unwrap();
        let graph = QueueGraph::parse(&normalized, &normalized_components).unwrap();
        assert_eq!(
            graph
                .blocks()
                .iter()
                .map(|block| block.id.as_str())
                .collect::<Vec<_>>(),
            vec!["queue-1-3", "queue-1", "queue-1-2", "queue-4"]
        );
        assert_eq!(
            backfill_missing_queue_ids(&normalized, &normalized_components).unwrap(),
            None,
            "normalization must be a fixed point"
        );
    }

    #[test]
    fn persisted_surrogates_survive_reordering_and_new_queue_insertion() {
        let initial = concat!(
            "<!-- agent:queue -->\n- A\n<!-- /agent:queue -->\n",
            "<!-- agent:queue -->\n- B\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(initial).unwrap();
        let normalized = backfill_missing_queue_ids(initial, &components)
            .unwrap()
            .unwrap();
        assert!(normalized.contains("id=queue-1"));
        assert!(normalized.contains("id=queue-2"));

        let edited = concat!(
            "<!-- agent:queue -->\n- new\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-2 -->\n- B\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-1 -->\n- A\n<!-- /agent:queue -->\n",
        );
        let edited_components = element::parse(edited).unwrap();
        let renormalized = backfill_missing_queue_ids(edited, &edited_components)
            .unwrap()
            .unwrap();
        assert!(renormalized.starts_with("<!-- agent:queue id=queue-1-2 -->"));
        assert!(renormalized.contains("<!-- agent:queue id=queue-2 -->\n- B"));
        assert!(renormalized.contains("<!-- agent:queue id=queue-1 -->\n- A"));
    }

    #[test]
    fn malformed_and_duplicate_explicit_ids_still_fail_closed_during_backfill() {
        for (doc, expected) in [
            (
                concat!(
                    "<!-- agent:queue id -->\n- A\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue -->\n- B\n<!-- /agent:queue -->\n",
                ),
                "invalid agent:queue id",
            ),
            (
                concat!(
                    "<!-- agent:queue id=bad! -->\n- A\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue -->\n- B\n<!-- /agent:queue -->\n",
                ),
                "invalid agent:queue id",
            ),
            (
                concat!(
                    "<!-- agent:queue id=same -->\n- A\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue id=same -->\n- B\n<!-- /agent:queue -->\n",
                    "<!-- agent:queue -->\n- C\n<!-- /agent:queue -->\n",
                ),
                "duplicate agent:queue id",
            ),
        ] {
            let components = element::parse(doc).unwrap();
            let error = backfill_missing_queue_ids(doc, &components)
                .unwrap_err()
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn after_is_all_of_and_source_order_breaks_ready_ties() {
        let doc = concat!(
            "<!-- agent:queue id=a -->\n~~- A done~~\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=b -->\n~~- B done~~\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=c after=a,b -->\n- C\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=d -->\n- D\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        assert_eq!(
            graph.schedule(),
            QueueScheduleDecision::Ready {
                queue_id: "c".into(),
                occurrence: 2,
            }
        );
    }

    #[test]
    fn predecessor_refill_blocks_the_next_dependent_decision() {
        let drained = concat!(
            "<!-- agent:queue id=a -->\n~~- A done~~\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=b after=a -->\n- B run 2\n<!-- /agent:queue -->\n",
        );
        let refilled = drained.replace("~~- A done~~", "- A run 2");
        let drained_components = element::parse(drained).unwrap();
        let refilled_components = element::parse(&refilled).unwrap();
        assert_eq!(
            QueueGraph::parse(drained, &drained_components)
                .unwrap()
                .selected()
                .unwrap()
                .id,
            "b"
        );
        assert_eq!(
            QueueGraph::parse(&refilled, &refilled_components)
                .unwrap()
                .selected()
                .unwrap()
                .id,
            "a"
        );
    }

    #[test]
    fn paused_or_deferred_predecessor_is_not_drained() {
        let doc = concat!(
            "<!-- agent:queue id=a -->\n- A\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=b after=a -->\n- B\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        for state in [QueueNodeState::Paused, QueueNodeState::Deferred] {
            let decision = graph.schedule_with_states(&HashMap::from([
                ("a".to_string(), state.clone()),
                ("b".to_string(), QueueNodeState::Ready),
            ]));
            assert!(matches!(decision, QueueScheduleDecision::Blocked { .. }));
        }
    }

    #[test]
    fn claimed_queue_does_not_hide_an_independent_ready_queue() {
        let doc = concat!(
            "<!-- agent:queue id=delegated -->\n- delegated work\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=issues -->\n- poll issues\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        let claimed = crate::queue_claim::ClaimedQueueItems::none()
            .with_heads(&["delegated work".to_string()]);

        assert_eq!(
            graph.schedule_excluding_claimed(&claimed),
            QueueScheduleDecision::Ready {
                queue_id: "issues".into(),
                occurrence: 1,
            }
        );
    }

    #[test]
    fn claimed_queue_still_blocks_its_dependent() {
        let doc = concat!(
            "<!-- agent:queue id=delegated -->\n- delegated work\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=publish after=delegated -->\n- publish\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        let claimed = crate::queue_claim::ClaimedQueueItems::none()
            .with_heads(&["delegated work".to_string()]);

        assert!(matches!(
            graph.schedule_excluding_claimed(&claimed),
            QueueScheduleDecision::Blocked { .. }
        ));
    }

    /// Three `agent:run` sections: the first has only claimed work, an
    /// operator-gated folded head, and a group label. That queue is paused,
    /// so the next independent queue is the runnable work. `after=` still waits.
    #[test]
    fn operator_or_label_only_queue_does_not_hide_a_later_run() {
        let doc = concat!(
            "<!-- agent:queue id=main subagents go -->\n",
            "- ☑️ do [#reported]\n",
            "- 🙋 do [#blocked]:\n",
            "  - 🙋 merge the pull request [#subagent-operator-request-blocked]\n",
            "- do [#group]\n",
            "  - do [#child]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=api-prs subagents -->\n",
            "- https://example.com/pull/759\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=contracts-prs subagents -->\n",
            "- https://example.com/pull/162\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        let claimed = crate::queue_claim::ClaimedQueueItems::none()
            .with_heads(&["☑️ do [#reported]".to_string(), "do [#child]".to_string()]);

        assert_eq!(
            graph.schedule(),
            QueueScheduleDecision::Ready {
                queue_id: "main".into(),
                occurrence: 0,
            }
        );
        assert_eq!(
            graph.schedule_excluding_claimed(&claimed),
            QueueScheduleDecision::Ready {
                queue_id: "api-prs".into(),
                occurrence: 1,
            }
        );

        let waiting = concat!(
            "<!-- agent:queue id=main subagents go -->\n",
            "- 🙋 do [#blocked]:\n",
            "  - 🙋 merge the pull request [#subagent-operator-request-blocked]\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=publish after=main -->\n",
            "- publish the release\n",
            "<!-- /agent:queue -->\n",
        );
        let waiting_components = element::parse(waiting).unwrap();
        let waiting_graph = QueueGraph::parse(waiting, &waiting_components).unwrap();
        assert!(matches!(
            waiting_graph
                .schedule_excluding_claimed(&crate::queue_claim::ClaimedQueueItems::none()),
            QueueScheduleDecision::Blocked { .. }
        ));
    }

    /// A `subagents` queue stays the dispatch owner, and an open later queue
    /// with inline work is the in-session selection. A closed later head
    /// (passed in the claim set) does not pull selection off the delegated
    /// queue. Two delegated runs do not skip from the first to the second.
    #[test]
    fn delegated_queue_yields_to_a_later_inline_queue() {
        let doc = concat!(
            "<!-- agent:queue id=queue-1 subagents go -->\n",
            "- delegated main work\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-2 go -->\n",
            "- #upgrade\n",
            "<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        let none = crate::queue_claim::ClaimedQueueItems::none();
        assert_eq!(
            graph.schedule_excluding_claimed(&none),
            QueueScheduleDecision::Ready {
                queue_id: "queue-1".into(),
                occurrence: 0,
            }
        );
        assert_eq!(
            graph.schedule_reaching_inline_work(&none),
            QueueScheduleDecision::Ready {
                queue_id: "queue-2".into(),
                occurrence: 1,
            }
        );
        let held = none.clone().with_heads(&["#upgrade".to_string()]);
        assert_eq!(
            graph.schedule_reaching_inline_work(&held),
            QueueScheduleDecision::Ready {
                queue_id: "queue-1".into(),
                occurrence: 0,
            }
        );

        let both = concat!(
            "<!-- agent:queue id=api-prs subagents -->\n",
            "- https://example.com/pull/759\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=contracts-prs subagents -->\n",
            "- https://example.com/pull/162\n",
            "<!-- /agent:queue -->\n",
        );
        let both_components = element::parse(both).unwrap();
        let both_graph = QueueGraph::parse(both, &both_components).unwrap();
        assert_eq!(
            both_graph.schedule_reaching_inline_work(&none),
            QueueScheduleDecision::Ready {
                queue_id: "api-prs".into(),
                occurrence: 0,
            }
        );

        let dependent = concat!(
            "<!-- agent:queue id=queue-1 subagents go -->\n",
            "- delegated main work\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=queue-2 after=queue-1 go -->\n",
            "- #upgrade\n",
            "<!-- /agent:queue -->\n",
        );
        let dependent_components = element::parse(dependent).unwrap();
        let dependent_graph = QueueGraph::parse(dependent, &dependent_components).unwrap();
        assert!(matches!(
            dependent_graph.schedule_reaching_inline_work(&none),
            QueueScheduleDecision::Blocked { .. }
        ));
    }

    /// A `pause` control (legacy marker `stop` included) holds that run and
    /// leaves the other runs selectable. The held run is not empty, so an
    /// `after=` dependent still waits. Frontmatter `queue: pause` holds every
    /// queue whose marker does not override it.
    #[test]
    fn paused_control_does_not_hide_later_runs() {
        let doc = concat!(
            "<!-- agent:run id=first pause -->\n- held work\n<!-- /agent:run -->\n",
            "<!-- agent:run id=second -->\n- second work\n<!-- /agent:run -->\n",
            "<!-- agent:run id=third -->\n- third work\n<!-- /agent:run -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        let none = crate::queue_claim::ClaimedQueueItems::none();
        assert_eq!(
            graph.block("first").unwrap().state_excluding_claimed(&none),
            QueueNodeState::Paused
        );
        assert_ne!(
            graph.block("first").unwrap().state(),
            QueueNodeState::Empty
        );
        assert_eq!(
            graph.schedule_excluding_claimed(&none),
            QueueScheduleDecision::Ready {
                queue_id: "second".into(),
                occurrence: 1,
            }
        );
        assert_eq!(
            graph.schedule_reaching_inline_work(&none),
            QueueScheduleDecision::Ready {
                queue_id: "second".into(),
                occurrence: 1,
            }
        );

        let stopped = doc.replace("id=first pause", "id=first stop");
        let stopped_components = element::parse(&stopped).unwrap();
        let stopped_graph = QueueGraph::parse(&stopped, &stopped_components).unwrap();
        assert_eq!(
            stopped_graph
                .block("first")
                .unwrap()
                .state_excluding_claimed(&none),
            QueueNodeState::Paused
        );

        let waiting = concat!(
            "<!-- agent:run id=first pause -->\n- held work\n<!-- /agent:run -->\n",
            "<!-- agent:run id=only after=first -->\n- dependent\n<!-- /agent:run -->\n",
        );
        let waiting_components = element::parse(waiting).unwrap();
        let waiting_graph = QueueGraph::parse(waiting, &waiting_components).unwrap();
        assert!(matches!(
            waiting_graph.schedule_excluding_claimed(&none),
            QueueScheduleDecision::Blocked { .. }
        ));

        let frontmatter = concat!(
            "---\nqueue: pause\n---\n",
            "<!-- agent:run id=first -->\n- held work\n<!-- /agent:run -->\n",
            "<!-- agent:run id=second go -->\n- second work\n<!-- /agent:run -->\n",
            "<!-- agent:run id=third -->\n- third held\n<!-- /agent:run -->\n",
        );
        let frontmatter_components = element::parse(frontmatter).unwrap();
        let frontmatter_graph =
            QueueGraph::parse(frontmatter, &frontmatter_components).unwrap();
        assert_eq!(
            frontmatter_graph.schedule_reaching_inline_work_with_control(&none, Some("pause")),
            QueueScheduleDecision::Ready {
                queue_id: "second".into(),
                occurrence: 1,
            }
        );
    }

    #[test]
    fn in_flight_dependent_is_sticky_across_predecessor_refill() {
        let doc = concat!(
            "<!-- agent:queue id=a -->\n- A refilled\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=b after=a -->\n- B next\n<!-- /agent:queue -->\n",
        );
        let components = element::parse(doc).unwrap();
        let graph = QueueGraph::parse(doc, &components).unwrap();
        assert_eq!(
            graph.schedule_with_states(&HashMap::from([(
                "b".to_string(),
                QueueNodeState::InFlight,
            )])),
            QueueScheduleDecision::InFlight {
                queue_id: "b".into(),
                occurrence: 1,
            }
        );
    }

    fn prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(document_queue::QueuePrompt::new(text))
    }

    #[test]
    fn batch_atomically_migrates_legacy_default_when_creating_a_second_queue() {
        let doc = "<!-- agent:queue -->\n- existing\n<!-- /agent:queue -->\n";
        let outcome = apply_append_batch(
            doc,
            7,
            &QueueAppendBatch {
                expected_generation: 7,
                appends: vec![QueueAppend {
                    queue_id: Some(QueueId::parse("publish").unwrap()),
                    create: Some(CreateQueue {
                        id: QueueId::parse("publish").unwrap(),
                        after: vec![QueueId::parse(DEFAULT_QUEUE_ID).unwrap()],
                        attrs: vec![("preset".into(), "#release".into())],
                    }),
                    items: vec![prompt("publish run 1")],
                }],
            },
        )
        .unwrap();
        assert_eq!(outcome.generation, 8);
        // Existing aliases are byte-preserved while newly-created components
        // use the canonical public marker.
        assert!(outcome.content.contains("agent:queue id=default -->"));
        assert!(
            outcome
                .content
                .contains("agent:run id=publish after=default preset=#release -->")
        );
        let components = element::parse(&outcome.content).unwrap();
        let graph = QueueGraph::parse(&outcome.content, &components).unwrap();
        assert_eq!(graph.blocks().len(), 2);
        assert_eq!(graph.selected().unwrap().id, DEFAULT_QUEUE_ID);
    }

    #[test]
    fn recurring_refill_appends_predecessor_and_dependent_in_one_generation() {
        let doc = concat!(
            "<!-- agent:queue id=build -->\n~~- build 1~~\n<!-- /agent:queue -->\n",
            "<!-- agent:queue id=publish after=build -->\n~~- publish 1~~\n<!-- /agent:queue -->\n",
        );
        let outcome = apply_append_batch(
            doc,
            11,
            &QueueAppendBatch {
                expected_generation: 11,
                appends: vec![
                    QueueAppend {
                        queue_id: Some(QueueId::parse("build").unwrap()),
                        create: None,
                        items: vec![prompt("build 2")],
                    },
                    QueueAppend {
                        queue_id: Some(QueueId::parse("publish").unwrap()),
                        create: None,
                        items: vec![prompt("publish 2")],
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(outcome.generation, 12);
        let components = element::parse(&outcome.content).unwrap();
        let graph = QueueGraph::parse(&outcome.content, &components).unwrap();
        assert_eq!(graph.selected().unwrap().id, "build");
        assert!(outcome.content.contains("- build 2"));
        assert!(outcome.content.contains("- publish 2"));
    }

    #[test]
    fn batch_rejects_stale_generation_and_invalid_final_graph_without_output() {
        let doc = "<!-- agent:queue id=build -->\n<!-- /agent:queue -->\n";
        let stale = apply_append_batch(
            doc,
            4,
            &QueueAppendBatch {
                expected_generation: 3,
                appends: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(stale, QueueAppendError::StaleGeneration { .. }));

        let invalid = apply_append_batch(
            doc,
            4,
            &QueueAppendBatch {
                expected_generation: 4,
                appends: vec![QueueAppend {
                    queue_id: Some(QueueId::parse("publish").unwrap()),
                    create: Some(CreateQueue {
                        id: QueueId::parse("publish").unwrap(),
                        after: vec![QueueId::parse("missing").unwrap()],
                        attrs: Vec::new(),
                    }),
                    items: vec![prompt("publish")],
                }],
            },
        )
        .unwrap_err();
        assert!(matches!(
            invalid,
            QueueAppendError::InvalidProposedGraph(QueueGraphError::MissingPredecessor { .. })
        ));
    }

    /// The `tasks/agent-doc/agent-doc-bugs.md` shape after the operator
    /// deleted both `id=` attributes and renamed the headings
    /// (`#idlessqueueidentity`). `ids` restores the old `main` / `issues` ids.
    pub(crate) fn idless_session_fixture(ids: bool) -> String {
        let (run_id, loop_id) = if ids {
            ("id=\"main\" ", "id=\"issues\" ")
        } else {
            ("", "")
        };
        format!(
            "---\nagent_doc_session: idless\nagent_doc_format: template\nqueue: go\n---\n\n\
## Exchange <!-- agent:exchange patch=append subagents -->\n<!-- /agent:exchange -->\n\n\
## Run <!-- agent:run {run_id}subagents preset=\"#spec-test-build-install-commit-push\" priority go -->\n\
- ☑️ [#subagent:autoupgrade#1] do [#fixautoupgrade]: keep restart-only + prompt\n\
- 🚧 [#subagent:noargparity#1] `/agent-doc` without arguments matches `/agent-doc <document-path>`\n\
- Why have the ☑️ run items not completed?\n\
- I removed id from run + loop. Ensure these items are distinct and have semantic meaning.\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop {loop_id}interval=\"1h\" priority -->\n\
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues.\n\
<!-- /agent:loop -->\n\n\
## Todo <!-- agent:todo priority -->\n<!-- /agent:todo -->\n\n\
## Backlog <!-- agent:backlog priority queue -->\n- [ ] [#fixautoupgrade] Fix the auto-upgrade.\n<!-- /agent:backlog -->\n\n\
## Review <!-- agent:review -->\n<!-- /agent:review -->\n\n\
## Icebox <!-- agent:icebox -->\n<!-- /agent:icebox -->\n\n\
## Done <!-- agent:done -->\n<!-- /agent:done -->\n"
        )
    }

    fn block_ids(doc: &str) -> Vec<String> {
        let components = element::parse(doc).unwrap();
        QueueGraph::parse(doc, &components)
            .unwrap()
            .blocks()
            .iter()
            .map(|block| block.id.clone())
            .collect()
    }

    #[test]
    fn idless_run_and_loop_are_distinct_queues_named_by_their_kind() {
        let doc = idless_session_fixture(false);
        let components = element::parse(&doc).unwrap();
        let graph = QueueGraph::parse(&doc, &components).expect("id-less run + loop must parse");
        assert_eq!(block_ids(&doc), vec!["run", "loop"]);
        let run = graph.block("run").unwrap();
        let looped = graph.block("loop").unwrap();
        assert!(!is_loop_queue(run.component));
        assert!(is_loop_queue(looped.component));
        assert_eq!(queue_kind(run.component), QueueKind::Run);
        assert_eq!(queue_kind(looped.component), QueueKind::Loop);
        assert_eq!(graph.selected().unwrap().id, "run");
        assert_eq!(
            insertion_queue_id(&doc, &components).unwrap().as_deref(),
            Some("run")
        );
        assert_eq!(
            backfill_missing_queue_ids(&doc, &components).unwrap(),
            None,
            "a kind-named queue keeps the operator's id-free marker"
        );

        // The identity is what the queue is, not where it sits.
        let run_start = doc.find("## Run").unwrap();
        let loop_start = doc.find("## Loop").unwrap();
        let todo_start = doc.find("## Todo").unwrap();
        let reordered = format!(
            "{}{}{}{}",
            &doc[..run_start],
            &doc[loop_start..todo_start],
            &doc[run_start..loop_start],
            &doc[todo_start..]
        );
        assert_eq!(block_ids(&reordered), vec!["loop", "run"]);
    }

    #[test]
    fn explicit_ids_keep_winning_in_the_same_document() {
        assert_eq!(
            block_ids(&idless_session_fixture(true)),
            vec!["main", "issues"]
        );
        let half = idless_session_fixture(true).replace("id=\"issues\" ", "");
        assert_eq!(block_ids(&half), vec!["main", "loop"]);
    }

    #[test]
    fn two_idless_runs_fail_closed_with_an_actionable_message() {
        let doc = concat!(
            "## A <!-- agent:run -->\n- a\n<!-- /agent:run -->\n",
            "## B <!-- agent:run -->\n- b\n<!-- /agent:run -->\n",
            "## L <!-- agent:loop interval=1h -->\n- l\n<!-- /agent:loop -->\n",
        );
        let components = element::parse(doc).unwrap();
        let error = QueueGraph::parse(doc, &components).unwrap_err();
        assert!(
            matches!(
                &error,
                QueueGraphError::MissingId {
                    occurrence: 0,
                    kind: "run",
                    line: 1,
                    reason: MissingIdReason::SharedKind { lines },
                } if lines == &vec![1, 4]
            ),
            "{error:?}"
        );
        let message = error.to_string();
        for needle in [
            "agent:run queue (occurrence 0, line 1) has no `id=`",
            "2 `run` queues (lines 1, 4)",
            "Give each `run` queue a distinct `id=`",
            "one `agent:run` or one `agent:loop`",
        ] {
            assert!(
                message.contains(needle),
                "{needle:?} missing from: {message}"
            );
        }

        // Preflight's backfill names only the ambiguous runs; the lone loop
        // keeps its id-free marker and its kind id.
        let normalized = backfill_missing_queue_ids(doc, &components)
            .unwrap()
            .expect("ambiguous runs get surrogates");
        assert!(normalized.contains("<!-- agent:run id=queue-1 -->"));
        assert!(normalized.contains("<!-- agent:run id=queue-2 -->"));
        assert!(normalized.contains("<!-- agent:loop interval=1h -->"));
        assert_eq!(block_ids(&normalized), vec!["queue-1", "queue-2", "loop"]);
    }

    #[test]
    fn idless_queue_whose_kind_id_is_taken_fails_closed() {
        let doc = concat!(
            "<!-- agent:loop id=run interval=1h -->\n- l\n<!-- /agent:loop -->\n",
            "<!-- agent:run -->\n- a\n<!-- /agent:run -->\n",
        );
        let components = element::parse(doc).unwrap();
        let message = QueueGraph::parse(doc, &components).unwrap_err().to_string();
        assert!(
            message.contains(
                "its kind id `run` is already the explicit `id=run` of the queue at line 1"
            ),
            "{message}"
        );
    }

    /// `#idlessqueueidentity`: every queue command resolves the id-less run
    /// and loop through the same kind-named ids, and the old explicit ids
    /// keep resolving exactly as before.
    #[test]
    fn idless_queue_commands_resolve_run_and_loop_without_explicit_ids() {
        for (ids, run_id, loop_id) in [(false, "run", "loop"), (true, "main", "issues")] {
            let doc = idless_session_fixture(ids);

            // `queue schedule`: only the loop declares a cadence.
            let schedules = crate::queue_schedule::identified_queue_schedules(&doc).unwrap();
            assert_eq!(
                schedules
                    .iter()
                    .map(|schedule| schedule.queue_id.as_str())
                    .collect::<Vec<_>>(),
                vec![loop_id]
            );
            assert_eq!(
                crate::queue_schedule::live_heads_for_queue(&doc, loop_id).unwrap(),
                vec!["📥 Watch https://github.com/btakita/agent-doc/issues for new issues."]
            );

            // `queue claim` / `queue brief`: the unclaimed free-text rows are
            // live heads again.
            let heads = crate::queue_continuation::live_queue_head_texts(&doc)
                .expect("the run must parse as the selected queue");
            let free_text = "Why have the ☑️ run items not completed?";
            assert_eq!(
                crate::queue_claim::resolve_claim_target(free_text, &heads).unwrap(),
                free_text
            );

            // The dispatch brief names the origin queue by its canonical id.
            let components = element::parse(&doc).unwrap();
            let auth = crate::subagent_brief::subagent_authorization("session.md", &doc, free_text);
            assert_eq!(auth.queue_id.as_deref(), Some(run_id), "ids={ids}");
            assert!(
                auth.subagent_prompt_preamble.contains(&format!(
                    "agent-doc queue follow-up session.md --id {run_id} --after <origin>"
                )),
                "{}",
                auth.subagent_prompt_preamble
            );

            // `queue follow-up --id <that id>` lands right after the origin.
            let updated = crate::queue_inbound::plan_queue_followups(
                &doc,
                run_id,
                free_text,
                &["follow up on the stale ☑️ rows".to_string()],
            )
            .unwrap();
            assert!(
                updated.contains(&format!(
                    "- {free_text}\n- follow up on the stale ☑️ rows\n- I removed id"
                )),
                "{updated}"
            );
            assert_eq!(
                component_for_id(&doc, &components, loop_id)
                    .unwrap()
                    .map(|c| c.open_start),
                QueueGraph::parse(&doc, &components)
                    .unwrap()
                    .blocks()
                    .get(1)
                    .map(|b| b.component.open_start)
            );
        }
    }
}
