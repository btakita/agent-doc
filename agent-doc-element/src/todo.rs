//! Pure `agent:todo` component helpers (`#operatortodo`).
//!
//! `agent:todo` is the operator-owned action checklist: concrete actions the
//! operator, not the agent, must perform. Agents add brief checklist items to
//! it; the operator checks them off. Session documents often predate the
//! component, so every agent-side todo write must be able to create it.
//!
//! ## Spec
//! - [`ensure_todo_component_in_document`] returns the document unchanged when
//!   an `agent:todo` component exists. Otherwise it inserts an empty
//!   `## Todo` section at one deterministic location, next to the other
//!   operator-facing tracked-work components: directly after the last
//!   `agent:review` component; else after the last `agent:backlog` /
//!   legacy `agent:pending`; else after the last `agent:queue`; else after the
//!   last `agent:exchange`; else at the end of the document.
//! - [`add_todo_item_to_document`] appends one `- [ ] <action>` line to the end
//!   of the component body, creating the component first when absent. It never
//!   removes, reorders, or rewrites an existing line, so every existing
//!   checklist item and every operator edit survives byte-for-byte.
//! - Adding is idempotent: when any existing checklist item (open, done, or
//!   gated) already carries the same action text, the add is a no-op. A replayed
//!   closeout therefore never duplicates a todo, and never resurrects one the
//!   operator already checked off.
//! - [`todo_unreferenced_ids`] reports which tracked-work ids the todo
//!   component does not reference as `#id`, for the closeout advisory.
//!
//! ## Agentic Contracts
//! - All functions are pure: no file IO, no clock, no randomness.
//! - Placement depends only on the component layout, so the same document
//!   always yields the same insertion point.
//! - An action must be one non-empty line; multi-line text is rejected rather
//!   than silently split into several items.

use anyhow::{Context, Result};

use crate::element::{self, Component};

/// Canonical component name for the operator action checklist.
pub const TODO_COMPONENT: &str = "todo";

/// Section inserted when a document has no `agent:todo` component. It matches
/// the heading and markers of the default template scaffold.
pub const TODO_SECTION: &str = "\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n";

/// Check whether a component name refers to the operator todo checklist.
pub fn is_todo_component(name: &str) -> bool {
    name == TODO_COMPONENT
}

/// Return the action text of one Markdown checklist line (`- [ ] text`,
/// `* [x] text`, `1. [/] text`, ...), or `None` for any other line.
pub fn checklist_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| {
            let digit_run = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_run > 0 {
                trimmed[digit_run..].strip_prefix(". ")
            } else {
                None
            }
        })?;
    rest.strip_prefix("[ ] ")
        .or_else(|| rest.strip_prefix("[x] "))
        .or_else(|| rest.strip_prefix("[/] "))
}

/// Count the Markdown checklist items in a todo body.
pub fn count_checklist_items(body: &str) -> usize {
    body.lines()
        .filter(|line| checklist_item_text(line).is_some())
        .count()
}

/// Find the `agent:todo` component in already-read document content.
pub fn find_todo_component(content: &str) -> Result<Option<Component>> {
    let components = element::parse(content).context("failed to parse components")?;
    Ok(components
        .into_iter()
        .find(|component| is_todo_component(&component.name)))
}

/// Byte offset where a missing `agent:todo` section is inserted.
fn todo_insertion_offset(content: &str, components: &[Component]) -> usize {
    let anchors: [fn(&str) -> bool; 4] = [
        element::is_review_component,
        element::is_backlog_component,
        |name| name == "queue",
        |name| name == "exchange",
    ];
    anchors
        .iter()
        .find_map(|is_anchor| {
            components
                .iter()
                .rfind(|component| is_anchor(&component.name))
                .map(|component| component.close_end)
        })
        .unwrap_or(content.len())
}

/// Insert an empty `agent:todo` section when the document has none.
///
/// Returns the possibly-updated content, the todo component span in that
/// content, and whether the section was created.
pub fn ensure_todo_component_in_document(content: &str) -> Result<(String, Component, bool)> {
    let components = element::parse(content).context("failed to parse components")?;
    if let Some(todo) = components
        .iter()
        .find(|component| is_todo_component(&component.name))
    {
        return Ok((content.to_string(), todo.clone(), false));
    }
    let offset = todo_insertion_offset(content, &components);
    let mut out = String::with_capacity(content.len() + TODO_SECTION.len() + 1);
    out.push_str(&content[..offset]);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(TODO_SECTION);
    out.push_str(&content[offset..]);
    let todo =
        find_todo_component(&out)?.context("document has no todo component after insertion")?;
    Ok((out, todo, true))
}

/// Result of [`add_todo_item_to_document`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoAddOutcome {
    /// The document after the add (unchanged when nothing was inserted).
    pub content: String,
    /// Whether a new checklist line was appended.
    pub inserted: bool,
    /// Whether the `agent:todo` component had to be created.
    pub created_component: bool,
}

/// Normalize one `--todo-add` action into its checklist text.
///
/// A caller may pass a bare action or an already-formatted `- [ ] action`
/// line; both yield the same action text.
pub fn normalize_todo_action(text: &str) -> Result<String> {
    let trimmed = text.trim();
    anyhow::ensure!(
        !trimmed.contains('\n'),
        "--todo-add expects one single-line operator action, got multi-line text"
    );
    let action = checklist_item_text(trimmed)
        .or_else(|| trimmed.strip_prefix("- "))
        .unwrap_or(trimmed)
        .trim();
    // A bare checkbox (`- [ ]`, `[x]`) carries no action.
    let action = match action {
        "[ ]" | "[x]" | "[/]" => "",
        other => other,
    };
    anyhow::ensure!(
        !action.is_empty(),
        "--todo-add expects a non-empty operator action"
    );
    Ok(action.to_string())
}

/// Append one operator action to `agent:todo`, creating the component when
/// absent and preserving every existing line.
pub fn add_todo_item_to_document(content: &str, text: &str) -> Result<TodoAddOutcome> {
    let action = normalize_todo_action(text)?;
    let (ensured, todo, created_component) = ensure_todo_component_in_document(content)?;
    let body = todo.content(&ensured);
    let already_present = body
        .lines()
        .filter_map(checklist_item_text)
        .any(|existing| existing.trim() == action);
    if already_present {
        return Ok(TodoAddOutcome {
            content: ensured,
            inserted: false,
            created_component,
        });
    }
    let mut new_body = String::with_capacity(body.len() + action.len() + 8);
    new_body.push_str(body);
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    new_body.push_str("- [ ] ");
    new_body.push_str(&action);
    new_body.push('\n');
    Ok(TodoAddOutcome {
        content: todo.replace_content(&ensured, &new_body),
        inserted: true,
        created_component,
    })
}

/// Whether `body` mentions `#id` as a whole id token.
fn body_references_id(body: &str, id: &str) -> bool {
    let needle = format!("#{id}");
    body.match_indices(&needle).any(|(start, _)| {
        body[start + needle.len()..]
            .chars()
            .next()
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '-' || next == '_'))
    })
}

/// Return the ids (normalized, without `#`) that the document's `agent:todo`
/// component does not reference as `#id`. Every id is unreferenced when the
/// component is absent or the document does not parse.
pub fn todo_unreferenced_ids(content: &str, ids: &[String]) -> Vec<String> {
    let body = find_todo_component(content)
        .ok()
        .flatten()
        .map(|todo| todo.content(content).to_string())
        .unwrap_or_default();
    let mut missing: Vec<String> = Vec::new();
    for id in ids {
        let id = id.trim().trim_start_matches('#');
        if id.is_empty() || missing.iter().any(|seen| seen == id) {
            continue;
        }
        if !body_references_id(&body, id) {
            missing.push(id.to_string());
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo_body(content: &str) -> String {
        find_todo_component(content)
            .unwrap()
            .expect("todo component")
            .content(content)
            .to_string()
    }

    #[test]
    fn ensure_inserts_todo_after_review_with_scaffold_heading() {
        let content = concat!(
            "<!-- agent:exchange -->\n### Re: prior\n<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n<!-- agent:backlog -->\n- [ ] [#work] do work\n<!-- /agent:backlog -->\n\n",
            "## Review\n\n<!-- agent:review -->\n- [/] [#gate] gated\n<!-- /agent:review -->\n\n",
            "## Done\n\n<!-- agent:done -->\n<!-- /agent:done -->\n",
        );
        let (updated, todo, created) = ensure_todo_component_in_document(content).unwrap();
        assert!(created);
        assert_eq!(todo.content(&updated), "");
        let review_end = updated.find("<!-- /agent:review -->").unwrap();
        let todo_start = updated.find("<!-- agent:todo -->").unwrap();
        let done_start = updated.find("<!-- agent:done -->").unwrap();
        assert!(
            review_end < todo_start && todo_start < done_start,
            "{updated}"
        );
        assert!(
            updated.contains("<!-- /agent:review -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\n\n## Done"),
            "{updated}"
        );
        // Everything outside the inserted section is byte-identical.
        assert_eq!(updated.replacen(TODO_SECTION, "", 1), content);
    }

    #[test]
    fn ensure_falls_back_to_backlog_then_queue_then_exchange_then_end() {
        let backlog = "<!-- agent:queue -->\n<!-- /agent:queue -->\n<!-- agent:pending -->\n- [ ] a\n<!-- /agent:pending -->\n<!-- agent:notes -->\n<!-- /agent:notes -->\n";
        let (updated, _, _) = ensure_todo_component_in_document(backlog).unwrap();
        assert!(
            updated.contains("<!-- /agent:pending -->\n\n## Todo\n\n<!-- agent:todo -->"),
            "{updated}"
        );

        let queue = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n<!-- agent:queue -->\n- do x\n<!-- /agent:queue -->\ntrailing prose\n";
        let (updated, _, _) = ensure_todo_component_in_document(queue).unwrap();
        assert!(
            updated.contains("<!-- /agent:queue -->\n\n## Todo\n\n<!-- agent:todo -->\n<!-- /agent:todo -->\ntrailing prose\n"),
            "{updated}"
        );

        let exchange = "# Title\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->";
        let (updated, _, _) = ensure_todo_component_in_document(exchange).unwrap();
        assert_eq!(
            updated,
            format!(
                "# Title\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n{TODO_SECTION}"
            )
        );

        let plain = "# Notes only\n\nprose";
        let (updated, _, _) = ensure_todo_component_in_document(plain).unwrap();
        assert_eq!(updated, format!("# Notes only\n\nprose\n{TODO_SECTION}"));
    }

    #[test]
    fn ensure_is_a_noop_when_todo_exists() {
        let content = "<!-- agent:review -->\n<!-- /agent:review -->\n<!-- agent:todo -->\n- [ ] keep\n<!-- /agent:todo -->\n";
        let (updated, todo, created) = ensure_todo_component_in_document(content).unwrap();
        assert!(!created);
        assert_eq!(updated, content);
        assert_eq!(todo.content(&updated), "- [ ] keep\n");
    }

    #[test]
    fn add_creates_component_and_appends_item() {
        let content = "<!-- agent:review -->\n- [/] [#gate] gated\n<!-- /agent:review -->\n";
        let outcome =
            add_todo_item_to_document(content, "Publish draft docs/launch-note.md").unwrap();
        assert!(outcome.inserted && outcome.created_component);
        assert_eq!(
            todo_body(&outcome.content),
            "- [ ] Publish draft docs/launch-note.md\n"
        );
    }

    #[test]
    fn add_preserves_every_existing_item_and_operator_text() {
        let content = concat!(
            "<!-- agent:todo -->\n",
            "Operator note: do these after lunch\n",
            "- [x] Merged the release PR\n",
            "- [ ] Rotate the deploy key\n",
            "<!-- /agent:todo -->\n",
        );
        let outcome = add_todo_item_to_document(content, "Verify [#gate] in a live pane").unwrap();
        assert!(outcome.inserted);
        assert!(!outcome.created_component);
        assert_eq!(
            todo_body(&outcome.content),
            concat!(
                "Operator note: do these after lunch\n",
                "- [x] Merged the release PR\n",
                "- [ ] Rotate the deploy key\n",
                "- [ ] Verify [#gate] in a live pane\n",
            )
        );
        assert_eq!(count_checklist_items(&todo_body(&outcome.content)), 3);
    }

    #[test]
    fn add_is_idempotent_against_open_and_checked_items() {
        let content = "<!-- agent:todo -->\n- [x] Rotate the deploy key\n<!-- /agent:todo -->\n";
        for text in [
            "Rotate the deploy key",
            "- [ ] Rotate the deploy key",
            "  Rotate the deploy key  ",
        ] {
            let outcome = add_todo_item_to_document(content, text).unwrap();
            assert!(!outcome.inserted, "{text:?}");
            assert_eq!(outcome.content, content);
        }
    }

    #[test]
    fn add_handles_body_without_trailing_newline() {
        let no_newline = "<!-- agent:todo -->\n- [ ] first<!-- /agent:todo -->\n";
        let outcome = add_todo_item_to_document(no_newline, "second").unwrap();
        assert_eq!(todo_body(&outcome.content), "- [ ] first\n- [ ] second\n");
    }

    #[test]
    fn add_rejects_empty_and_multi_line_actions() {
        let content = "<!-- agent:todo -->\n<!-- /agent:todo -->\n";
        assert!(add_todo_item_to_document(content, "   ").is_err());
        assert!(add_todo_item_to_document(content, "- [ ] ").is_err());
        assert!(add_todo_item_to_document(content, "one\ntwo").is_err());
    }

    #[test]
    fn unreferenced_ids_match_whole_id_tokens_only() {
        let content = "<!-- agent:todo -->\n- [ ] Verify [#gate] live\n- [ ] Check #gate2x\n<!-- /agent:todo -->\n";
        let ids = vec![
            "gate".to_string(),
            "#gate2".to_string(),
            "gate2x".to_string(),
            "gate".to_string(),
        ];
        assert_eq!(todo_unreferenced_ids(content, &ids), vec!["gate2"]);
        assert_eq!(
            todo_unreferenced_ids("<!-- agent:review -->\n<!-- /agent:review -->\n", &ids),
            vec!["gate", "gate2", "gate2x"]
        );
    }

    #[test]
    fn checklist_item_text_accepts_list_markers_and_states() {
        assert_eq!(checklist_item_text("- [ ] a"), Some("a"));
        assert_eq!(checklist_item_text("  * [x] b"), Some("b"));
        assert_eq!(checklist_item_text("+ [/] c"), Some("c"));
        assert_eq!(checklist_item_text("12. [ ] d"), Some("d"));
        assert_eq!(checklist_item_text("- plain bullet"), None);
        assert_eq!(checklist_item_text("[ ] no marker"), None);
    }
}
