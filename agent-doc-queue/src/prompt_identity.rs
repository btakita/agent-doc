//! Stable executable prompt ids and non-ordering document-local relations.
//!
//! This module is pure. It projects an editor-authoritative document cut into
//! a candidate with missing prompt ids admitted and a relation graph. The IO
//! adapter is responsible for persisting that candidate through the existing
//! queue-maintenance compare-and-swap boundary.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use agent_doc_markdown_ast::{
    mutations::{MutationItemNode, all_item_nodes},
    overlay::ItemKind,
};
use serde::{Deserialize, Serialize};

const GENERATED_ID_PREFIX: &str = "q-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuePromptIdAssignment {
    pub id: String,
    pub source_text: String,
    pub target_text: String,
    pub node_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRelationEdge {
    pub source_id: String,
    pub target_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRelationBacklinks {
    pub target_id: String,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRelationDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueRelationGraph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<QueueRelationEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backlinks: Vec<QueueRelationBacklinks>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<QueueRelationDiagnostic>,
}

impl QueueRelationGraph {
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty() && self.backlinks.is_empty() && self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePromptIdentityProjection {
    pub content: String,
    pub assignments: Vec<QueuePromptIdAssignment>,
    pub relations: QueueRelationGraph,
}

/// Admit stable ids for every executable queue node that lacks an authored id,
/// then derive document-local `Related` navigation from the resulting cut.
pub fn project(content: &str, document_id: &str) -> QueuePromptIdentityProjection {
    let nodes = executable_queue_nodes(content);
    let ancestor_paths = queue_ancestor_paths(content);
    let mut owned_ids: HashMap<String, String> = identified_queue_nodes(content)
        .into_iter()
        .filter_map(|node| node_id(&node).map(|id| (id, node.node_key)))
        .collect();
    let mut insertions = Vec::new();
    let mut assignments = Vec::new();
    let mut identity_diagnostics = Vec::new();
    for node in nodes {
        if node_id(&node).is_some() {
            continue;
        }
        let work = node.item.work_text().trim();
        let ancestors = ancestor_paths
            .get(&node.node_key)
            .map(String::as_str)
            .unwrap_or("");
        // The ancestor path is part of the mint. A removed row must not hand
        // its q-id to the same leaf under a different parent. An explicit
        // `[#q-…]` still follows the row and is not reminted.
        let seed = format!(
            "{document_id}\n{}\n{}\n{ancestors}\n{}",
            node.component,
            node.node_key,
            agent_doc_element_queue::normalize_multiline_dedup_text(work)
        );
        let id = available_generated_id(&seed, &node.node_key, &owned_ids);
        let Some(offset) = prompt_id_insertion_offset(content, &node) else {
            identity_diagnostics.push(diag(
                "queue_prompt_identity_unresolved",
                format!(
                    "queue prompt at {} could not be annotated safely and was left unchanged",
                    node.node_key
                ),
            ));
            continue;
        };
        owned_ids.insert(id.clone(), node.node_key.clone());
        insertions.push((offset, format!(" [#{id}]")));
        let target_text = prompt_text_with_id(work, &id);
        assignments.push(QueuePromptIdAssignment {
            id,
            source_text: work.to_string(),
            target_text,
            node_key: node.node_key,
        });
    }
    insertions.sort_by_key(|(offset, _)| *offset);
    let mut projected = content.to_string();
    for (offset, annotation) in insertions.into_iter().rev() {
        projected.insert_str(offset, &annotation);
    }
    let mut relations = relation_graph(&projected);
    relations.diagnostics.extend(identity_diagnostics);
    QueuePromptIdentityProjection {
        content: projected,
        assignments,
        relations,
    }
}

fn available_generated_id(
    seed: &str,
    node_key: &str,
    owned_ids: &HashMap<String, String>,
) -> String {
    // Once an earlier occurrence is admitted, its mutation node key changes
    // from the legacy text-derived key to the q-id-derived key. A later
    // identical occurrence can therefore reproduce the earlier seed even
    // though it is distinct work. Probe deterministically against the ids in
    // this immutable document cut: persisted owners never move, and every
    // authority replay of the same cut chooses the same first free candidate.
    let mut probe = 0usize;
    loop {
        let candidate_seed = if probe == 0 {
            seed.to_string()
        } else {
            format!("{seed}\nidentity-probe:{probe}")
        };
        let id = format!(
            "{GENERATED_ID_PREFIX}{}",
            agent_doc_hash::short_content_hash(&candidate_seed)
        );
        match owned_ids.get(&id) {
            None => return id,
            Some(owner) if owner == node_key => return id,
            Some(_) => probe += 1,
        }
    }
}

fn prompt_text_with_id(work: &str, id: &str) -> String {
    let first = work.lines().next().unwrap_or(work).trim_end();
    let offset = agent_doc_element_queue::queue_report_state_annotation(first)
        .map(|(annotation, _)| annotation.start)
        .unwrap_or(first.len());
    let mut target = work.to_string();
    target.insert_str(offset, &format!(" [#{id}]"));
    target
}

/// Read-only relation projection for probes and preflight output.
pub fn relation_graph(content: &str) -> QueueRelationGraph {
    let nodes = identified_queue_nodes(content);
    let mut by_id: BTreeMap<String, Vec<&MutationItemNode>> = BTreeMap::new();
    for node in &nodes {
        if let Some(id) = node_id(node) {
            by_id.entry(id).or_default().push(node);
        }
    }

    let mut diagnostics = Vec::new();
    for (id, owners) in &by_id {
        if owners.len() > 1 {
            diagnostics.push(diag(
                "queue_prompt_identity_duplicate",
                format!(
                    "queue prompt id [#{id}] names {} executable rows; identity was preserved and no row was silently reassigned",
                    owners.len()
                ),
            ));
        }
    }

    let mut declarations = related_declarations(content, &nodes, &mut diagnostics);
    let mut seen = BTreeSet::new();
    let mut edges = Vec::new();
    for (source_id, target_id) in declarations.drain(..) {
        if !seen.insert((source_id.clone(), target_id.clone())) {
            diagnostics.push(diag(
                "queue_related_duplicate",
                format!("duplicate Related edge [#{source_id}] -> [#{target_id}]"),
            ));
            continue;
        }
        if source_id == target_id {
            diagnostics.push(diag(
                "queue_related_self_cycle",
                format!("Related edge [#{source_id}] points to itself"),
            ));
        }
        if !by_id.contains_key(&target_id) {
            diagnostics.push(diag(
                "queue_related_dangling",
                format!(
                    "Related edge from [#{source_id}] references missing document-local queue prompt [#{target_id}]"
                ),
            ));
        }
        edges.push(QueueRelationEdge {
            source_id,
            target_id,
        });
    }

    diagnostics.extend(cycle_diagnostics(&edges));
    let mut backlinks: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &edges {
        if by_id.contains_key(&edge.target_id) {
            backlinks
                .entry(edge.target_id.clone())
                .or_default()
                .insert(edge.source_id.clone());
        }
    }
    QueueRelationGraph {
        edges,
        backlinks: backlinks
            .into_iter()
            .map(|(target_id, source_ids)| QueueRelationBacklinks {
                target_id,
                source_ids: source_ids.into_iter().collect(),
            })
            .collect(),
        diagnostics,
    }
}

fn executable_queue_nodes(content: &str) -> Vec<MutationItemNode> {
    let nodes = all_item_nodes(content)
        .into_iter()
        .filter(|node| node.component == "queue")
        .collect::<Vec<_>>();
    // `#waitingpacketowner`: a waiting item is identified by its Todo id, and
    // every row in its packet is decision evidence for it. Neither is an
    // executable queue node, so neither is ever admitted a `q-` id.
    let waiting_ranges = crate::waiting_packet::waiting_tree_byte_ranges(content);
    nodes
        .iter()
        .enumerate()
        .filter(|(index, node)| {
            if node.item.struck || !matches!(node.item.kind, ItemKind::Do | ItemKind::FreeText) {
                return false;
            }
            if waiting_ranges
                .iter()
                .any(|range| range.contains(&node.item.start_byte))
            {
                return false;
            }
            // Harness/supervisor slash controls are queue handoffs, not
            // executable agent prompts. Their exact bytes are the command
            // protocol, so correlation metadata must not be appended.
            if agent_doc_element_queue::strip_priority_markers(node.item.work_text())
                .trim_start()
                .starts_with('/')
            {
                return false;
            }
            // List comments are not item nodes, so a pull-request URL annotated
            // only with comments looks like a leaf. It is a catalog heading,
            // not work (`#prurlcommentnote`).
            if crate::document_queue::comment_only_pull_request_at(
                content,
                node.item.start_byte,
                node.item.text.trim(),
            ) {
                return false;
            }
            let indent = source_indent(content, node.item.start_byte);
            !nodes.get(index + 1).is_some_and(|next| {
                same_component_occurrence(node, next)
                    && source_indent(content, next.item.start_byte) > indent
            })
        })
        .map(|(_, node)| node.clone())
        .collect()
}

fn identified_queue_nodes(content: &str) -> Vec<MutationItemNode> {
    all_item_nodes(content)
        .into_iter()
        .filter(|node| {
            node.component == "queue" && matches!(node.item.kind, ItemKind::Do | ItemKind::FreeText)
        })
        .collect()
}

fn same_component_occurrence(left: &MutationItemNode, right: &MutationItemNode) -> bool {
    left.node_key.split(':').nth(1) == right.node_key.split(':').nth(1)
}

fn node_id(node: &MutationItemNode) -> Option<String> {
    node.item.explicit_id.then(|| node.item.id.clone())
}

fn prompt_id_insertion_offset(content: &str, node: &MutationItemNode) -> Option<usize> {
    let range = node.item.start_byte..node.item.end_byte;
    let raw = content.get(range.clone())?;
    let work = node.item.work_text().trim();
    let work_start = raw.find(work)?;
    let first = work.lines().next()?.trim_end();
    let relative = if let Some((annotation, _)) =
        agent_doc_element_queue::queue_report_state_annotation(first)
    {
        work_start + annotation.start
    } else {
        work_start + first.len()
    };
    Some(range.start + relative)
}

fn source_indent(content: &str, byte: usize) -> usize {
    let line_end = content[byte..]
        .find('\n')
        .map(|relative| byte + relative)
        .unwrap_or(content.len());
    content[byte..line_end]
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

/// Authored ancestor path for every live queue item, keyed by node key.
///
/// The path is part of the mint seed. A removed row must not hand its q-id to
/// the same leaf under a different parent. Labels are authored work, so
/// admitting a parent's own `[#q-…]` does not change a child's seed.
fn queue_ancestor_paths(content: &str) -> HashMap<String, String> {
    let nodes = all_item_nodes(content)
        .into_iter()
        .filter(|node| node.component == "queue" && !node.item.struck)
        .collect::<Vec<_>>();
    let mut paths = HashMap::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut component_index = String::new();
    for node in &nodes {
        let index = node.node_key.split(':').nth(1).unwrap_or("");
        if component_index != index {
            stack.clear();
            component_index = index.to_string();
        }
        let indent = source_indent(content, node.item.start_byte);
        while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
            stack.pop();
        }
        let path = stack
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        paths.insert(node.node_key.clone(), path);
        stack.push((indent, ancestor_seed_label(&node.item)));
    }
    paths
}

fn ancestor_seed_label(item: &agent_doc_markdown_ast::overlay::Item) -> String {
    let authored =
        agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item.work_text());
    agent_doc_element_queue::normalize_multiline_dedup_text(
        agent_doc_element_queue::strip_priority_markers(authored.trim()).trim(),
    )
}

fn related_declarations(
    content: &str,
    nodes: &[MutationItemNode],
    diagnostics: &mut Vec<QueueRelationDiagnostic>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for raw in content.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let Some((indent, body)) = list_item(line) else {
            offset += raw.len();
            continue;
        };
        let Some(refs) = body.trim().strip_prefix("**Related:**") else {
            offset += raw.len();
            continue;
        };
        let Some(source) = nodes
            .iter()
            .filter(|node| {
                node.item.start_byte < offset
                    && source_indent(content, node.item.start_byte) < indent
            })
            .max_by_key(|node| node.item.start_byte)
            .and_then(node_id)
        else {
            diagnostics.push(diag(
                "queue_related_source_missing",
                "Related metadata has no identified executable parent".to_string(),
            ));
            offset += raw.len();
            continue;
        };
        let mut any = false;
        for token in refs.split_whitespace() {
            if let Some(id) = token
                .strip_prefix("[#")
                .and_then(|rest| rest.strip_suffix(']'))
                && valid_id(id)
            {
                any = true;
                out.push((source.clone(), id.to_ascii_lowercase()));
            } else {
                diagnostics.push(diag(
                    "queue_related_invalid",
                    format!("invalid Related reference {token:?} under [#{source}]"),
                ));
            }
        }
        if !any {
            diagnostics.push(diag(
                "queue_related_empty",
                format!("Related metadata under [#{source}] has no valid references"),
            ));
        }
        offset += raw.len();
    }
    out
}

fn list_item(line: &str) -> Option<(usize, &str)> {
    let indent = line
        .chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .count();
    let rest = &line[indent..];
    if let Some(body) = rest.strip_prefix("- ") {
        return Some((indent, body));
    }
    let marker = rest.find(". ")?;
    (!rest[..marker].is_empty() && rest[..marker].bytes().all(|byte| byte.is_ascii_digit()))
        .then_some((indent, &rest[marker + 2..]))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn cycle_diagnostics(edges: &[QueueRelationEdge]) -> Vec<QueueRelationDiagnostic> {
    let graph = edges
        .iter()
        .fold(BTreeMap::<String, Vec<String>>::new(), |mut graph, edge| {
            graph
                .entry(edge.source_id.clone())
                .or_default()
                .push(edge.target_id.clone());
            graph
        });
    let mut diagnostics = Vec::new();
    let mut reported = BTreeSet::new();
    for start in graph.keys() {
        let mut stack = vec![(start.clone(), vec![start.clone()])];
        let mut visited = HashSet::new();
        while let Some((node, path)) = stack.pop() {
            if !visited.insert(node.clone()) {
                continue;
            }
            for next in graph.get(&node).into_iter().flatten() {
                if next == start && path.len() > 1 {
                    let mut cycle = path.clone();
                    cycle.push(start.clone());
                    let key = cycle.iter().cloned().collect::<BTreeSet<_>>();
                    let key = key.into_iter().collect::<Vec<_>>().join("|");
                    if reported.insert(key) {
                        diagnostics.push(diag(
                            "queue_related_cycle",
                            format!(
                                "Related cycle: {}",
                                cycle
                                    .iter()
                                    .map(|id| format!("[#{id}]"))
                                    .collect::<Vec<_>>()
                                    .join(" -> ")
                            ),
                        ));
                    }
                    continue;
                }
                if !path.contains(next) {
                    let mut next_path = path.clone();
                    next_path.push(next.clone());
                    stack.push((next.clone(), next_path));
                }
            }
        }
    }
    diagnostics
}

fn diag(code: &str, message: String) -> QueueRelationDiagnostic {
    QueueRelationDiagnostic {
        code: code.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_only_pull_request_url_is_not_admitted_a_queue_id() {
        let content = concat!(
            "<!-- agent:run id=api-prs -->\n",
            "- https://github.com/haiven-dev/haiven-api/pull/840\n",
            "  - %% fix(media): skip recording a scan attempt when the caller cancelled %%\n",
            "  - %% author: tmstorm %%\n",
            "- https://github.com/haiven-dev/haiven-api/pull/841\n",
            "  - %% author: tmstorm %%\n",
            "  - review the pull request\n",
            "- https://github.com/haiven-dev/haiven-api/pull/842\n",
            "<!-- /agent:run -->\n",
        );
        let projected = project(content, "api-md").content;
        assert!(
            projected.contains("- https://github.com/haiven-dev/haiven-api/pull/840\n"),
            "{projected}"
        );
        assert!(
            !projected.contains("pull/840 [#q-"),
            "a comment-only pull request must stay bare: {projected}"
        );
        assert!(
            !projected.contains("~~"),
            "admission must not strike the catalog row: {projected}"
        );
        assert!(
            projected.contains("review the pull request [#q-"),
            "a real prompt child is still admitted: {projected}"
        );
        assert!(
            projected.contains("pull/842 [#q-"),
            "a pull-request url with no children remains work: {projected}"
        );
        assert!(
            !projected.contains("pull/841 [#q-"),
            "the parent url is context, not a second id: {projected}"
        );
    }

    #[test]
    fn assigns_distinct_stable_ids_to_same_text_siblings_and_is_idempotent() {
        let content = "<!-- agent:run -->\n- same work\n- same work\n<!-- /agent:run -->\n";
        let first = project(content, "doc-a");
        assert_eq!(first.assignments.len(), 2);
        assert_ne!(first.assignments[0].id, first.assignments[1].id);
        let second = project(&first.content, "doc-a");
        assert!(second.assignments.is_empty());
        assert_eq!(second.content, first.content);
    }

    #[test]
    fn later_identical_prompt_gets_a_distinct_stable_id() {
        let original = concat!(
            "<!-- agent:run -->\n",
            "- first parent\n",
            "  - repeated work\n",
            "<!-- /agent:run -->\n",
        );
        let admitted = project(original, "doc-a");
        assert_eq!(admitted.assignments.len(), 1);

        let edited = admitted.content.replace(
            "<!-- /agent:run -->",
            "- second parent\n  - repeated work\n<!-- /agent:run -->",
        );
        let reprojection = project(&edited, "doc-a");

        assert_eq!(reprojection.assignments.len(), 1);
        assert!(
            reprojection
                .relations
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "queue_prompt_identity_collision")
        );
        let ids = identified_queue_nodes(&reprojection.content)
            .iter()
            .filter_map(node_id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 2);
        assert!(
            project(&reprojection.content, "doc-a")
                .assignments
                .is_empty()
        );
    }

    #[test]
    fn preserves_authored_ids_and_keeps_generated_metadata_inert() {
        let content = concat!(
            "<!-- agent:run -->\n",
            "- do [#authored] work\n",
            "- ~~done~~ — auto-struck: response: [#r-0123456789ab]\n",
            "  - **Response [#r-0123456789ab]:** summary\n",
            "    - **Choices:**\n",
            "      - **retry**: try again\n",
            "    - operator child prompt\n",
            "<!-- /agent:run -->\n",
        );
        let projection = project(content, "doc-a");
        assert!(projection.content.contains("do [#authored] work"));
        assert!(
            !projection
                .content
                .contains("**Response [#r-0123456789ab]:** summary [#q-")
        );
        assert!(!projection.content.contains("**Choices:** [#q-"));
        assert!(!projection.content.contains("**retry**: try again [#q-"));
        assert!(projection.content.contains("operator child prompt [#q-"));
    }

    #[test]
    fn relations_resolve_backlinks_and_diagnose_duplicate_dangling_and_cycle() {
        let content = concat!(
            "<!-- agent:run id=a -->\n",
            "- do [#one]\n",
            "  - **Related:** [#two] [#two] [#missing]\n",
            "<!-- /agent:run -->\n",
            "<!-- agent:run id=b -->\n",
            "- do [#two]\n",
            "  - **Related:** [#one]\n",
            "<!-- /agent:run -->\n",
        );
        let graph = relation_graph(content);
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.source_id == "one" && edge.target_id == "two")
        );
        assert!(
            graph
                .backlinks
                .iter()
                .any(|backlink| backlink.target_id == "two" && backlink.source_ids == ["one"])
        );
        let codes = graph
            .diagnostics
            .iter()
            .map(|diag| diag.code.as_str())
            .collect::<BTreeSet<_>>();
        assert!(codes.contains("queue_related_duplicate"));
        assert!(codes.contains("queue_related_dangling"));
        assert!(codes.contains("queue_related_cycle"));
    }

    #[test]
    fn move_strike_and_retry_keep_the_persisted_id() {
        let content = "<!-- agent:run -->\n- first\n- second\n<!-- /agent:run -->\n";
        let admitted = project(content, "doc-a").content;
        let first = admitted
            .lines()
            .find(|line| line.contains("first"))
            .unwrap();
        let id = first
            .split("[#")
            .nth(1)
            .unwrap()
            .trim_end_matches(']')
            .to_string();
        let moved = admitted.replace(
            &format!("- first [#{id}]\n- second"),
            &format!("- second\n- ~~first [#{id}]~~"),
        );
        let retried = project(&moved, "doc-a");
        assert!(retried.content.contains(&format!("~~first [#{id}]~~")));
        assert_eq!(retried.content.matches(&format!("[#{id}]")).count(), 1);
    }

    #[test]
    fn legacy_rows_migrate_only_in_projection_and_keep_authored_semantic_ids() {
        let legacy = concat!(
            "<!-- agent:run -->\n",
            "- legacy free text\n",
            "- do [#semantic] authored identity\n",
            "<!-- /agent:run -->\n",
        );
        assert!(matches!(
            agent_doc_element_queue::queue_head_identity("legacy free text"),
            agent_doc_element_queue::QueueItemIdentity::FreeText(_)
        ));
        assert_eq!(legacy.matches("[#q-").count(), 0);

        let admitted = project(legacy, "doc-a");
        assert_eq!(admitted.assignments.len(), 1);
        assert!(admitted.content.contains("legacy free text [#q-"));
        assert!(
            admitted
                .content
                .contains("do [#semantic] authored identity")
        );
        assert!(matches!(
            agent_doc_element_queue::queue_prompt_stable_id(
                admitted
                    .content
                    .lines()
                    .find(|line| line.contains("legacy free text"))
                    .unwrap()
                    .trim_start_matches("- ")
            ),
            Some(id) if id.starts_with("q-")
        ));
    }

    #[test]
    fn slash_control_handoffs_keep_their_exact_command_bytes() {
        let content = "<!-- agent:run -->\n- /clear\n- ordinary prompt\n<!-- /agent:run -->\n";
        let admitted = project(content, "doc-a");

        assert!(admitted.content.contains("- /clear\n"));
        assert!(!admitted.content.contains("/clear [#q-"));
        assert!(admitted.content.contains("ordinary prompt [#q-"));
    }

    #[test]
    fn concurrent_editor_cut_is_reprojected_without_losing_the_edit() {
        let observed = "<!-- agent:run -->\n- original prompt\n<!-- /agent:run -->\n";
        let losing_race = project(observed, "doc-a");
        let editor_cut = observed.replace(
            "- original prompt\n",
            "- original prompt\n- concurrent editor prompt\n",
        );
        let rebased = project(&editor_cut, "doc-a");

        assert_eq!(losing_race.assignments.len(), 1);
        assert_eq!(rebased.assignments.len(), 2);
        assert!(rebased.content.contains("original prompt [#q-"));
        assert!(rebased.content.contains("concurrent editor prompt [#q-"));
        assert_eq!(
            project(&rebased.content, "doc-a").content,
            rebased.content,
            "the winning authority cut is idempotent after admission"
        );
    }

    #[test]
    fn related_metadata_is_inert_but_explicit_children_are_promptable() {
        let content = concat!(
            "<!-- agent:run -->\n",
            "- do [#parent]\n",
            "  - **Related:** malformed\n",
            "    - explicit operator child\n",
            "<!-- /agent:run -->\n",
        );
        let projection = project(content, "doc-a");

        assert!(!projection.content.contains("**Related:** malformed [#q-"));
        assert!(projection.content.contains("explicit operator child [#q-"));
        assert!(
            projection
                .relations
                .diagnostics
                .iter()
                .any(|diag| diag.code == "queue_related_invalid")
        );
    }

    #[test]
    fn compaction_projection_keeps_queue_identity_out_of_exchange_patch() {
        let before = concat!(
            "<!-- agent:exchange patch=append -->\nold exchange\n<!-- /agent:exchange -->\n",
            "<!-- agent:run -->\n- prompt [#q-stable]\n<!-- /agent:run -->\n",
        );
        let after = before.replace("old exchange", "*Compacted. Content archived.*");
        let patches =
            agent_doc_document::component_patches::component_replace_patches(before, &after)
                .unwrap();

        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0]["component"], "exchange");
        assert_eq!(project(&after, "doc-a").content, after);
        assert!(after.contains("prompt [#q-stable]"));
    }

    #[test]
    fn stable_id_survives_report_state_and_echo_identity_matching() {
        let head = "answer this [#q-stable] ⟦published+local⟧";
        assert_eq!(
            crate::queue_continuation::extract_head_id(head).as_deref(),
            Some("q-stable")
        );
        assert_eq!(
            agent_doc_element_queue::queue_prompt_stable_id(head),
            Some("q-stable".to_string())
        );
        assert_eq!(
            crate::queue_continuation::extract_head_id(
                "🟡 [#subagent:worker#7] answer this [#q-stable]"
            )
            .as_deref(),
            Some("q-stable"),
            "claim-generation metadata must not eclipse the work identity"
        );
    }

    /// A q-id follows the row that shows it. After that row is gone, the same
    /// leaf under a different parent is a new row and mints a new id.
    #[test]
    fn removed_row_does_not_carry_its_q_id_to_the_same_leaf_under_a_different_parent() {
        let under_a = concat!(
            "<!-- agent:run -->\n",
            "- https://example.test/merge_requests/1\n",
            "  - #address\n",
            "<!-- /agent:run -->\n",
        );
        let admitted = project(under_a, "doc-a");
        assert_eq!(admitted.assignments.len(), 1);
        let carried = admitted.assignments[0].id.clone();

        let under_b = concat!(
            "<!-- agent:run -->\n",
            "- https://example.test/merge_requests/2\n",
            "  - #address\n",
            "<!-- /agent:run -->\n",
        );
        let moved = project(under_b, "doc-a");
        assert_eq!(moved.assignments.len(), 1);
        assert_ne!(
            moved.assignments[0].id, carried,
            "parent B reused the q-id minted under parent A"
        );
        assert!(project(&moved.content, "doc-a").assignments.is_empty());

        let explicit = admitted.content.replace(
            "https://example.test/merge_requests/1",
            "https://example.test/merge_requests/9",
        );
        let kept = project(&explicit, "doc-a");
        assert!(
            kept.assignments.is_empty(),
            "an explicit [#q-…] moved under a new parent was reminted: {kept:?}"
        );
        assert_eq!(kept.content.matches(&format!("[#{carried}]")).count(), 1);
    }
}
