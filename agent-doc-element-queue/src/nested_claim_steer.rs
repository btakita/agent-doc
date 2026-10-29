//! Post-claim split of a folded sub-prompt block (`#nestedclaimsteer`).
//!
//! Parsing still folds every eligible child into the parent. Once a claim
//! exists, the queue compares the claim-time direct children with the current
//! ones: children already present, plus the first direct child added since,
//! stay in the parent and steer that claim. Each later direct addition, with
//! its deeper unclaimed subtree, is its own head. The same comparison applies
//! at every depth when that node has its own claim.

use std::borrow::Cow;

use crate::strip_attempt_projection_prefix;
use crate::subprompts::{list_item_content, split_subprompt_block};

/// One direct child of a folded sub-prompt block.
///
/// Deeper lines belong to this child. They are not siblings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSubprompt {
    /// Space indent of the child's list line, matching the queue parser.
    pub indent: usize,
    /// `Some("1.")` for an ordered marker. `None` is `-`.
    pub ordered_marker: Option<String>,
    /// Text after the list marker, plus deeper lines of this child verbatim.
    pub head_text: String,
    /// The child's source lines, joined by `\n`, including its list marker.
    pub source_block: String,
}

/// Parent text that still steers the claim, and the direct children that route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedClaimSplit<'a> {
    /// Borrowed `current` when nothing routes, so the row stays byte-identical.
    pub steering: Cow<'a, str>,
    pub routable: Vec<DirectSubprompt>,
}

/// Direct children of `text`, grouped at the first indented list line's depth.
///
/// A grandchild stays inside its parent's [`DirectSubprompt::head_text`]. A
/// claim-marker prefix on the parent row is not a child.
pub fn direct_subprompts(text: &str) -> Vec<DirectSubprompt> {
    let Some(block) = split_subprompt_block(text).1 else {
        return Vec::new();
    };
    let lines: Vec<&str> = block.lines().collect();
    let Some((child_indent, _, _)) = lines.first().and_then(|line| split_space_list_item(line))
    else {
        return Vec::new();
    };
    let mut children = Vec::new();
    let mut current: Option<DirectSubprompt> = None;
    let mut current_lines: Vec<&str> = Vec::new();
    for line in lines {
        if let Some((indent, marker, rest)) = split_space_list_item(line)
            && indent == child_indent
            && list_item_content(line).is_some()
        {
            finish_child(&mut current, &current_lines, &mut children);
            current_lines.clear();
            current = Some(DirectSubprompt {
                indent,
                ordered_marker: marker,
                head_text: rest.to_string(),
                source_block: String::new(),
            });
            current_lines.push(line);
        } else if current.is_some() {
            current_lines.push(line);
        }
    }
    finish_child(&mut current, &current_lines, &mut children);
    children
}

fn finish_child(
    current: &mut Option<DirectSubprompt>,
    lines: &[&str],
    out: &mut Vec<DirectSubprompt>,
) {
    let Some(mut child) = current.take() else {
        return;
    };
    child.source_block = lines.join("\n");
    if lines.len() > 1 {
        child.head_text.push('\n');
        child.head_text.push_str(&lines[1..].join("\n"));
    }
    out.push(child);
}

/// `current` against the direct children of `claim_time`.
///
/// Unmatched claim-time children pair positionally with unmatched current
/// children as edits, so an edited claim-time child does not take the first
/// new slot. The first unmatched addition stays in the parent. Later additions
/// are [`NestedClaimSplit::routable`]. No routable child returns `current`
/// unchanged.
pub fn split_nested_claim_steer<'a>(current: &'a str, claim_time: &str) -> NestedClaimSplit<'a> {
    let unchanged = NestedClaimSplit {
        steering: Cow::Borrowed(current),
        routable: Vec::new(),
    };
    let current_children = direct_subprompts(current);
    if current_children.is_empty() {
        return unchanged;
    }
    let claim_keys: Vec<String> = direct_subprompts(claim_time)
        .iter()
        .map(|child| child_key(&child.head_text))
        .collect();
    let current_keys: Vec<String> = current_children
        .iter()
        .map(|child| child_key(&child.head_text))
        .collect();
    let matched = lcs_current_members(&claim_keys, &current_keys);
    let matched_count = matched.iter().filter(|keep| **keep).count();
    let unmatched_claim = claim_keys.len().saturating_sub(matched_count);
    let unmatched_current: Vec<usize> = matched
        .iter()
        .enumerate()
        .filter(|(_, keep)| !**keep)
        .map(|(index, _)| index)
        .collect();
    let edits = unmatched_claim.min(unmatched_current.len());
    let additions = &unmatched_current[edits..];
    if additions.len() <= 1 {
        return unchanged;
    }
    let routable_at: Vec<usize> = additions[1..].to_vec();
    let (parent, _) = split_subprompt_block(current);
    let mut steering = String::with_capacity(current.len());
    steering.push_str(parent);
    for (index, child) in current_children.iter().enumerate() {
        if routable_at.contains(&index) {
            continue;
        }
        steering.push('\n');
        steering.push_str(&child.source_block);
    }
    let routable = current_children
        .into_iter()
        .enumerate()
        .filter(|(index, _)| routable_at.contains(index))
        .map(|(_, child)| child)
        .collect();
    NestedClaimSplit {
        steering: Cow::Owned(steering),
        routable,
    }
}

fn child_key(head_text: &str) -> String {
    let first = head_text.lines().next().unwrap_or("");
    strip_attempt_projection_prefix(first)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Which `current` indexes sit on a longest common subsequence with `claim`.
fn lcs_current_members(claim: &[String], current: &[String]) -> Vec<bool> {
    let n = claim.len();
    let m = current.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if claim[i - 1] == current[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    let mut keep = vec![false; m];
    let mut i = n;
    let mut j = m;
    while i > 0 && j > 0 {
        if claim[i - 1] == current[j - 1] {
            keep[j - 1] = true;
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    keep
}

/// Space-indent list split, the same shape `document_queue::split_list_item` uses.
fn split_space_list_item(line: &str) -> Option<(usize, Option<String>, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let dedented = &line[indent..];
    if let Some(rest) = dedented.strip_prefix("- ") {
        return Some((indent, None, rest));
    }
    let marker_end = dedented.find(". ")?;
    let digits = &dedented[..marker_end];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let rest = &dedented[marker_end + 2..];
    Some((indent, Some(dedented[..=marker_end].to_string()), rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_marker_on_the_parent_is_not_a_direct_child() {
        let text = "🟡 [#subagent:label#1] Parent\n  - only child";
        let children = direct_subprompts(text);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].head_text, "only child");
        let split = split_nested_claim_steer(text, "🟡 [#subagent:label#1] Parent");
        assert!(split.routable.is_empty());
        assert_eq!(split.steering, text);
    }

    #[test]
    fn an_edit_plus_one_addition_stays_on_the_parent() {
        let claim = "Parent\n  - alpha\n  - beta\n  - gamma\n  - delta";
        let current = "Parent\n  - alpha\n  - beta edited\n  - gamma\n  - delta\n  - epsilon";
        let split = split_nested_claim_steer(current, claim);
        assert!(split.routable.is_empty(), "{:?}", split.routable);
        assert_eq!(split.steering, current);
    }

    #[test]
    fn the_second_added_child_routes_and_the_first_steers() {
        let current = "Parent\n  - alpha\n  - beta";
        let split = split_nested_claim_steer(current, "Parent");
        assert_eq!(split.steering, "Parent\n  - alpha");
        assert_eq!(split.routable.len(), 1);
        assert_eq!(split.routable[0].head_text, "beta");
        assert_eq!(split.routable[0].indent, 2);
    }

    #[test]
    fn claim_time_children_stay_when_later_ones_are_appended() {
        let claim = "Parent\n  - alpha\n  - beta";
        let current = "Parent\n  - alpha\n  - beta\n  - gamma\n  - delta";
        let split = split_nested_claim_steer(current, claim);
        assert_eq!(split.steering, "Parent\n  - alpha\n  - beta\n  - gamma");
        assert_eq!(split.routable.len(), 1);
        assert_eq!(split.routable[0].head_text, "delta");
    }

    #[test]
    fn an_insertion_between_claim_time_children_routes_the_later_new_row() {
        let claim = "Parent\n  - alpha\n  - beta";
        let current = "Parent\n  - alpha\n  - new one\n  - new two\n  - beta";
        let split = split_nested_claim_steer(current, claim);
        assert_eq!(split.steering, "Parent\n  - alpha\n  - new one\n  - beta");
        assert_eq!(
            split
                .routable
                .iter()
                .map(|child| child.head_text.as_str())
                .collect::<Vec<_>>(),
            vec!["new two"]
        );
    }

    #[test]
    fn an_edited_claim_child_does_not_consume_the_first_new_slot() {
        let claim = "Parent\n  - alpha\n  - beta";
        let current = "Parent\n  - alpha\n  - beta edited\n  - gamma\n  - delta";
        let split = split_nested_claim_steer(current, claim);
        assert_eq!(
            split.steering,
            "Parent\n  - alpha\n  - beta edited\n  - gamma"
        );
        assert_eq!(split.routable.len(), 1);
        assert_eq!(split.routable[0].head_text, "delta");
    }

    #[test]
    fn grandchildren_stay_inside_their_direct_parent() {
        let text = "Parent\n  - alpha\n    - grandchild\n  - beta";
        let children = direct_subprompts(text);
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].head_text, "alpha\n    - grandchild");
        assert_eq!(children[1].head_text, "beta");
        let nested = split_nested_claim_steer(&children[0].head_text, "alpha");
        assert_eq!(nested.steering, "alpha\n    - grandchild");
        assert!(nested.routable.is_empty());
        let deeper = "alpha\n    - first gc\n    - second gc";
        let split = split_nested_claim_steer(deeper, "alpha");
        assert_eq!(split.steering, "alpha\n    - first gc");
        assert_eq!(split.routable.len(), 1);
        assert_eq!(split.routable[0].head_text, "second gc");
        assert_eq!(split.routable[0].indent, 4);
    }
}
