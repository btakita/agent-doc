use super::*;

use agent_doc_element_queue::{
    queue_head_identity, queue_prompt_stable_identity, strip_attempt_projection_prefix,
    struck_work_text,
};

/// The anonymized live Run section the operator asked about: a recovery row
/// whose tree holds a running CI check and an answered restart, a cleanup
/// row with a decision beneath it, a prompt blocker with an answered child,
/// running workers, blocked rows and top-level operator rows.
const SESSION: &str = include_str!("../tests/fixtures/open_count_session.md");

const RECOVERY_ROW: &str = "- ### 🙋 [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]";
const RECOVERY_COUNTED: &str = "- ### 🙋 (1 decision, 1 running) [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]";
const CLEANUP_PREFIX: &str = "- ### 🙋 (1 decision) follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”";
const CI_ROW: &str = "  - 🚧 [#subagent:cizip#1] follow up on an action item from queue item “I authorize the combined `262` through `263.*` ZIP. ⟦published⟧”: agent: check CI run 45085206369 on `3b3561a1b` once it finishes. The new `verifyPlugin` step downloads IntelliJ 263 and the plugin verifier on the CI runner. [#q-f2ec555891ae]";
const RESTART_REPLY: &str = "    - ~~Restarted~~\n";

fn synced(document: &str) -> String {
    reconcile_open_counts(document)
        .unwrap()
        .unwrap_or_else(|| document.to_string())
}

fn changed_lines(before: &str, after: &str) -> Vec<(String, String)> {
    assert_eq!(before.lines().count(), after.lines().count());
    before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

/// The operator's example: the recovery row says `(1 decision, 1 running)`.
/// Its own decision counts; the restart row the operator already answered
/// (`~~Restarted~~`) does not; the running CI check does. Only top-level rows
/// with open work beneath them change.
#[test]
fn top_level_rows_carry_the_open_work_of_their_tree() {
    let after = synced(SESSION);
    assert!(after.contains(&format!("{RECOVERY_COUNTED}\n")), "{after}");
    let changed = changed_lines(SESSION, &after);
    assert_eq!(changed.len(), 2, "{changed:#?}");
    assert_eq!(
        changed[0],
        (RECOVERY_ROW.to_string(), RECOVERY_COUNTED.to_string())
    );
    assert!(changed[1].1.starts_with(CLEANUP_PREFIX), "{changed:#?}");
    // A leaf row with nothing beneath it, a running leaf and a blocked row
    // carry no token: their own glyph says what they are.
    for line in after.lines() {
        if line.contains("[#q-797479c6911e]")
            || line.contains("[#subagent:ctrlleak#1]")
            || line.contains("[#ghrecvpolicy]")
            || line.contains("[#prompt-blocker-b0ddfbae2f94]: Use")
        {
            assert!(
                agent_doc_element_queue::open_count_token(strip_attempt_projection_prefix(
                    line.trim_start_matches("- ### ")
                ))
                .is_none()
                    && !line.contains("decision")
                    && !line.contains(" action)"),
                "{line}"
            );
        }
    }
}

#[test]
fn a_second_sync_is_a_no_op() {
    let once = synced(SESSION);
    assert_eq!(reconcile_open_counts(&once).unwrap(), None);
}

/// The count agrees with the Loose ends digest: the token changes no row's
/// classification, and the digest of the counted document is the digest of
/// the original.
#[test]
fn the_count_and_the_digest_read_the_same_rows() {
    let after = synced(SESSION);
    assert_eq!(
        crate::loose_ends::loose_ends(&after).unwrap(),
        crate::loose_ends::loose_ends(SESSION).unwrap()
    );
    let before = queue_tree(SESSION, &QueueTreeOptions::default()).unwrap();
    let counted = queue_tree(&after, &QueueTreeOptions::default()).unwrap();
    assert_eq!(before.nodes.len(), counted.nodes.len());
    for (a, b) in before.nodes.iter().zip(&counted.nodes) {
        assert_eq!(
            (&a.text, &a.full_text, &a.id, a.state, a.closed, a.depth),
            (&b.text, &b.full_text, &b.id, b.state, b.closed, b.depth)
        );
    }
}

/// The count follows the tree: an unanswered child adds an action, striking
/// the running child takes it out, and once nothing beneath the row is open
/// the token is gone.
#[test]
fn the_count_follows_each_child_row_as_it_resolves() {
    let unanswered = SESSION.replacen(RESTART_REPLY, "", 1);
    let after = synced(&unanswered);
    assert!(
        after.contains("- ### 🙋 (1 decision, 1 action, 1 running) [from:recovery"),
        "{after}"
    );

    let ci_struck = after.replacen("  - 🚧 [#subagent:cizip#1] follow up", "  - ~~follow up", 1);
    let ci_struck = ci_struck.replacen(
        "plugin verifier on the CI runner. [#q-f2ec555891ae]",
        "plugin verifier on the CI runner. [#q-f2ec555891ae]~~",
        1,
    );
    assert_ne!(ci_struck, after);
    let after = synced(&ci_struck);
    assert!(
        after.contains("- ### 🙋 (1 decision, 1 action) [from:recovery"),
        "{after}"
    );
    assert_eq!(reconcile_open_counts(&after).unwrap(), None);

    // The operator answers the restart row: nothing beneath the recovery row
    // is open, so the token is removed and the row is byte-for-byte the
    // original.
    let answered = after.replacen(
        "[operator-verify] [#q-56fa509fdf08]\n",
        &format!("[operator-verify] [#q-56fa509fdf08]\n{RESTART_REPLY}"),
        1,
    );
    let after = synced(&answered);
    assert!(after.contains(&format!("{RECOVERY_ROW}\n")), "{after}");
    assert_eq!(reconcile_open_counts(&after).unwrap(), None);

    // The original fixture with the running row struck says the same.
    let original_struck = SESSION.replacen(CI_ROW, "  - ~~CI check~~", 1);
    assert_ne!(original_struck, SESSION);
    assert!(synced(&original_struck).contains(&format!("{RECOVERY_ROW}\n")));
}

/// A token left on a row that is no longer top-level (the operator nested
/// it), or a stale count, is re-rendered; struck rows are never edited.
#[test]
fn stale_tokens_are_corrected_and_struck_rows_left_alone() {
    let stale = synced(SESSION).replacen("(1 decision, 1 running)", "(3 decisions)", 1);
    assert!(synced(&stale).contains(&format!("{RECOVERY_COUNTED}\n")));

    let nested = SESSION.replacen(
        "    - ~~Restarted~~\n",
        "    - ~~Restarted~~\n  - (2 blocked) nested operator note\n",
        1,
    );
    assert!(synced(&nested).contains("\n  - nested operator note\n"));

    let struck = "- ~~(1 running) done work~~";
    assert_eq!(line_with_open_count(struck, None), None);
    assert_eq!(line_with_open_count(struck, Some("(1 decision)")), None);
}

/// Only the token is rewritten: spacing elsewhere in the row is kept, and
/// the token goes after the whole attempt projection prefix (glyph, owner
/// label), before the work text.
#[test]
fn only_the_token_is_rewritten() {
    assert_eq!(line_with_open_count("- 🙋  work   text", None), None);
    assert_eq!(
        line_with_open_count("- ### 🚧 [#subagent:x#1] work", Some("(1 running)")).as_deref(),
        Some("- ### 🚧 [#subagent:x#1] (1 running) work")
    );
    assert_eq!(
        line_with_open_count("  - ### plain work", Some("(2 actions)")).as_deref(),
        Some("  - ### (2 actions) plain work")
    );
    assert_eq!(
        line_with_open_count("- ### 🙋 (2 actions) work", None).as_deref(),
        Some("- ### 🙋 work")
    );
    assert_eq!(
        line_with_open_count("- ### 🙋 (2 actions) work", Some("(2 actions)")),
        None
    );
    // An operator parenthetical is work text, never a token.
    assert_eq!(
        line_with_open_count("- (see above) work", Some("(1 decision)")).as_deref(),
        Some("- (1 decision) (see above) work")
    );
}

/// `#looseends`: the token is never part of a row's identity, claim key,
/// ack-text match or dedupe key. Every parser strips it with the rest of the
/// projection prefix, the way claim labels are stripped.
#[test]
fn identity_ack_and_dedupe_keys_ignore_the_token() {
    let rows = [
        (
            RECOVERY_ROW.trim_start_matches("- ### "),
            RECOVERY_COUNTED.trim_start_matches("- ### "),
        ),
        (
            "🚧 [#subagent:x#1] do [#jbfrontendswap]: approved",
            "🚧 [#subagent:x#1] (1 decision, 2 actions) do [#jbfrontendswap]: approved",
        ),
        (
            "[from:chat key=topcount] free-text work",
            "(1 running) [from:chat key=topcount] free-text work",
        ),
        (
            "follow up on an action item from `x`: agent: check CI [#q-0123456789ab]",
            "(4 blocked, 1 running) follow up on an action item from `x`: agent: check CI [#q-0123456789ab]",
        ),
    ];
    for (bare, counted) in rows {
        assert_eq!(
            strip_attempt_projection_prefix(counted),
            strip_attempt_projection_prefix(bare),
            "{counted}"
        );
        assert_eq!(struck_work_text(counted), struck_work_text(bare));
        assert_eq!(queue_head_identity(counted), queue_head_identity(bare));
        assert_eq!(
            queue_prompt_stable_identity(counted),
            queue_prompt_stable_identity(bare)
        );
        assert_eq!(
            crate::queue_claim::claim_identity(counted),
            crate::queue_claim::claim_identity(bare)
        );
        // The CRDT / dedupe keyer and the ack-text matcher.
        assert_eq!(
            agent_doc_element_queue::strip_priority_markers(counted),
            agent_doc_element_queue::strip_priority_markers(bare)
        );
        assert_eq!(
            agent_doc_document::queue_projection::strip_priority_markers(counted),
            agent_doc_document::queue_projection::strip_priority_markers(bare)
        );
        for heading in ["", "- ", "- ### "] {
            assert_eq!(
                crate::document_queue::normalized_queue_line_for_match(&format!(
                    "{heading}{counted}"
                )),
                crate::document_queue::normalized_queue_line_for_match(&format!("{heading}{bare}")),
            );
        }
        assert_eq!(
            crate::followup_owner::followup_owner_key(counted),
            crate::followup_owner::followup_owner_key(bare)
        );
        // A strike drops the token with the rest of the projection.
        assert_eq!(
            agent_doc_element_queue::render_struck_row_body(counted),
            agent_doc_element_queue::render_struck_row_body(bare)
        );
    }
}
