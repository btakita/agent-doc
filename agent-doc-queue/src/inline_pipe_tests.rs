//! `#pipeprompt` inline form: `- #review | #address` and
//! `- #rebase |> #address |> #adversarial-pr`.

use super::*;
use crate::document_queue::{
    INLINE_PIPE_PROMPT_HEADER, QueueEntry, executable_prompt_indexes, ordered_step_waits, parse,
    submitted_prompt_at,
};

fn presets() -> Vec<(String, String)> {
    [
        "#review",
        "#address",
        "#adversarial-pr",
        "#rebase",
        "#gh-fix",
        "#pr",
        "#note",
    ]
    .into_iter()
    .map(|name| (name.to_string(), format!("Run {name}.")))
    .collect()
}

fn queue_doc(body: &str) -> String {
    format!("<!-- agent:run -->\n{body}<!-- /agent:run -->\n")
}

fn never_claimed(_: &str) -> bool {
    false
}

fn expand(body: &str) -> String {
    expand_inline_hand_offs(&queue_doc(body), &presets(), &never_claimed).0
}

/// Work text of the rows a drain may run now: executable and not held.
fn runnable(body: &str) -> Vec<String> {
    let entries = parse(body).unwrap();
    let waits = ordered_step_waits(&entries);
    executable_prompt_indexes(&entries)
        .into_iter()
        .filter(|index| waits[*index].is_none())
        .filter_map(|index| match &entries[index] {
            QueueEntry::Prompt(prompt) => Some(prompt.text.clone()),
            _ => None,
        })
        .collect()
}

/// sdk-dev's live shape: a PR row whose child is an inline hand-off chain.
#[test]
fn an_inline_hand_off_becomes_the_nested_hand_off_it_stands_for() {
    let body = concat!(
        "- ### 🔒 https://github.com/sample-org/sample-api/pull/933\n",
        "  - #rebase |> #address |> #adversarial-pr [#q-0123456789ab]\n",
        "- next item\n",
    );
    let (expanded, repairs) = expand_inline_hand_offs(&queue_doc(body), &presets(), &never_claimed);
    assert_eq!(
        expanded,
        queue_doc(concat!(
            "- ### 🔒 https://github.com/sample-org/sample-api/pull/933\n",
            "  - #rebase |> [#q-0123456789ab]\n",
            "    - #address |>\n",
            "      - #adversarial-pr\n",
            "- next item\n",
        ))
    );
    assert_eq!(repairs.len(), 1);
    assert_eq!(
        repairs[0].kind,
        crate::self_repair::SelfRepairKind::InlineHandOff
    );
    assert_eq!(
        repairs[0].rewritten,
        Some((
            "#rebase |> #address |> #adversarial-pr [#q-0123456789ab]".to_string(),
            "#rebase |> [#q-0123456789ab]".to_string()
        ))
    );
    // Idempotent.
    let (again, more) = expand_inline_hand_offs(&expanded, &presets(), &never_claimed);
    assert_eq!(again, expanded);
    assert!(more.is_empty());
    // Stage 1 runs first; the later stages wait on its hand-back.
    let nested = expanded
        .strip_prefix("<!-- agent:run -->\n")
        .and_then(|rest| rest.strip_suffix("<!-- /agent:run -->\n"))
        .unwrap();
    assert_eq!(
        runnable(nested),
        vec!["#rebase |> [#q-0123456789ab]", "next item"]
    );
}

#[test]
fn a_parenthetical_argument_stays_with_its_stage() {
    let body = "- #rebase |> #address (see https://github.com/sample-org/sample-api/pull/936#discussion_r1) |> #adversarial-pr\n";
    assert_eq!(
        expand(body),
        queue_doc(concat!(
            "- #rebase |>\n",
            "  - #address (see https://github.com/sample-org/sample-api/pull/936#discussion_r1) |>\n",
            "    - #adversarial-pr\n",
        ))
    );
    assert_eq!(
        expand("- #address |> #adversarial-pr\n"),
        queue_doc("- #address |>\n  - #adversarial-pr\n")
    );
}

#[test]
fn a_pipe_inside_one_stage_stays_inline_and_children_follow_the_last_stage() {
    assert_eq!(
        expand("- #review | #address |> #note\n"),
        queue_doc("- #review | #address |>\n  - #note\n")
    );
    // The row's trailing operator stays on the last stage, with its children.
    assert_eq!(
        expand("- #review |> #address |\n  - under 200 words\n  - no internal ids\n- next\n"),
        queue_doc(concat!(
            "- #review |>\n",
            "  - #address |\n",
            "    - under 200 words\n",
            "    - no internal ids\n",
            "- next\n",
        ))
    );
    assert_eq!(
        expand("1. #review |> #address\n   - context\n2. later\n"),
        queue_doc("1. #review |>\n   - #address\n     - context\n2. later\n")
    );
}

#[test]
fn rows_an_attempt_holds_and_rows_that_are_not_inline_pipes_are_never_rewritten() {
    for body in [
        // An attempt holds the row: it runs as one task.
        "- 🚧 [#subagent:w1#1] #rebase |> #address\n",
        // Struck: finished under the one-task reading.
        "- ~~#rebase |> #adversarial-pr ⟦published⟧~~\n",
        // Not every segment starts with a known preset.
        "- #rebase |> summarize the diff\n",
        "- #rebase |> #unknown-tag\n",
        "- url: #rebase |> #address\n",
        // Only ` | ` joins: one task, nothing to split.
        "- #review | #address\n",
        // Inside a folded block: the whole block is one task (linted).
        "- collect |\n  - #rebase |> #address\n",
        // A struck child already ran under the old reading.
        "- #rebase |> #address\n  - ~~old leaf~~\n",
        // Code span.
        "- run `#rebase |> #address`\n",
    ] {
        assert_eq!(expand(body), queue_doc(body), "{body}");
    }
    let claimed = |row: &str| row.starts_with("#rebase");
    let body = "- #rebase |> #address\n";
    assert_eq!(
        expand_inline_hand_offs(&queue_doc(body), &presets(), &claimed).0,
        queue_doc(body)
    );
    // The seen glyph holds no attempt.
    let seen = agent_doc_queue_icons::queue_icons().seen.clone();
    assert_eq!(
        expand(&format!("- {seen} #rebase |> #address\n")),
        queue_doc(&format!("- {seen} #rebase |>\n  - #address\n"))
    );
    // Outside a runnable component nothing is rewritten.
    let plain = "- #rebase |> #address\n";
    assert_eq!(
        expand_inline_hand_offs(plain, &presets(), &never_claimed).0,
        plain
    );
}

#[test]
fn self_repair_writes_the_nested_form_with_the_documents_presets() {
    let doc = queue_doc("- #rebase |> #address\n");
    let _presets = crate::goal_preset_terminal::InheritedPresetsGuard::install_resolved(presets());
    let (repaired, repairs) = crate::self_repair::self_repair_document_with_claims(
        &doc,
        &|_, _| true,
        &|_| false,
        &|_| false,
        &never_claimed,
    )
    .expect("expands");
    assert_eq!(repaired, queue_doc("- #rebase |>\n  - #address\n"));
    assert!(
        repairs
            .iter()
            .any(|repair| repair.kind == crate::self_repair::SelfRepairKind::InlineHandOff)
    );
    // A second run is a no-op.
    assert!(
        crate::self_repair::self_repair_document_with_claims(
            &repaired,
            &|_, _| true,
            &|_| false,
            &|_| false,
            &never_claimed,
        )
        .is_none()
    );
}

#[test]
fn an_inline_pipe_row_is_one_task_with_its_pipeline() {
    let entries = parse("- #review | #address [#q-0123456789ab]\n").unwrap();
    assert_eq!(executable_prompt_indexes(&entries), vec![0]);
    let submitted = submitted_prompt_at(&entries, 0).unwrap();
    assert_eq!(
        submitted,
        format!(
            "#review | #address [#q-0123456789ab]\n{INLINE_PIPE_PROMPT_HEADER}\n- #review | #address"
        )
    );
    // Under a parent the lineage prefixes the pipeline.
    let entries = parse("- epic\n  - #gh-fix https://github.com/o/r/issues/7 | #pr\n").unwrap();
    let submitted = submitted_prompt_at(&entries, 1).unwrap();
    assert!(
        submitted.ends_with(&format!(
            "\n{INLINE_PIPE_PROMPT_HEADER}\n- epic: #gh-fix https://github.com/o/r/issues/7 | #pr"
        )),
        "{submitted}"
    );
    // Prose with ` | ` gets no pipe header.
    let entries = parse("- pending | completed\n").unwrap();
    assert!(
        !submitted_prompt_at(&entries, 0)
            .unwrap()
            .contains(INLINE_PIPE_PROMPT_HEADER)
    );
    // A trailing ` |` folds the block: the nested statement's header, with the
    // inline stages leading the pipelines.
    let entries = parse("- #review | #address |\n  - under 200 words\n").unwrap();
    assert_eq!(entries.len(), 1);
    let submitted = submitted_prompt_at(&entries, 0).unwrap();
    assert!(
        submitted.ends_with("\n- #review | #address | under 200 words"),
        "{submitted}"
    );
}

#[test]
fn inline_pipe_lints() {
    use agent_doc_markdown_ast::overlay::{PipeLintKind, pipe_statement_lints};
    let doc =
        queue_doc("- #review | then summarize\n- #review | #address\n- pending | completed\n");
    let findings = pipe_statement_lints(&doc);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].kind, PipeLintKind::InlinePipeProseSegment);
    assert!(
        findings[0].message.contains("then summarize"),
        "{findings:?}"
    );
    assert!(findings[0].message.contains("nest"), "{findings:?}");
    // An inline hand-off inside a block runs as one task.
    let doc = queue_doc("- collect |\n  - #rebase |> #address\n");
    assert!(
        pipe_statement_lints(&doc)
            .iter()
            .any(|finding| finding.kind == PipeLintKind::HandOffInsideBlock),
        "{:?}",
        pipe_statement_lints(&doc)
    );
}

fn body_of(doc: &str) -> &str {
    doc.strip_prefix("<!-- agent:run -->\n")
        .and_then(|rest| rest.strip_suffix("<!-- /agent:run -->\n"))
        .unwrap()
}

/// The expanded chain runs stage by stage through the phase-2 machinery:
/// each stage is released by the `✅` of the one before it, takes that
/// stage's hand-back as input, and the last strike closes every head.
#[test]
fn an_expanded_chain_hands_off_one_stage_at_a_time() {
    let expanded = expand("- #rebase |> #address |> #adversarial-pr [#q-0123456789ab]\n- next\n");
    // Stage 1 reports: its row is struck with its response attached.
    let stage_one = expanded.replace(
        "- #rebase |> [#q-0123456789ab]\n",
        concat!(
            "- ~~#rebase |> [#q-0123456789ab]~~\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** rebased onto main\n",
        ),
    );
    let settled = crate::hand_off::settle_hand_off_heads(&stage_one).expect("stage 1 done");
    assert!(
        settled.contains("- ✅ #rebase |> [#q-0123456789ab]\n"),
        "{settled}"
    );
    assert_eq!(runnable(body_of(&settled)), vec!["#address |>", "next"]);
    let input = crate::hand_off::hand_off_input_for_item(&settled, "#address |>")
        .expect("stage 2 takes stage 1's hand-back");
    assert_eq!(input.from_stage, "#rebase");
    assert!(input.hand_backs[0].text.contains("rebased onto main"));
    // Stage 2 reports.
    let stage_two = settled.replace(
        "  - #address |>\n",
        concat!(
            "  - ~~#address |>~~\n",
            "    - **Response [#r-1123456789ab]**\n",
            "      - **Summary:** NO-WRITTEN-FIX\n",
        ),
    );
    let settled = crate::hand_off::settle_hand_off_heads(&stage_two).expect("stage 2 done");
    assert_eq!(runnable(body_of(&settled)), vec!["#adversarial-pr", "next"]);
    let input = crate::hand_off::hand_off_input_for_item(&settled, "#adversarial-pr")
        .expect("stage 3 takes stage 2's hand-back");
    assert_eq!(input.from_stage, "#address");
    // The last stage's strike closes both heads in one write.
    let stage_three = settled.replace("    - #adversarial-pr\n", "    - ~~#adversarial-pr~~\n");
    assert_ne!(stage_three, settled, "{settled}");
    let closed = crate::hand_off::settle_hand_off_heads(&stage_three).expect(&stage_three);
    assert!(
        closed.contains("- ~~#rebase |> [#q-0123456789ab]~~\n"),
        "{closed}"
    );
    assert!(closed.contains("  - ~~#address |>~~\n"), "{closed}");
    assert_eq!(runnable(body_of(&closed)), vec!["next"]);
}
