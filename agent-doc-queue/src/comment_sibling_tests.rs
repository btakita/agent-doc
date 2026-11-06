//! `#commentsiblingclose`: a list comment at the same depth or shallower ends
//! the preceding row's subtree, as a `re [#id]` reference row does.
//!
//! A comment is a tree node that can head its own children, so rows nested
//! under a top-level `- %% note %%` belong to that comment. Before this,
//! `entry_is_group_parent` skipped comments at any depth, reached those rows,
//! and turned the preceding sibling into a context-only group. That sibling
//! then dropped out of the runnable heads and was never dispatched.

use super::*;

const SIBLING_COMMENT_WITH_CHILD: &str = "- do [#alpha]\n- %% note %%\n  - do [#beta]\n";

fn parsed(body: &str) -> Vec<QueueEntry> {
    parse(body).unwrap()
}

fn prompt_texts(entries: &[QueueEntry]) -> Vec<&str> {
    prompts(entries)
        .iter()
        .map(|prompt| prompt.text.as_str())
        .collect()
}

#[test]
fn sibling_comment_with_children_leaves_the_preceding_row_a_runnable_leaf() {
    let entries = parsed(SIBLING_COMMENT_WITH_CHILD);
    assert!(matches!(entries[1], QueueEntry::Freeform(_)));
    assert!(!entry_is_group_parent(&entries, 0));
    assert_eq!(prompt_texts(&entries), vec!["do [#alpha]", "do [#beta]"]);
    assert_eq!(
        submitted_prompt_at(&entries, 0).as_deref(),
        Some("do [#alpha]")
    );
}

#[test]
fn sibling_comment_is_the_lineage_parent_of_its_children() {
    // An untracked leaf: a tracked `do [#id]` claim stays bare by design.
    let entries = parsed("- do [#alpha]\n- %% note %%\n  - deploy staging\n");
    assert!(entry_is_ancestor_of(&entries, 1, 2));
    assert!(!entry_is_ancestor_of(&entries, 0, 2));
    assert_eq!(
        lineage_claim_text(&entries, 2).as_deref(),
        Some("%% note %%: deploy staging")
    );
}

#[test]
fn multiline_sibling_comment_closes_the_preceding_subtree() {
    let entries = parsed("- do [#alpha]\n- %%\n  note\n  %%\n  - do [#beta]\n");
    assert!(!entry_is_group_parent(&entries, 0));
    assert_eq!(prompt_texts(&entries), vec!["do [#alpha]", "do [#beta]"]);
}

#[test]
fn nested_sibling_comment_closes_only_the_row_at_its_depth() {
    let entries = parsed(concat!(
        "- group\n",
        "  - do [#alpha]\n",
        "  - %% note %%\n",
        "    - deploy staging\n",
    ));
    assert!(entry_is_group_parent(&entries, 0));
    assert!(!entry_is_group_parent(&entries, 1));
    assert_eq!(
        lineage_claim_text(&entries, 3).as_deref(),
        Some("group: %% note %%: deploy staging")
    );
}

#[test]
fn shallower_comment_closes_every_deeper_subtree() {
    let entries = parsed(concat!(
        "- group\n",
        "  - do [#alpha]\n",
        "- %% note %%\n",
        "    - do [#beta]\n",
    ));
    assert!(entry_is_group_parent(&entries, 0));
    assert!(!entry_is_group_parent(&entries, 1));
    assert!(!entry_is_ancestor_of(&entries, 1, 3));
}

#[test]
fn deeper_comment_still_lets_a_real_child_make_a_group() {
    for body in [
        "- epic\n  - %% note %%\n  - do [#alpha]\n",
        "- epic\n  - %% note %%\n    - do [#alpha]\n",
    ] {
        let entries = parsed(body);
        assert!(entry_is_group_parent(&entries, 0), "{body}");
    }
}

#[test]
fn deeper_comment_alone_leaves_its_parent_runnable() {
    let entries = parsed("- do [#alpha]\n  - %% note %%\n- do [#gamma]\n");
    assert!(!entry_is_group_parent(&entries, 0));
    assert_eq!(prompt_texts(&entries), vec!["do [#alpha]", "do [#gamma]"]);
}
