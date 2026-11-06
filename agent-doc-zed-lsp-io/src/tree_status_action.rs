//! `#treestatuszed`: the "Agent Doc: Tree Status" editor action as LSP code
//! actions.
//!
//! Zed gives extensions no action or menu surface, so the shared editor
//! action (`editors/SPEC.md`, `#treestatusquery`) is a code action on the
//! caret's queue subtree. Its command shows the native tree-status `text`
//! verbatim through `window/showMessage`, and each `deletable` row is its own
//! quick fix whose `WorkspaceEdit` deletes lines `line..=end_line`. The answer
//! comes from `agent_doc_queue::tree_status`, the same projection the CLI and
//! the `agent_doc_queue_tree_status_json` export use; nothing here derives or
//! words a state.

use agent_doc_queue::tree_status::{TreeStatus, TreeStatusRow, render_text, tree_status};
use serde_json::{Value, json};

/// The `workspace/executeCommand` command that shows a tree-status answer.
pub(crate) const SHOW_TREE_STATUS_COMMAND: &str = "agent-doc.treeStatus";

/// Code action kind of the action that shows the answer.
const TREE_STATUS_KIND: &str = "source.agent-doc.treeStatus";
/// Code action kind of each deletable-subtree quick fix.
const DELETE_SUBTREE_KIND: &str = "quickfix";

/// LSP `CodeActionTriggerKind.Automatic`: the editor asked on its own (for
/// example a lightbulb or an on-save pass), not because the operator opened
/// the code action menu.
const TRIGGER_AUTOMATIC: u64 = 2;

/// `textDocument/codeAction` result for an agent-doc-mode buffer `text` with
/// caret at 0-based `caret_line`. `only` is the request's
/// `context.only` filter and `trigger_kind` its `context.triggerKind`.
///
/// Answers `[]` when the line is outside every queue component. The deletions
/// are offered only when the operator opened the menu: an automatic request
/// (an on-save code action pass) never receives an edit that deletes queue
/// text.
pub(crate) fn code_actions(
    uri: &str,
    text: &str,
    caret_line: usize,
    only: Option<&[String]>,
    trigger_kind: Option<u64>,
) -> Value {
    if !text.contains("<!-- agent:queue") {
        return json!([]);
    }
    let Ok(status) = tree_status(text, caret_line + 1) else {
        return json!([]);
    };
    let wanted = |kind: &str| {
        only.is_none_or(|only| {
            only.iter()
                .any(|requested| kind == requested || kind.starts_with(&format!("{requested}.")))
        })
    };
    let mut actions = Vec::new();
    if wanted(TREE_STATUS_KIND) {
        actions.push(show_action(&status));
    }
    if trigger_kind != Some(TRIGGER_AUTOMATIC) && wanted(DELETE_SUBTREE_KIND) {
        let line_count = text.split('\n').count();
        actions.extend(
            status
                .deletable
                .iter()
                .map(|row| delete_action(uri, text, line_count, row)),
        );
    }
    Value::Array(actions)
}

fn show_action(status: &TreeStatus) -> Value {
    let text = render_text(status);
    json!({
        "title": format!("Agent Doc: Tree Status ({})", status.counts_line()),
        "kind": TREE_STATUS_KIND,
        "command": {
            "title": "Agent Doc: Tree Status",
            "command": SHOW_TREE_STATUS_COMMAND,
            "arguments": [text],
        },
    })
}

fn delete_action(uri: &str, text: &str, line_count: usize, row: &TreeStatusRow) -> Value {
    let lines = if row.end_line > row.line {
        format!("L{}–L{}", row.line, row.end_line)
    } else {
        format!("L{}", row.line)
    };
    json!({
        "title": format!("Agent Doc: Delete {lines} {} {}", row.glyph, row.text),
        "kind": DELETE_SUBTREE_KIND,
        "edit": {
            "changes": {
                uri: [{
                    "range": delete_range(text, line_count, row.line, row.end_line),
                    "newText": "",
                }],
            },
        },
    })
}

/// The LSP range covering 1-based lines `line..=end_line` with their line
/// breaks. When `end_line` is the document's last line, the range starts at
/// the end of the line before it instead, so no line break is left behind.
fn delete_range(text: &str, line_count: usize, line: usize, end_line: usize) -> Value {
    if end_line < line_count {
        return json!({
            "start": { "line": line - 1, "character": 0 },
            "end": { "line": end_line, "character": 0 },
        });
    }
    let last = text.split('\n').next_back().unwrap_or_default();
    let end = json!({ "line": line_count - 1, "character": utf16_len(last) });
    if line <= 1 {
        return json!({ "start": { "line": 0, "character": 0 }, "end": end });
    }
    let before = text.split('\n').nth(line - 2).unwrap_or_default();
    json!({
        "start": { "line": line - 2, "character": utf16_len(before) },
        "end": end,
    })
}

fn utf16_len(line: &str) -> usize {
    line.trim_end_matches('\r').encode_utf16().count()
}

/// The `window/showMessage` notification for a `SHOW_TREE_STATUS_COMMAND`
/// call, or None when its argument is not the answer text.
pub(crate) fn show_message(arguments: Option<&Value>) -> Option<Value> {
    let text = arguments?.get(0)?.as_str()?;
    Some(json!({
        "jsonrpc": "2.0",
        "method": "window/showMessage",
        "params": { "type": 3, "message": text },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const URI: &str = "file:///s.md";
    const DOC: &str = concat!(
        "---\nagent_doc_session: s\n---\n",
        "<!-- agent:queue id=\"main\" -->\n",
        "- ~~do [#alpha]~~\n",
        "  - ~~step one~~\n",
        "- 🚧 [#subagent:beta#1] do [#beta]\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn the_caret_subtree_shows_the_native_text_and_deletes_each_deletable_subtree() {
        let actions = code_actions(URI, DOC, 3, None, Some(1));
        let actions = actions.as_array().unwrap();
        assert_eq!(actions.len(), 2, "{actions:?}");

        let status = tree_status(DOC, 4).unwrap();
        let show = &actions[0];
        assert_eq!(show["kind"], TREE_STATUS_KIND);
        assert_eq!(
            show["title"],
            format!("Agent Doc: Tree Status ({})", status.counts_line())
        );
        assert_eq!(show["command"]["command"], SHOW_TREE_STATUS_COMMAND);
        assert_eq!(show["command"]["arguments"][0], render_text(&status));

        let delete = &actions[1];
        assert_eq!(delete["kind"], "quickfix");
        assert!(
            delete["title"]
                .as_str()
                .unwrap()
                .starts_with("Agent Doc: Delete L5–L6 "),
            "{delete}"
        );
        assert_eq!(
            delete["edit"]["changes"][URI],
            json!([{
                "range": {
                    "start": { "line": 4, "character": 0 },
                    "end": { "line": 6, "character": 0 },
                },
                "newText": "",
            }])
        );
    }

    #[test]
    fn applying_the_delete_edit_removes_exactly_the_subtree_lines() {
        let actions = code_actions(URI, DOC, 3, None, None);
        let range = &actions[1]["edit"]["changes"][URI][0]["range"];
        let start = range["start"]["line"].as_u64().unwrap() as usize;
        let end = range["end"]["line"].as_u64().unwrap() as usize;
        let lines = DOC.split_inclusive('\n').collect::<Vec<_>>();
        let after = [&lines[..start], &lines[end..]].concat().concat();
        assert_eq!(
            after,
            concat!(
                "---\nagent_doc_session: s\n---\n",
                "<!-- agent:queue id=\"main\" -->\n",
                "- 🚧 [#subagent:beta#1] do [#beta]\n",
                "<!-- /agent:queue -->\n",
            )
        );
    }

    #[test]
    fn a_subtree_ending_on_the_last_line_takes_the_preceding_line_break() {
        let text = "a\nbé\nc";
        assert_eq!(
            delete_range(text, 3, 3, 3),
            json!({
                "start": { "line": 1, "character": 2 },
                "end": { "line": 2, "character": 1 },
            })
        );
        assert_eq!(
            delete_range(text, 3, 1, 3),
            json!({
                "start": { "line": 0, "character": 0 },
                "end": { "line": 2, "character": 1 },
            })
        );
    }

    #[test]
    fn an_automatic_request_never_receives_a_deletion() {
        let actions = code_actions(URI, DOC, 3, None, Some(TRIGGER_AUTOMATIC));
        let actions = actions.as_array().unwrap();
        assert_eq!(actions.len(), 1, "{actions:?}");
        assert_eq!(actions[0]["kind"], TREE_STATUS_KIND);
    }

    #[test]
    fn the_only_filter_selects_by_kind_prefix() {
        let kinds = |only: &[&str]| {
            let only = only.iter().map(|kind| kind.to_string()).collect::<Vec<_>>();
            code_actions(URI, DOC, 3, Some(&only), Some(1))
                .as_array()
                .unwrap()
                .iter()
                .map(|action| action["kind"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(&["quickfix"]), ["quickfix"]);
        assert_eq!(kinds(&["source"]), [TREE_STATUS_KIND]);
        assert!(kinds(&["source.organizeImports"]).is_empty());
    }

    #[test]
    fn a_line_outside_every_queue_answers_nothing() {
        assert_eq!(code_actions(URI, DOC, 0, None, None), json!([]));
        assert_eq!(code_actions(URI, "# notes\n", 0, None, None), json!([]));
    }

    #[test]
    fn the_show_command_displays_the_answer_text_verbatim() {
        let text = render_text(&tree_status(DOC, 4).unwrap());
        let message = show_message(Some(&json!([text]))).unwrap();
        assert_eq!(message["method"], "window/showMessage");
        assert_eq!(message["params"]["type"], 3);
        assert_eq!(message["params"]["message"], text);
        assert!(show_message(Some(&json!([]))).is_none());
        assert!(show_message(None).is_none());
    }
}
