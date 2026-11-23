//! `#ftquotedid`: a row that quotes the `[operator-verify]` tag inside inline
//! code is about a tagged row, not tagged itself. It is agent work: claimable,
//! dispatchable, never rested as `🙋`, never a Decide/Do loose end.

/// The operator's request, verbatim (it quotes an operator-verify row).
const QUOTING: &str = "The top-level item should have a summary count of what needs to be worked on in the subtrees. So `- ### 🙋 [from:recovery key=q-0e29d5fdf1c4-restore] follow up on an action item from `jbdocdrift`: operator: Decide whether to retain separate 262 and 263 modular ZIPs or authorize the combined `262` through `263.*` ZIP. [operator-verify] [#q-0e29d5fdf1c4]` should have `(1 desicion, 1 running)`. This status must be kept in sync and not drift from the actual state.";

const SHORT: &str = "[from:chat key=topcount] fix: a row that quotes a `[operator-verify]` token inside a code span is misread. #spec-test-build-install-commit-push";

#[test]
fn a_quoted_operator_verify_tag_leaves_the_row_claimable() {
    for row in [QUOTING, SHORT] {
        assert!(
            !crate::queue_continuation::head_awaits_operator_inline(row),
            "{row}"
        );
        assert_eq!(
            crate::queue_continuation::rest_operator_verify_text(row),
            None
        );
        assert!(!crate::followup_owner::carries_operator_owned_tag(row));
        assert!(!crate::subagent_intent::opts_out_of_queue_subagents(row));
    }
    // The row's own tag, outside code, still gates it.
    let tagged = format!("{SHORT} [operator-verify]");
    assert!(crate::queue_continuation::head_awaits_operator_inline(
        &tagged
    ));
    assert!(crate::subagent_intent::opts_out_of_queue_subagents(&tagged));
}

#[test]
fn a_quoted_operator_verify_tag_is_not_an_operator_loose_end() {
    let document = format!(
        "<!-- agent:queue -->\n- {SHORT}\n- operator: approve it. [operator-verify] [#q-0123456789ab]\n<!-- /agent:queue -->\n"
    );
    let ends = crate::loose_ends::loose_ends(&document).unwrap();
    assert_eq!(
        ends.iter().map(|end| end.id.as_str()).collect::<Vec<_>>(),
        ["q-0123456789ab"],
        "{ends:#?}"
    );
}
