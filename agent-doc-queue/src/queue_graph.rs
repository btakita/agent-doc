//! Identified multi-queue parsing and dependency-aware scheduling.
//!
//! Queue blocks are independent scheduling nodes. The legacy single unkeyed
//! block is retained as the `default` queue; multiple blocks require explicit
//! unique `id=` values and may wait for all predecessors with `after=a,b`.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element::{self, Component};
use anyhow::Result;

use crate::document_queue::{self, QueueEntry};

pub const DEFAULT_QUEUE_ID: &str = "default";

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
    MissingId { occurrence: usize },
    InvalidId { value: String },
    DuplicateId { id: String, occurrences: Vec<usize> },
    EmptyAfter { id: String },
    SelfEdge { id: String },
    MissingPredecessor { id: String, predecessor: String },
    Cycle { path: Vec<String> },
    InvalidBody { id: String, message: String },
}

impl std::fmt::Display for QueueGraphError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingId { occurrence } => write!(
                formatter,
                "agent:queue occurrence {occurrence} requires a nonempty explicit `id=` because the document contains multiple queue components"
            ),
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
        let requires_ids = queue_components.len() > 1;
        let mut blocks: Vec<QueueBlock<'a>> = Vec::new();
        let mut by_id = HashMap::new();
        for (occurrence, component) in queue_components.into_iter().enumerate() {
            let explicit_id = component.attrs.get("id").map(|value| value.trim());
            let id = match explicit_id {
                Some(value) if !value.is_empty() && value != "true" => value.to_string(),
                _ if requires_ids => return Err(QueueGraphError::MissingId { occurrence }),
                _ => DEFAULT_QUEUE_ID.to_string(),
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
    let legacy_single = graph.blocks().len() == 1
        && !graph.blocks()[0].component.attrs.contains_key("id");

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
        proposed.push_str("<!-- agent:queue");
        for (key, value) in attrs {
            proposed.push(' ');
            proposed.push_str(&key);
            proposed.push('=');
            proposed.push_str(&value);
        }
        proposed.push_str(" -->\n");
        proposed.push_str(&document_queue::render(&items));
        proposed.push_str("<!-- /agent:queue -->\n");
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
                Err(QueueGraphError::MissingId { occurrence: 1 })
            ));
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
        assert!(outcome.content.contains("agent:queue id=default -->"));
        assert!(
            outcome
                .content
                .contains("agent:queue id=publish after=default preset=#release -->")
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
}
