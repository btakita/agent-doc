//! Atomic assistant-response cells for the realtime document backbone.
//!
//! A response is one body-aware cell containing one or more assistant exchange
//! nodes. The operation is intentionally smaller than a template patch or
//! whole-document replacement: replaying the same cell is a no-op, while a
//! response with the same heading and a different body remains distinct.

use agent_doc_element::element;
use agent_doc_markdown_ast::exchange_tree::{ExchangeNode, ExchangeNodeKind, parse_exchange_nodes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseCellAddOutcome {
    pub content: String,
    pub cell_id: String,
    pub applied: bool,
}

fn parse_response_cell(response: &str) -> anyhow::Result<(Vec<ExchangeNode>, String)> {
    let response = response.trim_matches(['\n', '\r']);
    if response.is_empty() {
        anyhow::bail!("response cell is empty");
    }
    let rendered = format!("{}\n", response.trim_end());
    let nodes = parse_exchange_nodes(&rendered);
    if nodes.is_empty() {
        anyhow::bail!("response cell has no exchange nodes");
    }
    if nodes
        .iter()
        .any(|node| !matches!(node.kind, ExchangeNodeKind::Response { .. }))
    {
        anyhow::bail!("response cell may contain only assistant response nodes");
    }
    Ok((nodes, rendered))
}

fn response_cell_id(nodes: &[ExchangeNode]) -> String {
    if nodes.len() == 1 {
        return nodes[0].node_id();
    }
    format!(
        "response-cell:{}",
        nodes
            .iter()
            .map(ExchangeNode::node_id)
            .collect::<Vec<_>>()
            .join("+")
    )
}

/// Add one assistant response cell to the first `agent:exchange` component.
///
/// A cell may contain multiple response headings (for example, one closeout
/// answering several operator topics), but never prompt nodes. The ordered,
/// body-aware node ids make the group idempotent across retries. The current
/// document is parsed at apply time, so operator prompts appended while the agent
/// was working remain before the new response instead of being overwritten by a
/// stale whole-document candidate.
pub fn add_response_cell(doc: &str, response: &str) -> anyhow::Result<ResponseCellAddOutcome> {
    let (nodes, rendered) = parse_response_cell(response)?;
    let cell_id = response_cell_id(&nodes);
    let node_ids = nodes.iter().map(ExchangeNode::node_id).collect::<Vec<_>>();
    let components = element::parse(doc)?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    let current = exchange.content(doc);

    let current_node_ids = parse_exchange_nodes(current)
        .iter()
        .map(ExchangeNode::node_id)
        .collect::<Vec<_>>();
    if current_node_ids
        .windows(node_ids.len())
        .any(|window| window == node_ids)
    {
        return Ok(ResponseCellAddOutcome {
            content: doc.to_string(),
            cell_id,
            applied: false,
        });
    }

    let mut next = current.trim_end_matches('\n').to_string();
    if !next.trim().is_empty() {
        next.push_str("\n\n");
    }
    next.push_str(&rendered);

    Ok(ResponseCellAddOutcome {
        content: exchange.replace_content(doc, &next),
        cell_id,
        applied: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\n\u{276f} operator prompt\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";

    #[test]
    fn response_cell_add_preserves_live_prompt_and_is_replay_safe() {
        let response = "### Re: operator prompt — gpt-5\n\nDone.";
        let first = add_response_cell(DOC, response).unwrap();
        assert!(first.applied);
        assert!(first.content.contains("\u{276f} operator prompt"));
        assert!(
            first.content.find("\u{276f} operator prompt").unwrap()
                < first.content.find("### Re: operator prompt").unwrap()
        );

        let replay = add_response_cell(&first.content, response).unwrap();
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, first.cell_id);
        assert_eq!(replay.content, first.content);
    }

    #[test]
    fn same_heading_with_different_body_is_a_distinct_cell() {
        let first = add_response_cell(DOC, "### Re: topic\n\nFirst.").unwrap();
        let second = add_response_cell(&first.content, "### Re: topic\n\nSecond.").unwrap();
        assert!(second.applied);
        assert_ne!(second.cell_id, first.cell_id);
        assert_eq!(second.content.matches("### Re: topic").count(), 2);
    }

    #[test]
    fn multiple_response_nodes_are_one_replay_safe_cell() {
        let response =
            "### Re: first topic — gpt-5\n\nFirst.\n\n### Re: second topic — gpt-5\n\nSecond.";
        let first = add_response_cell(DOC, response).unwrap();
        assert!(first.applied);
        assert!(first.cell_id.starts_with("response-cell:r:"));
        assert_eq!(first.content.matches("### Re:").count(), 2);

        let replay = add_response_cell(&first.content, response).unwrap();
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, first.cell_id);
        assert_eq!(replay.content, first.content);
    }

    #[test]
    fn response_cell_rejects_embedded_operator_prompt() {
        let response = "### Re: topic — gpt-5\n\nDone.\n\n❯ operator prompt";
        let err = add_response_cell(DOC, response).unwrap_err();
        assert!(err.to_string().contains("only assistant response nodes"));
    }
}
