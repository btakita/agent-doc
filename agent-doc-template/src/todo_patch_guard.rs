//! Guard policy for destructive `agent:todo` template patchbacks.

use anyhow::{Context, Result};

use agent_doc_element::element;

use crate::PatchBlock;

fn count_markdown_checklist_items(body: &str) -> usize {
    body.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
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
                })
            else {
                return false;
            };

            rest.starts_with("[ ] ") || rest.starts_with("[x] ") || rest.starts_with("[/] ")
        })
        .count()
}

fn todo_component_checklist_count(current_content: &str) -> Result<Option<usize>> {
    let components = element::parse(current_content)
        .context("failed to parse components for todo patch validation")?;
    Ok(components
        .iter()
        .find(|component| component.name == "todo")
        .map(|component| count_markdown_checklist_items(component.content(current_content))))
}

pub fn enforce_no_destructive_todo_patch(
    current_content: &str,
    patches: &[PatchBlock],
) -> Result<()> {
    let Some(todo_patch) = patches.iter().rev().find(|patch| patch.name == "todo") else {
        return Ok(());
    };
    let Some(current_count) = todo_component_checklist_count(current_content)? else {
        return Ok(());
    };
    if current_count == 0 {
        return Ok(());
    }

    let patched_count = count_markdown_checklist_items(&todo_patch.content);
    if patched_count < current_count {
        anyhow::bail!(
            "ERR: patch:todo would reduce total checklist item count from {} to {} and is forbidden because it can silently delete untouched todo entries. Rewrite the full todo component or edit the document directly.",
            current_count,
            patched_count
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_todo(todo_body: &str) -> String {
        format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\nPlease reply\n<!-- /agent:exchange -->\n\n<!-- agent:todo patch=replace -->\n{todo_body}<!-- /agent:todo -->\n"
        )
    }

    #[test]
    fn destructive_todo_patch_is_rejected_when_it_drops_checklist_items() {
        let content = doc_with_todo(concat!(
            "### Phase 1\n\n",
            "- [x] Select benchmark\n",
            "- [x] Write methodology\n\n",
            "### Phase 2\n\n",
            "- [ ] Expand git signal extraction\n",
            "- [ ] Re-score sessions\n",
        ));
        let patches = vec![PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n",
            ),
        )];

        let err = enforce_no_destructive_todo_patch(&content, &patches)
            .expect_err("subset todo patch should fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("patch:todo would reduce total checklist item count from 4 to 2"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn todo_patch_with_same_checklist_count_is_allowed() {
        let content = doc_with_todo(concat!(
            "### Phase 1\n\n",
            "- [ ] Original item 1\n",
            "- [ ] Original item 2\n",
        ));
        let patches = vec![PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Updated item 1\n",
                "- [ ] Updated item 2\n",
            ),
        )];

        enforce_no_destructive_todo_patch(&content, &patches)
            .expect("same-size todo rewrite should remain allowed");
    }

    #[test]
    fn last_todo_patch_controls_final_guard_count() {
        let content = doc_with_todo(concat!(
            "- [ ] Original item 1\n",
            "- [ ] Original item 2\n",
        ));
        let patches = vec![
            PatchBlock::new("todo", "- [ ] Original item 1\n"),
            PatchBlock::new(
                "todo",
                concat!("- [x] Updated item 1\n", "- [ ] Updated item 2\n"),
            ),
        ];

        enforce_no_destructive_todo_patch(&content, &patches)
            .expect("guard should evaluate the final todo patch that will apply");
    }
}
