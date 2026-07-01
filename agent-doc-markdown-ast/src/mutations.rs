//! Node-keyed mutations for agent-component items.
//!
//! Phase 4 of `#md-ast-document-model`: mutation callers address component
//! items by durable node key rather than by matching raw text lines or logical
//! `[#id]` targets.

use crate::overlay::{self, Component, Item};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    ComponentNotFound(String),
    NodeNotFound { component: String, node_key: String },
    DuplicateNodeKey { component: String, node_key: String },
    StaleNode { component: String, node_key: String },
    InvalidNodeOrder(String),
    MalformedItem { component: String, node_key: String },
}

impl fmt::Display for MutationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MutationError::ComponentNotFound(component) => {
                write!(f, "component `{component}` was not found")
            }
            MutationError::NodeNotFound {
                component,
                node_key,
            } => write!(
                f,
                "node `{node_key}` was not found in component `{component}`"
            ),
            MutationError::DuplicateNodeKey {
                component,
                node_key,
            } => write!(
                f,
                "duplicate node key `{node_key}` in component `{component}`"
            ),
            MutationError::StaleNode {
                component,
                node_key,
            } => write!(
                f,
                "node `{node_key}` in component `{component}` no longer matches the patch baseline"
            ),
            MutationError::InvalidNodeOrder(message) => write!(f, "{message}"),
            MutationError::MalformedItem {
                component,
                node_key,
            } => write!(
                f,
                "node `{node_key}` in component `{component}` has malformed source"
            ),
        }
    }
}

impl std::error::Error for MutationError {}

pub type MutationResult<T> = Result<T, MutationError>;

/// A parsed item plus the durable node key mutation callers should retain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationItemNode {
    pub component: String,
    pub node_key: String,
    pub index: usize,
    pub item: Item,
}

/// Where to insert a new item relative to existing component nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationInsertPosition {
    Front,
    Back,
    Before(String),
    After(String),
}

/// Node-keyed IPC patch operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationNodePatchOp {
    Insert,
    Remove,
    Replace,
    Move,
    Strike,
    Unstrike,
}

/// A drift-resilient patch against one component item node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationNodePatch {
    pub component: String,
    pub node_key: String,
    pub op: MutationNodePatchOp,
    pub content: Option<String>,
    pub expected_content: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub order: Vec<String>,
}

/// Return component item nodes with deterministic node keys.
pub fn item_nodes(source: &str, component: &str) -> MutationResult<Vec<MutationItemNode>> {
    let (component_index, component_node) = find_component(source, component)?;
    Ok(item_nodes_for_component(
        component_index,
        component_node.name.clone(),
        component_node.items,
    ))
}

/// Return all component item nodes in document order with deterministic node keys.
pub fn all_item_nodes(source: &str) -> Vec<MutationItemNode> {
    overlay::components(source)
        .into_iter()
        .enumerate()
        .flat_map(|(component_index, component_node)| {
            item_nodes_for_component(
                component_index,
                component_node.name.clone(),
                component_node.items,
            )
        })
        .collect()
}

fn item_nodes_for_component(
    component_index: usize,
    component: String,
    items: Vec<Item>,
) -> Vec<MutationItemNode> {
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let identity = node_occurrence_identity(&item);
            let occurrence = occurrences.entry(identity).or_insert(0);
            let node_key = initial_node_key(&component, component_index, *occurrence, &item);
            *occurrence += 1;
            MutationItemNode {
                component: component.clone(),
                node_key,
                index,
                item,
            }
        })
        .collect()
}

/// Strike a component item by durable node key. Already-struck items are idempotent.
pub fn consume_node(source: &str, component: &str, node_key: &str) -> MutationResult<String> {
    consume_nodes(source, component, &[node_key])
}

/// Strike component items by durable node key. Already-struck items are idempotent.
pub fn consume_nodes(source: &str, component: &str, node_keys: &[&str]) -> MutationResult<String> {
    let nodes = item_nodes(source, component)?;
    let mut requested = HashSet::new();
    for node_key in node_keys {
        if !requested.insert(*node_key) {
            return Err(MutationError::DuplicateNodeKey {
                component: component.to_string(),
                node_key: (*node_key).to_string(),
            });
        }
    }
    let mut ranges = Vec::new();
    for node_key in node_keys {
        let node = find_node(&nodes, component, node_key)?;
        if !node.item.struck {
            ranges.push(raw_range(source, component, node)?);
        }
    }
    let mut out = source.to_string();
    ranges.sort_by_key(|(start, _)| *start);
    ranges.dedup();
    for (start, end) in ranges.into_iter().rev() {
        out.replace_range(start..end, &format!("~~{}~~", &source[start..end]));
    }
    Ok(out)
}

/// Remove duplicate nodes by node key, preserving intentional text/id duplicates.
pub fn dedup_node_keys(source: &str, component: &str) -> MutationResult<String> {
    let nodes = item_nodes(source, component)?;
    let mut seen = HashSet::new();
    let mut removals = Vec::new();
    for node in &nodes {
        if !seen.insert(node.node_key.clone()) {
            removals.push((node.item.start_byte, node.item.end_byte));
        }
    }
    Ok(remove_ranges(source, removals))
}

/// Reorder component items by an exact node-key permutation.
pub fn reorder_nodes(
    source: &str,
    component: &str,
    ordered_node_keys: &[&str],
) -> MutationResult<String> {
    let nodes = item_nodes(source, component)?;
    if nodes.is_empty() {
        return Ok(source.to_string());
    }
    if ordered_node_keys.len() != nodes.len() {
        return Err(MutationError::InvalidNodeOrder(format!(
            "node order for component `{component}` has {} keys for {} nodes",
            ordered_node_keys.len(),
            nodes.len()
        )));
    }

    let mut existing = HashSet::new();
    for node in &nodes {
        if !existing.insert(node.node_key.clone()) {
            return Err(MutationError::DuplicateNodeKey {
                component: component.to_string(),
                node_key: node.node_key.clone(),
            });
        }
    }

    let mut requested = HashSet::new();
    for node_key in ordered_node_keys {
        if !requested.insert((*node_key).to_string()) {
            return Err(MutationError::DuplicateNodeKey {
                component: component.to_string(),
                node_key: (*node_key).to_string(),
            });
        }
    }

    let by_key: HashMap<&str, &MutationItemNode> = nodes
        .iter()
        .map(|node| (node.node_key.as_str(), node))
        .collect();
    let mut ordered = Vec::new();
    for node_key in ordered_node_keys {
        let node = by_key
            .get(*node_key)
            .ok_or_else(|| MutationError::NodeNotFound {
                component: component.to_string(),
                node_key: (*node_key).to_string(),
            })?;
        ordered.push(*node);
    }

    let first = nodes.first().unwrap().item.start_byte;
    let last = nodes.last().unwrap().item.end_byte;
    let replacement = ordered
        .iter()
        .map(|node| &source[node.item.start_byte..node.item.end_byte])
        .collect::<String>();
    let mut out = source.to_string();
    out.replace_range(first..last, &replacement);
    Ok(out)
}

/// Insert a new bullet item into a component by node-key position.
pub fn enqueue_node(
    source: &str,
    component: &str,
    position: MutationInsertPosition,
    item_text: &str,
) -> MutationResult<String> {
    let (_, component_node) = find_component(source, component)?;
    let nodes = item_nodes(source, component)?;
    let insert_at = match position {
        MutationInsertPosition::Front => nodes
            .first()
            .map(|node| node.item.start_byte)
            .unwrap_or_else(|| open_marker_end(source, &component_node)),
        MutationInsertPosition::Back => nodes
            .last()
            .map(|node| node.item.end_byte)
            .unwrap_or_else(|| open_marker_end(source, &component_node)),
        MutationInsertPosition::Before(node_key) => {
            find_node(&nodes, component, &node_key)?.item.start_byte
        }
        MutationInsertPosition::After(node_key) => {
            find_node(&nodes, component, &node_key)?.item.end_byte
        }
    };
    let mut out = source.to_string();
    out.insert_str(insert_at, &format!("- {}\n", item_text.trim()));
    Ok(out)
}

/// Apply node-keyed IPC patches against the current document snapshot.
pub fn apply_node_patches(source: &str, patches: &[MutationNodePatch]) -> MutationResult<String> {
    let mut out = source.to_string();
    for patch in patches {
        out = apply_node_patch(&out, patch)?;
    }
    Ok(out)
}

/// Parse a convergence IPC payload's `node_patches` field into mutation patches.
///
/// Missing `node_patches` is a valid no-op payload. Malformed entries or unknown
/// operations return `None` so callers can fail closed.
pub fn parse_node_patches_payload(payload: &serde_json::Value) -> Option<Vec<MutationNodePatch>> {
    let Some(node_patches_value) = payload.get("node_patches") else {
        return Some(Vec::new());
    };
    let node_patches = node_patches_value.as_array()?;
    let mut parsed = Vec::with_capacity(node_patches.len());
    for patch in node_patches {
        let op = parse_node_patch_op(patch.get("op").and_then(|value| value.as_str())?)?;
        let order = match patch.get("order") {
            Some(value) => value
                .as_array()?
                .iter()
                .map(|item| item.as_str().map(str::to_string))
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        parsed.push(MutationNodePatch {
            component: patch
                .get("component")
                .and_then(|value| value.as_str())?
                .to_string(),
            node_key: patch
                .get("node_key")
                .and_then(|value| value.as_str())?
                .to_string(),
            op,
            content: optional_payload_string(patch, "content")?,
            expected_content: None,
            before: optional_payload_string(patch, "before")?,
            after: optional_payload_string(patch, "after")?,
            order,
        });
    }
    Some(parsed)
}

/// Return true when applying node patches would leave `source` unchanged.
pub fn node_patches_already_landed(source: &str, patches: &[MutationNodePatch]) -> bool {
    apply_node_patches(source, patches)
        .map(|after| after == source)
        .unwrap_or(false)
}

fn parse_node_patch_op(value: &str) -> Option<MutationNodePatchOp> {
    match value {
        "insert" => Some(MutationNodePatchOp::Insert),
        "remove" => Some(MutationNodePatchOp::Remove),
        "replace" => Some(MutationNodePatchOp::Replace),
        "move" => Some(MutationNodePatchOp::Move),
        "strike" => Some(MutationNodePatchOp::Strike),
        "unstrike" => Some(MutationNodePatchOp::Unstrike),
        _ => None,
    }
}

fn optional_payload_string(value: &serde_json::Value, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        None | Some(serde_json::Value::Null) => Some(None),
        Some(value) => value.as_str().map(|s| Some(s.to_string())),
    }
}

fn apply_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    match patch.op {
        MutationNodePatchOp::Insert => insert_node_patch(source, patch),
        MutationNodePatchOp::Remove => remove_node_patch(source, patch),
        MutationNodePatchOp::Replace => replace_node_patch(source, patch),
        MutationNodePatchOp::Move => move_node_patch(source, patch),
        MutationNodePatchOp::Strike => strike_node_patch(source, patch),
        MutationNodePatchOp::Unstrike => unstrike_node_patch(source, patch),
    }
}

fn insert_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    let content = patch_node_source(patch)?;
    let (_, component_node) = find_component(source, &patch.component)?;
    let nodes = item_nodes(source, &patch.component)?;
    if nodes.iter().any(|node| node.node_key == patch.node_key) {
        return Ok(source.to_string());
    }

    let insert_at = anchored_insert_offset(source, &component_node, &nodes, patch);
    let mut out = source.to_string();
    out.insert_str(insert_at, &content);
    Ok(out)
}

fn remove_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    let nodes = item_nodes(source, &patch.component)?;
    let Some(node) = nodes.iter().find(|node| node.node_key == patch.node_key) else {
        return Ok(source.to_string());
    };
    ensure_expected_node_content(source, node, patch)?;
    let mut out = source.to_string();
    out.replace_range(node.item.start_byte..node.item.end_byte, "");
    Ok(out)
}

fn replace_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    let content = patch_node_source(patch)?;
    let nodes = item_nodes(source, &patch.component)?;
    let node = find_node(&nodes, &patch.component, &patch.node_key)?;
    ensure_expected_node_content(source, node, patch)?;
    let mut out = source.to_string();
    out.replace_range(node.item.start_byte..node.item.end_byte, &content);
    Ok(out)
}

fn move_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    if !patch.order.is_empty() {
        let order = patch.order.iter().map(String::as_str).collect::<Vec<_>>();
        return reorder_nodes(source, &patch.component, &order);
    }

    find_component(source, &patch.component)?;
    let nodes = item_nodes(source, &patch.component)?;
    let node = find_node(&nodes, &patch.component, &patch.node_key)?;
    ensure_expected_node_content(source, node, patch)?;
    let moved = source[node.item.start_byte..node.item.end_byte].to_string();

    let mut without_node = source.to_string();
    without_node.replace_range(node.item.start_byte..node.item.end_byte, "");

    let (_, component_after_remove) = find_component(&without_node, &patch.component)?;
    let nodes_after_remove = item_nodes(&without_node, &patch.component)?;
    let insert_at = anchored_insert_offset(
        &without_node,
        &component_after_remove,
        &nodes_after_remove,
        patch,
    );
    without_node.insert_str(insert_at, &moved);
    Ok(without_node)
}

fn strike_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    let nodes = item_nodes(source, &patch.component)?;
    let node = find_node(&nodes, &patch.component, &patch.node_key)?;
    ensure_expected_node_content(source, node, patch)?;
    consume_node(source, &patch.component, &patch.node_key)
}

fn unstrike_node_patch(source: &str, patch: &MutationNodePatch) -> MutationResult<String> {
    let nodes = item_nodes(source, &patch.component)?;
    let node = find_node(&nodes, &patch.component, &patch.node_key)?;
    ensure_expected_node_content(source, node, patch)?;
    if !node.item.struck {
        return Ok(source.to_string());
    }
    let (start, end) = raw_range(source, &patch.component, node)?;
    let raw = &source[start..end];
    // Strip both the `~~…~~` wrapper and any `#qstrikenote` annotation appended
    // outside it (`~~text~~ — auto-struck: …`). The annotated shape closes the
    // wrapper before the deterministic separator, so split there first.
    let body_after_open = raw
        .strip_prefix("~~")
        .ok_or_else(|| MutationError::MalformedItem {
            component: patch.component.clone(),
            node_key: patch.node_key.clone(),
        })?;
    let annotated_needle = format!("~~{}", crate::overlay::STRUCK_ANNOTATION_SEPARATOR);
    let unstruck = if let Some(close) = body_after_open.find(&annotated_needle) {
        &body_after_open[..close]
    } else if let Some(inner) = body_after_open.strip_suffix("~~") {
        inner
    } else {
        return Err(MutationError::MalformedItem {
            component: patch.component.clone(),
            node_key: patch.node_key.clone(),
        });
    };
    let unstruck = unstruck.to_string();
    let mut out = source.to_string();
    out.replace_range(start..end, &unstruck);
    Ok(out)
}

fn ensure_expected_node_content(
    source: &str,
    node: &MutationItemNode,
    patch: &MutationNodePatch,
) -> MutationResult<()> {
    let Some(expected) = patch.expected_content.as_deref() else {
        return Ok(());
    };
    let actual = &source[node.item.start_byte..node.item.end_byte];
    if actual == expected {
        return Ok(());
    }
    Err(MutationError::StaleNode {
        component: patch.component.clone(),
        node_key: patch.node_key.clone(),
    })
}

fn anchored_insert_offset(
    source: &str,
    component: &Component,
    nodes: &[MutationItemNode],
    patch: &MutationNodePatch,
) -> usize {
    if let Some(after) = patch.after.as_deref()
        && let Some(node) = nodes.iter().find(|node| node.node_key == after)
    {
        return node.item.end_byte;
    }
    if let Some(before) = patch.before.as_deref()
        && let Some(node) = nodes.iter().find(|node| node.node_key == before)
    {
        return node.item.start_byte;
    }
    nodes
        .last()
        .map(|node| node.item.end_byte)
        .unwrap_or_else(|| open_marker_end(source, component))
}

fn patch_node_source(patch: &MutationNodePatch) -> MutationResult<String> {
    let content = patch
        .content
        .as_deref()
        .ok_or_else(|| MutationError::InvalidNodeOrder("node patch requires content".into()))?;
    let mut content = content.trim_end_matches('\n').to_string();
    content.push('\n');
    Ok(content)
}

fn find_component(source: &str, component: &str) -> MutationResult<(usize, Component)> {
    overlay::components(source)
        .into_iter()
        .enumerate()
        .find(|(_, candidate)| candidate.name == component)
        .ok_or_else(|| MutationError::ComponentNotFound(component.to_string()))
}

fn find_node<'a>(
    nodes: &'a [MutationItemNode],
    component: &str,
    node_key: &str,
) -> MutationResult<&'a MutationItemNode> {
    nodes
        .iter()
        .find(|node| node.node_key == node_key)
        .ok_or_else(|| MutationError::NodeNotFound {
            component: component.to_string(),
            node_key: node_key.to_string(),
        })
}

fn raw_range(
    source: &str,
    component: &str,
    node: &MutationItemNode,
) -> MutationResult<(usize, usize)> {
    let line = &source[node.item.start_byte..node.item.end_byte];
    let relative_start = line
        .find(&node.item.raw)
        .ok_or_else(|| MutationError::MalformedItem {
            component: component.to_string(),
            node_key: node.node_key.clone(),
        })?;
    let start = node.item.start_byte + relative_start;
    Ok((start, start + node.item.raw.len()))
}

fn remove_ranges(source: &str, mut ranges: Vec<(usize, usize)>) -> String {
    ranges.sort_by_key(|(start, _)| *start);
    let mut out = source.to_string();
    for (start, end) in ranges.into_iter().rev() {
        out.replace_range(start..end, "");
    }
    out
}

fn open_marker_end(source: &str, component: &Component) -> usize {
    source[component.start_byte..component.end_byte]
        .find('\n')
        .map(|offset| component.start_byte + offset + 1)
        .unwrap_or(component.end_byte)
}

fn node_occurrence_identity(item: &Item) -> String {
    item.id.clone()
}

fn initial_node_key(
    component_name: &str,
    component_index: usize,
    occurrence: usize,
    item: &Item,
) -> String {
    format!(
        "{component_name}:{component_index}:{}:{occurrence}",
        item.id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "\
<!-- agent:queue preset=\"#spec-test\" priority go -->
- :pushpin: do [#alpha]
- do [#beta]
- do [#beta]
- duplicate prose
- duplicate prose
<!-- /agent:queue -->
";

    fn node_patch(
        node_key: &str,
        op: MutationNodePatchOp,
        content: Option<&str>,
        before: Option<&str>,
        after: Option<&str>,
    ) -> MutationNodePatch {
        MutationNodePatch {
            component: "queue".to_string(),
            node_key: node_key.to_string(),
            op,
            content: content.map(str::to_string),
            expected_content: None,
            before: before.map(str::to_string),
            after: after.map(str::to_string),
            order: Vec::new(),
        }
    }

    #[test]
    fn consume_strikes_exact_node_key_without_matching_text() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let beta_nodes = nodes
            .iter()
            .filter(|node| node.item.id == "beta")
            .collect::<Vec<_>>();

        let updated = consume_node(DOC, "queue", &beta_nodes[1].node_key).unwrap();

        assert!(updated.contains("- do [#beta]\n- ~~do [#beta]~~\n"));
    }

    #[test]
    fn consume_is_idempotent_for_struck_nodes() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let alpha = nodes
            .iter()
            .find(|node| node.item.id == "alpha")
            .unwrap()
            .node_key
            .clone();

        let once = consume_node(DOC, "queue", &alpha).unwrap();
        let twice = consume_node(&once, "queue", &alpha).unwrap();

        assert_eq!(once, twice);
    }

    #[test]
    fn node_keys_survive_unrelated_prefix_drift_and_strike() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let beta = nodes
            .iter()
            .filter(|node| node.item.id == "beta")
            .nth(1)
            .unwrap()
            .node_key
            .clone();
        let drifted = format!("operator note\n{DOC}");
        let drifted_nodes = item_nodes(&drifted, "queue").unwrap();
        assert!(
            drifted_nodes.iter().any(|node| node.node_key == beta),
            "node key should not depend on absolute byte offsets"
        );

        let struck = consume_node(DOC, "queue", &beta).unwrap();
        let struck_nodes = item_nodes(&struck, "queue").unwrap();
        assert!(
            struck_nodes.iter().any(|node| node.node_key == beta),
            "node key should not change after consume wraps the item in strikethrough"
        );
    }

    #[test]
    fn consume_nodes_strikes_multiple_keys_from_initial_snapshot() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let alpha = nodes[0].node_key.as_str();
        let second_beta = nodes[2].node_key.as_str();

        let updated = consume_nodes(DOC, "queue", &[alpha, second_beta]).unwrap();

        assert!(updated.contains("- ~~:pushpin: do [#alpha]~~\n"));
        assert!(updated.contains("- do [#beta]\n- ~~do [#beta]~~\n"));
    }

    #[test]
    fn dedup_node_keys_preserves_text_and_id_duplicates() {
        let updated = dedup_node_keys(DOC, "queue").unwrap();

        assert_eq!(updated.matches("- do [#beta]\n").count(), 2);
        assert_eq!(updated.matches("- duplicate prose\n").count(), 2);
    }

    #[test]
    fn same_id_different_text_gets_distinct_node_keys() {
        let doc = "\
<!-- agent:queue -->
- do [#same] first
- do [#same] second
<!-- /agent:queue -->
";
        let nodes = item_nodes(doc, "queue").unwrap();

        assert_eq!(nodes[0].node_key, "queue:0:same:0");
        assert_eq!(nodes[1].node_key, "queue:0:same:1");
    }

    #[test]
    fn all_item_nodes_preserves_component_indexes() {
        let doc = "\
<!-- agent:queue -->
- do [#alpha]
<!-- /agent:queue -->

<!-- agent:queue -->
- do [#beta]
<!-- /agent:queue -->
";
        let nodes = all_item_nodes(doc);

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].node_key, "queue:0:alpha:0");
        assert_eq!(nodes[1].node_key, "queue:1:beta:0");
    }

    #[test]
    fn reorder_uses_exact_node_key_permutation() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let order = [
            nodes[2].node_key.as_str(),
            nodes[0].node_key.as_str(),
            nodes[1].node_key.as_str(),
            nodes[3].node_key.as_str(),
            nodes[4].node_key.as_str(),
        ];

        let updated = reorder_nodes(DOC, "queue", &order).unwrap();
        let lines = updated
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect::<Vec<_>>();

        assert_eq!(lines[0], "- do [#beta]");
        assert_eq!(lines[1], "- :pushpin: do [#alpha]");
        assert_eq!(lines[2], "- do [#beta]");
    }

    #[test]
    fn enqueue_inserts_after_target_node_key() {
        let nodes = item_nodes(DOC, "queue").unwrap();
        let updated = enqueue_node(
            DOC,
            "queue",
            MutationInsertPosition::After(nodes[0].node_key.clone()),
            "do [#inserted]",
        )
        .unwrap();
        let alpha = updated.find("- :pushpin: do [#alpha]\n").unwrap();
        let inserted = updated.find("- do [#inserted]\n").unwrap();
        let beta = updated.find("- do [#beta]\n").unwrap();

        assert!(alpha < inserted);
        assert!(inserted < beta);
    }

    #[test]
    fn apply_node_patches_preserves_unrelated_live_buffer_items() {
        let live = "\
operator note
<!-- agent:queue preset=\"#spec-test\" priority go -->
- :pushpin: do [#alpha]
- do [#beta]
- live buffer addition
<!-- /agent:queue -->
";
        let patches = [
            node_patch(
                "queue:0:beta:0",
                MutationNodePatchOp::Strike,
                None,
                None,
                None,
            ),
            node_patch(
                "queue:0:gamma:0",
                MutationNodePatchOp::Insert,
                Some("- do [#gamma]\n"),
                None,
                Some("queue:0:beta:0"),
            ),
        ];

        let updated = apply_node_patches(live, &patches).unwrap();

        assert!(updated.contains("operator note\n"));
        assert!(updated.contains("- live buffer addition\n"));
        assert!(updated.contains("- ~~do [#beta]~~\n- do [#gamma]\n"));
    }

    #[test]
    fn apply_node_patches_replace_unstrike_and_move_by_node_key() {
        let doc = "\
<!-- agent:queue -->
- do [#alpha]
- ~~do [#beta]~~
- do [#gamma]
<!-- /agent:queue -->
";
        let patches = [
            node_patch(
                "queue:0:beta:0",
                MutationNodePatchOp::Unstrike,
                None,
                None,
                None,
            ),
            node_patch(
                "queue:0:gamma:0",
                MutationNodePatchOp::Replace,
                Some("- :round_pushpin: do [#gamma]\n"),
                None,
                None,
            ),
            node_patch(
                "queue:0:gamma:0",
                MutationNodePatchOp::Move,
                None,
                Some("queue:0:alpha:0"),
                None,
            ),
        ];

        let updated = apply_node_patches(doc, &patches).unwrap();
        let lines = updated
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect::<Vec<_>>();

        assert_eq!(lines[0], "- :round_pushpin: do [#gamma]");
        assert_eq!(lines[1], "- do [#alpha]");
        assert_eq!(lines[2], "- do [#beta]");
    }

    #[test]
    fn unstrike_strips_qstrikenote_annotation_and_wrapper() {
        // An annotated struck free-text head (#qstrikenote) must unstrike back to
        // its bare text, dropping both the `~~…~~` wrapper and the note.
        let doc = "\
<!-- agent:queue -->
- ~~answered free-text head~~ — auto-struck: answered this cycle (#ftstrike)
<!-- /agent:queue -->
";
        let nodes = item_nodes(doc, "queue").unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].item.struck);
        let key = nodes[0].node_key.clone();
        let patches = [node_patch(
            &key,
            MutationNodePatchOp::Unstrike,
            None,
            None,
            None,
        )];
        let updated = apply_node_patches(doc, &patches).unwrap();
        assert!(
            updated.contains("- answered free-text head\n"),
            "unstrike must restore bare text:\n{updated}"
        );
        assert!(
            !updated.contains("~~"),
            "no strike wrapper remains:\n{updated}"
        );
        assert!(
            !updated.contains("auto-struck"),
            "the note must be removed:\n{updated}"
        );
    }

    #[test]
    fn apply_node_patches_is_idempotent_for_insert_remove_and_move() {
        let doc = "\
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let patches = [
            node_patch(
                "queue:0:beta:0",
                MutationNodePatchOp::Move,
                None,
                Some("queue:0:alpha:0"),
                None,
            ),
            node_patch(
                "queue:0:gamma:0",
                MutationNodePatchOp::Insert,
                Some("- do [#gamma]\n"),
                None,
                Some("queue:0:alpha:0"),
            ),
            node_patch(
                "queue:0:missing:0",
                MutationNodePatchOp::Remove,
                None,
                None,
                None,
            ),
        ];

        let once = apply_node_patches(doc, &patches).unwrap();
        let twice = apply_node_patches(&once, &patches).unwrap();

        assert_eq!(once, twice);
        assert_eq!(once.matches("- do [#gamma]\n").count(), 1);
    }

    #[test]
    fn parse_node_patches_payload_accepts_valid_node_patches_json() {
        let payload = serde_json::json!({
            "node_patches": [
                {
                    "component": "queue",
                    "node_key": "queue:0:alpha:0",
                    "op": "insert",
                    "content": "- do [#alpha]\n",
                    "before": null,
                    "after": "queue:0:beta:0",
                    "order": ["queue:0:alpha:0", "queue:0:beta:0"]
                }
            ]
        });

        let patches = parse_node_patches_payload(&payload).unwrap();

        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].component, "queue");
        assert_eq!(patches[0].node_key, "queue:0:alpha:0");
        assert_eq!(patches[0].op, MutationNodePatchOp::Insert);
        assert_eq!(patches[0].content.as_deref(), Some("- do [#alpha]\n"));
        assert_eq!(patches[0].before, None);
        assert_eq!(patches[0].after.as_deref(), Some("queue:0:beta:0"));
        assert_eq!(patches[0].order, ["queue:0:alpha:0", "queue:0:beta:0"]);
    }

    #[test]
    fn parse_node_patches_payload_missing_node_patches_is_empty_vec() {
        let payload = serde_json::json!({ "patches": [] });

        let patches = parse_node_patches_payload(&payload).unwrap();

        assert!(patches.is_empty());
    }

    #[test]
    fn parse_node_patches_payload_invalid_op_returns_none() {
        let payload = serde_json::json!({
            "node_patches": [
                {
                    "component": "queue",
                    "node_key": "queue:0:alpha:0",
                    "op": "rewrite"
                }
            ]
        });

        assert!(parse_node_patches_payload(&payload).is_none());
    }

    #[test]
    fn node_patches_already_landed_reports_idempotent_patch_payload() {
        let doc = "\
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
<!-- /agent:queue -->
";
        let payload = serde_json::json!({
            "node_patches": [
                {
                    "component": "queue",
                    "node_key": "queue:0:beta:0",
                    "op": "strike"
                }
            ]
        });
        let patches = parse_node_patches_payload(&payload).unwrap();
        let landed = apply_node_patches(doc, &patches).unwrap();

        assert!(!node_patches_already_landed(doc, &patches));
        assert!(node_patches_already_landed(&landed, &patches));
    }

    #[test]
    fn apply_node_patches_rejects_stale_target_node_but_allows_unrelated_drift() {
        let doc = "\
operator note
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
- live buffer addition
<!-- /agent:queue -->
";
        let stale = node_patch(
            "queue:0:beta:0",
            MutationNodePatchOp::Strike,
            None,
            None,
            None,
        );
        let mut stale = stale;
        stale.expected_content = Some("- do [#beta] changed elsewhere\n".to_string());
        let err = apply_node_patches(doc, &[stale]).unwrap_err();
        assert!(matches!(err, MutationError::StaleNode { .. }));

        let accepted = node_patch(
            "queue:0:beta:0",
            MutationNodePatchOp::Strike,
            None,
            None,
            None,
        );
        let mut accepted = accepted;
        accepted.expected_content = Some("- do [#beta]\n".to_string());
        let updated = apply_node_patches(doc, &[accepted]).unwrap();
        assert!(
            updated.contains("- live buffer addition\n"),
            "unrelated document drift must be preserved:\n{updated}"
        );
        assert!(updated.contains("- ~~do [#beta]~~\n"));
    }
}
