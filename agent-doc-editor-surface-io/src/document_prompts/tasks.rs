//! Attach Explorer submissions to their document targets and read their
//! status from the same queue and Exchange evidence that the session uses.

use super::{
    ANSWER_INSTRUCTION, Action, Projection, Target, checked_worker_response, compact_label,
    valid_receipt, visit_worker_exchange_answers,
};
use agent_doc_document::queue_projection::{QueueLifecycleMarker as Marker, lifecycle_marker};
use agent_doc_hash::content_hash;
use agent_doc_markdown_ast::{
    exchange_tree::{ExchangeNode, ExchangeNodeKind, parse_exchange_nodes},
    overlay::ComponentMarkerScan,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::ops::Range;

pub(super) const ANSWER_PREFIX: &str = "repo-context-answer:";
const ANCHOR_PREFIX: &str = "<!-- repo-context-target:";

#[derive(Serialize)]
pub(super) struct PromptTask {
    request_id: String,
    pub(super) state: PromptTaskState,
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum PromptTaskState {
    Submitted,
    Queued,
    Running,
    Reported,
    Answered,
    Closed,
    Blocked,
    Waiting,
    Skipped,
    Interrupted,
}

impl PromptTaskState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Submitted => "Submitted",
            Self::Queued => "Queued",
            Self::Running => "Working",
            Self::Reported => "Reported",
            Self::Answered => "Answered",
            Self::Closed => "Closed",
            Self::Blocked => "Blocked",
            Self::Waiting => "Waiting for you",
            Self::Skipped => "Skipped",
            Self::Interrupted => "Interrupted",
        }
    }
}

struct ExchangeRequest<'a> {
    request_id: &'a str,
    anchor: Option<&'a str>,
    context: Option<&'a str>,
    marked_running: bool,
}

pub(super) fn anchor_comment(selection_key: &str) -> String {
    format!(" {ANCHOR_PREFIX}{} -->", content_hash(selection_key))
}

pub(super) fn request_id(line: &str) -> Option<&str> {
    let (_, suffix) = line.rsplit_once(ANSWER_INSTRUCTION)?;
    let suffix = suffix.strip_prefix(ANSWER_PREFIX)?;
    let (id, after_id) = suffix.split_at_checked(32)?;
    (valid_receipt(id) && after_id.starts_with(" in your matching response.")).then_some(id)
}

fn active_request_id(prompt: &str) -> Option<&str> {
    prompt.lines().find_map(|line| {
        let line = line.trim_start();
        (!line.starts_with('>')).then(|| request_id(line)).flatten()
    })
}

pub(super) fn queue_task(
    stripped_header: &str,
    raw_text: &str,
    completed: bool,
) -> Option<PromptTask> {
    if !stripped_header.starts_with(ANSWER_INSTRUCTION) {
        return None;
    }
    let id = request_id(stripped_header)?;
    let state = if completed {
        PromptTaskState::Closed
    } else {
        match lifecycle_marker(raw_text) {
            Some(Marker::Running) => PromptTaskState::Running,
            Some(Marker::Reported | Marker::LostReported | Marker::Reconciling) => {
                PromptTaskState::Reported
            }
            Some(Marker::Blocked) => PromptTaskState::Blocked,
            Some(Marker::AwaitingOperator) => PromptTaskState::Waiting,
            Some(Marker::Skipped) => PromptTaskState::Skipped,
            Some(Marker::Lost | Marker::LostClaimed | Marker::LostRunning | Marker::Unknown) => {
                PromptTaskState::Interrupted
            }
            Some(Marker::Pending | Marker::Claimed | Marker::Seen) | None => {
                PromptTaskState::Queued
            }
        }
    };
    Some(PromptTask {
        request_id: id.to_string(),
        state,
    })
}

pub(super) fn is_explorer_exchange_request(node: &ExchangeNode) -> bool {
    exchange_request(node).is_some()
}

fn exchange_request(node: &ExchangeNode) -> Option<ExchangeRequest<'_>> {
    if node.kind != ExchangeNodeKind::Prompt {
        return None;
    }
    let header = node.lines.iter().find(|line| !line.trim().is_empty())?;
    let header = header.trim().strip_prefix('❯')?.trim_start();
    let active = header.strip_prefix("🚧 ");
    let header = active.unwrap_or(header);
    let header = header.strip_prefix(super::PROMPT_HEADER).or_else(|| {
        super::LEGACY_PROMPT_HEADERS
            .iter()
            .find_map(|prefix| header.strip_prefix(prefix))
    })?;
    let id = request_id(header)?;
    let (context, _) = header.rsplit_once(ANSWER_INSTRUCTION)?;
    let context = context
        .strip_prefix(" about ")
        .and_then(|text| text.rsplit_once(" (selected at line "))
        .map(|(label, _)| label);
    let anchor = header
        .rsplit_once(ANCHOR_PREFIX)
        .and_then(|(_, tail)| tail.strip_suffix(" -->"))
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    Some(ExchangeRequest {
        request_id: id,
        anchor,
        context,
        marked_running: active.is_some(),
    })
}

fn answer_receipt(line: &str) -> Option<&str> {
    let id = line
        .trim()
        .strip_prefix("<!-- repo-context-answer:")?
        .strip_suffix(" -->")?;
    valid_receipt(id).then_some(id)
}

fn request_label(node: &ExchangeNode) -> String {
    node.lines
        .iter()
        .filter_map(|line| line.trim_start().strip_prefix("> "))
        .map(compact_label)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| "Prompt".into())
}

fn attach_candidate<'a>(
    candidates: &mut HashMap<&'a str, Option<usize>>,
    label: &'a str,
    index: usize,
) {
    candidates
        .entry(label)
        .and_modify(|match_index| *match_index = None)
        .or_insert(Some(index));
}

pub(super) fn project(projection: &mut Projection<'_>, active_prompts: &[String]) {
    let active: HashSet<_> = active_prompts
        .iter()
        .filter_map(|prompt| active_request_id(prompt))
        .collect();
    let Some(exchange) = projection.exchange else {
        update_queue_tasks(&mut projection.nodes, &active, &HashMap::new());
        return;
    };
    let body = exchange.content(projection.source);
    if !body.contains(ANSWER_PREFIX) {
        update_queue_tasks(&mut projection.nodes, &active, &HashMap::new());
        return;
    }
    let scan = ComponentMarkerScan::new(body);
    let exchange_nodes = parse_exchange_nodes(body);
    let mut answers = HashMap::new();
    let mut response_ranges = Vec::new();
    let mut requests = Vec::new();
    let mut offset = 0;
    for node in &exchange_nodes {
        let start = offset;
        let is_response = matches!(node.kind, ExchangeNodeKind::Response { .. });
        let end = start + node.lines.iter().map(String::len).sum::<usize>();
        if is_response {
            response_ranges.push(start..end);
        }
        if let Some(request) = exchange_request(node) {
            requests.push((start..end, request, node));
        }
        for line in &node.lines {
            if is_response
                && !scan.in_code(offset)
                && let Some(receipt) = answer_receipt(line)
            {
                answers.insert(receipt.to_owned(), start..end);
            }
            offset += line.len();
        }
    }
    // A directly marked answer is canonical when both the coordinator and a
    // retained worker record answer the same prompt.
    visit_worker_exchange_answers(body, &scan, |start, item, response| {
        if let Some(receipt) = active_request_id(item)
            && !answers.contains_key(receipt)
            && checked_worker_response(response, &format!("{ANSWER_PREFIX}{receipt}")).is_some()
            && let Some(range) =
                response_ranges.get(response_ranges.partition_point(|range| range.end <= start))
            && range.contains(&start)
        {
            answers.insert(receipt.to_owned(), range.clone());
        }
    });

    update_queue_tasks(&mut projection.nodes, &active, &answers);
    if requests.is_empty() && answers.is_empty() {
        return;
    }

    let mut anchors = HashMap::new();
    let mut contexts = HashMap::new();
    let mut exchange_index = 0;
    for (index, target) in projection.nodes.iter().enumerate() {
        if let Some(key) = &target.selection_key {
            anchors.insert(content_hash(key), index);
        }
        if let Some(context) = &target.context {
            attach_candidate(&mut contexts, context, index);
        }
        if target.id == "exchange" {
            exchange_index = index;
        }
    }

    let mut children: HashMap<usize, Vec<Target>> = HashMap::new();
    let mut projected = HashSet::new();
    for (range, request, node) in requests {
        if !projected.insert(request.request_id) {
            continue;
        }
        let parent = request
            .anchor
            .and_then(|anchor| anchors.get(anchor).copied())
            .or_else(|| {
                request
                    .context
                    .and_then(|context| contexts.get(context).copied().flatten())
            })
            .unwrap_or(exchange_index);
        let mut target = projection.target(
            request_label(node),
            exchange.open_end + range.start,
            projection.nodes[parent].depth + 1,
        );
        target.end_line = Some(projection.line(exchange.open_end + range.end - 1));
        target.id = format!("prompt-task:{}", request.request_id);
        target.selection_key = Some(target.id.clone());
        target.action = Action::Navigate;
        target.task = Some(PromptTask {
            request_id: request.request_id.to_string(),
            state: if answers.contains_key(request.request_id) {
                PromptTaskState::Answered
            } else if request.marked_running || active.contains(request.request_id) {
                PromptTaskState::Running
            } else {
                PromptTaskState::Submitted
            },
        });
        children.entry(parent).or_default().push(target);
    }

    let mut attached = HashSet::new();
    projection.nodes = std::mem::take(&mut projection.nodes)
        .into_iter()
        .enumerate()
        .flat_map(|(index, target)| {
            std::iter::once(target).chain(children.remove(&index).into_iter().flatten())
        })
        .flat_map(|target| {
            let answer = target.task.as_ref().and_then(|task| {
                let range = answers.get(&task.request_id)?;
                if !attached.insert(task.request_id.clone()) {
                    return None;
                }
                let mut answer = projection.target(
                    "Answer".into(),
                    exchange.open_end + range.start,
                    target.depth + 1,
                );
                answer.end_line = Some(projection.line(exchange.open_end + range.end - 1));
                answer.id = format!("prompt-answer:{}", task.request_id);
                answer.selection_key = Some(answer.id.clone());
                Some(answer)
            });
            std::iter::once(target).chain(answer)
        })
        .collect();
}

fn update_queue_tasks(
    nodes: &mut [Target],
    active: &HashSet<&str>,
    answers: &HashMap<String, Range<usize>>,
) {
    for task in nodes.iter_mut().filter_map(|target| target.task.as_mut()) {
        if answers.contains_key(&task.request_id) {
            task.state = match task.state {
                PromptTaskState::Reported => PromptTaskState::Reported,
                _ => PromptTaskState::Answered,
            };
        } else if task.state == PromptTaskState::Queued && active.contains(task.request_id.as_str())
        {
            task.state = PromptTaskState::Running;
        }
    }
}
