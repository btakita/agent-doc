use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExecutionScope {
    Normal,
    PlanBacklogOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizePendingMutationKind {
    ResolveExisting,
    ExpectAdd,
}

#[derive(Debug, Clone, Copy)]
pub struct FinalizePendingMutation<'a> {
    pub kind: FinalizePendingMutationKind,
    pub id: &'a str,
    pub target_files: &'a [String],
}

pub fn prompt_targets_from_changes(changes: &[agent_doc_diff::PromptBearingChange]) -> Vec<String> {
    changes
        .iter()
        .filter(|change| change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| change.text.clone())
        .collect()
}

pub fn prompt_targets_from_diff(diff_text: &str) -> Vec<String> {
    let mut targets: Vec<String> =
        prompt_targets_from_changes(&agent_doc_diff::classify_prompt_bearing_changes(diff_text))
            .into_iter()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect();

    if targets.is_empty() {
        for directive in agent_doc_diff::extract_imperative_directives(diff_text) {
            if !targets.iter().any(|existing| existing == &directive) {
                targets.push(directive);
            }
        }
    }

    targets
}

/// Compute prompt changes that represent fresh user intent for the active turn.
///
/// Synthetic queue continuations and prompt-bearing changes classified as
/// independent of the turn must not preempt an active queue drain. When no
/// affectedness classifier ran, this falls back to filtering managed component
/// bookkeeping only.
pub fn compute_user_intent_prompt_changes(
    prompt_bearing_changes: &[agent_doc_diff::PromptBearingChange],
    diff_from_queue_head_only: bool,
    op_affectedness: Option<&agent_doc_turn::turn_scope::CycleAffectedness>,
) -> Vec<agent_doc_diff::PromptBearingChange> {
    if diff_from_queue_head_only {
        return Vec::new();
    }
    if op_affectedness.is_some_and(|affectedness| {
        !affectedness.turn_affected && !affectedness.classified.is_empty()
    }) {
        return Vec::new();
    }
    prompt_bearing_changes
        .iter()
        .filter(|change| !agent_doc_diff::change_is_managed_state_only(change))
        .cloned()
        .collect()
}

/// Derive the turn-scope manifest for prompts answered by this cycle.
///
/// The queue driver is resolved from prompt target ids, while the exchange tail
/// floor preserves old committed exchange bullets as independent context.
pub fn derive_turn_scope(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_turn::turn_scope::TurnScope> {
    if prompt_targets.is_empty() {
        return None;
    }
    let driver = resolve_driver_address(content, prompt_targets);
    let exchange_tail_floor = exchange_node_count(content);
    Some(
        agent_doc_turn::turn_scope::TurnScope::for_driver_with_exchange_tail(
            driver,
            exchange_tail_floor,
        ),
    )
}

/// Count exchange item nodes present at turn start.
pub fn exchange_node_count(content: &str) -> Option<usize> {
    let count = agent_doc_markdown_ast::mutations::all_item_nodes(content)
        .iter()
        .filter(|node| node.component == "exchange")
        .count();
    (count > 0).then_some(count)
}

/// Find the queue item node a prompt target refers to and address it.
pub fn resolve_driver_address(
    content: &str,
    prompt_targets: &[String],
) -> Option<agent_doc_turn::turn_scope::Address> {
    let nodes = agent_doc_markdown_ast::mutations::all_item_nodes(content);
    for target in prompt_targets {
        let Some(id) = extract_target_id(target) else {
            continue;
        };
        if let Some(node) = nodes
            .iter()
            .find(|node| node.component == "queue" && node.item.id == id)
        {
            let occurrence = component_occurrence_from_node_key(&node.node_key);
            return Some(agent_doc_turn::turn_scope::Address::node(
                "queue",
                occurrence,
                &node.node_key,
            ));
        }
    }
    None
}

/// Extract a backlog/queue id (`[#id]` or bare `#id`) from a prompt target.
pub fn extract_target_id(target: &str) -> Option<String> {
    if let Some(start) = target.find("[#") {
        let rest = &target[start + 2..];
        if let Some(close) = rest.find(']') {
            let id = &rest[..close];
            if agent_doc_element_backlog::backlog::is_valid_pending_id(id) {
                return Some(id.to_string());
            }
        }
    }
    if let Some(start) = target.find('#') {
        let rest = &target[start + 1..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !id.is_empty() && agent_doc_element_backlog::backlog::is_valid_pending_id(&id) {
            return Some(id);
        }
    }
    None
}

/// Component occurrence index encoded in a node key (`component:index:id:dup`).
pub fn component_occurrence_from_node_key(node_key: &str) -> usize {
    node_key
        .split(':')
        .nth(1)
        .and_then(|field| field.parse().ok())
        .unwrap_or(0)
}

pub fn classify_execution_scope(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    harness_prompt_only: bool,
    prompt_presets: &IndexMap<String, String>,
) -> SessionExecutionScope {
    let agent_doc_bug_requested =
        prompt_targets_reference_preset(prompt_targets, prompt_presets, "#agent-doc-bug")
            || harness_prompt_only
                && prompt_targets_reference_preset(
                    added_diff_lines,
                    prompt_presets,
                    "#agent-doc-bug",
                );
    if agent_doc_bug_requested {
        SessionExecutionScope::PlanBacklogOnly
    } else {
        SessionExecutionScope::Normal
    }
}

pub fn finalize_command(
    file: &Path,
    mode: agent_doc_frontmatter::frontmatter::ResolvedMode,
    pending_mutations: &[FinalizePendingMutation<'_>],
) -> String {
    let mut finalize = format!(
        "agent-doc finalize {} --baseline-file <preflight.baseline_file> --origin skill",
        file.display()
    );
    for mutation in pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == FinalizePendingMutationKind::ResolveExisting)
    {
        finalize.push_str(" --done ");
        finalize.push_str(mutation.id);
    }
    for mutation in pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == FinalizePendingMutationKind::ExpectAdd)
    {
        for target in mutation.target_files {
            finalize.push_str(" --pending-add-to ");
            finalize.push_str(target);
            finalize.push_str(" \"<item>\"");
        }
    }
    if mode.is_crdt() {
        finalize.push_str(" --stream");
    } else if mode.is_template() {
        finalize.push_str(" --template");
    }
    finalize
}

fn prompt_targets_reference_preset(
    prompt_targets: &[String],
    prompt_presets: &IndexMap<String, String>,
    preset_name: &str,
) -> bool {
    effective_prompt_texts(prompt_targets, &[], prompt_presets)
        .iter()
        .any(|text| {
            referenced_presets_in_text(text, prompt_presets)
                .iter()
                .any(|preset| preset == preset_name)
        })
}

fn effective_prompt_texts(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_presets: &IndexMap<String, String>,
) -> Vec<String> {
    let mut queue = prompt_targets.iter().cloned().collect::<VecDeque<_>>();
    queue.extend(added_diff_lines.iter().cloned());
    let mut seen_presets = HashSet::new();
    let mut texts = Vec::new();

    while let Some(text) = queue.pop_front() {
        let text = without_prompt_preset_definition_lines(&text, prompt_presets);
        if text.trim().is_empty() {
            continue;
        }

        texts.push(text.clone());
        for preset in referenced_presets_in_text(&text, prompt_presets) {
            if seen_presets.insert(preset.clone())
                && let Some(body) = prompt_presets.get(&preset)
            {
                queue.push_back(body.clone());
            }
        }
    }

    texts
}

fn referenced_presets_in_text(
    text: &str,
    prompt_presets: &IndexMap<String, String>,
) -> Vec<String> {
    let mut referenced = Vec::new();

    for line in text.lines() {
        if line_defines_prompt_preset(line, prompt_presets) {
            continue;
        }

        for preset in agent_doc_diff::extract_prompt_preset_requests_from_text(line) {
            if let Some(preset) = agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(
                prompt_presets,
                &preset,
            ) && !referenced.iter().any(|existing| existing == &preset)
            {
                referenced.push(preset);
            }
        }

        for token in extract_hashtag_tokens(line) {
            if prompt_presets.contains_key(token.as_str())
                && !referenced.iter().any(|existing| existing == &token)
            {
                referenced.push(token);
            }
        }
    }

    referenced
}

fn without_prompt_preset_definition_lines(
    text: &str,
    prompt_presets: &IndexMap<String, String>,
) -> String {
    text.lines()
        .filter(|line| !line_defines_prompt_preset(line, prompt_presets))
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_defines_prompt_preset(line: &str, prompt_presets: &IndexMap<String, String>) -> bool {
    let trimmed = line.trim_start();
    prompt_presets
        .keys()
        .any(|preset| line_starts_with_yaml_key(trimmed, preset))
}

fn line_starts_with_yaml_key(line: &str, key: &str) -> bool {
    if let Some(rest) = line.strip_prefix(key) {
        return rest.trim_start().starts_with(':');
    }

    for quote in ['\'', '"'] {
        let Some(rest) = line.strip_prefix(quote) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(quote) else {
            continue;
        };
        if rest.trim_start().starts_with(':') {
            return true;
        }
    }

    false
}

fn extract_hashtag_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let chars = text.char_indices().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        let (byte_idx, ch) = chars[idx];
        if ch != '#' {
            idx += 1;
            continue;
        }

        let start = byte_idx;
        let mut end = start + ch.len_utf8();
        let mut cursor = idx + 1;
        while cursor < chars.len() {
            let (next_byte, next_ch) = chars[cursor];
            if next_ch.is_ascii_alphanumeric() || next_ch == '-' || next_ch == '_' {
                end = next_byte + next_ch.len_utf8();
                cursor += 1;
                continue;
            }
            break;
        }

        if end > start + 1 {
            let token = text[start..end].to_string();
            if !tokens.iter().any(|existing| existing == &token) {
                tokens.push(token);
            }
        }
        idx = cursor;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt_change(text: &str) -> agent_doc_diff::PromptBearingChange {
        agent_doc_diff::PromptBearingChange {
            kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
            text: text.to_string(),
        }
    }

    fn affectedness(turn_affected: bool) -> agent_doc_turn::turn_scope::CycleAffectedness {
        use agent_doc_turn::turn_scope::{AffectednessClass, ClassifiedOp};
        agent_doc_turn::turn_scope::CycleAffectedness {
            turn_affected,
            classified: vec![ClassifiedOp {
                component: "queue".to_string(),
                node_key: "queue:0:other:0".to_string(),
                op_kind: "move".to_string(),
                actor: agent_doc_turn::op_log::OpActor::User,
                class: if turn_affected {
                    AffectednessClass::InputAffecting
                } else {
                    AffectednessClass::Independent
                },
            }],
        }
    }

    #[test]
    fn prompt_targets_keep_prompt_target_order() {
        let changes = vec![prompt_change("first"), prompt_change("second")];

        assert_eq!(
            prompt_targets_from_changes(&changes),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn prompt_targets_from_diff_trims_and_drops_empty_targets() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            The prior explanation was incomplete\n\
            +  Why was the prompt prefix omitted here?  \n\
            The rest of the response stays the same\n";

        assert_eq!(
            prompt_targets_from_diff(diff),
            vec!["Why was the prompt prefix omitted here?".to_string()]
        );
    }

    #[test]
    fn prompt_targets_from_diff_includes_imperative_directives() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n\
            +run benchmarks\n";

        assert_eq!(
            prompt_targets_from_diff(diff),
            vec!["run benchmarks".to_string()]
        );
    }

    #[test]
    fn derive_turn_scope_resolves_queue_driver_and_sets() {
        let content = "<!-- agent:queue -->\n- do [#op-scoped-drift-2]\n- do [#later]\n<!-- /agent:queue -->\n";
        let targets = vec!["do [#op-scoped-drift-2]".to_string()];
        let scope = derive_turn_scope(content, &targets).expect("scope derived");
        let driver = scope.driver.as_ref().expect("driver resolved");
        assert_eq!(driver.component, "queue");
        assert_eq!(
            driver.node_key.as_deref(),
            Some("queue:0:op-scoped-drift-2:0")
        );
        assert!(scope.read_set.contains(driver));
        assert!(scope.write_set.contains(driver));
        for &component in agent_doc_turn::turn_scope::MANAGED_OUTPUT_COMPONENTS {
            assert!(
                scope
                    .write_set
                    .contains(&agent_doc_turn::turn_scope::Address::component(
                        component, 0
                    )),
                "turn scope write set should include managed output component {component}"
            );
        }
    }

    #[test]
    fn derive_turn_scope_sets_exchange_tail_floor() {
        let content = "\
<!-- agent:exchange -->
- old context bullet one
- old context bullet two
<!-- /agent:exchange -->

<!-- agent:queue -->
- do [#driver]
<!-- /agent:queue -->
";
        let targets = vec!["do [#driver]".to_string()];
        let scope = derive_turn_scope(content, &targets).expect("scope derived");

        assert_eq!(scope.exchange_tail_floor, Some(2));
    }

    #[test]
    fn derive_turn_scope_none_without_prompt_targets() {
        let content = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
        assert!(derive_turn_scope(content, &[]).is_none());
    }

    #[test]
    fn derive_turn_scope_without_matching_queue_node_has_no_driver() {
        let content = "<!-- agent:queue -->\n- do [#present]\n<!-- /agent:queue -->\n";
        let targets = vec!["do [#absent]".to_string()];
        let scope = derive_turn_scope(content, &targets).expect("scope derived");

        assert!(scope.driver.is_none());
        assert!(scope.write_set.iter().all(|a| a.component != "queue"));
    }

    #[test]
    fn extract_target_id_handles_bracket_and_bare_forms() {
        assert_eq!(
            extract_target_id("do [#op-scoped-drift-2]").as_deref(),
            Some("op-scoped-drift-2")
        );
        assert_eq!(extract_target_id("do #fix1").as_deref(), Some("fix1"));
        assert_eq!(extract_target_id("no id here"), None);
    }

    #[test]
    fn user_intent_empty_for_synthetic_queue_continuation() {
        let changes = vec![prompt_change("do [#next]")];
        assert!(
            compute_user_intent_prompt_changes(&changes, true, Some(&affectedness(true)))
                .is_empty()
        );
    }

    #[test]
    fn user_intent_drops_turn_independent_edits() {
        let changes = vec![prompt_change("a stray note in the parking lot")];
        let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(false)));

        assert!(
            out.is_empty(),
            "independent edit should not preempt: {out:?}"
        );
    }

    #[test]
    fn user_intent_keeps_turn_affecting_prompt() {
        let changes = vec![prompt_change("please also handle the error case")];
        let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));

        assert_eq!(out.len(), 1, "turn-affecting prompt must preempt");
    }

    #[test]
    fn user_intent_filters_managed_state_when_turn_affected() {
        let changes = vec![agent_doc_diff::PromptBearingChange {
            kind: agent_doc_diff::PromptBearingChangeKind::ContentEdit,
            text: "- [ ] [#newitem] track a follow-up".to_string(),
        }];
        let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));

        assert!(
            out.is_empty(),
            "managed-state edit must not preempt: {out:?}"
        );
    }

    #[test]
    fn user_intent_conservative_without_classifier() {
        let changes = vec![prompt_change("a real prompt with no classifier")];
        let out = compute_user_intent_prompt_changes(&changes, false, None);

        assert_eq!(out.len(), 1, "without classifier, a real change preempts");
    }

    #[test]
    fn classify_execution_scope_keeps_agent_doc_bug_plan_only() {
        let mut presets = IndexMap::new();
        presets.insert(
            "#agent-doc-bug".to_string(),
            "Please create a plan.".to_string(),
        );

        let scope = classify_execution_scope(
            &["#agent-doc-bug\nBroken flow".to_string()],
            &[],
            false,
            &presets,
        );

        assert_eq!(scope, SessionExecutionScope::PlanBacklogOnly);
    }

    #[test]
    fn classify_execution_scope_uses_harness_only_added_diff_lines() {
        let mut presets = IndexMap::new();
        presets.insert(
            "#agent-doc-bug".to_string(),
            "Please create a plan.".to_string(),
        );

        assert_eq!(
            classify_execution_scope(
                &[],
                &["Please inspect #agent-doc-bug".to_string()],
                true,
                &presets,
            ),
            SessionExecutionScope::PlanBacklogOnly
        );
        assert_eq!(
            classify_execution_scope(
                &[],
                &["Please inspect #agent-doc-bug".to_string()],
                false,
                &presets,
            ),
            SessionExecutionScope::Normal
        );
    }

    #[test]
    fn finalize_command_includes_done_flags_and_stream_mode() {
        let targets = vec!["tasks/bugs.md".to_string()];
        let pending = vec![
            FinalizePendingMutation {
                kind: FinalizePendingMutationKind::ResolveExisting,
                id: "abc1",
                target_files: &[],
            },
            FinalizePendingMutation {
                kind: FinalizePendingMutationKind::ExpectAdd,
                id: "",
                target_files: &targets,
            },
        ];
        let mode = agent_doc_frontmatter::frontmatter::ResolvedMode {
            format: agent_doc_frontmatter::frontmatter::AgentDocFormat::Template,
            write: agent_doc_frontmatter::frontmatter::AgentDocWrite::Crdt,
        };

        let command = finalize_command(Path::new("tasks/doc.md"), mode, &pending);

        assert!(command.contains("--done abc1"));
        assert!(command.contains("--pending-add-to tasks/bugs.md \"<item>\""));
        assert!(command.ends_with(" --stream"));
    }
}
