use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy)]
pub struct FinalizeRerunCommand<'a> {
    pub required_commit: bool,
    pub file: &'a Path,
    pub is_template: bool,
    pub is_stream: bool,
    pub is_ipc: bool,
    pub force_disk: bool,
    pub origin: Option<&'a str>,
    pub no_pending_capture: bool,
    pub pending_add: &'a [String],
    pub pending_add_to: &'a [String],
    pub pending_add_gated: &'a [String],
    pub pending_add_after: &'a [String],
    pub pending_add_before: &'a [String],
    pub pending_add_back: &'a [String],
    pub icebox_add: &'a [String],
    pub icebox_add_after: &'a [String],
    pub icebox_add_before: &'a [String],
    pub icebox_add_back: &'a [String],
    pub pending_done: &'a [String],
    pub pending_edit: &'a [String],
    pub pending_clear: bool,
    pub pending_reorder: Option<&'a str>,
    pub pending_gate: &'a [String],
    pub pending_ungate: &'a [String],
    pub pending_resolve_gate: &'a [String],
    pub pending_set_gate_type: &'a [String],
    pub pending_set_verify: &'a [String],
    pub review_add: &'a [String],
    pub review_edit: &'a [String],
    pub allow_replace_pending: bool,
    pub pending_only: bool,
    pub status: Option<&'a str>,
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
    let mut finalize = format!("agent-doc finalize {} --origin skill", file.display());
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

pub fn shell_quote_cli_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\"'\"'"))
}

pub fn finalize_rerun_command_base(command: FinalizeRerunCommand<'_>) -> Option<String> {
    if !command.required_commit {
        return None;
    }

    let mut args = vec!["agent-doc".to_string(), "finalize".to_string()];
    args.push(command.file.display().to_string());
    if command.is_template {
        args.push("--template".to_string());
    }
    if command.is_stream {
        args.push("--stream".to_string());
    }
    if command.is_ipc {
        args.push("--ipc".to_string());
    }
    if command.force_disk {
        args.push("--force-disk".to_string());
    }
    if let Some(origin) = command.origin {
        push_arg(&mut args, "--origin", origin);
    }
    if command.no_pending_capture {
        args.push("--no-followups".to_string());
    }
    push_repeated_args(&mut args, "--backlog-add", command.pending_add);
    push_repeated_pair_args(&mut args, "--backlog-add-to", command.pending_add_to);
    push_repeated_args(&mut args, "--backlog-add-gated", command.pending_add_gated);
    push_repeated_pair_args(&mut args, "--backlog-add-after", command.pending_add_after);
    push_repeated_pair_args(
        &mut args,
        "--backlog-add-before",
        command.pending_add_before,
    );
    push_repeated_args(&mut args, "--backlog-add-back", command.pending_add_back);
    push_repeated_args(&mut args, "--icebox-add", command.icebox_add);
    push_repeated_pair_args(&mut args, "--icebox-add-after", command.icebox_add_after);
    push_repeated_pair_args(&mut args, "--icebox-add-before", command.icebox_add_before);
    push_repeated_args(&mut args, "--icebox-add-back", command.icebox_add_back);
    push_repeated_args(&mut args, "--done", command.pending_done);
    push_repeated_args(&mut args, "--backlog-edit", command.pending_edit);
    if command.pending_clear {
        args.push("--backlog-clear".to_string());
    }
    if let Some(value) = command.pending_reorder {
        push_arg(&mut args, "--backlog-reorder", value);
    }
    push_repeated_args(&mut args, "--backlog-gate", command.pending_gate);
    push_repeated_args(&mut args, "--backlog-ungate", command.pending_ungate);
    push_repeated_args(
        &mut args,
        "--backlog-resolve-gate",
        command.pending_resolve_gate,
    );
    push_repeated_args(
        &mut args,
        "--backlog-set-gate-type",
        command.pending_set_gate_type,
    );
    push_repeated_args(
        &mut args,
        "--backlog-set-verify",
        command.pending_set_verify,
    );
    push_repeated_args(&mut args, "--review-add", command.review_add);
    push_repeated_args(&mut args, "--review-edit", command.review_edit);
    if command.allow_replace_pending {
        args.push("--allow-replace-pending".to_string());
    }
    if command.pending_only {
        args.push("--backlog-only".to_string());
    }
    if let Some(status) = command.status {
        push_arg(&mut args, "--status", status);
    }
    Some(render_cli_command(args))
}

pub fn compact_command_hint(file: &Path) -> String {
    format!("agent-doc compact {} --commit", file.display())
}

pub fn pending_kept_open_ids_from_mutations(
    pending_edit: &[String],
    pending_gate: &[String],
    pending_ungate: &[String],
    pending_set_gate_type: &[String],
    pending_set_verify: &[String],
    review_edit: &[String],
    pending_reorder: Option<&str>,
) -> Vec<String> {
    let mut ids = Vec::new();

    for pair in pending_edit {
        push_assignment_id(&mut ids, pair);
    }
    ids.extend(pending_gate.iter().cloned());
    ids.extend(pending_ungate.iter().cloned());
    for pair in pending_set_gate_type {
        push_assignment_id(&mut ids, pair);
    }
    for pair in pending_set_verify {
        push_assignment_id(&mut ids, pair);
    }
    for pair in review_edit {
        push_assignment_id(&mut ids, pair);
    }
    if let Some(order) = pending_reorder {
        ids.extend(parse_id_order(order));
    }

    ids
}

pub fn group_pending_add_targets(raw: &[String]) -> Result<Vec<(PathBuf, Vec<String>)>> {
    if !raw.len().is_multiple_of(2) {
        anyhow::bail!("--backlog-add-to expects repeated FILE TEXT pairs");
    }

    let mut grouped: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for pair in raw.chunks(2) {
        let target = PathBuf::from(&pair[0]);
        let text = pair[1].clone();
        if let Some((_, items)) = grouped.iter_mut().find(|(existing, _)| existing == &target) {
            items.push(text);
        } else {
            grouped.push((target, vec![text]));
        }
    }
    Ok(grouped)
}

pub fn parse_id_order(order: &str) -> Vec<String> {
    order
        .split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn parse_tracked_work_edits(raw: &[String], flag: &str) -> Result<Vec<(String, String)>> {
    raw.iter()
        .map(|pair| {
            let (id, text) = pair
                .split_once('=')
                .with_context(|| format!("{flag} expects 'id=text', got: {pair}"))?;
            Ok((id.to_string(), text.to_string()))
        })
        .collect()
}

fn push_assignment_id(ids: &mut Vec<String>, pair: &str) {
    if let Some((id, _)) = pair.split_once('=') {
        ids.push(id.to_string());
    }
}

fn push_arg(args: &mut Vec<String>, flag: &str, value: &str) {
    args.push(flag.to_string());
    args.push(value.to_string());
}

fn push_repeated_args(args: &mut Vec<String>, flag: &str, values: &[String]) {
    for value in values {
        push_arg(args, flag, value);
    }
}

fn push_repeated_pair_args(args: &mut Vec<String>, flag: &str, values: &[String]) {
    for pair in values.chunks(2) {
        if let [first, second] = pair {
            args.push(flag.to_string());
            args.push(first.clone());
            args.push(second.clone());
        }
    }
}

fn render_cli_command(args: Vec<String>) -> String {
    args.into_iter()
        .map(|arg| shell_quote_cli_arg(&arg))
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn shell_quote_cli_arg_quotes_only_when_needed() {
        assert_eq!(shell_quote_cli_arg("tasks/doc.md"), "tasks/doc.md");
        assert_eq!(shell_quote_cli_arg(""), "''");
        assert_eq!(shell_quote_cli_arg("two words"), "'two words'");
        assert_eq!(shell_quote_cli_arg("it'll"), "'it'\"'\"'ll'");
    }

    #[test]
    fn finalize_rerun_command_base_is_only_for_required_commit_closeout() {
        let empty = Vec::new();
        let command = FinalizeRerunCommand {
            required_commit: false,
            file: Path::new("tasks/doc.md"),
            is_template: false,
            is_stream: false,
            is_ipc: false,
            force_disk: false,
            origin: None,
            no_pending_capture: false,
            pending_add: &empty,
            pending_add_to: &empty,
            pending_add_gated: &empty,
            pending_add_after: &empty,
            pending_add_before: &empty,
            pending_add_back: &empty,
            icebox_add: &empty,
            icebox_add_after: &empty,
            icebox_add_before: &empty,
            icebox_add_back: &empty,
            pending_done: &empty,
            pending_edit: &empty,
            pending_clear: false,
            pending_reorder: None,
            pending_gate: &empty,
            pending_ungate: &empty,
            pending_resolve_gate: &empty,
            pending_set_gate_type: &empty,
            pending_set_verify: &empty,
            review_add: &empty,
            review_edit: &empty,
            allow_replace_pending: false,
            pending_only: false,
            status: None,
        };

        assert_eq!(finalize_rerun_command_base(command), None);
    }

    #[test]
    fn finalize_rerun_command_base_renders_flags_and_quotes_values() {
        let pending_add = vec!["item with spaces".to_string()];
        let pending_add_to = vec!["tasks/other.md".to_string(), "target item".to_string()];
        let pending_add_gated = vec!["gated".to_string()];
        let pending_add_after = vec!["anchor".to_string(), "after item".to_string()];
        let pending_add_before = vec!["anchor".to_string(), "before item".to_string()];
        let pending_add_back = vec!["tail item".to_string()];
        let icebox_add = vec!["ice item".to_string()];
        let icebox_add_after = vec!["ice-anchor".to_string(), "ice after".to_string()];
        let icebox_add_before = vec!["ice-anchor".to_string(), "ice before".to_string()];
        let icebox_add_back = vec!["ice tail".to_string()];
        let pending_done = vec!["done1".to_string()];
        let pending_edit = vec!["edit1=new text".to_string()];
        let pending_gate = vec!["gate1".to_string()];
        let pending_ungate = vec!["ungate1".to_string()];
        let pending_resolve_gate = vec!["manual".to_string()];
        let pending_set_gate_type = vec!["gate1=blocked".to_string()];
        let pending_set_verify = vec!["gate1=test".to_string()];
        let review_add = vec!["review me".to_string()];
        let review_edit = vec!["review1=fix it".to_string()];
        let command = FinalizeRerunCommand {
            required_commit: true,
            file: Path::new("tasks/doc.md"),
            is_template: true,
            is_stream: true,
            is_ipc: true,
            force_disk: true,
            origin: Some("skill"),
            no_pending_capture: true,
            pending_add: &pending_add,
            pending_add_to: &pending_add_to,
            pending_add_gated: &pending_add_gated,
            pending_add_after: &pending_add_after,
            pending_add_before: &pending_add_before,
            pending_add_back: &pending_add_back,
            icebox_add: &icebox_add,
            icebox_add_after: &icebox_add_after,
            icebox_add_before: &icebox_add_before,
            icebox_add_back: &icebox_add_back,
            pending_done: &pending_done,
            pending_edit: &pending_edit,
            pending_clear: true,
            pending_reorder: Some("done1,gate1"),
            pending_gate: &pending_gate,
            pending_ungate: &pending_ungate,
            pending_resolve_gate: &pending_resolve_gate,
            pending_set_gate_type: &pending_set_gate_type,
            pending_set_verify: &pending_set_verify,
            review_add: &review_add,
            review_edit: &review_edit,
            allow_replace_pending: true,
            pending_only: true,
            status: Some("working hard"),
        };

        let rendered = finalize_rerun_command_base(command).unwrap();

        assert!(rendered.starts_with("agent-doc finalize tasks/doc.md"));
        assert!(!rendered.contains("--baseline-file"));
        assert!(rendered.contains("--template --stream --ipc --force-disk"));
        assert!(rendered.contains("--origin skill"));
        assert!(rendered.contains("--no-followups"));
        assert!(rendered.contains("--backlog-add 'item with spaces'"));
        assert!(rendered.contains("--backlog-add-to tasks/other.md 'target item'"));
        assert!(rendered.contains("--backlog-add-gated gated"));
        assert!(rendered.contains("--backlog-add-after anchor 'after item'"));
        assert!(rendered.contains("--backlog-add-before anchor 'before item'"));
        assert!(rendered.contains("--backlog-add-back 'tail item'"));
        assert!(rendered.contains("--icebox-add 'ice item'"));
        assert!(rendered.contains("--icebox-add-after ice-anchor 'ice after'"));
        assert!(rendered.contains("--icebox-add-before ice-anchor 'ice before'"));
        assert!(rendered.contains("--icebox-add-back 'ice tail'"));
        assert!(rendered.contains("--done done1"));
        assert!(rendered.contains("--backlog-edit 'edit1=new text'"));
        assert!(rendered.contains("--backlog-clear"));
        assert!(rendered.contains("--backlog-reorder 'done1,gate1'"));
        assert!(rendered.contains("--backlog-gate gate1"));
        assert!(rendered.contains("--backlog-ungate ungate1"));
        assert!(rendered.contains("--backlog-resolve-gate manual"));
        assert!(rendered.contains("--backlog-set-gate-type gate1=blocked"));
        assert!(rendered.contains("--backlog-set-verify gate1=test"));
        assert!(rendered.contains("--review-add 'review me'"));
        assert!(rendered.contains("--review-edit 'review1=fix it'"));
        assert!(rendered.contains("--allow-replace-pending"));
        assert!(rendered.contains("--backlog-only"));
        assert!(rendered.contains("--status 'working hard'"));
    }

    #[test]
    fn compact_command_hint_renders_binary_owned_closeout_command() {
        assert_eq!(
            compact_command_hint(Path::new("tasks/doc.md")),
            "agent-doc compact tasks/doc.md --commit"
        );
    }

    #[test]
    fn pending_kept_open_ids_collects_edit_gate_and_reorder_targets() {
        let pending_edit = vec!["fix1=keep writing".to_string()];
        let pending_gate = vec!["gate1".to_string()];
        let pending_ungate = vec!["ungate1".to_string()];
        let pending_set_gate_type = vec!["gate2=blocked".to_string()];
        let pending_set_verify = vec!["verify1=test".to_string()];
        let review_edit = vec!["review1=still open".to_string()];

        assert_eq!(
            pending_kept_open_ids_from_mutations(
                &pending_edit,
                &pending_gate,
                &pending_ungate,
                &pending_set_gate_type,
                &pending_set_verify,
                &review_edit,
                Some("ordered1, ordered2,, "),
            ),
            vec![
                "fix1".to_string(),
                "gate1".to_string(),
                "ungate1".to_string(),
                "gate2".to_string(),
                "verify1".to_string(),
                "review1".to_string(),
                "ordered1".to_string(),
                "ordered2".to_string(),
            ]
        );
    }

    #[test]
    fn group_pending_add_targets_groups_repeated_file_pairs() {
        let grouped = group_pending_add_targets(&[
            "tasks/a.md".to_string(),
            "first".to_string(),
            "tasks/b.md".to_string(),
            "other".to_string(),
            "tasks/a.md".to_string(),
            "second".to_string(),
        ])
        .unwrap();

        assert_eq!(
            grouped,
            vec![
                (
                    PathBuf::from("tasks/a.md"),
                    vec!["first".to_string(), "second".to_string()]
                ),
                (PathBuf::from("tasks/b.md"), vec!["other".to_string()]),
            ]
        );
    }

    #[test]
    fn group_pending_add_targets_rejects_odd_input() {
        let err = group_pending_add_targets(&["tasks/a.md".to_string()]).unwrap_err();

        assert!(
            err.to_string()
                .contains("--backlog-add-to expects repeated FILE TEXT pairs"),
            "{err:#}"
        );
    }

    #[test]
    fn parse_id_order_trims_and_drops_empty_entries() {
        assert_eq!(
            parse_id_order(" first,second ,, third "),
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );
    }

    #[test]
    fn parse_tracked_work_edits_preserves_id_and_text() {
        assert_eq!(
            parse_tracked_work_edits(
                &["fix1=keep=a literal equals".to_string()],
                "--backlog-edit"
            )
            .unwrap(),
            vec![("fix1".to_string(), "keep=a literal equals".to_string())]
        );
    }

    #[test]
    fn parse_tracked_work_edits_names_the_failed_flag() {
        let err = parse_tracked_work_edits(&["missing separator".to_string()], "--icebox-edit")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("--icebox-edit expects 'id=text', got: missing separator"),
            "{err:#}"
        );
    }
}
