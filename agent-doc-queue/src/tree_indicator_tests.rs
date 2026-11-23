//! Tests for [`super`] (`#treetopglyph`).

use super::*;
use crate::response_tree_sync::sync_response_trees_striking;

fn no_claims(_: &str) -> bool {
    false
}

fn doc_with(attrs: &str, queue: &str) -> String {
    format!(
        "---\nagent_doc_session: tree-glyph-fixture\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue id=\"main\"{attrs} -->\n{queue}<!-- /agent:queue -->\n"
    )
}

fn doc(queue: &str) -> String {
    doc_with("", queue)
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

/// The queue section of `document`, for readable assertions.
fn queue_of(document: &str) -> &str {
    let start = document.find("<!-- agent:queue").unwrap();
    let start = start + document[start..].find('\n').unwrap() + 1;
    let end = document.find("<!-- /agent:queue -->").unwrap();
    &document[start..end]
}

/// `document` with every tree glyph removed, the way an older binary wrote it.
fn without_glyphs(document: &str) -> String {
    document
        .replace("🗑\u{fe0f} ", "")
        .replace(&format!("{} ", TreeIndicator::Keep.glyph()), "")
}

const FINISHED_TREE: &str = "\
- ### ~~Fix the parser. ⟦published⟧~~
  - **Response [#r-aaaaaaaaaaaa]**
    - **Timestamp:** 2026-11-22T00:00:00.000Z
    - **Summary:** Fixed.
    - **State:** Complete.
    - **Guidance:** 🗑️ Safe to delete this tree.
  - ~~agent: check CI for `abc1234`. [#q-0123456789ab]~~
    - **Response [#r-bbbbbbbbbbbb]**
      - **Timestamp:** 2026-11-22T00:00:00.000Z
      - **Summary:** CI passed.
      - **State:** Complete.
      - **Guidance:** ✅ Done. 🗑️ Safe to delete this tree.
";

/// A finished tree: only its top-level row carries 🗑️, inside the strike.
#[test]
fn the_top_most_deletable_row_carries_the_delete_glyph() {
    let synced = sync(&doc(FINISHED_TREE));
    let queue = queue_of(&synced);
    assert!(
        queue.starts_with("- ### ~~🗑\u{fe0f} Fix the parser. ⟦published⟧~~\n"),
        "{queue}"
    );
    assert!(
        queue.contains("  - ~~agent: check CI for `abc1234`. [#q-0123456789ab]~~\n"),
        "a row under the 🗑️ row carries no glyph of its own:\n{queue}"
    );
    assert_eq!(
        queue.matches('🗑').count(),
        3,
        "row + two Guidance lines:\n{queue}"
    );
    // Nothing but the glyph changed.
    assert_eq!(without_glyphs(&synced), without_glyphs(&doc(FINISHED_TREE)));
}

const KEPT_TREE: &str = "\
- ### 🙋 follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs. [operator-verify] [#q-0e29d5fdf1c4]
  - ~~follow up on an action item from queue item “recover”: operator: restart the IDE. [operator-verify] [#q-56fa509fdf08]~~
    - ~~Restarted~~
  - ~~follow up on an action item from queue item “recover”: agent: check CI run 45085206369. [#q-f2ec555891ae]~~
    - **Response [#r-56cf9a1ad146]**
      - **Timestamp:** 2026-11-23T00:36:11.000Z
      - **Summary:** CI passed.
      - **State:** Complete.
      - **Guidance:** ✅ Done.
  - 🚧 [#subagent:pub#1] follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]
    - ~~agent: build the ZIP first.~~
";

/// A kept tree with deletable subtrees: 🔒 on the top-level row, after its
/// lifecycle marker; 🗑️ on each top-most deletable subtree; nothing on a
/// struck row whose parent is live work, or on a reply under a 🗑️ row.
#[test]
fn a_kept_root_with_deletable_subtrees_carries_the_keep_glyph() {
    let synced = sync(&doc(KEPT_TREE));
    let queue = queue_of(&synced);
    let expected = "\
- ### 🙋 🔒 follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs. [operator-verify] [#q-0e29d5fdf1c4]
  - ~~🗑\u{fe0f} follow up on an action item from queue item “recover”: operator: restart the IDE. [operator-verify] [#q-56fa509fdf08]~~
    - ~~Restarted~~
  - ~~🗑\u{fe0f} follow up on an action item from queue item “recover”: agent: check CI run 45085206369. [#q-f2ec555891ae]~~
";
    assert!(queue.starts_with(expected), "{queue}");
    assert!(
        queue.contains("  - 🚧 [#subagent:pub#1] follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]\n    - ~~🗑\u{fe0f} agent: build the ZIP first.~~\n"),
        "a struck leaf under live work is its own deletable subtree:\n{queue}"
    );
    assert_eq!(without_glyphs(&synced), without_glyphs(&doc(KEPT_TREE)));
}

/// No deletable subtree, no keep glyph: a tree that is all open work, or a
/// struck root that still has open work under it, carries nothing.
#[test]
fn a_kept_tree_without_deletable_subtrees_carries_no_glyph() {
    let open = doc(
        "- ### 🙋 operator: decide. [operator-verify] [#q-aaaaaaaaaaaa]\n  - 📥 agent: do the next step. [#q-bbbbbbbbbbbb]\n",
    );
    assert!(
        sync_response_trees_striking(&open, &no_claims).is_none_or(|s| !s.document.contains('🔒'))
    );
    let synced = sync(&open);
    assert!(!synced.contains('🔒') && !synced.contains('🗑'), "{synced}");
}

/// The glyphs flip as the tree changes: striking the last open rows turns
/// the 🔒 root into the 🗑️ row and removes the subtree glyphs; reopening a row
/// brings the keep glyph back.
#[test]
fn the_glyph_flips_when_the_tree_changes() {
    let kept = sync(&doc(KEPT_TREE));
    let finished = kept
        .replace(
            "- ### 🙋 🔒 follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs. [operator-verify] [#q-0e29d5fdf1c4]",
            "- ### ~~follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs. [operator-verify] [#q-0e29d5fdf1c4]~~",
        )
        .replace(
            "  - 🚧 [#subagent:pub#1] follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]",
            "  - ~~follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]~~",
        );
    let finished = sync(&finished);
    let queue = queue_of(&finished);
    assert!(
        queue.starts_with("- ### ~~🗑\u{fe0f} follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs."),
        "{queue}"
    );
    assert_eq!(
        queue.matches("~~🗑").count(),
        1,
        "only the top-level row carries 🗑️ now:\n{queue}"
    );
    assert!(!queue.contains('🔒'), "{queue}");

    // The operator reopens the publish row: the tree is kept again.
    let reopened = finished.replace(
        "  - ~~follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]~~",
        "  - 📥 follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]",
    );
    let reopened = sync(&reopened);
    let queue = queue_of(&reopened);
    assert!(
        queue.starts_with("- ### ~~🔒 follow up on an action item from queue item “recover”: operator: decide whether to keep the ZIPs."),
        "a struck root that must be kept carries 🔒 inside its strike:\n{queue}"
    );
    assert!(
        queue.contains("  - ~~🗑\u{fe0f} follow up on an action item from queue item “recover”: operator: restart the IDE."),
        "{queue}"
    );
    assert!(
        queue.contains("  - 📥 follow up on an action item from queue item “recover”: agent: publish the plugin. [#q-1111111111aa]\n    - ~~🗑\u{fe0f} agent: build the ZIP first.~~\n"),
        "{queue}"
    );
}

/// A stale, doubled, or misplaced glyph is repaired to the one the tree
/// says, and a glyph on a row that should carry none is removed.
#[test]
fn stale_glyphs_are_repaired() {
    let stale = doc(
        "- ### 📥 🔒 🔒 agent: do it. [#q-aaaaaaaaaaaa]\n- ### ~~🔒 done thing~~\n  - ~~🗑 done child~~\n",
    );
    let synced = sync(&stale);
    let queue = queue_of(&synced);
    assert_eq!(
        queue,
        "- ### 📥 agent: do it. [#q-aaaaaaaaaaaa]\n- ### ~~🗑\u{fe0f} done thing~~\n  - ~~done child~~\n"
    );
}

/// A loop queue's pass roster owns its strikes: it carries no glyph, and a
/// stale one is removed. A keep queue's top-level row is always kept.
#[test]
fn loop_and_keep_queues() {
    let looped = sync(&doc_with(
        " loop",
        "- ### ~~🗑\u{fe0f} pass one~~\n  - ~~step~~\n",
    ));
    assert_eq!(queue_of(&looped), "- ### ~~pass one~~\n  - ~~step~~\n");

    let kept = sync(&doc_with(
        " keep",
        "- ### recurring check\n  - ~~step one~~\n",
    ));
    assert_eq!(
        queue_of(&kept),
        "- ### 🔒 recurring check\n  - ~~🗑\u{fe0f} step one~~\n"
    );
}

/// The glyph is projection: identity, claim key, ack-text match, merge key
/// and the overlay node key read past it, live or struck.
#[test]
fn identity_and_ack_ignore_the_glyph() {
    use agent_doc_element_queue::{
        queue_head_identity, queue_prompt_stable_id, queue_prompt_stable_identity,
        strip_attempt_projection_prefix, struck_work_text,
    };
    for (marked, bare) in [
        (
            "🙋 🔒 follow up on …: operator: decide. [operator-verify] [#q-0e29d5fdf1c4]",
            "🙋 follow up on …: operator: decide. [operator-verify] [#q-0e29d5fdf1c4]",
        ),
        (
            "🚧 [#subagent:x#1] 🔒 do [#foo]",
            "🚧 [#subagent:x#1] do [#foo]",
        ),
        ("🔒 free text work", "free text work"),
        ("🗑\u{fe0f} do [#foo]: note", "do [#foo]: note"),
        ("🗑 do [#foo]: note", "do [#foo]: note"),
    ] {
        assert_eq!(
            strip_attempt_projection_prefix(marked),
            strip_attempt_projection_prefix(bare),
            "{marked}"
        );
        assert_eq!(struck_work_text(marked), struck_work_text(bare));
        assert_eq!(
            agent_doc_element_queue::strip_priority_markers(marked),
            agent_doc_element_queue::strip_priority_markers(bare)
        );
        // The ack-text matcher's normalizer.
        assert_eq!(
            agent_doc_document::queue_projection::strip_priority_markers(marked),
            agent_doc_document::queue_projection::strip_priority_markers(bare),
            "{marked}"
        );
        assert_eq!(queue_head_identity(marked), queue_head_identity(bare));
        assert_eq!(
            queue_prompt_stable_identity(marked),
            queue_prompt_stable_identity(bare)
        );
        assert_eq!(queue_prompt_stable_id(marked), queue_prompt_stable_id(bare));
        assert_eq!(
            crate::queue_claim::claim_identity(marked),
            crate::queue_claim::claim_identity(bare)
        );
        assert_eq!(
            agent_doc_element_queue::render_struck_row_body(marked),
            agent_doc_element_queue::render_struck_row_body(bare)
        );
    }

    // The overlay keys a struck row on its work text, glyph or not, and the
    // queue tree reads the same id and text.
    let marked = sync(&doc(KEPT_TREE));
    let bare = without_glyphs(&marked);
    let keys = |document: &str| {
        agent_doc_markdown_ast::mutations::item_nodes_all(document, "queue")
            .unwrap()
            .into_iter()
            .map(|node| (node.node_key, node.item.struck, node.item.text))
            .collect::<Vec<_>>()
    };
    let (marked_keys, bare_keys) = (keys(&marked), keys(&bare));
    assert_eq!(
        marked_keys
            .iter()
            .map(|(key, struck, _)| (key, struck))
            .collect::<Vec<_>>(),
        bare_keys
            .iter()
            .map(|(key, struck, _)| (key, struck))
            .collect::<Vec<_>>()
    );
    let tree_rows = |document: &str| {
        queue_tree(document, &QueueTreeOptions::default())
            .unwrap()
            .nodes
            .into_iter()
            .map(|node| (node.id, node.full_text, node.closed, node.state))
            .collect::<Vec<_>>()
    };
    assert_eq!(tree_rows(&marked), tree_rows(&bare));
}

/// One row-line rule: only the glyph is rewritten, every other byte kept.
#[test]
fn line_edits_touch_only_the_glyph() {
    let keep = Some(TreeIndicator::Keep);
    let delete = Some(TreeIndicator::Delete);
    assert_eq!(
        line_with_tree_indicator("- ### 🙋🚧 [#subagent:x#2] do [#a]", keep).as_deref(),
        Some("- ### 🙋🚧 [#subagent:x#2] 🔒 do [#a]")
    );
    assert_eq!(
        line_with_tree_indicator("- ### 🙋🚧 [#subagent:x#2] 🔒 do [#a]", keep),
        None
    );
    assert_eq!(
        line_with_tree_indicator("- ### 🔒 🙋 do [#a]", None).as_deref(),
        Some("- ### 🙋 do [#a]")
    );
    // `#looseends`: the keep glyph sits before the open-work count, wherever
    // either pass found it.
    assert_eq!(
        line_with_tree_indicator(
            "- ### 🙋 (1 decision, 1 running) work [#q-aaaaaaaaaaaa]",
            keep
        )
        .as_deref(),
        Some("- ### 🙋 🔒 (1 decision, 1 running) work [#q-aaaaaaaaaaaa]")
    );
    assert_eq!(
        line_with_tree_indicator("- ### 🙋 🔒 (1 decision) work", keep),
        None
    );
    assert_eq!(
        line_with_tree_indicator("- ### 🙋 (1 decision) 🔒 work", keep),
        None,
        "a glyph the count pass moved is already the right one"
    );
    assert_eq!(
        line_with_tree_indicator("    - ~~x ⟦published⟧~~", delete).as_deref(),
        Some("    - ~~🗑\u{fe0f} x ⟦published⟧~~")
    );
    assert_eq!(
        line_with_tree_indicator("    - ~~🗑\u{fe0f} x~~", keep).as_deref(),
        Some("    - ~~🔒 x~~")
    );
    assert_eq!(
        line_with_tree_indicator("    - ~~🗑\u{fe0f} x~~", delete),
        None
    );
    // Never edited: a legacy `~~### …~~` row, an older strike that still
    // holds a lifecycle marker, a single-tilde strike, a non-list line, an
    // empty row. A live row never takes 🗑️.
    assert_eq!(line_with_tree_indicator("- ~~### old~~", delete), None);
    assert_eq!(
        line_with_tree_indicator("- ### ~~🚧 do [#done]~~", delete),
        None
    );
    assert_eq!(line_with_tree_indicator("- ~do [#x]~", delete), None);
    assert_eq!(line_with_tree_indicator("- ~do [#x]~", keep), None);
    assert_eq!(line_with_tree_indicator("- ### 📥 do [#a]", delete), None);
    assert_eq!(
        line_with_tree_indicator("- ### 📥 🗑\u{fe0f} do [#a]", delete).as_deref(),
        Some("- ### 📥 do [#a]")
    );
    assert_eq!(line_with_tree_indicator("plain prose", keep), None);
    assert_eq!(line_with_tree_indicator("- 🚧", keep), None);
}

/// `#looseends`: the open-work count and the keep glyph share the root's
/// prefix in one order (`🙋 🔒 (…)`), and neither pass undoes the other.
#[test]
fn the_keep_glyph_and_the_open_count_settle_together() {
    let synced = sync(&doc(KEPT_TREE));
    let counted = crate::open_count::reconcile_open_counts(&synced)
        .unwrap()
        .expect("the kept root gets its count");
    let root = queue_of(&counted).lines().next().unwrap().to_string();
    assert!(
        root.starts_with("- ### 🙋 🔒 (") && root.contains(") follow up on an action item"),
        "{root}"
    );
    assert!(
        sync_response_trees_striking(&counted, &no_claims).is_none(),
        "the glyph pass leaves the count alone:\n{counted}"
    );
    assert!(
        crate::open_count::reconcile_open_counts(&counted)
            .unwrap()
            .is_none()
    );
}
