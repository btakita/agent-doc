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
    // Checking a release's outcome is agent work; `confirm` is not a signal.
    "After the first release, confirm that `release.yml` ran",
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
    ("An org owner makes `HAIVEN_RELEASE_APP_ID`", "org owner"),
    ("Decide who owns each public README", "decide"),
    ("Confirm this was the operator's failure", "sso"),
    (
        "Get a non-author approval on https://github.com/haiven-dev/haiven-infra/pull/336",
        "non-author",
    ),
    (
        "Get a non-author approval on https://github.com/haiven-dev/haiven-api/pull/896",
        "non-author",
    ),
    ("A haiven-sdk repo admin stops GitHub deleting", "admin"),
    ("Merging was not authorized for this item", "not authorized"),
    (
        "Decide whether `haiven-contracts`/`haiven-api` mentions",
        "decide",
    ),
    (
        "Get a human approval on https://github.com/haiven-dev/haiven-sdk/pull/242",
        "human approval",
    ),
    (
        "Decide how promotion PRs merge into haiven-sdk `main`",
        "decide",
    ),
    (
        "Approve and merge https://github.com/haiven-dev/haiven-sdk-godot/pull/9",
        "approve",
    ),
    ("Decide whether the first release-please release", "decide"),
    (
        "A haiven-sdk-godot admin creates the `sdk-dev` ruleset",
        "admin",
    ),
    (
        "at the new head `42152a0` (btakita cannot approve)",
        "non-author",
    ),
    (
        "at head `6dd0abe` from someone other than the author",
        "someone other than",
    ),
    (
        "Someone other than the author approves and merges PR 18",
        "approves",
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
    ("Rehearse before 4 PM", "rehearse"),
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
            OwnerEvidence::Signal("approve")
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

/// Review of #426, finding 1: a tagged row whose worker text holds
/// `#<token>:` (a pull request number) carries no operator verdict. Only a
/// colon after the tag, or after the `[#…]` id that follows it, is the
/// operator's answer. The row stays undrainable, keeps its `⏳`, and the turn
/// end waits on the operator instead of refusing over it.
#[test]
fn worker_text_with_a_hash_colon_is_not_an_operator_verdict() {
    use crate::queue_continuation::{
        DrainScope, head_awaits_operator_inline, head_carries_operator_verdict,
        live_drainable_continuation_head_entry,
    };
    let human = "follow up on an action item from `x`: human (tmstorm): approve PR #240: hosted CI is green [operator-verify] [#q-0123456789ad]";
    for row in [
        human.to_string(),
        format!("📥 {human}"),
        format!("⏳ {human}"),
        // An authored bracket id inside the worker's text, before the tag.
        "follow up on an action item from `x`: approve [#240]: CI is green [operator-verify] [#q-0123456789ad]".to_string(),
        // The tag without the binary's id.
        "follow up on an action item from `x`: approve PR #240: CI is green [operator-verify]".to_string(),
        // An id-less origin label that quotes an earlier `[#q-…]`.
        "follow up on an action item from queue item “ship [#q-0123456789aa]”: approve PR #240: CI is green [operator-verify]".to_string(),
    ] {
        assert!(!head_carries_operator_verdict(&row), "{row}");
        assert!(head_awaits_operator_inline(&row), "{row}");
    }
    // The operator's answer after the tag or after its id still hands the row
    // back to an agent.
    for answered in [
        "follow up on an action item from `x`: approve PR #240: CI is green [operator-verify] [#q-0123456789ad]: approved, merge it",
        "follow up on an action item from `x`: approve PR #240: CI is green [operator-verify]: done",
        "follow up on an action item from queue item “ship [#q-0123456789aa]”: approve PR #240 [operator-verify] [#q-0123456789ad]: done",
    ] {
        assert!(head_carries_operator_verdict(answered), "{answered}");
        assert!(!head_awaits_operator_inline(answered), "{answered}");
    }

    // Drain: neither scope selects the row; the agent row behind it drains.
    let content = format!(
        "---\nagent_doc_format: template\nqueue: go\n---\n\n\
<!-- agent:queue go -->\n\
- 📥 {human}\n\
- 📥 follow up on an action item from `x`: Fix the typo in README.md. [#q-0123456789ac]\n\
<!-- /agent:queue -->\n"
    );
    for scope in [DrainScope::InSessionLoop, DrainScope::Supervisor] {
        let head = live_drainable_continuation_head_entry(&content, scope)
            .map(|entry| entry.prompt_text)
            .unwrap_or_default();
        assert!(head.contains("Fix the typo"), "{scope:?}: {head}");
    }

    // Preflight's lifecycle projection paints `⏳`, never strips it.
    let entries = crate::document_queue::parse(&format!("- 📥 {human}\n- ⏳ {human}\n")).unwrap();
    let none = std::collections::HashSet::new();
    let marked = crate::document_queue::set_prompts_lifecycle_markers(&entries, &none, &none)
        .expect("the 📥 row gains ⏳");
    for prompt in crate::document_queue::prompts(&marked) {
        assert!(
            agent_doc_document::queue_projection::has_awaiting_operator_marker(&prompt.text),
            "{}",
            prompt.text
        );
    }

    // Stop gate: the row waits on the operator; only the agent row is a debt.
    let assessment = assess_transient_closeout(&content, &CloseoutEvidence::default()).unwrap();
    assert_eq!(
        assessment.quiescent_count(QuiescentReason::AwaitingOperator),
        1
    );
    assert_eq!(assessment.debts.len(), 1);
}

/// Review of #426, finding 2: agent phrasing that names a human word in a
/// non-human position stays agent work.
#[test]
fn agent_phrasing_with_review_or_auth_words_stays_agent_work() {
    for item in [
        "Confirm hosted CI is green on the merge commit",
        "Fix the authorization header in client.rs",
        "Update the approval-flow docs",
        "Update the approval docs for the new flow",
        "Re-run CI after a reviewer comments",
        "Fix the login page redirect",
        "Fix the password reset email template",
        "Fix the approve button handler",
        "Document how the client authorizes requests",
        "After the first release, confirm that the release workflow attached both zips",
    ] {
        assert_eq!(
            classify_action_item(item),
            (ActionItemOwner::Agent, OwnerEvidence::Default),
            "{item}"
        );
    }
}

/// Review of #426, finding 2: a credential, second-factor, sign-in, or device
/// check only a person can pass is human work, as is an instruction to
/// approve or authorize.
#[test]
fn credential_and_sign_in_checkpoints_are_human_work() {
    for (item, signal) in [
        (
            "Complete the npm security-key or password checkpoint and the PyPI unrecognized-device confirmation plus account login, then rerun the publish",
            "security-key",
        ),
        ("Complete the npm security key prompt", "security key"),
        (
            "Complete the npm password checkpoint",
            "password checkpoint",
        ),
        (
            "Complete the PyPI device confirmation",
            "device confirmation",
        ),
        ("Finish the PyPI account login", "account login"),
        ("Enter the 2FA code for crates.io", "2fa"),
        ("Enter the OTP for npm publish", "otp"),
        ("Register a passkey on the npm account", "passkey"),
        ("Log into the AWS console and rotate the key", "log into"),
        (
            "Enter the verification code GitHub emailed",
            "verification code",
        ),
        ("Re-run CI, then approve PR 9", "approve"),
        ("Review and approve #240", "approve"),
        ("**Approve** PR 9 once CI is green", "approve"),
        ("Authorize the haiven-release app for the org", "authorize"),
        ("btakita cannot approve PR 9 (author)", "cannot approve"),
        ("Merge #240 once tmstorm approves", "approves"),
        ("Needs authorization from the repo owner", "repo owner"),
    ] {
        assert_eq!(
            classify_action_item(item),
            (ActionItemOwner::Human, OwnerEvidence::Signal(signal)),
            "{item}"
        );
    }
}

/// Re-review of #426, finding 1: an unlabelled item that asks for someone
/// else's approval in a dependent form (not an instruction-position
/// `Approve …`) is human work. Each of these was human before the bare
/// `approv*` prefix was dropped, and must stay human so a later narrowing of
/// the vocabulary cannot silently turn them back into dispatchable rows.
#[test]
fn dependent_approval_phrasing_is_human_work() {
    for (item, signal) in [
        (
            "PR #240 needs approval before it can merge",
            "needs … approval",
        ),
        ("PR #240 requires approval", "requires … approval"),
        ("Wait for tmstorm to approve PR #240", "to approve"),
        ("Ask tmstorm to approve PR #240", "to approve"),
        ("tmstorm needs to approve PR #240", "to approve"),
        ("Have someone approve PR #240", "have … approve"),
        ("Merge once a reviewer has approved PR #240", "has approved"),
        ("Needs tmstorm's approval to merge", "needs … approval"),
        ("Get PR #240 approved", "get … approved"),
        ("the operator should approve the release", "<modal> approve"),
        // The remaining dependent forms.
        ("PR #240 is awaiting approval", "awaiting … approval"),
        (
            "Waiting on tmstorm's approval for PR #240",
            "awaiting … approval",
        ),
        ("Merge after tmstorm’s approval", "'s approval"),
        ("Merge once PR #240 is approved by tmstorm", "approved by"),
        ("PR #240 must be approved before merging", "be approved"),
        ("Then let tmstorm approve PR #240", "have … approve"),
    ] {
        assert_eq!(
            classify_action_item(item),
            (ActionItemOwner::Human, OwnerEvidence::Signal(signal)),
            "{item}"
        );
    }
}

/// The dependent approval forms read only inside the target's clause, and an
/// approval that is the agent's subject matter stays agent work.
#[test]
fn dependent_approval_forms_stay_inside_their_clause() {
    for item in [
        "Update the approval-flow docs",
        "Update the approval docs for the new flow",
        "Fix the approve button handler",
        "Wire the approved state into the badge",
        "Add the approval column; needs a migration",
        "Rename `needs_approval` in the client",
    ] {
        assert_eq!(
            classify_action_item(item),
            (ActionItemOwner::Agent, OwnerEvidence::Default),
            "{item}"
        );
    }
}

/// Review of #426, finding 2: preflight classifies a row once. An operator who
/// deletes `[operator-verify]` to hand a row back to an agent is not overruled
/// on the next cycle; a row preflight has never seen is still tagged.
#[test]
fn an_operator_removed_tag_stays_removed() {
    let doc = |rows: &str| format!("<!-- agent:queue go -->\n{rows}<!-- /agent:queue -->\n");
    let legacy =
        "- 📥 follow up on an action item from `x`: A non-author approves PR 9 [#q-0123456789ab]\n";
    let agent =
        "- 📥 follow up on an action item from `x`: Fix the typo in README.md. [#q-0123456789ac]\n";

    // First cycle: nothing seen yet; the legacy row is tagged.
    let first = doc(&format!("{legacy}{agent}"));
    let (tagged, rows) = reclassify_unseen_human_followup_rows(&first, &HashSet::new())
        .unwrap()
        .unwrap();
    assert_eq!(rows.len(), 1);
    let seen = followup_row_keys(&tagged).unwrap();
    assert_eq!(seen.classified.len(), 2);
    assert_eq!(seen.classified, seen.live);

    // The operator deletes the tag; `⏳` may linger or be repainted `📥`.
    let tagged_line = tagged
        .lines()
        .find(|line| line.contains("[operator-verify]"))
        .unwrap();
    for handed_back in [
        tagged_line.replace(" [operator-verify]", ""),
        tagged_line
            .replace(" [operator-verify]", "")
            .replace('⏳', "📥"),
    ] {
        let edited = tagged.replace(tagged_line, &handed_back);
        assert_eq!(
            followup_row_keys(&edited).unwrap().classified,
            seen.classified,
            "deleting the tag keeps the row's key"
        );
        assert!(
            reclassify_unseen_human_followup_rows(&edited, &seen.classified)
                .unwrap()
                .is_none(),
            "{handed_back}"
        );
        // The explicit repair still tags an unstarted `📥` row on request.
        assert_eq!(
            reclassify_human_followup_rows(&edited).unwrap().is_some(),
            handed_back.contains('📥'),
            "{handed_back}"
        );
    }

    // A new human row the operator never saw is still tagged.
    let fresh = "- 📥 follow up on an action item from `y`: Get a non-author approval on PR 10 [#q-0123456789ae]\n";
    let next = tagged.replace(agent, &format!("{agent}{fresh}"));
    let (_, rows) = reclassify_unseen_human_followup_rows(&next, &seen.classified)
        .unwrap()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].before.contains("PR 10"), "{:?}", rows[0]);

    // A row with an attempt in flight is live but not yet classified, so it
    // is classified once it is unstarted again.
    let claimed = doc(
        "- 🟡 [#subagent:a#1] follow up on an action item from `x`: approve PR 9 [#q-0123456789ab]\n",
    );
    let keys = followup_row_keys(&claimed).unwrap();
    assert!(keys.classified.is_empty());
    assert_eq!(keys.live.len(), 1);
}
