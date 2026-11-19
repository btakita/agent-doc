use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use agent_doc_element::{element, todo};
use agent_doc_element_backlog::backlog;
use agent_doc_queue::queue_claim::{QueueClaim, QueueClaimState, claim_identity};
use agent_doc_queue::subagent_brief::tracked_ids_for_item;

use super::{
    Candidate, SteeringChange, SteeringDispatch, SteeringItem, SteeringSource, SteeringWatermark,
    content_hash, executable_queue_items,
};

/// Changes to tracked text belong to the claim that consumed the brief. They
/// share the queue steering settle gate, but not queue alignment or its
/// acknowledgement: the queue row itself did not change.
#[derive(Default)]
pub struct TrackedEdits {
    pub(super) candidates: Vec<Candidate>,
    pub(super) awaiting_provenance: bool,
}

/// Derive attributable edits to backlog/Todo text referenced by a running
/// worker. For attached editors, `operator_cut` is the captured operator
/// replay on the steering baseline. For a detached document it is the current
/// file. A changed live row alone does not prove editor authorship.
pub fn claimed_tracked_edits(
    watermark: &SteeringWatermark,
    current: &str,
    operator_cut: Option<&str>,
    claims: &[QueueClaim],
) -> TrackedEdits {
    let mut result = TrackedEdits::default();
    if !claims
        .iter()
        .any(|claim| claim.state == QueueClaimState::Running && claim.worker.is_some())
    {
        return result;
    }
    let live: HashSet<_> = executable_queue_items(current)
        .iter()
        .map(|item| claim_identity(&item.norm))
        .collect();
    let before = TrackedItems::from_content(&watermark.baseline);
    let after = TrackedItems::from_content(current);
    let authored = operator_cut
        .filter(|cut| *cut != current)
        .map(TrackedItems::from_content);
    for claim in claims {
        let Some(worker) = &claim.worker else {
            continue;
        };
        if claim.state != QueueClaimState::Running || !live.contains(&claim.identity) {
            continue;
        }
        let targets: Vec<_> = tracked_ids_for_item(&claim.item_text)
            .into_iter()
            .filter_map(|id| after.get(&id).map(|item| (id, item)))
            .collect();
        // Id-backed claims retain the task text they originally briefed.
        // Only use that alias when its tracked target is unambiguous.
        let claim_time_text = match (targets.as_slice(), claim.tracked_item_aliases.as_slice()) {
            ([_], [text]) if claim.identity.is_id_backed() => Some(text.as_str()),
            _ => None,
        };
        for (id, new) in targets {
            let old = before.get(&id);
            if old.as_ref().is_some_and(|old| old.source != new.source) {
                continue;
            }
            let key = format!(
                "tracked:{}:{id}:{}:{}:{}",
                new.source.as_str(),
                claim.owner,
                claim.generation,
                content_hash(&worker.worker_id),
            );
            let Some(previous) = watermark
                .acknowledged_tracked
                .get(&key)
                .map(String::as_str)
                .or(claim_time_text)
                .or_else(|| old.as_ref().map(|old| old.text.as_str()))
            else {
                continue;
            };
            let normalized = task_text(&new.text, &id, new.source);
            if task_text(previous, &id, new.source) == normalized {
                continue;
            }
            if operator_cut.is_none() {
                // Authorship can arrive after the live content. Leave the
                // content gate open for the next observation of this cut.
                result.awaiting_provenance = true;
                continue;
            }
            if authored.as_ref().is_some_and(|authored| {
                authored.get(&id).is_none_or(|authored| {
                    authored.source != new.source
                        || task_text(&authored.text, &id, authored.source) != normalized
                })
            }) {
                // The latest editor-op cut does not prove this revision.
                // A later hook may see a newer capture at the same text.
                result.awaiting_provenance = true;
                continue;
            }
            let content_hash = content_hash(&normalized.join("\n"));
            result.candidates.push(Candidate {
                key: format!("{key}:{content_hash}"),
                content_hash,
                exchange_identity: None,
                item: SteeringItem {
                    source: new.source,
                    change: SteeringChange::Edited,
                    dispatch: SteeringDispatch::ForwardToOwner,
                    current_item: false,
                    verbatim: new.text,
                    previous: Some(previous.to_owned()),
                    presets: Vec::new(),
                    possibly_partial: false,
                    owner: Some(claim.owner.clone()),
                    generation: Some(claim.generation),
                    worker_id: Some(worker.worker_id.clone()),
                    explicit: false,
                    // Queue commands always target the actual claimed row.
                    // The tracked work's text is the message, not its handle.
                    vector: Some(claim.item_text.clone()),
                    prompt_tree: None,
                    queue_mode: None,
                },
            });
        }
    }
    result
}

struct TrackedText {
    source: SteeringSource,
    text: String,
}

#[derive(Default)]
struct TrackedItems {
    backlog: BTreeMap<String, String>,
    todos: todo::OwnedTodoIndex,
}

impl TrackedItems {
    fn from_content(content: &str) -> Self {
        let Ok(components) = element::parse(content) else {
            return Self::default();
        };
        let mut backlog = BTreeMap::new();
        for component in components
            .iter()
            .filter(|c| element::is_backlog_component(&c.name))
        {
            for item in backlog::parse_items(component.content(content)).1 {
                if !item.id.is_empty() && !item.is_done() {
                    let mut text = item.text;
                    let continuation = item.continuation.trim_end();
                    if !continuation.trim().is_empty() {
                        text.push('\n');
                        text.push_str(continuation);
                    }
                    backlog.entry(item.id).or_insert(text);
                }
            }
        }
        Self {
            backlog,
            todos: todo::OwnedTodoIndex::from_components(content, &components),
        }
    }

    fn get(&self, id: &str) -> Option<TrackedText> {
        self.backlog
            .get(id)
            .map(|text| TrackedText {
                source: SteeringSource::Backlog,
                text: text.clone(),
            })
            .or_else(|| {
                let item = self.todos.owning(id)?;
                item.state.is_open().then(|| TrackedText {
                    source: SteeringSource::Todo,
                    text: item.text.trim_end().to_string(),
                })
            })
    }
}

/// Compare only task text. List numbering, the parent checkbox, and its own
/// id are already excluded by the parsers. Normalize the canonical Todo
/// source locator and generated unchecked backlog subtask prefixes as well.
fn task_text<'a>(text: &'a str, id: &str, source: SteeringSource) -> Vec<Cow<'a, str>> {
    let mut lines = text.lines();
    let action = lines.next().unwrap_or_default().trim_end();
    let action = if source == SteeringSource::Todo {
        todo::strip_trailing_source_reference(action).unwrap_or(action)
    } else {
        action
    };
    std::iter::once(Cow::Borrowed(action))
        .chain(lines.map(|line| {
            let line = line.trim_end();
            let unindented = line.trim_start();
            if source == SteeringSource::Backlog
                && let Some(row) = todo::parse_todo_line(unindented)
                && row.state == todo::TodoState::Open
                && let Some(child) = row.own_id
                && child
                    .strip_prefix(id)
                    .is_some_and(|suffix| suffix.starts_with('-'))
            {
                let indent = &line[..line.len() - unindented.len()];
                return Cow::Owned(format!("{indent}- {}", row.action));
            }
            Cow::Borrowed(line)
        }))
        .collect()
}
