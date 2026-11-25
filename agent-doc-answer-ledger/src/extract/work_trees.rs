//! Queue heads and done trees, the work the upstream watch covers beyond the
//! tracked lists (`#operatorshouldnneed`).
//!
//! Tracked items ([`super::TrackedItemObs`]) are the todo, backlog, review and
//! icebox rows. Two more places name GitHub artifacts the document can drift
//! against:
//!
//! - a live head of an `agent:run` / `agent:queue` component, such as a bare
//!   pull-request heading (`- ### https://github.com/o/r/pull/403` with
//!   `%% title %%` notes) or a follow-up row that asks a reviewer to approve
//!   one. The head is still queued after the pull request merged.
//! - a dated `agent:done` entry, inline or in its archive file. The entry says
//!   the work is done while the pull request it cites was closed unmerged.
//!
//! The scan is structural, like the rest of [`super::observe`]: rows and their
//! indentation, never prose.

use super::{indent_of, list_body, work_text};

/// One live head of an `agent:run` / `agent:queue` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueHeadObs {
    /// The component's `id` attribute, else its name.
    pub queue: String,
    /// 1-based document line of the head row.
    pub line: usize,
    /// The head row's work text without lifecycle glyphs, heading hashes,
    /// attempt handles or trailing generated ids: the `queue consume
    /// --ack-text` selector of a note-only heading.
    pub text: String,
    /// Every `[#id]` the head row carries, generated handles included.
    pub ids: Vec<String>,
    /// A worker attempt handle (`[#subagent:leaf#1]`) is on the row: the head
    /// is being worked.
    pub claimed: bool,
    /// The head row plus every descendant line that is not under a struck
    /// row: the live tree whose artifacts decide whether the head is settled.
    pub tree_text: String,
}

/// One dated entry of `agent:done` (inline, or in its archive file), with its
/// continuation lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneTreeObs {
    /// Own id, lowercase. Entries without one are not watched: nothing can
    /// reopen them.
    pub id: String,
    /// Archive date, `YYYY-MM-DD`.
    pub date: String,
    /// Entry text after the date.
    pub text: String,
    /// The entry row plus its continuation lines.
    pub tree_text: String,
}

fn is_struck(body: &str) -> bool {
    let (_, struck) = work_text(body);
    struck || body.trim_start().starts_with("~~")
}

/// Live heads of one queue body. `line_offset` is the document line count
/// before the body.
pub(super) fn queue_heads(body: &str, queue: &str, line_offset: usize) -> Vec<QueueHeadObs> {
    let lines: Vec<&str> = body.lines().collect();
    let Some(top) = lines
        .iter()
        .filter(|line| list_body(line).is_some())
        .map(|line| indent_of(line))
        .min()
    else {
        return Vec::new();
    };
    let mut heads = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let head = match list_body(line) {
            Some(body) if indent_of(line) == top => body,
            _ => {
                index += 1;
                continue;
            }
        };
        let mut end = index + 1;
        while end < lines.len() && (lines[end].trim().is_empty() || indent_of(lines[end]) > top) {
            end += 1;
        }
        if !is_struck(head) {
            let (work, _) = work_text(head);
            heads.push(QueueHeadObs {
                queue: queue.to_string(),
                line: line_offset + index + 1,
                text: trim_trailing_handles(
                    agent_doc_element_queue::strip_attempt_projection_prefix(&work),
                ),
                ids: super::bracket_ids(head),
                claimed: has_attempt_handle(head),
                tree_text: live_tree_text(&lines[index..end]),
            });
        }
        index = end;
    }
    heads
}

/// `work` without trailing generated `[#q-…]` / `[#r-…]` ids.
fn trim_trailing_handles(work: &str) -> String {
    let mut out = work.trim_end();
    while let Some(head) = out.strip_suffix(']')
        && let Some(start) = head.rfind("[#")
        && super::is_handle(&head[start + 2..])
        && !head[start + 2..].contains(['[', ']'])
    {
        out = head[..start].trim_end();
    }
    out.to_string()
}

/// A `[#owner:leaf#N]`-shaped token: an id with `:` is an attempt handle.
fn has_attempt_handle(row: &str) -> bool {
    let mut rest = row;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else { break };
        if after[..end].contains(':') {
            return true;
        }
        rest = &after[end..];
    }
    false
}

/// The head line plus descendants that are not under a struck row.
fn live_tree_text(tree: &[&str]) -> String {
    let mut out = String::new();
    let mut struck_at: Option<usize> = None;
    for line in tree {
        if line.trim().is_empty() {
            continue;
        }
        let indent = indent_of(line);
        if struck_at.is_some_and(|at| indent > at) {
            continue;
        }
        struck_at = None;
        if let Some(body) = list_body(line)
            && is_struck(body)
        {
            struck_at = Some(indent);
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// `YYYY-MM-DD` at the start of `text`.
fn leading_date(text: &str) -> Option<(&str, &str)> {
    let date = text.get(..10)?;
    let bytes = date.as_bytes();
    let shaped = bytes.iter().enumerate().all(|(at, byte)| match at {
        4 | 7 => *byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    shaped.then(|| (date, text[10..].trim_start()))
}

/// Dated entries of one `agent:done` body or archive file.
pub(super) fn done_trees(text: &str) -> Vec<DoneTreeObs> {
    let lines: Vec<&str> = text.lines().collect();
    let mut trees = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let entry = list_body(line).and_then(leading_date);
        let Some((date, rest)) = entry else {
            index += 1;
            continue;
        };
        let indent = indent_of(line);
        let mut end = index + 1;
        while end < lines.len() && (lines[end].trim().is_empty() || indent_of(lines[end]) > indent)
        {
            end += 1;
        }
        let own = rest
            .strip_prefix("[#")
            .and_then(|after| after.find(']').map(|close| &after[..close]))
            .filter(|id| !id.contains(':'))
            .map(str::to_ascii_lowercase);
        if let Some(id) = own {
            trees.push(DoneTreeObs {
                id,
                date: date.to_string(),
                text: rest.to_string(),
                tree_text: lines[index..end].join("\n") + "\n",
            });
        }
        index = end;
    }
    trees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_headings_and_follow_up_rows_are_heads_and_struck_ones_are_not() {
        let body = concat!(
            "- ### https://github.com/o/r/pull/98\n",
            "  - %% docs(standards): permit promotion %%\n",
            "\t- #adversarial-pr [#q-4ae8c3d35ca1]\n",
            "- ### ~~https://github.com/o/r/pull/340~~\n",
            "  - %% merged %%\n",
            "- 🙋 [#q-8ab86db0ef93] follow-up: human (reviewer): approve https://github.com/o/r/pull/930. [operator-verify] [#q-2cf094356f78]\n",
            "- ### (2 decisions, 11 actions) https://github.com/o/r/pull/368\n",
            "  - 🙋 follow-up: operator: decide\n",
        );
        let heads = queue_heads(body, "prs", 10);
        let found: Vec<(usize, &str)> = heads
            .iter()
            .map(|head| (head.line, head.text.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![
                (11, "https://github.com/o/r/pull/98"),
                (
                    16,
                    "[#q-8ab86db0ef93] follow-up: human (reviewer): approve https://github.com/o/r/pull/930. [operator-verify]"
                ),
                (17, "https://github.com/o/r/pull/368"),
            ]
        );
        assert!(heads[0].tree_text.contains("adversarial-pr"));
        assert!(!heads[0].claimed);
        assert!(has_attempt_handle("🚧 [#subagent:upwatch#1] do [#x]"));
        assert_eq!(heads[1].ids, vec!["q-8ab86db0ef93", "q-2cf094356f78"]);
    }

    #[test]
    fn a_struck_child_and_its_subtree_leave_the_live_tree() {
        let body = concat!(
            "- do [#ship] https://github.com/o/r/pull/1\n",
            "  - ~~review https://github.com/o/r/pull/2~~\n",
            "    - note https://github.com/o/r/pull/3\n",
            "  - still open https://github.com/o/r/pull/4\n",
        );
        let heads = queue_heads(body, "run", 0);
        assert_eq!(heads.len(), 1);
        let tree = &heads[0].tree_text;
        assert!(tree.contains("pull/1") && tree.contains("pull/4"), "{tree}");
        assert!(
            !tree.contains("pull/2") && !tree.contains("pull/3"),
            "{tree}"
        );
    }

    #[test]
    fn dated_done_entries_keep_their_continuation_and_need_an_own_id() {
        let text = concat!(
            "# Agent Doc Completed Work\n\n",
            "- 2026-11-20 [#shipit] Ship it https://github.com/o/r/pull/7\n",
            "  - follow-up https://github.com/o/r/pull/8\n",
            "- 2026-11-21 archived waiting item `#q-5ee01d33f4a3`: https://github.com/o/r/pull/9\n",
            "  > quoted\n",
            "- not dated [#nodate] https://github.com/o/r/pull/10\n",
        );
        let trees = done_trees(text);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].id, "shipit");
        assert_eq!(trees[0].date, "2026-11-20");
        assert!(trees[0].tree_text.contains("pull/8"));
        assert!(!trees[0].tree_text.contains("pull/9"));
    }
}
