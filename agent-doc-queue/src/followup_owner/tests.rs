use super::*;
use crate::transient_closeout::{
    CloseoutEvidence, QuiescentReason, assess_transient_closeout, normal_closeout_failure,
};

/// Live rows copied from a session document whose Stop hook looped over
/// human-owned follow-up rows (2026-11-13).
const FIXTURE: &str = include_str!("../../tests/fixtures/human_followup_rows.md");

/// Fixture rows an agent can do, by a distinctive fragment of each.
const AGENT_ROWS: &[&str] = &[
    "Run a first promotion as a CI dry-run",
    "Fix the same wrong assumption in ADR 0036",
    "After #240 merges, rebase #241",
    "Address tmstorm's CHANGES_REQUESTED review on #238",
    "job a required check on `main`",
    "Either change the Python requirement",
    "Update the upstream haiven-sdk generator",
    "Retarget https://github.com/haiven-dev/haiven-sdk-js/pull/2",
    "Add a byte-identical copy of",
    "After PR 18 and the `sdk-dev` copy land, dispatch",
];

/// Fixture rows only a person can do, with the signal that decides each.
const HUMAN_ROWS: &[(&str, &str)] = &[
    (
        "tmstorm needs to re-review and approve PR #100",
        "re-review",
    ),
    (
        "A non-author approves https://github.com/haiven-dev/haiven-api/pull/855",
        "non-author",
    ),
    ("An org owner makes `HAIVEN_RELEASE_APP_ID`", "confirms"),
    ("Decide who owns each public README", "decide"),
    ("Confirm this was the operator's failure", "confirm"),
    (
        "Get a non-author approval on https://github.com/haiven-dev/haiven-infra/pull/336",
        "non-author",
    ),
    (
        "Get a non-author approval on https://github.com/haiven-dev/haiven-api/pull/896",
        "non-author",
    ),
    ("A haiven-sdk repo admin stops GitHub deleting", "admin"),
    ("Merging was not authorized for this item", "authorized"),
    (
        "Decide whether `haiven-contracts`/`haiven-api` mentions",
        "decide",
    ),
    (
        "Get a human approval on https://github.com/haiven-dev/haiven-sdk/pull/242",
        "approv",
    ),
    (
        "Decide how promotion PRs merge into haiven-sdk `main`",
        "decide",
    ),
    (
        "Approve and merge https://github.com/haiven-dev/haiven-sdk-godot/pull/9",
        "approv",
    ),
    ("Decide whether the first release-please release", "decide"),
    (
        "A haiven-sdk-godot admin creates the `sdk-dev` ruleset",
        "admin",
    ),
    (
        "After the first release, confirm that `release.yml` ran",
        "confirm",
    ),
    (
        "at the new head `42152a0` (btakita cannot approve)",
        "non-author",
    ),
    (
        "at head `6dd0abe` from someone other than the author",
        "approv",
    ),
    (
        "Someone other than the author approves and merges PR 18",
        "approv",
    ),
    (
        "An org owner adds haiven-cli (repo id `1413929956`)",
        "org owner",
    ),
    (
        "An org owner gives the haiven-release app Repository permission",
        "org owner",
    ),
    ("org variable `HAIVEN_RELEASE_APP_ID`", "org owner"),
    ("org secret `HAIVEN_RELEASE_APP_PRIVATE_KEY`", "org owner"),
    ("Rehearse before 4 PM", "confirm"),
    ("haiven-sdk-unreal-lyra/pull/10", "non-author"),
    (
        "https://github.com/haiven-dev/haiven-sdk/pull/249",
        "non-author",
    ),
];

/// Every live `📥 follow up on an action item` row of `content`, as written.
fn pending_followup_lines(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter(|line| line.contains("📥 follow up on an action item from "))
        .collect()
}

fn line_containing<'a>(content: &'a str, needle: &str) -> &'a str {
    let mut hits = content.lines().filter(|line| line.contains(needle));
    let line = hits
        .next()
        .unwrap_or_else(|| panic!("no line contains {needle:?}"));
    assert!(hits.next().is_none(), "{needle:?} names more than one line");
    line
}

/// The action item a fixture line quotes.
fn item_of(line: &str) -> String {
    let text = line.trim_start().strip_prefix("- ").unwrap();
    let text = text.strip_prefix("### ").unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    let item = followup_row_action_item(work).unwrap_or_else(|| panic!("{line}"));
    agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item)
}

#[test]
fn explicit_owner_labels_decide_the_owner() {
    for (item, owner) in [
        ("agent: merge #240 once CI is green", ActionItemOwner::Agent),
        (
            "Agent: approve nothing; just rebase",
            ActionItemOwner::Agent,
        ),
        ("**agent:** update the README", ActionItemOwner::Agent),
        ("**Agent**: update the README", ActionItemOwner::Agent),
        ("agent (codex): rebase #241", ActionItemOwner::Agent),
        ("operator: pick the release channel", ActionItemOwner::Human),
        ("human: rotate the deploy key", ActionItemOwner::Human),
        ("human (tmstorm): approve PR #100", ActionItemOwner::Human),
        (
            "**human (org owner):** grant Workflows write",
            ActionItemOwner::Human,
        ),
        (
            "Human (a haiven-sdk admin) : add the ruleset",
            ActionItemOwner::Human,
        ),
    ] {
        assert_eq!(explicit_owner_label(item), Some(owner), "{item}");
        assert_eq!(
            classify_action_item(item),
            (owner, OwnerEvidence::Label),
            "{item}"
        );
    }
    // Not a label: the word is not followed by `:`.
    for item in [
        "Agent's retry budget is too small",
        "Operator decides the channel",
        "human review is needed",
        "agentic flows: rebase",
    ] {
        assert_eq!(explicit_owner_label(item), None, "{item}");
    }
    // A label beats the vocabulary in both directions.
    assert_eq!(
        action_item_owner("agent: confirm CI is green, then merge"),
        ActionItemOwner::Agent
    );
    assert_eq!(
        action_item_owner("human (btakita): rebase the stack by hand"),
        ActionItemOwner::Human
    );
}

#[test]
fn the_vocabulary_ignores_agent_verbs_and_inline_code() {
    for item in [
        "the gutter does not show the stamp",
        "Address the review on #238",
        "Rebase #241 after #240 merges",
        "Fix the merge conflict in `approve.yml`",
        "Add a test for `confirm_dialog()`",
        "Update the non-public wording",
    ] {
        assert_eq!(human_owner_signal(item), None, "{item}");
    }
}

/// Every pending follow-up row of the fixture classifies as expected: the
/// rows an agent can do stay agent work, the rest are human-owned and name
/// the phrase that decided them.
#[test]
fn fixture_rows_classify_by_owner() {
    let rows = pending_followup_lines(FIXTURE);
    assert_eq!(rows.len(), AGENT_ROWS.len() + HUMAN_ROWS.len());
    for fragment in AGENT_ROWS {
        let item = item_of(line_containing(FIXTURE, fragment));
        assert_eq!(
            classify_action_item(&item),
            (ActionItemOwner::Agent, OwnerEvidence::Default),
            "{item}"
        );
    }
    for (fragment, signal) in HUMAN_ROWS {
        let item = item_of(line_containing(FIXTURE, fragment));
        assert_eq!(
            classify_action_item(&item),
            (ActionItemOwner::Human, OwnerEvidence::Signal(signal)),
            "{item}"
        );
    }
}

#[test]
fn the_tag_lands_before_the_generated_row_id() {
    assert_eq!(
        tag_operator_owned("follow up on an action item from `x`: approve PR 9 [#q-0123456789ab]"),
        "follow up on an action item from `x`: approve PR 9 [operator-verify] [#q-0123456789ab]"
    );
    assert_eq!(
        tag_operator_owned("follow up on an action item from `x`: approve PR 9"),
        "follow up on an action item from `x`: approve PR 9 [operator-verify]"
    );
    let tagged =
        "follow up on an action item from `x`: approve [operator-verify] [#q-0123456789ab]";
    assert_eq!(tag_operator_owned(tagged), tagged);
    // The generated id still identifies the row.
    assert_eq!(
        agent_doc_element_queue::queue_prompt_stable_id(tagged).as_deref(),
        Some("q-0123456789ab")
    );
}

/// The reclassification tags each human-owned pending row in place, swaps
/// its `📥` for `⏳`, and touches no other byte. A second run changes nothing.
#[test]
fn reclassification_tags_human_rows_in_place_and_is_idempotent() {
    let (updated, rows) = reclassify_human_followup_rows(FIXTURE)
        .unwrap()
        .expect("the fixture holds human-owned rows");
    assert_eq!(rows.len(), HUMAN_ROWS.len());
    for (fragment, _) in HUMAN_ROWS {
        let line = line_containing(&updated, fragment);
        assert!(line.contains("⏳ follow up on an action item"), "{line}");
        assert!(!line.contains('📥'), "{line}");
        assert!(line.contains(" [operator-verify]"), "{line}");
        // Every reader of the row sees the gate, including an id-less row
        // whose origin quote holds an earlier `[#q-…]`.
        assert!(
            crate::queue_continuation::head_awaits_operator_inline(line.trim_start_matches("- ")),
            "{line}"
        );
        // The binary's row id, when there is one, still ends the line.
        let before = line_containing(FIXTURE, fragment);
        if let Some(id) = agent_doc_element_queue::queue_prompt_stable_id(
            before
                .trim_start()
                .trim_start_matches("- ")
                .trim_start_matches("### "),
        ) {
            assert!(
                line.ends_with(&format!("[operator-verify] [#{id}]")),
                "{line}"
            );
        }
    }
    for fragment in AGENT_ROWS {
        assert_eq!(
            line_containing(&updated, fragment),
            line_containing(FIXTURE, fragment)
        );
    }
    // Lines are only ever rewritten, never added or removed; every line that
    // changed is one of the reported rows.
    let before: Vec<&str> = FIXTURE.lines().collect();
    let after: Vec<&str> = updated.lines().collect();
    assert_eq!(before.len(), after.len());
    let changed = before
        .iter()
        .zip(&after)
        .filter(|(old, new)| old != new)
        .count();
    assert_eq!(changed, rows.len());
    // Untouched: the non-follow-up human row, `🙋⏳` rows, struck rows.
    for fragment in [
        "📥 A non-author (tmstorm) approves https://github.com/haiven-dev/haiven-sdk/pull/245",
        "🙋⏳ follow up on an action item from queue item “haiven-cli: Setup Release Please workflow to merge sdk-dev into main: see https:…”: An eligible non-author",
    ] {
        assert_eq!(
            line_containing(&updated, fragment),
            line_containing(FIXTURE, fragment)
        );
    }
    assert!(reclassify_human_followup_rows(&updated).unwrap().is_none());
}

/// A row with an attempt in flight keeps its text: the claim is keyed by it.
#[test]
fn reclassification_skips_started_rows() {
    for row in [
        "🟡 [#subagent:a#1] follow up on an action item from `x`: approve PR 9 [#q-0123456789ab]",
        "🚧 [#subagent:a#1] follow up on an action item from `x`: approve PR 9 [#q-0123456789ab]",
        "☑️ [#subagent:a#1] follow up on an action item from `x`: approve PR 9 [#q-0123456789ab]",
        "~~follow up on an action item from `x`: approve PR 9~~",
        "🙋📥 follow up on an action item from `x`: approve PR 9 [#subagent-operator-request-a]",
    ] {
        assert_eq!(reclassify_followup_row(row), None, "{row}");
    }
    assert_eq!(
        reclassify_followup_row("follow up on an action item from `x`: approve PR 9"),
        Some((
            "follow up on an action item from `x`: approve PR 9 [operator-verify]".to_string(),
            OwnerEvidence::Signal("approv")
        ))
    );
}

/// The Stop hook's gate: on the fixture it refuses over every pending
/// follow-up row. After the reclassification it no longer names any
/// human-owned row and still refuses over every agent-owned one.
#[test]
fn stop_gate_releases_human_rows_and_still_refuses_agent_rows() {
    let before = normal_closeout_failure(FIXTURE)
        .unwrap()
        .expect("the fixture's pending rows refuse");
    for (fragment, _) in HUMAN_ROWS {
        assert!(before.contains(fragment), "{fragment}\n{before}");
    }

    let (updated, _) = reclassify_human_followup_rows(FIXTURE).unwrap().unwrap();
    let after = normal_closeout_failure(&updated)
        .unwrap()
        .expect("agent-owned rows still refuse");
    for (fragment, _) in HUMAN_ROWS {
        assert!(!after.contains(fragment), "{fragment}\n{after}");
    }
    for fragment in AGENT_ROWS {
        assert!(after.contains(fragment), "{fragment}\n{after}");
    }
    // The hand-written non-follow-up row is outside this repair.
    assert!(after.contains("A non-author (tmstorm) approves"), "{after}");

    // A projection that repaints `📥` over the tag does not bring the
    // refusal back: the tagged pending row waits on the operator.
    let repainted = updated
        .lines()
        .map(|line| {
            if line.contains("[operator-verify]") {
                line.replace("⏳ follow up", "📥 follow up")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let assessment = assess_transient_closeout(&repainted, &CloseoutEvidence::default()).unwrap();
    assert_eq!(
        assessment.quiescent_count(QuiescentReason::AwaitingOperator),
        HUMAN_ROWS.len()
    );
    assert_eq!(assessment.debts.len(), AGENT_ROWS.len() + 1);
    assert_eq!(assessment.quiescent_state(), Some("waiting_on_operator"));

    // With the agent rows done (struck), the turn may end.
    let mut finished = updated.clone();
    for fragment in AGENT_ROWS
        .iter()
        .chain(std::iter::once(&"A non-author (tmstorm) approves"))
    {
        let line = line_containing(&finished, fragment).to_string();
        let text = line.trim_start().strip_prefix("- ").unwrap();
        let heading = if text.starts_with("### ") { "### " } else { "" };
        let work = agent_doc_element_queue::struck_work_text(text.trim_start_matches("### "));
        let struck = format!("- {heading}~~{work}~~");
        finished = finished.replacen(&line, &struck, 1);
    }
    assert!(normal_closeout_failure(&finished).unwrap().is_none());
}

/// No agent drain scope selects a tagged row; the next agent row drains.
#[test]
fn tagged_rows_are_never_drainable() {
    use crate::queue_continuation::{DrainScope, live_drainable_continuation_head_entry};
    let content = "---\nagent_doc_format: template\nqueue: go\n---\n\n\
<!-- agent:queue go -->\n\
- ⏳ follow up on an action item from `x`: A non-author approves PR 9. [operator-verify] [#q-0123456789ab]\n\
- 📥 follow up on an action item from `x`: Fix the typo in README.md. [#q-0123456789ac]\n\
<!-- /agent:queue -->\n";
    for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
        let head = live_drainable_continuation_head_entry(content, scope)
            .map(|entry| entry.prompt_text)
            .unwrap_or_default();
        assert!(head.contains("Fix the typo"), "{scope:?}: {head}");
    }
    assert!(crate::queue_continuation::head_awaits_operator_inline(
        "📥 follow up on an action item from `x`: A non-author approves PR 9. [operator-verify] [#q-0123456789ab]"
    ));
    // An inline operator answer hands the row back to an agent.
    assert!(!crate::queue_continuation::head_awaits_operator_inline(
        "follow up on an action item from `x`: approve PR 9 [operator-verify] [#q-0123456789ab]: approved, merge it"
    ));
}
