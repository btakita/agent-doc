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
///
/// `#halftypedcoin`: text the shared typing gate calls plainly unfinished
/// (`Add a ``, `… publish the`, an open delimiter) is never coined into a
/// backlog id, however long it has been quiet: a coined id is durable, and a
/// half-typed stub plus the operator's finished line beside it is a meaningless
/// item and a duplicate. The line stays free text until it reads finished.
pub fn free_text_prompt_is_backlog_task(text: &str) -> bool {
    let trimmed = normalize_admitted_free_text(text);
    !trimmed.is_empty()
        && !trimmed.starts_with('/')
        && !trimmed.starts_with('#')
        && !crate::queue_heads::is_do_directive(&trimmed)
        && agent_doc_prompt_lines::text_line_looks_like_prompt_target(&trimmed)
        && !free_text_looks_unfinished(&trimmed)
}

/// The shared typing gate's structural verdict (`#steeringtypinggate`):
/// true when the text is plainly still being typed.
fn free_text_looks_unfinished(text: &str) -> bool {
    agent_doc_debounce::edit_settle::completion_signal(text)
        == agent_doc_debounce::edit_settle::CompletionSignal::Incomplete
}

/// Typing-gate evidence for coining free text into backlog ids
/// (`#halftypedcoin`).
///
/// Preflight's operator-edit quiescence wait (`#qheadcomposing`) observes the
/// live text while the operator types. When it saw edits, it knows the text it
/// last settled on and how long ago that text last changed. A queue line that
/// is not in that settled text appeared (or changed) after the wait returned:
/// the operator is still typing it. Each candidate line goes through the same
/// [`agent_doc_debounce::edit_settle::settle_decision`] steering delivery
/// reads; a line the gate holds is left as free text for this pass instead of
/// being coined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FreeTextCoinGate {
    /// `None`: no edit was observed during preflight, so there is no live
    /// typing evidence and only the structural rule applies.
    pub observed: Option<FreeTextCoinObservation>,
    /// The document's quiet window.
    pub debounce_ms: u64,
    /// Hard max-hold, past which a structurally finished line coins anyway.
    pub max_hold_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeTextCoinObservation {
    /// The text the quiescence wait last observed.
    pub settled_text: String,
    /// How long before this maintenance pass that text last changed.
    pub quiet_for_ms: u64,
}

impl FreeTextCoinGate {
    /// No live typing evidence: only the structural rule applies.
    pub fn structural_only() -> Self {
        Self::default()
    }

    /// The settle decision for coining one queue prompt.
    pub fn decision(&self, text: &str) -> agent_doc_debounce::edit_settle::SettleDecision {
        use agent_doc_debounce::edit_settle::{
            CompletionSignal, SettleDecision, SettleInputs, completion_signal,
            has_unbalanced_delimiters, settle_decision,
        };
        let normalized = normalize_admitted_free_text(text);
        let signal = completion_signal(&normalized);
        if signal == CompletionSignal::Incomplete {
            // Never coined on quiescence or max-hold: an unfinished line stays
            // free text rather than becoming a durable stub id.
            return SettleDecision::Held {
                recheck_after_ms: (self.debounce_ms / 2).max(100),
            };
        }
        let Some(observed) = &self.observed else {
            return SettleDecision::Settled;
        };
        let quiet_for_ms = if text_present_in_settled(&normalized, &observed.settled_text) {
            observed.quiet_for_ms
        } else {
            0
        };
        settle_decision(SettleInputs {
            quiet_for_ms: Some(quiet_for_ms),
            stable_for_ms: None,
            held_for_ms: quiet_for_ms,
            debounce_ms: self.debounce_ms,
            max_hold_ms: self.max_hold_ms,
            signal,
            unbalanced_delimiters: has_unbalanced_delimiters(&normalized),
            verdict: None,
        })
    }

    /// Whether the gate lets this queue prompt be coined in this pass.
    pub fn allows_coining(&self, text: &str) -> bool {
        self.decision(text).deliver()
    }
}

/// True when every line of `text` already stood as a whole line (modulo list
/// marker) in `settled`: the line existed, unchanged, when the operator last
/// paused.
fn text_present_in_settled(text: &str, settled: &str) -> bool {
    let strip_marker = |line: &str| -> String {
        let t = line.trim();
        let t = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.strip_prefix("+ "))
            .unwrap_or_else(|| {
                let digits = t.chars().take_while(char::is_ascii_digit).count();
                if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
                    &t[digits + 2..]
                } else {
                    t
                }
            });
        normalize_admitted_free_text(t)
    };
    let settled_lines: HashSet<String> = settled.lines().map(strip_marker).collect();
    let mut any = false;
    for line in text
        .lines()
        .map(strip_marker)
        .filter(|line| !line.is_empty())
    {
        any = true;
        if !settled_lines.contains(&line) {
            return false;
        }
    }
    any
}

/// Narrow a queue admission scope to the prompts the typing gate lets coin
/// this pass (`#halftypedcoin`). Returns the narrowed scope and the raw texts
/// of the prompts it held back.
pub fn gate_free_text_admission_scope(
    scope: FreeTextAdmissionScope,
    entries: &[crate::document_queue::QueueEntry],
    gate: &FreeTextCoinGate,
) -> (FreeTextAdmissionScope, Vec<String>) {
    if matches!(scope, FreeTextAdmissionScope::None) {
        return (scope, Vec::new());
    }
    let mut allowed = HashSet::new();
    let mut held = Vec::new();
    for entry in entries {
        let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
            continue;
        };
        if !scope.allows_prompt(&prompt.text) {
            continue;
        }
        if gate.allows_coining(&prompt.text) {
            allowed.insert(crate::queue_response::normalize_for_answer_match(
                &normalize_admitted_free_text(&prompt.text),
            ));
        } else {
            held.push(prompt.text.clone());
        }
    }
    if held.is_empty() {
        return (scope, held);
    }
    let narrowed = if allowed.is_empty() {
        FreeTextAdmissionScope::None
    } else {
        FreeTextAdmissionScope::NormalizedKeys(allowed)
    };
    (narrowed, held)
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
                // `#halftypedcoin`: a stub coined from a half-typed line
                // (`Add a ``) is too short for the prefix floor, but the typing
                // gate already says it was unfinished, so the operator's
                // continued line is its completion, not a second task.
                let prefix_floor_met = old_key.len() >= 8 || free_text_looks_unfinished(&item.text);
                (!old_key.is_empty()
                    && prefix_floor_met
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

/// A proven revision of free text that one agent projection already
/// materialized as an id-backed queue/backlog item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedFreeTextRevision {
    pub id: String,
    pub text: String,
}

/// Result of reconciling one raced materialization projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedFreeTextReconciliation {
    pub queue_entries: Vec<crate::document_queue::QueueEntry>,
    pub backlog_edits: Vec<MaterializedFreeTextRevision>,
}

/// Reconcile an operator revision that raced free-text materialization.
///
/// Identity here is the three-way projection provenance, never a text-prefix
/// guess:
///
/// 1. `baseline -> projected` removes a set of free-text source nodes and adds
///    one `do [#id]` node per source. The projected backlog text establishes a
///    unique bijection from each removed source node to its durable id.
/// 2. Removing those exact projected id nodes from `observed` must recover the
///    baseline queue shape, with only the proven source slots allowed to differ.
/// 3. Each differing source slot must still be actionable free text. Its text is
///    the authoritative revision of the same source identity, so the backlog id
///    is edited and the id-less replay row is removed.
///
/// Any extra row, missing id, ambiguous equal-text source, or change outside a
/// source slot makes the proof fail closed. Consequently retries are idempotent:
/// after one reconciliation the projected ids remain but the source replay rows
/// do not, so step 2 can no longer match.
pub fn reconcile_materialized_free_text_projection(
    baseline: &[crate::document_queue::QueueEntry],
    projected: &[crate::document_queue::QueueEntry],
    observed: &[crate::document_queue::QueueEntry],
    projected_backlog_text_by_id: &HashMap<String, String>,
) -> Option<MaterializedFreeTextReconciliation> {
    use crate::document_queue::QueueEntry;

    let mut baseline_id_counts = HashMap::<String, usize>::new();
    for id in baseline
        .iter()
        .filter_map(crate::queue_projection::queue_entry_do_id)
    {
        *baseline_id_counts.entry(id).or_default() += 1;
    }

    let mut projected_seen = HashMap::<String, usize>::new();
    let mut projected_ids = Vec::<String>::new();
    for id in projected
        .iter()
        .filter_map(crate::queue_projection::queue_entry_do_id)
    {
        let seen = projected_seen.entry(id.clone()).or_default();
        *seen += 1;
        if *seen > baseline_id_counts.get(&id).copied().unwrap_or_default() {
            projected_ids.push(id);
        }
    }
    if projected_ids.is_empty() {
        return None;
    }
    let projected_id_set = projected_ids.iter().cloned().collect::<HashSet<_>>();
    if projected_id_set.len() != projected_ids.len()
        || projected_id_set
            .iter()
            .any(|id| baseline_id_counts.contains_key(id))
    {
        return None;
    }

    // Establish the materialization receipt. Exact text is used only to bind
    // the pre-projection source node to the id minted from that same text; it is
    // never used to decide whether the later operator revision is "similar".
    let mut source_by_id = HashMap::<String, usize>::new();
    let mut claimed_sources = HashSet::<usize>::new();
    for id in &projected_ids {
        let backlog_text = projected_backlog_text_by_id.get(id)?;
        let candidates = baseline
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let QueueEntry::Prompt(prompt) = entry else {
                    return None;
                };
                (!claimed_sources.contains(&index)
                    && free_text_prompt_is_backlog_task(&prompt.text)
                    && normalize_admitted_free_text(&prompt.text)
                        == normalize_admitted_free_text(backlog_text))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let [source_index] = candidates.as_slice() else {
            return None;
        };
        claimed_sources.insert(*source_index);
        source_by_id.insert(id.clone(), *source_index);
    }

    let baseline_without_sources = baseline
        .iter()
        .enumerate()
        .filter(|(index, _)| !claimed_sources.contains(index))
        .map(|(_, entry)| entry.clone())
        .collect::<Vec<_>>();
    let projected_without_materialized_ids = projected
        .iter()
        .filter(|entry| {
            crate::queue_projection::queue_entry_do_id(entry)
                .is_none_or(|id| !projected_id_set.contains(&id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if projected_without_materialized_ids != baseline_without_sources {
        return None;
    }

    let mut remaining_projected_ids =
        projected_ids
            .iter()
            .fold(HashMap::<String, usize>::new(), |mut counts, id| {
                *counts.entry(id.clone()).or_default() += 1;
                counts
            });
    let mut observed_without_ids = Vec::<(usize, &QueueEntry)>::new();
    for (index, entry) in observed.iter().enumerate() {
        let remove_projected_id = crate::queue_projection::queue_entry_do_id(entry)
            .and_then(|id| remaining_projected_ids.get_mut(&id))
            .is_some_and(|remaining| {
                if *remaining == 0 {
                    false
                } else {
                    *remaining -= 1;
                    true
                }
            });
        if !remove_projected_id {
            observed_without_ids.push((index, entry));
        }
    }
    if remaining_projected_ids
        .values()
        .any(|remaining| *remaining != 0)
        || observed_without_ids.len() != baseline.len()
    {
        return None;
    }

    let id_by_source = source_by_id
        .into_iter()
        .map(|(id, source)| (source, id))
        .collect::<HashMap<_, _>>();
    let mut replay_rows = HashSet::<usize>::new();
    let mut backlog_edits = Vec::with_capacity(id_by_source.len());
    for (source_index, baseline_entry) in baseline.iter().enumerate() {
        let (observed_index, observed_entry) = observed_without_ids[source_index];
        let Some(id) = id_by_source.get(&source_index) else {
            if observed_entry != baseline_entry {
                return None;
            }
            continue;
        };
        let QueueEntry::Prompt(prompt) = observed_entry else {
            return None;
        };
        if !free_text_prompt_is_backlog_task(&prompt.text) {
            return None;
        }
        replay_rows.insert(observed_index);
        backlog_edits.push(MaterializedFreeTextRevision {
            id: id.clone(),
            text: normalize_admitted_free_text(&prompt.text),
        });
    }
    backlog_edits.sort_by(|left, right| left.id.cmp(&right.id));

    let queue_entries = observed
        .iter()
        .enumerate()
        .filter(|(index, _)| !replay_rows.contains(index))
        .map(|(_, entry)| entry.clone())
        .collect();
    Some(MaterializedFreeTextReconciliation {
        queue_entries,
        backlog_edits,
    })
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
    pub warnings: Vec<String>,
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
    let mut warnings = Vec::new();
    if !texts_to_add.is_empty() {
        let reserved =
            agent_doc_element_backlog::backlog::document_reserved_identity_ids(&current);
        let outcome = agent_doc_element_backlog::backlog::op_prepend_many_with_outcomes_reserved(
            &backlog_body,
            &texts_to_add,
            document_id,
            false,
            &reserved,
        )?;
        for item_outcome in outcome.outcomes {
            let key = crate::queue_response::normalize_for_answer_match(&item_outcome.text);
            id_by_text.insert(key, item_outcome.id.clone());
        }
        for failure in outcome.failures {
            warnings.push(format!(
                "skipped free-text queue item {:?}: {}",
                failure.text, failure.error
            ));
        }
        backlog_body = outcome.body;
    }
    if backlog_body != original_backlog_body {
        current = backlog.replace_content(&current, &backlog_body);
    }

    let mut unique_ids = Vec::new();
    let mut seen_ids = HashSet::new();
    for key in &prompt_keys {
        let Some(id) = id_by_text.get(key) else {
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
    let admitted_keys = prompt_keys
        .iter()
        .filter(|key| id_by_text.contains_key(*key))
        .cloned()
        .collect::<HashSet<_>>();
    let queue_entries = entries
        .iter()
        .filter(|entry| {
            if !queue_entry_is_admitted_free_text(entry, queue_scope) {
                return true;
            }
            let crate::document_queue::QueueEntry::Prompt(prompt) = entry else {
                return true;
            };
            let key = crate::queue_response::normalize_for_answer_match(
                &normalize_admitted_free_text(&prompt.text),
            );
            !admitted_keys.contains(&key)
        })
        .cloned()
        .collect();

    Ok(Some(PreparedFreeTextAdmission {
        content: current,
        unique_ids,
        admitted_count: admitted_keys.len(),
        warnings,
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
        Some(agent_doc_frontmatter::frontmatter::QueueControl::Pause) => false,
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
    fn materialized_projection_identity_is_idempotent_for_both_crdt_orders() {
        let old = "Re: Cross-platform mise bootstrap: Can this be run remotely via uvx?";
        let revised = concat!(
            "Re: Cross-platform mise bootstrap: Can this be run remotely via uvx...",
            "cross platform?",
        );
        let baseline =
            crate::document_queue::parse(&format!("- {old}\n- do [#neighbor]\n")).unwrap();
        let projected =
            crate::document_queue::parse("- do [#crossplatformmise]\n- do [#neighbor]\n").unwrap();
        let backlog = HashMap::from([("crossplatformmise".to_string(), old.to_string())]);

        for observed_body in [
            format!("- do [#crossplatformmise]\n- {revised}\n- do [#neighbor]\n"),
            format!("- {revised}\n- do [#crossplatformmise]\n- do [#neighbor]\n"),
        ] {
            let observed = crate::document_queue::parse(&observed_body).unwrap();
            let reconciled = reconcile_materialized_free_text_projection(
                &baseline, &projected, &observed, &backlog,
            )
            .expect("projection order must not change source identity");
            assert_eq!(reconciled.backlog_edits.len(), 1);
            assert_eq!(reconciled.backlog_edits[0].id, "crossplatformmise");
            assert_eq!(reconciled.backlog_edits[0].text, revised);
            assert_eq!(
                crate::document_queue::render(&reconciled.queue_entries),
                "- do [#crossplatformmise]\n- do [#neighbor]\n"
            );
            assert!(
                reconcile_materialized_free_text_projection(
                    &baseline,
                    &projected,
                    &reconciled.queue_entries,
                    &backlog,
                )
                .is_none(),
                "the projection join must be idempotent"
            );
        }
    }

    #[test]
    fn materialized_projection_identity_fails_closed_on_extra_authored_work() {
        let old = "Can this be run remotely via uvx?";
        let baseline = crate::document_queue::parse(&format!("- {old}\n")).unwrap();
        let projected = crate::document_queue::parse("- do [#crossplatformmise]\n").unwrap();
        let observed = crate::document_queue::parse(concat!(
            "- do [#crossplatformmise]\n",
            "- Can this be run remotely via uvx on every platform?\n",
            "- Publish an independent portability report.\n",
        ))
        .unwrap();
        let backlog = HashMap::from([("crossplatformmise".to_string(), old.to_string())]);

        assert!(
            reconcile_materialized_free_text_projection(
                &baseline, &projected, &observed, &backlog,
            )
            .is_none(),
            "cardinality growth is independent authoring, never a revision proof"
        );
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
    fn admission_keeps_fenced_backlog_examples_literal() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "~~~prompt\n",
            "Run Agent Doc on tools.md duplicated my queue item:\n",
            "```\n",
            "- do - do [#crossplatformmise]\n",
            "```\n",
            "```\n",
            "- [ ] [#crossplatformmise-txpw] Re: Cross-platform mise bootstrap\n",
            "- [ ] [#crossplatformmise] Re: Cross-platform mise bootstrap\n",
            "```\n",
            "~~~\n",
            "<!-- /agent:queue -->\n",
        );
        let entries = queue_entries_from_content(content);

        let prepared = prepare_free_text_admission(
            content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            false,
            "tools-doc",
        )
        .unwrap()
        .unwrap();
        let components = agent_doc_element::element::parse(&prepared.content).unwrap();
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .unwrap();
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(
            backlog.content(&prepared.content),
        );

        assert_eq!(prepared.admitted_count, 1);
        assert_eq!(items.len(), 1, "fenced examples became backlog siblings");
        assert_eq!(items[0].id, prepared.unique_ids[0]);
        assert!(items[0].continuation.contains("[#crossplatformmise-txpw]"));

        let admission = prepared
            .finish(FreeTextAdmissionExecution::Queue)
            .unwrap();
        let queue_entries = queue_entries_from_content(&admission.content);
        let queue_prompts = crate::document_queue::prompts(&queue_entries);
        assert_eq!(queue_prompts.len(), 1);
        assert_eq!(queue_prompts[0].text, format!("do [#{}]", items[0].id));
    }

    #[test]
    fn midline_existing_id_mention_is_coined_as_fresh_free_text_work() {
        let content = concat!(
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#existing] Existing work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- verify [#existing] yourself\n",
            "- Why is the sibling missing?\n",
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

        assert_eq!(prepared.admitted_count, 2);
        assert_eq!(prepared.unique_ids.len(), 2);
        assert!(prepared.warnings.is_empty(), "{:?}", prepared.warnings);
        assert!(!prepared.unique_ids.iter().any(|id| id == "existing"));
        assert!(prepared.content.contains("verify [#existing] yourself"));
        assert!(prepared.content.contains("Why is the sibling missing?"));

        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let queue_entries = queue_entries_from_content(&admission.content);
        assert!(!queue_entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Prompt(prompt)
                if prompt.text == "verify [#existing] yourself"
                    || prompt.text == "Why is the sibling missing?"
        )));
        assert!(crate::queue_response::queue_prompt_text_is_free_text(
            content,
            "verify [#existing] yourself"
        ));
    }

    #[test]
    fn queue_execution_discards_trailing_empty_editor_placeholder() {
        let prompt = "Can you run this on localhost so I can demo this?";
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- Can you run this on localhost so I can demo this?\n",
            "- \n",
            "<!-- /agent:queue -->\n",
            "\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#neighbor] Preserve neighboring work\n",
            "<!-- /agent:backlog -->\n",
        );
        let entries = queue_entries_from_content(content);
        assert_eq!(entries.len(), 1, "the empty marker is not a queue node");

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
        let id = prepared.unique_ids[0].clone();
        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let components = agent_doc_element::element::parse(&admission.content).unwrap();
        let queue = components
            .iter()
            .find(|component| component.name == "queue")
            .unwrap();
        let queue_body = queue.content(&admission.content);

        assert_eq!(queue_body, format!("- do [#{id}]\n"));
        assert!(admission.content.contains(prompt));
        assert!(
            admission
                .content
                .contains("- [ ] [#neighbor] Preserve neighboring work")
        );
        assert!(
            !queue_body.lines().any(|line| line.trim() == "-"),
            "Run Agent Doc must not project the editor placeholder as a bare dash"
        );
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

    fn backlog_items(content: &str) -> Vec<agent_doc_element_backlog::backlog::PendingItem> {
        let components = agent_doc_element::element::parse(content).unwrap();
        let backlog = components
            .iter()
            .find(|component| component.name == "backlog")
            .unwrap();
        agent_doc_element_backlog::backlog::parse_items(backlog.content(content)).1
    }

    const ABOUT_LINE: &str = "Add an `About Agent Doc` Editor action + menu item to show which version of Agent Doc + Plugin is running?";

    /// `#halftypedcoin` real case (tasks/agent-doc/agent-doc-bugs.md,
    /// 2026-10-04): the operator had just typed `- Add a ``` (the editor
    /// auto-paired the backticks) when queue maintenance coined it into a stub
    /// backlog id. The half-typed line must stay free text; the finished line
    /// beside it is still coined.
    #[test]
    fn half_typed_code_span_queue_line_is_not_coined() {
        assert!(!free_text_prompt_is_backlog_task("Add a ``"));
        assert!(free_text_prompt_is_backlog_task(ABOUT_LINE));

        let content =
            format!("<!-- agent:queue -->\n- {ABOUT_LINE}\n- Add a ``\n<!-- /agent:queue -->\n");
        let entries = queue_entries_from_content(&content);
        let prepared = prepare_free_text_admission(
            &content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            false,
            "doc-id",
        )
        .unwrap()
        .unwrap();
        assert_eq!(prepared.admitted_count, 1);
        assert_eq!(prepared.unique_ids.len(), 1);

        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let items = backlog_items(&admission.content);
        assert_eq!(items.len(), 1, "{items:#?}");
        assert_eq!(items[0].text, ABOUT_LINE);
        let queue_entries = queue_entries_from_content(&admission.content);
        assert!(
            queue_entries.iter().any(|entry| matches!(
                entry,
                crate::document_queue::QueueEntry::Prompt(prompt) if prompt.text == "Add a ``"
            )),
            "the half-typed line must stay as free text:\n{}",
            admission.content
        );
    }

    /// A finished line is still coined, with or without typing evidence, once
    /// it has been quiet for the window.
    #[test]
    fn complete_queue_line_is_still_coined() {
        let content = format!("<!-- agent:queue -->\n- {ABOUT_LINE}\n<!-- /agent:queue -->\n");
        let entries = queue_entries_from_content(&content);
        let settled = FreeTextCoinGate {
            observed: Some(FreeTextCoinObservation {
                settled_text: content.clone(),
                quiet_for_ms: 2_500,
            }),
            debounce_ms: 2_000,
            max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
        };
        for gate in [FreeTextCoinGate::structural_only(), settled] {
            let (scope, held) =
                gate_free_text_admission_scope(FreeTextAdmissionScope::All, &entries, &gate);
            assert!(held.is_empty(), "{gate:?}: {held:?}");
            let prepared =
                prepare_free_text_admission(&content, &entries, None, &scope, false, "doc-id")
                    .unwrap()
                    .expect("a finished, settled line is coined");
            assert_eq!(prepared.unique_ids.len(), 1);
        }
    }

    /// The typing-gate hold defers coining: a line that changed after the
    /// quiescence wait settled (absent from the settled text) or that has not
    /// been quiet for the window is held as free text this pass. A plainly
    /// unfinished line is never coined, even past the max-hold.
    #[test]
    fn typing_gate_hold_defers_coining() {
        let line = "Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard";
        let content = format!("<!-- agent:queue -->\n- {line}\n<!-- /agent:queue -->\n");
        let entries = queue_entries_from_content(&content);
        let gate = |settled_text: String, quiet_for_ms: u64| FreeTextCoinGate {
            observed: Some(FreeTextCoinObservation {
                settled_text,
                quiet_for_ms,
            }),
            debounce_ms: 2_000,
            max_hold_ms: agent_doc_debounce::edit_settle::DEFAULT_MAX_HOLD_MS,
        };

        // The wait settled on the half-typed draft; the operator kept typing.
        let typed_after_settle = gate(
            "<!-- agent:queue -->\n- Add a ``\n<!-- /agent:queue -->\n".to_string(),
            2_400,
        );
        let (scope, held) = gate_free_text_admission_scope(
            FreeTextAdmissionScope::All,
            &entries,
            &typed_after_settle,
        );
        assert_eq!(held, vec![line.to_string()]);
        assert!(
            prepare_free_text_admission(&content, &entries, None, &scope, false, "doc-id")
                .unwrap()
                .is_none(),
            "a line still being typed must not be coined"
        );

        // Present in the settled text but inside the hold window.
        let recent = gate(content.clone(), 300);
        let (_, held) =
            gate_free_text_admission_scope(FreeTextAdmissionScope::All, &entries, &recent);
        assert_eq!(held.len(), 1);

        // Quiet past the window: coined.
        let quiet = gate(content.clone(), 2_100);
        let (scope, held) =
            gate_free_text_admission_scope(FreeTextAdmissionScope::All, &entries, &quiet);
        assert!(held.is_empty());
        assert!(
            prepare_free_text_admission(&content, &entries, None, &scope, false, "doc-id")
                .unwrap()
                .is_some()
        );

        // Unfinished text is never coined, even past the max-hold.
        let stale = gate("- Add a ``\n".to_string(), 600_000);
        assert!(!stale.allows_coining("Add a ``"));
        assert!(!FreeTextCoinGate::structural_only().allows_coining("Should we publish the"));
    }

    /// A stub coined from a half-typed line before this fix (`[#gvqv] Add a
    /// ```) absorbs the operator's continued line beside its `do` head instead
    /// of leaving a meaningless stub plus a duplicate free-text head, even
    /// though the stub is below the strict-prefix length floor.
    #[test]
    fn half_typed_stub_absorbs_its_continued_line() {
        let continued = "Add a `Dashboard` editor prompt + action to show the Agent Doc dashboard. Can we also support a dashboard md file that live updates?";
        let content = format!(
            "<!-- agent:backlog priority queue -->\n- [ ] [#gvqv] Add a ``\n<!-- /agent:backlog -->\n\n<!-- agent:queue go -->\n- do [#gvqv]\n- {continued}\n<!-- /agent:queue -->\n"
        );
        let entries = queue_entries_from_content(&content);
        let prepared = prepare_free_text_admission(
            &content,
            &entries,
            None,
            &FreeTextAdmissionScope::All,
            false,
            "doc-id",
        )
        .unwrap()
        .unwrap();
        assert_eq!(prepared.unique_ids, vec!["gvqv".to_string()]);

        let admission = prepared.finish(FreeTextAdmissionExecution::Queue).unwrap();
        let items = backlog_items(&admission.content);
        assert_eq!(items.len(), 1, "{items:#?}");
        assert_eq!(items[0].id, "gvqv");
        assert_eq!(items[0].text, continued);
        let queue_entries = queue_entries_from_content(&admission.content);
        assert_eq!(
            queue_entries
                .iter()
                .filter_map(crate::queue_projection::queue_entry_do_id)
                .filter(|id| id == "gvqv")
                .count(),
            1,
            "{}",
            admission.content
        );
        assert!(!admission.content.contains(&format!("- {continued}\n")));
    }
}
