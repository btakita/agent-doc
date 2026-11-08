//! `agent:worktrees`: the document's inventory of live worktrees (`#agentworktrees`).
//!
//! Each entry is one directory that still exists for this document. The row
//! carries the queue item's topic, the branch, and the directory. Render is
//! pure: the caller decides which directories are live, and this module does
//! not remove a checkout. A second render is a no-op. Binary-owned topic,
//! branch, and directory lines are rewritten from the records. An operator
//! list child under a topic is an action (delete, remove, merge, or any other
//! instruction). A non-list line stays a note. Children of a directory that
//! left the inventory leave with it.

use std::collections::BTreeMap;

use anyhow::{Context, Result};

use agent_doc_element::element::{self, WORKTREES_COMPONENT, WORKTREES_SECTION};

/// One live worktree, already chosen and ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub topic: String,
    pub branch: String,
    pub directory: String,
}

/// One lineage observation before the live-directory filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCandidate {
    /// Queue item text. The first line, bounded, becomes the topic.
    pub topic: String,
    /// Recorded or discovered branch. Empty means detached.
    pub branch: String,
    pub directory: String,
    pub generation: u64,
    /// The directory exists and the attempt was not removed.
    pub active: bool,
}

/// A GitHub pull request named by a worktree topic.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PullRequestRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// Whether that pull request is still open. `Unknown` is not closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestState {
    Open,
    Closed,
    Unknown,
}

/// What the creating attempt's work is doing, for the prune decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    /// Integrated, released, reported, or the queue row is struck.
    Complete,
    /// A live queue row, or a claim that is still in progress.
    Incomplete,
    /// No completion receipt and no sign the work is still open.
    Unknown,
}

/// Claim ledger signal for one lineage attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimSignal {
    /// Claimed, running, lost, or quota-blocked.
    InProgress,
    /// The worker reported.
    Reported,
    /// No claim for this attempt.
    None,
}

/// How the queue row for one attempt appears in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowSignal {
    /// An unstruck row still names the work.
    Live,
    /// The only rows are struck or checked done.
    Struck,
    /// No row names the work.
    Absent,
}

/// One operator list child under a worktree topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeOperatorChild {
    pub directory: String,
    pub branch: String,
    pub topic: String,
    pub kind: WorktreeOperatorChildKind,
}

/// What the operator asked for under the topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeOperatorChildKind {
    /// `delete` or `remove`: the binary removes the worktree.
    Remove,
    /// Merge and every other instruction: the agent performs the text.
    Instruction(String),
}

/// An instruction promoted into the selected queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInstruction {
    pub directory: String,
    pub branch: String,
    pub topic: String,
    pub text: String,
}

/// Marker shared by the queue prompt and the "still queued" prune guard.
pub const INSTRUCTION_PROMPT_MARK: &str = "Perform the operator instruction on worktree ";

const MAX_TOPIC_CHARS: usize = 100;

/// First line of a queue item, collapsed and bounded, for the topic row.
pub fn topic_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = collapsed.trim();
    if collapsed.is_empty() {
        return "(untitled)".to_string();
    }
    let chars: Vec<char> = collapsed.chars().collect();
    if chars.len() <= MAX_TOPIC_CHARS {
        return collapsed.to_string();
    }
    let mut bounded: String = chars.into_iter().take(MAX_TOPIC_CHARS).collect();
    bounded.push('…');
    bounded
}

fn branch_line(branch: &str) -> String {
    let branch = branch.lines().next().unwrap_or("").trim();
    if branch.is_empty() {
        "detached".to_string()
    } else {
        branch.to_string()
    }
}

/// Live directories only. The same directory keeps the highest generation,
/// and a tie keeps the later candidate. The result is ordered by directory.
pub fn active_records(candidates: &[WorktreeCandidate]) -> Vec<WorktreeRecord> {
    let mut best: BTreeMap<&str, (u64, usize)> = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        if !candidate.active {
            continue;
        }
        let directory = candidate.directory.trim();
        if directory.is_empty() || directory.contains('`') || directory.contains('\n') {
            continue;
        }
        match best.get(directory) {
            Some((generation, _)) if *generation > candidate.generation => {}
            _ => {
                best.insert(directory, (candidate.generation, index));
            }
        }
    }
    best.into_values()
        .map(|(_, index)| {
            let candidate = &candidates[index];
            WorktreeRecord {
                topic: topic_line(&candidate.topic),
                branch: branch_line(&candidate.branch),
                directory: candidate.directory.trim().to_string(),
            }
        })
        .collect()
}

/// Replace `agent:worktrees` with `records`, inserting the section when a
/// live worktree has nowhere to go. An empty inventory does not create the
/// section. `None` means the document is already the projection.
pub fn reconcile(content: &str, records: &[WorktreeRecord]) -> Result<Option<String>> {
    let records = sorted_records(records);
    let components = element::parse(content).context("agent:worktrees: parse components")?;
    let present = components
        .iter()
        .any(|component| component.name == WORKTREES_COMPONENT);
    if !present && records.is_empty() {
        return Ok(None);
    }
    let doc = if present {
        content.to_string()
    } else {
        ensure_section(content, &components)?
    };
    let components = element::parse(&doc).context("agent:worktrees: parse components")?;
    let Some(component) = components
        .iter()
        .find(|component| component.name == WORKTREES_COMPONENT)
    else {
        anyhow::bail!("agent:worktrees section disappeared after insertion");
    };
    let existing = component.content(&doc);
    let body = render_body(&records, existing);
    if existing == body {
        return Ok(None);
    }
    Ok(Some(component.replace_content(&doc, &body)))
}

fn sorted_records(records: &[WorktreeRecord]) -> Vec<WorktreeRecord> {
    let mut records = records.to_vec();
    records.sort_by(|left, right| {
        left.directory
            .cmp(&right.directory)
            .then_with(|| left.topic.cmp(&right.topic))
            .then_with(|| left.branch.cmp(&right.branch))
    });
    records
}

fn render_body(records: &[WorktreeRecord], existing: &str) -> String {
    let notes = operator_children(existing);
    let mut out = String::new();
    for record in records {
        let topic = topic_line(&record.topic);
        let branch = branch_line(&record.branch);
        out.push_str(&format!("- **Topic:** {topic}\n"));
        out.push_str(&format!("  - **Branch:** {branch}\n"));
        out.push_str(&format!("  - **Directory:** `{}`\n", record.directory));
        if let Some(lines) = notes.get(&record.directory) {
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

fn is_topic_line(line: &str) -> bool {
    line.trim_start().starts_with("- **Topic:**")
}

fn is_branch_line(line: &str) -> bool {
    line.trim_start().starts_with("- **Branch:**")
}

fn directory_line(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("- **Directory:**")?.trim();
    let path = rest
        .strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))?;
    (!path.is_empty()).then(|| path.to_string())
}

/// Operator lines under each directory. Topic, branch, and directory lines
/// are binary-owned and are not returned.
fn operator_children(body: &str) -> BTreeMap<String, Vec<String>> {
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut directory: Option<String> = None;
    let mut pending: Vec<String> = Vec::new();
    let mut in_entry = false;
    for line in body.lines() {
        if is_topic_line(line) {
            directory = None;
            pending.clear();
            in_entry = true;
            continue;
        }
        if !in_entry || is_branch_line(line) {
            continue;
        }
        if let Some(dir) = directory_line(line) {
            directory = Some(dir.clone());
            notes.entry(dir).or_default().append(&mut pending);
            continue;
        }
        if let Some(dir) = &directory {
            notes.entry(dir.clone()).or_default().push(line.to_string());
        } else {
            pending.push(line.to_string());
        }
    }
    notes
}

/// Pull requests named on the topic's first line, in order.
pub fn named_pull_requests(topic: &str) -> Vec<PullRequestRef> {
    let line = topic.lines().next().unwrap_or("");
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("github.com/") {
        rest = &rest[start + "github.com/".len()..];
        let Some(pr) = pull_request_at(rest) else {
            continue;
        };
        if !found.contains(&pr) {
            found.push(pr);
        }
    }
    found
}

fn pull_request_at(rest: &str) -> Option<PullRequestRef> {
    let (owner, rest) = rest.split_once('/')?;
    let (repo, rest) = rest.split_once('/')?;
    let (kind, rest) = rest.split_once('/')?;
    if kind != "pull" || !github_name(owner) || !github_name(repo) {
        return None;
    }
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    let number: u64 = digits.parse().ok()?;
    (number > 0).then(|| PullRequestRef {
        owner: owner.to_string(),
        repo: repo.to_string(),
        number,
    })
}

fn github_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

/// One state for every named pull request. Open wins, then unknown.
pub fn combine_pull_request_states(states: &[PullRequestState]) -> Option<PullRequestState> {
    if states.is_empty() {
        return None;
    }
    if states.iter().any(|state| *state == PullRequestState::Open) {
        return Some(PullRequestState::Open);
    }
    if states
        .iter()
        .any(|state| *state == PullRequestState::Unknown)
    {
        return Some(PullRequestState::Unknown);
    }
    Some(PullRequestState::Closed)
}

/// `gh pr view --json state` text. Merged counts as closed. The first
/// recognized state word wins, so the JSON key `state` is ignored. Anything
/// else is unknown.
pub fn parse_pull_request_state(text: &str) -> PullRequestState {
    for word in text.split(|ch: char| !ch.is_ascii_alphabetic()) {
        match word.to_ascii_uppercase().as_str() {
            "OPEN" => return PullRequestState::Open,
            "CLOSED" | "MERGED" => return PullRequestState::Closed,
            _ => {}
        }
    }
    PullRequestState::Unknown
}

/// Complete, incomplete, or unknown from the three durable signals.
pub fn work_status(completed_receipt: bool, claim: ClaimSignal, row: RowSignal) -> WorkStatus {
    if claim == ClaimSignal::InProgress || row == RowSignal::Live {
        return WorkStatus::Incomplete;
    }
    if completed_receipt || claim == ClaimSignal::Reported || row == RowSignal::Struck {
        return WorkStatus::Complete;
    }
    WorkStatus::Unknown
}

/// Automatic prune. An open or unknown pull request stays. Incomplete work stays.
/// A closed pull request, or completed work with no open pull request, goes.
pub fn auto_prune(pr: Option<PullRequestState>, work: WorkStatus) -> bool {
    if matches!(
        pr,
        Some(PullRequestState::Open) | Some(PullRequestState::Unknown)
    ) {
        return false;
    }
    if work == WorkStatus::Incomplete {
        return false;
    }
    matches!(pr, Some(PullRequestState::Closed)) || work == WorkStatus::Complete
}

/// How `item_text` shows up in the runnable and tracked-work components.
pub fn row_signal(content: &str, item_text: &str) -> Result<RowSignal> {
    let Some(token) = row_token(item_text) else {
        return Ok(RowSignal::Absent);
    };
    let components = element::parse(content).context("agent:worktrees: parse components")?;
    let mut struck = false;
    for component in &components {
        let name = element::canonical_component_name(&component.name);
        if !matches!(
            name,
            "queue" | "backlog" | "pending" | "review" | "todo" | "icebox"
        ) {
            continue;
        }
        for line in component.content(content).lines() {
            if !line.contains(token.as_str()) {
                continue;
            }
            if done_row(line) {
                struck = true;
            } else {
                return Ok(RowSignal::Live);
            }
        }
    }
    Ok(if struck {
        RowSignal::Struck
    } else {
        RowSignal::Absent
    })
}

fn row_token(item_text: &str) -> Option<String> {
    let first = item_text.lines().next().unwrap_or("");
    if let Some(start) = first.find("[#")
        && let Some(end) = first[start..].find(']')
    {
        let token = &first[start..start + end + 1];
        if token.len() >= 4 && !token[2..token.len() - 1].chars().any(char::is_whitespace) {
            return Some(token.to_string());
        }
    }
    let collapsed = first.split_whitespace().collect::<Vec<_>>().join(" ");
    (collapsed.chars().count() >= 12).then_some(collapsed)
}

fn done_row(line: &str) -> bool {
    if line.contains("~~") {
        return true;
    }
    let text = line.trim().trim_start_matches(['-', '*']).trim();
    text.starts_with("[x]") || text.starts_with("[X]")
}

/// Operator list children under each directory in a worktrees body.
pub fn operator_child_actions(body: &str) -> Vec<WorktreeOperatorChild> {
    let mut actions = Vec::new();
    let mut topic = String::new();
    let mut branch = String::new();
    let mut directory: Option<String> = None;
    let mut pending: Vec<String> = Vec::new();
    let mut in_entry = false;
    for line in body.lines() {
        if is_topic_line(line) {
            topic = line
                .trim()
                .strip_prefix("- **Topic:**")
                .unwrap_or("")
                .trim()
                .to_string();
            branch.clear();
            directory = None;
            pending.clear();
            in_entry = true;
            continue;
        }
        if !in_entry {
            continue;
        }
        if is_branch_line(line) {
            branch = line
                .trim()
                .strip_prefix("- **Branch:**")
                .unwrap_or("")
                .trim()
                .to_string();
            continue;
        }
        if let Some(dir) = directory_line(line) {
            directory = Some(dir.clone());
            for text in pending.drain(..) {
                push_action(&mut actions, &dir, &branch, &topic, &text);
            }
            continue;
        }
        let Some(text) = list_item_text(line) else {
            continue;
        };
        if let Some(dir) = &directory {
            push_action(&mut actions, dir, &branch, &topic, text);
        } else {
            pending.push(text.to_string());
        }
    }
    actions
}

fn push_action(
    actions: &mut Vec<WorktreeOperatorChild>,
    directory: &str,
    branch: &str,
    topic: &str,
    text: &str,
) {
    let Some(kind) = child_kind(text) else {
        return;
    };
    actions.push(WorktreeOperatorChild {
        directory: directory.to_string(),
        branch: branch.to_string(),
        topic: topic.to_string(),
        kind,
    });
}

fn list_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("- ")?.trim();
    if rest.starts_with("**Topic:**")
        || rest.starts_with("**Branch:**")
        || rest.starts_with("**Directory:**")
    {
        return None;
    }
    Some(rest)
}

fn child_kind(text: &str) -> Option<WorktreeOperatorChildKind> {
    let text = text.trim();
    if text.starts_with("[x]") || text.starts_with("[X]") {
        return None;
    }
    let text = text
        .strip_prefix("[ ]")
        .or_else(|| text.strip_prefix("[/]"))
        .unwrap_or(text)
        .trim();
    if text.is_empty() {
        return None;
    }
    if removal_command(text) {
        Some(WorktreeOperatorChildKind::Remove)
    } else {
        Some(WorktreeOperatorChildKind::Instruction(text.to_string()))
    }
}

fn removal_command(text: &str) -> bool {
    let normalized = text
        .trim()
        .trim_matches(|ch: char| ch == '*' || ch == '`')
        .trim()
        .trim_end_matches(['.', '!', ':'])
        .trim();
    let lower = normalized.to_ascii_lowercase();
    let lower = lower.strip_prefix("please ").unwrap_or(&lower);
    matches!(
        lower.split_whitespace().next().unwrap_or(""),
        "delete" | "remove"
    )
}

/// Queue prompt the agent performs. One line, naming the checkout.
pub fn instruction_prompt(instruction: &WorktreeInstruction) -> String {
    let text = instruction
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{INSTRUCTION_PROMPT_MARK}`{}` (branch `{}`, topic: {}): {text}",
        instruction.directory,
        branch_line(&instruction.branch),
        topic_line(&instruction.topic)
    )
}

/// A live queue row still asks the agent to perform an instruction on `directory`.
pub fn queued_worktree_instruction(content: &str, directory: &str) -> Result<bool> {
    let needle = format!("`{directory}`");
    let components = element::parse(content).context("agent:worktrees: parse components")?;
    for component in &components {
        if element::canonical_component_name(&component.name) != element::QUEUE_COMPONENT {
            continue;
        }
        for line in component.content(content).lines() {
            if done_row(line) {
                continue;
            }
            if line.contains(INSTRUCTION_PROMPT_MARK) && line.contains(&needle) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Consume operator instruction children into `queue_id` and drop them from
/// the inventory. A topic that also has a removal child is left untouched.
/// `None` means there was nothing to enqueue.
pub fn promote_worktree_instructions(
    content: &str,
    queue_id: &str,
) -> Result<Option<WorktreePromotion>> {
    let components = element::parse(content).context("agent:worktrees: parse components")?;
    let Some(worktrees) = components
        .iter()
        .find(|component| component.name == WORKTREES_COMPONENT)
    else {
        return Ok(None);
    };
    let actions = operator_child_actions(worktrees.content(content));
    let removal_dirs: std::collections::BTreeSet<&str> = actions
        .iter()
        .filter(|action| matches!(action.kind, WorktreeOperatorChildKind::Remove))
        .map(|action| action.directory.as_str())
        .collect();
    let instructions: Vec<WorktreeInstruction> = actions
        .iter()
        .filter(|action| !removal_dirs.contains(action.directory.as_str()))
        .filter_map(|action| match &action.kind {
            WorktreeOperatorChildKind::Instruction(text) => Some(WorktreeInstruction {
                directory: action.directory.clone(),
                branch: action.branch.clone(),
                topic: action.topic.clone(),
                text: text.clone(),
            }),
            WorktreeOperatorChildKind::Remove => None,
        })
        .collect();
    if instructions.is_empty() {
        return Ok(None);
    }
    let Some(queue) = agent_doc_queue::queue_set::component_for_id(content, &components, queue_id)?
    else {
        return Ok(None);
    };
    let body = queue.content(content);
    let mut entries = agent_doc_queue::document_queue::parse(body)
        .context("agent:worktrees: parse the selected queue")?;
    let mut existing = entries
        .iter()
        .filter_map(queue_entry_text)
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let insert_at = entries
        .iter()
        .take_while(|entry| {
            matches!(
                entry,
                agent_doc_queue::document_queue::QueueEntry::Preset(_)
                    | agent_doc_queue::document_queue::QueueEntry::Dispatch(_)
            )
        })
        .count();
    let mut inserted = 0usize;
    let mut consumed: std::collections::BTreeSet<(String, String)> =
        std::collections::BTreeSet::new();
    for instruction in &instructions {
        let prompt = instruction_prompt(instruction);
        consumed.insert((instruction.directory.clone(), instruction.text.clone()));
        if existing.contains(&prompt) {
            continue;
        }
        existing.insert(prompt.clone());
        entries.insert(
            insert_at + inserted,
            agent_doc_queue::document_queue::QueueEntry::Prompt(
                agent_doc_queue::document_queue::QueuePrompt::new(prompt),
            ),
        );
        inserted += 1;
    }
    let new_body = agent_doc_queue::document_queue::render(&entries);
    let with_queue = queue.replace_content(content, &new_body);
    let components = element::parse(&with_queue).context("agent:worktrees: parse components")?;
    let Some(worktrees) = components
        .iter()
        .find(|component| component.name == WORKTREES_COMPONENT)
    else {
        anyhow::bail!("agent:worktrees section disappeared while queueing an instruction");
    };
    let stripped = strip_consumed_instructions(worktrees.content(&with_queue), &consumed);
    let content = worktrees.replace_content(&with_queue, &stripped);
    Ok(Some(WorktreePromotion { content, inserted }))
}

/// The document after instruction children moved onto the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePromotion {
    pub content: String,
    /// Prompts inserted. A child whose prompt was already queued is consumed
    /// and does not increment this.
    pub inserted: usize,
}

fn queue_entry_text(entry: &agent_doc_queue::document_queue::QueueEntry) -> Option<&str> {
    match entry {
        agent_doc_queue::document_queue::QueueEntry::Prompt(prompt)
        | agent_doc_queue::document_queue::QueueEntry::Completed(prompt) => {
            Some(prompt.text.trim())
        }
        _ => None,
    }
}

fn strip_consumed_instructions(
    body: &str,
    consumed: &std::collections::BTreeSet<(String, String)>,
) -> String {
    let mut out = String::new();
    let mut directory: Option<String> = None;
    let mut in_entry = false;
    for line in body.lines() {
        if is_topic_line(line) {
            directory = None;
            in_entry = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_entry && let Some(dir) = directory_line(line) {
            directory = Some(dir);
        }
        if let (Some(dir), Some(text)) = (&directory, list_item_text(line))
            && let Some(WorktreeOperatorChildKind::Instruction(instruction)) = child_kind(text)
            && consumed.contains(&(dir.clone(), instruction))
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn ensure_section(doc: &str, components: &[element::Component]) -> Result<String> {
    let offset =
        element::canonical_managed_component_insertion_offset(doc, components, WORKTREES_COMPONENT);
    let (before, after) = doc.split_at(offset);
    let mut out = String::with_capacity(doc.len() + WORKTREES_SECTION.len() + 2);
    out.push_str(before);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(WORKTREES_SECTION);
    if !after.is_empty() && !after.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(after);
    Ok(out)
}
