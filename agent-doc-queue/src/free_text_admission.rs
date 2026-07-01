//! Pure queue free-text admission policy.
//!
//! Orchestration owns file IO and document mutation. This module owns the
//! queue-specific question: which queue prompt text is eligible to become
//! tracked backlog work, and which queue-origin prompts may be admitted for the
//! current maintenance pass.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

/// Queue-origin prompt admission scope for a maintenance pass.
#[derive(Debug, Clone, Default)]
pub enum FreeTextAdmissionScope {
    /// Admit no queue-origin prompts.
    #[default]
    None,
    /// Admit every actionable queue-origin prompt.
    All,
    /// Admit only queue-origin prompts whose normalized keys are listed.
    NormalizedKeys(HashSet<String>),
}

impl FreeTextAdmissionScope {
    /// Whether a raw queue prompt is actionable and allowed by this scope.
    pub fn allows_prompt(&self, text: &str) -> bool {
        if !free_text_prompt_is_backlog_task(text) {
            return false;
        }
        match self {
            Self::None => false,
            Self::All => true,
            Self::NormalizedKeys(keys) => {
                let key = crate::queue_response::normalize_for_answer_match(
                    &normalize_admitted_free_text(text),
                );
                keys.contains(&key)
            }
        }
    }
}

/// Normalize prompt text before using it as admitted backlog work.
pub fn normalize_admitted_free_text(text: &str) -> String {
    text.trim().trim_start_matches('❯').trim().to_string()
}

/// True when free text is suitable to materialize as tracked backlog work.
pub fn free_text_prompt_is_backlog_task(text: &str) -> bool {
    let trimmed = normalize_admitted_free_text(text);
    !trimmed.is_empty()
        && !trimmed.starts_with('/')
        && !trimmed.starts_with('#')
        && !crate::queue_heads::is_do_directive(&trimmed)
        && agent_doc_prompt_lines::text_line_looks_like_prompt_target(&trimmed)
}

pub fn explicit_queue_go_mode(
    attrs: &std::collections::HashMap<String, String>,
    frontmatter_queue: Option<&str>,
) -> bool {
    attrs.contains_key("go")
        || frontmatter_queue.is_some_and(|raw| raw.trim().eq_ignore_ascii_case("go"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeTextWorkPrompt {
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActionableFreeTextPrompts {
    pub prompts: Vec<FreeTextWorkPrompt>,
}

impl ActionableFreeTextPrompts {
    pub fn has_work(&self) -> bool {
        !self.prompts.is_empty()
    }
}

pub fn collect_actionable_free_text_prompts(
    exchange_prompt: Option<&str>,
    entries: &[crate::document_queue::QueueEntry],
    queue_scope: &FreeTextAdmissionScope,
) -> ActionableFreeTextPrompts {
    let mut prompts = Vec::new();
    if let Some(exchange_prompt) = exchange_prompt
        && free_text_prompt_is_backlog_task(exchange_prompt)
    {
        prompts.push(FreeTextWorkPrompt {
            text: normalize_admitted_free_text(exchange_prompt),
        });
    }
    for entry in entries {
        if let crate::document_queue::QueueEntry::Prompt(prompt) = entry
            && queue_scope.allows_prompt(&prompt.text)
        {
            prompts.push(FreeTextWorkPrompt {
                text: normalize_admitted_free_text(&prompt.text),
            });
        }
    }
    let mut seen = std::collections::HashSet::new();
    prompts.retain(|prompt| {
        let key = crate::queue_response::normalize_for_answer_match(&prompt.text);
        !key.is_empty() && seen.insert(key)
    });
    ActionableFreeTextPrompts { prompts }
}

pub fn append_empty_agent_component(content: &str, name: &str) -> String {
    let mut out = content.trim_end_matches('\n').to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str("<!-- agent:");
    out.push_str(name);
    out.push_str(" -->\n<!-- /agent:");
    out.push_str(name);
    out.push_str(" -->\n");
    out
}

pub fn queue_entry_is_admitted_free_text(
    entry: &crate::document_queue::QueueEntry,
    queue_scope: &FreeTextAdmissionScope,
) -> bool {
    matches!(
        entry,
        crate::document_queue::QueueEntry::Prompt(prompt) if queue_scope.allows_prompt(&prompt.text)
    )
}

pub fn ensure_queue_priority_attr(content: &str) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(content.to_string());
    };
    if queue.attrs.contains_key("priority") {
        return Ok(content.to_string());
    }
    let raw_tag = &content[queue.open_start..queue.open_end];
    let Some(close_idx) = raw_tag.rfind("-->") else {
        return Ok(content.to_string());
    };
    let (head, tail) = raw_tag.split_at(close_idx);
    let mut new_tag = head.trim_end().to_string();
    new_tag.push_str(" priority ");
    new_tag.push_str(tail);

    let mut rebuilt = String::with_capacity(content.len() + " priority".len());
    rebuilt.push_str(&content[..queue.open_start]);
    rebuilt.push_str(&new_tag);
    rebuilt.push_str(&content[queue.open_end..]);
    Ok(rebuilt)
}

pub fn goal_command_for_ids(ids: &[String]) -> String {
    let refs = ids
        .iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("/goal Implement backlog item(s): {refs}")
}

pub fn goal_command_already_queued(
    entries: &[crate::document_queue::QueueEntry],
    ids: &[String],
) -> bool {
    entries.iter().any(|entry| {
        let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
            return false;
        };
        let text = prompt.text.trim();
        text.starts_with("/goal")
            && ids
                .iter()
                .all(|id| text.contains(&format!("#{id}")) || text.contains(&format!("[#{id}]")))
    })
}

/// Whether the document currently declares an active queue for free-text
/// admission purposes.
pub fn queue_currently_active_for_free_text_admission(
    content: &str,
    queue_attrs: &HashMap<String, String>,
) -> bool {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap_or_default();
    let frontmatter_active = match fm
        .queue
        .as_deref()
        .and_then(agent_doc_frontmatter::frontmatter::QueueControl::parse)
    {
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Start) => true,
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Stop) => false,
        None => fm.queue_active.unwrap_or(false),
    };
    let marker_active = crate::document_queue::has_auto_attr(queue_attrs)
        || matches!(
            crate::document_queue::marker_control(queue_attrs),
            Some(agent_doc_frontmatter::frontmatter::QueueControl::Start)
        );
    frontmatter_active || marker_active
}

/// Compute which queue-origin free-text prompts may be admitted.
///
/// Inactive queues admit all actionable queue-origin free text. Active queues
/// only admit newly-added actionable free text compared to the caller-provided
/// snapshot content, so existing active queue prompts are not repeatedly
/// converted into backlog work.
pub fn queue_free_text_admission_scope(
    content: &str,
    queue_attrs: &HashMap<String, String>,
    entries: &[crate::document_queue::QueueEntry],
    snapshot_content: Option<&str>,
) -> FreeTextAdmissionScope {
    if !queue_currently_active_for_free_text_admission(content, queue_attrs) {
        return FreeTextAdmissionScope::All;
    }

    let Some(snapshot_content) = snapshot_content else {
        return FreeTextAdmissionScope::None;
    };
    let snapshot_keys = snapshot_queue_free_text_prompt_keys(snapshot_content);
    let mut new_keys = HashSet::new();
    for entry in entries {
        let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if !free_text_prompt_is_backlog_task(&prompt.text) {
            continue;
        }
        let key = crate::queue_response::normalize_for_answer_match(&normalize_admitted_free_text(
            &prompt.text,
        ));
        if !key.is_empty() && !snapshot_keys.contains(&key) {
            new_keys.insert(key);
        }
    }
    if new_keys.is_empty() {
        FreeTextAdmissionScope::None
    } else {
        FreeTextAdmissionScope::NormalizedKeys(new_keys)
    }
}

/// Return normalized keys for actionable queue free-text prompts in a snapshot.
pub fn snapshot_queue_free_text_prompt_keys(content: &str) -> HashSet<String> {
    let mut keys = HashSet::new();
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return keys;
    };
    let Some(queue) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return keys;
    };
    let body = &content[queue.open_end..queue.close_start];
    let Ok(entries) = crate::document_queue::parse(body) else {
        return keys;
    };
    for entry in entries {
        let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if free_text_prompt_is_backlog_task(&prompt.text) {
            let key = crate::queue_response::normalize_for_answer_match(
                &normalize_admitted_free_text(&prompt.text),
            );
            if !key.is_empty() {
                keys.insert(key);
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_text_prompt_is_backlog_task_rejects_commands_and_directives() {
        assert!(free_text_prompt_is_backlog_task("Implement checkout setup"));
        assert!(free_text_prompt_is_backlog_task(
            "❯ Implement checkout setup"
        ));
        assert!(!free_text_prompt_is_backlog_task(""));
        assert!(!free_text_prompt_is_backlog_task("/clear"));
        assert!(!free_text_prompt_is_backlog_task("#setup"));
        assert!(!free_text_prompt_is_backlog_task("do [#setup]"));
    }

    #[test]
    fn active_scope_only_allows_new_snapshot_prompts() {
        let content = concat!(
            "---\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- Implement existing work\n",
            "- Implement new work\n",
            "<!-- /agent:queue -->\n",
        );
        let snapshot = content.replace("- Implement new work\n", "");
        let queue_attrs = HashMap::new();
        let body = content
            .split("<!-- agent:queue -->")
            .nth(1)
            .unwrap()
            .split("<!-- /agent:queue -->")
            .next()
            .unwrap();
        let entries = crate::document_queue::parse(body).unwrap();
        let scope =
            queue_free_text_admission_scope(content, &queue_attrs, &entries, Some(&snapshot));

        assert!(!scope.allows_prompt("Implement existing work"));
        assert!(scope.allows_prompt("Implement new work"));
    }

    #[test]
    fn collect_actionable_free_text_prompts_dedupes_exchange_and_queue_prompts() {
        let entries = vec![crate::document_queue::QueueEntry::Prompt(
            crate::document_queue::QueuePrompt {
                text: "❯ Implement checkout setup".to_string(),
                multiline: false,
            },
        )];

        let prompts = collect_actionable_free_text_prompts(
            Some("Implement checkout setup"),
            &entries,
            &FreeTextAdmissionScope::All,
        );

        assert_eq!(
            prompts,
            ActionableFreeTextPrompts {
                prompts: vec![FreeTextWorkPrompt {
                    text: "Implement checkout setup".to_string()
                }]
            }
        );
        assert!(prompts.has_work());
    }

    #[test]
    fn ensure_queue_priority_attr_adds_priority_to_queue_opener() {
        let content = concat!(
            "<!-- agent:queue go -->\n",
            "- do [#work]\n",
            "<!-- /agent:queue -->\n",
        );

        let updated = ensure_queue_priority_attr(content).unwrap();

        assert!(updated.contains("<!-- agent:queue go priority -->"));
    }

    #[test]
    fn goal_command_matching_accepts_plain_and_bracketed_refs() {
        let ids = vec!["abc123".to_string(), "def456".to_string()];
        let command = goal_command_for_ids(&ids);
        let entries = vec![crate::document_queue::QueueEntry::Prompt(
            crate::document_queue::QueuePrompt {
                text: command,
                multiline: false,
            },
        )];

        assert!(goal_command_already_queued(&entries, &ids));

        let bracketed = vec![crate::document_queue::QueueEntry::Prompt(
            crate::document_queue::QueuePrompt {
                text: "/goal Implement [#abc123] and [#def456]".to_string(),
                multiline: false,
            },
        )];
        assert!(goal_command_already_queued(&bracketed, &ids));
    }
}
