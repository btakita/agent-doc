//! Atomic assistant-response cells for the realtime document backbone.
//!
//! A response is one body-aware cell containing one or more assistant exchange
//! nodes. The operation is intentionally smaller than a template patch or
//! whole-document replacement: replaying the same cell is a no-op, while a
//! response with the same heading and a different body remains distinct.

use agent_doc_element::element;
use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
};
use std::collections::HashSet;

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

fn boundary_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
}

fn supersede_response_tail_after_anchor(
    doc: &str,
    anchor_id: &str,
    retained_response_ids: &HashSet<String>,
    response: &str,
) -> anyhow::Result<Option<ResponseCellAddOutcome>> {
    let components = element::parse(doc)?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    let current_nodes = parse_exchange_nodes(exchange.content(doc));
    let Some(anchor_index) = current_nodes
        .iter()
        .rposition(|node| node.node_id() == anchor_id)
    else {
        return Ok(None);
    };

    let mut removed = false;
    let mut boundary = None;
    let mut retained = Vec::with_capacity(current_nodes.len());
    for (index, mut node) in current_nodes.into_iter().enumerate() {
        for line in &node.lines {
            if boundary_line(line) {
                boundary = Some(line.trim().to_string());
            }
        }
        node.lines.retain(|line| !boundary_line(line));
        let stale_response = index > anchor_index
            && matches!(node.kind, ExchangeNodeKind::Response { .. })
            && !retained_response_ids.contains(&node.node_id());
        if stale_response {
            removed = true;
            continue;
        }
        if !node.lines.is_empty() {
            retained.push(node);
        }
    }
    if !removed {
        return Ok(None);
    }

    let cleaned_exchange = render_exchange_nodes(&retained);
    let cleaned_doc = exchange.replace_content(doc, &cleaned_exchange);
    let mut outcome = add_response_cell(&cleaned_doc, response)?;
    if let Some(boundary) = boundary {
        let reparsed = element::parse(&outcome.content)?;
        let reparsed_exchange = reparsed
            .iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
        let mut inner = reparsed_exchange
            .content(&outcome.content)
            .trim_end_matches('\n')
            .to_string();
        if !inner.is_empty() {
            inner.push('\n');
        }
        inner.push_str(&boundary);
        inner.push('\n');
        outcome.content = reparsed_exchange.replace_content(&outcome.content, &inner);
    }
    outcome.applied = true;
    Ok(Some(outcome))
}

/// Replace response nodes appended after the last unchanged committed response,
/// then add the latest complete response as one semantic cell.
///
/// This is the normal closeout recovery path when an older retained response is
/// restored after the next complete response has already been captured. Prompt
/// nodes are never removed. If the last committed response cannot be found
/// unchanged in the current document, the operation fails safe to additive
/// behavior so operator edits to committed history are preserved.
pub fn supersede_uncommitted_response_tail(
    doc: &str,
    committed_doc: &str,
    response: &str,
) -> anyhow::Result<ResponseCellAddOutcome> {
    let committed_components = element::parse(committed_doc)?;
    let Some(committed_exchange) = committed_components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return add_response_cell(doc, response);
    };
    let committed_nodes = parse_exchange_nodes(committed_exchange.content(committed_doc));
    let committed_response_ids = committed_nodes
        .iter()
        .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(ExchangeNode::node_id)
        .collect::<HashSet<_>>();
    let Some(last_committed_response_id) = committed_nodes
        .iter()
        .rev()
        .find(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(ExchangeNode::node_id)
    else {
        return add_response_cell(doc, response);
    };

    if let Some(outcome) = supersede_response_tail_after_anchor(
        doc,
        &last_committed_response_id,
        &committed_response_ids,
        response,
    )? {
        Ok(outcome)
    } else {
        add_response_cell(doc, response)
    }
}

/// Reconcile an editor still based on a superseded deferred target with the
/// latest authoritative response tail.
///
/// The last response common to both targets is the safe anchor. Responses that
/// exist only in the prior target are removed from `doc`, responses that exist
/// only in the latest target are installed as one cell, and operator prompts in
/// `doc` remain untouched. `None` means the targets do not prove a response-tail
/// supersession and the caller must use its ordinary merge policy.
pub fn reconcile_superseded_response_targets(
    doc: &str,
    prior_target: &str,
    latest_target: &str,
) -> anyhow::Result<Option<String>> {
    let prior_components = element::parse(prior_target)?;
    let Some(prior_exchange) = prior_components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let latest_components = element::parse(latest_target)?;
    let Some(latest_exchange) = latest_components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let prior_nodes = parse_exchange_nodes(prior_exchange.content(prior_target));
    let latest_nodes = parse_exchange_nodes(latest_exchange.content(latest_target));
    let latest_node_ids = latest_nodes
        .iter()
        .map(ExchangeNode::node_id)
        .collect::<HashSet<_>>();
    let latest_response_ids = latest_nodes
        .iter()
        .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(ExchangeNode::node_id)
        .collect::<HashSet<_>>();
    let Some((prior_anchor_index, anchor_id)) = prior_nodes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, node)| latest_node_ids.contains(&node.node_id()))
        .map(|(index, node)| (index, node.node_id()))
    else {
        return Ok(None);
    };
    let prior_has_superseded_response =
        prior_nodes.iter().skip(prior_anchor_index + 1).any(|node| {
            matches!(node.kind, ExchangeNodeKind::Response { .. })
                && !latest_response_ids.contains(&node.node_id())
        });
    if !prior_has_superseded_response {
        return Ok(None);
    }
    let Some(latest_anchor_index) = latest_nodes
        .iter()
        .rposition(|node| node.node_id() == anchor_id)
    else {
        return Ok(None);
    };
    let prior_response_ids = prior_nodes
        .iter()
        .filter(|node| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(ExchangeNode::node_id)
        .collect::<HashSet<_>>();
    let common_response_ids = prior_response_ids
        .intersection(&latest_response_ids)
        .cloned()
        .collect::<HashSet<_>>();
    let replacement_nodes = latest_nodes
        .iter()
        .skip(latest_anchor_index + 1)
        .filter(|node| {
            matches!(node.kind, ExchangeNodeKind::Response { .. })
                && !prior_response_ids.contains(&node.node_id())
        })
        .cloned()
        .map(|mut node| {
            node.lines.retain(|line| !boundary_line(line));
            node
        })
        .filter(|node| !node.lines.is_empty())
        .collect::<Vec<_>>();
    if replacement_nodes.is_empty() {
        return Ok(None);
    }

    let response = render_exchange_nodes(&replacement_nodes);
    Ok(
        supersede_response_tail_after_anchor(doc, &anchor_id, &common_response_ids, &response)?
            .map(|outcome| outcome.content),
    )
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
    fn latest_complete_response_supersedes_uncommitted_response_tail() {
        let committed = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ original prompt\n\n",
            "### Re: committed — gpt-5 (HEAD)\n\nCommitted.\n",
            "<!-- agent:boundary:committed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = committed.replace(
            "<!-- agent:boundary:committed -->",
            concat!(
                "### Re: stale retained — gpt-5\n\nOld response.\n\n",
                "❯ operator follow-up\n\n",
                "### Re: interrupted retry — gpt-5\n\nPartial response.\n",
                "<!-- agent:boundary:latest -->",
            ),
        );
        let latest = "### Re: complete retry — gpt-5\n\nComplete response.";

        let outcome = supersede_uncommitted_response_tail(&current, committed, latest).unwrap();

        assert!(outcome.applied);
        assert!(outcome.content.contains("Committed."));
        assert!(outcome.content.contains("❯ operator follow-up"));
        assert!(!outcome.content.contains("Old response."));
        assert!(!outcome.content.contains("Partial response."));
        assert_eq!(outcome.content.matches("Complete response.").count(), 1);
        assert_eq!(outcome.content.matches("agent:boundary:").count(), 1);
        assert!(
            outcome.content.find("❯ operator follow-up").unwrap()
                < outcome.content.find("### Re: complete retry").unwrap()
        );
        assert!(
            outcome.content.find("### Re: complete retry").unwrap()
                < outcome.content.find("agent:boundary:latest").unwrap()
        );
    }

    #[test]
    fn supersession_fails_safe_when_last_committed_response_was_edited() {
        let committed = DOC.replace(
            "<!-- agent:boundary:abc -->",
            "### Re: committed — gpt-5\n\nCommitted.\n<!-- agent:boundary:abc -->",
        );
        let current = committed
            .replace("Committed.", "Operator-edited committed response.")
            .replace(
                "<!-- agent:boundary:abc -->",
                "### Re: pending — gpt-5\n\nPending.\n<!-- agent:boundary:next -->",
            );

        let outcome = supersede_uncommitted_response_tail(
            &current,
            &committed,
            "### Re: latest — gpt-5\n\nLatest.",
        )
        .unwrap();

        assert!(
            outcome
                .content
                .contains("Operator-edited committed response.")
        );
        assert!(outcome.content.contains("Pending."));
        assert!(outcome.content.contains("Latest."));
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
    fn deferred_target_reconcile_anchors_on_first_turn_prompt() {
        let prior_target = DOC.replace(
            "<!-- agent:boundary:abc -->",
            "### Re: stale — gpt-5\n\nStale response.\n<!-- agent:boundary:stale -->",
        );
        let latest_target = DOC.replace(
            "<!-- agent:boundary:abc -->",
            "### Re: latest — gpt-5\n\nComplete response.\n<!-- agent:boundary:latest -->",
        );
        let editor_with_follow_up = prior_target.replace(
            "<!-- agent:boundary:stale -->",
            "❯ operator follow-up\n<!-- agent:boundary:editor -->",
        );

        let reconciled = reconcile_superseded_response_targets(
            &editor_with_follow_up,
            &prior_target,
            &latest_target,
        )
        .unwrap()
        .expect("the shared prompt should anchor first-turn response supersession");

        assert!(reconciled.contains("❯ operator prompt"));
        assert!(reconciled.contains("❯ operator follow-up"));
        assert!(reconciled.contains("### Re: latest — gpt-5"));
        assert!(!reconciled.contains("### Re: stale — gpt-5"));
        assert_eq!(
            reconciled.matches("agent:boundary:").count(),
            1,
            "reconciled document:\n{reconciled}"
        );
    }

    #[test]
    fn response_cell_rejects_embedded_operator_prompt() {
        let response = "### Re: topic — gpt-5\n\nDone.\n\n❯ operator prompt";
        let err = add_response_cell(DOC, response).unwrap_err();
        assert!(err.to_string().contains("only assistant response nodes"));
    }
}
