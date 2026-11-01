//! Project-level mutable salient-decision registry.
//!
//! `decisions.md` is the current authority. `decisions-archive.md` is an
//! append-only revision ledger. Git history and commit trailers provide the
//! durable lookup path; operational databases may cache this data but never
//! replace either Markdown authority.

use agent_doc_hash::content_hash;
use agent_doc_markdown_ast::exchange_tree::{
    is_compact_response_id, response_id_from_lookup_marker,
};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const REGISTRY_FILE: &str = "decisions.md";
pub const ARCHIVE_FILE: &str = "decisions-archive.md";
const REGISTRY_HEADER: &str =
    "# Decisions\n\nThis file is the authoritative current decision registry.\n\n";
const ARCHIVE_HEADER: &str =
    "# Decision revision archive\n\nAppend-only provenance for `decisions.md`.\n\n";
const OPEN_PREFIX: &str = "<!-- agent:decision id=\"";
const OPEN_SUFFIX: &str = "\" -->";
const CLOSE: &str = "<!-- /agent:decision -->";
const REVISION_PREFIX: &str = "<!-- agent:decision-revision id=\"";
const REVISION_CLOSE: &str = "<!-- /agent:decision-revision -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Accepted,
    Superseded,
    Reversed,
    Deprecated,
}

impl DecisionState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Superseded => "superseded",
            Self::Reversed => "reversed",
            Self::Deprecated => "deprecated",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim() {
            "accepted" => Ok(Self::Accepted),
            "superseded" => Ok(Self::Superseded),
            "reversed" => Ok(Self::Reversed),
            "deprecated" => Ok(Self::Deprecated),
            other => bail!(
                "invalid decision state {other:?}; expected accepted, superseded, reversed, or deprecated"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: String,
    pub state: DecisionState,
    pub decision: String,
    pub rationale: String,
    pub evidence_response_ids: Vec<String>,
    pub updated_at: String,
    #[serde(default)]
    pub supersedes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub records: BTreeMap<String, DecisionRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct MutationRequest {
    pub id: String,
    pub state: Option<DecisionState>,
    pub decision: Option<String>,
    pub rationale: Option<String>,
    pub add_evidence: Vec<String>,
    pub remove_evidence: Vec<String>,
    pub supersedes: Option<Vec<String>>,
    pub delete: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum PlannedMutation {
    Upsert {
        before: Option<DecisionRecord>,
        after: DecisionRecord,
    },
    Delete {
        before: DecisionRecord,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionPlan {
    pub version: u8,
    pub project_root: PathBuf,
    pub registry_path: PathBuf,
    pub archive_path: PathBuf,
    pub base_registry_hash: String,
    pub base_archive_hash: String,
    pub mutation: PlannedMutation,
    pub affected_decision_ids: Vec<String>,
    pub evidence_response_ids_added: Vec<String>,
    pub evidence_response_ids_removed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub decision_count: usize,
    pub pinned_response_ids: Vec<String>,
    pub dangling_response_ids: Vec<String>,
    pub dangling_decision_references: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitTrailers {
    pub decision_ids: BTreeSet<String>,
    pub response_ids: BTreeSet<String>,
}

impl CommitTrailers {
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        lines.extend(
            self.decision_ids
                .iter()
                .map(|id| format!("Agent-Doc-Decision: {id}")),
        );
        lines.extend(
            self.response_ids
                .iter()
                .map(|id| format!("Agent-Doc-Response: {id}")),
        );
        lines.join("\n")
    }
}

pub fn registry_path(project_root: &Path) -> PathBuf {
    project_root.join(REGISTRY_FILE)
}

pub fn archive_path(project_root: &Path) -> PathBuf {
    project_root.join(ARCHIVE_FILE)
}

pub fn validate_decision_id(id: &str) -> Result<()> {
    ensure!(id.starts_with("DEC-"), "decision id must start with DEC-");
    let payload = &id[4..];
    ensure!(!payload.is_empty(), "decision id payload must not be empty");
    ensure!(
        payload
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        "decision id {id:?} contains unsupported characters"
    );
    Ok(())
}

fn validate_record(record: &DecisionRecord) -> Result<()> {
    validate_decision_id(&record.id)?;
    ensure!(
        !record.decision.trim().is_empty(),
        "decision text must not be empty"
    );
    ensure!(
        !record.rationale.trim().is_empty(),
        "decision rationale must not be empty"
    );
    ensure!(
        !record.updated_at.trim().is_empty(),
        "updated timestamp must not be empty"
    );
    for value in [&record.decision, &record.rationale] {
        ensure!(
            !value.contains(OPEN_PREFIX) && !value.contains(CLOSE),
            "decision text contains a reserved registry marker"
        );
    }
    for response_id in &record.evidence_response_ids {
        ensure!(
            is_compact_response_id(response_id),
            "invalid response evidence id {response_id:?}"
        );
    }
    for superseded in &record.supersedes {
        validate_decision_id(superseded)?;
        ensure!(
            superseded != &record.id,
            "a decision cannot supersede itself"
        );
    }
    Ok(())
}

fn marker_id(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(OPEN_PREFIX)?
        .strip_suffix(OPEN_SUFFIX)
}

fn parse_list_field(line: &str, prefix: &str) -> Result<Vec<String>> {
    let value = line
        .strip_prefix(prefix)
        .with_context(|| format!("expected registry field {prefix:?}"))?
        .trim();
    if value == "(none)" || value.is_empty() {
        return Ok(Vec::new());
    }
    let mut values = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    Ok(values)
}

fn parse_record_block(block: &str, marker_id_value: &str) -> Result<DecisionRecord> {
    let lines = block.lines().collect::<Vec<_>>();
    ensure!(
        lines.len() >= 9,
        "decision {marker_id_value} block is incomplete"
    );
    ensure!(
        lines.first().and_then(|line| marker_id(line)) == Some(marker_id_value),
        "decision marker mismatch"
    );
    ensure!(
        lines.last().is_some_and(|line| line.trim() == CLOSE),
        "decision {marker_id_value} is missing its closing marker"
    );
    ensure!(
        lines.get(1).copied() == Some(format!("## {marker_id_value}").as_str()),
        "decision {marker_id_value} heading does not match its marker"
    );
    let state = DecisionState::parse(
        lines
            .get(2)
            .and_then(|line| line.strip_prefix("- State: "))
            .context("missing - State field")?,
    )?;
    let updated_at = lines
        .get(3)
        .and_then(|line| line.strip_prefix("- Updated: "))
        .context("missing - Updated field")?
        .trim()
        .to_string();
    let evidence_response_ids = parse_list_field(
        lines.get(4).context("missing - Evidence field")?,
        "- Evidence: ",
    )?;
    let supersedes = parse_list_field(
        lines.get(5).context("missing - Supersedes field")?,
        "- Supersedes: ",
    )?;
    ensure!(
        lines.get(6).copied() == Some("### Decision"),
        "missing Decision section"
    );
    let rationale_index = lines
        .iter()
        .position(|line| *line == "### Rationale")
        .context("missing Rationale section")?;
    ensure!(
        rationale_index > 6,
        "decision text must follow Decision heading"
    );
    let decision = lines[7..rationale_index].join("\n").trim().to_string();
    let rationale = lines[rationale_index + 1..lines.len() - 1]
        .join("\n")
        .trim()
        .to_string();
    let record = DecisionRecord {
        id: marker_id_value.to_string(),
        state,
        decision,
        rationale,
        evidence_response_ids,
        updated_at,
        supersedes,
    };
    validate_record(&record)?;
    Ok(record)
}

pub fn parse_registry(markdown: &str) -> Result<Registry> {
    if markdown.trim().is_empty() {
        return Ok(Registry {
            records: BTreeMap::new(),
        });
    }
    ensure!(
        markdown.starts_with("# Decisions"),
        "decision registry must start with # Decisions"
    );
    let mut records = BTreeMap::new();
    let mut cursor = 0usize;
    let mut outside_blocks = String::new();
    while let Some(relative) = markdown[cursor..].find(OPEN_PREFIX) {
        let start = cursor + relative;
        outside_blocks.push_str(&markdown[cursor..start]);
        let open_end = markdown[start..]
            .find('\n')
            .map(|offset| start + offset + 1)
            .context("decision marker must end with a newline")?;
        let id = marker_id(markdown[start..open_end].trim_end())
            .context("invalid decision marker")?
            .to_string();
        let close_relative = markdown[open_end..]
            .find(CLOSE)
            .with_context(|| format!("decision {id} is missing its closing marker"))?;
        let end = open_end + close_relative + CLOSE.len();
        let record = parse_record_block(&markdown[start..end], &id)?;
        ensure!(
            records.insert(id.clone(), record).is_none(),
            "duplicate decision id {id}"
        );
        cursor = end;
    }
    outside_blocks.push_str(&markdown[cursor..]);
    let outside = outside_blocks.split_whitespace().collect::<Vec<_>>().join(" ");
    let canonical_outside = REGISTRY_HEADER
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    ensure!(
        outside == "# Decisions" || outside == canonical_outside,
        "decision registry contains text outside canonical decision blocks"
    );
    Ok(Registry { records })
}

fn render_list(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}

fn render_record(record: &DecisionRecord) -> String {
    format!(
        "{OPEN_PREFIX}{}{OPEN_SUFFIX}\n## {}\n- State: {}\n- Updated: {}\n- Evidence: {}\n- Supersedes: {}\n### Decision\n{}\n### Rationale\n{}\n{CLOSE}\n\n",
        record.id,
        record.id,
        record.state.as_str(),
        record.updated_at,
        render_list(&record.evidence_response_ids),
        render_list(&record.supersedes),
        record.decision.trim(),
        record.rationale.trim(),
    )
}

pub fn render_registry(registry: &Registry) -> String {
    let mut rendered = REGISTRY_HEADER.to_string();
    for record in registry.records.values() {
        rendered.push_str(&render_record(record));
    }
    rendered
}

fn read_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn sorted_unique(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

fn archive_mentions_id(archive: &str, id: &str) -> bool {
    archive.lines().any(|line| {
        line.trim()
            .strip_prefix(REVISION_PREFIX)
            .and_then(|value| value.split_once('"').map(|(value, _)| value))
            == Some(id)
    })
}

pub fn plan(project_root: &Path, request: MutationRequest) -> Result<DecisionPlan> {
    validate_decision_id(&request.id)?;
    let project_root = project_root
        .canonicalize()
        .with_context(|| format!("project root does not exist: {}", project_root.display()))?;
    let registry_path = registry_path(&project_root);
    let archive_path = archive_path(&project_root);
    let registry_text = read_or_empty(&registry_path)?;
    let archive_text = read_or_empty(&archive_path)?;
    let registry = parse_registry(&registry_text)?;
    let before = registry.records.get(&request.id).cloned();
    let mutation = if request.delete {
        ensure!(
            request.state.is_none()
                && request.decision.is_none()
                && request.rationale.is_none()
                && request.add_evidence.is_empty()
                && request.remove_evidence.is_empty()
                && request.supersedes.is_none(),
            "delete cannot be combined with record updates"
        );
        PlannedMutation::Delete {
            before: before.context("cannot delete a decision that does not exist")?,
        }
    } else {
        if before.is_none() {
            ensure!(
                !archive_mentions_id(&archive_text, &request.id),
                "decision id {} already exists in revision history and cannot be reused",
                request.id
            );
        }
        let mut after = before.clone().unwrap_or_else(|| DecisionRecord {
            id: request.id.clone(),
            state: DecisionState::Accepted,
            decision: String::new(),
            rationale: String::new(),
            evidence_response_ids: Vec::new(),
            updated_at: String::new(),
            supersedes: Vec::new(),
        });
        if let Some(state) = request.state {
            after.state = state;
        }
        if let Some(decision) = request.decision {
            after.decision = decision;
        }
        if let Some(rationale) = request.rationale {
            after.rationale = rationale;
        }
        let remove = request.remove_evidence.into_iter().collect::<BTreeSet<_>>();
        after
            .evidence_response_ids
            .retain(|response_id| !remove.contains(response_id));
        after.evidence_response_ids.extend(request.add_evidence);
        after.evidence_response_ids = sorted_unique(after.evidence_response_ids);
        if let Some(supersedes) = request.supersedes {
            after.supersedes = sorted_unique(supersedes);
        }
        after.updated_at = request
            .updated_at
            .unwrap_or_else(agent_doc_log_time::current_log_timestamp);
        validate_record(&after)?;
        PlannedMutation::Upsert { before, after }
    };

    let (old_evidence, new_evidence) = match &mutation {
        PlannedMutation::Upsert { before, after } => (
            before
                .as_ref()
                .map(|record| record.evidence_response_ids.clone())
                .unwrap_or_default(),
            after.evidence_response_ids.clone(),
        ),
        PlannedMutation::Delete { before } => (before.evidence_response_ids.clone(), Vec::new()),
    };
    let old = old_evidence.into_iter().collect::<BTreeSet<_>>();
    let new = new_evidence.into_iter().collect::<BTreeSet<_>>();
    Ok(DecisionPlan {
        version: 1,
        project_root,
        registry_path,
        archive_path,
        base_registry_hash: content_hash(&registry_text),
        base_archive_hash: content_hash(&archive_text),
        mutation,
        affected_decision_ids: vec![request.id],
        evidence_response_ids_added: new.difference(&old).cloned().collect(),
        evidence_response_ids_removed: old.difference(&new).cloned().collect(),
    })
}

fn render_revision(plan: &DecisionPlan) -> String {
    let delete_timestamp = agent_doc_log_time::current_log_timestamp();
    let (id, before, after, operation, timestamp) = match &plan.mutation {
        PlannedMutation::Upsert { before, after } => (
            after.id.as_str(),
            before.as_ref(),
            Some(after),
            if before.is_some() {
                "updated"
            } else {
                "created"
            },
            after.updated_at.as_str(),
        ),
        PlannedMutation::Delete { before } => (
            before.id.as_str(),
            Some(before),
            None,
            "deleted",
            delete_timestamp.as_str(),
        ),
    };
    let before_state = before
        .map(|record| record.state.as_str())
        .unwrap_or("(none)");
    let after_state = after
        .map(|record| record.state.as_str())
        .unwrap_or("(deleted)");
    let before_supersedes = before
        .map(|record| render_list(&record.supersedes))
        .unwrap_or_else(|| "(none)".to_string());
    let after_supersedes = after
        .map(|record| render_list(&record.supersedes))
        .unwrap_or_else(|| "(deleted)".to_string());
    let decision = after
        .map(|record| record.decision.as_str())
        .or_else(|| before.map(|record| record.decision.as_str()))
        .unwrap_or("(none)");
    let rationale = after
        .map(|record| record.rationale.as_str())
        .or_else(|| before.map(|record| record.rationale.as_str()))
        .unwrap_or("(none)");
    format!(
        "{REVISION_PREFIX}{id}\" updated=\"{timestamp}\" -->\n## Revision {id} — {timestamp}\n- Operation: {operation}\n- State: {before_state} -> {after_state}\n- Supersedes: {before_supersedes} -> {after_supersedes}\n- Evidence added: {}\n- Evidence removed: {}\n### Decision at revision\n{}\n### Rationale at revision\n{}\n{REVISION_CLOSE}\n\n",
        render_list(&plan.evidence_response_ids_added),
        render_list(&plan.evidence_response_ids_removed),
        decision.trim(),
        rationale.trim(),
    )
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.agent-doc-{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("decision"),
        std::process::id(),
        agent_doc_log_time::current_epoch_secs()
    ));
    fs::write(&temp, content).with_context(|| format!("failed to write {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("failed to replace {}", path.display()))
}

pub fn apply(plan: &DecisionPlan) -> Result<()> {
    ensure!(
        plan.version == 1,
        "unsupported decision plan version {}",
        plan.version
    );
    let lock_path = plan
        .project_root
        .join(".agent-doc")
        .join("decision-registry.lock");
    fs::create_dir_all(lock_path.parent().context("decision lock has no parent")?)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("failed to lock {}", lock_path.display()))?;
    let registry_text = read_or_empty(&plan.registry_path)?;
    let archive_text = read_or_empty(&plan.archive_path)?;
    ensure!(
        content_hash(&registry_text) == plan.base_registry_hash,
        "decision registry changed after planning; create a fresh plan"
    );
    ensure!(
        content_hash(&archive_text) == plan.base_archive_hash,
        "decision archive changed after planning; create a fresh plan"
    );
    let mut registry = parse_registry(&registry_text)?;
    match &plan.mutation {
        PlannedMutation::Upsert { before, after } => {
            ensure!(
                registry.records.get(&after.id) == before.as_ref(),
                "decision {} changed after planning",
                after.id
            );
            registry.records.insert(after.id.clone(), after.clone());
        }
        PlannedMutation::Delete { before } => {
            ensure!(
                registry.records.get(&before.id) == Some(before),
                "decision {} changed after planning",
                before.id
            );
            registry.records.remove(&before.id);
        }
    }
    let new_registry = render_registry(&registry);
    let mut new_archive = if archive_text.is_empty() {
        ARCHIVE_HEADER.to_string()
    } else {
        archive_text
    };
    if !new_archive.ends_with('\n') {
        new_archive.push('\n');
    }
    new_archive.push_str(&render_revision(plan));
    // The archive is written first: if a process is interrupted between writes,
    // immutable evidence may be duplicated by a fresh plan, but is never lost.
    atomic_write(&plan.archive_path, &new_archive)?;
    atomic_write(&plan.registry_path, &new_registry)?;
    Ok(())
}

pub fn response_ids_in_markdown(markdown: &str) -> BTreeSet<String> {
    markdown
        .lines()
        .filter_map(|line| response_id_from_lookup_marker(line.trim()))
        .map(str::to_string)
        .collect()
}

pub fn decision_references(markdown: &str) -> BTreeSet<String> {
    let mut references = BTreeSet::new();
    let mut remainder = markdown;
    while let Some(index) = remainder.find("decision:DEC-") {
        let value = &remainder[index + "decision:".len()..];
        let end = value
            .find(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
            })
            .unwrap_or(value.len());
        let id = &value[..end];
        if validate_decision_id(id).is_ok() {
            references.insert(id.to_string());
        }
        remainder = &value[end..];
    }
    references
}

fn visit_markdown_files(root: &Path, callback: &mut impl FnMut(&Path, &str)) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            visit_markdown_files(&path, callback)?;
        } else if path.extension().is_some_and(|extension| extension == "md")
            && let Ok(markdown) = fs::read_to_string(&path)
        {
            callback(&path, &markdown);
        }
    }
    Ok(())
}

/// Response evidence pins consumed by archive/GC implementations.
///
/// This intentionally reads the tracked registry every time. An index may cache
/// the returned set, but must not become its authority.
pub fn pinned_response_ids(project_root: &Path) -> Result<BTreeSet<String>> {
    let markdown = read_or_empty(&registry_path(project_root))?;
    let registry = parse_registry(&markdown)?;
    Ok(registry
        .records
        .values()
        .flat_map(|record| record.evidence_response_ids.iter().cloned())
        .collect())
}

pub fn read_registry(project_root: &Path) -> Result<Registry> {
    parse_registry(&read_or_empty(&registry_path(project_root))?)
}

pub fn read_decision(project_root: &Path, id: &str) -> Result<Option<DecisionRecord>> {
    validate_decision_id(id)?;
    Ok(read_registry(project_root)?.records.remove(id))
}

pub fn validate(project_root: &Path) -> Result<ValidationReport> {
    let registry_markdown = read_or_empty(&registry_path(project_root))?;
    let registry = parse_registry(&registry_markdown)?;
    let pinned = pinned_response_ids(project_root)?;
    let mut materialized_responses = BTreeSet::new();
    let mut referenced_decisions = BTreeSet::new();
    visit_markdown_files(project_root, &mut |path, markdown| {
        materialized_responses.extend(response_ids_in_markdown(markdown));
        if path.file_name().and_then(|name| name.to_str()) != Some(REGISTRY_FILE)
            && path.file_name().and_then(|name| name.to_str()) != Some(ARCHIVE_FILE)
            && markdown.contains("<!-- agent:")
        {
            referenced_decisions.extend(decision_references(markdown));
        }
    })?;
    let dangling_response_ids = pinned
        .difference(&materialized_responses)
        .cloned()
        .collect::<Vec<_>>();
    let known = registry.records.keys().cloned().collect::<BTreeSet<_>>();
    let mut dangling_decision_references = referenced_decisions
        .difference(&known)
        .cloned()
        .collect::<Vec<_>>();
    for record in registry.records.values() {
        dangling_decision_references.extend(
            record
                .supersedes
                .iter()
                .filter(|id| !known.contains(*id))
                .cloned(),
        );
    }
    dangling_decision_references.sort();
    dangling_decision_references.dedup();
    Ok(ValidationReport {
        valid: dangling_response_ids.is_empty() && dangling_decision_references.is_empty(),
        decision_count: registry.records.len(),
        pinned_response_ids: pinned.into_iter().collect(),
        dangling_response_ids,
        dangling_decision_references,
    })
}

pub fn history(project_root: &Path, id: &str) -> Result<Vec<String>> {
    validate_decision_id(id)?;
    let markdown = read_or_empty(&archive_path(project_root))?;
    let needle = format!("{REVISION_PREFIX}{id}\"");
    let mut revisions = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = markdown[cursor..].find(&needle) {
        let start = cursor + relative;
        let end_relative = markdown[start..]
            .find(REVISION_CLOSE)
            .with_context(|| format!("revision archive entry for {id} is not closed"))?;
        let end = start + end_relative + REVISION_CLOSE.len();
        revisions.push(markdown[start..end].to_string());
        cursor = end;
    }
    Ok(revisions)
}

fn record_evidence(registry: &Registry) -> BTreeMap<String, BTreeSet<String>> {
    registry
        .records
        .iter()
        .map(|(id, record)| {
            (
                id.clone(),
                record.evidence_response_ids.iter().cloned().collect(),
            )
        })
        .collect()
}

/// Derive sorted/deduplicated decision and response trailers from a Markdown
/// file's HEAD/current pair. This covers record mutations and decision reference
/// additions/removals in queue, todo, notes, prompts, or any other component.
pub fn trailers_for_diff(old: &str, new: &str) -> CommitTrailers {
    let old_refs = decision_references(old);
    let new_refs = decision_references(new);
    let mut trailers = CommitTrailers::default();
    trailers
        .decision_ids
        .extend(old_refs.symmetric_difference(&new_refs).cloned());

    if let (Ok(old_registry), Ok(new_registry)) = (parse_registry(old), parse_registry(new)) {
        let ids = old_registry
            .records
            .keys()
            .chain(new_registry.records.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for id in ids {
            if old_registry.records.get(&id) != new_registry.records.get(&id) {
                trailers.decision_ids.insert(id);
            }
        }
        let old_evidence = record_evidence(&old_registry);
        let new_evidence = record_evidence(&new_registry);
        let ids = old_evidence
            .keys()
            .chain(new_evidence.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for id in ids {
            let old = old_evidence.get(&id).cloned().unwrap_or_default();
            let new = new_evidence.get(&id).cloned().unwrap_or_default();
            trailers
                .response_ids
                .extend(old.symmetric_difference(&new).cloned());
        }
    }
    trailers
}

pub fn commit_plan(plan: &DecisionPlan, subject: Option<&str>) -> Result<bool> {
    let root_output = Command::new("git")
        .args([
            "-C",
            &plan.project_root.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
        .context("failed to run git rev-parse")?;
    ensure!(
        root_output.status.success(),
        "project root is not inside a git repository"
    );
    let git_root = PathBuf::from(String::from_utf8_lossy(&root_output.stdout).trim());
    let registry = plan
        .registry_path
        .strip_prefix(&git_root)
        .with_context(|| {
            format!(
                "{} is outside git root {}",
                plan.registry_path.display(),
                git_root.display()
            )
        })?;
    let archive = plan.archive_path.strip_prefix(&git_root).with_context(|| {
        format!(
            "{} is outside git root {}",
            plan.archive_path.display(),
            git_root.display()
        )
    })?;
    let add = Command::new("git")
        .arg("-C")
        .arg(&git_root)
        .arg("add")
        .arg("--")
        .arg(registry)
        .arg(archive)
        .output()
        .context("failed to stage decision registry")?;
    ensure!(
        add.status.success(),
        "git add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let mut message = subject.unwrap_or("agent-doc: update decisions").to_string();
    message.push_str("\n\n");
    let trailers = CommitTrailers {
        decision_ids: plan.affected_decision_ids.iter().cloned().collect(),
        response_ids: plan
            .evidence_response_ids_added
            .iter()
            .chain(plan.evidence_response_ids_removed.iter())
            .cloned()
            .collect(),
    };
    message.push_str(&trailers.render());
    let commit = Command::new("git")
        .arg("-C")
        .arg(&git_root)
        .arg("commit")
        .arg("--only")
        .arg("-m")
        .arg(&message)
        .arg("--")
        .arg(registry)
        .arg(archive)
        .output()
        .context("failed to commit decision registry")?;
    if commit.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&commit.stderr);
    if stderr.contains("nothing to commit") || stderr.contains("no changes added to commit") {
        Ok(false)
    } else {
        bail!("git commit failed: {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(id: &str) -> String {
        format!(
            "<!-- agent:exchange -->\n### Re: test\n**Response {id}**\nbody\n<!-- /agent:exchange -->\n"
        )
    }

    fn request(id: &str, response_id: &str) -> MutationRequest {
        MutationRequest {
            id: id.to_string(),
            decision: Some("Keep Markdown authoritative.".to_string()),
            rationale: Some("It remains reviewable and portable.".to_string()),
            add_evidence: vec![response_id.to_string()],
            updated_at: Some("2026-11-01T12:00:00Z".to_string()),
            ..MutationRequest::default()
        }
    }

    #[test]
    fn mutable_update_keeps_the_stable_record_and_appends_history() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("session.md"),
            response("[#r-0123456789ab]"),
        )
        .unwrap();
        let create = plan(temp.path(), request("DEC-authority", "[#r-0123456789ab]")).unwrap();
        apply(&create).unwrap();
        let update = plan(
            temp.path(),
            MutationRequest {
                id: "DEC-authority".to_string(),
                state: Some(DecisionState::Superseded),
                rationale: Some("A newer decision replaces it.".to_string()),
                updated_at: Some("2026-11-02T12:00:00Z".to_string()),
                ..MutationRequest::default()
            },
        )
        .unwrap();
        apply(&update).unwrap();
        let registry =
            parse_registry(&fs::read_to_string(registry_path(temp.path())).unwrap()).unwrap();
        assert_eq!(registry.records.len(), 1);
        assert_eq!(
            registry.records["DEC-authority"].state,
            DecisionState::Superseded
        );
        assert_eq!(history(temp.path(), "DEC-authority").unwrap().len(), 2);
    }

    #[test]
    fn concurrent_registry_edit_refuses_stale_plan() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan(temp.path(), request("DEC-race", "[#r-0123456789ab]")).unwrap();
        fs::write(registry_path(temp.path()), "# Decisions\n\noperator edit\n").unwrap();
        let error = apply(&plan).unwrap_err().to_string();
        assert!(error.contains("changed after planning"));
    }

    #[test]
    fn concurrent_plans_serialize_and_only_one_can_apply() {
        let temp = tempfile::tempdir().unwrap();
        let first = plan(temp.path(), request("DEC-first", "[#r-0123456789ab]")).unwrap();
        let second = plan(temp.path(), request("DEC-second", "[#r-abcdefabcdef]")).unwrap();
        let first_thread = std::thread::spawn(move || apply(&first));
        let second_thread = std::thread::spawn(move || apply(&second));
        let outcomes = [first_thread.join().unwrap(), second_thread.join().unwrap()];
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert!(
            outcomes
                .iter()
                .filter_map(|outcome| outcome.as_ref().err())
                .any(|error| error.to_string().contains("changed after planning"))
        );
    }

    #[test]
    fn validation_detects_dangling_evidence_and_decision_references() {
        let temp = tempfile::tempdir().unwrap();
        let plan = plan(temp.path(), request("DEC-known", "[#r-0123456789ab]")).unwrap();
        apply(&plan).unwrap();
        fs::write(
            temp.path().join("session.md"),
            "<!-- agent:queue -->\n- [ ] [missing](decision:DEC-missing)\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let report = validate(temp.path()).unwrap();
        assert!(!report.valid);
        assert_eq!(report.dangling_response_ids, ["[#r-0123456789ab]"]);
        assert_eq!(report.dangling_decision_references, ["DEC-missing"]);
    }

    #[test]
    fn diff_trailers_cover_mutation_reference_and_evidence_changes_sorted() {
        let old = "# Decisions\n\n";
        let record = DecisionRecord {
            id: "DEC-b".to_string(),
            state: DecisionState::Accepted,
            decision: "B".to_string(),
            rationale: "Because".to_string(),
            evidence_response_ids: vec!["[#r-abcdefabcdef]".to_string()],
            updated_at: "2026-11-01T00:00:00Z".to_string(),
            supersedes: Vec::new(),
        };
        let new = render_registry(&Registry {
            records: BTreeMap::from([(record.id.clone(), record)]),
        });
        let trailers = trailers_for_diff(old, &new);
        assert_eq!(
            trailers.decision_ids.into_iter().collect::<Vec<_>>(),
            ["DEC-b"]
        );
        assert_eq!(
            trailers.response_ids.into_iter().collect::<Vec<_>>(),
            ["[#r-abcdefabcdef]"]
        );
        let references = trailers_for_diff(
            "<!-- agent:todo -->\n<!-- /agent:todo -->",
            "<!-- agent:todo -->\n[one](decision:DEC-z) [two](decision:DEC-a)\n<!-- /agent:todo -->",
        );
        assert_eq!(
            references.decision_ids.into_iter().collect::<Vec<_>>(),
            ["DEC-a", "DEC-z"]
        );
    }

    #[test]
    fn materialized_response_satisfies_validation_and_pins_it() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("session.md"),
            response("[#r-0123456789ab]"),
        )
        .unwrap();
        let plan = plan(temp.path(), request("DEC-pin", "[#r-0123456789ab]")).unwrap();
        apply(&plan).unwrap();
        let report = validate(temp.path()).unwrap();
        assert!(report.valid);
        assert_eq!(report.pinned_response_ids, ["[#r-0123456789ab]"]);
    }

    #[test]
    fn scale_lookup_and_trailer_extraction_stay_on_tracked_markdown() {
        let temp = tempfile::tempdir().unwrap();
        let mut records = BTreeMap::new();
        for index in 0..5_000 {
            let id = format!("DEC-scale-{index:05}");
            records.insert(
                id.clone(),
                DecisionRecord {
                    id,
                    state: DecisionState::Accepted,
                    decision: format!("Decision {index}"),
                    rationale: "Scale fixture".to_string(),
                    evidence_response_ids: Vec::new(),
                    updated_at: "2026-11-01T00:00:00Z".to_string(),
                    supersedes: Vec::new(),
                },
            );
        }
        let old = render_registry(&Registry { records });
        fs::write(registry_path(temp.path()), &old).unwrap();
        let lookup_started = std::time::Instant::now();
        let found = read_decision(temp.path(), "DEC-scale-04999").unwrap();
        let lookup_elapsed = lookup_started.elapsed();
        assert_eq!(found.unwrap().decision, "Decision 4999");

        let new = old.replace(
            "[one](decision:DEC-scale-00001)",
            "[one](decision:DEC-scale-00002)",
        ) + "\n<!-- agent:todo -->[one](decision:DEC-scale-00001)<!-- /agent:todo -->\n";
        let trailer_started = std::time::Instant::now();
        let trailers = trailers_for_diff(&old, &new);
        let trailer_elapsed = trailer_started.elapsed();
        assert_eq!(
            trailers.decision_ids,
            BTreeSet::from(["DEC-scale-00001".to_string()])
        );
        assert!(
            lookup_elapsed.as_secs() < 5,
            "lookup took {lookup_elapsed:?}"
        );
        assert!(
            trailer_elapsed.as_secs() < 5,
            "trailer diff took {trailer_elapsed:?}"
        );
        eprintln!(
            "decision_scale count=5000 lookup_ms={} trailer_ms={}",
            lookup_elapsed.as_millis(),
            trailer_elapsed.as_millis()
        );
    }

    #[test]
    fn commit_emits_sorted_decision_and_response_trailers() {
        let temp = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "agent-doc@example.invalid"],
            vec!["config", "user.name", "Agent Doc"],
        ] {
            let status = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
        let plan = plan(temp.path(), request("DEC-trailer", "[#r-abcdefabcdef]")).unwrap();
        apply(&plan).unwrap();
        assert!(commit_plan(&plan, Some("test decision")).unwrap());
        let output = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args(["log", "-1", "--pretty=%B"])
            .output()
            .unwrap();
        let message = String::from_utf8(output.stdout).unwrap();
        assert!(message.contains("Agent-Doc-Decision: DEC-trailer"));
        assert!(message.contains("Agent-Doc-Response: [#r-abcdefabcdef]"));
    }
}
