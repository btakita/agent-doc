//! Address-run completion (`#reviewfixonhead`).
//!
//! Addressing a pull-request review is not done until that review's written
//! fix is the code on the exact current head. A later review, a comment that
//! the item is met, or different behavior does not replace the written fix.
//! The hand-back names the head that contains it:
//!
//! ```text
//! FIX-ON-HEAD <review-url> head=<sha>
//! ```
//!
//! A hand-back that omits that line, or that admits the written fix is absent
//! or was superseded, does not complete the address run. When every review
//! states no code change, the hand-back records that with `NO-WRITTEN-FIX`
//! instead of inventing a commit. Exactly one of the two lines settles the
//! address half, so a row that also names `#adversarial-pr` can continue to
//! that review (`#crossagenthandoff`).

use agent_doc_document::queue_projection::strip_priority_markers;

/// Binding closeout for an `#address` run (`#reviewfixonhead`).
///
/// It overrides a reading that treats a later review, or a different behavior,
/// as addressing a review whose written fix is not on the current head.
pub const ADDRESS_FIX_ON_HEAD_RULE: &str = "Address run (#reviewfixonhead): a review is not addressed until its written fix is the code on the exact current head. The written fix is the code change that review states. Land that change, commit it with the repository's conventions, and publish it only to the existing pull-request head before reporting completion. A later review, a comment that the item is met, or different behavior does not replace that written fix. Do not report the review complete, and do not finish this run, while that fix is absent from the current head. The hand-back names each such review on its own line: `FIX-ON-HEAD <review-url> head=<sha>`. Emit that line only when that head's code is the written fix. When every review states no code change, do not invent a commit. Finish that address half with one line `NO-WRITTEN-FIX head=<sha>`. Do not emit that line together with `FIX-ON-HEAD`, and do not emit it when any review states a code change. On a row that also names `#adversarial-pr`, that line ends the address half so the next worker posts the independent review. Neither line is completion by itself (#addressproof). The binary completes the address only when it observes the pull-request head SHA change, a review reply on the addressed thread, or nothing to address on the current head (#noopaddress): no unresolved review thread, no changes-requested review, and no failing or unfinished required check. A no-op address hands back `NO-WRITTEN-FIX head=<sha>` once that head is in that state; first wait for its required checks with `gh pr checks <number> --repo <owner>/<repo> --required --watch`. When that state waits on someone else, such as a reviewer who must re-review, hand back the prompt-blocker structure with **Evidence:** naming that observation. A hand-back that claims an effect without that observation stays open. Never ask the operator to delete or edit the row.";

const ABSENT_FIX_PHRASES: &[&str] = &[
    "written fix is not",
    "written fix is still not",
    "is not the code",
    "not the code",
    "does not do that written fix",
    "does not have that fix",
    "do not have that fix",
    "fix is not on",
    "written fix is absent",
    "were superseded",
    "was superseded",
    "superseded by the review",
    "superseded by a later",
    "not reopened",
];

/// True when `head` references the `#address` preset in `presets`.
pub fn head_is_address_run(head: &str, presets: &[(String, String)]) -> bool {
    crate::queue_response::queue_prompt_preset_expansions_from(
        head,
        presets.iter().map(|(name, body)| (name, body)),
    )
    .iter()
    .any(|(name, _)| name.eq_ignore_ascii_case("#address"))
}

/// True when `item` carries the `#address` preset token.
pub fn item_mentions_address_preset(item: &str) -> bool {
    let text = strip_priority_markers(item);
    let text = crate::queue_continuation::mask_directive_scan(&text);
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .any(|token| token.eq_ignore_ascii_case("#address"))
}

/// `(review url, head sha)` receipts in `response`.
pub fn address_fix_receipts(response: &str) -> Vec<(String, String)> {
    let mut receipts = Vec::new();
    for line in response.lines() {
        if let Some(receipt) = line_fix_receipt(line) {
            receipts.push(receipt);
        }
    }
    receipts
}

/// True when `response` says the written fix is absent, or that a review was
/// superseded instead of applied.
pub fn address_response_admits_fix_absent(response: &str) -> bool {
    let prose = collapsed_prose(response);
    ABSENT_FIX_PHRASES
        .iter()
        .any(|phrase| prose.contains(phrase))
}

/// True when `response` lands the review's written fix on a named head, or
/// records that every review states no code change.
///
/// Exactly one receipt kind settles the run. Admitting the fix is absent, or
/// that the review was superseded, keeps the run incomplete. Emitting both
/// `FIX-ON-HEAD` and `NO-WRITTEN-FIX` does not settle it.
pub fn address_run_lands_written_fix(response: &str) -> bool {
    if address_response_admits_fix_absent(response) {
        return false;
    }
    let fixes = !address_fix_receipts(response).is_empty();
    let none = !no_written_fix_receipts(response).is_empty();
    matches!((fixes, none), (true, false) | (false, true))
}

/// True when `response` settles an address run because the reviews state no
/// code change. A landed `FIX-ON-HEAD` is not this case.
pub fn address_run_settled_without_written_fix(response: &str) -> bool {
    address_run_lands_written_fix(response) && address_fix_receipts(response).is_empty()
}

/// Head shas named by `NO-WRITTEN-FIX head=<sha>` lines.
pub fn no_written_fix_receipts(response: &str) -> Vec<String> {
    response.lines().filter_map(line_no_written_fix).collect()
}

fn collapsed_prose(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn receipt_payload(line: &str) -> &str {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        rest = rest.trim_start_matches(['*', '_', '`']);
        if rest == before {
            break;
        }
    }
    rest
}

fn line_fix_receipt(line: &str) -> Option<(String, String)> {
    let rest = receipt_payload(line);
    let rest = rest.strip_prefix("FIX-ON-HEAD")?;
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let url = parts.next()?;
    let head = parts.next()?;
    let sha = head.strip_prefix("head=")?;
    let sha = sha.trim_end_matches([',', '.', ';', ')', '`']);
    if !is_head_sha(sha) {
        return None;
    }
    Some((url.to_string(), sha.to_string()))
}

fn line_no_written_fix(line: &str) -> Option<String> {
    let rest = receipt_payload(line);
    let rest = rest.strip_prefix("NO-WRITTEN-FIX")?;
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let head = rest.split_whitespace().next()?;
    let sha = head.strip_prefix("head=")?;
    let sha = sha.trim_end_matches([',', '.', ';', ')', '`']);
    if !is_head_sha(sha) {
        return None;
    }
    Some(sha.to_string())
}

fn is_head_sha(sha: &str) -> bool {
    (7..=40).contains(&sha.len()) && sha.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVIEW: &str =
        "https://github.com/sample-org/sample-repo/pull/12#pullrequestreview-5837752992";
    const HEAD: &str = "fa3a3f449986d565f21aeb6e0f792323bc8d1d58";

    fn presets() -> Vec<(String, String)> {
        vec![(
            "#address".to_string(),
            "Address every actionable review item on the exact current head.".to_string(),
        )]
    }

    #[test]
    fn an_address_token_is_an_address_run_and_a_goal_row_is_not() {
        let presets = presets();
        assert!(head_is_address_run(
            "https://github.com/sample-org/sample-repo/pull/12: #address",
            &presets,
        ));
        assert!(!head_is_address_run(
            "https://github.com/sample-org/sample-repo/pull/12: #adversarial-pr",
            &presets,
        ));
        assert!(item_mentions_address_preset("#address [#q-1]"));
        assert!(!item_mentions_address_preset("#adversarial-pr"));
    }

    /// `#directivequote`: agent-loop `agent-doc-bugs.md` (2026-11-24) row
    /// `stoploopcap` quotes another row's `'2. 📥 #address'`. It was read as an
    /// address run, so `queue report-complete` requeued every hand-back with
    /// `#reviewfixonhead` and no agent command could close it. A token inside
    /// quotes or a code span describes another row and is never a directive.
    #[test]
    fn a_quoted_address_token_is_not_an_address_run() {
        let presets = presets();
        let row = STOPLOOPCAP_ROW;
        assert!(!head_is_address_run(row, &presets), "{row}");
        assert!(!item_mentions_address_preset(row));
        assert!(!crate::inline_discussion::item_mentions_inline_discussion_goal(row));
        for quoted in [
            "the debt was \"#address\" on another row",
            "the debt was “#address” on another row",
            "the debt was ‘#address’ on another row",
            "the debt was `#address` on another row",
        ] {
            assert!(!head_is_address_run(quoted, &presets), "{quoted}");
            assert!(!item_mentions_address_preset(quoted), "{quoted}");
        }
        // A real directive still counts, also after an apostrophe or a quote.
        for real in [
            "it doesn't land 'yet' https://github.com/sample-org/sample-repo/pull/12: #address",
            "https://github.com/sample-org/sample-repo/pull/12 'context' #address",
        ] {
            assert!(head_is_address_run(real, &presets), "{real}");
            assert!(item_mentions_address_preset(real), "{real}");
        }
    }

    /// `#directiveposition`: the live agent-loop `agent-doc-bugs.md` rows
    /// (2026-11-24) `noopaddress` and `directivepos` name `#address` and
    /// `#addressproof` in descriptive prose, outside any quote. Only a bare
    /// directive row or the row's leading / trailing tags are directives, so
    /// neither row is an address run and its hand-back takes the ordinary
    /// report path instead of a `#reviewfixonhead` requeue.
    #[test]
    fn a_prose_address_token_is_not_an_address_run() {
        let presets = presets();
        for row in [
            NOOPADDRESS_ROW.to_string(),
            format!("📥 {NOOPADDRESS_ROW}"),
            format!("🚧 [#subagent:noopaddr#1] {NOOPADDRESS_ROW}"),
            DIRECTIVEPOS_ROW.to_string(),
            STOPLOOPCAP_ROW.to_string(),
            format!("📥 {STOPLOOPCAP_ROW}"),
            "fix: a no-op #address could not close, see PR 12".to_string(),
        ] {
            assert!(!head_is_address_run(&row, &presets), "{row}");
            assert!(!item_mentions_address_preset(&row), "{row}");
            assert!(
                !crate::inline_discussion::item_mentions_inline_discussion_goal(&row),
                "{row}"
            );
            assert!(
                crate::preset_order::temporal_preset_order(&row, &presets).is_empty(),
                "{row}"
            );
        }
        // Directive position still counts: a bare row, leading tags, and the
        // trailing tags, with metadata, connectors, and punctuation between.
        for real in [
            "#address",
            "#address [#q-1]",
            "📥 [#subagent:fix#2] #address [#q-1]",
            "#address https://github.com/sample-org/sample-repo/pull/12",
            "https://github.com/sample-org/sample-repo/pull/12: #address",
            "https://github.com/sample-org/sample-repo/pull/12: #address.",
            "https://github.com/sample-org/sample-repo/pull/12: #address then #adversarial-pr",
            "https://github.com/sample-org/sample-repo/pull/12: #address [operator-verify] [#q-1]",
            "[from:chat key=x] https://github.com/sample-org/sample-repo/pull/12 — #address",
        ] {
            assert!(head_is_address_run(real, &presets), "{real}");
            assert!(item_mentions_address_preset(real), "{real}");
        }
    }

    const NOOPADDRESS_ROW: &str = "[from:chat key=noopaddress] fix: agent-doc must never require the operator to delete or edit a row to satisfy its own hook. Operator: 'The operator should not have to delete a row to resolve agent-doc's hook. Why does agent-doc now require the user to delete a row? Is there another solution that agent-doc can do to resolve these issue?' On sdk-dev.md the agent told the operator '2. 📥 #address under #389 … needs you to delete it' because a no-op #address (PR #389 had nothing to address) could not be closed by any agent path: #addressproof (goal_loop_handoff address_effect_unproven / address_effect.rs) requeues an address hand-back with no observed head change and no review reply, and a no-op hand-back has neither. Make 'nothing to address' a provable agent outcome (observe no unresolved review threads, no requested changes, no failing required checks on the current head and record it as the address effect, or settle via the declared NO-WRITTEN-FIX final state with that evidence), and audit every Stop/closeout refusal and every agent-facing hint so each names an agent-executable command, never 'delete the row' or any operator document edit. #spec-test-build-install-commit-push";

    const DIRECTIVEPOS_ROW: &str = "[from:dogfood key=directivepos] fix: two finished agent-doc-bugs.md rows (the stoploopcap and noopaddress rows) still have no agent close path on build 0.35.589+b07b1936f8ce734f: (1) queue report-complete keeps treating them as #address hand-backs and requeues them (#reviewfixonhead) because their prose mentions #address/#addressproof outside quotes — 40a91157e6 only skips quoted spans; a preset/directive must count only in directive position (a bare directive row like '#address', or the trailing preset tags), never inside descriptive prose; (2) after `queue freeze` of the finished attempt, `queue consume --ack-text … --summary` still refuses with 'refuses queue work with a live claim', and --ack-owner 'subagent:x#1' reports 'did not match an exact --ack-text lineage'. A frozen/finished attempt must not block the coordinator's ack-text closeout. Replay both live rows as failing tests, fix, and make sure every refusal names a working agent command. #spec-test-build-install-commit-push";

    const STOPLOOPCAP_ROW: &str = "[from:dogfood key=stoploopcap] fix: the Claude Stop #transientcloseout gate refused 9 consecutive recursive Stops (stop_hook_active=true, debts=1, unchanged) on sdk-dev.md until Claude Code's block cap overrode it ('A hook blocked the turn from ending 9 consecutive times'), 2026-11-24 00:04–00:05Z. The debt was '2. 📥 #address' (a numbered step whose claims, s389address gens 1–3, never projected onto the row), so state=inbox_or_running never releases. #stophookactiveguard promises the harness cap is never the only guard. A 📥/🚧 row the agent provably cannot move in this turn (claimed this turn but still unprojected, or no claim/close path resolves it) must be released on an unchanged recursive Stop with a systemMessage naming it, and every state needs a bounded refusal count per unchanged debt set. #spec-test-build-install-commit-push";

    #[test]
    fn a_superseded_review_without_the_written_fix_does_not_land() {
        let response = format!(
            "Every actionable item on {REVIEW} is already fixed on head 4b1ec500. No new commit.\n\
             Earlier reviews that asked for the shadow apply were superseded by the review above and were not reopened."
        );
        assert!(address_fix_receipts(&response).is_empty());
        assert!(address_response_admits_fix_absent(&response));
        assert!(!address_run_lands_written_fix(&response));
    }

    #[test]
    fn admitting_the_written_fix_is_not_the_code_does_not_land_even_with_a_receipt() {
        let response = format!(
            "FIX-ON-HEAD {REVIEW} head={HEAD}\n\
             The review's written fix, store everything as allowed, is not the code."
        );
        assert!(!address_fix_receipts(&response).is_empty());
        assert!(!address_run_lands_written_fix(&response));
    }

    #[test]
    fn a_receipt_for_the_head_that_contains_the_written_fix_lands() {
        let response = format!(
            "The retry applies under shadow. The stored state, the audit applied_state, and the acknowledgement are allowed.\n\
             FIX-ON-HEAD {REVIEW} head={HEAD}"
        );
        assert_eq!(
            address_fix_receipts(&response),
            vec![(REVIEW.to_string(), HEAD.to_string())]
        );
        assert!(address_run_lands_written_fix(&response));
        assert!(!address_run_settled_without_written_fix(&response));
    }

    #[test]
    fn no_written_fix_settles_an_address_run_and_prose_does_not() {
        let response = format!(
            "**Summary**\n\nThe approval states no code change, so nothing was committed.\n\
             NO-WRITTEN-FIX head={HEAD}\n"
        );
        assert_eq!(no_written_fix_receipts(&response), vec![HEAD.to_string()]);
        assert!(address_run_lands_written_fix(&response));
        assert!(address_run_settled_without_written_fix(&response));

        let prose = "The approval does not ask for a code change. No FIX-ON-HEAD line, because it states no code edit.\n";
        assert!(no_written_fix_receipts(prose).is_empty());
        assert!(!address_run_lands_written_fix(prose));
    }

    #[test]
    fn both_fix_receipts_together_do_not_settle() {
        let response = format!("FIX-ON-HEAD {REVIEW} head={HEAD}\nNO-WRITTEN-FIX head={HEAD}\n");
        assert!(!address_run_lands_written_fix(&response));
        assert!(!address_run_settled_without_written_fix(&response));
    }
}
