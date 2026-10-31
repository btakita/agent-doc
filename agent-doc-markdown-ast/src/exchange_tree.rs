//! Exchange document-tree — the `agent:exchange` component body modeled as an
//! ordered list of distinct nodes, so a merge can never bleed one response's text
//! into another response or into a user prompt.
//!
//! - Each `### Re:` response (its heading line plus the following content) is one
//!   [`ExchangeNode`] with [`ExchangeNodeKind::Response`].
//! - Each user prompt — a caret `❯ …` line, or free user text before the first
//!   response — is its own [`ExchangeNodeKind::Prompt`] node, never absorbed into a
//!   neighboring response.
//!
//! Round-trip is byte-stable: `render_exchange_nodes(&parse_exchange_nodes(s)) == s`
//! for any exchange body, so this can back the CRDT representation without changing
//! the on-disk format.
//!
//! This is Phase 2 of
//! `tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`. Phase 3 maps each
//! node onto a `lazily::SeqCrdt` element (structure) + `TextCrdt` (body), replacing
//! the whole-doc `yrs` merge that allows cross-response bleed. The heading helpers
//! here intentionally mirror `agent-doc-merge::document_cell_merge` (`is_h3_heading` /
//! `normalize_heading_key`) and should be unified into this module in Phase 3.

use agent_doc_hash::{content_hash, short_content_hash};
use std::{collections::HashSet, ops::Range};

/// What a single exchange node represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeNodeKind {
    /// A user prompt turn — a caret `❯ …` line or free user text before the first
    /// response. Never merged into a neighboring response.
    Prompt,
    /// Operator-visible, non-final assistant progress for one open cycle.
    /// The enclosing protocol comments make this node replaceable without
    /// allowing its body to prove that a prompt was answered.
    Salient { cycle_id: String },
    /// An agent response turn — a `### Re: …` heading plus its following content.
    /// `key` is the normalized heading identity (leading `### `, a surrounding
    /// `~~…~~` strike wrapper, and a trailing ` (HEAD)` annotation stripped) used
    /// to recognize the same turn across replicas.
    Response { key: String },
}

/// One node of the exchange tree: a prompt or a response, with its verbatim lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeNode {
    pub kind: ExchangeNodeKind,
    /// Verbatim source lines, each retaining its trailing newline exactly as
    /// captured, so [`ExchangeNode::render`] is byte-stable.
    pub lines: Vec<String>,
}

impl ExchangeNode {
    /// Concatenate the node's verbatim lines back to source.
    pub fn render(&self) -> String {
        self.lines.concat()
    }

    /// Stable, content-derived identity that survives re-parse (Phase 3 maps it
    /// onto a `lazily::SeqCrdt` element id). Responses key off the normalized
    /// heading **and body** (see [`response_identity_digest`]); prompts off their
    /// trimmed body text.
    ///
    /// `#qcellmerge-response-body-id`: a response identity is heading + body, not
    /// heading alone. Two response turns that share a heading (a same-topic /
    /// same-preset follow-up drained from the queue) but carry different bodies are
    /// **distinct** turns and must never collide — heading-only identity made a
    /// fresh response a false duplicate of a prior committed turn, so the cell
    /// merge dropped it ("a cell-merge chose the existing content"). Byte-identical
    /// mirror-ordered duplicates (a poisoned buffer from a failed 3-way merge)
    /// still share an identity and still collapse, because their heading **and**
    /// body match.
    pub fn node_id(&self) -> String {
        match &self.kind {
            ExchangeNodeKind::Response { .. } => {
                format!(
                    "r:{}",
                    short_content_hash(&response_identity_digest(&self.lines))
                )
            }
            ExchangeNodeKind::Prompt => {
                let body = self
                    .lines
                    .iter()
                    .map(|l| l.trim())
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("p:{}", short_content_hash(body.trim()))
            }
            ExchangeNodeKind::Salient { cycle_id } => format!("s:{cycle_id}"),
        }
    }
}

/// Is `trimmed` an h3 heading line (`### …`)? Mirrors the exchange turn shape.
fn is_h3_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### ") || trimmed == "###"
}

/// A caret prompt line (`❯ …`) — the canonical operator prompt marker.
fn is_caret_prompt(trimmed: &str) -> bool {
    trimmed.starts_with('❯')
}

const SALIENT_OPEN_PREFIX: &str = "<!-- agent:salient-response cycle=\"";
const SALIENT_OPEN_SUFFIX: &str = "\" -->";
const SALIENT_CLOSE: &str = "<!-- /agent:salient-response -->";
const SALIENT_HEADING: &str = "#### Live response (not final)";

fn salient_cycle_id_from_open_marker(trimmed: &str) -> Option<String> {
    let cycle_id = trimmed
        .strip_prefix(SALIENT_OPEN_PREFIX)?
        .strip_suffix(SALIENT_OPEN_SUFFIX)?;
    (!cycle_id.is_empty()
        && cycle_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character)))
    .then(|| cycle_id.to_string())
}

fn is_salient_close_marker(trimmed: &str) -> bool {
    trimmed == SALIENT_CLOSE
}

/// Normalize a `### ` heading line into a stable turn-identity key: strip the
/// leading `### `, a surrounding `~~…~~` strike wrapper, and a trailing ` (HEAD)`
/// boundary annotation (transient — must not affect identity).
fn normalize_heading_key(trimmed: &str) -> String {
    let body = trimmed.strip_prefix("###").unwrap_or(trimmed).trim();
    let mut t = body.trim();
    if t.len() >= 4 && t.starts_with("~~") && t.ends_with("~~") {
        t = t[2..t.len() - 2].trim();
    }
    if let Some(stripped) = t.strip_suffix("(HEAD)") {
        t = stripped.trim_end();
    }
    t.to_string()
}

/// Is `trimmed` a transient `<!-- agent:boundary:… -->` marker line? Boundary
/// markers carry per-cycle ids and ride at the tail of the exchange body, so they
/// must never contribute to a turn's identity.
fn is_transient_boundary(trimmed: &str) -> bool {
    trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
}

/// Is `trimmed` the binary-owned lookup marker for a response cell?
///
/// The marker contains a public id derived from the response cell, so it must be
/// identity-neutral to avoid a self-referential hash. Human-authored
/// `%% response … %%` salient snippets remain identity-bearing; only the
/// A binary-owned response lookup marker is transient. Current markers expose a
/// compact bracketed id (`%% response [#r-<hash>] %%`); the legacy `r:<hash>`
/// form remains accepted so old documents normalize without changing identity.
pub fn is_response_id_lookup_marker(trimmed: &str) -> bool {
    response_id_from_lookup_marker(trimmed).is_some()
}

/// Return the response id carried by a binary-owned lookup marker.
pub fn response_id_from_lookup_marker(trimmed: &str) -> Option<&str> {
    let response_id = trimmed.strip_prefix("%% response ")?.strip_suffix(" %%")?;
    if is_compact_response_id(response_id) {
        return Some(response_id);
    }
    response_id
        .strip_prefix("r:")
        .filter(|hash| {
            !hash.is_empty() && hash.chars().all(|character| character.is_ascii_hexdigit())
        })
        .map(|_| response_id)
}

/// Public response ids use the normal document lookup shape while keeping the
/// structural response-node/cell identity private.
pub fn is_compact_response_id(response_id: &str) -> bool {
    response_id
        .strip_prefix("[#r-")
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|hash| {
            hash.len() >= 12 && hash.chars().all(|character| character.is_ascii_hexdigit())
        })
}

/// Normalize a response turn's verbatim lines into a transient-invariant identity
/// digest: heading + body, ignoring the working-tree-only ` (HEAD)` annotation, a
/// `~~…~~` strike wrapper, per-cycle boundary markers, and trailing whitespace /
/// blank lines. See [`ExchangeNode::node_id`] for why the body — not the heading
/// alone — is part of the identity (`#qcellmerge-response-body-id`).
///
/// The first `### ` heading normalizes through [`normalize_heading_key`]; every
/// other line keeps its leading whitespace (code / indentation is significant) but
/// drops trailing whitespace. Boundary markers and trailing blank lines are
/// removed so two renderings of the same turn that differ only by transient
/// framing hash identically.
pub fn response_identity_digest(lines: &[String]) -> String {
    let mut norm: Vec<String> = Vec::new();
    for line in lines {
        let raw = line.trim_end_matches(['\n', '\r']);
        let trimmed = raw.trim();
        if is_h3_heading(trimmed) {
            norm.push(normalize_heading_key(trimmed));
        } else if is_transient_boundary(trimmed) || is_response_id_lookup_marker(trimmed) {
            continue;
        } else {
            norm.push(raw.trim_end().to_string());
        }
    }
    while norm.last().is_some_and(|l| l.trim().is_empty()) {
        norm.pop();
    }
    norm.join("\n")
}

/// Shared identity and order-binding policy for one captured assistant response
/// cell. Disk closeout, realtime/IPC insertion, and repair all use this policy so
/// transient framing and repeated response headings cannot be interpreted
/// differently by each path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseTurnCellPolicy {
    cell_id: String,
    response_id: String,
    node_ids: Vec<String>,
    order_signature_lines: HashSet<String>,
}

impl ResponseTurnCellPolicy {
    pub fn from_response(response: Option<&str>) -> Self {
        let response = response.unwrap_or("").trim_matches(['\n', '\r']);
        let nodes = if response.is_empty() {
            Vec::new()
        } else {
            parse_exchange_nodes(&format!("{}\n", response.trim_end()))
                .into_iter()
                .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
                .collect::<Vec<_>>()
        };
        Self::from_response_nodes(&nodes)
    }

    pub fn from_response_nodes(nodes: &[ExchangeNode]) -> Self {
        let response_nodes = nodes
            .iter()
            .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
            .collect::<Vec<_>>();
        let node_ids = response_nodes
            .iter()
            .map(|node| node.node_id())
            .collect::<Vec<_>>();
        let cell_id = match node_ids.as_slice() {
            [] => String::new(),
            [node_id] => node_id.clone(),
            node_ids => format!("response-cell:{}", node_ids.join("+")),
        };
        let response_id = compact_response_id(&cell_id, 12);
        let order_signature_lines = response_nodes
            .iter()
            .flat_map(|node| node.lines.iter())
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .filter(|line| !line.starts_with("<!--"))
            .filter(|line| *line != "Done.")
            .map(normalize_response_order_line)
            .collect();
        Self {
            cell_id,
            response_id,
            node_ids,
            order_signature_lines,
        }
    }

    pub fn cell_id(&self) -> &str {
        &self.cell_id
    }

    /// Compact operator-facing lookup id. The ordered composite [`Self::cell_id`]
    /// remains the internal replay/idempotence identity.
    pub fn response_id(&self) -> &str {
        &self.response_id
    }

    pub fn node_ids(&self) -> &[String] {
        &self.node_ids
    }

    pub fn matches_order_line(&self, line: &str) -> bool {
        !self.node_ids.is_empty()
            && self
                .order_signature_lines
                .contains(&normalize_response_order_line(line))
    }

    pub fn matches_order_block<'a>(&self, lines: impl IntoIterator<Item = &'a str>) -> bool {
        !self.node_ids.is_empty() && lines.into_iter().any(|line| self.matches_order_line(line))
    }

    /// Return true only when `lines` contain the complete captured response cell.
    /// Order repair needs this stronger proof before moving anything across a
    /// boundary: a boundary can transiently bisect a multi-paragraph response,
    /// and one matching signature line is not proof that the remaining suffix is
    /// a user prompt.
    pub fn matches_identity_block<'a>(&self, lines: impl IntoIterator<Item = &'a str>) -> bool {
        if self.node_ids.is_empty() {
            return false;
        }
        let candidate = lines.into_iter().collect::<Vec<_>>().join("\n");
        Self::from_response(Some(&candidate)).cell_id == self.cell_id
    }

    /// Bind to the newest response heading matching this captured cell. A prior
    /// response with the same generic heading must never claim a later prompt.
    pub fn newest_matching_index<'a>(
        &self,
        candidates: impl IntoIterator<Item = (usize, &'a str)>,
    ) -> Option<usize> {
        candidates
            .into_iter()
            .filter(|(_, line)| self.matches_order_line(line))
            .map(|(index, _)| index)
            .last()
    }
}

fn compact_response_id(cell_id: &str, hash_len: usize) -> String {
    if cell_id.is_empty() {
        return String::new();
    }
    let hash = content_hash(cell_id);
    let hash_len = hash_len.clamp(12, hash.len());
    format!("[#r-{}]", &hash[..hash_len])
}

fn response_cell_node_range(
    nodes: &[ExchangeNode],
    policy: &ResponseTurnCellPolicy,
) -> Option<Range<usize>> {
    let width = policy.node_ids.len();
    if width == 0 || width > nodes.len() {
        return None;
    }
    nodes
        .windows(width)
        .enumerate()
        .filter(|(_, window)| {
            window.iter().zip(&policy.node_ids).all(|(node, expected)| {
                matches!(node.kind, ExchangeNodeKind::Response { .. })
                    && node.node_id() == *expected
            })
        })
        .map(|(start, _)| start..start + width)
        .next_back()
}

/// Whether the exact internal response-cell identity is present in this exchange.
pub fn response_cell_materialized(inner: &str, policy: &ResponseTurnCellPolicy) -> bool {
    response_cell_node_range(&parse_exchange_nodes(inner), policy).is_some()
}

/// Resolve the public id for a response cell in one exchange. A retry reuses the
/// marker already attached to that cell. A short-prefix collision with another
/// response extends the hash deterministically until the id is unique.
pub fn response_lookup_id(inner: &str, policy: &ResponseTurnCellPolicy) -> String {
    if policy.cell_id.is_empty() {
        return String::new();
    }
    let nodes = parse_exchange_nodes(inner);
    let Some(target) = response_cell_node_range(&nodes, policy) else {
        return policy.response_id.clone();
    };
    if let Some(existing) = nodes[target.clone()]
        .iter()
        .flat_map(|node| &node.lines)
        .find_map(|line| response_id_from_lookup_marker(line.trim()))
        .filter(|response_id| is_compact_response_id(response_id))
    {
        return existing.to_string();
    }

    let occupied = nodes
        .iter()
        .enumerate()
        .filter(|(index, _)| !target.contains(index))
        .flat_map(|(_, node)| &node.lines)
        .filter_map(|line| response_id_from_lookup_marker(line.trim()))
        .filter(|response_id| is_compact_response_id(response_id))
        .collect::<HashSet<_>>();
    let full_hash_len = content_hash(&policy.cell_id).len();
    (12..=full_hash_len)
        .map(|hash_len| compact_response_id(&policy.cell_id, hash_len))
        .find(|candidate| !occupied.contains(candidate.as_str()))
        .unwrap_or_else(|| policy.response_id.clone())
}

/// Normalize transient response-turn framing for order matching. ` (HEAD)` is a
/// working-tree annotation and a leaked prompt marker is transport syntax; neither
/// changes the response cell's identity.
pub fn normalize_response_order_line(line: &str) -> String {
    let normalized = line.trim().trim_start_matches('❯').trim();
    normalized
        .strip_suffix(" (HEAD)")
        .unwrap_or(normalized)
        .to_string()
}

/// Split a string into lines that each retain their trailing `\n` (unlike
/// [`str::lines`]), so the parts concatenate back to the exact input.
fn split_keep_newlines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            out.push(s[start..=i].to_string());
            start = i + 1;
        }
    }
    if start < s.len() {
        out.push(s[start..].to_string());
    }
    out
}

/// Parse an exchange component body into an ordered list of distinct prompt /
/// response nodes. Every source line lands in exactly one node, so
/// [`render_exchange_nodes`] reproduces the input byte-for-byte.
///
/// A `### ` heading starts a new response node; a caret `❯` line is always its own
/// prompt node; any other line appends to the node in progress (or opens a leading
/// prompt node when none is open yet). Because each `### Re:` response is a separate
/// node, a merge can never fold one response's content into another, and caret /
/// leading prompts stay separate from responses.
pub fn parse_exchange_nodes(inner: &str) -> Vec<ExchangeNode> {
    let mut nodes: Vec<ExchangeNode> = Vec::new();
    let mut current: Option<ExchangeNode> = None;
    let mut salient_closed = false;

    for line in split_keep_newlines(inner) {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if matches!(
            current.as_ref().map(|node| &node.kind),
            Some(ExchangeNodeKind::Salient { .. })
        ) {
            if !salient_closed {
                salient_closed = is_salient_close_marker(trimmed);
                current
                    .as_mut()
                    .expect("salient node exists")
                    .lines
                    .push(line);
                continue;
            }
            if trimmed.is_empty() {
                current
                    .as_mut()
                    .expect("salient node exists")
                    .lines
                    .push(line);
                continue;
            }
            nodes.push(current.take().expect("salient node exists"));
            salient_closed = false;
        }

        if let Some(cycle_id) = salient_cycle_id_from_open_marker(trimmed) {
            if let Some(n) = current.take() {
                nodes.push(n);
            }
            current = Some(ExchangeNode {
                kind: ExchangeNodeKind::Salient { cycle_id },
                lines: vec![line],
            });
        } else if is_h3_heading(trimmed) {
            if let Some(n) = current.take() {
                nodes.push(n);
            }
            current = Some(ExchangeNode {
                kind: ExchangeNodeKind::Response {
                    key: normalize_heading_key(trimmed),
                },
                lines: vec![line],
            });
        } else if is_caret_prompt(trimmed) {
            if let Some(n) = current.take() {
                nodes.push(n);
            }
            current = Some(ExchangeNode {
                kind: ExchangeNodeKind::Prompt,
                lines: vec![line],
            });
        } else if let Some(n) = current.as_mut() {
            n.lines.push(line);
        } else {
            current = Some(ExchangeNode {
                kind: ExchangeNodeKind::Prompt,
                lines: vec![line],
            });
        }
    }
    if let Some(n) = current.take() {
        nodes.push(n);
    }
    nodes
}

/// Render an exchange node list back to a byte-stable source string.
pub fn render_exchange_nodes(nodes: &[ExchangeNode]) -> String {
    nodes.iter().map(ExchangeNode::render).collect()
}

/// Add one compact public id to the matching response cell as a terse lookup
/// marker. Multi-section cells receive one marker on their final response node.
/// The internal ordered node identity selects the cell but is never rendered.
pub fn annotate_response_with_lookup_id(
    inner: &str,
    policy: &ResponseTurnCellPolicy,
    response_id: &str,
) -> String {
    if !is_compact_response_id(response_id) {
        return inner.to_string();
    }
    let marker = format!("%% response {response_id} %%");
    let mut nodes = parse_exchange_nodes(inner);
    let Some(target) = response_cell_node_range(&nodes, policy) else {
        return inner.to_string();
    };
    for node in &mut nodes[target.clone()] {
        node.lines
            .retain(|line| !is_response_id_lookup_marker(line.trim()));
    }
    let node = &mut nodes[target.end - 1];
    let insert_at = node
        .lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(node.lines.len());
    node.lines.insert(insert_at, format!("{marker}\n"));
    render_exchange_nodes(&nodes)
}

// ---------------------------------------------------------------------------
// Structural operations (Phase 4 API) — the calls an agent/editor should use to
// mutate the exchange instead of a raw text edit + snapshot re-baseline. All are
// pure `inner -> inner` transforms over the node model, so they cannot bleed one
// node's content into another.
// ---------------------------------------------------------------------------

/// A lightweight summary of one exchange node for listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeNodeSummary {
    pub node_id: String,
    /// `"response"` or `"prompt"`.
    pub kind: String,
    /// The heading line (responses) or first non-empty line (prompts).
    pub label: String,
}

/// List the exchange nodes with stable id, kind, and a short label.
pub fn list_exchange_nodes(inner: &str) -> Vec<ExchangeNodeSummary> {
    parse_exchange_nodes(inner)
        .iter()
        .map(|n| {
            let (kind, label) = match &n.kind {
                ExchangeNodeKind::Response { .. } => (
                    "response",
                    n.lines
                        .first()
                        .map(|l| l.trim().to_string())
                        .unwrap_or_default(),
                ),
                ExchangeNodeKind::Prompt => (
                    "prompt",
                    n.lines
                        .iter()
                        .map(|l| l.trim())
                        .find(|l| !l.is_empty())
                        .unwrap_or("")
                        .to_string(),
                ),
                ExchangeNodeKind::Salient { cycle_id } => {
                    ("salient", format!("live response for {cycle_id}"))
                }
            };
            ExchangeNodeSummary {
                node_id: n.node_id(),
                kind: kind.to_string(),
                label,
            }
        })
        .collect()
}

/// Collapse runs of 3+ consecutive newlines into a single blank line, so removing
/// or moving a node does not leave a widening gap between neighbors.
fn normalize_blank_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newlines = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push(ch);
            }
        } else {
            newlines = 0;
            out.push(ch);
        }
    }
    out
}

/// Remove the node whose [`ExchangeNode::node_id`] equals `node_id`. Returns the
/// new inner body, or `None` if no such node exists. This is the operation that
/// should have removed the reitrades IPC-diagnostic blocks (instead of a raw edit).
pub fn remove_exchange_node(inner: &str, node_id: &str) -> Option<String> {
    let nodes = parse_exchange_nodes(inner);
    if !nodes.iter().any(|n| n.node_id() == node_id) {
        return None;
    }
    let kept: Vec<ExchangeNode> = nodes
        .into_iter()
        .filter(|n| n.node_id() != node_id)
        .collect();
    Some(normalize_blank_runs(&render_exchange_nodes(&kept)))
}

/// Append a text turn to the end of the exchange body with a blank-line separator.
fn append_turn(inner: &str, turn: &str) -> String {
    if inner.trim().is_empty() {
        return turn.to_string();
    }
    let mut out = inner.trim_end_matches('\n').to_string();
    out.push_str("\n\n");
    out.push_str(turn);
    out
}

/// Append a new agent response turn (`### Re: {header}` + `body`) at the end of the
/// exchange. `body` is trimmed of trailing whitespace and terminated with a newline.
pub fn add_response(inner: &str, header: &str, body: &str) -> String {
    let turn = format!("### Re: {}\n\n{}\n", header.trim(), body.trim_end());
    append_turn(inner, &turn)
}

/// Append a new user prompt turn at the end of the exchange. The text is caret-
/// prefixed (`❯ …`) if it is not already, so it parses back as a distinct
/// [`ExchangeNodeKind::Prompt`] node rather than being folded into a response.
pub fn add_prompt(inner: &str, text: &str) -> String {
    let t = text.trim();
    let turn = if t.starts_with('❯') {
        format!("{t}\n")
    } else {
        format!("❯ {t}\n")
    };
    append_turn(inner, &turn)
}

/// Render one complete, cycle-keyed non-final response node.
pub fn render_salient_response(cycle_id: &str, body: &str) -> String {
    format!(
        "{SALIENT_OPEN_PREFIX}{cycle_id}{SALIENT_OPEN_SUFFIX}\n\
{SALIENT_HEADING}\n\n{}\n\
{SALIENT_CLOSE}\n",
        body.trim_matches(['\n', '\r'])
    )
}

/// Append or replace the one salient response node owned by `cycle_id`.
pub fn upsert_salient_response(inner: &str, cycle_id: &str, body: &str) -> String {
    let rendered = render_salient_response(cycle_id, body);
    let replacement = parse_exchange_nodes(&rendered)
        .into_iter()
        .next()
        .expect("rendered salient response is one node");
    let mut nodes = parse_exchange_nodes(inner);
    if let Some(index) = nodes.iter().position(|node| {
        matches!(
            &node.kind,
            ExchangeNodeKind::Salient {
                cycle_id: existing
            } if existing == cycle_id
        )
    }) {
        if nodes[index].render().trim_end() == replacement.render().trim_end() {
            return inner.to_string();
        }
        nodes[index] = replacement;
        return render_exchange_nodes(&nodes);
    }
    append_turn(inner, &rendered)
}

/// Remove every non-final response node. Final response insertion uses this
/// unconditionally so replay also cleans a stale progress projection.
pub fn remove_all_salient_responses(inner: &str) -> String {
    render_exchange_nodes(
        &parse_exchange_nodes(inner)
            .into_iter()
            .filter(|node| !matches!(node.kind, ExchangeNodeKind::Salient { .. }))
            .collect::<Vec<_>>(),
    )
}

/// Exchange content with its non-final salient nodes separated out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SalientPartition {
    /// The exchange content with every salient node removed.
    pub content: String,
    /// The rendered salient node owned by the still-open cycle, if any.
    pub live: Option<String>,
    /// Salient nodes owned by any other (ended or abandoned) cycle.
    pub dropped_stale: usize,
}

/// Separate every salient node from `inner`.
///
/// A salient node is display state for one open cycle, never history, so
/// compaction must neither archive nor commit it. The node owned by
/// `open_cycle_id` is returned in [`SalientPartition::live`] so the live
/// projection can keep showing it until final response insertion removes it;
/// a node from any other cycle is a stale orphan and is dropped.
pub fn partition_salient_responses(inner: &str, open_cycle_id: Option<&str>) -> SalientPartition {
    let mut kept = Vec::new();
    let mut live = String::new();
    let mut dropped_stale = 0;
    for node in parse_exchange_nodes(inner) {
        match &node.kind {
            ExchangeNodeKind::Salient { cycle_id } if Some(cycle_id.as_str()) == open_cycle_id => {
                live.push_str(&node.render());
            }
            ExchangeNodeKind::Salient { .. } => dropped_stale += 1,
            _ => kept.push(node),
        }
    }
    SalientPartition {
        content: render_exchange_nodes(&kept),
        live: (!live.trim().is_empty()).then_some(live),
        dropped_stale,
    }
}

pub fn salient_response_materialized(inner: &str, cycle_id: &str, body: &str) -> bool {
    let expected = render_salient_response(cycle_id, body);
    parse_exchange_nodes(inner).into_iter().any(|node| {
        matches!(
            &node.kind,
            ExchangeNodeKind::Salient {
                cycle_id: existing
            } if existing == cycle_id
        ) && node.render().trim_end() == expected.trim_end()
    })
}

/// Move `node_id` to immediately before (`before = true`) or after the node
/// `anchor_id`. Returns `None` if either id is missing.
pub fn move_exchange_node(
    inner: &str,
    node_id: &str,
    anchor_id: &str,
    before: bool,
) -> Option<String> {
    let mut nodes = parse_exchange_nodes(inner);
    let from = nodes.iter().position(|n| n.node_id() == node_id)?;
    if !nodes.iter().any(|n| n.node_id() == anchor_id) {
        return None;
    }
    let node = nodes.remove(from);
    let anchor_pos = nodes.iter().position(|n| n.node_id() == anchor_id)?;
    let insert_at = if before { anchor_pos } else { anchor_pos + 1 };
    nodes.insert(insert_at, node);
    Some(render_exchange_nodes(&nodes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Leading user note.\n\n❯ Regenerate the response.\n\n### Re: Regenerated — opus-4-8\n\nHere is the answer.\n\n### Re: Follow-up — opus-4-8\n\nSecond answer.\n";

    #[test]
    fn round_trip_is_byte_stable() {
        let nodes = parse_exchange_nodes(SAMPLE);
        assert_eq!(render_exchange_nodes(&nodes), SAMPLE);
    }

    #[test]
    fn each_response_is_a_distinct_node_no_cross_bleed() {
        let nodes = parse_exchange_nodes(SAMPLE);
        let responses: Vec<_> = nodes
            .iter()
            .filter(|n| matches!(n.kind, ExchangeNodeKind::Response { .. }))
            .collect();
        assert_eq!(responses.len(), 2, "two separate response nodes");
        // The second response's text must not appear in the first node.
        assert!(responses[0].render().contains("Here is the answer."));
        assert!(!responses[0].render().contains("Second answer."));
        assert!(responses[1].render().contains("Second answer."));
    }

    #[test]
    fn caret_and_leading_prompts_are_distinct_from_responses() {
        let nodes = parse_exchange_nodes(SAMPLE);
        let prompts: Vec<_> = nodes
            .iter()
            .filter(|n| n.kind == ExchangeNodeKind::Prompt)
            .collect();
        // Leading note + the caret prompt are two prompt nodes, neither of which
        // is folded into a response.
        assert_eq!(prompts.len(), 2);
        assert!(
            prompts
                .iter()
                .any(|p| p.render().contains("Leading user note."))
        );
        assert!(
            prompts
                .iter()
                .any(|p| p.render().contains("❯ Regenerate the response."))
        );
    }

    #[test]
    fn node_ids_are_stable_across_reparse_and_distinct_per_response() {
        let a = parse_exchange_nodes(SAMPLE);
        let b = parse_exchange_nodes(SAMPLE);
        let ids_a: Vec<_> = a.iter().map(ExchangeNode::node_id).collect();
        let ids_b: Vec<_> = b.iter().map(ExchangeNode::node_id).collect();
        assert_eq!(ids_a, ids_b, "ids are deterministic across re-parse");
        // The two responses have distinct ids (heading-keyed).
        let resp_ids: Vec<_> = a
            .iter()
            .filter(|n| matches!(n.kind, ExchangeNodeKind::Response { .. }))
            .map(ExchangeNode::node_id)
            .collect();
        assert_ne!(resp_ids[0], resp_ids[1]);
    }

    #[test]
    fn transient_head_annotation_does_not_change_response_identity() {
        let plain = parse_exchange_nodes("### Re: Topic — opus-4-8\n\nBody.\n");
        let head = parse_exchange_nodes("### Re: Topic — opus-4-8 (HEAD)\n\nBody.\n");
        assert_eq!(plain[0].node_id(), head[0].node_id());
    }

    #[test]
    fn response_turn_cell_policy_binds_identity_and_newest_matching_heading() {
        let plain =
            ResponseTurnCellPolicy::from_response(Some("### Re: Topic — opus-4-8\n\nBody.\n"));
        let head = ResponseTurnCellPolicy::from_response(Some(
            "### Re: Topic — opus-4-8 (HEAD)\n\nBody.\n",
        ));

        assert_eq!(plain.cell_id(), head.cell_id());
        assert_eq!(
            plain.newest_matching_index([
                (1, "### Re: Topic — opus-4-8"),
                (4, "### Re: Other — opus-4-8"),
                (7, "### Re: Topic — opus-4-8 (HEAD)"),
            ]),
            Some(7),
        );
        assert!(plain.matches_order_block(["unrelated", "Body."]));
        assert!(plain.matches_identity_block([
            "### Re: Topic — opus-4-8 (HEAD)",
            "",
            "Body.",
            "<!-- agent:boundary:b0 -->",
        ]));
        assert!(!plain.matches_identity_block(["### Re: Topic — opus-4-8", "", "Bod"]));
    }

    #[test]
    fn empty_exchange_yields_no_nodes() {
        assert!(parse_exchange_nodes("").is_empty());
        assert_eq!(render_exchange_nodes(&[]), "");
    }

    #[test]
    fn salient_block_is_one_non_prompt_non_response_node() {
        let block = render_salient_response(
            "cycle-7",
            "Confirmed diagnosis.\n\n### Heading inside progress\n\n❯ example",
        );
        let nodes = parse_exchange_nodes(&block);
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            ExchangeNodeKind::Salient {
                cycle_id: "cycle-7".to_string()
            }
        );
        assert_eq!(nodes[0].node_id(), "s:cycle-7");
        assert_eq!(render_exchange_nodes(&nodes), block);
    }

    #[test]
    fn salient_upsert_replaces_by_cycle_and_final_cleanup_removes_it() {
        let initial = "❯ Investigate.\n";
        let first = upsert_salient_response(initial, "cycle-7", "First finding.");
        let replay = upsert_salient_response(&first, "cycle-7", "First finding.");
        assert_eq!(replay, first);

        let second = upsert_salient_response(&first, "cycle-7", "Confirmed finding.");
        assert!(!second.contains("First finding."));
        assert!(salient_response_materialized(
            &second,
            "cycle-7",
            "Confirmed finding."
        ));
        assert_eq!(
            remove_all_salient_responses(&second).trim_end(),
            initial.trim_end()
        );
    }

    #[test]
    fn salient_partition_keeps_open_cycle_live_and_drops_stale_orphans() {
        let history = "### Re: Older\n\nDone.\n";
        let stale = upsert_salient_response(history, "cycle-old", "Abandoned finding.");
        let both = upsert_salient_response(&stale, "cycle-open", "Live finding.");

        let partition = partition_salient_responses(&both, Some("cycle-open"));
        assert_eq!(partition.content.trim_end(), history.trim_end());
        assert_eq!(partition.dropped_stale, 1);
        let live = partition.live.expect("the open cycle's node stays live");
        assert!(live.contains("Live finding.") && !live.contains("Abandoned finding."));
        assert!(salient_response_materialized(
            &live,
            "cycle-open",
            "Live finding."
        ));

        let no_open_cycle = partition_salient_responses(&both, None);
        assert_eq!(no_open_cycle.live, None);
        assert_eq!(no_open_cycle.dropped_stale, 2);
        assert_eq!(no_open_cycle.content.trim_end(), history.trim_end());

        let untouched = partition_salient_responses(history, Some("cycle-open"));
        assert_eq!(untouched.content, history);
        assert_eq!((untouched.live, untouched.dropped_stale), (None, 0));
    }

    // --- Phase 4 structural operations ---

    #[test]
    fn list_reports_each_node_with_kind_and_label() {
        let summaries = list_exchange_nodes(SAMPLE);
        assert_eq!(summaries.iter().filter(|s| s.kind == "response").count(), 2);
        assert_eq!(summaries.iter().filter(|s| s.kind == "prompt").count(), 2);
        assert!(summaries.iter().any(|s| s.label.contains("Regenerated")));
    }

    #[test]
    fn remove_drops_only_the_targeted_node() {
        let nodes = parse_exchange_nodes(SAMPLE);
        let target = nodes
            .iter()
            .find(|n| n.render().contains("Second answer."))
            .unwrap()
            .node_id();
        let out = remove_exchange_node(SAMPLE, &target).unwrap();
        assert!(!out.contains("Second answer."));
        // Everything else survives.
        assert!(out.contains("Here is the answer."));
        assert!(out.contains("❯ Regenerate the response."));
        assert!(out.contains("Leading user note."));
    }

    #[test]
    fn remove_unknown_id_returns_none() {
        assert!(remove_exchange_node(SAMPLE, "r:deadbeef").is_none());
    }

    #[test]
    fn add_response_appends_a_distinct_parseable_turn() {
        let out = add_response(SAMPLE, "New topic — opus", "Fresh answer.");
        let nodes = parse_exchange_nodes(&out);
        assert_eq!(
            nodes
                .iter()
                .filter(|n| matches!(n.kind, ExchangeNodeKind::Response { .. }))
                .count(),
            3
        );
        assert!(out.contains("### Re: New topic — opus"));
        assert!(out.contains("Fresh answer."));
    }

    #[test]
    fn add_prompt_caret_prefixes_and_stays_a_prompt_node() {
        let out = add_prompt(SAMPLE, "Please continue.");
        assert!(out.contains("❯ Please continue."));
        let added = parse_exchange_nodes(&out)
            .into_iter()
            .find(|n| n.render().contains("Please continue."))
            .unwrap();
        assert_eq!(added.kind, ExchangeNodeKind::Prompt);
    }

    #[test]
    fn response_lookup_marker_is_searchable_but_identity_neutral() {
        let inner = "### Re: lookup\n\nCompleted the work.\n\n❯ Next prompt.\n";
        let policy =
            ResponseTurnCellPolicy::from_response(Some("### Re: lookup\n\nCompleted the work.\n"));
        let response_id = response_lookup_id(inner, &policy);

        let annotated = annotate_response_with_lookup_id(inner, &policy, &response_id);

        assert!(
            annotated.contains(&format!("%% response {response_id} %%\n\n❯ Next prompt.")),
            "{annotated}"
        );
        assert!(is_compact_response_id(&response_id));
        assert!(!annotated.contains(policy.cell_id()));
        assert_eq!(
            ResponseTurnCellPolicy::from_response(Some(&annotated)).cell_id(),
            policy.cell_id()
        );
        assert_eq!(
            response_lookup_id(&annotated, &policy),
            response_id,
            "a retry reuses the cell's public id"
        );
        assert_eq!(
            annotate_response_with_lookup_id(&annotated, &policy, &response_id),
            annotated
        );
    }

    #[test]
    fn multi_section_response_cell_exposes_one_compact_id() {
        let response = concat!(
            "### Re: lookup — test\n\nCompleted the work.\n\n",
            "### Verification\n\nAll focused tests pass.\n",
        );
        let inner = format!("{response}\n❯ Next prompt.\n");
        let policy = ResponseTurnCellPolicy::from_response(Some(response));
        assert!(policy.cell_id().starts_with("response-cell:r:"));

        let response_id = response_lookup_id(&inner, &policy);
        let annotated = annotate_response_with_lookup_id(&inner, &policy, &response_id);

        assert_eq!(annotated.matches(&response_id).count(), 1, "{annotated}");
        assert!(
            annotated.contains(&format!(
                "All focused tests pass.\n%% response {response_id} %%\n\n❯ Next prompt."
            )),
            "{annotated}"
        );
        assert!(!annotated.contains(policy.cell_id()));
    }

    #[test]
    fn compact_response_ids_distinguish_same_heading_different_bodies() {
        let first = ResponseTurnCellPolicy::from_response(Some("### Re: topic\n\nFirst body.\n"));
        let second = ResponseTurnCellPolicy::from_response(Some("### Re: topic\n\nSecond body.\n"));

        assert_ne!(first.cell_id(), second.cell_id());
        assert_ne!(first.response_id(), second.response_id());
    }

    #[test]
    fn compact_response_id_extends_on_short_prefix_collision() {
        let response = "### Re: target\n\nTarget body.\n";
        let policy = ResponseTurnCellPolicy::from_response(Some(response));
        let inner = format!(
            "### Re: older\n\nOlder body.\n%% response {} %%\n\n{response}",
            policy.response_id()
        );

        let resolved = response_lookup_id(&inner, &policy);

        assert!(resolved.starts_with(policy.response_id().trim_end_matches(']')));
        assert_eq!(resolved.len(), policy.response_id().len() + 1);
        assert_ne!(resolved, policy.response_id());
    }

    #[test]
    fn salient_response_marker_remains_identity_bearing() {
        let plain = "### Re: lookup\n\nCompleted the work.\n";
        let salient = "### Re: lookup\n\nCompleted the work.\n%% response published locally %%\n";
        let plain_id = parse_exchange_nodes(plain)[0].node_id();
        let salient_id = parse_exchange_nodes(salient)[0].node_id();
        assert_ne!(plain_id, salient_id);
    }

    #[test]
    fn move_reorders_a_node_relative_to_an_anchor() {
        let nodes = parse_exchange_nodes(SAMPLE);
        let second = nodes
            .iter()
            .find(|n| n.render().contains("Second answer."))
            .unwrap()
            .node_id();
        let first = nodes
            .iter()
            .find(|n| n.render().contains("Here is the answer."))
            .unwrap()
            .node_id();
        let out = move_exchange_node(SAMPLE, &second, &first, true).unwrap();
        assert!(out.find("Second answer.").unwrap() < out.find("Here is the answer.").unwrap());
    }
}
