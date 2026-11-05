//! `#queuetreegutter`: the queue tree as LSP folding ranges and inlay hints.
//!
//! Zed gives extensions no gutter-icon surface, so the queue tree's per-row
//! glyph is an inlay hint at the start of the row (its tooltip is the native
//! one), and each closed subtree (`line + 1 ..= end_line`) is a folding range.
//! Both come from the same native projection the other editors call through
//! `agent_doc_queue_tree_json`; nothing here derives a state.

use agent_doc_queue::queue_tree::{QueueTree, QueueTreeOptions, queue_tree};
use serde_json::{Value, json};

fn tree(text: &str) -> Option<QueueTree> {
    if !text.contains("<!-- agent:queue") {
        return None;
    }
    queue_tree(text, &QueueTreeOptions::default()).ok()
}

/// `textDocument/foldingRange` result: one region per closed node with a subtree.
pub(crate) fn folding_ranges(text: &str) -> Value {
    let Some(tree) = tree(text) else {
        return json!([]);
    };
    Value::Array(
        tree.nodes
            .iter()
            .filter(|node| node.closed && node.end_line > node.line)
            .map(|node| {
                let hidden = node.end_line - node.line;
                json!({
                    "startLine": node.line - 1,
                    "endLine": node.end_line - 1,
                    "kind": "region",
                    "collapsedText": format!(
                        "{} {} … {hidden} {}",
                        node.glyph,
                        node.text,
                        if hidden == 1 { "line" } else { "lines" }
                    ),
                })
            })
            .collect(),
    )
}

/// `textDocument/inlayHint` result: the node glyph before each row whose line
/// lies in the requested 0-based `[start_line, end_line]`.
pub(crate) fn inlay_hints(text: &str, start_line: usize, end_line: usize) -> Value {
    let Some(tree) = tree(text) else {
        return json!([]);
    };
    Value::Array(
        tree.nodes
            .iter()
            .filter(|node| (start_line..=end_line).contains(&(node.line - 1)))
            .map(|node| {
                json!({
                    "position": { "line": node.line - 1, "character": 0 },
                    "label": node.glyph,
                    "tooltip": format!("agent-doc queue: {}", node.tooltip),
                    "paddingRight": true,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "---\nagent_doc_session: s\n---\n",
        "<!-- agent:queue id=\"main\" -->\n",
        "- ### ~~Ship alpha [#q-alpha]~~\n",
        "  - ~~step one~~\n",
        "- ### ☑️ [#subagent:gamma#1] Fix gamma [#q-gamma]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn closed_subtrees_fold_below_their_row() {
        let ranges = folding_ranges(DOC);
        assert_eq!(ranges.as_array().unwrap().len(), 1, "{ranges}");
        assert_eq!(ranges[0]["startLine"], 4);
        assert_eq!(ranges[0]["endLine"], 5);
        assert_eq!(ranges[0]["kind"], "region");
    }

    #[test]
    fn each_row_shows_its_native_glyph_and_tooltip() {
        let hints = inlay_hints(DOC, 0, 100);
        let hints = hints.as_array().unwrap();
        assert_eq!(hints.len(), 3);
        assert_eq!(hints[0]["position"], json!({ "line": 4, "character": 0 }));
        assert_eq!(hints[0]["tooltip"], "agent-doc queue: done");
        assert_eq!(hints[2]["tooltip"], "agent-doc queue: needs integration");
        assert_eq!(inlay_hints(DOC, 5, 5).as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_document_without_a_queue_answers_nothing() {
        assert_eq!(folding_ranges("# notes\n"), json!([]));
        assert_eq!(inlay_hints("# notes\n", 0, 10), json!([]));
    }
}
