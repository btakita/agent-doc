//! `agent:worktrees`: the document's inventory of live worktrees (`#agentworktrees`).
//!
//! Each entry is one directory that still exists for this document. The row
//! carries the queue item's topic, the branch, and the directory. Render is
//! pure: the caller decides which directories are live. A second render is a
//! no-op, and operator-authored children under a directory that is still
//! listed are kept. Children of a directory that left the inventory leave
//! with it.

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
