//! Focused, multi-surface document extraction.
//!
//! The planner first computes both complete document projections without I/O. Apply is
//! deliberately limited to detached documents: two independently attached CRDT
//! replicas cannot provide an atomic cross-document compare-and-swap. The I/O
//! adapter writes the destination first, rolls it back if the source write fails,
//! then checkpoints both baselines. A deterministic operation marker makes a
//! target-first crash resumable without duplicating content.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const SELECTIVE_LIST_COMPONENTS: [&str; 6] =
    ["queue", "todo", "backlog", "review", "icebox", "done"];
const ALLOWED_WHOLE_COMPONENTS: [&str; 10] = [
    "status", "exchange", "queue", "backlog", "pending", "todo", "review", "icebox", "done",
    "notes",
];

#[derive(Debug, Clone)]
pub struct FocusedExtractRequest {
    pub focus: Option<String>,
    pub rationale: Option<String>,
    pub ids: Vec<String>,
    pub exchange_node_ids: Vec<String>,
    pub archive_locators: Vec<String>,
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionManifest {
    focus: String,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default, alias = "exchange_nodes")]
    exchange_node_ids: Vec<String>,
    #[serde(default, alias = "exchange_archive_locators")]
    archive_locators: Vec<String>,
    #[serde(default)]
    components: Vec<String>,
}

pub fn request_from_selection_file(path: &Path) -> Result<FocusedExtractRequest> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read selection manifest {}", path.display()))?;
    let manifest: SelectionManifest = serde_json::from_str(&content)
        .with_context(|| format!("invalid selection manifest {}", path.display()))?;
    let focus = manifest.focus.trim();
    if focus.is_empty() {
        bail!("selection manifest focus must not be empty");
    }
    Ok(FocusedExtractRequest {
        focus: Some(focus.to_string()),
        rationale: manifest
            .rationale
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        ids: manifest.ids,
        exchange_node_ids: manifest.exchange_node_ids,
        archive_locators: manifest.archive_locators,
        components: manifest.components,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FocusedExtractTransfer {
    pub component: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub node_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub archive_locators: Vec<String>,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FocusedExtractPreview {
    pub operation_id: String,
    pub source: PathBuf,
    pub target: PathBuf,
    /// Focused extraction is a move: apply removes every listed transfer from the source.
    pub source_action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub target_would_be_created: bool,
    pub already_applied: bool,
    pub transfers: Vec<FocusedExtractTransfer>,
    pub unmatched_ids: Vec<String>,
    pub unmatched_exchange_node_ids: Vec<String>,
    pub unmatched_archive_locators: Vec<String>,
}

#[derive(Debug, Clone)]
struct ArchivedExchangeSelection {
    locator: String,
    text: String,
    node_ids: Vec<String>,
}

#[derive(Debug)]
struct PreparedExtract {
    preview: FocusedExtractPreview,
    source_before: String,
    target_before: Option<String>,
    source_after: String,
    target_after: String,
    /// Exact bytes each transfer removes from the source, in transfer order.
    /// `None` for a non-destructive transfer (an archived-context copy).
    source_removals: Vec<Option<String>>,
}

/// Typed refusal for an operation whose destination marker survived while the
/// destination lost content that operation moved (GH #324).
///
/// The operation ID hashes the source bytes, so re-running an extract on an
/// unchanged source re-derives the same marker. A marked target therefore
/// means "already received" only while it still holds every selected node;
/// otherwise removing the selection from the source again would destroy the
/// only remaining live copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedExtractTargetLostNodes {
    pub operation_id: String,
    pub target: PathBuf,
    /// `component:id` labels: exchange node IDs, tracked item IDs, archive
    /// locators, or a whole component with no stable IDs.
    pub missing: Vec<String>,
    /// Set when the loss was found by verifying the target against an earlier
    /// operation's recovery copy because the source was already stripped (the
    /// re-run derived a new operation ID, so the target carries no marker for
    /// it). `None` for the marked-target resume check.
    pub recovery_copy: Option<PathBuf>,
}

impl std::fmt::Display for FocusedExtractTargetLostNodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(recovery_copy) = &self.recovery_copy {
            return write!(
                f,
                "focused-extract operation {} already removed this selection from the source into {}, but the target no longer holds: {}; refusing to report the extract as already applied (no documents were changed). Restore the missing content in the target from the recovery copy {} before re-running",
                self.operation_id,
                self.target.display(),
                self.missing.join(", "),
                recovery_copy.display()
            );
        }
        write!(
            f,
            "focused-extract operation {} is already marked in {}, but the target no longer holds: {}; refusing to remove that content from the source again (no documents were changed). Restore the missing content in the target or recover it from the source's history before re-running",
            self.operation_id,
            self.target.display(),
            self.missing.join(", ")
        )
    }
}

impl std::error::Error for FocusedExtractTargetLostNodes {}

impl FocusedExtractTargetLostNodes {
    fn ops_log_line(&self) -> String {
        let mut line = format!(
            "focused_extract_target_lost_nodes operation={} target={} node_ids={}",
            self.operation_id,
            self.target.display(),
            self.missing.join(",")
        );
        if let Some(recovery_copy) = &self.recovery_copy {
            line.push_str(&format!(" recovery_copy={}", recovery_copy.display()));
        }
        line
    }
}

fn normalize_id(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_start_matches('#')
        .to_string()
}

fn canonical_component(raw: &str) -> String {
    match raw.trim() {
        "pending" => "backlog".to_string(),
        other => other.to_string(),
    }
}

fn lexical_absolute(path: &Path) -> PathBuf {
    use std::path::Component;

    let anchored = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut normalized = PathBuf::new();
    for component in anchored.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn lexical_relative(base: &Path, target: &Path) -> PathBuf {
    let base_components: Vec<_> = base.components().collect();
    let target_components: Vec<_> = target.components().collect();
    let common = base_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return target.to_path_buf();
    }
    let mut relative = PathBuf::new();
    for _ in common..base_components.len() {
        relative.push("..");
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn rebase_link(link: &str, source: &Path, target: &Path) -> String {
    if link.contains("://") || link.starts_with('#') || Path::new(link).is_absolute() {
        return link.to_string();
    }
    let source_dir = lexical_absolute(source.parent().unwrap_or_else(|| Path::new(".")));
    let target_dir = lexical_absolute(target.parent().unwrap_or_else(|| Path::new(".")));
    lexical_relative(&target_dir, &lexical_absolute(&source_dir.join(link)))
        .to_string_lossy()
        .into_owned()
}

fn render_focused_target_scaffold(
    title: &str,
    agent: &str,
    source_fm: &frontmatter::Frontmatter,
    source: &Path,
    target: &Path,
) -> Result<String> {
    let scaffold =
        super::extract::render_target_scaffold(title, agent, uuid::Uuid::new_v4(), source_fm);
    let (mut target_fm, body) = frontmatter::parse(&scaffold)?;
    target_fm.links = source_fm
        .links
        .iter()
        .map(|link| rebase_link(link, source, target))
        .collect();
    frontmatter::write(&target_fm, body)
}

fn content_mentions_id(content: &str, id: &str) -> bool {
    let bracketed = format!("[#{id}]");
    if content.contains(&bracketed) {
        return true;
    }
    let needle = format!("#{id}");
    content.match_indices(&needle).any(|(start, _)| {
        let end = start + needle.len();
        let before_ok = start == 0
            || !content[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        let after_ok = end == content.len()
            || !content[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        before_ok && after_ok
    })
}

fn tracked_ids(content: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut rest = content;
    while let Some(start) = rest.find("[#") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(']') else {
            break;
        };
        let id = normalize_id(&rest[..end]);
        if !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            ids.insert(id);
        }
        rest = &rest[end + 1..];
    }
    ids
}

fn done_entry_id(line: &str) -> Option<String> {
    let line = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))?;
    let start = line.find("[#")? + 2;
    let end = start + line[start..].find(']')?;
    let id = normalize_id(&line[start..end]);
    (!id.is_empty()
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        }))
    .then_some(id.to_ascii_lowercase())
}

/// Extract binary-owned `agent:done` archive entries without converting their
/// dated archive shape into active checklist items. An entry owns all bytes up
/// to the next flush-left archive entry, matching the done-archive reopen
/// parser's boundary contract.
fn extract_done_entries_by_id(body: &str, ids: &BTreeSet<String>) -> (String, String, Vec<String>) {
    let mut headers = Vec::new();
    let mut offset = 0usize;
    for raw in body.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(id) = done_entry_id(line) {
            headers.push((offset, id));
        }
        offset += raw.len();
    }

    let mut remaining = String::with_capacity(body.len());
    let mut moved = String::new();
    let mut found = Vec::new();
    let mut cursor = 0usize;
    for (index, (start, id)) in headers.iter().enumerate() {
        let end = headers
            .get(index + 1)
            .map(|(next, _)| *next)
            .unwrap_or(body.len());
        remaining.push_str(&body[cursor..*start]);
        if ids.contains(id) {
            moved.push_str(&body[*start..end]);
            found.push(id.clone());
        } else {
            remaining.push_str(&body[*start..end]);
        }
        cursor = end;
    }
    remaining.push_str(&body[cursor..]);
    (remaining, moved, found)
}

fn normalized_archive_locator(archive_path: &str, turn_ordinal: i64) -> String {
    format!("archive:{archive_path}#{turn_ordinal}")
}

fn archived_response_with_prompt(
    source: &Path,
    archive_path: &str,
    turn_ordinal: i64,
) -> Result<ArchivedExchangeSelection> {
    let turns = agent_doc_sqlite::archive_index::fetch_turn_window(
        source,
        archive_path,
        turn_ordinal,
        1,
        0,
    )?;
    let response = turns
        .iter()
        .find(|turn| turn.turn_ordinal == turn_ordinal)
        .with_context(|| {
            format!(
                "archive response not found for {}#{}",
                archive_path, turn_ordinal
            )
        })?;
    if response.speaker != "assistant" {
        bail!(
            "focused-extract archive locator is not an assistant response: {}",
            normalized_archive_locator(archive_path, turn_ordinal)
        );
    }

    let expected_source = source
        .canonicalize()
        .unwrap_or_else(|_| lexical_absolute(source));
    let project_root = agent_doc_fs::find_project_root(&expected_source)
        .with_context(|| format!("project root not found for {}", source.display()))?;
    let indexed_source = lexical_absolute(&project_root.join(&response.document_path));
    if indexed_source != expected_source {
        bail!(
            "focused-extract archive locator belongs to {}, not {}: {}",
            response.document_path,
            source.display(),
            normalized_archive_locator(archive_path, turn_ordinal)
        );
    }

    let prompt = turns.iter().find(|turn| {
        turn.speaker == "user" && turn.turn_ordinal.checked_add(1) == Some(turn_ordinal)
    });
    let text = match prompt {
        Some(prompt) => format!(
            "{}\n{}",
            prompt.text.trim_end_matches('\n'),
            response.text.trim_start_matches('\n')
        ),
        None => response.text.clone(),
    };
    let node_ids = parse_exchange_nodes(&text)
        .into_iter()
        .filter(|node| !matches!(node.kind, ExchangeNodeKind::Salient { .. }))
        .map(|node| node.node_id())
        .collect();
    Ok(ArchivedExchangeSelection {
        locator: normalized_archive_locator(&response.archive_path, response.turn_ordinal),
        text,
        node_ids,
    })
}

fn resolve_archive_locator(source: &Path, locator: &str) -> Result<ArchivedExchangeSelection> {
    let Some(locator_body) = locator.trim().strip_prefix("archive:") else {
        bail!("focused-extract archive locator must start with 'archive:': {locator}");
    };
    let Some((archive_path, ordinal)) = locator_body.rsplit_once('#') else {
        bail!("focused-extract archive locator must look like archive:<path>#<turn>");
    };
    let turn_ordinal: i64 = ordinal
        .parse()
        .with_context(|| format!("invalid archive turn '{ordinal}'"))?;
    let mut selection = archived_response_with_prompt(source, archive_path, turn_ordinal)
        .with_context(|| format!("archive locator not found: {locator}"))?;
    selection.locator = locator.trim().to_string();
    Ok(selection)
}

fn archived_node_selections(
    source: &Path,
    node_ids: &BTreeSet<String>,
) -> Result<(Vec<ArchivedExchangeSelection>, BTreeSet<String>)> {
    if node_ids.is_empty() {
        return Ok((Vec::new(), BTreeSet::new()));
    }
    let turns = agent_doc_sqlite::archive_index::list_recent_turns(source, None, None, usize::MAX)?;
    let mut selections = Vec::new();
    let mut matched = BTreeSet::new();
    for turn in turns {
        let archived =
            archived_response_with_prompt(source, &turn.archive_path, turn.turn_ordinal)?;
        let (remaining, moved, selected_ids, _, selected_explicit) =
            exchange_selection(&archived.text, &BTreeSet::new(), node_ids);
        let _ = remaining;
        if moved.trim().is_empty() {
            continue;
        }
        matched.extend(selected_explicit);
        selections.push(ArchivedExchangeSelection {
            locator: archived.locator,
            text: moved,
            node_ids: selected_ids,
        });
        if matched == *node_ids {
            break;
        }
    }
    Ok((selections, matched))
}

fn archive_selection_marker(locator: &str) -> String {
    let escaped = locator.replace('&', "&amp;").replace('"', "&quot;");
    format!("<!-- agent-doc-extract-archive locator=\"{escaped}\" -->")
}

fn render_archived_exchange(selection: &ArchivedExchangeSelection) -> String {
    format!(
        "{}\n{}\n<!-- /agent-doc-extract-archive -->",
        archive_selection_marker(&selection.locator),
        selection.text.trim_matches('\n')
    )
}

fn find_component(doc: &str, name: &str) -> Result<Option<element::Component>> {
    let components = element::parse(doc).context("failed to parse agent components")?;
    Ok(components.into_iter().find(|component| {
        component.name == name || (name == "backlog" && component.name == "pending")
    }))
}

fn ensure_component(mut doc: String, name: &str) -> Result<String> {
    if find_component(&doc, name)?.is_some() {
        return Ok(doc);
    }
    let heading = match name {
        "status" => "Status",
        "exchange" => "Exchange",
        "queue" => "Queue",
        "backlog" => "Backlog",
        "todo" => "Todo",
        "review" => "Review",
        "icebox" => "Icebox",
        "done" => "Done",
        "notes" => "Notes",
        _ => bail!("unsupported focused-extract component '{name}'"),
    };
    let patch = match name {
        "status" => " patch=replace",
        "exchange" => " patch=append",
        _ => "",
    };
    doc.push_str(&format!(
        "\n\n## {heading}\n\n<!-- agent:{name}{patch} -->\n<!-- /agent:{name} -->\n"
    ));
    Ok(doc)
}

fn append_to_component(doc: &str, name: &str, moved: &str) -> Result<String> {
    let component = find_component(doc, name)?
        .with_context(|| format!("target component '{name}' was not initialized"))?;
    let existing = component.content(doc);
    let separator = if existing.trim().is_empty() { "" } else { "\n" };
    let combined = format!(
        "{}{}{}\n",
        existing.trim_end(),
        separator,
        moved.trim_matches('\n')
    );
    Ok(component.replace_content(doc, &combined))
}

fn append_to_exchange_preserving_boundary(doc: &str, moved: &str) -> Result<String> {
    let component = find_component(doc, "exchange")?
        .context("target component 'exchange' was not initialized")?;
    if let Some(boundary_id) =
        agent_doc_element_boundary::boundary::find_boundary_id_in_component(doc, &component)
    {
        Ok(component.append_with_boundary(doc, moved, &boundary_id))
    } else {
        append_to_component(doc, "exchange", moved)
    }
}

fn operation_marker(operation_id: &str, source: &Path) -> String {
    format!(
        "<!-- agent-doc-extract operation=\"{operation_id}\" source=\"{}\" -->",
        source.display()
    )
}

fn source_pointer(operation_id: &str, target: &Path) -> String {
    format!(
        "<!-- agent-doc-extract-destination operation=\"{operation_id}\" target=\"{}\" -->",
        target.display()
    )
}

fn unchanged_target_message(target: &Path, target_would_be_created: bool) -> String {
    if target_would_be_created {
        format!("destination {} was not created", target.display())
    } else {
        format!("destination {} was left unchanged", target.display())
    }
}

fn attached_editor_refusal_message(
    source: &Path,
    target: &Path,
    source_attached: bool,
    target_attached: bool,
    target_would_be_created: bool,
    attached_during_preparation: bool,
) -> String {
    let mut attached = Vec::new();
    if source_attached {
        attached.push(format!("source {}", source.display()));
    }
    if target_attached {
        attached.push(format!("target {}", target.display()));
    }
    let timing = if attached_during_preparation {
        " An editor attached while the extraction plan was being prepared."
    } else {
        ""
    };
    format!(
        "focused extract did not apply; no documents were changed and {}. Live editor authority is attached to: {}.{} For supported JetBrains tabs, re-run the same command with --apply --coordinate-live-editors; otherwise close the listed editor tab(s), then re-run with --apply. Cross-document atomic compare-and-swap is unavailable across live CRDT authorities",
        unchanged_target_message(target, target_would_be_created),
        attached.join(", "),
        timing,
    )
}

const COORDINATED_EXTRACT_CAPABILITY: &str = "coordinated_extract_v1";
const COORDINATED_EXTRACT_DETACH_POLLS: usize = 201;
const COORDINATED_EXTRACT_DETACH_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(25);

#[derive(Debug, Clone)]
struct FrozenEditor {
    project_root: PathBuf,
    file: PathBuf,
    editor_pid: u64,
    editor_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveReplicaObservation {
    source: usize,
    target: usize,
}

impl LiveReplicaObservation {
    fn detached(self) -> bool {
        self.source == 0 && self.target == 0
    }
}

fn live_replica_count(file: &Path) -> Result<usize> {
    if !file.exists() {
        return Ok(0);
    }
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(file);
    let inspection = agent_doc_controller_io::project_controller::inspect_actor(
        &project_root,
        Some(file),
        None,
        None,
    )
    .with_context(|| {
        format!(
            "failed to inspect live CRDT replica membership for {}",
            file.display()
        )
    })?;
    inspection
        .editor_replica
        .map(|health| health.live_replicas)
        .with_context(|| {
            format!(
                "controller did not report CRDT replica membership for {}",
                file.display()
            )
        })
}

fn observe_live_replicas(source: &Path, target: &Path) -> Result<LiveReplicaObservation> {
    Ok(LiveReplicaObservation {
        source: live_replica_count(source)?,
        target: live_replica_count(target)?,
    })
}

fn wait_for_replica_detach_with<Observe, Wait>(
    max_polls: usize,
    mut observe: Observe,
    mut wait: Wait,
) -> Result<LiveReplicaObservation>
where
    Observe: FnMut() -> Result<LiveReplicaObservation>,
    Wait: FnMut(),
{
    anyhow::ensure!(
        max_polls > 0,
        "replica detach wait requires at least one poll"
    );
    let mut last = LiveReplicaObservation {
        source: 0,
        target: 0,
    };
    for poll in 0..max_polls {
        last = observe()?;
        if last.detached() {
            return Ok(last);
        }
        if poll + 1 < max_polls {
            wait();
        }
    }
    bail!(
        "timed out waiting for coordinated CRDT replica detach (source_live_replicas={}, target_live_replicas={})",
        last.source,
        last.target
    )
}

fn wait_for_live_replicas_to_detach(
    source: &Path,
    target: &Path,
) -> Result<LiveReplicaObservation> {
    wait_for_replica_detach_with(
        COORDINATED_EXTRACT_DETACH_POLLS,
        || observe_live_replicas(source, target),
        || std::thread::sleep(COORDINATED_EXTRACT_DETACH_POLL_INTERVAL),
    )
}

fn ensure_replica_mutation_boundary(observation: LiveReplicaObservation) -> Result<()> {
    anyhow::ensure!(
        observation.detached(),
        "focused extract did not apply; a CRDT replica attached after coordinated freeze (source_live_replicas={}, target_live_replicas={})",
        observation.source,
        observation.target
    );
    Ok(())
}

fn registration_was_frozen(
    registered_file: &Path,
    editor_pid: u64,
    editor_id: &str,
    frozen: &[FrozenEditor],
) -> bool {
    frozen.iter().any(|editor| {
        editor.file == registered_file
            && editor.editor_pid == editor_pid
            && editor.editor_id == editor_id
    })
}

fn ensure_only_frozen_editor_registrations(
    source: &Path,
    target: &Path,
    frozen: &[FrozenEditor],
) -> Result<()> {
    for file in [source, target] {
        if !file.exists() {
            continue;
        }
        for registration in
            agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file)?
        {
            let registered_file = registered_editor_file(file, &registration.path)?;
            anyhow::ensure!(
                registration_was_frozen(
                    &registered_file,
                    registration.pid,
                    &registration.editor_id,
                    frozen,
                ),
                "focused extract did not apply; editor {} pid {} attached to {} after coordinated freeze",
                registration.editor_id,
                registration.pid,
                registered_file.display()
            );
        }
    }
    Ok(())
}

fn frozen_file_identities(frozen: &[FrozenEditor]) -> BTreeSet<PathBuf> {
    frozen.iter().map(|editor| editor.file.clone()).collect()
}

fn frozen_file_for<'a>(file: &Path, frozen_files: &'a BTreeSet<PathBuf>) -> Option<&'a PathBuf> {
    file.canonicalize()
        .ok()
        .and_then(|identity| frozen_files.get(&identity))
}

fn write_extract_document(
    file: &Path,
    expected_current: &str,
    content: &str,
    frozen_files: &BTreeSet<PathBuf>,
) -> Result<()> {
    if let Some(frozen_file) = frozen_file_for(file, frozen_files) {
        agent_doc_document_realtime_io::atomic_write_detached_canonical_through_authority(
            frozen_file,
            expected_current,
            content,
            "focused_extract_coordinated_freeze",
        )
    } else {
        agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
            file,
            expected_current,
            content,
            "focused_extract",
        )
    }
}

fn registered_editor_file(requested_file: &Path, registered_path: &str) -> Result<PathBuf> {
    anyhow::ensure!(
        !registered_path.trim().is_empty(),
        "live editor registration for {} has no document path",
        requested_file.display()
    );
    let registered_file = PathBuf::from(registered_path);
    let requested_identity = requested_file.canonicalize().with_context(|| {
        format!(
            "failed to resolve focused-extract document {}",
            requested_file.display()
        )
    })?;
    let registered_identity = registered_file.canonicalize().with_context(|| {
        format!(
            "live editor registration names an unavailable document: {}",
            registered_file.display()
        )
    })?;
    anyhow::ensure!(
        requested_identity == registered_identity,
        "live editor registration path {} does not match focused-extract document {}",
        registered_file.display(),
        requested_file.display()
    );
    Ok(registered_file)
}

fn resume_frozen_editors(operation_id: &str, frozen: &[FrozenEditor]) -> Result<()> {
    let mut failures = Vec::new();
    for editor in frozen.iter().rev() {
        let mut last_error = None;
        for attempt in 0..3 {
            match agent_doc_ipc_io::send_resume_after_extract_to_editor(
                &editor.project_root,
                editor.editor_pid,
                &editor.editor_id,
                &editor.file.to_string_lossy(),
                operation_id,
            ) {
                Ok(_) => {
                    last_error = None;
                    break;
                }
                Err(error) => last_error = Some(error),
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        if let Some(error) = last_error {
            failures.push(format!("{}: {error:#}", editor.file.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "editor resume failed after three attempts: {}. The affected tabs remain read-only so recovery cannot overwrite the extracted result; reload or restart the editor plugin to re-register them",
            failures.join("; ")
        )
    }
}

fn freeze_live_editors(
    operation_id: &str,
    source: &Path,
    target: &Path,
) -> Result<Vec<FrozenEditor>> {
    let mut registrations = Vec::new();
    for file in [source, target] {
        if !file.exists() {
            continue;
        }
        for registration in
            agent_doc_controller_io::project_controller::live_editor_registrations_for_file(file)?
        {
            anyhow::ensure!(
                registration.editor_kind == "jetbrains"
                    && registration
                        .capabilities
                        .iter()
                        .any(|value| value == COORDINATED_EXTRACT_CAPABILITY),
                "live focused extract requires a JetBrains editor advertising {COORDINATED_EXTRACT_CAPABILITY}; {} is attached through {} {}. Upgrade/reload that editor plugin or omit --coordinate-live-editors",
                file.display(),
                registration.editor_kind,
                registration.editor_version,
            );
            let editor_file = registered_editor_file(file, &registration.path)?;
            registrations.push((editor_file, registration));
        }
    }

    let mut frozen = Vec::new();
    let freeze_result = (|| -> Result<()> {
        for (file, registration) in registrations {
            let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
                &file,
                "focused_extract_freeze",
            )?;
            let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&file);
            agent_doc_ipc_io::send_persist_current_to_editor(
                &project_root,
                registration.pid,
                &registration.editor_id,
                &file.to_string_lossy(),
                &agent_doc_hash::content_hash(&current),
                current.len(),
            )?;
            // Record the compensating resume before the freeze request crosses
            // the IPC boundary. A timeout can mean the editor applied the
            // freeze but its receipt was lost; rollback must still resume it.
            frozen.push(FrozenEditor {
                project_root: project_root.clone(),
                file: file.clone(),
                editor_pid: registration.pid,
                editor_id: registration.editor_id.clone(),
            });
            agent_doc_ipc_io::send_freeze_for_extract_to_editor(
                &project_root,
                registration.pid,
                &registration.editor_id,
                &file.to_string_lossy(),
                operation_id,
            )?;
        }

        // The tab remains open and its durable endpoint registration remains
        // live while frozen. Detachment is proven only by the controller-owned
        // CRDT relay membership count reaching zero.
        wait_for_live_replicas_to_detach(source, target)?;
        Ok(())
    })();
    if let Err(error) = freeze_result {
        let resume_error = resume_frozen_editors(operation_id, &frozen).err();
        return match resume_error {
            Some(resume_error) => Err(error.context(format!(
                "coordinated editor freeze failed and rollback resume also failed: {resume_error:#}"
            ))),
            None => {
                Err(error.context("coordinated editor freeze failed; frozen tabs were resumed"))
            }
        };
    }
    Ok(frozen)
}

fn exchange_selection(
    content: &str,
    ids: &BTreeSet<String>,
    explicit: &BTreeSet<String>,
) -> (
    String,
    String,
    Vec<String>,
    BTreeSet<String>,
    BTreeSet<String>,
) {
    let mut nodes = parse_exchange_nodes(content);
    // The exchange parser deliberately keeps a trailing boundary marker in the
    // last semantic node for byte-stable round trips. A focused move must not
    // treat that document-local skeleton as response content: retain it in the
    // source and let the target keep its own boundary identity.
    let trailing_boundary = nodes
        .last_mut()
        .and_then(split_trailing_exchange_boundary)
        .unwrap_or_default();
    let mut selected = vec![false; nodes.len()];
    let mut matched_ids = BTreeSet::new();
    let mut matched_explicit = BTreeSet::new();

    for (index, node) in nodes.iter().enumerate() {
        let rendered = node.render();
        let node_id = node.node_id();
        if explicit.contains(&node_id) {
            selected[index] = true;
            matched_explicit.insert(node_id);
        }
        for id in ids {
            if content_mentions_id(&rendered, id) {
                selected[index] = true;
                matched_ids.insert(id.clone());
            }
        }
    }

    // Preserve prompt/response adjacency for selected conversation turns.
    let initially_selected = selected.clone();
    for (index, is_selected) in initially_selected.into_iter().enumerate() {
        if !is_selected {
            continue;
        }
        match nodes[index].kind {
            ExchangeNodeKind::Response { .. } if index > 0 => {
                if matches!(nodes[index - 1].kind, ExchangeNodeKind::Prompt) {
                    selected[index - 1] = true;
                }
            }
            ExchangeNodeKind::Prompt if index + 1 < nodes.len() => {
                if matches!(nodes[index + 1].kind, ExchangeNodeKind::Response { .. }) {
                    selected[index + 1] = true;
                }
            }
            _ => {}
        }
    }

    let mut kept: Vec<ExchangeNode> = Vec::new();
    let mut moved: Vec<ExchangeNode> = Vec::new();
    let mut node_ids = Vec::new();
    for (node, is_selected) in nodes.into_iter().zip(selected) {
        if is_selected && !matches!(node.kind, ExchangeNodeKind::Salient { .. }) {
            node_ids.push(node.node_id());
            moved.push(node);
        } else {
            kept.push(node);
        }
    }
    let mut remaining = render_exchange_nodes(&kept);
    remaining.push_str(&trailing_boundary.concat());
    (
        remaining,
        render_exchange_nodes(&moved),
        node_ids,
        matched_ids,
        matched_explicit,
    )
}

fn split_trailing_exchange_boundary(node: &mut ExchangeNode) -> Option<Vec<String>> {
    let mut split = node.lines.len();
    let mut saw_boundary = false;
    while split > 0 {
        let trimmed = node.lines[split - 1].trim_end_matches(['\n', '\r']).trim();
        if trimmed.is_empty() {
            split -= 1;
        } else if trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->") {
            saw_boundary = true;
            split -= 1;
            break;
        } else {
            break;
        }
    }
    saw_boundary.then(|| node.lines.split_off(split))
}

fn target_covers_selectors(
    target: &str,
    ids: &BTreeSet<String>,
    explicit_nodes: &BTreeSet<String>,
    archive_locators: &BTreeSet<String>,
) -> Result<bool> {
    if ids.is_empty() && explicit_nodes.is_empty() && archive_locators.is_empty() {
        return Ok(false);
    }
    let mut covered_ids = BTreeSet::new();
    for component_name in SELECTIVE_LIST_COMPONENTS {
        let Some(component) = find_component(target, component_name)? else {
            continue;
        };
        for id in ids {
            let matches = if component_name == "done" {
                agent_doc_element_backlog::backlog::extract_done_item_own_ids(
                    component.content(target),
                )
                .contains(id)
            } else {
                content_mentions_id(component.content(target), id)
            };
            if matches {
                covered_ids.insert(id.clone());
            }
        }
    }
    let mut covered_nodes = BTreeSet::new();
    if let Some(exchange) = find_component(target, "exchange")? {
        for node in parse_exchange_nodes(exchange.content(target)) {
            let node_id = node.node_id();
            if explicit_nodes.contains(&node_id) {
                covered_nodes.insert(node_id);
            }
            let rendered = node.render();
            for id in ids {
                if content_mentions_id(&rendered, id) {
                    covered_ids.insert(id.clone());
                }
            }
        }
    }
    let covered_archive_locators = archive_locators
        .iter()
        .filter(|locator| target.contains(&archive_selection_marker(locator)))
        .cloned()
        .collect::<BTreeSet<_>>();
    Ok(covered_ids == *ids
        && covered_nodes == *explicit_nodes
        && covered_archive_locators == *archive_locators)
}

/// For an already-marked target, name every node of one transfer that the
/// target no longer holds. Exchange turns are held by their stable node ID.
/// Tracked items are held by their own component, or by `done` because a target
/// may legitimately complete an extracted item before a resumed apply.
fn marked_target_missing(
    target: &str,
    component_name: &str,
    moved: &str,
    node_ids: &[String],
    item_ids: &[String],
) -> Result<Vec<String>> {
    if component_name == "exchange" {
        let wanted: Vec<String> = if node_ids.is_empty() {
            parse_exchange_nodes(moved)
                .into_iter()
                .filter(|node| !matches!(node.kind, ExchangeNodeKind::Salient { .. }))
                .map(|node| node.node_id())
                .collect()
        } else {
            node_ids.to_vec()
        };
        let held: HashSet<String> = match find_component(target, "exchange")? {
            Some(exchange) => parse_exchange_nodes(exchange.content(target))
                .into_iter()
                .map(|node| node.node_id())
                .collect(),
            None => HashSet::new(),
        };
        return Ok(wanted
            .into_iter()
            .filter(|node_id| !held.contains(node_id))
            .map(|node_id| format!("exchange:{node_id}"))
            .collect());
    }
    if SELECTIVE_LIST_COMPONENTS.contains(&component_name) {
        let wanted: Vec<String> = if item_ids.is_empty() {
            tracked_ids(moved).into_iter().collect()
        } else {
            item_ids.to_vec()
        };
        if !wanted.is_empty() {
            // Every item of one focused extract usually shares the selected
            // ID, so "the ID appears somewhere in the target" would let one
            // surviving item vouch for its lost siblings. An item is held only
            // by its own component, or by `done` once the target completed it.
            let mut held = BTreeSet::new();
            for list in [component_name, "done"] {
                if let Some(component) = find_component(target, list)? {
                    held.extend(
                        tracked_ids(component.content(target))
                            .into_iter()
                            .map(|id| id.to_ascii_lowercase()),
                    );
                }
            }
            return Ok(wanted
                .into_iter()
                .filter(|id| !held.contains(&id.to_ascii_lowercase()))
                .map(|id| format!("{component_name}:#{id}"))
                .collect());
        }
    }
    // A component without stable IDs (status, notes, or an ID-less list) is
    // held only when its moved bytes are still present in the target component.
    let held = find_component(target, component_name)?
        .is_some_and(|component| component.content(target).contains(moved.trim()));
    Ok(if held {
        Vec::new()
    } else {
        vec![format!("{component_name}:whole_component")]
    })
}

fn prepare(
    source: &Path,
    target: &Path,
    source_content: String,
    target_content: Option<String>,
    source_fm: &frontmatter::Frontmatter,
    request: &FocusedExtractRequest,
) -> Result<PreparedExtract> {
    prepare_with_recovery(
        source,
        target,
        source_content,
        target_content,
        source_fm,
        request,
        &|operation_id: &str| read_recovery_archive(source, operation_id),
    )
}

/// Loads one earlier operation's recovery copy by operation ID, returning its
/// path and text, or `None` when no copy exists.
type RecoveryLoader<'a> = &'a dyn Fn(&str) -> Option<(PathBuf, String)>;

fn read_recovery_archive(source: &Path, operation_id: &str) -> Option<(PathBuf, String)> {
    let path = recovery_archive_path(source, operation_id);
    let text = std::fs::read_to_string(&path).ok()?;
    Some((path, text))
}

/// One labelled section of a recovery copy: the exact bytes one transfer
/// removed from the source, with the IDs the transfer recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoverySection {
    component: String,
    ids: Vec<String>,
    node_ids: Vec<String>,
    removed: String,
}

const RECOVERY_SECTION_OPEN: &str = "<!-- agent-doc-extract-removed ";
const RECOVERY_SECTION_CLOSE: &str = "<!-- /agent-doc-extract-removed -->\n";

fn recovery_header_attr(header: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let start = header.find(&needle)? + needle.len();
    let len = header[start..].find('"')?;
    Some(header[start..start + len].to_string())
}

fn split_recorded_list(raw: Option<String>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse the sections `render_recovery_archive` wrote. A truncated trailing
/// section (no closing marker) is ignored rather than guessed at.
fn parse_recovery_sections(text: &str) -> Vec<RecoverySection> {
    let mut sections = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(RECOVERY_SECTION_OPEN) {
        let after_open = &rest[open..];
        let Some(header_end) = after_open.find("-->\n") else {
            break;
        };
        let header = &after_open[..header_end];
        let body = &after_open[header_end + "-->\n".len()..];
        let Some(close) = body.find(RECOVERY_SECTION_CLOSE) else {
            break;
        };
        sections.push(RecoverySection {
            component: recovery_header_attr(header, "component").unwrap_or_default(),
            ids: split_recorded_list(recovery_header_attr(header, "ids")),
            node_ids: split_recorded_list(recovery_header_attr(header, "node_ids")),
            removed: body[..close].to_string(),
        });
        rest = &body[close + RECOVERY_SECTION_CLOSE.len()..];
    }
    sections
}

fn marker_operation_ids(content: &str, prefix: &str, suffix: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix(prefix)?
                .strip_suffix(suffix)
                .filter(|operation_id| !operation_id.is_empty() && !operation_id.contains('"'))
                .map(str::to_string)
        })
        .collect()
}

/// Operation IDs of earlier extracts from this source into this target, from
/// the source's destination pointers and the target's operation markers, in
/// first-seen order. The source pointer survives a stripped source even when
/// the target lost its marker.
fn recorded_operation_ids(
    source_content: &str,
    target_content: &str,
    source_identity: &Path,
    target_identity: &Path,
) -> Vec<String> {
    let pointer_suffix = format!("\" target=\"{}\" -->", target_identity.display());
    let marker_suffix = format!("\" source=\"{}\" -->", source_identity.display());
    let mut seen = BTreeSet::new();
    marker_operation_ids(
        source_content,
        "<!-- agent-doc-extract-destination operation=\"",
        &pointer_suffix,
    )
    .into_iter()
    .chain(marker_operation_ids(
        target_content,
        "<!-- agent-doc-extract operation=\"",
        &marker_suffix,
    ))
    .filter(|operation_id| seen.insert(operation_id.clone()))
    .collect()
}

fn recovery_section_selected(
    section: &RecoverySection,
    ids: &BTreeSet<String>,
    explicit_nodes: &BTreeSet<String>,
) -> bool {
    section.ids.iter().any(|id| ids.contains(&normalize_id(id)))
        || section
            .node_ids
            .iter()
            .any(|node_id| explicit_nodes.contains(node_id))
        || ids
            .iter()
            .any(|id| content_mentions_id(&section.removed, id))
}

/// For a re-run whose source holds none of the selection, verify the target
/// against the recovery copy of every earlier operation of this source/target
/// pair: each recorded section the current selectors select must still be held
/// by the target under the marked-target holding rules (exchange turns by
/// recorded node ID, tracked items by ID in their component or `done`).
/// Returns the first operation, in recorded order, whose moved nodes the target
/// lost. `None` when nothing is lost or no recovery copy exists (an extract
/// that predates recovery copies keeps the selector-coverage answer).
fn recorded_target_losses(
    source_content: &str,
    target_content: &str,
    source_identity: &Path,
    target_identity: &Path,
    ids: &BTreeSet<String>,
    explicit_nodes: &BTreeSet<String>,
    load_recovery: RecoveryLoader<'_>,
) -> Result<Option<FocusedExtractTargetLostNodes>> {
    if ids.is_empty() && explicit_nodes.is_empty() {
        return Ok(None);
    }
    for operation_id in recorded_operation_ids(
        source_content,
        target_content,
        source_identity,
        target_identity,
    ) {
        let Some((path, text)) = load_recovery(&operation_id) else {
            continue;
        };
        let mut missing = Vec::new();
        for section in parse_recovery_sections(&text)
            .iter()
            .filter(|section| recovery_section_selected(section, ids, explicit_nodes))
        {
            for label in marked_target_missing(
                target_content,
                &section.component,
                &section.removed,
                &section.node_ids,
                &section.ids,
            )? {
                if !missing.contains(&label) {
                    missing.push(label);
                }
            }
        }
        if !missing.is_empty() {
            return Ok(Some(FocusedExtractTargetLostNodes {
                operation_id,
                target: target_identity.to_path_buf(),
                missing,
                recovery_copy: Some(path),
            }));
        }
    }
    Ok(None)
}

fn prepare_with_recovery(
    source: &Path,
    target: &Path,
    source_content: String,
    target_content: Option<String>,
    source_fm: &frontmatter::Frontmatter,
    request: &FocusedExtractRequest,
    load_recovery: RecoveryLoader<'_>,
) -> Result<PreparedExtract> {
    let ids: BTreeSet<String> = request
        .ids
        .iter()
        .map(|id| normalize_id(id))
        .filter(|id| !id.is_empty())
        .collect();
    let explicit_nodes: BTreeSet<String> = request
        .exchange_node_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    let archive_locators: BTreeSet<String> = request
        .archive_locators
        .iter()
        .map(|locator| locator.trim().to_string())
        .filter(|locator| !locator.is_empty())
        .collect();
    let whole_components: BTreeSet<String> = request
        .components
        .iter()
        .map(|name| canonical_component(name))
        .filter(|name| !name.is_empty())
        .collect();
    if ids.is_empty()
        && explicit_nodes.is_empty()
        && archive_locators.is_empty()
        && whole_components.is_empty()
    {
        bail!(
            "focused extract requires at least one --ids, --exchange-node, --archive-locator, or --components selector"
        );
    }
    for component in &whole_components {
        if !ALLOWED_WHOLE_COMPONENTS.contains(&component.as_str()) {
            bail!("unsupported focused-extract component '{component}'");
        }
    }

    let source_identity = source
        .canonicalize()
        .unwrap_or_else(|_| lexical_absolute(source));
    let target_identity = target
        .canonicalize()
        .unwrap_or_else(|_| lexical_absolute(target));
    let operation_seed = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        source_identity.display(),
        target_identity.display(),
        request.focus.as_deref().unwrap_or_default(),
        request.rationale.as_deref().unwrap_or_default(),
        ids.iter().cloned().collect::<Vec<_>>().join(","),
        explicit_nodes.iter().cloned().collect::<Vec<_>>().join(","),
        archive_locators
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
        whole_components
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
        agent_doc_hash::content_hash(&source_content),
    );
    let operation_id = agent_doc_hash::short_content_hash(&operation_seed);
    let marker = operation_marker(&operation_id, &source_identity);
    let target_would_be_created = target_content.is_none();
    let title = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Focused session");
    let agent = source_fm.active_resume_harness();
    let target_before = target_content;
    let mut target_after = match target_before.clone() {
        Some(content) => content,
        None => render_focused_target_scaffold(
            title,
            agent,
            &super::extract::scaffold_inheritance_frontmatter(&source_content)?,
            source,
            target,
        )?,
    };
    let already_marked = target_after.contains(&marker);
    let mut source_after = source_content.clone();
    let mut transfers = Vec::new();
    let mut source_removals: Vec<Option<String>> = Vec::new();
    // Selected content an already-marked target no longer holds (GH #324).
    let mut target_lost: Vec<String> = Vec::new();
    let mut matched_ids = BTreeSet::new();
    let mut matched_explicit = BTreeSet::new();
    let mut matched_archive_locators = BTreeSet::new();

    for component_name in &whole_components {
        let Some(source_component) = find_component(&source_after, component_name)? else {
            bail!(
                "component '{component_name}' not found in {}",
                source.display()
            );
        };
        let moved = source_component.content(&source_after).to_string();
        if moved.trim().is_empty() {
            continue;
        }

        for id in &ids {
            if content_mentions_id(&moved, id) {
                matched_ids.insert(id.clone());
            }
        }
        let moved_node_ids = if component_name == "exchange" {
            let node_ids: BTreeSet<String> = parse_exchange_nodes(&moved)
                .into_iter()
                .map(|node| node.node_id())
                .collect();
            matched_explicit.extend(explicit_nodes.intersection(&node_ids).cloned());
            node_ids
        } else {
            BTreeSet::new()
        };

        if !already_marked {
            target_after = ensure_component(target_after, component_name)?;
            let target_component = find_component(&target_after, component_name)?.unwrap();
            let target_body = target_component.content(&target_after);
            if component_name == "exchange" {
                let target_node_ids: BTreeSet<String> = parse_exchange_nodes(target_body)
                    .into_iter()
                    .map(|node| node.node_id())
                    .collect();
                if let Some(collision) = moved_node_ids.intersection(&target_node_ids).next() {
                    bail!(
                        "target exchange already contains node {collision}; refusing whole-component duplication"
                    );
                }
            } else if SELECTIVE_LIST_COMPONENTS.contains(&component_name.as_str()) {
                let moved_ids = tracked_ids(&moved);
                let target_ids = tracked_ids(target_body);
                if let Some(collision) = moved_ids.intersection(&target_ids).next() {
                    bail!(
                        "target component '{component_name}' already contains tracked id {collision}; refusing whole-component duplication"
                    );
                }
            }
        }
        source_after = source_component.replace_content(&source_after, "\n");
        if !already_marked {
            target_after = append_to_component(&target_after, component_name, &moved)?;
        } else {
            target_lost.extend(marked_target_missing(
                &target_after,
                component_name,
                &moved,
                &[],
                &[],
            )?);
        }
        source_removals.push(Some(moved.clone()));
        transfers.push(FocusedExtractTransfer {
            component: component_name.clone(),
            mode: "whole_component".to_string(),
            ids: Vec::new(),
            node_ids: Vec::new(),
            archive_locators: Vec::new(),
            bytes: moved.len(),
        });
    }

    if !whole_components.contains("exchange")
        && (!ids.is_empty() || !explicit_nodes.is_empty())
        && let Some(source_exchange) = find_component(&source_after, "exchange")?
    {
        let body = source_exchange.content(&source_after);
        let (remaining, moved, node_ids, exchange_ids, exchange_explicit) =
            exchange_selection(body, &ids, &explicit_nodes);
        matched_ids.extend(exchange_ids);
        matched_explicit.extend(exchange_explicit);
        if !moved.trim().is_empty() {
            source_after = source_exchange.replace_content(&source_after, &remaining);
            if !already_marked {
                target_after = ensure_component(target_after, "exchange")?;
                let target_exchange = find_component(&target_after, "exchange")?.unwrap();
                let target_ids: HashSet<String> =
                    parse_exchange_nodes(target_exchange.content(&target_after))
                        .into_iter()
                        .map(|node| node.node_id())
                        .collect();
                if let Some(collision) = node_ids.iter().find(|id| target_ids.contains(*id)) {
                    bail!(
                        "target exchange already contains node {collision}; refusing to duplicate it"
                    );
                }
                target_after = append_to_exchange_preserving_boundary(&target_after, &moved)?;
            } else {
                target_lost.extend(marked_target_missing(
                    &target_after,
                    "exchange",
                    &moved,
                    &node_ids,
                    &[],
                )?);
            }
            source_removals.push(Some(moved.clone()));
            transfers.push(FocusedExtractTransfer {
                component: "exchange".to_string(),
                mode: "selected_nodes".to_string(),
                ids: Vec::new(),
                node_ids,
                archive_locators: Vec::new(),
                bytes: moved.len(),
            });
        }
    }

    if !whole_components.contains("exchange")
        && (!archive_locators.is_empty() || matched_explicit != explicit_nodes)
    {
        let mut archived = BTreeMap::<String, ArchivedExchangeSelection>::new();
        for locator in &archive_locators {
            let selection = resolve_archive_locator(source, locator)?;
            let selection_node_ids = selection.node_ids.iter().cloned().collect::<BTreeSet<_>>();
            matched_explicit.extend(explicit_nodes.intersection(&selection_node_ids).cloned());
            matched_archive_locators.insert(locator.clone());
            archived.insert(selection.locator.clone(), selection);
        }

        let unresolved_nodes = explicit_nodes
            .difference(&matched_explicit)
            .cloned()
            .collect::<BTreeSet<_>>();
        let (fallbacks, fallback_matches) = archived_node_selections(source, &unresolved_nodes)?;
        matched_explicit.extend(fallback_matches);
        for selection in fallbacks {
            archived
                .entry(selection.locator.clone())
                .or_insert(selection);
        }

        for selection in archived.into_values() {
            let marker = archive_selection_marker(&selection.locator);
            if target_after.contains(&marker) {
                continue;
            }
            let rendered = render_archived_exchange(&selection);
            if !already_marked {
                target_after = ensure_component(target_after, "exchange")?;
                let target_exchange = find_component(&target_after, "exchange")?.unwrap();
                let target_ids = parse_exchange_nodes(target_exchange.content(&target_after))
                    .into_iter()
                    .map(|node| node.node_id())
                    .collect::<HashSet<_>>();
                if let Some(collision) = selection
                    .node_ids
                    .iter()
                    .find(|node_id| target_ids.contains(*node_id))
                {
                    bail!(
                        "target exchange already contains archived node {collision}; refusing to duplicate it"
                    );
                }
                target_after = append_to_exchange_preserving_boundary(&target_after, &rendered)?;
            } else {
                // The operation marker survived but this copy's provenance
                // marker did not: the target lost the archived context.
                target_lost.push(format!("exchange:{}", selection.locator));
            }
            source_removals.push(None);
            transfers.push(FocusedExtractTransfer {
                component: "exchange".to_string(),
                mode: "archived_context_copy".to_string(),
                ids: Vec::new(),
                node_ids: selection.node_ids,
                archive_locators: vec![selection.locator],
                bytes: rendered.len(),
            });
        }
    }

    for component_name in SELECTIVE_LIST_COMPONENTS {
        if whole_components.contains(component_name) || ids.is_empty() {
            continue;
        }
        let Some(source_component) = find_component(&source_after, component_name)? else {
            continue;
        };
        let requested: Vec<String> = ids.iter().cloned().collect();
        let (remaining, moved, found) = if component_name == "done" {
            extract_done_entries_by_id(source_component.content(&source_after), &ids)
        } else {
            agent_doc_element_backlog::backlog::extract_items_by_id(
                source_component.content(&source_after),
                &requested,
            )?
        };
        if found.is_empty() {
            continue;
        }
        matched_ids.extend(found.iter().cloned());
        source_after = source_component.replace_content(
            &source_after,
            if remaining.trim().is_empty() {
                "\n"
            } else {
                &remaining
            },
        );
        if !already_marked {
            target_after = ensure_component(target_after, component_name)?;
            let target_component = find_component(&target_after, component_name)?.unwrap();
            let target_found = if component_name == "done" {
                let found = found.iter().cloned().collect::<BTreeSet<_>>();
                let (_, _, target_found) =
                    extract_done_entries_by_id(target_component.content(&target_after), &found);
                target_found
            } else {
                let (_, _, target_found) = agent_doc_element_backlog::backlog::extract_items_by_id(
                    target_component.content(&target_after),
                    &found,
                )?;
                target_found
            };
            if !target_found.is_empty() {
                bail!(
                    "target component '{component_name}' already contains selected id(s): {}",
                    target_found.join(", ")
                );
            }
            target_after = append_to_component(&target_after, component_name, &moved)?;
        } else {
            target_lost.extend(marked_target_missing(
                &target_after,
                component_name,
                &moved,
                &[],
                &found,
            )?);
        }
        source_removals.push(Some(moved.clone()));
        transfers.push(FocusedExtractTransfer {
            component: component_name.to_string(),
            mode: "selected_items".to_string(),
            ids: found,
            node_ids: Vec::new(),
            archive_locators: Vec::new(),
            bytes: moved.len(),
        });
    }

    if !target_lost.is_empty() {
        return Err(FocusedExtractTargetLostNodes {
            operation_id,
            target: target.to_path_buf(),
            missing: target_lost,
            recovery_copy: None,
        }
        .into());
    }

    let unmatched_ids: Vec<String> = ids.difference(&matched_ids).cloned().collect();
    let unmatched_exchange_node_ids: Vec<String> = explicit_nodes
        .difference(&matched_explicit)
        .cloned()
        .collect();
    let unmatched_archive_locators: Vec<String> = archive_locators
        .difference(&matched_archive_locators)
        .cloned()
        .collect();
    let target_already_covers_selectors =
        target_covers_selectors(&target_after, &ids, &explicit_nodes, &archive_locators)?;
    if !unmatched_ids.is_empty() && !(transfers.is_empty() && target_already_covers_selectors) {
        bail!(
            "focused-extract selector id(s) not found: {}",
            unmatched_ids.join(", ")
        );
    }
    if !unmatched_exchange_node_ids.is_empty()
        && !(transfers.is_empty() && target_already_covers_selectors)
    {
        bail!(
            "focused-extract exchange node id(s) not found: {}",
            unmatched_exchange_node_ids.join(", ")
        );
    }
    if !unmatched_archive_locators.is_empty()
        && !(transfers.is_empty() && target_already_covers_selectors)
    {
        bail!(
            "focused-extract archive locator(s) not found: {}",
            unmatched_archive_locators.join(", ")
        );
    }
    if transfers.is_empty() {
        if already_marked || target_already_covers_selectors {
            // Selector coverage only proves the target mentions each ID
            // somewhere. When an earlier operation already stripped the source,
            // its recovery copy records exactly what moved; the target must
            // still hold every recorded node before this counts as applied.
            if let Some(lost) = recorded_target_losses(
                &source_content,
                &target_after,
                &source_identity,
                &target_identity,
                &ids,
                &explicit_nodes,
                load_recovery,
            )? {
                return Err(FocusedExtractTargetLostNodes {
                    target: target.to_path_buf(),
                    ..lost
                }
                .into());
            }
            return Ok(PreparedExtract {
                preview: FocusedExtractPreview {
                    operation_id,
                    source: source.to_path_buf(),
                    target: target.to_path_buf(),
                    source_action: "remove_on_apply",
                    focus: request.focus.clone(),
                    rationale: request.rationale.clone(),
                    target_would_be_created,
                    already_applied: true,
                    transfers,
                    unmatched_ids,
                    unmatched_exchange_node_ids,
                    unmatched_archive_locators,
                },
                source_before: source_content.clone(),
                target_before,
                source_after: source_content,
                target_after,
                source_removals: Vec::new(),
            });
        }
        bail!("focused-extract selectors matched no movable content");
    }

    target_after = ensure_component(target_after, "notes")?;
    if !already_marked {
        target_after = append_to_component(&target_after, "notes", &marker)?;
    }
    source_after = ensure_component(source_after, "notes")?;
    let pointer = source_pointer(&operation_id, &target_identity);
    if !source_after.contains(&pointer) {
        source_after = append_to_component(&source_after, "notes", &pointer)?;
    }

    Ok(PreparedExtract {
        preview: FocusedExtractPreview {
            operation_id,
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            source_action: "remove_on_apply",
            focus: request.focus.clone(),
            rationale: request.rationale.clone(),
            target_would_be_created,
            already_applied: false,
            transfers,
            unmatched_ids,
            unmatched_exchange_node_ids,
            unmatched_archive_locators,
        },
        source_before: source_content,
        target_before,
        source_after,
        target_after,
        source_removals,
    })
}

/// Recovery copies live in a subdirectory so the compact-archive index, which
/// scans only the top level of `.agent-doc/archives`, never parses them as
/// compacted exchange history.
const RECOVERY_ARCHIVE_DIR: &str = ".agent-doc/archives/extract";

fn recovery_archive_path(source: &Path, operation_id: &str) -> PathBuf {
    let canonical = source
        .canonicalize()
        .unwrap_or_else(|_| lexical_absolute(source));
    let root = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    root.join(RECOVERY_ARCHIVE_DIR)
        .join(format!("{operation_id}.md"))
}

/// Render every byte the operation removes from the source, keyed by operation
/// ID and labelled with the transfer that moved it. `None` when the operation
/// removes nothing (an archived-context copy only).
fn render_recovery_archive(prepared: &PreparedExtract) -> Option<String> {
    let preview = &prepared.preview;
    let mut sections = String::new();
    for (transfer, removed) in preview.transfers.iter().zip(&prepared.source_removals) {
        let Some(removed) = removed else {
            continue;
        };
        sections.push_str(&format!(
            "<!-- agent-doc-extract-removed component=\"{}\" mode=\"{}\" ids=\"{}\" node_ids=\"{}\" -->\n",
            transfer.component,
            transfer.mode,
            transfer.ids.join(","),
            transfer.node_ids.join(",")
        ));
        sections.push_str(removed);
        if !removed.ends_with('\n') {
            sections.push('\n');
        }
        sections.push_str("<!-- /agent-doc-extract-removed -->\n\n");
    }
    if sections.is_empty() {
        return None;
    }
    Some(format!(
        "---\nkind: focused-extract-recovery\noperation: {}\nsource: {}\ntarget: {}\n---\n\n# Focused extract {}: content removed from the source\n\n{}",
        preview.operation_id,
        preview.source.display(),
        preview.target.display(),
        preview.operation_id,
        sections
    ))
}

fn write_recovery_archive(prepared: &PreparedExtract, source: &Path) -> Result<Option<PathBuf>> {
    let Some(rendered) = render_recovery_archive(prepared) else {
        return Ok(None);
    };
    let path = recovery_archive_path(source, &prepared.preview.operation_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    agent_doc_fs::write_atomic(&path, rendered.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(Some(path))
}

fn rollback(
    prepared: &PreparedExtract,
    source: &Path,
    target: &Path,
    frozen_files: &BTreeSet<PathBuf>,
) {
    if let Err(error) = write_extract_document(
        source,
        &prepared.source_after,
        &prepared.source_before,
        frozen_files,
    ) {
        eprintln!("[extract] WARNING: failed to roll back source: {error:#}");
    }
    match &prepared.target_before {
        Some(content) => {
            if let Err(error) = write_extract_document(
                target,
                &prepared.target_after,
                content,
                frozen_files,
            ) {
                eprintln!("[extract] WARNING: failed to roll back target: {error:#}");
            }
        }
        None => {
            if target.exists()
                && let Err(error) = std::fs::remove_file(target)
            {
                eprintln!("[extract] WARNING: failed to remove newly-created target: {error:#}");
            }
        }
    }
}

pub fn run(
    source: &Path,
    target: &Path,
    request: FocusedExtractRequest,
    apply: bool,
    bypass_claim: bool,
    coordinate_live_editors: bool,
) -> Result<()> {
    if !source.exists() {
        bail!("source file not found: {}", source.display());
    }
    let source_canonical = source.canonicalize()?;
    let target_identity = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    if source_canonical == target_identity {
        bail!("source and target must be different documents");
    }
    let source_content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        source,
        "focused_extract_prepare",
    )
    .with_context(|| format!("failed to read current {}", source.display()))?;
    let (source_fm, _) =
        agent_doc_frontmatter_io::session::parse_for_file(&source_content, source)?;
    let target_content = if target.exists() {
        Some(
            agent_doc_document_realtime_io::try_resolve_current_document_content(
                target,
                "focused_extract_prepare",
            )
            .with_context(|| format!("failed to read current {}", target.display()))?,
        )
    } else {
        None
    };
    let target_fm = target_content
        .as_ref()
        .map(|content| agent_doc_frontmatter_io::session::parse_for_file(content, target))
        .transpose()?
        .map(|(fm, _)| fm);
    agent_doc_frontmatter_io::security_review::enforce_cross_document_review(
        "extract",
        source,
        &source_fm,
        target,
        target_fm.as_ref(),
    )?;

    let prepared = match prepare(
        source,
        target,
        source_content,
        target_content,
        &source_fm,
        &request,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Some(lost) = error.downcast_ref::<FocusedExtractTargetLostNodes>() {
                // Attributed to the destination: it is the document that lost
                // the nodes, while the source was deliberately left unchanged.
                agent_doc_ops_log_io::log_op(target, &lost.ops_log_line());
            }
            return Err(error);
        }
    };
    println!("{}", serde_json::to_string_pretty(&prepared.preview)?);
    if !apply || prepared.preview.already_applied {
        if !apply {
            eprintln!(
                "[extract] preview only; no documents were changed and {}. Re-run the same command with --apply to write both documents",
                unchanged_target_message(target, prepared.preview.target_would_be_created)
            );
        }
        return Ok(());
    }

    let source_attached =
        agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(source);
    let target_attached = target.exists()
        && agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(target);
    if (source_attached || target_attached) && !coordinate_live_editors {
        bail!(attached_editor_refusal_message(
            source,
            target,
            source_attached,
            target_attached,
            prepared.preview.target_would_be_created,
            false,
        ));
    }
    let frozen = if source_attached || target_attached {
        freeze_live_editors(&prepared.preview.operation_id, source, target)?
    } else {
        Vec::new()
    };
    let frozen_files = frozen_file_identities(&frozen);
    let operation_id = prepared.preview.operation_id.clone();
    let apply_result = (|| -> Result<()> {
        if !bypass_claim {
            super::extract::check_target_ownership(target)?;
        }

        // Recheck the controller-owned CRDT membership at the mutation
        // boundary. Durable endpoint registrations intentionally remain live
        // for frozen, still-open tabs; only a live replica retains mutation
        // authority. Also refuse a newly arrived endpoint that was not part of
        // this operation's frozen set.
        ensure_replica_mutation_boundary(observe_live_replicas(source, target)?)?;
        ensure_only_frozen_editor_registrations(source, target, &frozen)?;

        // Frozen tabs retain their durable endpoint registration while their
        // relay member is intentionally absent. Capture that zero-replica
        // fence across apply plus rollback. Each write revalidates it, advances
        // controller canonical by CAS, then materializes disk as its projection.
        let _detached_canonical_scopes = frozen_files
            .iter()
            .map(|file| {
                agent_doc_document_realtime_io::begin_detached_canonical_authority_scope(
                    file,
                    "focused_extract_coordinated_freeze",
                )
            })
            .collect::<Result<Vec<_>>>()?;
        if !frozen.is_empty() {
            let source_disk = std::fs::read_to_string(source)?;
            anyhow::ensure!(
                source_disk == prepared.source_before,
                "source changed between focused-extract preview and coordinated freeze"
            );
            let target_disk = if target.exists() {
                Some(std::fs::read_to_string(target)?)
            } else {
                None
            };
            anyhow::ensure!(
                target_disk == prepared.target_before,
                "target changed between focused-extract preview and coordinated freeze"
            );
        }

        // Keep a recovery copy of everything this operation removes from the
        // source before either document changes, so a destination that later
        // loses the moved nodes is recoverable without git archaeology.
        if let Some(path) = write_recovery_archive(&prepared, source)
            .context("failed to write focused-extract recovery copy; no documents were changed")?
        {
            agent_doc_ops_log_io::log_op(
                source,
                &format!(
                    "focused_extract_recovery_archived operation={} path={}",
                    operation_id,
                    path.display()
                ),
            );
            eprintln!(
                "[extract] recovery copy of removed source content: {}",
                path.display()
            );
        }
        if let Some(parent) = target.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }
        write_extract_document(
            target,
            prepared.target_before.as_deref().unwrap_or(""),
            &prepared.target_after,
            &frozen_files,
        )
        .with_context(|| format!("failed to write target {}", target.display()))?;
        if let Err(error) = write_extract_document(
            source,
            &prepared.source_before,
            &prepared.source_after,
            &frozen_files,
        ) {
            rollback(&prepared, source, target, &frozen_files);
            return Err(error)
                .context("source write failed after target write; target rollback attempted");
        }
        if let Err(error) = agent_doc_snapshot_io::checkpoint_document_baseline(
            target,
            &prepared.target_after,
            agent_doc_ops_log_io::log_op,
        )
        .and_then(|_| {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                source,
                &prepared.source_after,
                agent_doc_ops_log_io::log_op,
            )
        }) {
            rollback(&prepared, source, target, &frozen_files);
            if let Err(snapshot_error) = agent_doc_snapshot_io::checkpoint_document_baseline(
                source,
                &prepared.source_before,
                agent_doc_ops_log_io::log_op,
            ) {
                eprintln!(
                    "[extract] WARNING: failed to restore source snapshot: {snapshot_error:#}"
                );
            }
            if let Some(target_before) = &prepared.target_before
                && let Err(snapshot_error) = agent_doc_snapshot_io::checkpoint_document_baseline(
                    target,
                    target_before,
                    agent_doc_ops_log_io::log_op,
                )
            {
                eprintln!(
                    "[extract] WARNING: failed to restore target snapshot: {snapshot_error:#}"
                );
            } else if prepared.target_before.is_none()
                && let Err(snapshot_error) = agent_doc_snapshot_io::clear_document_baseline(target)
            {
                eprintln!(
                    "[extract] WARNING: failed to clear new-target snapshot: {snapshot_error:#}"
                );
            }
            return Err(error).context("snapshot checkpoint failed; document rollback attempted");
        }
        eprintln!(
            "[extract] applied {}: {} → {}",
            operation_id,
            source.display(),
            target.display()
        );
        Ok(())
    })();
    let resume_result = resume_frozen_editors(&operation_id, &frozen);
    match (apply_result, resume_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.context("focused extract applied")),
        (Err(error), Err(resume_error)) => Err(error.context(format!(
            "focused extract failed and editor resume also failed: {resume_error:#}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinated_detach_waits_for_controller_replica_membership_to_reach_zero() {
        let mut observations = std::collections::VecDeque::from([
            LiveReplicaObservation {
                source: 1,
                target: 1,
            },
            LiveReplicaObservation {
                source: 0,
                target: 1,
            },
            LiveReplicaObservation {
                source: 0,
                target: 0,
            },
        ]);
        let mut waits = 0;

        let detached = wait_for_replica_detach_with(
            3,
            || Ok(observations.pop_front().unwrap()),
            || waits += 1,
        )
        .unwrap();

        assert!(detached.detached());
        assert_eq!(waits, 2);
    }

    #[test]
    fn coordinated_detach_timeout_reports_the_remaining_replica_membership() {
        let mut waits = 0;

        let error = wait_for_replica_detach_with(
            3,
            || {
                Ok(LiveReplicaObservation {
                    source: 0,
                    target: 1,
                })
            },
            || waits += 1,
        )
        .unwrap_err();

        assert_eq!(waits, 2);
        assert!(error.to_string().contains("source_live_replicas=0"));
        assert!(error.to_string().contains("target_live_replicas=1"));
    }

    #[test]
    fn coordinated_mutation_boundary_accepts_open_tabs_only_after_replica_detach() {
        ensure_replica_mutation_boundary(LiveReplicaObservation {
            source: 0,
            target: 0,
        })
        .unwrap();

        let source_error = ensure_replica_mutation_boundary(LiveReplicaObservation {
            source: 1,
            target: 0,
        })
        .unwrap_err();
        assert!(source_error.to_string().contains("source_live_replicas=1"));

        let target_error = ensure_replica_mutation_boundary(LiveReplicaObservation {
            source: 0,
            target: 1,
        })
        .unwrap_err();
        assert!(target_error.to_string().contains("target_live_replicas=1"));
    }

    #[test]
    fn coordinated_extract_uses_the_registered_editor_path_for_a_relative_cli_path() {
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::TempDir::new_in(&cwd).unwrap();
        let document = dir.path().join("source.md");
        std::fs::write(&document, "# source\n").unwrap();
        let registered = document.canonicalize().unwrap();
        let requested = document.strip_prefix(&cwd).unwrap();

        assert!(!requested.is_absolute());
        assert_eq!(
            registered_editor_file(requested, &registered.to_string_lossy()).unwrap(),
            registered.clone()
        );
        let frozen_files = BTreeSet::from([registered.clone()]);
        assert_eq!(frozen_file_for(requested, &frozen_files), Some(&registered));
        let frozen = [FrozenEditor {
            project_root: cwd,
            file: registered.clone(),
            editor_pid: 41,
            editor_id: "editor-1".to_string(),
        }];
        assert!(registration_was_frozen(
            &registered,
            41,
            "editor-1",
            &frozen
        ));
        assert!(!registration_was_frozen(
            &registered,
            42,
            "editor-2",
            &frozen
        ));
    }

    #[test]
    fn attached_editor_refusal_names_the_live_source_and_uncreated_destination() {
        let error = attached_editor_refusal_message(
            Path::new("tasks/sdk/sdk-dev.md"),
            Path::new("tasks/sdk/sdk-demo.md"),
            true,
            false,
            true,
            false,
        );

        assert!(error.contains("focused extract did not apply; no documents were changed"));
        assert!(error.contains("destination tasks/sdk/sdk-demo.md was not created"));
        assert!(error.contains("source tasks/sdk/sdk-dev.md"));
        assert!(error.contains("--apply --coordinate-live-editors"));
    }

    #[test]
    fn agent_selection_manifest_carries_semantic_intent_into_deterministic_request() {
        let dir = tempfile::TempDir::new().unwrap();
        let manifest = dir.path().join("legal-selection.json");
        std::fs::write(
            &manifest,
            r#"{
  "focus": "SDK legal and compliance",
  "rationale": "Keep counsel decisions with the legal backlog and their originating turns.",
  "ids": ["legal1"],
  "exchange_node_ids": ["r:abc123"],
  "archive_locators": ["archive:.agent-doc/archives/legal.md#2"],
  "components": ["notes"]
}"#,
        )
        .unwrap();

        let request = request_from_selection_file(&manifest).unwrap();
        assert_eq!(request.focus.as_deref(), Some("SDK legal and compliance"));
        assert!(request.rationale.unwrap().contains("counsel decisions"));
        assert_eq!(request.ids, ["legal1"]);
        assert_eq!(request.exchange_node_ids, ["r:abc123"]);
        assert_eq!(
            request.archive_locators,
            ["archive:.agent-doc/archives/legal.md#2"]
        );
        assert_eq!(request.components, ["notes"]);
    }

    #[test]
    fn focused_scaffold_inherits_context_and_rebases_relative_links() {
        let source_fm = frontmatter::Frontmatter {
            model: Some("high".to_string()),
            codex_model: Some("gpt-6.1-sol".to_string()),
            enable_tool_search: Some(true),
            no_mcp: Some(false),
            links: vec![
                "context.md".to_string(),
                "../legal/policy.md".to_string(),
                "https://example.com/reference".to_string(),
            ],
            ..Default::default()
        };
        let scaffold = render_focused_target_scaffold(
            "SDK Legal",
            "codex",
            &source_fm,
            Path::new("tasks/sdk.md"),
            Path::new("tasks/sdk/sdk-legal.md"),
        )
        .unwrap();
        let (target_fm, _) = frontmatter::parse(&scaffold).unwrap();

        assert_eq!(target_fm.model, source_fm.model);
        assert_eq!(target_fm.codex_model, source_fm.codex_model);
        assert_eq!(target_fm.enable_tool_search, Some(true));
        assert_eq!(target_fm.no_mcp, Some(false));
        assert_eq!(
            target_fm.links,
            [
                "../context.md",
                "../../legal/policy.md",
                "https://example.com/reference"
            ]
        );
        assert_eq!(target_fm.resume, None);
        assert_eq!(target_fm.branch, None);
    }

    fn source_doc() -> String {
        super::super::extract::render_target_scaffold(
            "Source",
            "codex",
            uuid::Uuid::nil(),
            &frontmatter::Frontmatter::default(),
        )
        .replace(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:exchange patch=append -->\n❯ Decide [#legal1]\n### Re: Legal\n\nDecision for #legal1.\n### Re: Dev\n\nUnrelated.\n",
        )
        .replace(
            "<!-- agent:todo priority -->\n",
            "<!-- agent:todo priority -->\n- [ ] [#legal1] Ask counsel\n- [ ] [#dev1] Ship SDK\n",
        )
        .replace(
            "<!-- agent:run priority subagents clear -->\n",
            "<!-- agent:run priority subagents clear -->\n- [#legal1]: Draft request\n- do [#dev1]\n",
        )
        .replace(
            "<!-- agent:backlog priority queue -->\n",
            "<!-- agent:backlog priority queue -->\n- [/] [#legal1] Privacy decision\n- [ ] [#dev1] Build client\n",
        )
        .replace(
            "<!-- agent:review -->\n",
            "<!-- agent:review -->\n- [/] [#legal1] Await approval\n",
        )
    }

    #[test]
    fn ids_select_completed_entries_from_done_without_rewriting_archive_shape() {
        let source_content = source_doc().replace(
            "<!-- agent:done -->\n",
            concat!(
                "<!-- agent:done -->\n",
                "- 2026-10-16 [#donelegal] Completed legal review\n",
                "  - evidence remains attached\n",
                "- 2026-10-17 [#doneother] Completed unrelated work\n",
            ),
        );
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["donelegal".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();

        assert!(
            prepared
                .target_after
                .contains("- 2026-10-16 [#donelegal] Completed legal review")
        );
        assert!(prepared.target_after.contains("evidence remains attached"));
        assert!(!prepared.source_after.contains("[#donelegal]"));
        assert!(prepared.source_after.contains("[#doneother]"));
        let done_transfer = prepared
            .preview
            .transfers
            .iter()
            .find(|transfer| transfer.component == "done")
            .unwrap();
        assert_eq!(done_transfer.ids, ["donelegal"]);
    }

    #[test]
    fn compacted_exchange_node_selector_falls_back_to_its_archive_locator() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.md");
        let target = dir.path().join("target.md");
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archived_prompt = "❯ Request archived legal decision.";
        let archived_response = "### Re: Archived legal decision\n\nCounsel approved [#archlegal].";
        let archived_pair = format!("{archived_prompt}\n{archived_response}");
        let archived_nodes = parse_exchange_nodes(&archived_pair);
        let archived_prompt_id = archived_nodes[0].node_id();
        let archived_node_id = archived_nodes[1].node_id();
        let archive = archive_dir.join("legal.md");
        std::fs::write(
            &archive,
            format!(
                "---\narchived_from: compact\narchived_at: 20261017-120000\ncomponent: exchange\ndocument: source.md\n---\n\n## User\n\n{archived_prompt}\n\n## Assistant\n\n{archived_response}\n"
            ),
        )
        .unwrap();
        let source_content = source_doc().replace(
            "❯ Decide [#legal1]\n### Re: Legal\n\nDecision for #legal1.\n### Re: Dev\n\nUnrelated.\n",
            "### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/legal.md`*\n",
        );
        std::fs::write(&source, &source_content).unwrap();

        let prepared = prepare(
            &source,
            &target,
            source_content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: vec![archived_node_id.clone()],
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();

        assert!(prepared.target_after.contains(archived_prompt));
        assert!(prepared.target_after.contains(archived_response));
        assert!(prepared.target_after.contains(
            "agent-doc-extract-archive locator=\"archive:.agent-doc/archives/legal.md#2\""
        ));
        let transfer = prepared
            .preview
            .transfers
            .iter()
            .find(|transfer| transfer.mode == "archived_context_copy")
            .unwrap();
        assert_eq!(transfer.node_ids, [archived_prompt_id, archived_node_id]);
        assert_eq!(
            transfer.archive_locators,
            ["archive:.agent-doc/archives/legal.md#2"]
        );
    }

    #[test]
    fn compacted_prompt_node_selector_resolves_through_the_response_archive_locator() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.md");
        let target = dir.path().join("target.md");
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archived_prompt = "❯ Request archived legal decision.";
        let archived_response = "### Re: Archived legal decision\n\nCounsel approved [#archlegal].";
        let archived_pair = format!("{archived_prompt}\n{archived_response}");
        let archived_nodes = parse_exchange_nodes(&archived_pair);
        let archived_prompt_id = archived_nodes[0].node_id();
        let archived_response_id = archived_nodes[1].node_id();
        std::fs::write(
            archive_dir.join("legal-prompt.md"),
            format!(
                "---\narchived_from: compact\narchived_at: 20261017-120002\ncomponent: exchange\ndocument: source.md\n---\n\n## User\n\n{archived_prompt}\n\n## Assistant\n\n{archived_response}\n"
            ),
        )
        .unwrap();
        let source_content = source_doc().replace(
            "❯ Decide [#legal1]\n### Re: Legal\n\nDecision for #legal1.\n### Re: Dev\n\nUnrelated.\n",
            "### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/legal-prompt.md`*\n",
        );
        std::fs::write(&source, &source_content).unwrap();

        let prepared = prepare(
            &source,
            &target,
            source_content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: vec![archived_prompt_id.clone()],
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();

        assert!(prepared.target_after.contains(archived_prompt));
        assert!(prepared.target_after.contains(archived_response));
        let transfer = prepared
            .preview
            .transfers
            .iter()
            .find(|transfer| transfer.mode == "archived_context_copy")
            .unwrap();
        assert_eq!(
            transfer.node_ids,
            [archived_prompt_id, archived_response_id]
        );
        assert_eq!(
            transfer.archive_locators,
            ["archive:.agent-doc/archives/legal-prompt.md#2"]
        );
    }

    #[test]
    fn selection_manifest_archive_locator_resolves_after_compaction() {
        let dir = tempfile::TempDir::new().unwrap();
        let source = dir.path().join("source.md");
        let target = dir.path().join("target.md");
        let archive_dir = dir.path().join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let archived_response = "### Re: Archived compliance\n\nRetained semantic context.";
        std::fs::write(
            archive_dir.join("compliance.md"),
            format!(
                "---\narchived_from: compact\narchived_at: 20261017-120001\ncomponent: exchange\ndocument: source.md\n---\n\n## User\n\n❯ Request archived compliance context\n\n## Assistant\n\n{archived_response}\n"
            ),
        )
        .unwrap();
        let source_content = source_doc();
        std::fs::write(&source, &source_content).unwrap();

        let request = FocusedExtractRequest {
            focus: Some("compliance".into()),
            rationale: None,
            ids: Vec::new(),
            exchange_node_ids: Vec::new(),
            archive_locators: vec!["archive:.agent-doc/archives/compliance.md#2".into()],
            components: Vec::new(),
        };
        let prepared = prepare(
            &source,
            &target,
            source_content,
            None,
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();

        assert!(
            prepared
                .target_after
                .contains("❯ Request archived compliance context")
        );
        assert!(prepared.target_after.contains(archived_response));
        assert!(prepared.preview.unmatched_archive_locators.is_empty());

        let rerun = prepare(
            &source,
            &target,
            prepared.source_after.clone(),
            Some(prepared.target_after.clone()),
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();
        assert!(rerun.preview.already_applied);
    }

    /// `#extractpresetcopy`: extracting into a new document writes only the
    /// source's own per-document overrides. The effective source frontmatter
    /// carries every `.agent-doc/project.md` preset (and project defaults such
    /// as `model`); copying it materialized the whole project preset map into
    /// the new document's frontmatter.
    #[test]
    fn new_target_inherits_only_document_local_presets_not_project_presets() {
        let project_md = concat!(
            "---\n",
            "model: project-model\n",
            "presets:\n",
            "  '#auth': 'I authorize you to do this.'\n",
            "  '#repo': 'repo'\n",
            "  '#push': 'commit + push: #auth'\n",
            "---\n",
        );
        let source_content = source_doc().replacen(
            "---\n",
            concat!(
                "---\n",
                "presets:\n",
                "  '#pr-aggregate': '#repo: Aggregate PR'\n",
                "  '#repo': 'haiven-dev/haiven-sdk'\n",
            ),
            1,
        );
        let mut defaults = frontmatter::FrontmatterDefaults::default();
        defaults.overlay_document(project_md).unwrap();
        let (effective, _) = defaults.parse(&source_content).unwrap();
        assert!(
            effective.prompt_presets.contains_key("#auth"),
            "fixture: the effective source frontmatter carries project presets"
        );
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("legal.md"),
            source_content,
            None,
            &effective,
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();
        let (target_fm, _) = frontmatter::parse(&prepared.target_after).unwrap();
        assert_eq!(
            target_fm.prompt_presets.keys().cloned().collect::<Vec<_>>(),
            ["#pr-aggregate", "#repo"],
            "only the source document's own presets are per-document overrides"
        );
        assert_eq!(
            target_fm.prompt_presets.get("#repo").map(String::as_str),
            Some("haiven-dev/haiven-sdk")
        );
        assert_eq!(target_fm.model, None, "project model default is not copied");
        assert!(
            !prepared
                .target_after
                .contains("I authorize you to do this.")
        );
    }

    #[test]
    fn focused_plan_moves_linked_surfaces_and_exchange_pair() {
        let source = Path::new("source.md");
        let target = Path::new("legal.md");
        let source_content = source_doc();
        let prepared = prepare(
            source,
            target,
            source_content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();
        assert!(
            prepared
                .target_after
                .contains("- [ ] [#legal1] Ask counsel")
        );
        assert!(prepared.target_after.contains("Draft request"));
        assert!(
            prepared
                .target_after
                .contains("- [/] [#legal1] Privacy decision")
        );
        assert!(
            prepared
                .target_after
                .contains("- [/] [#legal1] Await approval")
        );
        assert!(prepared.target_after.contains("❯ Decide [#legal1]"));
        assert!(prepared.target_after.contains("### Re: Legal"));
        assert!(!prepared.target_after.contains("### Re: Dev"));
        assert!(!prepared.source_after.contains("Ask counsel"));
        assert!(!prepared.source_after.contains("Draft request"));
        assert!(!prepared.source_after.contains("Privacy decision"));
        assert!(!prepared.source_after.contains("Await approval"));
        assert!(!prepared.source_after.contains("❯ Decide [#legal1]"));
        assert!(!prepared.source_after.contains("### Re: Legal"));
        assert!(prepared.source_after.contains("- [ ] [#dev1] Ship SDK"));
        assert_eq!(prepared.preview.source_action, "remove_on_apply");
        assert_eq!(
            serde_json::to_value(&prepared.preview).unwrap()["source_action"],
            "remove_on_apply"
        );
        assert!(
            prepared
                .source_after
                .contains("agent-doc-extract-destination")
        );
    }

    #[test]
    fn explicit_exchange_node_keeps_adjacent_prompt() {
        let content = source_doc();
        let exchange = find_component(&content, "exchange").unwrap().unwrap();
        let nodes = parse_exchange_nodes(exchange.content(&content));
        let response_id = nodes
            .iter()
            .find(|node| node.render().contains("### Re: Legal"))
            .unwrap()
            .node_id();
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: vec![response_id],
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();
        assert!(prepared.target_after.contains("❯ Decide [#legal1]"));
        assert!(prepared.target_after.contains("### Re: Legal"));
    }

    #[test]
    fn selected_trailing_response_keeps_each_documents_single_boundary() {
        let content = source_doc().replace(
            "<!-- /agent:exchange -->",
            "<!-- agent:boundary:source-boundary -->\n<!-- /agent:exchange -->",
        );
        let target_content = super::super::extract::render_target_scaffold(
            "Target",
            "codex",
            uuid::Uuid::from_u128(1),
            &frontmatter::Frontmatter::default(),
        )
        .replace(
            "<!-- /agent:exchange -->",
            "<!-- agent:boundary:target-boundary -->\n<!-- /agent:exchange -->",
        );
        let source_boundary =
            agent_doc_element_boundary::boundary::find_boundary_id(&content, "exchange").unwrap();
        let source_boundary_marker = format!("<!-- agent:boundary:{source_boundary} -->");
        let exchange = find_component(&content, "exchange").unwrap().unwrap();
        let response_id = parse_exchange_nodes(exchange.content(&content))
            .into_iter()
            .find(|node| node.render().contains("### Re: Dev"))
            .unwrap()
            .node_id();

        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            content,
            Some(target_content),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: vec![response_id],
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();

        let target_boundary = agent_doc_element_boundary::boundary::find_boundary_id(
            &prepared.target_after,
            "exchange",
        )
        .unwrap();
        let target_boundary_marker = format!("<!-- agent:boundary:{target_boundary} -->");
        assert_ne!(source_boundary, target_boundary);
        assert_eq!(
            prepared
                .source_after
                .matches(&source_boundary_marker)
                .count(),
            1
        );
        assert_eq!(
            prepared
                .target_after
                .matches(&target_boundary_marker)
                .count(),
            1
        );
        assert!(!prepared.target_after.contains(&source_boundary_marker));
        assert!(!prepared.source_after.contains("### Re: Dev"));
        let response_position = prepared.target_after.find("### Re: Dev").unwrap();
        let boundary_position = prepared.target_after.find(&target_boundary_marker).unwrap();
        assert!(response_position < boundary_position);
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&prepared.source_after),
            None
        );
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&prepared.target_after),
            None
        );
    }

    #[test]
    fn whole_component_move_initializes_complete_target_scaffold() {
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: vec!["notes".into()],
            },
        );
        assert!(
            prepared.is_err(),
            "empty component is not a movable selection"
        );
        let scaffold = super::super::extract::render_target_scaffold(
            "Target",
            "codex",
            uuid::Uuid::nil(),
            &frontmatter::Frontmatter::default(),
        );
        for component in ["todo", "review", "done", "notes"] {
            assert!(scaffold.contains(&format!("<!-- agent:{component}")));
        }
    }

    #[test]
    fn whole_exchange_component_satisfies_combined_id_and_node_selectors() {
        let content = source_doc();
        let exchange = find_component(&content, "exchange").unwrap().unwrap();
        let response_id = parse_exchange_nodes(exchange.content(&content))
            .into_iter()
            .find(|node| node.render().contains("### Re: Legal"))
            .unwrap()
            .node_id();
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            content,
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: vec![response_id],
                archive_locators: Vec::new(),
                components: vec!["exchange".into()],
            },
        )
        .unwrap();

        assert!(prepared.target_after.contains("### Re: Dev"));
        assert!(prepared.target_after.contains("Privacy decision"));
        assert!(!prepared.source_after.contains("### Re: Legal"));
    }

    #[test]
    fn whole_component_tracked_id_collision_is_rejected() {
        let source = source_doc();
        let target = super::super::extract::render_target_scaffold(
            "Target",
            "codex",
            uuid::Uuid::nil(),
            &frontmatter::Frontmatter::default(),
        )
        .replace(
            "<!-- agent:backlog priority queue -->\n",
            "<!-- agent:backlog priority queue -->\n- [ ] [#legal1] Existing target item\n",
        );
        let error = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source,
            Some(target),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: Vec::new(),
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: vec!["backlog".into()],
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("tracked id legal1"));
    }

    #[test]
    fn target_id_collision_is_rejected_before_writes() {
        let source = source_doc();
        let target = source_doc();
        let error = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source,
            Some(target),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("already contains node"));
    }

    #[test]
    fn completed_extract_is_idempotent_when_target_covers_selectors() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();
        let second = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            first.source_after,
            Some(first.target_after),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                focus: None,
                rationale: None,
                ids: vec!["legal1".into()],
                exchange_node_ids: Vec::new(),
                archive_locators: Vec::new(),
                components: Vec::new(),
            },
        )
        .unwrap();
        assert!(second.preview.already_applied);
        assert!(second.preview.transfers.is_empty());
    }

    #[test]
    fn target_first_interruption_resumes_source_removal_without_duplicate_append() {
        let original_source = source_doc();
        let request = FocusedExtractRequest {
            focus: Some("legal".into()),
            rationale: Some("separate legal work".into()),
            ids: vec!["legal1".into()],
            exchange_node_ids: Vec::new(),
            archive_locators: Vec::new(),
            components: Vec::new(),
        };
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source.clone(),
            None,
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();

        let resumed = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source,
            Some(first.target_after.clone()),
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();

        assert_eq!(resumed.preview.operation_id, first.preview.operation_id);
        assert_eq!(resumed.target_after, first.target_after);
        assert!(!resumed.source_after.contains("Ask counsel"));
        assert_eq!(
            resumed.target_after.matches("Ask counsel").count(),
            1,
            "retry must not append the selected content twice"
        );
    }

    fn legal_request() -> FocusedExtractRequest {
        FocusedExtractRequest {
            focus: Some("legal".into()),
            rationale: Some("separate legal work".into()),
            ids: vec!["legal1".into()],
            exchange_node_ids: Vec::new(),
            archive_locators: Vec::new(),
            components: Vec::new(),
        }
    }

    fn remove_once(doc: &str, needle: &str) -> String {
        assert_eq!(
            doc.matches(needle).count(),
            1,
            "fixture must contain {needle:?} once"
        );
        doc.replacen(needle, "", 1)
    }

    /// GH #324: the target kept this operation's marker but lost the moved
    /// exchange turns. A re-run on the unchanged source re-derives the same
    /// operation ID, so the old planner stripped the source again and appended
    /// nothing, leaving the turns in neither document.
    #[test]
    fn marked_target_that_lost_exchange_turns_refuses_to_strip_the_source_again() {
        let original_source = source_doc();
        let request = legal_request();
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source.clone(),
            None,
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();
        let exchange_node_ids = first
            .preview
            .transfers
            .iter()
            .find(|transfer| transfer.mode == "selected_nodes")
            .expect("the legal exchange pair moves")
            .node_ids
            .clone();
        assert_eq!(
            exchange_node_ids.len(),
            2,
            "prompt and response move together"
        );

        let lost_target = remove_once(
            &remove_once(&first.target_after, "❯ Decide [#legal1]\n"),
            "### Re: Legal\n\nDecision for #legal1.\n",
        );
        assert!(
            lost_target.contains(&operation_marker(
                &first.preview.operation_id,
                &Path::new("source.md")
                    .canonicalize()
                    .unwrap_or_else(|_| lexical_absolute(Path::new("source.md")))
            ))
        );

        let error = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source,
            Some(lost_target),
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap_err();
        let lost = error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(lost.operation_id, first.preview.operation_id);
        let expected = exchange_node_ids
            .iter()
            .map(|node_id| format!("exchange:{node_id}"))
            .collect::<Vec<_>>();
        assert_eq!(lost.missing, expected, "only the lost turns are named");
        assert!(
            lost.ops_log_line().starts_with(&format!(
                "focused_extract_target_lost_nodes operation={} ",
                first.preview.operation_id
            )),
            "{}",
            lost.ops_log_line()
        );
        assert!(error.to_string().contains("no documents were changed"));
    }

    #[test]
    fn marked_target_that_lost_a_tracked_item_refuses_and_names_it() {
        let original_source = source_doc();
        let request = legal_request();
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source.clone(),
            None,
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();
        // Sibling legal1 items survive in todo/queue/backlog; they must not
        // vouch for the lost review item.
        let lost_target = remove_once(&first.target_after, "- [/] [#legal1] Await approval\n");
        assert!(lost_target.contains("- [ ] [#legal1] Ask counsel\n"));
        let error = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source,
            Some(lost_target),
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap_err();
        let lost = error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(lost.missing, vec!["review:#legal1".to_string()]);
    }

    #[test]
    fn marked_target_item_moved_to_done_still_resumes_source_removal() {
        let original_source = source_doc();
        let request = legal_request();
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source.clone(),
            None,
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();
        // The target legitimately completed its todo item after the extract.
        let completed_target = remove_once(&first.target_after, "- [ ] [#legal1] Ask counsel\n")
            .replace(
                "<!-- agent:done -->\n",
                "<!-- agent:done -->\n- 2026-10-19 [#legal1] Ask counsel\n",
            );
        let resumed = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            original_source,
            Some(completed_target.clone()),
            &frontmatter::Frontmatter::default(),
            &request,
        )
        .unwrap();
        assert_eq!(resumed.target_after, completed_target);
        assert!(!resumed.source_after.contains("Ask counsel"));
    }

    #[test]
    fn recovery_archive_holds_every_byte_removed_from_the_source() {
        let prepared = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        assert_eq!(
            prepared.source_removals.len(),
            prepared.preview.transfers.len()
        );
        let archive = render_recovery_archive(&prepared).expect("a move removes content");
        assert!(archive.contains(&format!("operation: {}", prepared.preview.operation_id)));
        for removed in [
            "❯ Decide [#legal1]",
            "### Re: Legal\n\nDecision for #legal1.",
            "- [ ] [#legal1] Ask counsel",
            "- [ ] [#legal1] : Draft request",
            "- [/] [#legal1] Privacy decision",
            "- [/] [#legal1] Await approval",
        ] {
            assert!(
                archive.contains(removed),
                "missing {removed:?} in\n{archive}"
            );
            assert!(!prepared.source_after.contains(removed));
        }
        assert!(
            !archive.contains("Unrelated."),
            "kept content is not archived"
        );
        assert!(
            recovery_archive_path(Path::new("source.md"), &prepared.preview.operation_id)
                .ends_with(format!(
                    ".agent-doc/archives/extract/{}.md",
                    prepared.preview.operation_id
                ))
        );
    }

    /// The recovery copy of `first`, served by operation ID the way
    /// `read_recovery_archive` serves `.agent-doc/archives/extract/<id>.md`.
    fn recovery_loader(
        copies: Vec<&PreparedExtract>,
    ) -> impl Fn(&str) -> Option<(PathBuf, String)> {
        let copies: Vec<(String, String)> = copies
            .into_iter()
            .map(|prepared| {
                (
                    prepared.preview.operation_id.clone(),
                    render_recovery_archive(prepared).expect("a move removes content"),
                )
            })
            .collect();
        move |operation_id: &str| {
            copies
                .iter()
                .find(|(id, _)| id == operation_id)
                .map(|(id, text)| {
                    (
                        PathBuf::from(format!(".agent-doc/archives/extract/{id}.md")),
                        text.clone(),
                    )
                })
        }
    }

    fn no_recovery_copy(_: &str) -> Option<(PathBuf, String)> {
        None
    }

    fn rerun_stripped(
        first: &PreparedExtract,
        target_content: String,
        load_recovery: RecoveryLoader<'_>,
    ) -> Result<PreparedExtract> {
        prepare_with_recovery(
            Path::new("source.md"),
            Path::new("target.md"),
            first.source_after.clone(),
            Some(target_content),
            &frontmatter::Frontmatter::default(),
            &legal_request(),
            load_recovery,
        )
    }

    fn lost_legal_turns(target: &str) -> String {
        remove_once(
            &remove_once(target, "❯ Decide [#legal1]\n"),
            "### Re: Legal\n\nDecision for #legal1.\n",
        )
    }

    #[test]
    fn recovery_sections_round_trip_each_transfer_label() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        let sections = parse_recovery_sections(&render_recovery_archive(&first).unwrap());
        assert_eq!(sections.len(), first.preview.transfers.len());
        for ((section, transfer), removed) in sections
            .iter()
            .zip(&first.preview.transfers)
            .zip(&first.source_removals)
        {
            assert_eq!(section.component, transfer.component);
            assert_eq!(section.ids, transfer.ids);
            assert_eq!(section.node_ids, transfer.node_ids);
            let removed = removed.as_deref().unwrap();
            assert_eq!(section.removed.trim_end(), removed.trim_end());
        }
    }

    /// GH #324 gap: once the source was stripped, a re-run derives a new
    /// operation ID and moves nothing, so only selector coverage was checked.
    /// Surviving `[#legal1]` list items vouched for the lost exchange turns
    /// and the re-run reported `already_applied`. The recovery copy's recorded
    /// node IDs now decide.
    #[test]
    fn stripped_source_rerun_refuses_when_target_lost_recorded_turns() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        let node_ids = first
            .preview
            .transfers
            .iter()
            .find(|transfer| transfer.mode == "selected_nodes")
            .expect("the legal exchange pair moves")
            .node_ids
            .clone();
        let lost_target = lost_legal_turns(&first.target_after);
        assert!(lost_target.contains("- [ ] [#legal1] Ask counsel\n"));

        // Without a recovery copy (an extract that predates them) selector
        // coverage still answers, which is exactly the loose answer this
        // check replaces.
        let legacy = rerun_stripped(&first, lost_target.clone(), &no_recovery_copy).unwrap();
        assert!(legacy.preview.already_applied);
        assert_ne!(legacy.preview.operation_id, first.preview.operation_id);

        let loader = recovery_loader(vec![&first]);
        let error = rerun_stripped(&first, lost_target, &loader).unwrap_err();
        let lost = error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(
            lost.operation_id, first.preview.operation_id,
            "the refusal names the operation that moved the turns"
        );
        assert_eq!(
            lost.missing,
            node_ids
                .iter()
                .map(|node_id| format!("exchange:{node_id}"))
                .collect::<Vec<_>>()
        );
        let recovery_copy = PathBuf::from(format!(
            ".agent-doc/archives/extract/{}.md",
            first.preview.operation_id
        ));
        assert_eq!(lost.recovery_copy.as_deref(), Some(recovery_copy.as_path()));
        assert!(
            lost.ops_log_line()
                .ends_with(&format!(" recovery_copy={}", recovery_copy.display())),
            "{}",
            lost.ops_log_line()
        );
        let message = error.to_string();
        assert!(message.contains("refusing to report the extract as already applied"));
        assert!(message.contains("no documents were changed"));
        assert!(message.contains(&recovery_copy.display().to_string()));
    }

    #[test]
    fn stripped_source_rerun_refuses_when_target_lost_a_recorded_item() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        // Sibling legal1 items survive elsewhere; they must not vouch for the
        // lost review item.
        let lost_target = remove_once(&first.target_after, "- [/] [#legal1] Await approval\n");
        let loader = recovery_loader(vec![&first]);
        let error = rerun_stripped(&first, lost_target, &loader).unwrap_err();
        let lost = error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(lost.missing, vec!["review:#legal1".to_string()]);
    }

    /// The source's destination pointer survives the strip, so the recovery
    /// copy is still found when the target lost its operation marker too.
    #[test]
    fn stripped_source_rerun_finds_the_recovery_copy_without_the_target_marker() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        let marker = operation_marker(
            &first.preview.operation_id,
            &Path::new("source.md")
                .canonicalize()
                .unwrap_or_else(|_| lexical_absolute(Path::new("source.md"))),
        );
        let lost_target = lost_legal_turns(&remove_once(&first.target_after, &marker));
        let loader = recovery_loader(vec![&first]);
        let error = rerun_stripped(&first, lost_target, &loader).unwrap_err();
        let lost = error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(lost.operation_id, first.preview.operation_id);
        assert_eq!(lost.missing.len(), 2);
    }

    #[test]
    fn stripped_source_rerun_with_a_holding_target_stays_already_applied() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        let loader = recovery_loader(vec![&first]);
        let intact = rerun_stripped(&first, first.target_after.clone(), &loader).unwrap();
        assert!(intact.preview.already_applied);
        assert!(intact.preview.transfers.is_empty());
        assert_eq!(intact.source_after, first.source_after);
        assert_eq!(intact.target_after, first.target_after);

        // A target that completed its todo item still holds it via `done`.
        let completed = remove_once(&first.target_after, "- [ ] [#legal1] Ask counsel\n").replace(
            "<!-- agent:done -->\n",
            "<!-- agent:done -->\n- 2026-10-19 [#legal1] Ask counsel\n",
        );
        let resumed = rerun_stripped(&first, completed, &loader).unwrap();
        assert!(resumed.preview.already_applied);
    }

    /// A loss under another operation the current selectors do not select
    /// does not block this selection's idempotent re-run.
    #[test]
    fn stripped_source_rerun_ignores_losses_from_unselected_operations() {
        let first = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            source_doc(),
            None,
            &frontmatter::Frontmatter::default(),
            &legal_request(),
        )
        .unwrap();
        let dev = prepare(
            Path::new("source.md"),
            Path::new("target.md"),
            first.source_after.clone(),
            Some(first.target_after.clone()),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                ids: vec!["dev1".into()],
                ..legal_request()
            },
        )
        .unwrap();
        assert!(!dev.preview.already_applied);
        let lost_dev_item = remove_once(&dev.target_after, "- [ ] [#dev1] Ship SDK\n");
        let loader = recovery_loader(vec![&first, &dev]);
        let legal_rerun = prepare_with_recovery(
            Path::new("source.md"),
            Path::new("target.md"),
            dev.source_after.clone(),
            Some(lost_dev_item.clone()),
            &frontmatter::Frontmatter::default(),
            &legal_request(),
            &loader,
        )
        .unwrap();
        assert!(legal_rerun.preview.already_applied);

        let dev_error = prepare_with_recovery(
            Path::new("source.md"),
            Path::new("target.md"),
            dev.source_after.clone(),
            Some(lost_dev_item),
            &frontmatter::Frontmatter::default(),
            &FocusedExtractRequest {
                ids: vec!["dev1".into()],
                ..legal_request()
            },
            &loader,
        )
        .unwrap_err();
        let lost = dev_error
            .downcast_ref::<FocusedExtractTargetLostNodes>()
            .expect("typed lost-node refusal");
        assert_eq!(lost.operation_id, dev.preview.operation_id);
        assert_eq!(lost.missing, vec!["todo:#dev1".to_string()]);
    }
}
