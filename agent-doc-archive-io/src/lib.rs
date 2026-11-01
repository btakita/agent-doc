//! Compact archive file I/O adapters.
//!
//! The sibling exchange archive and response index are the durable project
//! database for compacted response cells. SQLite remains a rebuildable search
//! projection. A retained response is eligible for collection only after
//! [`record_exchange_archive_receipt`] proves the document, archive, and index
//! as exact blobs in one Git commit.

use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, ResponseTurnCellPolicy, is_compact_response_id,
    parse_exchange_nodes, response_lookup_id,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const EXCHANGE_ARCHIVE_SHARD_THRESHOLD_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_RESPONSE_ARCHIVE_SHARDS: usize = 32;
const ARCHIVE_HEADER: &str = "<!-- agent-doc-exchange-archive-v1 -->";
const INDEX_HEADER: &str = "<!-- agent-doc-response-index-v1 -->";
const ENTRY_PREFIX: &str = "<!-- agent-doc-response ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseIndexEntry {
    /// Bracketed public id, for example `[#r-0123456789ab]`.
    pub response_id: String,
    /// Project-relative ordinary Markdown archive path.
    pub archive_path: String,
    /// Stable, unbracketed HTML anchor (`r-…`).
    pub anchor: String,
    /// Hash of the exact response node bytes, excluding the archive anchor.
    pub content_sha256: String,
    /// The action which last changed this locator (`moved`, `restored`, or
    /// `reindexed`). This action map is authoritative; commit trailers only
    /// accelerate candidate lookup.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeArchiveWrite {
    pub archive_path: PathBuf,
    pub index_path: PathBuf,
    pub entries: Vec<ResponseIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeArchiveReceipt {
    pub commit: String,
    pub document_path: String,
    pub document_sha256: String,
    pub archive_path: String,
    pub archive_sha256: String,
    pub index_path: String,
    pub index_sha256: String,
    pub response_actions: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverySource {
    RetainedState,
    ResponseIndex,
    SqliteCache,
    GitTrailerCandidate,
    /// Compatibility recovery for response ids created before affected-id
    /// trailers and per-section index entries were available.
    GitHistoryCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredResponse {
    pub source: RecoverySource,
    pub response_id: String,
    pub content: String,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryChoice {
    Restore,
    DetachReference,
    ReplaceEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseRecovery {
    Found(RecoveredResponse),
    NeedsEditorDecision {
        response_id: String,
        reason: String,
        choices: Vec<RecoveryChoice>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseLookupMetrics {
    pub route: &'static str,
    pub elapsed_micros: u128,
    pub git_candidates_checked: usize,
}

/// Stable sibling paths. The first archive is intentionally unsharded. The
/// shard helper specifies the deterministic dated policy for a later bounded
/// rollout; the v1 writer deliberately keeps one sibling archive.
pub fn response_index_path(doc: &Path) -> PathBuf {
    sibling_path(doc, "response-index")
}

pub fn exchange_archive_path(doc: &Path) -> PathBuf {
    sibling_path(doc, "exchange-archive")
}

pub fn exchange_archive_shard_path(doc: &Path, utc_timestamp: &str, current_len: u64) -> PathBuf {
    if current_len < EXCHANGE_ARCHIVE_SHARD_THRESHOLD_BYTES {
        return exchange_archive_path(doc);
    }
    let month = utc_timestamp.get(..7).unwrap_or("unknown");
    let stem = doc
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    doc.with_file_name(format!("{stem}.exchange-archive-{month}.md"))
}

fn sibling_path(doc: &Path, suffix: &str) -> PathBuf {
    // Commit and lookup operate in the document's real repository. Resolve an
    // existing document before deriving a sibling so a path reached through a
    // workspace symlink cannot place the archive/index outside its submodule.
    let doc = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());
    let stem = doc
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("session");
    doc.with_file_name(format!("{stem}.{suffix}.md"))
}

fn anchor_for(response_id: &str) -> Result<String> {
    ensure!(
        is_compact_response_id(response_id),
        "invalid response id `{response_id}`"
    );
    Ok(response_id
        .trim_start_matches("[#")
        .trim_end_matches(']')
        .to_string())
}

fn document_response_nodes(content: &str) -> Vec<ExchangeNode> {
    let exchange_bodies = agent_doc_element::element::parse(content)
        .ok()
        .map(|components| {
            components
                .into_iter()
                .filter(|component| component.name == "exchange")
                .map(|component| component.content(content))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let scopes = if exchange_bodies.is_empty() {
        vec![content]
    } else {
        exchange_bodies
    };
    scopes
        .into_iter()
        .flat_map(parse_exchange_nodes)
        .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .collect()
}

fn response_nodes(content: &str) -> Vec<(String, String)> {
    let exchange_bodies = agent_doc_element::element::parse(content)
        .ok()
        .map(|components| {
            components
                .into_iter()
                .filter(|component| component.name == "exchange")
                .map(|component| component.content(content))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let scopes = if exchange_bodies.is_empty() {
        vec![content]
    } else {
        exchange_bodies
    };
    scopes
        .into_iter()
        .flat_map(|inner| {
            parse_exchange_nodes(inner)
                .into_iter()
                .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
                .map(move |node| {
                    let policy =
                        ResponseTurnCellPolicy::from_response_nodes(std::slice::from_ref(&node));
                    (response_lookup_id(inner, &policy), node.render())
                })
        })
        .filter(|(response_id, body)| !response_id.is_empty() && !body.trim().is_empty())
        .collect()
}

/// Response ids owned by response sections in `content`.
///
/// Unlike [`extract_response_reference_ids`], this derives ids from the
/// response cells themselves. It is used when a durable retained closeout must
/// prove that a projected backlink belongs to the exact response still waiting
/// to materialize.
pub fn response_ids_in_content(content: &str) -> BTreeSet<String> {
    response_nodes(content)
        .into_iter()
        .map(|(response_id, _)| response_id)
        .collect()
}

/// Every response identity that a retained response body can durably own.
///
/// Current responses own one id per response section. Responses captured by
/// older projections can also have a single aggregate turn id, so retained
/// closeout recovery accepts that compatibility identity without teaching the
/// response index to create new aggregate entries.
pub fn retained_response_ids_in_content(content: &str) -> BTreeSet<String> {
    let mut response_ids = response_ids_in_content(content);
    let aggregate = ResponseTurnCellPolicy::from_response(Some(content));
    if !aggregate.response_id().is_empty() {
        response_ids.insert(aggregate.response_id().to_string());
    }
    response_ids
}

fn terminal_response_cells(content: &str) -> Vec<(String, String, String)> {
    let nodes = parse_exchange_nodes(content);
    nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(|(index, node)| {
            let policy = ResponseTurnCellPolicy::from_response_nodes(std::slice::from_ref(node));
            let response_id = response_lookup_id(content, &policy);
            let response = node.render();
            let prompt = index
                .checked_sub(1)
                .and_then(|previous| nodes.get(previous))
                .filter(|node| matches!(node.kind, ExchangeNodeKind::Prompt))
                .map(|node| node.render())
                .unwrap_or_default();
            let cell = format!("{prompt}{response}");
            (response_id, response, cell)
        })
        .filter(|(response_id, response, _)| !response_id.is_empty() && !response.trim().is_empty())
        .collect()
}

/// Append complete terminal prompt/response cells to the one ordinary Markdown
/// sibling archive and atomically replace its compact response-id index. Cell
/// bytes are copied verbatim; anchors are archive framing and never inserted
/// into a prompt or response.
pub fn write_exchange_archive(doc: &Path, archived_exchange: &str) -> Result<ExchangeArchiveWrite> {
    let nodes = terminal_response_cells(archived_exchange);
    ensure!(
        !nodes.is_empty(),
        "exchange compact selected no complete terminal response cells"
    );
    let root = project_root(doc)?;
    let archive_path = exchange_archive_path(doc);
    let index_path = response_index_path(doc);
    let archive_rel = relative_slash(&root, &archive_path)?;

    let mut archive = match std::fs::read_to_string(&archive_path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => format!(
            "# Exchange archive for `{}`\n\n{}\n\n",
            doc.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("session.md"),
            ARCHIVE_HEADER,
        ),
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", archive_path.display()));
        }
    };
    let mut entries = parse_response_index_file(&index_path)?;
    let mut changed = false;
    for (response_id, response, cell) in nodes {
        let anchor = anchor_for(&response_id)?;
        let anchor_marker = format!("<a id=\"{anchor}\"></a>");
        let checksum = agent_doc_hash::content_hash(&response);
        if !archive.contains(&anchor_marker) {
            if !archive.ends_with('\n') {
                archive.push('\n');
            }
            // A prompt-shaped delimiter makes the preceding H3 response
            // terminate according to the exchange parser before archive
            // framing begins. Consequently reparsing the archive yields the
            // byte-identical response node whose checksum was indexed, even
            // after many cells have been appended.
            archive.push_str(&format!("❯ Archived response locator {response_id}\n\n"));
            archive.push_str(&anchor_marker);
            archive.push('\n');
            archive.push_str(&cell);
            if !archive.ends_with('\n') {
                archive.push('\n');
            }
            changed = true;
        }
        entries.insert(
            response_id.clone(),
            ResponseIndexEntry {
                response_id,
                archive_path: archive_rel.clone(),
                anchor,
                content_sha256: checksum,
                action: "moved".to_string(),
            },
        );
    }
    if changed || !archive_path.exists() {
        atomic_write(&archive_path, archive.as_bytes())?;
    }
    let rendered_index = render_response_index(doc, entries.values());
    if std::fs::read_to_string(&index_path).ok().as_deref() != Some(rendered_index.as_str()) {
        atomic_write(&index_path, rendered_index.as_bytes())?;
    }
    Ok(ExchangeArchiveWrite {
        archive_path,
        index_path,
        entries: entries.into_values().collect(),
    })
}

fn render_response_index<'a>(
    doc: &Path,
    entries: impl IntoIterator<Item = &'a ResponseIndexEntry>,
) -> String {
    let mut out = format!(
        "# Response index for `{}`\n\n{}\n\n",
        doc.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("session.md"),
        INDEX_HEADER,
    );
    for entry in entries {
        out.push_str(&format!(
            "{}id=\"{}\" path=\"{}\" anchor=\"{}\" sha256=\"{}\" action=\"{}\" -->\n",
            ENTRY_PREFIX,
            entry.response_id,
            entry.archive_path,
            entry.anchor,
            entry.content_sha256,
            entry.action,
        ));
        out.push_str(&format!(
            "- {} → [`{}#{}`]({}#{})\n",
            entry.response_id, entry.archive_path, entry.anchor, entry.archive_path, entry.anchor,
        ));
    }
    out
}

pub fn parse_response_index_file(path: &Path) -> Result<BTreeMap<String, ResponseIndexEntry>> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    parse_response_index(&content)
}

pub fn parse_response_index(content: &str) -> Result<BTreeMap<String, ResponseIndexEntry>> {
    let mut entries = BTreeMap::new();
    for line in content
        .lines()
        .filter(|line| line.starts_with(ENTRY_PREFIX))
    {
        let attrs = parse_attrs(
            line.trim_start_matches(ENTRY_PREFIX)
                .trim_end_matches("-->")
                .trim(),
        );
        let response_id = required_attr(&attrs, "id")?;
        let entry = ResponseIndexEntry {
            response_id: response_id.clone(),
            archive_path: required_attr(&attrs, "path")?,
            anchor: required_attr(&attrs, "anchor")?,
            content_sha256: required_attr(&attrs, "sha256")?,
            action: required_attr(&attrs, "action")?,
        };
        ensure!(
            is_compact_response_id(&response_id),
            "invalid indexed response id `{response_id}`"
        );
        entries.insert(response_id, entry);
    }
    Ok(entries)
}

fn parse_attrs(value: &str) -> BTreeMap<String, String> {
    value
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .map(|(key, value)| (key.to_string(), value.trim_matches('"').to_string()))
        .collect()
}

fn required_attr(attrs: &BTreeMap<String, String>, key: &str) -> Result<String> {
    attrs
        .get(key)
        .cloned()
        .with_context(|| format!("response index entry missing `{key}`"))
}

/// Paths that must be staged with the live document. The returned set is
/// deterministic and contains only existing sibling archive/index files.
pub fn response_archive_commit_paths(doc: &Path) -> Result<Vec<PathBuf>> {
    let index_path = response_index_path(doc);
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let root = project_root(doc)?;
    let entries = parse_response_index_file(&index_path)?;
    let mut paths = BTreeSet::from([index_path]);
    for entry in entries.values() {
        let path = root.join(&entry.archive_path);
        ensure!(
            path.exists(),
            "response index points to missing archive {}",
            path.display()
        );
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

/// Verify that every response-id reference resolves either to a live response
/// cell or to the durable sibling index. This is the shared compaction and
/// session-check orphan guard.
pub fn dangling_response_references(doc: &Path, content: &str) -> Result<Vec<String>> {
    dangling_response_references_with_pins(doc, content, std::iter::empty())
}

pub fn dangling_response_references_with_pins(
    doc: &Path,
    content: &str,
    pinned: impl IntoIterator<Item = String>,
) -> Result<Vec<String>> {
    let indexed = verified_indexed_response_ids(doc)?;
    let live = response_nodes(content)
        .into_iter()
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let mut refs = extract_response_reference_ids(content);
    refs.extend(pinned);
    let resolved = live.union(&indexed).cloned().collect::<BTreeSet<_>>();
    let mut dangling = refs.difference(&resolved).cloned().collect::<Vec<_>>();
    dangling.retain(|response_id| {
        compatibility_git_history_response(doc, response_id)
            .ok()
            .flatten()
            .is_none()
    });
    Ok(dangling)
}

fn verified_indexed_response_ids(doc: &Path) -> Result<BTreeSet<String>> {
    let index = parse_response_index_file(&response_index_path(doc))?;
    let root = project_root(doc)?;
    let mut indexed = BTreeSet::new();
    let mut archive_proofs = BTreeMap::<String, BTreeMap<String, String>>::new();
    for entry in index.values() {
        if !archive_proofs.contains_key(&entry.archive_path) {
            let archive =
                std::fs::read_to_string(root.join(&entry.archive_path)).with_context(|| {
                    format!(
                        "response index points to unreadable archive {}",
                        entry.archive_path
                    )
                })?;
            archive_proofs.insert(
                entry.archive_path.clone(),
                response_nodes(&archive)
                    .into_iter()
                    .map(|(id, body)| (id, agent_doc_hash::content_hash(&body)))
                    .collect(),
            );
        }
        ensure!(
            archive_proofs[&entry.archive_path].get(&entry.response_id)
                == Some(&entry.content_sha256),
            "archive checksum proof failed for {}",
            entry.response_id,
        );
        indexed.insert(entry.response_id.clone());
    }
    Ok(indexed)
}

/// Render only response bodies not pinned by authoritative decision evidence.
/// Archive bytes are never rewritten; this view is solely a retained-state GC
/// input after the Git durability receipt verifies.
pub fn unpinned_response_content(archive: &str, pinned: &BTreeSet<String>) -> String {
    response_nodes(archive)
        .into_iter()
        .filter(|(response_id, _)| !pinned.contains(response_id))
        .map(|(_, body)| body)
        .collect()
}

pub fn ensure_no_dangling_response_references(doc: &Path, content: &str) -> Result<()> {
    let dangling = dangling_response_references(doc, content)?;
    ensure!(
        dangling.is_empty(),
        "response index would orphan referenced response id(s): {}",
        dangling.into_iter().collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

pub fn extract_response_ids(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = content;
    while let Some(start) = rest.find("[#r-") {
        rest = &rest[start..];
        let Some(end) = rest.find(']') else { break };
        let candidate = &rest[..=end];
        if is_compact_response_id(candidate) {
            out.insert(candidate.to_string());
        }
        rest = &rest[end + 1..];
    }
    out
}

/// Response ids that carry durable evidence semantics in a session document.
///
/// Canonical ids shown in prose, fenced examples, or inline code are ordinary
/// documentation and must not become archive obligations. Evidence references
/// use one of the binary-owned projection shapes (response annotation, visible
/// response row/details, or the legacy `%% response … %%` child). Decision pins
/// are supplied separately by the decision registry.
pub fn extract_response_reference_ids(content: &str) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    let mut fence: Option<String> = None;
    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        let fence_marker = trimmed
            .strip_prefix("```")
            .map(|_| "```")
            .or_else(|| trimmed.strip_prefix("~~~").map(|_| "~~~"));
        if let Some(marker) = fence_marker {
            if fence.as_deref() == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker.to_string());
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        let visible = strip_inline_code(raw_line);
        let evidence_line = visible.contains("response: [#r-")
            || visible.contains("**Response [#r-")
            || visible.contains("%% response [#r-")
            || visible.trim_start().starts_with("### Re:") && visible.contains("[#r-");
        if evidence_line {
            refs.extend(extract_response_ids(&visible));
        }
        let mut attributes = visible.as_str();
        const RESPONSE_ATTRIBUTE: &str = "data-agent-doc-response-id=\"";
        while let Some(start) = attributes.find(RESPONSE_ATTRIBUTE) {
            let value = &attributes[start + RESPONSE_ATTRIBUTE.len()..];
            let Some(end) = value.find('"') else { break };
            let candidate = format!("[#{}]", &value[..end]);
            if is_compact_response_id(&candidate) {
                refs.insert(candidate);
            }
            attributes = &value[end + 1..];
        }
    }
    refs
}

fn strip_inline_code(line: &str) -> String {
    let mut visible = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut delimiter_width = 0usize;
    while let Some(character) = chars.next() {
        if character != '`' {
            if delimiter_width == 0 {
                visible.push(character);
            }
            continue;
        }
        let mut width = 1usize;
        while chars.peek() == Some(&'`') {
            chars.next();
            width += 1;
        }
        if delimiter_width == 0 {
            delimiter_width = width;
        } else if delimiter_width == width {
            delimiter_width = 0;
        }
    }
    visible
}

/// Generic trailer API. Decision ids are supplied by the decision-registry
/// owner; this archive layer deliberately does not invent that parser.
pub fn append_affected_id_trailers(
    message: &str,
    document_path: &str,
    response_ids: impl IntoIterator<Item = String>,
    decision_ids: impl IntoIterator<Item = String>,
) -> String {
    let responses = response_ids.into_iter().collect::<BTreeSet<_>>();
    let decisions = decision_ids.into_iter().collect::<BTreeSet<_>>();
    let mut out = message.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(&format!("Agent-Doc-Document: {document_path}\n"));
    for response_id in responses {
        out.push_str(&format!(
            "Agent-Doc-Response: {}\n",
            response_id.trim_start_matches("[#").trim_end_matches(']')
        ));
    }
    for decision_id in decisions {
        out.push_str(&format!("Agent-Doc-Decision: {decision_id}\n"));
    }
    out
}

pub fn indexed_response_ids(doc: &Path) -> Result<BTreeSet<String>> {
    Ok(parse_response_index_file(&response_index_path(doc))?
        .into_keys()
        .collect())
}

/// Rebuild the SQLite response-location acceleration cache from the tracked
/// index after verifying every referenced shard and response checksum. The
/// bound prevents a corrupted index from turning recovery into an unbounded
/// filesystem scan.
pub fn rebuild_response_location_cache(doc: &Path) -> Result<usize> {
    let root = project_root(doc)?;
    let entries = parse_response_index_file(&response_index_path(doc))?;
    let shards = entries
        .values()
        .map(|entry| entry.archive_path.as_str())
        .collect::<BTreeSet<_>>();
    ensure!(
        shards.len() <= MAX_RESPONSE_ARCHIVE_SHARDS,
        "response index references {} shards; bounded rebuild limit is {}",
        shards.len(),
        MAX_RESPONSE_ARCHIVE_SHARDS,
    );
    let mut archives = BTreeMap::new();
    for shard in shards {
        archives.insert(shard, std::fs::read_to_string(root.join(shard))?);
    }
    for entry in entries.values() {
        verify_entry_in_archive(entry, archives[entry.archive_path.as_str()].as_str())?;
    }
    let cache_entries = entries
        .values()
        .map(
            |entry| agent_doc_sqlite::archive_index::ResponseLocationCacheEntry {
                response_id: entry.response_id.clone(),
                archive_path: entry.archive_path.clone(),
                anchor: entry.anchor.clone(),
                content_sha256: entry.content_sha256.clone(),
            },
        )
        .collect::<Vec<_>>();
    agent_doc_sqlite::archive_index::replace_response_location_cache(doc, &cache_entries)
}

/// Response IDs affected by this prospective commit. This includes response
/// references added/removed in the document and entries moved/restored/reindexed
/// in the authoritative tracked index. Historical unchanged entries are omitted.
pub fn affected_response_ids_for_commit(
    doc: &Path,
    before_document: Option<&str>,
    after_document: &str,
) -> Result<BTreeSet<String>> {
    let root = project_root(doc)?;
    let index_path = response_index_path(doc);
    let index_rel = relative_slash(&root, &index_path)?;
    let before_index = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["show", &format!("HEAD:{index_rel}")])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|content| parse_response_index(&content))
        .transpose()?
        .unwrap_or_default();
    let after_index = parse_response_index_file(&index_path)?;
    let mut affected = BTreeSet::new();
    for response_id in before_index.keys().chain(after_index.keys()) {
        if before_index.get(response_id) != after_index.get(response_id) {
            affected.insert(response_id.clone());
        }
    }
    let before_refs = before_document.map(response_id_counts).unwrap_or_default();
    let after_refs = response_id_counts(after_document);
    for response_id in before_refs.keys().chain(after_refs.keys()) {
        if before_refs.get(response_id) != after_refs.get(response_id) {
            affected.insert(response_id.clone());
        }
    }
    let before_responses = before_document.map(response_checksums).unwrap_or_default();
    let after_responses = response_checksums(after_document);
    for response_id in before_responses.keys().chain(after_responses.keys()) {
        if before_responses.get(response_id) != after_responses.get(response_id) {
            affected.insert(response_id.clone());
        }
    }
    Ok(affected)
}

/// Cross the GC eligibility boundary. This succeeds only when the current HEAD
/// contains exact document, archive, and response-index blobs and every indexed
/// response checksum is present in its archive. The receipt itself is an
/// operational projection; Git remains the durable database.
pub fn record_exchange_archive_receipt(doc: &Path) -> Result<Option<ExchangeArchiveReceipt>> {
    let index_path = response_index_path(doc);
    if !index_path.exists() {
        return Ok(None);
    }
    let root = project_root(doc)?;
    let commit = git_stdout(&root, &["rev-parse", "HEAD"])?;
    let document_path = relative_slash(&root, doc)?;
    let index_rel = relative_slash(&root, &index_path)?;
    let document = head_blob(&root, &commit, &document_path)?;
    let index = head_blob(&root, &commit, &index_rel)?;
    let entries = parse_response_index(&index)?;
    ensure!(
        !entries.is_empty(),
        "committed response index has no entries"
    );
    let archive_rel = entries
        .values()
        .next()
        .context("missing response index entry")?
        .archive_path
        .clone();
    ensure!(
        entries
            .values()
            .all(|entry| entry.archive_path == archive_rel),
        "one receipt cannot cover multiple archive shards"
    );
    let archive = head_blob(&root, &commit, &archive_rel)?;
    for entry in entries.values() {
        verify_entry_in_archive(entry, &archive)?;
    }
    let receipt = ExchangeArchiveReceipt {
        commit,
        document_path,
        document_sha256: agent_doc_hash::content_hash(&document),
        archive_path: archive_rel,
        archive_sha256: agent_doc_hash::content_hash(&archive),
        index_path: index_rel,
        index_sha256: agent_doc_hash::content_hash(&index),
        response_actions: entries
            .into_iter()
            .map(|(id, entry)| (id, entry.action))
            .collect(),
    };
    let receipt_path = receipt_path(doc)?;
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    atomic_write(&receipt_path, &bytes)?;
    Ok(Some(receipt))
}

pub fn exchange_archive_receipt_proves_durability(doc: &Path) -> Result<bool> {
    let path = receipt_path(doc)?;
    let receipt: ExchangeArchiveReceipt = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("parse exchange archive receipt")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let root = project_root(doc)?;
    if git_stdout(&root, &["rev-parse", "HEAD"])? != receipt.commit {
        return Ok(false);
    }
    let document = head_blob(&root, &receipt.commit, &receipt.document_path)?;
    let archive = head_blob(&root, &receipt.commit, &receipt.archive_path)?;
    let index = head_blob(&root, &receipt.commit, &receipt.index_path)?;
    Ok(
        agent_doc_hash::content_hash(&document) == receipt.document_sha256
            && agent_doc_hash::content_hash(&archive) == receipt.archive_sha256
            && agent_doc_hash::content_hash(&index) == receipt.index_sha256,
    )
}

pub fn read_current_exchange_archive(doc: &Path) -> Result<Option<String>> {
    let index = parse_response_index_file(&response_index_path(doc))?;
    let Some(entry) = index.values().next() else {
        return Ok(None);
    };
    let root = project_root(doc)?;
    Ok(Some(std::fs::read_to_string(
        root.join(&entry.archive_path),
    )?))
}

fn compatibility_git_history_response(
    doc: &Path,
    response_id: &str,
) -> Result<Option<CompatibilityGitHistoryResponse>> {
    let root = project_root(doc)?;
    let doc_rel = relative_slash(&root, doc)?;
    let candidates = git_stdout(
        &root,
        &[
            "log",
            "--all",
            "--format=%H",
            "-n",
            "64",
            &format!("-S{response_id}"),
            "--",
            &doc_rel,
        ],
    )?;
    for (offset, commit) in candidates
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        let checked = offset + 1;
        let Ok(current) = head_blob(&root, commit, &doc_rel) else {
            continue;
        };
        if let Some(content) = response_content_by_id(&current, response_id, None) {
            return Ok(Some(CompatibilityGitHistoryResponse {
                content,
                commit: commit.to_string(),
                candidates_checked: checked,
                inline_projection: false,
            }));
        }

        let parent = head_blob(&root, &format!("{commit}^"), &doc_rel).unwrap_or_default();
        let mut parent_counts = BTreeMap::<String, usize>::new();
        for node in document_response_nodes(&parent) {
            *parent_counts.entry(node.node_id()).or_default() += 1;
        }
        let added = document_response_nodes(&current)
            .into_iter()
            .filter(|node| {
                let count = parent_counts.entry(node.node_id()).or_default();
                if *count == 0 {
                    true
                } else {
                    *count -= 1;
                    false
                }
            })
            .collect::<Vec<_>>();
        if let Some(content) = historical_response_window(&added, response_id) {
            return Ok(Some(CompatibilityGitHistoryResponse {
                content,
                commit: commit.to_string(),
                candidates_checked: checked,
                inline_projection: false,
            }));
        }

        let inline_added = added_inline_response_summaries(&parent, &current, response_id);
        if let [summary] = inline_added.as_slice() {
            return Ok(Some(CompatibilityGitHistoryResponse {
                content: inline_summary_response_cell(response_id, summary),
                commit: commit.to_string(),
                candidates_checked: checked,
                inline_projection: true,
            }));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompatibilityGitHistoryResponse {
    content: String,
    commit: String,
    candidates_checked: usize,
    inline_projection: bool,
}

fn inline_response_summaries(content: &str, response_id: &str) -> Vec<String> {
    let prefix = format!("**Response {response_id}:**");
    let mut summaries = Vec::new();
    let mut fence: Option<String> = None;
    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        let fence_marker = trimmed
            .strip_prefix("```")
            .map(|_| "```")
            .or_else(|| trimmed.strip_prefix("~~~").map(|_| "~~~"));
        if let Some(marker) = fence_marker {
            if fence.as_deref() == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker.to_string());
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let Some(summary) = item.strip_prefix(&prefix) else {
            continue;
        };
        let summary = summary.trim();
        if !summary.is_empty() {
            summaries.push(summary.to_string());
        }
    }
    summaries.extend(structured_inline_response_summaries(content, response_id));
    summaries
}

#[derive(Debug)]
struct StructuredInlineResponse {
    indent: usize,
    timestamp_seen: bool,
    summary: Option<String>,
    invalid: bool,
}

fn finish_structured_inline_response(
    candidate: StructuredInlineResponse,
    summaries: &mut BTreeSet<String>,
) {
    if candidate.timestamp_seen
        && !candidate.invalid
        && let Some(summary) = candidate.summary
    {
        summaries.insert(summary);
    }
}

/// Recover the current binary-owned response projection shape:
///
/// ```text
/// - **Response [#r-…]**
///   - **Timestamp:** …
///   - **Summary:** …
/// ```
///
/// The timestamp is deliberately not response identity. Only one non-empty,
/// direct `Summary` child proves content. Identical copies in several prompt
/// trees collapse to one value; conflicting summaries remain multiple values
/// and are rejected by the caller's exact-one compatibility gate.
fn structured_inline_response_summaries(content: &str, response_id: &str) -> BTreeSet<String> {
    let response_item = format!("**Response {response_id}**");
    let mut summaries = BTreeSet::new();
    let mut candidate: Option<StructuredInlineResponse> = None;
    let mut fence: Option<String> = None;

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();
        let fence_marker = trimmed
            .strip_prefix("```")
            .map(|_| "```")
            .or_else(|| trimmed.strip_prefix("~~~").map(|_| "~~~"));
        if let Some(marker) = fence_marker {
            if fence.is_none()
                && let Some(active) = candidate.as_mut()
            {
                let space_trimmed = raw_line.trim_start_matches(' ');
                let indent = raw_line.len() - space_trimmed.len();
                if indent > active.indent {
                    active.invalid = true;
                }
                finish_structured_inline_response(
                    candidate.take().expect("candidate is present"),
                    &mut summaries,
                );
            }
            if fence.as_deref() == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker.to_string());
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }

        // Binary projections use space indentation. Tabs and non-list
        // continuations never prove a structured response child.
        let space_trimmed = raw_line.trim_start_matches(' ');
        let indent = raw_line.len() - space_trimmed.len();
        let item = space_trimmed.strip_prefix("- ");

        if let Some(active) = candidate.as_mut() {
            if !raw_line.trim().is_empty() && indent <= active.indent {
                finish_structured_inline_response(
                    candidate.take().expect("candidate is present"),
                    &mut summaries,
                );
            } else if let Some(item) = item {
                if indent == active.indent + 2 {
                    if let Some(summary) = item.strip_prefix("**Summary:**") {
                        let summary = summary.trim();
                        if !active.timestamp_seen || summary.is_empty() || active.summary.is_some()
                        {
                            active.invalid = true;
                        } else {
                            active.summary = Some(summary.to_string());
                        }
                    } else if let Some(timestamp) = item.strip_prefix("**Timestamp:**") {
                        if active.timestamp_seen
                            || active.summary.is_some()
                            || timestamp.trim().is_empty()
                        {
                            active.invalid = true;
                        } else {
                            active.timestamp_seen = true;
                        }
                    } else {
                        active.invalid = true;
                    }
                } else if !raw_line.trim().is_empty() {
                    active.invalid = true;
                }
                continue;
            } else if !raw_line.trim().is_empty() {
                active.invalid = true;
                continue;
            }
        }

        if item == Some(response_item.as_str()) {
            candidate = Some(StructuredInlineResponse {
                indent,
                timestamp_seen: false,
                summary: None,
                invalid: false,
            });
        }
    }

    if let Some(candidate) = candidate {
        finish_structured_inline_response(candidate, &mut summaries);
    }
    summaries
}

fn added_inline_response_summaries(parent: &str, current: &str, response_id: &str) -> Vec<String> {
    let mut parent_counts = BTreeMap::<String, usize>::new();
    for summary in inline_response_summaries(parent, response_id) {
        *parent_counts.entry(summary).or_default() += 1;
    }
    inline_response_summaries(current, response_id)
        .into_iter()
        .filter(|summary| {
            let count = parent_counts.entry(summary.clone()).or_default();
            if *count == 0 {
                true
            } else {
                *count -= 1;
                false
            }
        })
        .collect()
}

fn inline_summary_response_cell(response_id: &str, summary: &str) -> String {
    format!("### Re: recovered inline queue response\n\n{summary}\n\n**Response {response_id}**\n")
}

/// Restore canonical inline response summaries that were committed before the
/// sibling response index existed. Recovery is deliberately narrow: only a
/// single newly introduced legacy `**Response [#r-…]:**` row or current
/// `**Response [#r-…]**` projection with exact `Timestamp` and `Summary` children
/// in one of the bounded Git candidates is eligible. Aggregate exchange
/// reconstruction remains lookup compatibility only because splitting it into
/// indexed per-section evidence would invent ownership.
pub fn restore_inline_response_references_from_git(
    doc: &Path,
    content: &str,
) -> Result<Vec<String>> {
    let refs = extract_response_reference_ids(content);
    ensure!(
        refs.len() <= 64,
        "document references {} response ids; bounded compatibility recovery limit is 64",
        refs.len()
    );
    let live = response_nodes(content)
        .into_iter()
        .map(|(response_id, _)| response_id)
        .collect::<BTreeSet<_>>();
    let indexed = verified_indexed_response_ids(doc)?;
    let unresolved = refs
        .difference(&live.union(&indexed).cloned().collect())
        .cloned()
        .collect::<Vec<_>>();

    let mut recovered = Vec::<(String, String)>::new();
    for response_id in unresolved {
        let Some(compatibility) = compatibility_git_history_response(doc, &response_id)? else {
            continue;
        };
        if compatibility.inline_projection {
            recovered.push((response_id, compatibility.content));
        }
    }
    if recovered.is_empty() {
        return Ok(Vec::new());
    }

    let recovered_cells = recovered
        .iter()
        .map(|(_, content)| content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let write = write_exchange_archive(doc, &recovered_cells)?;
    let restored_ids = recovered
        .into_iter()
        .map(|(response_id, _)| response_id)
        .collect::<BTreeSet<_>>();
    let mut entries = parse_response_index_file(&write.index_path)?;
    for response_id in &restored_ids {
        let entry = entries.get_mut(response_id).with_context(|| {
            format!("restored response archive omitted indexed id {response_id}")
        })?;
        entry.action = "restored".to_string();
    }
    atomic_write(
        &write.index_path,
        render_response_index(doc, entries.values()).as_bytes(),
    )?;

    let cache_entries = entries
        .values()
        .map(
            |entry| agent_doc_sqlite::archive_index::ResponseLocationCacheEntry {
                response_id: entry.response_id.clone(),
                archive_path: entry.archive_path.clone(),
                anchor: entry.anchor.clone(),
                content_sha256: entry.content_sha256.clone(),
            },
        )
        .collect::<Vec<_>>();
    agent_doc_sqlite::archive_index::replace_response_location_cache(doc, &cache_entries)?;
    agent_doc_sqlite::archive_index::index_archive(doc, &write.archive_path)?;
    Ok(restored_ids.into_iter().collect())
}

fn historical_response_window(nodes: &[ExchangeNode], response_id: &str) -> Option<String> {
    // Compatibility lookup is reached only for an unresolved durable evidence
    // reference. Bound the quadratic legacy-cell search so corrupted history
    // cannot turn session-check into an unbounded repository scan.
    let nodes = nodes
        .get(nodes.len().saturating_sub(128)..)
        .unwrap_or(nodes);
    for width in (1..=nodes.len()).rev() {
        for window in nodes.windows(width) {
            let policy = ResponseTurnCellPolicy::from_response_nodes(window);
            let legacy_transport_policy = {
                let mut legacy = window.to_vec();
                if let Some(last) = legacy.last_mut() {
                    last.lines.push("<!-- /patch:exchange -->\n".to_string());
                }
                ResponseTurnCellPolicy::from_response_nodes(&legacy)
            };
            if policy.response_id() == response_id
                || legacy_transport_policy.response_id() == response_id
            {
                return Some(window.iter().map(ExchangeNode::render).collect());
            }
        }
    }
    None
}

/// Recovery order is retained state, response index/archive, Git trailer
/// candidates with mandatory checksum verification, then bounded compatibility
/// reconstruction for legacy composite ids. Ambiguous or checksum-bad evidence
/// returns choices and never writes over editor authority.
pub fn recover_response(
    doc: &Path,
    response_id: &str,
    retained_state: Option<&str>,
) -> Result<ResponseRecovery> {
    recover_response_with_metrics(doc, response_id, retained_state).map(|(outcome, _)| outcome)
}

pub fn recover_response_with_metrics(
    doc: &Path,
    response_id: &str,
    retained_state: Option<&str>,
) -> Result<(ResponseRecovery, ResponseLookupMetrics)> {
    let started = std::time::Instant::now();
    ensure!(
        is_compact_response_id(response_id),
        "invalid response id `{response_id}`"
    );
    if let Some(retained) = retained_state
        && let Some(content) = response_content_by_id(retained, response_id, None)
    {
        return Ok((
            ResponseRecovery::Found(RecoveredResponse {
                source: RecoverySource::RetainedState,
                response_id: response_id.to_string(),
                content,
                commit: None,
            }),
            lookup_metrics("retained", started, 0),
        ));
    }
    let root = project_root(doc)?;
    if let Some(entry) = parse_response_index_file(&response_index_path(doc))?
        .get(response_id)
        .cloned()
        && let Ok(archive) = std::fs::read_to_string(root.join(&entry.archive_path))
        && let Some(content) =
            response_content_by_id(&archive, response_id, Some(&entry.content_sha256))
    {
        return Ok((
            ResponseRecovery::Found(RecoveredResponse {
                source: RecoverySource::ResponseIndex,
                response_id: response_id.to_string(),
                content,
                commit: None,
            }),
            lookup_metrics("tracked-index", started, 0),
        ));
    }
    // The tracked index is authoritative and always checked first. SQLite is a
    // rebuildable acceleration layer; its candidate is accepted only after the
    // archive content matches the cached checksum.
    if let Ok(Some(entry)) =
        agent_doc_sqlite::archive_index::lookup_response_location_cache(doc, response_id)
        && let Ok(archive) = std::fs::read_to_string(root.join(&entry.archive_path))
        && let Some(content) =
            response_content_by_id(&archive, response_id, Some(&entry.content_sha256))
    {
        return Ok((
            ResponseRecovery::Found(RecoveredResponse {
                source: RecoverySource::SqliteCache,
                response_id: response_id.to_string(),
                content,
                commit: None,
            }),
            lookup_metrics("sqlite-cache", started, 0),
        ));
    }
    let trailer = format!(
        "Agent-Doc-Response: {}",
        response_id.trim_start_matches("[#").trim_end_matches(']')
    );
    let candidates = git_stdout(
        &root,
        &["log", "--all", "--format=%H", &format!("--grep={trailer}")],
    )?;
    let mut checked = 0;
    for commit in candidates.lines().filter(|line| !line.trim().is_empty()) {
        checked += 1;
        let index_rel = relative_slash(&root, &response_index_path(doc))?;
        let Ok(index) = head_blob(&root, commit, &index_rel) else {
            continue;
        };
        let Ok(entries) = parse_response_index(&index) else {
            continue;
        };
        let Some(entry) = entries.get(response_id) else {
            continue;
        };
        let Ok(archive) = head_blob(&root, commit, &entry.archive_path) else {
            continue;
        };
        if let Some(content) =
            response_content_by_id(&archive, response_id, Some(&entry.content_sha256))
        {
            return Ok((
                ResponseRecovery::Found(RecoveredResponse {
                    source: RecoverySource::GitTrailerCandidate,
                    response_id: response_id.to_string(),
                    content,
                    commit: Some(commit.to_string()),
                }),
                lookup_metrics("git-trailer", started, checked),
            ));
        }
    }
    if let Some(compatibility) = compatibility_git_history_response(doc, response_id)? {
        checked += compatibility.candidates_checked;
        return Ok((
            ResponseRecovery::Found(RecoveredResponse {
                source: RecoverySource::GitHistoryCompatibility,
                response_id: response_id.to_string(),
                content: compatibility.content,
                commit: Some(compatibility.commit),
            }),
            lookup_metrics("git-history-compatibility", started, checked),
        ));
    }
    Ok((ResponseRecovery::NeedsEditorDecision {
        response_id: response_id.to_string(),
        reason: "retained state, the response index/archive, checksum-verified Git trailer candidates, and the bounded legacy-history compatibility scan did not prove one authoritative response".to_string(),
        choices: vec![RecoveryChoice::Restore, RecoveryChoice::DetachReference, RecoveryChoice::ReplaceEvidence],
    }, lookup_metrics("missing", started, checked)))
}

fn lookup_metrics(
    route: &'static str,
    started: std::time::Instant,
    git_candidates_checked: usize,
) -> ResponseLookupMetrics {
    ResponseLookupMetrics {
        route,
        elapsed_micros: started.elapsed().as_micros(),
        git_candidates_checked,
    }
}

fn response_content_by_id(
    content: &str,
    response_id: &str,
    checksum: Option<&str>,
) -> Option<String> {
    response_nodes(content).into_iter().find_map(|(id, body)| {
        (id == response_id
            && checksum.is_none_or(|expected| agent_doc_hash::content_hash(&body) == expected))
        .then_some(body)
    })
}

fn verify_entry_in_archive(entry: &ResponseIndexEntry, archive: &str) -> Result<()> {
    ensure!(
        response_content_by_id(archive, &entry.response_id, Some(&entry.content_sha256)).is_some(),
        "archive checksum proof failed for {}",
        entry.response_id
    );
    Ok(())
}

fn project_root(doc: &Path) -> Result<PathBuf> {
    let parent = doc.parent().unwrap_or_else(|| Path::new("."));
    match git_stdout(parent, &["rev-parse", "--show-toplevel"]) {
        Ok(root) => Ok(PathBuf::from(root)),
        Err(_) => agent_doc_project_root_io::project_root_or_file_parent(doc),
    }
}

fn response_id_counts(content: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let mut rest = content;
    while let Some(start) = rest.find("[#r-") {
        rest = &rest[start..];
        let Some(end) = rest.find(']') else { break };
        let candidate = &rest[..=end];
        if is_compact_response_id(candidate) {
            *counts.entry(candidate.to_string()).or_default() += 1;
        }
        rest = &rest[end + 1..];
    }
    counts
}

fn response_checksums(content: &str) -> BTreeMap<String, String> {
    response_nodes(content)
        .into_iter()
        .map(|(response_id, body)| (response_id, agent_doc_hash::content_hash(&body)))
        .collect()
}

fn relative_slash(root: &Path, path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(absolute
        .strip_prefix(root)
        .with_context(|| format!("{} is outside {}", absolute.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn head_blob(root: &Path, commit: &str, relative: &str) -> Result<String> {
    let object = format!("{commit}:{relative}");
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &object])
        .output()?;
    ensure!(
        output.status.success(),
        "Git commit `{commit}` does not contain `{relative}`"
    );
    String::from_utf8(output.stdout).context("committed archive blob is not UTF-8")
}

fn receipt_path(doc: &Path) -> Result<PathBuf> {
    let root = project_root(doc)?;
    let dir = root.join(".agent-doc/receipts");
    std::fs::create_dir_all(&dir)?;
    let stem = doc
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("session");
    Ok(dir.join(format!("{stem}-exchange-archive.json")))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("archive");
    let temp = parent.join(format!(".{name}.agent-doc-write-{}", std::process::id()));
    std::fs::write(&temp, bytes).with_context(|| format!("write {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))
}

/// Read every compact archive referenced by `head`, preserving pointer order and
/// ignoring missing or out-of-scope archive pointers.
pub fn read_head_compact_archives(file: &Path, head: &str) -> Vec<String> {
    agent_doc_document::compact_archive::compact_archive_pointers(head)
        .into_iter()
        .filter_map(|pointer| read_head_compact_archive(file, pointer))
        .collect()
}

/// Read a HEAD-referenced compact archive when the pointer resolves under the
/// owning project's `.agent-doc/archives` directory.
pub fn read_head_compact_archive(file: &Path, pointer: &str) -> Option<String> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let archive_root = project_root
        .join(".agent-doc/archives")
        .canonicalize()
        .ok()?;
    let pointer_path = Path::new(pointer);
    let archive_path = if pointer_path.is_absolute() {
        pointer_path.to_path_buf()
    } else {
        project_root.join(pointer_path)
    };
    let archive_path = archive_path.canonicalize().ok()?;
    let indexed_sibling = parse_response_index_file(&response_index_path(file))
        .ok()
        .is_some_and(|entries| {
            entries.values().any(|entry| {
                project_root
                    .join(&entry.archive_path)
                    .canonicalize()
                    .ok()
                    .as_ref()
                    == Some(&archive_path)
            })
        });
    if !archive_path.starts_with(&archive_root) && !indexed_sibling {
        return None;
    }
    std::fs::read_to_string(archive_path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Agent Doc Test")
            .env("GIT_AUTHOR_EMAIL", "agent-doc@example.invalid")
            .env("GIT_COMMITTER_NAME", "Agent Doc Test")
            .env("GIT_COMMITTER_EMAIL", "agent-doc@example.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn init_repo(root: &Path) {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Agent Doc Test"]);
        git(root, &["config", "user.email", "agent-doc@example.invalid"]);
    }

    fn response(topic: &str, prose: &str) -> String {
        format!("### Re: {topic}\n\n{prose}\n")
    }

    #[test]
    fn read_head_compact_archives_resolves_relative_pointers_under_archive_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let archive_dir = root.join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "visible").unwrap();
        std::fs::write(archive_dir.join("a.md"), "archived response").unwrap();

        let head = "*Compacted. Content archived to `.agent-doc/archives/a.md`*\n";

        assert_eq!(
            read_head_compact_archives(&doc, head),
            vec!["archived response".to_string()]
        );
    }

    #[test]
    fn read_head_compact_archive_rejects_pointers_outside_archive_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        let doc = root.join("session.md");
        let outside = root.join("outside.md");
        std::fs::write(&doc, "visible").unwrap();
        std::fs::write(&outside, "outside").unwrap();

        assert_eq!(read_head_compact_archive(&doc, "outside.md"), None);
    }

    #[test]
    fn sibling_archive_is_idempotent_and_preserves_exact_response_bytes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let cell = response(
            "first",
            "Exact body and prompt echo.\n\n> **Queue prompt:** do it",
        );

        let first = write_exchange_archive(&doc, &cell).unwrap();
        let archive_once = std::fs::read_to_string(&first.archive_path).unwrap();
        let second = write_exchange_archive(&doc, &cell).unwrap();
        let archive_twice = std::fs::read_to_string(&second.archive_path).unwrap();

        assert_eq!(archive_once, archive_twice);
        assert!(archive_once.contains(&cell));
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries, second.entries);
        let index = parse_response_index_file(&first.index_path).unwrap();
        assert_eq!(index.len(), 1);
        assert!(archive_once.contains(&format!("<a id=\"{}\"></a>", first.entries[0].anchor)));
    }

    #[test]
    fn sibling_archive_moves_prompt_and_response_as_one_terminal_cell() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let cell = "❯ preserve this exact prompt\n\n### Re: exact\n\nExact response.\n";

        let write = write_exchange_archive(&doc, cell).unwrap();
        let archive = std::fs::read_to_string(write.archive_path).unwrap();
        assert!(
            archive.contains(cell),
            "terminal cell was split:\n{archive}"
        );
    }

    #[test]
    fn archive_sharding_threshold_is_deterministic_and_starts_with_one_sibling() {
        let doc = Path::new("notes/session.md");
        assert_eq!(
            exchange_archive_shard_path(doc, "2026-11-01T12:00:00Z", 1),
            Path::new("notes/session.exchange-archive.md")
        );
        assert_eq!(
            exchange_archive_shard_path(
                doc,
                "2026-11-01T12:00:00Z",
                EXCHANGE_ARCHIVE_SHARD_THRESHOLD_BYTES,
            ),
            Path::new("notes/session.exchange-archive-2026-11.md")
        );
    }

    #[test]
    fn commit_failure_keeps_receipt_absent_and_gc_ineligible() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        write_exchange_archive(&doc, &response("first", "body")).unwrap();

        assert!(record_exchange_archive_receipt(&doc).is_err());
        assert!(!exchange_archive_receipt_proves_durability(&doc).unwrap());
    }

    #[test]
    fn atomic_git_receipt_covers_document_archive_and_index() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live compacted\n").unwrap();
        let write = write_exchange_archive(&doc, &response("first", "body")).unwrap();
        git(
            dir.path(),
            &[
                "add",
                "session.md",
                "session.exchange-archive.md",
                "session.response-index.md",
            ],
        );
        git(dir.path(), &["commit", "-q", "-m", "compact"]);

        let receipt = record_exchange_archive_receipt(&doc).unwrap().unwrap();
        assert!(
            receipt
                .response_actions
                .values()
                .all(|action| action == "moved")
        );
        assert!(exchange_archive_receipt_proves_durability(&doc).unwrap());
        assert_eq!(write.entries.len(), 1);
    }

    #[test]
    fn moved_archive_and_dangling_reference_are_resolved_from_index() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let write = write_exchange_archive(&doc, &response("first", "body")).unwrap();
        let response_id = write.entries[0].response_id.clone();
        let moved = dir.path().join("history.md");
        std::fs::rename(&write.archive_path, &moved).unwrap();
        let mut entries = parse_response_index_file(&write.index_path).unwrap();
        let entry = entries.get_mut(&response_id).unwrap();
        entry.archive_path = "history.md".to_string();
        entry.action = "reindexed".to_string();
        atomic_write(
            &write.index_path,
            render_response_index(&doc, entries.values()).as_bytes(),
        )
        .unwrap();
        let live = format!("- decision evidence {response_id}\n");

        assert!(
            dangling_response_references(&doc, &live)
                .unwrap()
                .is_empty()
        );
        match recover_response(&doc, &response_id, None).unwrap() {
            ResponseRecovery::Found(found) => {
                assert_eq!(found.source, RecoverySource::ResponseIndex);
                assert!(found.content.contains("body"));
            }
            other => panic!("unexpected recovery: {other:?}"),
        }
    }

    #[test]
    fn live_per_section_response_ids_keep_queue_backlinks_resolvable() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let raw = concat!(
            "### Re: alpha\n\nAlpha result.\n\n",
            "### Re: beta\n\nBeta result.\n",
        );
        let nodes = parse_exchange_nodes(raw)
            .into_iter()
            .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
            .collect::<Vec<_>>();
        let mut live = raw.to_string();
        let mut ids = Vec::new();
        for node in &nodes {
            let policy = ResponseTurnCellPolicy::from_response_nodes(std::slice::from_ref(node));
            let response_id = response_lookup_id(&live, &policy);
            live = agent_doc_markdown_ast::exchange_tree::annotate_response_with_lookup_id(
                &live,
                &policy,
                &response_id,
            );
            ids.push(response_id);
        }
        live.push_str(&format!(
            "\n- ~~alpha~~ — auto-struck: response: {}\n- ~~beta~~ — auto-struck: response: {}\n",
            ids[0], ids[1]
        ));

        assert_ne!(ids[0], ids[1]);
        assert!(
            dangling_response_references(&doc, &live)
                .unwrap()
                .is_empty(),
            "every queue backlink must resolve to its own live response section:\n{live}"
        );
    }

    #[test]
    fn response_reference_scan_ignores_examples_but_keeps_legacy_and_visible_evidence() {
        let exact_mix = concat!(
            "```md\n",
            "- **Response [#r-0123456789ab]:** example only\n",
            "```\n",
            "- Durable response IDs such as `[#r-0123456789ab]`.\n",
            "- We don't need `; response: [#r-07f614245f75]` in this example.\n",
            "- ~~historical item~~\n",
            "  - %% response [#r-0f15203ed8b8] %%\n",
            "- ~~old aggregate item~~ — auto-struck: response: [#r-3bb63500aa30]\n",
            "  - **Response [#r-3bb63500aa30]:** Historical summary.\n",
        );

        assert_eq!(
            extract_response_reference_ids(exact_mix),
            BTreeSet::from([
                "[#r-0f15203ed8b8]".to_string(),
                "[#r-3bb63500aa30]".to_string(),
            ])
        );
        assert_eq!(
            extract_response_ids(exact_mix),
            BTreeSet::from([
                "[#r-0123456789ab]".to_string(),
                "[#r-07f614245f75]".to_string(),
                "[#r-0f15203ed8b8]".to_string(),
                "[#r-3bb63500aa30]".to_string(),
            ]),
            "the raw lookup helper remains lossless for diagnostics"
        );
        assert_eq!(
            extract_response_reference_ids(
                "<details data-agent-doc-response-id=\"r-0f15203ed8b8\">\n"
            ),
            BTreeSet::from(["[#r-0f15203ed8b8]".to_string()])
        );
    }

    #[test]
    fn old_composite_response_id_recovers_from_the_commit_that_introduced_it() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        let initial = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: older\n\nOlder response.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- older item\n<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);

        let captured = concat!(
            "### Re: alpha\n\nAlpha result.\n\n",
            "### Re: beta\n\nBeta result.\n",
        );
        let legacy_transport =
            format!("<!-- patch:exchange -->\n{captured}<!-- /patch:exchange -->\n");
        let response_id = ResponseTurnCellPolicy::from_response(Some(&legacy_transport))
            .response_id()
            .to_string();
        let historical = format!(
            concat!(
                "<!-- agent:exchange -->\n",
                "### Re: older\n\nOlder response.\n",
                "{}",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue -->\n",
                "- ~~alpha~~ — auto-struck: response: {}\n",
                "  - **Response {}:** Alpha result.\n",
                "<!-- /agent:queue -->\n",
            ),
            captured, response_id, response_id
        );
        std::fs::write(&doc, &historical).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(
            dir.path(),
            &["commit", "-q", "-m", "legacy aggregate response"],
        );

        let recovered = recover_response(&doc, &response_id, None).unwrap();
        let ResponseRecovery::Found(recovered) = recovered else {
            panic!("legacy composite response should be recoverable")
        };
        assert_eq!(recovered.source, RecoverySource::GitHistoryCompatibility);
        assert_eq!(recovered.content, captured);
        assert!(
            dangling_response_references(&doc, &historical)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn structured_inline_response_summary_parsing_is_exact_and_fail_closed() {
        const RESPONSE_ID: &str = "[#r-f656df4698e6]";
        const SUMMARY: &str = "Recovered nested response evidence.";
        let mirrored = format!(
            concat!(
                "- first prompt\n",
                "  - **Response {id}**\n",
                "    - **Timestamp:** 2026-11-01T20:01:15.000Z\n",
                "    - **Summary:** {summary}\n",
                "- second prompt\n",
                "  - **Response {id}**\n",
                "    - **Timestamp:** 2026-11-01T20:02:30.000Z\n",
                "    - **Summary:** {summary}\n",
                "```md\n",
                "- **Response {id}**\n",
                "  - **Summary:** fenced example only\n",
                "```\n",
            ),
            id = RESPONSE_ID,
            summary = SUMMARY,
        );
        assert_eq!(
            inline_response_summaries(&mirrored, RESPONSE_ID),
            [SUMMARY.to_string()],
            "timestamps do not split identical cross-tree evidence"
        );

        let conflicting = format!(
            concat!(
                "- **Response {id}**\n",
                "  - **Timestamp:** 2026-11-01T20:01:15.000Z\n",
                "  - **Summary:** alpha\n",
                "- **Response {id}**\n",
                "  - **Timestamp:** 2026-11-01T20:01:15.000Z\n",
                "  - **Summary:** beta\n",
            ),
            id = RESPONSE_ID,
        );
        assert_eq!(
            inline_response_summaries(&conflicting, RESPONSE_ID).len(),
            2,
            "different summaries must remain ambiguous"
        );

        let malformed = [
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n- **Summary:** sibling\n"
            ),
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n    - **Summary:** too deep\n"
            ),
            format!("- **Response {RESPONSE_ID}**\n  - **Summary:** missing timestamp\n"),
            format!("- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n  - **Summary:**\n"),
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n\t- **Summary:** tabbed\n"
            ),
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n  - **Summary:** first\n  - **Summary:** second\n"
            ),
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Timestamp:** now\n  - **Timestamp:** later\n  - **Summary:** duplicate timestamp\n"
            ),
            format!(
                "- **Response {RESPONSE_ID}**\n  - **Summary:** wrong order\n  - **Timestamp:** now\n"
            ),
            concat!(
                "- **Response [#r-deadbeefcafe]**\n",
                "  - **Timestamp:** now\n",
                "  - **Summary:** wrong id\n",
            )
            .to_string(),
        ];
        for content in malformed {
            assert!(
                inline_response_summaries(&content, RESPONSE_ID).is_empty(),
                "malformed projection was accepted:\n{content}"
            );
        }

        let legacy = format!("- **Response {RESPONSE_ID}:** legacy summary\n");
        assert_eq!(
            inline_response_summaries(&legacy, RESPONSE_ID),
            ["legacy summary".to_string()],
            "legacy one-line evidence remains readable"
        );
        let legacy_ambiguous =
            format!("- **Response {RESPONSE_ID}:** same\n- **Response {RESPONSE_ID}:** same\n");
        assert_eq!(
            inline_response_summaries(&legacy_ambiguous, RESPONSE_ID).len(),
            2,
            "legacy duplicate evidence keeps its existing ambiguity semantics"
        );
    }

    #[test]
    fn committed_structured_response_summary_is_restored_to_the_tracked_index() {
        const RESPONSE_ID: &str = "[#r-a1b9cb2aef83]";
        const SUMMARY: &str = "Use semantic child roles, not a blanket parent exception.";
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let initial = concat!(
            "<!-- agent:queue -->\n",
            "- first prompt\n",
            "- second prompt\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);

        let historical = format!(
            concat!(
                "<!-- agent:queue -->\n",
                "- ~~first prompt~~ — auto-struck: response: {id}\n",
                "  - **Response {id}**\n",
                "    - **Timestamp:** 2026-11-01T20:01:15.000Z\n",
                "    - **Summary:** {summary}\n",
                "- ~~second prompt~~ — auto-struck: response: {id}\n",
                "  - **Response {id}**\n",
                "    - **Timestamp:** 2026-11-01T20:01:15.000Z\n",
                "    - **Summary:** {summary}\n",
                "<!-- /agent:queue -->\n",
                "~~~md\n",
                "- **Response {id}**\n",
                "  - **Summary:** conflicting fenced example\n",
                "~~~\n",
            ),
            id = RESPONSE_ID,
            summary = SUMMARY,
        );
        std::fs::write(&doc, &historical).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(
            dir.path(),
            &["commit", "-q", "-m", "committed structured response"],
        );

        let recovered = recover_response(&doc, RESPONSE_ID, None).unwrap();
        let ResponseRecovery::Found(recovered) = recovered else {
            panic!("committed structured response should be recoverable")
        };
        assert_eq!(recovered.source, RecoverySource::GitHistoryCompatibility);
        assert!(recovered.content.contains(SUMMARY), "{}", recovered.content);

        assert_eq!(
            restore_inline_response_references_from_git(&doc, &historical).unwrap(),
            [RESPONSE_ID.to_string()]
        );
        let index = parse_response_index_file(&response_index_path(&doc)).unwrap();
        assert_eq!(index[RESPONSE_ID].action, "restored");
        let archive = std::fs::read_to_string(exchange_archive_path(&doc)).unwrap();
        assert!(archive.contains(SUMMARY), "{archive}");
        assert!(archive.contains(&format!("**Response {RESPONSE_ID}**")));
        assert!(
            dangling_response_references(&doc, &historical)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn committed_inline_response_summary_is_restored_to_the_tracked_index() {
        const RESPONSE_ID: &str = "[#r-f656df4698e6]";
        const UNRELATED_ID: &str = "[#r-deadbeefcafe]";
        const SUMMARY: &str = "Implemented and installed. Every open agent:todo now emits a selector-preserving todo_sync_required preflight obligation.";
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let initial = concat!(
            "<!-- agent:queue -->\n",
            "- agent-doc issues in sdk-dev.md\n",
            "  - The same response was duplicated for several queue items.\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, initial).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(dir.path(), &["commit", "-q", "-m", "initial"]);

        let historical = format!(
            concat!(
                "<!-- agent:queue -->\n",
                "- agent-doc issues in sdk-dev.md\n",
                "  - ~~The same response was duplicated for several queue items.~~ — auto-struck: response: {id}\n",
                "    - **Response {id}:** {summary}\n",
                "  - ~~unrelated unresolved item~~ — auto-struck: response: {unrelated}\n",
                "<!-- /agent:queue -->\n",
                "```md\n",
                "- **Response {id}:** example only\n",
                "```\n",
            ),
            id = RESPONSE_ID,
            summary = SUMMARY,
            unrelated = UNRELATED_ID,
        );
        std::fs::write(&doc, &historical).unwrap();
        git(dir.path(), &["add", "session.md"]);
        git(
            dir.path(),
            &["commit", "-q", "-m", "committed inline response"],
        );

        let recovered = recover_response(&doc, RESPONSE_ID, None).unwrap();
        let ResponseRecovery::Found(recovered) = recovered else {
            panic!("committed inline response should be recoverable")
        };
        assert_eq!(recovered.source, RecoverySource::GitHistoryCompatibility);
        assert!(recovered.content.contains(SUMMARY), "{}", recovered.content);
        assert!(
            recovered
                .content
                .contains(&format!("**Response {RESPONSE_ID}**"))
        );

        assert_eq!(
            restore_inline_response_references_from_git(&doc, &historical).unwrap(),
            [RESPONSE_ID.to_string()]
        );
        let index = parse_response_index_file(&response_index_path(&doc)).unwrap();
        assert_eq!(index[RESPONSE_ID].action, "restored");
        let archive = std::fs::read_to_string(exchange_archive_path(&doc)).unwrap();
        assert!(archive.contains(SUMMARY), "{archive}");
        assert!(archive.contains(&format!("**Response {RESPONSE_ID}**")));
        assert_eq!(
            dangling_response_references(&doc, &historical).unwrap(),
            [UNRELATED_ID.to_string()],
            "bounded recovery must not cover an unrelated true orphan"
        );
        assert_eq!(
            restore_inline_response_references_from_git(&doc, &historical).unwrap(),
            Vec::<String>::new(),
            "restoration is idempotent once the tracked index resolves the id"
        );
    }

    #[test]
    fn decision_pins_are_dangling_without_verified_live_or_archive_evidence() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let pinned = "[#r-0123456789ab]".to_string();

        assert_eq!(
            dangling_response_references_with_pins(&doc, "live\n", [pinned.clone()]).unwrap(),
            [pinned]
        );
    }

    #[test]
    fn decision_pins_are_excluded_from_retained_state_gc_input() {
        let first = response("first", "keep pinned");
        let second = response("second", "collect durable");
        let archive = format!("{first}{second}");
        let first_id = response_nodes(&first)[0].0.clone();
        let gc = unpinned_response_content(&archive, &BTreeSet::from([first_id]));
        assert!(!gc.contains("keep pinned"));
        assert!(gc.contains("collect durable"));
    }

    #[test]
    fn git_trailer_candidate_is_checksum_verified_before_recovery() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let write = write_exchange_archive(&doc, &response("first", "body from git")).unwrap();
        let response_id = write.entries[0].response_id.clone();
        let message = append_affected_id_trailers(
            "compact",
            "session.md",
            [response_id.clone()],
            std::iter::empty(),
        );
        git(
            dir.path(),
            &[
                "add",
                "session.md",
                "session.exchange-archive.md",
                "session.response-index.md",
            ],
        );
        git(dir.path(), &["commit", "-q", "-m", &message]);
        std::fs::remove_file(&write.archive_path).unwrap();
        std::fs::remove_file(&write.index_path).unwrap();

        let (outcome, metrics) = recover_response_with_metrics(&doc, &response_id, None).unwrap();
        assert_eq!(metrics.route, "git-trailer");
        assert_eq!(metrics.git_candidates_checked, 1);
        match outcome {
            ResponseRecovery::Found(found) => {
                assert_eq!(found.source, RecoverySource::GitTrailerCandidate);
                assert!(found.content.contains("body from git"));
                assert!(found.commit.is_some());
            }
            other => panic!("unexpected recovery: {other:?}"),
        }
    }

    #[test]
    fn response_lookup_routes_are_bounded_and_measured_at_scale() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        const SCALE: usize = 100;
        let cells = (0..SCALE)
            .map(|n| response(&format!("topic {n}"), &format!("body {n}")))
            .collect::<String>();
        let write = write_exchange_archive(&doc, &cells).unwrap();
        let response_id = write.entries[61].response_id.clone();

        let (_, cold) = recover_response_with_metrics(&doc, &response_id, None).unwrap();
        assert_eq!(cold.route, "tracked-index");
        assert_eq!(cold.git_candidates_checked, 0);

        let rebuild_started = std::time::Instant::now();
        assert_eq!(rebuild_response_location_cache(&doc).unwrap(), SCALE);
        let rebuild_micros = rebuild_started.elapsed().as_micros();
        std::fs::remove_file(&write.index_path).unwrap();
        let (_, cached) = recover_response_with_metrics(&doc, &response_id, None).unwrap();
        assert_eq!(cached.route, "sqlite-cache");
        assert_eq!(cached.git_candidates_checked, 0);

        eprintln!(
            "[perf] response-index entries={} cold_us={} cached_us={} rebuild_us={} shard_bound={}",
            SCALE,
            cold.elapsed_micros,
            cached.elapsed_micros,
            rebuild_micros,
            MAX_RESPONSE_ARCHIVE_SHARDS,
        );
        // These are regression tripwires, not machine-speed benchmarks. They
        // chiefly prove neither normal route invokes Git history and rebuild is
        // bounded by indexed shards rather than repository history.
        assert!(cold.elapsed_micros < 5_000_000);
        assert!(cached.elapsed_micros < 5_000_000);
        assert!(rebuild_micros < 10_000_000);
    }

    #[test]
    fn ambiguous_deletion_offers_structured_choices_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "operator authority\n").unwrap();
        let before = std::fs::read_to_string(&doc).unwrap();
        let missing = "[#r-0123456789ab]";

        let outcome = recover_response(&doc, missing, None).unwrap();
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), before);
        assert_eq!(
            outcome,
            ResponseRecovery::NeedsEditorDecision {
                response_id: missing.to_string(),
                reason: "retained state, the response index/archive, checksum-verified Git trailer candidates, and the bounded legacy-history compatibility scan did not prove one authoritative response".to_string(),
                choices: vec![RecoveryChoice::Restore, RecoveryChoice::DetachReference, RecoveryChoice::ReplaceEvidence],
            }
        );
    }

    #[test]
    fn affected_id_trailers_are_sorted_deduplicated_and_support_decisions() {
        let message = append_affected_id_trailers(
            "agent-doc(session): now",
            "notes/session.md",
            [
                "[#r-bbbbbbbbbbbb]".to_string(),
                "[#r-aaaaaaaaaaaa]".to_string(),
                "[#r-bbbbbbbbbbbb]".to_string(),
            ],
            ["d-z".to_string(), "d-a".to_string(), "d-a".to_string()],
        );
        assert_eq!(
            message
                .matches("Agent-Doc-Response: r-bbbbbbbbbbbb")
                .count(),
            1
        );
        assert!(message.find("r-aaaaaaaaaaaa").unwrap() < message.find("r-bbbbbbbbbbbb").unwrap());
        assert!(
            message.find("Agent-Doc-Decision: d-a").unwrap()
                < message.find("Agent-Doc-Decision: d-z").unwrap()
        );
        assert!(message.contains("Agent-Doc-Document: notes/session.md"));
    }

    #[test]
    fn changed_response_body_is_affected_even_when_lookup_marker_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "live\n").unwrap();
        let response_id = "[#r-0123456789ab]";
        let before = format!("### Re: stable\n\n**Response {response_id}**\n\nold body\n");
        let after = before.replace("old body", "new body");

        assert_eq!(
            affected_response_ids_for_commit(&doc, Some(&before), &after).unwrap(),
            BTreeSet::from([response_id.to_string()]),
        );
    }
}
