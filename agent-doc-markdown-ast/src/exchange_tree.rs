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
//! here intentionally mirror `agent-doc-merge::semantic_merge` (`is_h3_heading` /
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
    /// heading; prompts off their trimmed body text.
    pub fn node_id(&self) -> String {
        match &self.kind {
            ExchangeNodeKind::Response { key } => format!("r:{}", short_content_hash(key)),
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
        assert!(prompts.iter().any(|p| p.render().contains("Leading user note.")));
        assert!(prompts
            .iter()
            .any(|p| p.render().contains("❯ Regenerate the response.")));
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
}
