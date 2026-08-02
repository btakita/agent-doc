//! Atomic assistant-response cells for the realtime document backbone.
//!
//! A response is one body-aware cell containing one or more assistant exchange
//! nodes. The operation is intentionally smaller than a template patch or
//! whole-document replacement: replaying the same cell is a no-op, while a
//! response with the same heading and a different body remains distinct.

use agent_doc_element::element;
use agent_doc_markdown_ast::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, ResponseTurnCellPolicy, parse_exchange_nodes,
    remove_all_salient_responses, render_exchange_nodes,
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

/// `#crdtcaschkpt` — how many leading nodes of `incoming` are already the
/// trailing run of `current`.
///
/// Anchored at the tail so only a response this cycle just published
/// incrementally can be treated as an already-applied prefix. Returns 0 when
/// nothing lines up, which keeps the ordinary whole-response append unchanged.
fn trailing_prefix_len(current: &[String], incoming: &[String]) -> usize {
    let max = current.len().min(incoming.len());
    // Longest first: with repeated identical sections, the longest overlap is the
    // one that leaves no duplicate behind.
    (1..=max)
        .rev()
        .find(|&k| current[current.len() - k..] == incoming[..k])
        .unwrap_or(0)
}

/// Byte-stable rendering of a node run, matching [`parse_response_cell`]'s
/// single trailing newline so an appended suffix is indistinguishable from the
/// same sections written in one pass.
fn render_response_nodes(nodes: &[ExchangeNode]) -> String {
    let joined = nodes.iter().map(ExchangeNode::render).collect::<String>();
    format!("{}\n", joined.trim_end())
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
    let response_policy = ResponseTurnCellPolicy::from_response_nodes(&nodes);
    let cell_id = response_policy.cell_id().to_string();
    let components = element::parse(doc)?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    let current = exchange.content(doc);
    let current_without_salient = remove_all_salient_responses(current);
    let cleaned_doc = if current_without_salient == current {
        doc.to_string()
    } else {
        exchange.replace_content(doc, &current_without_salient)
    };
    let cleaned_components = element::parse(&cleaned_doc)?;
    let cleaned_exchange = cleaned_components
        .iter()
        .find(|component| component.name == "exchange")
        .ok_or_else(|| anyhow::anyhow!("document has no agent:exchange component"))?;
    let current = cleaned_exchange.content(&cleaned_doc);
    let removed_salient = cleaned_doc != doc;
    let node_ids = response_policy.node_ids();

    let current_node_ids = parse_exchange_nodes(current)
        .iter()
        .map(ExchangeNode::node_id)
        .collect::<Vec<_>>();
    if !node_ids.is_empty()
        && current_node_ids.len() >= node_ids.len()
        && current_node_ids
            .windows(node_ids.len())
            .any(|window| window == node_ids)
    {
        return Ok(ResponseCellAddOutcome {
            content: cleaned_doc,
            cell_id,
            applied: removed_salient,
        });
    }

    // `#crdtcaschkpt`: the cell may extend a response this cycle already
    // published incrementally via `agent-doc response-checkpoint`. The whole
    // point of checkpointing is that finalize then has little left to converge,
    // but the exact-sequence replay check above only recognizes the response as a
    // whole: a checkpoint of sections 1-2 followed by a finalize of sections
    // 1-2-3 matched nothing and appended all three, duplicating 1 and 2 in the
    // operator's document. Append only the suffix that is not already the tail of
    // the exchange.
    //
    // Anchored at the TAIL deliberately. The same heading with a different body
    // is a genuinely distinct node (different node id), so an unanchored search
    // could treat an older, unrelated turn as this response's prefix and silently
    // drop sections.
    let already = trailing_prefix_len(&current_node_ids, node_ids);
    let pending = &nodes[already..];
    if pending.is_empty() {
        return Ok(ResponseCellAddOutcome {
            content: cleaned_doc,
            cell_id,
            applied: removed_salient,
        });
    }
    let rendered = if already == 0 {
        rendered
    } else {
        render_response_nodes(pending)
    };

    let mut next = current.trim_end_matches('\n').to_string();
    if !next.trim().is_empty() {
        next.push_str("\n\n");
    }
    next.push_str(&rendered);

    Ok(ResponseCellAddOutcome {
        content: cleaned_exchange.replace_content(&cleaned_doc, &next),
        cell_id,
        applied: true,
    })
}

fn boundary_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
}

#[derive(Debug, Default)]
struct MarkdownFence {
    delimiter: Option<(char, usize)>,
}

impl MarkdownFence {
    /// Return whether `line` is a protocol boundary while tracking fenced code.
    /// Boundary-looking examples inside code blocks are document content, not
    /// recovery metadata, and must never be removed by replay normalization.
    fn is_protocol_boundary(&mut self, line: &str) -> bool {
        let outside_fence = self.delimiter.is_none();
        let trimmed = line.trim_start();
        let delimiter = trimmed.chars().next().and_then(|character| {
            matches!(character, '`' | '~').then(|| {
                let count = trimmed
                    .chars()
                    .take_while(|candidate| *candidate == character)
                    .count();
                (character, count)
            })
        });

        if let Some((character, count)) = delimiter.filter(|(_, count)| *count >= 3) {
            match self.delimiter {
                None => self.delimiter = Some((character, count)),
                Some((open_character, open_count))
                    if character == open_character
                        && count >= open_count
                        && trimmed.chars().skip(count).all(char::is_whitespace) =>
                {
                    self.delimiter = None;
                }
                Some(_) => {}
            }
        }

        outside_fence && boundary_line(line)
    }
}

fn take_protocol_boundaries(
    lines: Vec<String>,
    fence: &mut MarkdownFence,
) -> (Vec<String>, Vec<String>) {
    let mut retained = Vec::with_capacity(lines.len());
    let mut boundaries = Vec::new();
    for line in lines {
        if fence.is_protocol_boundary(&line) {
            boundaries.push(line.trim().to_string());
        } else {
            retained.push(line);
        }
    }
    (retained, boundaries)
}

/// Remove repeated response nodes with the same body-aware identity.
///
/// Response cells are idempotent: replaying the same heading and body is a
/// no-op. Component-level three-way merges can nevertheless materialize both
/// sides when an editor reconnects during a force-disk projection. Preserve
/// the first semantic response occurrence, every prompt, and the newest
/// boundary marker.
pub fn deduplicate_response_cells(doc: &str) -> anyhow::Result<Option<String>> {
    let components = element::parse(doc)?;
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(None);
    };
    let nodes = parse_exchange_nodes(exchange.content(doc));
    let mut response_ids = HashSet::new();
    let mut boundary = None;
    let mut removed = false;
    let mut retained = Vec::with_capacity(nodes.len());
    let mut fence = MarkdownFence::default();
    for mut node in nodes {
        let (lines, boundaries) = take_protocol_boundaries(node.lines, &mut fence);
        node.lines = lines;
        for candidate in boundaries {
            if boundary.is_some() {
                removed = true;
            }
            boundary = Some(candidate);
        }
        if matches!(node.kind, ExchangeNodeKind::Response { .. })
            && !response_ids.insert(node.node_id())
        {
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

    let mut inner = render_exchange_nodes(&retained)
        .trim_end_matches('\n')
        .to_string();
    if let Some(boundary) = boundary {
        if !inner.is_empty() {
            inner.push('\n');
        }
        inner.push_str(&boundary);
        inner.push('\n');
    }
    Ok(Some(exchange.replace_content(doc, &inner)))
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
    let mut fence = MarkdownFence::default();
    for (index, mut node) in current_nodes.into_iter().enumerate() {
        let (lines, boundaries) = take_protocol_boundaries(node.lines, &mut fence);
        node.lines = lines;
        for candidate in boundaries {
            boundary = Some(candidate);
        }
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
    // A retry can classify the already-present response as the uncommitted tail,
    // remove it, and then reconstruct the exact same document. Treat that as a
    // semantic no-op so callers do not emit another CRDT update/ACK cycle.
    outcome.applied = outcome.content != doc;
    Ok(Some(outcome))
}

/// Replace response nodes appended after the last unchanged committed exchange
/// node, then add the latest complete response as one semantic cell.
///
/// This is the normal closeout recovery path when an older retained response is
/// restored after the next complete response has already been captured. Prompt
/// nodes are never removed. If the last committed response cannot be found
/// unchanged in the current document, the operation fails safe to additive
/// behavior so operator edits to committed history are preserved. Anchoring on
/// any committed exchange node (rather than requiring an older response) also
/// makes the first response in a document replaceable by a later cumulative
/// semantic checkpoint.
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
    let Some(last_committed_node_id) = committed_nodes.last().map(ExchangeNode::node_id) else {
        return add_response_cell(doc, response);
    };

    if let Some(outcome) = supersede_response_tail_after_anchor(
        doc,
        &last_committed_node_id,
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
    let mut replacement_nodes = Vec::new();
    let mut fence = MarkdownFence::default();
    for mut node in latest_nodes.into_iter().skip(latest_anchor_index + 1) {
        let (lines, _) = take_protocol_boundaries(node.lines, &mut fence);
        node.lines = lines;
        if matches!(node.kind, ExchangeNodeKind::Response { .. })
            && !prior_response_ids.contains(&node.node_id())
            && !node.lines.is_empty()
        {
            replacement_nodes.push(node);
        }
    }
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

    /// `#crdtcaschkpt` — a response published incrementally by
    /// `agent-doc response-checkpoint` must not be duplicated when finalize
    /// later writes the full response.
    ///
    /// This is the property that makes checkpointing usable at all, and it did
    /// not hold: the replay guard only recognized the response as a whole, so a
    /// checkpoint of sections 1-2 followed by a finalize of 1-2-3 matched nothing
    /// and appended all three, leaving 1 and 2 twice in the operator's document.
    #[test]
    fn finalize_after_incremental_checkpoint_appends_only_the_new_sections() {
        let partial = "### Re: one — gpt-5\n\nFirst.";
        let full = "### Re: one — gpt-5\n\nFirst.\n\n### Re: two — gpt-5\n\nSecond.";

        let checkpoint = add_response_cell(DOC, partial).unwrap();
        assert!(checkpoint.applied);

        let finalized = add_response_cell(&checkpoint.content, full).unwrap();
        assert!(finalized.applied, "the new section must still be appended");
        assert_eq!(
            finalized.content.matches("### Re: one").count(),
            1,
            "the checkpointed section must not be duplicated"
        );
        assert_eq!(finalized.content.matches("### Re: two").count(), 1);
        assert_eq!(finalized.content.matches("First.").count(), 1);

        // Byte-identical to writing the whole response in one pass.
        let one_pass = add_response_cell(DOC, full).unwrap();
        assert_eq!(finalized.content, one_pass.content);
    }

    /// Checkpointing every section leaves finalize with nothing to converge —
    /// the point of the whole exercise.
    #[test]
    fn finalize_after_complete_checkpointing_is_a_no_op() {
        let full = "### Re: one — gpt-5\n\nFirst.\n\n### Re: two — gpt-5\n\nSecond.";
        let checkpointed = add_response_cell(DOC, full).unwrap();
        assert!(checkpointed.applied);

        let finalized = add_response_cell(&checkpointed.content, full).unwrap();
        assert!(!finalized.applied);
        assert_eq!(finalized.content, checkpointed.content);
    }

    #[test]
    fn finalize_replaces_salient_node_and_replay_cleans_stale_salient() {
        let live =
            crate::salient_response::upsert_salient_response_node(DOC, "cycle-1", "Diagnosis.")
                .unwrap();
        let response = "### Re: operator prompt — gpt-5\n\nFinal answer.";
        let finalized = add_response_cell(&live.content, response).unwrap();
        assert!(finalized.applied);
        assert!(!finalized.content.contains("agent:salient-response"));
        assert!(!finalized.content.contains("Diagnosis."));
        assert_eq!(finalized.content.matches("Final answer.").count(), 1);

        let stale = crate::salient_response::upsert_salient_response_node(
            &finalized.content,
            "cycle-1",
            "Stale replay.",
        )
        .unwrap();
        let replay = add_response_cell(&stale.content, response).unwrap();
        assert!(replay.applied, "stale progress cleanup is a real mutation");
        assert!(!replay.content.contains("agent:salient-response"));
        assert_eq!(replay.content.matches("Final answer.").count(), 1);
    }

    /// The prefix match is anchored at the tail, so an older unrelated turn that
    /// happens to share a leading section is never mistaken for this cycle's
    /// checkpoint — that would silently DROP sections instead of duplicating
    /// them, which is the worse failure.
    #[test]
    fn an_earlier_matching_section_is_not_treated_as_this_cycles_prefix() {
        let earlier = add_response_cell(DOC, "### Re: one — gpt-5\n\nFirst.").unwrap();
        let intervening =
            add_response_cell(&earlier.content, "### Re: other — gpt-5\n\nUnrelated.").unwrap();
        assert!(intervening.applied);

        let full = "### Re: one — gpt-5\n\nFirst.\n\n### Re: two — gpt-5\n\nSecond.";
        let finalized = add_response_cell(&intervening.content, full).unwrap();
        assert!(finalized.applied);
        assert_eq!(
            finalized.content.matches("### Re: two").count(),
            1,
            "the new section must be present exactly once"
        );
        assert!(
            finalized.content.contains("### Re: other"),
            "the intervening turn must survive"
        );
    }

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
    fn duplicate_response_cells_from_reconnect_merge_are_collapsed() {
        let duplicated = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ original prompt\n\n",
            "### Re: topic — gpt-5\n\nDone.\n\n",
            "❯ prompt retained from the live editor\n\n",
            "### Re: topic — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:next -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let deduplicated = deduplicate_response_cells(duplicated)
            .unwrap()
            .expect("the repeated semantic response should be removed");

        assert_eq!(deduplicated.matches("### Re: topic").count(), 1);
        assert_eq!(deduplicated.matches("Done.").count(), 1);
        assert!(deduplicated.contains("❯ original prompt"));
        assert!(deduplicated.contains("❯ prompt retained from the live editor"));
        assert_eq!(deduplicated.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn duplicate_boundaries_without_duplicate_responses_are_collapsed() {
        let duplicated = DOC.replace(
            "<!-- agent:boundary:abc -->",
            concat!(
                "<!-- agent:boundary:stale -->\n",
                "### Re: retained — gpt-5\n\nRetained response.\n",
                "<!-- agent:boundary:latest -->",
            ),
        );

        let deduplicated = deduplicate_response_cells(&duplicated)
            .unwrap()
            .expect("duplicate protocol boundaries should be normalized");

        assert_eq!(deduplicated.matches("agent:boundary:").count(), 1);
        assert!(deduplicated.contains("agent:boundary:latest"));
        assert_eq!(deduplicated.matches("Retained response.").count(), 1);
        assert!(deduplicated.contains("❯ operator prompt"));
    }

    #[test]
    fn boundary_examples_inside_fenced_code_are_not_recovery_metadata() {
        let document = DOC.replace(
            "<!-- agent:boundary:abc -->",
            concat!(
                "### Re: example — gpt-5\n\n",
                "```markdown\n",
                "<!-- agent:boundary:example -->\n",
                "```\n",
                "<!-- agent:boundary:real -->",
            ),
        );

        assert_eq!(deduplicate_response_cells(&document).unwrap(), None);

        let duplicated = document.replace(
            "<!-- agent:boundary:real -->",
            "<!-- agent:boundary:stale -->\n<!-- agent:boundary:real -->",
        );
        let normalized = deduplicate_response_cells(&duplicated)
            .unwrap()
            .expect("the duplicate protocol boundary should be normalized");
        assert!(normalized.contains("<!-- agent:boundary:example -->"));
        assert!(normalized.contains("<!-- agent:boundary:real -->"));
        assert!(!normalized.contains("<!-- agent:boundary:stale -->"));
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

        let replay =
            supersede_uncommitted_response_tail(&outcome.content, committed, latest).unwrap();
        assert!(!replay.applied);
        assert_eq!(replay.cell_id, outcome.cell_id);
        assert_eq!(replay.content, outcome.content);
        assert_eq!(replay.content.matches("Complete response.").count(), 1);
        assert_eq!(replay.content.matches("agent:boundary:").count(), 1);
    }

    #[test]
    fn cumulative_checkpoint_supersedes_first_uncommitted_response() {
        let first = supersede_uncommitted_response_tail(
            DOC,
            DOC,
            "### Re: checkpoint — gpt-5\n\nFirst complete section.",
        )
        .unwrap();
        assert!(first.applied);

        let latest = supersede_uncommitted_response_tail(
            &first.content,
            DOC,
            concat!(
                "### Re: checkpoint — gpt-5\n\nFirst complete section.\n\n",
                "### Re: follow-up — gpt-5\n\nSecond complete section.",
            ),
        )
        .unwrap();

        assert!(latest.applied);
        assert_eq!(latest.content.matches("First complete section.").count(), 1);
        assert_eq!(
            latest.content.matches("Second complete section.").count(),
            1
        );
        assert_eq!(latest.content.matches("### Re:").count(), 2);
        assert!(latest.content.contains("❯ operator prompt"));
        assert_eq!(latest.content.matches("agent:boundary:").count(), 1);
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
