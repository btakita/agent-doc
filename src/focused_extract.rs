//! Focused, multi-surface document extraction.
//!
//! The planner first computes both complete document projections without I/O. Apply is
//! deliberately limited to detached documents: two independently attached CRDT
//! replicas cannot provide an atomic cross-document compare-and-swap. The I/O
//! adapter writes the destination first, rolls it back if the source write fails,
//! then checkpoints both baselines. A deterministic operation marker makes a
//! target-first crash resumable without duplicating content.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const SELECTIVE_LIST_COMPONENTS: [&str; 5] = ["queue", "todo", "backlog", "review", "icebox"];
const ALLOWED_WHOLE_COMPONENTS: [&str; 9] = [
    "status", "exchange", "queue", "backlog", "pending", "todo", "review", "icebox", "notes",
];

#[derive(Debug, Clone)]
pub struct FocusedExtractRequest {
    pub focus: Option<String>,
    pub rationale: Option<String>,
    pub ids: Vec<String>,
    pub exchange_node_ids: Vec<String>,
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
}

#[derive(Debug)]
struct PreparedExtract {
    preview: FocusedExtractPreview,
    source_before: String,
    target_before: Option<String>,
    source_after: String,
    target_after: String,
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
        "focused extract did not apply; no documents were changed and {}. Live editor authority is attached to: {}.{} Close the listed editor tab(s), then re-run the same command with --apply. Cross-document atomic compare-and-swap is unavailable across live CRDT authorities",
        unchanged_target_message(target, target_would_be_created),
        attached.join(", "),
        timing,
    )
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
    let nodes = parse_exchange_nodes(content);
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
    (
        render_exchange_nodes(&kept),
        render_exchange_nodes(&moved),
        node_ids,
        matched_ids,
        matched_explicit,
    )
}

fn target_covers_selectors(
    target: &str,
    ids: &BTreeSet<String>,
    explicit_nodes: &BTreeSet<String>,
) -> Result<bool> {
    if ids.is_empty() && explicit_nodes.is_empty() {
        return Ok(false);
    }
    let mut covered_ids = BTreeSet::new();
    for component_name in SELECTIVE_LIST_COMPONENTS {
        let Some(component) = find_component(target, component_name)? else {
            continue;
        };
        for id in ids {
            if content_mentions_id(component.content(target), id) {
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
    Ok(covered_ids == *ids && covered_nodes == *explicit_nodes)
}

fn prepare(
    source: &Path,
    target: &Path,
    source_content: String,
    target_content: Option<String>,
    source_fm: &frontmatter::Frontmatter,
    request: &FocusedExtractRequest,
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
    let whole_components: BTreeSet<String> = request
        .components
        .iter()
        .map(|name| canonical_component(name))
        .filter(|name| !name.is_empty())
        .collect();
    if ids.is_empty() && explicit_nodes.is_empty() && whole_components.is_empty() {
        bail!(
            "focused extract requires at least one --ids, --exchange-node, or --components selector"
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
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        source_identity.display(),
        target_identity.display(),
        request.focus.as_deref().unwrap_or_default(),
        request.rationale.as_deref().unwrap_or_default(),
        ids.iter().cloned().collect::<Vec<_>>().join(","),
        explicit_nodes.iter().cloned().collect::<Vec<_>>().join(","),
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
    let agent = source_fm.agent.as_deref().unwrap_or("codex");
    let target_before = target_content;
    let mut target_after = match target_before.clone() {
        Some(content) => content,
        None => render_focused_target_scaffold(title, agent, source_fm, source, target)?,
    };
    let already_marked = target_after.contains(&marker);
    let mut source_after = source_content.clone();
    let mut transfers = Vec::new();
    let mut matched_ids = BTreeSet::new();
    let mut matched_explicit = BTreeSet::new();

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
        }
        transfers.push(FocusedExtractTransfer {
            component: component_name.clone(),
            mode: "whole_component".to_string(),
            ids: Vec::new(),
            node_ids: Vec::new(),
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
                target_after = append_to_component(&target_after, "exchange", &moved)?;
            }
            transfers.push(FocusedExtractTransfer {
                component: "exchange".to_string(),
                mode: "selected_nodes".to_string(),
                ids: Vec::new(),
                node_ids,
                bytes: moved.len(),
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
        let (remaining, moved, found) = agent_doc_element_backlog::backlog::extract_items_by_id(
            source_component.content(&source_after),
            &requested,
        )?;
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
            let (_, _, target_found) = agent_doc_element_backlog::backlog::extract_items_by_id(
                target_component.content(&target_after),
                &found,
            )?;
            if !target_found.is_empty() {
                bail!(
                    "target component '{component_name}' already contains selected id(s): {}",
                    target_found.join(", ")
                );
            }
            target_after = append_to_component(&target_after, component_name, &moved)?;
        }
        transfers.push(FocusedExtractTransfer {
            component: component_name.to_string(),
            mode: "selected_items".to_string(),
            ids: found,
            node_ids: Vec::new(),
            bytes: moved.len(),
        });
    }

    let unmatched_ids: Vec<String> = ids.difference(&matched_ids).cloned().collect();
    let unmatched_exchange_node_ids: Vec<String> = explicit_nodes
        .difference(&matched_explicit)
        .cloned()
        .collect();
    let target_already_covers_selectors =
        target_covers_selectors(&target_after, &ids, &explicit_nodes)?;
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
    if transfers.is_empty() {
        if already_marked || target_already_covers_selectors {
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
                },
                source_before: source_content.clone(),
                target_before,
                source_after: source_content,
                target_after,
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
        },
        source_before: source_content,
        target_before,
        source_after,
        target_after,
    })
}

fn rollback(prepared: &PreparedExtract, source: &Path, target: &Path) {
    if let Err(error) = agent_doc_document_realtime_io::atomic_write_through_authority(
        source,
        &prepared.source_before,
    ) {
        eprintln!("[extract] WARNING: failed to roll back source: {error:#}");
    }
    match &prepared.target_before {
        Some(content) => {
            if let Err(error) =
                agent_doc_document_realtime_io::atomic_write_through_authority(target, content)
            {
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
    let source_content = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read {}", source.display()))?;
    let (source_fm, _) =
        agent_doc_frontmatter_io::session::parse_for_file(&source_content, source)?;
    let target_content = if target.exists() {
        Some(
            std::fs::read_to_string(target)
                .with_context(|| format!("failed to read {}", target.display()))?,
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

    let prepared = prepare(
        source,
        target,
        source_content,
        target_content,
        &source_fm,
        &request,
    )?;
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
    if source_attached || target_attached {
        bail!(attached_editor_refusal_message(
            source,
            target,
            source_attached,
            target_attached,
            prepared.preview.target_would_be_created,
            false,
        ));
    }
    if !bypass_claim {
        super::extract::check_target_ownership(target)?;
    }

    // Recheck at the mutation boundary as well as command admission. Planning,
    // security review, or operator inspection must not leave a stale detached
    // observation authorizing writes after an editor has attached.
    let source_attached =
        agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(source);
    let target_attached = target.exists()
        && agent_doc_document_realtime_io::live_editor_endpoint_attached_for_file(target);
    if source_attached || target_attached {
        bail!(attached_editor_refusal_message(
            source,
            target,
            source_attached,
            target_attached,
            prepared.preview.target_would_be_created,
            true,
        ));
    }

    if let Some(parent) = target.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    agent_doc_document_realtime_io::atomic_write_through_authority(target, &prepared.target_after)
        .with_context(|| format!("failed to write target {}", target.display()))?;
    if let Err(error) = agent_doc_document_realtime_io::atomic_write_through_authority(
        source,
        &prepared.source_after,
    ) {
        rollback(&prepared, source, target);
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
        rollback(&prepared, source, target);
        if let Err(snapshot_error) = agent_doc_snapshot_io::checkpoint_document_baseline(
            source,
            &prepared.source_before,
            agent_doc_ops_log_io::log_op,
        ) {
            eprintln!("[extract] WARNING: failed to restore source snapshot: {snapshot_error:#}");
        }
        if let Some(target_before) = &prepared.target_before
            && let Err(snapshot_error) = agent_doc_snapshot_io::checkpoint_document_baseline(
                target,
                target_before,
                agent_doc_ops_log_io::log_op,
            )
        {
            eprintln!("[extract] WARNING: failed to restore target snapshot: {snapshot_error:#}");
        } else if prepared.target_before.is_none()
            && let Err(snapshot_error) = agent_doc_snapshot_io::clear_document_baseline(target)
        {
            eprintln!("[extract] WARNING: failed to clear new-target snapshot: {snapshot_error:#}");
        }
        return Err(error).context("snapshot checkpoint failed; document rollback attempted");
    }
    eprintln!(
        "[extract] applied {}: {} → {}",
        prepared.preview.operation_id,
        source.display(),
        target.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(error.contains("re-run the same command with --apply"));
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
  "components": ["notes"]
}"#,
        )
        .unwrap();

        let request = request_from_selection_file(&manifest).unwrap();
        assert_eq!(request.focus.as_deref(), Some("SDK legal and compliance"));
        assert!(request.rationale.unwrap().contains("counsel decisions"));
        assert_eq!(request.ids, ["legal1"]);
        assert_eq!(request.exchange_node_ids, ["r:abc123"]);
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
            "<!-- agent:queue priority subagents -->\n",
            "<!-- agent:queue priority subagents -->\n- [#legal1]: Draft request\n- do [#dev1]\n",
        )
        .replace(
            "<!-- agent:backlog -->\n",
            "<!-- agent:backlog -->\n- [/] [#legal1] Privacy decision\n- [ ] [#dev1] Build client\n",
        )
        .replace(
            "<!-- agent:review -->\n",
            "<!-- agent:review -->\n- [/] [#legal1] Await approval\n",
        )
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
                components: Vec::new(),
            },
        )
        .unwrap();
        assert!(prepared.target_after.contains("❯ Decide [#legal1]"));
        assert!(prepared.target_after.contains("### Re: Legal"));
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
            "<!-- agent:backlog -->\n",
            "<!-- agent:backlog -->\n- [ ] [#legal1] Existing target item\n",
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
}
