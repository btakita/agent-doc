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

use agent_doc_hash::short_content_hash;

/// What a single exchange node represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeNodeKind {
    /// A user prompt turn — a caret `❯ …` line or free user text before the first
    /// response. Never merged into a neighboring response.
    Prompt,
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
                format!("r:{}", short_content_hash(&response_identity_digest(&self.lines)))
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
        } else if is_transient_boundary(trimmed) {
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

    for line in split_keep_newlines(inner) {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if is_h3_heading(trimmed) {
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
    fn empty_exchange_yields_no_nodes() {
        assert!(parse_exchange_nodes("").is_empty());
        assert_eq!(render_exchange_nodes(&[]), "");
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
