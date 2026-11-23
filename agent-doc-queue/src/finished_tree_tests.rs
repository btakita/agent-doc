//! Tests for [`super`] (`#treefinish`).

use super::*;
use crate::response_tree_sync::{sync_response_trees, sync_response_trees_striking};

fn no_claims(_: &str) -> bool {
    false
}

fn doc(queue: &str) -> String {
    format!(
        "---\nagent_doc_session: finished-tree-fixture\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue id=\"main\" -->\n{queue}<!-- /agent:queue -->\n"
    )
}

/// Sync with no claims; asserts a second sync is a no-op.
fn sync(document: &str) -> String {
    let synced = sync_response_trees_striking(document, &no_claims)
        .map_or_else(|| document.to_string(), |synced| synced.document);
    assert!(
        sync_response_trees_striking(&synced, &no_claims).is_none(),
        "a second sync changes nothing:\n{synced}"
    );
    synced
}

fn guidance_under(document: &str, response: &str) -> String {
    let at = document.find(response).expect("response");
    document[at..]
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("- **Guidance:** "))
        .expect("guidance")
        .to_string()
}

/// The audited `--ack-text` tree: every row and action item is done except
/// operator rows asking to clean up artifacts self-repair already removed.
/// The whole tree is struck from the root down and the root says it may go.
#[test]
fn the_ack_text_tree_is_struck_once_self_repair_resolved_its_operator_rows() {
    let session = include_str!("../tests/fixtures/response_remaining_session.md");
    let leaf = "        - Complete the rest of the work.";
    let at = session.find(leaf).unwrap();
    let end = at + session[at..].find('\n').unwrap();
    let body = &session[at + "        - ".len()..end];
    let document = format!("{}        - ~~{body}~~{}", &session[..at], &session[end..]);
    let (repaired, _) = crate::self_repair::self_repair_document(&document, &|_, _| false, &|id| {
        id == "[#r-8eb6516470b1]"
    })
    .unwrap();
    let operator_row = "    - 🙋 follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the editor, delete the misplaced copy";
    assert!(repaired.contains(operator_row));

    let synced = sync(&repaired);
    assert!(
        synced.contains(
            "- ### ~~🗑\u{fe0f} follow up on an action item from queue item “follow up on an action item from queue item 'agent-doc duplicated my prompt ` - …”: agent: stop `queue consume --ack-text`"
        ),
        "the root is struck:\n{synced}"
    );
    assert!(
        synced.contains(
            "    - ~~follow up on an action item from queue item “follow up on an action item from queue item 'follow up on an action item from qu…”: operator: in the editor, delete the misplaced copy"
        ),
        "the resolved operator row is struck, its text kept:\n{synced}"
    );
    assert!(
        !synced.contains(operator_row),
        "no live 🙋 row is left:\n{synced}"
    );
    let guidance = guidance_under(&synced, "- **Response [#r-2da6da4a7fd8]**");
    assert!(guidance.starts_with("✅ "), "{guidance}");
    assert!(
        guidance.ends_with(" done. 🗑️ Safe to delete this tree."),
        "{guidance}"
    );
    // Every row of the tree is struck.
    let tree = crate::queue_tree::queue_tree(&synced, &QueueTreeOptions::default()).unwrap();
    let root = tree
        .nodes
        .iter()
        .position(|node| {
            node.full_text
                .contains("agent: stop `queue consume --ack-text`")
        })
        .unwrap();
    assert!(
        crate::queue_tree::tree_indexes(&tree, root).all(|index| tree.nodes[index].closed),
        "{}",
        crate::queue_tree::render_text(&tree)
    );
}

const OPEN_OPERATOR_TREE: &str = "\
- ### ~~do [#rootwork]~~
  - **Response [#r-aaaaaaaaaaaa]**
    - **Timestamp:** 2026-11-22T00:00:00.000Z
    - **Summary:** Done.
    - **State:** Complete.
    - **Guidance:** Safe to delete this completed row; the item is complete and has no remaining tracked or unpublished work.
  - ~~step one~~
    - **Response [#r-cccccccccccc]**
      - **Timestamp:** 2026-11-22T00:00:00.000Z
      - **Summary:** Done.
      - **State:** Complete.
      - **Guidance:** This row is done, but its root item [#rootwork] is still open: `operator: approve the release…` is open. Safe to delete this completed row (nothing tracked or unpublished remains in it); keep the rest of the tree.
  - 🙋 follow up on an action item from queue item “do [#rootwork]”: operator: approve the release in the dashboard. [operator-verify] [#q-0123456789ab]
";

/// A tree with one open operator row is never struck; its root says to keep
/// it, naming the row, and a finished child says only that it is done.
#[test]
fn an_open_operator_row_keeps_its_tree() {
    let document = doc(OPEN_OPERATOR_TREE);
    let synced = sync(&document);
    assert!(
        synced.contains("  - 🙋 follow up on an action item from queue item “do [#rootwork]”: operator: approve the release in the dashboard. [operator-verify] [#q-0123456789ab]\n"),
        "the open operator row is untouched:\n{synced}"
    );
    assert_eq!(
        guidance_under(&synced, "**Response [#r-aaaaaaaaaaaa]**"),
        "🔒 Keep: 1 open — `operator: approve the release in the dashboard`."
    );
    assert_eq!(
        guidance_under(&synced, "**Response [#r-cccccccccccc]**"),
        "✅ Done."
    );
    assert!(
        crate::finished_tree::strike_finished_trees(&synced, &no_claims).is_none(),
        "nothing is struck"
    );

    // The operator strikes the row: the root says the tree may go.
    let done = synced.replace(
        "  - 🙋 follow up on an action item from queue item “do [#rootwork]”: operator: approve the release in the dashboard. [operator-verify] [#q-0123456789ab]",
        "  - ~~follow up on an action item from queue item “do [#rootwork]”: operator: approve the release in the dashboard. [operator-verify] [#q-0123456789ab]~~",
    );
    let done = sync(&done);
    assert_eq!(
        guidance_under(&done, "**Response [#r-aaaaaaaaaaaa]**"),
        "🗑️ Safe to delete this tree."
    );
    assert_eq!(
        guidance_under(&done, "**Response [#r-cccccccccccc]**"),
        "✅ Done. 🗑️ Safe to delete this tree."
    );
}

const DELEGATED_TREE: &str = "\
- ### origin task [#origin]
  - **Response [#r-bbbbbbbbbbbb]**
    - **Timestamp:** 2026-11-22T00:00:00.000Z
    - **Summary:** Shipped.
    - **Action items:** <!-- agent-doc:response-detail -->
      - agent: check CI for `abc1234`. <!-- agent-doc:response-detail -->
    - **State:** Follow-up work remains.
    - **Guidance:** More work remains. Keep this tree and follow the action items in this response.
  - 🙋 follow up on an action item from queue item “origin task [#origin]”: operator: approve the deploy. [operator-verify]
- follow up on an action item from queue item “origin task [#origin]”: agent: check CI for `abc1234`.
- 🙋 follow up on an action item from queue item “origin task”: operator: approve the deploy. [operator-verify]
";

/// An origin tree whose only remaining work moved to top-level follow-up rows
/// is finished: it is struck, and the follow-up rows stay live.
#[test]
fn an_origin_tree_whose_work_moved_to_top_level_followups_is_struck() {
    let document = doc(DELEGATED_TREE);
    let synced = sync(&document);
    assert!(
        synced.contains("- ### ~~🗑\u{fe0f} origin task [#origin]~~\n"),
        "the origin root is struck:\n{synced}"
    );
    assert!(
        synced.contains("  - ~~follow up on an action item from queue item “origin task [#origin]”: operator: approve the deploy. [operator-verify]~~\n"),
        "the delegated nested row is struck:\n{synced}"
    );
    assert!(
        synced.contains("- follow up on an action item from queue item “origin task [#origin]”: agent: check CI for `abc1234`.\n")
            && synced.contains("- 🙋 follow up on an action item from queue item “origin task”: operator: approve the deploy. [operator-verify]\n"),
        "the top-level follow-up rows carry the work and stay live:\n{synced}"
    );
    assert!(synced.contains("    - **State:** Complete.\n"), "{synced}");
    let guidance = guidance_under(&synced, "**Response [#r-bbbbbbbbbbbb]**");
    assert!(
        guidance.ends_with(" done. 🗑️ Safe to delete this tree."),
        "{guidance}"
    );
}

/// A row a live worker claims is never struck, and neither is its tree.
#[test]
fn a_live_claimed_row_is_never_struck() {
    let document = doc(DELEGATED_TREE);
    let claimed = |row: &str| row.contains("origin task [#origin]") && !row.contains("follow up");
    let synced = sync_response_trees_striking(&document, &claimed)
        .expect("the response still syncs")
        .document;
    assert!(
        synced.contains("- ### origin task [#origin]\n"),
        "the claimed root stays live:\n{synced}"
    );
    assert!(
        synced.contains("  - 🙋 follow up on an action item from queue item “origin task [#origin]”: operator: approve the deploy. [operator-verify]\n"),
        "nothing in its tree is struck:\n{synced}"
    );
    assert!(strike_finished_trees(&synced, &claimed).is_none());
    // The plain sync never strikes.
    let (plain, _, _) = sync_response_trees(&document).unwrap();
    assert!(plain.contains("- ### origin task [#origin]\n"), "{plain}");
}

/// A `keep` queue's top-level row stays live; its finished rows are struck.
#[test]
fn a_keep_queue_keeps_its_top_level_row() {
    let document = doc(DELEGATED_TREE).replace(
        "<!-- agent:queue id=\"main\" -->",
        "<!-- agent:queue id=\"main\" keep -->",
    );
    let synced = sync(&document);
    // `#treetopglyph`: the kept root says to keep it; its finished row may go.
    assert!(
        synced.contains("- ### 🔒 origin task [#origin]\n"),
        "{synced}"
    );
    assert!(
        synced.contains("  - ~~🗑\u{fe0f} follow up on an action item from queue item “origin task [#origin]”: operator: approve the deploy. [operator-verify]~~\n"),
        "{synced}"
    );
}

/// Striking wraps only the row text: operator edits in it are kept.
#[test]
fn the_strike_keeps_row_text_byte_for_byte() {
    let document = doc(
        "- ### epic\n  - ~~done step~~\n  - follow up on an action item from queue item “epic”: agent: re-run `make check` (operator note: flaky on CI).\n- follow up on an action item from queue item “elsewhere”: agent: re-run `make check` (operator note: flaky on CI).\n",
    );
    let synced = sync(&document);
    assert!(
        synced.contains("  - ~~follow up on an action item from queue item “epic”: agent: re-run `make check` (operator note: flaky on CI).~~\n"),
        "{synced}"
    );
    assert!(synced.contains("- ### ~~🗑\u{fe0f} epic~~\n"), "{synced}");
}

/// A row whose tracked id is still open is never struck: the strike would
/// read as that item's completion.
#[test]
fn a_row_tracking_an_open_item_is_never_struck() {
    let document = doc(&DELEGATED_TREE.replace("- ### origin task [#origin]\n", "- ### do [#origin]\n")).replace(
        "<!-- /agent:queue -->\n",
        "<!-- /agent:queue -->\n\n<!-- agent:todo -->\n- [ ] [#origin] origin task\n<!-- /agent:todo -->\n",
    );
    let synced = sync_response_trees_striking(&document, &no_claims)
        .expect("the response still syncs")
        .document;
    assert!(synced.contains("- ### do [#origin]\n"), "{synced}");
    assert!(strike_finished_trees(&synced, &no_claims).is_none());
    // Once the item is done, the tree is struck.
    let done = synced.replace("- [ ] [#origin]", "- [x] [#origin]");
    let struck = sync(&done);
    assert!(
        struck.contains("- ### ~~🗑\u{fe0f} do [#origin]~~\n"),
        "{struck}"
    );
}

/// Short follow-up titles (`[#<origin>] follow-up: <item>`) delegate the
/// same way as the legacy form.
#[test]
fn a_short_title_followup_delegates_its_origin_tree() {
    let document = doc("\
- ### do [#shortorigin]
  - ~~step~~
  - 🙋 [#shortorigin] follow-up: operator: approve the deploy. [operator-verify]
- 🙋 [#shortorigin] follow-up: operator: approve the deploy. [operator-verify] [#q-0123456789ab]
");
    let synced = sync(&document);
    assert!(
        synced.contains("- ### ~~🗑\u{fe0f} do [#shortorigin]~~\n"),
        "{synced}"
    );
    assert!(
        synced.contains(
            "  - ~~[#shortorigin] follow-up: operator: approve the deploy. [operator-verify]~~\n"
        ),
        "{synced}"
    );
    assert!(
        synced.contains("- 🙋 [#shortorigin] follow-up: operator: approve the deploy. [operator-verify] [#q-0123456789ab]\n"),
        "{synced}"
    );
}
