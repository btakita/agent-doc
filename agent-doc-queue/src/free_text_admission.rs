//! Pure queue free-text admission policy.
//!
//! Orchestration owns file IO and document mutation. This module owns the
//! queue-specific question: which queue prompt text is eligible to become
//! tracked backlog work, and which queue-origin prompts may be admitted for the
//! current maintenance pass.

use std::collections::{HashMap, HashSet};

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
}
