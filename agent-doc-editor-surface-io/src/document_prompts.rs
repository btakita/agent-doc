//! Derive the Explorer tree and one prompt splice from the same editor snapshot.
//! The adapter publishes snapshots on document events and applies the returned
//! insertion only if the editor still has the captured revision.

use agent_doc_document::{outline_projection::project_markdown_outline, subagent_completion};
use agent_doc_element::element::{self, Component};
use agent_doc_element_queue::{
    strip_attempt_projection_prefix, strip_generated_queue_prompt_id_annotation,
};
use agent_doc_frontmatter::frontmatter::split_frontmatter_parts;
use agent_doc_hash::{content_hash, short_content_hash};
use agent_doc_markdown_ast::{
    exchange_tree::{ExchangeNodeKind, parse_exchange_nodes},
    overlay::{self, ItemKind},
};
use agent_doc_queue::document_queue::{
    self, QueueEntry, QueuePrompt, entry_is_ancestor_of, entry_is_edge_block,
};
use agent_doc_queue::queue_continuation::head_awaits_operator_inline;
use agent_doc_queue::queue_graph::QueueGraph;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

mod tasks;
use tasks::PromptTask;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct Evidence {
    source: String,
    submission: Option<Submission>,
    answer: Option<String>,
    active_prompts: Vec<String>,
    file: Option<PathBuf>,
}

#[derive(Deserialize)]
pub struct Submission {
    pub revision: String,
    pub target: String,
    pub draft: String,
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Action {
    Exchange,
    Queue,
    Subprompt,
    Navigate,
}

const ANSWER_INSTRUCTION: &str =
    "Please answer the quoted request and include a standalone HTML comment whose text is ";

#[derive(Serialize)]
struct Target {
    id: String,
    selection_key: Option<String>,
    label: String,
    line: usize,
    depth: usize,
    action: Action,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<PromptTask>,
    #[serde(skip)]
    offset: usize,
    #[serde(skip)]
    indent: usize,
    #[serde(skip)]
    context: Option<String>,
}

struct Projection<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
    exchange: Option<&'a Component>,
    nodes: Vec<Target>,
}

#[derive(Serialize)]
struct PromptEdit {
    offset: usize,
    text: String,
    action: Action,
    #[serde(skip)]
    byte_offset: usize,
}

pub fn project_document_prompts(source: &str) -> Result<Value> {
    project_document_prompts_with_active(source, &[])
}

pub fn project_document_prompts_with_active(
    source: &str,
    active_prompts: &[String],
) -> Result<Value> {
    let components = element::parse(source)?;
    let mut projection = Projection::new(source, &components)?;
    tasks::project(&mut projection, active_prompts);
    Ok(json!({"revision": content_hash(source), "nodes": projection.nodes}))
}

fn prepare_document_prompt(source: &str, submission: &Submission) -> Result<PromptEdit> {
    ensure!(
        content_hash(source) == submission.revision,
        "The document changed. Select the target again."
    );
    let components = element::parse(source)?;
    Projection::new(source, &components)?.submit(submission)
}

pub fn insert_document_prompt(source: &str, submission: &Submission) -> Result<String> {
    let edit = prepare_document_prompt(source, submission)?;
    let mut result = source.to_owned();
    result.insert_str(edit.byte_offset, &edit.text);
    Ok(result)
}

pub(crate) fn derive(evidence: Evidence) -> Value {
    let source = &evidence.source;
    let result = if let Some(request_id) = &evidence.answer {
        element::parse(source).map(|components| worker_answer(source, &components, request_id))
    } else if let Some(submission) = &evidence.submission {
        prepare_document_prompt(source, submission)
            .and_then(|edit| serde_json::to_value(edit).map_err(Into::into))
    } else {
        let mut active = evidence.active_prompts;
        if source.contains(tasks::ANSWER_PREFIX)
            && let Some(file) = evidence.file.as_deref()
        {
            match agent_doc_cycle_state_io::load_with_closeout_projection(file) {
                Ok(Some(cycle)) if cycle.is_open() => {
                    active.extend(cycle.prompt_targets);
                    active.extend(cycle.selected_free_text_queue_heads);
                    active.extend(cycle.absorbed_steering_prompts);
                }
                Err(error) => {
                    eprintln!(
                        "[explorer] turn activity unavailable for {}: {error:#}",
                        file.display()
                    );
                }
                _ => {}
            }
        }
        project_document_prompts_with_active(source, &active)
    };
    result.unwrap_or_else(|error| json!({"error": error.to_string()}))
}

impl<'a> Projection<'a> {
    fn new(source: &'a str, components: &'a [Component]) -> Result<Self> {
        let mut exchanges = components
            .iter()
            .filter(|component| component.name == "exchange");
        let exchange = exchanges.next().filter(|_| exchanges.next().is_none());
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(index, _)| index + 1));
        let mut projection = Self {
            source,
            line_starts,
            exchange,
            nodes: Vec::new(),
        };
        projection.context_node("Document".to_string(), 0, 0, None);
        projection.nodes[0].id = "document".to_string();
        projection.nodes[0].selection_key = Some("document".to_string());

        let normalized = source.replace("\r\n", "\n");
        let (frontmatter, markdown) = split_frontmatter_parts(&normalized);
        let frontmatter_lines = normalized[..normalized.len() - markdown.len()]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        if frontmatter.is_some() {
            projection.context_node(
                "Frontmatter".into(),
                0,
                1,
                Some("session frontmatter".into()),
            );
            projection
                .nodes
                .last_mut()
                .expect("frontmatter node")
                .selection_key = Some("frontmatter".into());
        }
        let mut outlines = project_markdown_outline(markdown)
            .into_iter()
            .filter(|heading| heading.depth > 0)
            .map(|mut heading| {
                heading.line += frontmatter_lines;
                heading
            })
            .peekable();
        let mut overlay = overlay::components(source).into_iter().peekable();
        let mut heading_levels = Vec::new();
        // A duplicate or otherwise invalid run graph is still navigable. A
        // contextual Exchange question remains legal while run placement is ambiguous.
        let valid_queue_graph = QueueGraph::parse(source, components).is_ok();

        for component in components {
            let component_line = projection.line(component.open_start);
            while outlines
                .peek()
                .is_some_and(|heading| heading.line < component_line)
            {
                let heading = outlines.next().expect("peeked heading");
                projection.heading(
                    heading.line,
                    heading.depth,
                    &heading.heading,
                    &mut heading_levels,
                );
            }
            let inline_heading = outlines
                .next_if(|heading| heading.line == component_line)
                .map(|heading| {
                    while heading_levels
                        .last()
                        .is_some_and(|level| *level >= heading.depth)
                    {
                        heading_levels.pop();
                    }
                    (
                        heading.depth,
                        compact_label(heading.heading.split("<!--").next().unwrap_or("")),
                    )
                });
            let depth = heading_levels.len() + 1;
            let opener = &source[component.open_start..component.open_end];
            let raw = overlay::parse_open_marker_line(opener)
                .map(|(name, _)| name)
                .unwrap_or_else(|| component.name.clone());
            let marker = match component.attrs.get("id") {
                Some(id) => format!("agent:{raw} ({id})"),
                None => format!("agent:{raw}"),
            };
            let label = match inline_heading.as_ref() {
                Some((_, heading)) if !heading.is_empty() => format!("{heading} · {marker}"),
                _ => marker,
            };
            let open = component.open_start;
            if component.name == "queue" {
                projection.queue(component, &label, depth, valid_queue_graph)?;
            } else {
                let direct = component.name == "exchange" && exchange == Some(component);
                projection.context_node(
                    label.clone(),
                    open,
                    depth,
                    (!direct).then(|| label.clone()),
                );
                let node = projection.nodes.last_mut().expect("component node");
                node.selection_key = Some(format!("section:{label}"));
                if direct {
                    node.id = "exchange".to_string();
                    projection.exchange_nodes(component, depth + 1);
                } else if let Some(parsed) = overlay.next_if(|parsed| {
                    let line = projection.line(parsed.start_byte);
                    line == component_line
                }) {
                    projection.component_items(&label, &parsed.items, depth + 1);
                }
            }
            // Other parsers may recognize a different set of nested components.
            // Keep their iterator aligned by source position.
            while overlay
                .peek()
                .is_some_and(|parsed| parsed.end_byte <= component.close_end)
            {
                overlay.next();
            }
            if let Some((level, _)) = inline_heading {
                heading_levels.push(level);
            }
            while outlines.peek().is_some_and(|heading| {
                projection
                    .line_starts
                    .get(heading.line - 1)
                    .is_some_and(|start| *start < component.close_end)
            }) {
                outlines.next();
            }
        }
        for heading in outlines {
            projection.heading(
                heading.line,
                heading.depth,
                &heading.heading,
                &mut heading_levels,
            );
        }

        projection.require_unique_selection_keys();
        Ok(projection)
    }

    fn require_unique_selection_keys(&mut self) {
        let mut counts = HashMap::new();
        for key in self
            .nodes
            .iter()
            .filter_map(|node| node.selection_key.as_ref())
        {
            *counts.entry(key.clone()).or_insert(0) += 1;
        }
        for node in &mut self.nodes {
            if node
                .selection_key
                .as_ref()
                .is_some_and(|key| counts[key] > 1)
            {
                node.selection_key = None;
            }
        }
    }

    fn line(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|start| *start <= offset)
    }

    fn target(&self, label: String, start: usize, depth: usize) -> Target {
        let line = self.line(start);
        let line_start = self.line_starts[line - 1];
        let line_end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.source.len());
        let id = format!(
            "{start}:{}",
            short_content_hash(&self.source[line_start..line_end])
        );
        Target {
            id,
            selection_key: None,
            label,
            line,
            depth,
            action: Action::Navigate,
            task: None,
            offset: start,
            indent: 0,
            context: None,
        }
    }

    fn context_node(&mut self, label: String, start: usize, depth: usize, context: Option<String>) {
        let mut node = self.target(label, start, depth);
        node.action = if self.exchange.is_some() {
            Action::Exchange
        } else {
            Action::Navigate
        };
        node.context = context;
        self.nodes.push(node);
    }

    fn heading(&mut self, line: usize, level: usize, raw: &str, levels: &mut Vec<usize>) {
        let Some(&offset) = self.line_starts.get(line - 1) else {
            return;
        };
        while levels.last().is_some_and(|previous| *previous >= level) {
            levels.pop();
        }
        let label = compact_label(raw);
        if !label.is_empty() {
            self.context_node(label.clone(), offset, levels.len() + 1, Some(label.clone()));
            self.nodes.last_mut().expect("heading node").selection_key =
                Some(format!("heading:{label}"));
            levels.push(level);
        }
    }

    fn component_items(&mut self, section: &str, items: &[overlay::Item], base: usize) {
        let mut parents = Vec::new();
        for item in items {
            let line_start = self.line_starts[self.line(item.start_byte) - 1];
            let indent = self.source[line_start..item.start_byte].len()
                + source_indent(&self.source[item.start_byte..]);
            let depth = row_depth(&mut parents, indent, base);
            let label = compact_label(item.work_text());
            if !label.is_empty() {
                let context =
                    if matches!(item.kind, ItemKind::BacklogTask { .. }) && item.explicit_id {
                        format!("{section}, item [#{}] \"{label}\"", item.id)
                    } else {
                        format!("{section}, item \"{label}\"")
                    };
                self.context_node(label, item.start_byte, depth, Some(context));
                if item.explicit_id {
                    self.nodes.last_mut().expect("item node").selection_key =
                        Some(format!("tracked:{section}:{}", item.id));
                }
            }
        }
    }

    fn exchange_nodes(&mut self, component: &Component, depth: usize) {
        let mut offset = component.open_end;
        for node in parse_exchange_nodes(component.content(self.source)) {
            if !matches!(node.kind, ExchangeNodeKind::Salient { .. })
                && !tasks::is_explorer_exchange_request(&node)
                && let Some(line) = node.lines.iter().find(|line| {
                    let text = line.trim();
                    !text.is_empty() && !text.starts_with("<!--")
                })
            {
                let label = compact_label(line);
                let context = format!("agent:exchange, \"{label}\"");
                self.context_node(label, offset, depth, Some(context));
                self.nodes
                    .last_mut()
                    .expect("Exchange item node")
                    .selection_key = Some(node.node_id());
            }
            offset += node.lines.iter().map(String::len).sum::<usize>();
        }
    }

    fn queue(
        &mut self,
        component: &Component,
        label: &str,
        depth: usize,
        valid: bool,
    ) -> Result<()> {
        let body = component.content(self.source);
        let parsed = document_queue::parse_spans(body)?;
        let entries: Vec<_> = parsed.iter().map(|(entry, _)| entry.clone()).collect();
        self.context_node(
            label.to_string(),
            component.open_start,
            depth,
            Some(label.to_string()),
        );
        self.nodes.last_mut().expect("run node").selection_key = Some(format!("section:{label}"));
        if valid {
            let node = self.nodes.last_mut().expect("run node");
            node.action = Action::Queue;
            node.offset = parsed
                .iter()
                .find_map(|(entry, range)| {
                    matches!(entry, QueueEntry::StrayCloseText(_))
                        .then_some(component.open_end + range.start)
                })
                .unwrap_or(component.close_start);
        }

        let mut parents = Vec::new();
        let mut subtree_ends: Vec<usize> = parsed.iter().map(|(_, range)| range.end).collect();
        let mut descendants = Vec::new();
        let mut child_indents = vec![None; parsed.len()];
        for slot in document_queue::queue_tree_shape(&entries) {
            while descendants
                .last()
                .is_some_and(|&(index, _)| !entry_is_ancestor_of(&entries, index, slot.entry_index))
            {
                descendants.pop();
            }
            if let Some(&(parent, _)) = descendants.last() {
                child_indents[parent].get_or_insert(slot.indent);
            }
            let end = parsed[slot.entry_index].1.end;
            for &(parent, _) in &descendants {
                subtree_ends[parent] = end;
            }
            descendants.push((slot.entry_index, slot.indent));
        }
        for (index, (entry, span)) in parsed.into_iter().enumerate() {
            let start = component.open_end + span.start;
            match entry {
                QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => {
                    let row_depth = row_depth(&mut parents, prompt.indent, depth + 1);
                    let text = strip_attempt_projection_prefix(&prompt.text);
                    let title = if text.starts_with(ANSWER_INSTRUCTION) {
                        text.lines()
                            .skip(1)
                            .find_map(|line| line.trim_start().strip_prefix("> "))
                            .map_or_else(|| compact_label(text), compact_label)
                    } else {
                        compact_label(text)
                    };
                    let mut node = self.target(title, start, row_depth);
                    node.task = tasks::queue_task(
                        text.lines().next().unwrap_or(""),
                        &prompt.text,
                        matches!(entries[index], QueueEntry::Completed(_)),
                    );
                    let lineage = document_queue::nest_vector_at(&entries, index, text)
                        .unwrap_or_else(|| text.to_string());
                    let work_key = strip_generated_queue_prompt_id_annotation(&lineage);
                    node.selection_key =
                        Some(format!("queue:{label}:{}", short_content_hash(&work_key)));
                    node.context = Some(format!("{label}, row \"{}\"", compact_label(&lineage)));
                    node.action = if self.exchange.is_some() {
                        Action::Exchange
                    } else {
                        Action::Navigate
                    };
                    // A bound or annotated attempt keeps its existing shape.
                    // A multiline fence and a folded/pipe subtree cannot be
                    // extended with an ordinary list child.
                    let span_text = &body[span];
                    let plain_list = span_text.trim_start().starts_with(['-', '*', '+'])
                        || prompt.ordered_marker.is_some();
                    let pending = prompt
                        .text
                        .trim_start()
                        .strip_prefix("📥 ")
                        .unwrap_or(prompt.text.trim_start());
                    if valid
                        && plain_list
                        && !prompt.multiline
                        && (pending == text || matches!(entries[index], QueueEntry::Completed(_)))
                        && !entry_is_edge_block(&entries, index)
                        && !head_awaits_operator_inline(text)
                    {
                        node.action = Action::Subprompt;
                        node.offset = component.open_end + subtree_ends[index];
                        node.indent = child_indents[index].unwrap_or(
                            prompt.indent
                                + prompt
                                    .ordered_marker
                                    .as_ref()
                                    .map_or(2, |marker| marker.len() + 1),
                        );
                    }
                    self.nodes.push(node);
                }
                QueueEntry::Preset(_)
                | QueueEntry::Dispatch(_)
                | QueueEntry::StartFence(_)
                | QueueEntry::StopFence => {
                    let title = compact_label(&self.source[start..component.open_end + span.end]);
                    let mut node = self.target(title, start, depth + 1);
                    node.action = Action::Navigate;
                    self.nodes.push(node);
                }
                QueueEntry::Freeform(_) | QueueEntry::StrayCloseText(_) => {}
            }
        }
        Ok(())
    }

    fn submit(&self, submission: &Submission) -> Result<PromptEdit> {
        let draft = submission.draft.trim();
        ensure!(
            !draft.is_empty() && draft.chars().count() <= 1500,
            "Enter a question between 1 and 1500 characters."
        );
        ensure!(
            valid_receipt(&submission.request_id),
            "Invalid question receipt."
        );
        let node = self
            .nodes
            .iter()
            .find(|target| target.id == submission.target)
            .ok_or_else(|| anyhow::anyhow!("The prompt target changed. Select it again."))?;
        let newline = if self.source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let receipt = format!(
            "{ANSWER_INSTRUCTION}repo-context-answer:{} in your matching response.",
            submission.request_id,
        );
        let draft_lines = draft
            .lines()
            .map(|line| format!("> {}", escape_comment(line)));
        let (offset, mut insertion) = match node.action {
            Action::Exchange => {
                let exchange = self
                    .exchange
                    .ok_or_else(|| anyhow::anyhow!("This session has no unambiguous Exchange."))?;
                let body = exchange.content(self.source);
                let mut addition = String::new();
                if !body.is_empty() {
                    if !body.ends_with('\n') {
                        addition.push_str(newline);
                    }
                    if !body.ends_with(&newline.repeat(2)) {
                        addition.push_str(newline);
                    }
                }
                addition.push_str("❯ Repo Context Explorer question");
                if let Some(context) = &node.context {
                    addition.push_str(&format!(
                        " about {} (selected at line {} before submission)",
                        escape_comment(context),
                        node.line
                    ));
                }
                addition.push_str(&format!(". {receipt}"));
                if let Some(key) = &node.selection_key {
                    addition.push_str(&tasks::anchor_comment(key));
                }
                addition.push_str(newline);
                for line in draft_lines {
                    addition.push_str(&line);
                    addition.push_str(newline);
                }
                (exchange.close_start, addition)
            }
            Action::Queue | Action::Subprompt => {
                let mut prompt = QueuePrompt::new(receipt);
                prompt.indent = node.indent;
                let continuation = " ".repeat(node.indent + 2);
                for line in draft_lines {
                    prompt.text.push_str(&format!("\n{continuation}{line}"));
                }
                let addition = document_queue::render(&[QueueEntry::Prompt(prompt)]);
                (node.offset, addition.replace('\n', newline))
            }
            Action::Navigate => anyhow::bail!(
                "This row can be opened in the editor. Choose a prompt target to submit."
            ),
        };
        if offset > 0
            && !self.source[..offset].ends_with('\n')
            && !insertion.starts_with('\n')
            && !insertion.starts_with("\r\n")
        {
            insertion.insert_str(0, newline);
        }
        Ok(PromptEdit {
            offset: self.source[..offset].encode_utf16().count(),
            text: insertion,
            action: node.action,
            byte_offset: offset,
        })
    }
}

fn valid_receipt(request_id: &str) -> bool {
    request_id.len() == 32
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn worker_answer(source: &str, components: &[Component], request_id: &str) -> Value {
    let mut exchanges = components
        .iter()
        .filter(|component| component.name == "exchange");
    let Some(exchange) = exchanges.next().filter(|_| exchanges.next().is_none()) else {
        return Value::Null;
    };
    if !valid_receipt(request_id) {
        return Value::Null;
    }
    let token = format!("repo-context-answer:{request_id}");
    let body = exchange.content(source);
    let scan = overlay::ComponentMarkerScan::new(body);
    let mut matches = Vec::new();
    visit_worker_exchange_answers(body, &scan, |start, item, response| {
        if item.contains(&token)
            && let Some(text) = checked_worker_response(response, &token)
        {
            let offset = source[..exchange.open_end + start].encode_utf16().count();
            matches.push(json!({ "offset": offset, "text": text }));
        }
    });
    if matches.len() == 1 {
        matches.pop().expect("one matching response")
    } else {
        Value::Null
    }
}

fn visit_worker_exchange_answers(
    body: &str,
    scan: &overlay::ComponentMarkerScan,
    mut visit: impl FnMut(usize, &str, &str),
) {
    let lines: Vec<_> = std::iter::once(0)
        .chain(body.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect();
    for range in subagent_completion::completion_cell_line_ranges(body) {
        let start = lines[range.start];
        if scan.in_code(start) {
            continue;
        }
        let end = lines.get(range.end).copied().unwrap_or(body.len());
        let cell = &body[start..end];
        for evidence in subagent_completion::completion_evidence(cell) {
            if let Some(response) = subagent_completion::completion_response(cell, &evidence.key) {
                visit(start, &evidence.item, &response);
            }
        }
    }
    for handback in subagent_completion::handback_records(body) {
        let start = lines[handback.line];
        if !scan.in_code(start) {
            visit(start, &handback.item, &handback.response);
        }
    }
}

fn checked_worker_response(response: &str, token: &str) -> Option<String> {
    let marker = format!("<!-- {token} -->");
    let scan = overlay::ComponentMarkerScan::new(response);
    let mut offset = 0;
    let mut found = false;
    let mut body = String::new();
    for line in response.split_inclusive('\n') {
        if line.trim() == marker && !scan.in_code(offset) {
            if found {
                return None;
            }
            found = true;
        } else {
            body.push_str(line);
        }
        offset += line.len();
    }
    let body = body.trim();
    (found && !body.is_empty()).then(|| body.to_string())
}

fn row_depth(ancestors: &mut Vec<usize>, indent: usize, base: usize) -> usize {
    while ancestors.last().is_some_and(|ancestor| *ancestor >= indent) {
        ancestors.pop();
    }
    let depth = base + ancestors.len();
    ancestors.push(indent);
    depth
}

fn compact_label(text: &str) -> String {
    let line = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('#')
        .trim();
    let mut chars = line.chars();
    let mut label: String = chars.by_ref().take(145).collect();
    if chars.next().is_some() {
        label.push('…');
    }
    label
}

fn source_indent(line: &str) -> usize {
    agent_doc_element_queue::subprompts::line_indent_width(line)
}

fn escape_comment(text: &str) -> String {
    text.replace("<!--", "&lt;!--").replace("-->", "--&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIPT: &str = "0123456789abcdef0123456789abcdef";
    const DOCUMENT: &str = concat!(
        "# Project 🧭\n",
        "Intro text still being edited.\n",
        "## Current Run <!-- agent:run id=earlier go -->\n",
        "- Earlier work\n",
        "<!-- /agent:run -->\n",
        "## Follow-ups <!-- agent:loop id=later go -->\n",
        "- Follow up on this\n",
        "  - Existing subprompt\n",
        "- Next sibling\n",
        "--- stop\n",
        "<!-- /agent:loop -->\n",
        "## Backlog <!-- agent:backlog -->\n",
        "- [ ] [#alpha] Identical title\n",
        "- [ ] [#beta] Identical title\n",
        "  - [ ] [#beta-child] Nested work\n",
        "<!-- /agent:backlog -->\n",
        "<!-- agent:exchange -->\n",
        "### Re: Prior answer\n",
        "Existing response.\n",
        "<!-- agent:boundary:sample -->\n",
        "<!-- /agent:exchange -->\n",
    );

    fn outline(source: &str) -> Value {
        let request = json!({ "derivation": "document_prompts", "input": { "source": source } });
        super::super::derive_editor_from_json(&request.to_string()).unwrap()["output"].clone()
    }

    fn node<'a>(projection: &'a Value, label: &str) -> &'a Value {
        projection["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| {
                node["label"]
                    .as_str()
                    .is_some_and(|title| title.contains(label))
            })
            .unwrap_or_else(|| panic!("missing {label:?} in {projection:#}"))
    }

    fn edit(source: &str, revision: &Value, target: &Value, draft: &str) -> Value {
        derive(Evidence {
            source: source.to_string(),
            submission: Some(Submission {
                revision: revision.as_str().unwrap().to_string(),
                target: target.as_str().unwrap().to_string(),
                draft: draft.to_string(),
                request_id: RECEIPT.to_string(),
            }),
            ..Evidence::default()
        })
    }

    fn apply(source: &str, edit: &Value) -> String {
        let offset = edit["offset"]
            .as_u64()
            .unwrap_or_else(|| panic!("{edit:#}")) as usize;
        let mut text: Vec<u16> = source.encode_utf16().collect();
        text.splice(
            offset..offset,
            edit["text"].as_str().unwrap().encode_utf16(),
        );
        String::from_utf16(&text).unwrap()
    }

    fn task_projection(source: &str, receipt: &str, active: &[String]) -> (Value, usize) {
        let value = project_document_prompts_with_active(source, active).unwrap();
        let index = value["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .position(|node| node["task"]["request_id"] == receipt)
            .unwrap_or_else(|| panic!("missing task {receipt} in {value:#}"));
        (value, index)
    }

    #[test]
    fn explorer_tasks_remain_under_their_target_through_live_activity_and_answer() {
        let projection = outline(DOCUMENT);
        let beta = node(&projection, "beta] Identical title");
        let edit = edit(
            DOCUMENT,
            &projection["revision"],
            &beta["id"],
            "Review the change\nTest it",
        );
        let inserted = apply(DOCUMENT, &edit);
        let shifted = format!("Preface added concurrently.\n{inserted}");
        let (pending, at) = task_projection(&shifted, RECEIPT, &[]);
        let nodes = pending["nodes"].as_array().unwrap();
        assert_eq!(nodes[at - 1]["selection_key"], beta["selection_key"]);
        assert_eq!(nodes[at]["label"], "Review the change");
        assert_eq!(nodes[at]["action"], "navigate");
        assert_eq!(nodes[at]["task"]["state"], "submitted");

        let header = shifted
            .lines()
            .find(|line| line.contains(ANSWER_INSTRUCTION))
            .unwrap();
        let running = task_projection(&shifted, RECEIPT, &[header.into()]).0;
        assert_eq!(running["revision"], pending["revision"]);
        assert_eq!(running["nodes"][at]["task"]["state"], "running");
        let marked = shifted.replace(
            "❯ Repo Context Explorer question",
            "❯ 🚧 Repo Context Explorer question",
        );
        assert_eq!(
            task_projection(&marked, RECEIPT, &[]).0["nodes"][at]["task"]["state"],
            "running"
        );

        let receipt = format!("<!-- repo-context-answer:{RECEIPT} -->");
        let append = |answer: &str| {
            shifted.replace(
                "<!-- /agent:exchange -->",
                &format!("\n### Re: Review\n{answer}\n<!-- /agent:exchange -->"),
            )
        };
        let sample = append(&format!("\x60\x60\x60md\n{receipt}\n\x60\x60\x60"));
        assert_eq!(
            task_projection(&sample, RECEIPT, &[]).0["nodes"][at]["task"]["state"],
            "submitted"
        );
        let answered = append(&format!("Review complete.\n{receipt}"));
        let (result, at) = task_projection(&answered, RECEIPT, &[header.into()]);
        assert_eq!(
            result["nodes"][at - 1]["selection_key"],
            beta["selection_key"]
        );
        assert_eq!(result["nodes"][at]["task"]["state"], "answered");
    }

    #[test]
    fn explorer_queue_task_uses_the_real_queue_child_and_lifecycle() {
        let projection = outline(DOCUMENT);
        let parent = node(&projection, "Earlier work");
        let inserted = apply(
            DOCUMENT,
            &edit(
                DOCUMENT,
                &projection["revision"],
                &parent["id"],
                "Check the change",
            ),
        );
        let (queued, at) = task_projection(&inserted, RECEIPT, &[]);
        assert_eq!(
            queued["nodes"][at - 1]["selection_key"],
            parent["selection_key"]
        );
        assert_eq!(queued["nodes"][at]["label"], "Check the change");
        assert_eq!(queued["nodes"][at]["task"]["state"], "queued");
        for (marker, expected) in [
            ("🚧", "running"),
            ("☑️", "reported"),
            ("⛔", "blocked"),
            ("⚠️", "interrupted"),
        ] {
            let marked = inserted.replace(
                ANSWER_INSTRUCTION,
                &format!("{marker} {ANSWER_INSTRUCTION}"),
            );
            let (projected, at) = task_projection(&marked, RECEIPT, &[]);
            assert_eq!(projected["nodes"][at]["task"]["state"], expected);
        }
    }

    #[test]
    fn root_submission_uses_exchange_without_a_repository_source() {
        let source = DOCUMENT.replace('\n', "\r\n");
        let projection = outline(&source);
        let root = node(&projection, "Document");
        let heading = node(&projection, "Project");
        assert_ne!(root["id"], heading["id"]);
        assert_eq!(heading["depth"], 1);
        assert_eq!(node(&projection, "Prior answer")["action"], "exchange");
        let edit = edit(
            &source,
            &projection["revision"],
            &root["id"],
            "Can we iterate here?",
        );
        let updated = apply(&source, &edit);
        assert_eq!(updated.replace(edit["text"].as_str().unwrap(), ""), source);
        let body = updated
            .split_once("<!-- agent:boundary:sample -->\r\n")
            .unwrap()
            .1;
        assert!(body.contains("❯ Repo Context Explorer question."), "{body}");
        assert!(body.contains("> Can we iterate here?\r\n"), "{body}");
    }

    #[test]
    fn session_metadata_is_one_frontmatter_node_with_correct_body_lines() {
        let source = format!(
            "---\r\nagent_doc_session: fixture\r\n---\r\n{}",
            DOCUMENT.replace('\n', "\r\n")
        );
        let projection = outline(&source);
        assert_eq!(node(&projection, "Frontmatter")["line"], 1);
        assert_eq!(node(&projection, "Project")["line"], 4);
        assert!(projection["nodes"].as_array().unwrap().iter().all(|node| {
            !node["label"]
                .as_str()
                .unwrap()
                .contains("agent_doc_session")
        }));
    }

    #[test]
    fn backlog_context_distinguishes_identical_titles_and_refuses_stale_capture() {
        let projection = outline(DOCUMENT);
        let alpha = node(&projection, "[#alpha]");
        let beta = node(&projection, "[#beta]");
        assert_ne!(alpha["id"], beta["id"]);
        assert_ne!(alpha["selection_key"], beta["selection_key"]);
        let shifted = outline(&format!("New notes.\n{DOCUMENT}"));
        assert_ne!(node(&shifted, "[#beta]")["id"], beta["id"]);
        assert_eq!(
            node(&shifted, "[#beta]")["selection_key"],
            beta["selection_key"]
        );
        let duplicate =
            outline(&DOCUMENT.replace("[#beta] Identical title", "[#alpha] Identical title"));
        assert!(node(&duplicate, "[#alpha]")["selection_key"].is_null());
        assert_eq!(
            node(&projection, "[#beta-child]")["depth"].as_u64(),
            beta["depth"].as_u64().map(|n| n + 1)
        );
        let addition = edit(
            DOCUMENT,
            &projection["revision"],
            &beta["id"],
            "What remains?",
        );
        assert_eq!(addition["action"], "exchange");
        assert!(addition["text"].as_str().unwrap().contains("item [#beta]"));
        assert!(!addition["text"].as_str().unwrap().contains("[#alpha]"));

        let changed = DOCUMENT.replace("[#beta] Identical title", "[#beta] Renamed");
        let stale = edit(
            &changed,
            &projection["revision"],
            &beta["id"],
            "What remains?",
        );
        assert!(
            stale["error"]
                .as_str()
                .unwrap()
                .contains("document changed")
        );
        let moved = edit(
            &changed,
            &outline(&changed)["revision"],
            &beta["id"],
            "What remains?",
        );
        assert!(moved["error"].as_str().unwrap().contains("target changed"));
    }

    #[test]
    fn multiline_subprompt_is_one_child_in_selected_run() {
        let projection = outline(DOCUMENT);
        let parent = node(&projection, "Follow up on this");
        let edit = edit(
            DOCUMENT,
            &projection["revision"],
            &parent["id"],
            "First line\n\n- A list item\n--- stop\n<!-- /agent:loop -->\n/clear",
        );
        assert_eq!(edit["action"], "subprompt");
        let updated = apply(DOCUMENT, &edit);
        assert_eq!(
            updated.replace(edit["text"].as_str().unwrap(), ""),
            DOCUMENT
        );
        let components = element::parse(&updated).unwrap();
        let graph = QueueGraph::parse(&updated, &components).unwrap();
        let body = graph.block("later").unwrap().component.content(&updated);
        let rows = document_queue::parse(body).unwrap();
        let [
            QueueEntry::Prompt(_),
            QueueEntry::Prompt(_),
            QueueEntry::Prompt(child),
            QueueEntry::Prompt(_),
            QueueEntry::StopFence,
        ] = rows.as_slice()
        else {
            panic!("expected four prompts and the original stop fence: {rows:#?}");
        };
        assert_eq!(child.indent, 2);
        assert!(child.text.contains("> - A list item"), "{}", child.text);
        assert!(child.text.contains("> /clear"), "{}", child.text);
        assert_eq!(node(&projection, "--- stop")["action"], "navigate");
    }

    #[test]
    fn invalid_run_ids_and_in_flight_rows_keep_a_contextual_exchange_target() {
        let duplicate = DOCUMENT.replace("id=later", "id=earlier");
        let projection = outline(&duplicate);
        let run = node(&projection, "Follow-ups");
        assert_eq!(run["action"], "exchange");

        let running = DOCUMENT.replace(
            "- Follow up on this",
            "- 🚧 [#subagent:worker#1] Follow up on this",
        );
        let projection = outline(&running);
        let parent = node(&projection, "Follow up on this");
        assert_eq!(parent["action"], "exchange");
        assert_eq!(node(&projection, "Earlier work")["action"], "subprompt");
    }

    #[test]
    fn queue_answer_requires_the_binary_worker_record_and_a_receipt_outside_code() {
        let marker = format!("<!-- repo-context-answer:{RECEIPT} -->");
        let item = format!("{ANSWER_INSTRUCTION}repo-context-answer:{RECEIPT}.");
        let completed = subagent_completion::cell_with_response(
            "subagent:sample",
            &item,
            "sample-key",
            &format!("The matching worker answered.\n\n{marker}"),
        )
        .unwrap();
        let with_exchange = |content: &str| {
            format!("# Project 🧭\n<!-- agent:exchange -->\n{content}\n<!-- /agent:exchange -->\n")
        };
        let answer = |source: String| {
            derive(Evidence {
                source,
                answer: Some(RECEIPT.to_string()),
                ..Evidence::default()
            })
        };
        let live = with_exchange(&completed);
        let attached = answer(live.clone());
        assert_eq!(attached["text"], "The matching worker answered.");
        assert_eq!(
            attached["offset"].as_u64().unwrap() as usize,
            live[..live.find("### Re:").unwrap()].encode_utf16().count()
        );
        let retained = format!(
            "{}\n> **Subagent response:**\n> The retained worker answered.\n> {marker}",
            subagent_completion::handback_record_heading("subagent:sample", &item),
        );
        assert_eq!(
            answer(with_exchange(&retained))["text"],
            "The retained worker answered."
        );

        let untrusted_quote = "### Re: Ordinary quoted example\n> **Subagent response:**";
        assert!(answer(with_exchange(&format!("{untrusted_quote}\n> {marker}"))).is_null());
        assert!(
            answer(with_exchange(
                &completed.replace("matching worker", "altered worker")
            ))
            .is_null()
        );
        let example = subagent_completion::cell_with_response(
            "subagent:sample",
            &item,
            "example-key",
            &format!("Only an example.\n\n\x60\x60\x60html\n{marker}\n\x60\x60\x60"),
        )
        .unwrap();
        assert!(answer(with_exchange(&example)).is_null());
    }
}
