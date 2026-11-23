//! `#answeredentry`: the live fixed point after 65b900c8bc, driven through
//! the same entry points `agent-doc queue sync`
//! ([`crate::reconcile_incremental::queue_sync_response_chain`]) and the
//! controller's background maintenance
//! ([`crate::reconcile_incremental::reconcile_full`]) run.

use crate::reconcile_incremental::{ReconcileInputs, queue_sync_response_chain, reconcile_full};

use super::*;

/// The step-two tree as two live `queue sync` passes left it. The
/// superseded-blocker pass ran before self-repair and rewrote the nested
/// `[#r-6837aa93963f]` response to `Complete.` + `Historical blocker
/// resolved…`, so its Remaining entry stayed `🙋` (no pass managed it any
/// more), the torn duplicate `🙋 1/2` Guidance under the hoisted entry no
/// longer had a twin and survived, and with the operator's answer nested
/// under that stray line the root read `✅ 4/5 done.`: a ✅ glyph beside an
/// incomplete count.
const LIVE: &str = include_str!("../tests/fixtures/response_remaining_live_historical_blocker.md");
const ROOT_RESPONSE: &str = "- **Response [#r-a4794e8fee5c]**";
const NESTED_RESPONSE: &str = "- **Response [#r-6837aa93963f]**";
const NESTED_ENTRY: &str = "              - ✅ operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n";
const OUTER_ANSWER: &str = "      - ✅ operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n        - ~~VSCodium not running. [#q-c4b5011d0725]~~\n";

fn with_inputs<T>(f: impl FnOnce(&ReconcileInputs<'_>) -> T) -> T {
    f(&ReconcileInputs {
        is_live: &|_, _| false,
        response_proven: &|_| true,
        struck_provenance: &|_| false,
        is_claimed: &|_| false,
    })
}

/// The State and Guidance values of the response `marker`.
fn state_guidance(document: &str, marker: &str) -> (String, String) {
    let start = document.find(marker).expect("response");
    let indent = document[..start].len() - document[..start].trim_end_matches(' ').len();
    let detail = format!("{}- ", " ".repeat(indent + 2));
    let mut state = None;
    let mut guidance = None;
    for line in document[start..].lines().skip(1) {
        let line_indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && line_indent <= indent {
            break;
        }
        let Some(rest) = line.strip_prefix(&detail) else {
            continue;
        };
        if let Some(value) = rest.strip_prefix("**State:** ") {
            state.get_or_insert(value.to_string());
        } else if let Some(value) = rest.strip_prefix("**Guidance:** ") {
            guidance.get_or_insert(value.to_string());
        }
    }
    (state.expect("a State"), guidance.expect("a Guidance"))
}

fn assert_converged(document: &str) {
    let (state, guidance) = state_guidance(document, NESTED_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{document}");
    assert_eq!(guidance, "✅ 2/2 done.", "{document}");
    assert!(document.contains(NESTED_ENTRY), "{document}");
    let (state, guidance) = state_guidance(document, ROOT_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{document}");
    assert_eq!(
        guidance,
        format!("✅ 5/5 done. {NOTHING_REMAINS_ROW_GUIDANCE}"),
        "{document}"
    );
    // The stray Guidance is gone: the answer is the hoisted entry's first
    // child again.
    assert!(document.contains(OUTER_ANSWER), "{document}");
    assert!(!document.contains(OPERATOR_OPEN_GLYPH), "{document}");
    assert!(!document.contains("yours"), "{document}");
    assert!(!document.contains("Historical blocker"), "{document}");
    assert!(
        document.contains("- ### ~~🗑️ [#q-e9483473162d] follow-up:"),
        "{document}"
    );
}

#[test]
fn queue_sync_converges_the_live_fixed_point() {
    let first = with_inputs(|inputs| queue_sync_response_chain(LIVE, inputs)).document;
    assert_converged(&first);
    let second = with_inputs(|inputs| queue_sync_response_chain(&first, inputs)).document;
    assert_eq!(second, first, "a second queue sync is a no-op");
}

#[test]
fn controller_maintenance_converges_the_live_fixed_point() {
    let first = with_inputs(|inputs| reconcile_full(LIVE, inputs)).document;
    assert_converged(&first);
    let second = with_inputs(|inputs| reconcile_full(&first, inputs)).document;
    assert_eq!(second, first, "a second maintenance pass is a no-op");
    // Either path reaches the same tree from the other's output.
    let via_sync = with_inputs(|inputs| queue_sync_response_chain(&first, inputs)).document;
    assert_eq!(via_sync, first);
}

/// The order that made the live tree: the original torn tree (the nested
/// response still `Waiting on operator input.` with its 🙋 tally, the
/// duplicate tally under the hoisted entry) through `queue sync` lands on
/// the same converged tree, so the superseded-blocker pass never rewrites a
/// tally the Remaining pass owns.
#[test]
fn queue_sync_from_the_torn_tree_never_writes_a_historical_blocker() {
    let torn = LIVE
        .replace(
            "            - **State:** Complete.\n            - **Guidance:** Historical blocker resolved by the owning prompt tree is durably complete. This response remains as history and requires no operator action.\n",
            "            - **State:** Waiting on operator input.\n            - **Guidance:** 🙋 1/2 done, 1 yours. Only you can finish this: reload any open VSCodium windows. Strike or answer its row when done.\n",
        )
        .replace(
            "    - **State:** Complete.\n    - **Guidance:** ✅ 4/5 done. 🗑️ Safe to delete this tree.\n",
            "    - **State:** Waiting on operator input.\n    - **Guidance:** 🙋 4/5 done, 1 yours. Only you can finish this: reload any open VSCodium windows. Strike or answer its row when done.\n",
        );
    assert_ne!(torn, LIVE);
    let first = with_inputs(|inputs| queue_sync_response_chain(&torn, inputs)).document;
    assert_converged(&first);
    let second = with_inputs(|inputs| queue_sync_response_chain(&first, inputs)).document;
    assert_eq!(second, first);
}
