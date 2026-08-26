//! Pure queue free-text admission policy.
//!
//! Orchestration owns file IO and document mutation. This module owns the
//! queue-specific question: which queue prompt text is eligible to become
//! tracked backlog work, and which queue-origin prompts may be admitted for the
//! current maintenance pass.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

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

/// Match the editor race where queue maintenance admitted a partial free-text
/// draft into backlog and the editor then projected the continued draft beside
/// the generated `do [#id]` head. The adjacency and strict-prefix requirements
/// keep intentionally similar, independently queued tasks distinct.
fn adjacent_snapshot_extension_claims(
    entries: &[crate::document_queue::QueueEntry],
    existing_items: &[agent_doc_element_backlog::backlog::PendingItem],
    actionable_keys: &HashSet<String>,
) -> Vec<(String, String, String)> {
    let mut provisional = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        let new_text = normalize_admitted_free_text(&prompt.text);
        let new_key = crate::queue_response::normalize_for_answer_match(&new_text);
        if new_key.is_empty() || !actionable_keys.contains(&new_key) {
            continue;
        }

        let mut adjacent_ids = HashSet::new();
        for neighbor in [index.checked_sub(1), index.checked_add(1)]
            .into_iter()
            .flatten()
            .filter_map(|neighbor| entries.get(neighbor))
        {
            if let Some(id) = crate::queue_projection::queue_entry_do_id(neighbor) {
                adjacent_ids.insert(id);
            }
        }
        let mut candidates = existing_items
            .iter()
            .filter(|item| item.state != agent_doc_element_backlog::backlog::PendingState::Done)
            .filter(|item| adjacent_ids.contains(&item.id.trim().to_ascii_lowercase()))
            .filter_map(|item| {
                let old_key = crate::queue_response::normalize_for_answer_match(&item.text);
                (old_key.len() >= 8
                    && new_key.len() > old_key.len()
                    && new_key.starts_with(&old_key))
                .then(|| (item.id.trim().to_ascii_lowercase(), new_text.clone()))
            })
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        if let [candidate] = candidates.as_slice() {
            provisional.push((new_key, candidate.0.clone(), candidate.1.clone()));
        }
    }

    let mut claims_per_id = HashMap::<String, usize>::new();
    for (_, id, _) in &provisional {
        *claims_per_id.entry(id.clone()).or_default() += 1;
    }
    provisional
        .into_iter()
        .filter(|(_, id, _)| claims_per_id.get(id) == Some(&1))
        .collect()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeTextAdmissionExecution {
    Goal,
    Queue,
}

impl FreeTextAdmissionExecution {
    pub fn label(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Queue => "queue",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeTextAdmission {
    pub content: String,
    pub queued_ids: Vec<String>,
    pub admitted_count: usize,
    pub execution_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedFreeTextAdmission {
    pub content: String,
    pub unique_ids: Vec<String>,
    pub admitted_count: usize,
    queue_entries: Vec<crate::document_queue::QueueEntry>,
    queue_start_required: bool,
}

impl PreparedFreeTextAdmission {
    pub fn finish(self, execution: FreeTextAdmissionExecution) -> Result<FreeTextAdmission> {
        let components = agent_doc_element::element::parse(&self.content)?;
        let queue = components
            .iter()
            .find(|c| c.name == "queue")
            .context("free-text admission: queue component missing")?
            .clone();
        let mut queue_entries = self.queue_entries;
        let queued_ids = match execution {
            FreeTextAdmissionExecution::Goal => {
                let command = goal_command_for_ids(&self.unique_ids);
                if !goal_command_already_queued(&queue_entries, &self.unique_ids) {
                    queue_entries.insert(
                        0,
                        crate::document_queue::QueueEntry::Prompt(
                            crate::document_queue::QueuePrompt {
                                text: command,
                                multiline: false,
                                indent: 0,
                                ordered_marker: None,
                            },
                        ),
                    );
                    if self.queue_start_required {
                        queue_entries
                            .insert(0, crate::document_queue::QueueEntry::StartFence(None));
                    }
                }
                Vec::new()
            }
            FreeTextAdmissionExecution::Queue => {
                let before_ids: HashSet<String> = queue_entries
                    .iter()
                    .filter_map(crate::queue_projection::queue_entry_do_id)
                    .collect();
                let synced = crate::document_queue::sync_backlog_into_queue(
                    &queue_entries,
                    &self.unique_ids,
                    crate::document_queue::BacklogQueueSyncMode::Prepend,
                )
                .unwrap_or_else(|| queue_entries.clone());
                queue_entries = synced;
                if self.queue_start_required
                    && !matches!(
                        queue_entries.first(),
                        Some(crate::document_queue::QueueEntry::StartFence(_))
                    )
                {
                    queue_entries.insert(0, crate::document_queue::QueueEntry::StartFence(None));
                }
                queue_entries
                    .iter()
                    .filter_map(crate::queue_projection::queue_entry_do_id)
                    .filter(|id| !before_ids.contains(id))
                    .collect()
            }
        };

        let new_queue_body = crate::document_queue::render(&queue_entries);
        let mut content = queue.replace_content(&self.content, &new_queue_body);
        if execution == FreeTextAdmissionExecution::Queue {
            content = ensure_queue_priority_attr(&content)?;
        }
        Ok(FreeTextAdmission {
            content,
            queued_ids,
            admitted_count: self.admitted_count,
            execution_label: execution.label(),
        })
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

pub fn prepare_free_text_admission(
    content: &str,
    entries: &[crate::document_queue::QueueEntry],
    exchange_prompt: Option<&str>,
    queue_scope: &FreeTextAdmissionScope,
    queue_start_required: bool,
    document_id: &str,
) -> Result<Option<PreparedFreeTextAdmission>> {
    let prompts = collect_actionable_free_text_prompts(exchange_prompt, entries, queue_scope);
    if !prompts.has_work() {
        return Ok(None);
    }

    let mut current = if agent_doc_element::element::parse(content)?
        .iter()
        .any(|c| c.name == "backlog")
    {
        content.to_string()
    } else {
        append_empty_agent_component(content, "backlog")
    };
    let mut components = agent_doc_element::element::parse(&current)?;
    let backlog = components
        .iter()
        .find(|c| c.name == "backlog")
        .context("free-text admission: backlog component missing after ensure")?
        .clone();
    let original_backlog_body = backlog.content(&current).to_string();
    let mut backlog_body = original_backlog_body.clone();
    let (_, existing_items, _) = agent_doc_element_backlog::backlog::parse_items(&backlog_body);
    let actionable_keys = prompts
        .prompts
        .iter()
        .map(|prompt| {
            crate::queue_response::normalize_for_answer_match(&normalize_admitted_free_text(
                &prompt.text,
            ))
        })
        .collect::<HashSet<_>>();
    let extension_claims =
        adjacent_snapshot_extension_claims(entries, &existing_items, &actionable_keys);
    if !extension_claims.is_empty() {
        let edits = extension_claims
            .iter()
            .map(|(_, id, text)| (id.clone(), text.clone()))
            .collect::<Vec<_>>();
        backlog_body = agent_doc_element_backlog::backlog::op_edit_many(&backlog_body, &edits)
            .context("free-text admission: update continued queue draft in backlog")?;
    }
    let mut id_by_text = HashMap::new();
    for item in existing_items {
        if item.state != agent_doc_element_backlog::backlog::PendingState::Done {
            let key = crate::queue_response::normalize_for_answer_match(&item.text);
            if !key.is_empty() {
                id_by_text.entry(key).or_insert(item.id);
            }
        }
    }
    for (key, id, _) in extension_claims {
        id_by_text.insert(key, id);
    }

    let mut prompt_keys = Vec::new();
    let mut texts_to_add = Vec::new();
    for prompt in &prompts.prompts {
        let key = crate::queue_response::normalize_for_answer_match(&prompt.text);
        if !id_by_text.contains_key(&key) {
            texts_to_add.push(prompt.text.clone());
        }
        prompt_keys.push(key);
    }
    if !texts_to_add.is_empty() {
        let outcome = agent_doc_element_backlog::backlog::op_prepend_many_with_outcomes(
            &backlog_body,
            &texts_to_add,
            document_id,
            false,
        )?;
        for (text, item_outcome) in texts_to_add.iter().zip(outcome.outcomes) {
            let key = crate::queue_response::normalize_for_answer_match(text);
            id_by_text.insert(key, item_outcome.id.clone());
        }
        backlog_body = outcome.body;
    }
    if backlog_body != original_backlog_body {
        current = backlog.replace_content(&current, &backlog_body);
    }

    let mut unique_ids = Vec::new();
    let mut seen_ids = HashSet::new();
    for key in prompt_keys {
        let Some(id) = id_by_text.get(&key) else {
            continue;
        };
        let normalized = id.trim().to_ascii_lowercase();
        if !normalized.is_empty() && seen_ids.insert(normalized.clone()) {
            unique_ids.push(normalized);
        }
    }
    if unique_ids.is_empty() {
        return Ok(None);
    }

    components = agent_doc_element::element::parse(&current)?;
    components
        .iter()
        .find(|c| c.name == "queue")
        .context("free-text admission: queue component missing")?;
    let queue_entries = entries
        .iter()
        .filter(|entry| !queue_entry_is_admitted_free_text(entry, queue_scope))
        .cloned()
        .collect();

    Ok(Some(PreparedFreeTextAdmission {
        content: current,
        unique_ids,
        admitted_count: prompts.prompts.len(),
        queue_entries,
        queue_start_required,
    }))
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
                indent: 0,
                ordered_marker: None,
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
                indent: 0,
                ordered_marker: None,
            },
        )];

        assert!(goal_command_already_queued(&entries, &ids));

        let bracketed = vec![crate::document_queue::QueueEntry::Prompt(
            crate::document_queue::QueuePrompt {
                text: "/goal Implement [#abc123] and [#def456]".to_string(),
                multiline: false,
                indent: 0,
                ordered_marker: None,
            },
        )];
        assert!(goal_command_already_queued(&bracketed, &ids));
    }

    fn queue_entries_from_content(content: &str) -> Vec<crate::document_queue::QueueEntry> {
        let components = agent_doc_element::element::parse(content).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap();
        crate::document_queue::parse(queue.content(content)).unwrap()
    }

    #[test]
    fn prepare_and_finish_queue_execution_adds_backlog_and_synced_queue() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- Implement checkout setup\n",
            "<!-- /agent:queue -->\n",
        );
        let entries = queue_entries_from_content(content);

        let prepared = prepare_free_text_admission(
            content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            true,
            "doc-id",
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.admitted_count, 1);
        assert_eq!(prepared.unique_ids.len(), 1);
        assert!(prepared.content.contains("<!-- agent:backlog -->"));

        let id = prepared.unique_ids[0].clone();
        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let queue_entries = queue_entries_from_content(&admission.content);

        assert_eq!(admission.execution_label, "queue");
        assert_eq!(admission.queued_ids, vec![id.clone()]);
        assert!(admission.content.contains("<!-- agent:queue priority -->"));
        assert!(matches!(
            queue_entries.first(),
            Some(crate::document_queue::QueueEntry::StartFence(None))
        ));
        assert!(queue_entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Prompt(prompt)
                if prompt.text == format!("do [#{id}]")
        )));
        assert!(!queue_entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Prompt(prompt)
                if prompt.text == "Implement checkout setup"
        )));
    }

    #[test]
    fn prepare_and_finish_goal_execution_reuses_existing_backlog_id() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#existing] Implement checkout setup\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- Implement checkout setup\n",
            "<!-- /agent:queue -->\n",
        );
        let entries = queue_entries_from_content(content);

        let prepared = prepare_free_text_admission(
            content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            true,
            "doc-id",
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.unique_ids, vec!["existing".to_string()]);

        let admission = prepared.finish(FreeTextAdmissionExecution::Goal).unwrap();
        let queue_entries = queue_entries_from_content(&admission.content);

        assert_eq!(admission.execution_label, "goal");
        assert!(admission.queued_ids.is_empty());
        assert!(matches!(
            queue_entries.first(),
            Some(crate::document_queue::QueueEntry::StartFence(None))
        ));
        assert!(queue_entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Prompt(prompt)
                if prompt.text == "/goal Implement backlog item(s): #existing"
        )));
        assert!(!queue_entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Prompt(prompt)
            if prompt.text == "Implement checkout setup"
        )));
    }

    #[test]
    fn continued_queue_draft_updates_adjacent_snapshot_item_instead_of_duplicating_it() {
        let content = concat!(
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#existing] Implement release and publish\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 do [#existing]\n",
            "- Implement release and publish and install\n",
            "<!-- /agent:queue -->\n",
        );
        let entries = queue_entries_from_content(content);

        let prepared = prepare_free_text_admission(
            content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            false,
            "doc-id",
        )
        .unwrap()
        .unwrap();
        assert_eq!(prepared.unique_ids, vec!["existing".to_string()]);

        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let components = agent_doc_element::element::parse(&admission.content).unwrap();
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .unwrap();
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(backlog.content(&admission.content));
        assert_eq!(items.len(), 1, "{:#?}", items);
        assert_eq!(items[0].id, "existing");
        assert_eq!(items[0].text, "Implement release and publish and install");

        let queue_entries = queue_entries_from_content(&admission.content);
        assert_eq!(
            queue_entries
                .iter()
                .filter_map(crate::queue_projection::queue_entry_do_id)
                .filter(|id| id == "existing")
                .count(),
            1,
            "the partial snapshot and continued draft must converge to one queue head:\n{}",
            admission.content
        );
        assert!(
            !admission
                .content
                .contains("- Implement release and publish and install\n")
        );
    }

    #[test]
    fn non_adjacent_similar_queue_draft_remains_distinct_work() {
        let content = concat!(
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#existing] Implement release and publish\n",
            "- [ ] [#other] Review release notes\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 🚧 do [#existing]\n",
            "- do [#other]\n",
            "- Implement release and publish and install\n",
            "<!-- /agent:queue -->\n",
        );
        let entries = queue_entries_from_content(content);

        let prepared = prepare_free_text_admission(
            content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            false,
            "doc-id",
        )
        .unwrap()
        .unwrap();
        assert_eq!(prepared.unique_ids.len(), 1);
        assert_ne!(prepared.unique_ids[0], "existing");

        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let components = agent_doc_element::element::parse(&admission.content).unwrap();
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .unwrap();
        let (_, items, _) =
            agent_doc_element_backlog::backlog::parse_items(backlog.content(&admission.content));
        assert_eq!(items.len(), 3, "{:#?}", items);
        assert!(
            items.iter().any(|item| {
                item.id == "existing" && item.text == "Implement release and publish"
            })
        );
        assert!(items.iter().any(|item| {
            item.id != "existing" && item.text == "Implement release and publish and install"
        }));
    }
}
