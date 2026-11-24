//! `#orderedsteps`: self-repair for numbered step rows that an older binary
//! wrote wrong.
//!
//! Observed in a session document on 2026-11-23 (anonymized in the tests),
//! under a PR row whose `steps` label held `1. #rebase`, `2. #address` and
//! `3. #adversarial-pr`:
//!
//! - **Half-closed strike.** Step 1's acknowledgement strike landed as
//!   `1. ~~#rebase~` (one closing tilde). The parser read it as a legacy
//!   single-tilde strike of `~#rebase`, so its work text, claim key and
//!   response lookup no longer matched the step. [`repair_numbered_steps`]
//!   closes it: `~~X~` whose `X` holds no other tilde becomes `~~X~~`.
//! - **Response under the wrong step.** The response writers placed a struck
//!   row's `**Response [#r-…]**` tree at the row's indent plus 2, after the
//!   row's descendants, and their descendant scan only knew `- ` siblings.
//!   For a numbered step that is the end of the whole numbered list, so step
//!   1's response sat under step 3, at a column that is not even step 3's
//!   content column. A response row at that bug column (a numbered row's
//!   indent plus 2, short of its content column) under a live step moves to
//!   the one earlier struck step in the same list that has no response of
//!   its own, at that step's content column. With no such step, or more than
//!   one, nothing proves the owner and the tree stays.
//!
//! Only line breaks, leading spaces and the missing tilde change. A second run
//! changes nothing.

use agent_doc_element::element;

use crate::child_row_renest::{parents, rows_of};
use crate::self_repair::{SelfRepair, SelfRepairKind};

fn is_response_row(item: &str) -> bool {
    let item = item.trim();
    item.starts_with("**Response [#r-") && item.ends_with("]**")
}

/// `~~X~~` around the row's work after an optional `### ` heading.
fn is_struck(item: &str) -> bool {
    let item = item.trim();
    let item = item.strip_prefix("### ").unwrap_or(item);
    item.len() > 4 && item.starts_with("~~") && item.ends_with("~~")
}

/// The closed form of a half-closed strike (`~~X~` → `~~X~~`), for the text
/// after a list marker.
fn closed_strike(item: &str) -> Option<String> {
    let trimmed = item.trim_end();
    let (heading, body) = match trimmed.strip_prefix("### ") {
        Some(rest) => ("### ", rest),
        None => ("", trimmed),
    };
    let inner = body.strip_prefix("~~")?.strip_suffix('~')?;
    if inner.trim().is_empty() || inner.contains('~') {
        return None;
    }
    Some(format!("{heading}~~{inner}~~"))
}

fn close_half_strikes(body: &str, repairs: &mut Vec<SelfRepair>) -> String {
    let rows = rows_of(body);
    let mut out = String::with_capacity(body.len() + 8);
    let mut cursor = 0;
    for (row, line) in rows.iter().zip(body.split_inclusive('\n')) {
        let line_end = cursor + line.len();
        cursor = line_end;
        let Some(item) = row.item.filter(|_| !row.fenced) else {
            out.push_str(line);
            continue;
        };
        let Some(closed) = closed_strike(item) else {
            out.push_str(line);
            continue;
        };
        let ending = &line[line.trim_end_matches(['\n', '\r']).len()..];
        let item_start = row.indent + row.marker;
        out.push_str(&line[..item_start]);
        out.push_str(&closed);
        out.push_str(ending);
        repairs.push(SelfRepair {
            kind: SelfRepairKind::HalfClosedStrike,
            detail: item.to_string(),
            rewritten: Some((item.to_string(), closed)),
        });
    }
    out
}

/// One past the last line under the row at `index`.
fn subtree_end(rows: &[crate::child_row_renest::Row<'_>], index: usize) -> usize {
    let indent = rows[index].indent;
    (index + 1..rows.len())
        .find(|at| !rows[*at].blank && rows[*at].indent <= indent)
        .unwrap_or(rows.len())
}

/// The queue item a worker's completion cell names, by owner label:
/// `### Re: Subagent completed: `subagent:x` returned work for queue item
/// “<item>”`. An owner with two different items is left out.
fn completed_items_by_owner(document: &str) -> std::collections::HashMap<String, String> {
    const LEAD: &str = "### Re: Subagent completed: `";
    let mut items: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for line in document.lines() {
        let Some(rest) = line.trim().strip_prefix(LEAD) else {
            continue;
        };
        let Some((owner, rest)) = rest.split_once('`') else {
            continue;
        };
        let Some(rest) = rest.split_once('“').map(|(_, rest)| rest) else {
            continue;
        };
        let Some((item, _)) = rest.rsplit_once('”') else {
            continue;
        };
        let slot = items
            .entry(owner.to_string())
            .or_insert_with(|| Some(item.to_string()));
        if slot.as_deref() != Some(item) {
            *slot = None;
        }
    }
    items
        .into_iter()
        .filter_map(|(owner, item)| item.map(|item| (owner, item)))
        .collect()
}

/// The `subagent:x` owner a response tree's `**Details:**` row names.
fn details_owner(
    rows: &[crate::child_row_renest::Row<'_>],
    index: usize,
    end: usize,
) -> Option<String> {
    rows[index + 1..end].iter().find_map(|row| {
        let item = row.item?.trim();
        let rest = item.strip_prefix("**Details:**")?;
        let (_, after) = rest.split_once("`subagent:")?;
        let (label, _) = after.split_once('`')?;
        Some(format!("subagent:{label}"))
    })
}

fn rehome_step_responses(
    body: &str,
    owners_by_label: &std::collections::HashMap<String, String>,
    repairs: &mut Vec<SelfRepair>,
) -> String {
    let mut current = body.to_string();
    for _ in 0..64 {
        let rows = rows_of(&current);
        let parents = parents(&rows);
        let found = (0..rows.len()).find_map(|index| {
            let row = &rows[index];
            if row.fenced || row.tabbed || !row.item.is_some_and(is_response_row) {
                return None;
            }
            let holder_index = parents[index]?;
            let holder = &rows[holder_index];
            // Two misplacements: under a live numbered step at the bug column
            // (past a `- ` marker's width, short of the step's own content
            // column), or re-nested by an older self-repair to sit among the
            // numbered steps as their sibling.
            let (list_parent, step_indent, before) = if holder.marker > 2 {
                if row.indent != holder.indent + 2 || holder.item.is_some_and(is_struck) {
                    return None;
                }
                (parents[holder_index], holder.indent, holder_index)
            } else {
                // A struck parent's own response may follow its steps.
                if holder.item.is_some_and(is_struck) {
                    return None;
                }
                let among_steps = (holder_index + 1..index).any(|sibling| {
                    parents[sibling] == Some(holder_index)
                        && rows[sibling].indent == row.indent
                        && rows[sibling].marker > 2
                });
                if !among_steps {
                    return None;
                }
                (Some(holder_index), row.indent, index)
            };
            let candidates = (0..before)
                .filter(|&sibling| {
                    parents[sibling] == list_parent
                        && rows[sibling].indent == step_indent
                        && rows[sibling].marker > 2
                        && rows[sibling].item.is_some_and(is_struck)
                        && !rows[sibling + 1..subtree_end(&rows, sibling)]
                            .iter()
                            .any(|child| child.item.is_some_and(is_response_row))
                })
                .collect::<Vec<_>>();
            let block_end = subtree_end(&rows, index);
            // Proof by the hand-back: the response names its worker, and that
            // worker's completion cell names the step's work as its leaf.
            let named = details_owner(&rows, index, block_end)
                .and_then(|owner| owners_by_label.get(&owner))
                .and_then(|item| {
                    let leaf = item.rsplit(": ").next().unwrap_or(item).trim();
                    let matching = candidates
                        .iter()
                        .copied()
                        .filter(|&step| {
                            rows[step].item.is_some_and(|text| {
                                crate::response_tree_rehome::row_work_text(text) == leaf
                            })
                        })
                        .collect::<Vec<_>>();
                    match matching.as_slice() {
                        [step] => Some(*step),
                        _ => None,
                    }
                });
            let owner = match (named, candidates.as_slice()) {
                (Some(step), _) => step,
                (None, [step]) => *step,
                _ => return None,
            };
            if rows[index..block_end]
                .iter()
                .any(|line| line.tabbed || line.fenced)
            {
                return None;
            }
            Some((index, block_end, owner))
        });
        let Some((index, block_end, owner)) = found else {
            break;
        };
        let shift = rows[owner].indent + rows[owner].marker - rows[index].indent;
        let pad = " ".repeat(shift);
        let block_start = rows[index].start;
        let block_stop = rows.get(block_end).map_or(current.len(), |row| row.start);
        let mut moved = String::new();
        for line in current[block_start..block_stop].split_inclusive('\n') {
            if !line.trim().is_empty() {
                moved.push_str(&pad);
            }
            moved.push_str(line);
        }
        if !moved.ends_with('\n') {
            moved.push('\n');
        }
        let insert_at = rows
            .get(subtree_end(&rows, owner))
            .map_or(current.len(), |row| row.start);
        let mut out = String::with_capacity(current.len() + moved.len());
        out.push_str(&current[..insert_at]);
        out.push_str(&moved);
        out.push_str(&current[insert_at..block_start]);
        out.push_str(&current[block_stop..]);
        repairs.push(SelfRepair {
            kind: SelfRepairKind::NumberedStepResponse,
            detail: rows[index].item.unwrap_or_default().to_string(),
            rewritten: None,
        });
        if out == current {
            break;
        }
        current = out;
    }
    current
}

/// Close half-closed strikes and move responses that landed under the wrong
/// numbered step (see the module docs), in every queue body of `document`.
pub fn repair_numbered_steps(document: &str) -> (String, Vec<SelfRepair>) {
    let Ok(components) = element::parse(document) else {
        return (document.to_string(), Vec::new());
    };
    let owners_by_label = completed_items_by_owner(document);
    let mut repairs = Vec::new();
    let mut edits = Vec::new();
    for queue in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = queue.content(document);
        let closed = close_half_strikes(body, &mut repairs);
        let repaired = rehome_step_responses(&closed, &owners_by_label, &mut repairs);
        if repaired != body {
            edits.push((queue.open_end, queue.close_start, repaired));
        }
    }
    let mut out = document.to_string();
    for (start, end, body) in edits.into_iter().rev() {
        out.replace_range(start..end, &body);
    }
    (out, repairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!(
            "---\nagent_doc_session: s\n---\n\n<!-- agent:run id=\"prs\" subagents keep -->\n{queue}<!-- /agent:run -->\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n"
        )
    }

    /// The live 2026-11-23 shape: step 1 half-struck, its response under the
    /// running step 3 at the `- ` column.
    const LIVE: &str = "\
- ### https://github.com/o/r/pull/389
  - %% feat(release): name every download as source or binary %%
  - steps
    1. ~~#rebase~
    2. 📥 #address
    3. 🚧 [#subagent:s389adversarial-pr#1] #adversarial-pr
      - **Response [#r-b3e90758f950]**
        - **Timestamp:** 2026-11-23T23:11:27.000Z
        - **Summary:** rebased onto sdk-dev.
        - **State:** Complete.
        - **Guidance:** ✅ Done.
- ### https://github.com/o/r/pull/390
  - #adversarial-pr
";

    #[test]
    fn the_live_shape_is_closed_and_rehomed_under_step_one() {
        let (repaired, repairs) = repair_numbered_steps(&doc(LIVE));
        let expected = "\
- ### https://github.com/o/r/pull/389
  - %% feat(release): name every download as source or binary %%
  - steps
    1. ~~#rebase~~
       - **Response [#r-b3e90758f950]**
         - **Timestamp:** 2026-11-23T23:11:27.000Z
         - **Summary:** rebased onto sdk-dev.
         - **State:** Complete.
         - **Guidance:** ✅ Done.
    2. 📥 #address
    3. 🚧 [#subagent:s389adversarial-pr#1] #adversarial-pr
- ### https://github.com/o/r/pull/390
  - #adversarial-pr
";
        assert_eq!(repaired, doc(expected), "{repaired}");
        let kinds = repairs.iter().map(|r| r.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                SelfRepairKind::HalfClosedStrike,
                SelfRepairKind::NumberedStepResponse
            ]
        );
        // The parsed step is the clean struck work again.
        let entries = crate::document_queue::parse(expected).unwrap();
        assert!(entries.iter().any(|entry| matches!(
            entry,
            crate::document_queue::QueueEntry::Completed(p) if p.text == "#rebase"
        )));
        let (again, more) = repair_numbered_steps(&repaired);
        assert_eq!(again, repaired);
        assert!(more.is_empty());
    }

    /// The live shape after an older self-repair re-nested step 1's response
    /// to sit among the steps, with step 3 struck too: two struck steps lack
    /// a response, and the hand-back's completion cell names step 1.
    #[test]
    fn a_response_among_the_steps_goes_to_the_step_its_handback_names() {
        let queue = "\
- ### https://github.com/o/r/pull/389
  - %% feat(release): name every download as source or binary %%
  - steps
    1. ~~#rebase~
    2. 📥 #address
    3. ~~#adversarial-pr~~
    - **Response [#r-b3e90758f950]**
      - **Timestamp:** 2026-11-23T23:11:27.000Z
      - **Details:** What changed, Verification, Notes and Next actions are in the `subagent:rebase389` hand-back in agent:exchange.
      - **Remaining:**
        - ⏳ agent: #address <!-- agent-doc:response-detail -->
      - **State:** Follow-up work remains.
- ### https://github.com/o/r/pull/390
  - #adversarial-pr
";
        let cell = "### Re: Subagent completed: `subagent:rebase389` returned work for queue item “https://github.com/o/r/pull/389: steps: #rebase” — agent-doc\n";
        let document = doc(queue).replace(
            "<!-- agent:exchange -->\n",
            &format!("<!-- agent:exchange -->\n{cell}"),
        );
        let (repaired, repairs) = repair_numbered_steps(&document);
        let expected = "\
  - steps
    1. ~~#rebase~~
       - **Response [#r-b3e90758f950]**
         - **Timestamp:** 2026-11-23T23:11:27.000Z
         - **Details:** What changed, Verification, Notes and Next actions are in the `subagent:rebase389` hand-back in agent:exchange.
         - **Remaining:**
           - ⏳ agent: #address <!-- agent-doc:response-detail -->
         - **State:** Follow-up work remains.
    2. 📥 #address
    3. ~~#adversarial-pr~~
- ### https://github.com/o/r/pull/390
";
        assert!(repaired.contains(expected), "{repaired}");
        assert_eq!(repairs.len(), 2, "{repairs:?}");
        assert_eq!(repair_numbered_steps(&repaired).0, repaired);

        // Without the completion cell, two candidates prove nothing.
        let (unproven, _) = repair_numbered_steps(&doc(queue));
        assert!(
            unproven.contains("    3. ~~#adversarial-pr~~\n    - **Response"),
            "{unproven}"
        );
        // A struck parent's own response after its steps stays.
        let own = "- epic\n  - ~~steps~~\n    1. ~~a~~\n    - **Response [#r-aaaaaaaaaaaa]**\n      - **State:** Complete.\n";
        assert_eq!(repair_numbered_steps(&doc(own)).0, doc(own));
    }

    /// No proof, no move: two earlier struck steps without responses, a
    /// response already at the step's own column, or a struck holder.
    #[test]
    fn an_unproven_owner_leaves_the_tree_alone() {
        let ambiguous = LIVE
            .replace("    1. ~~#rebase~\n", "    1. ~~#rebase~~\n")
            .replace("    2. 📥 #address\n", "    2. ~~#address~~\n");
        let (repaired, repairs) = repair_numbered_steps(&doc(&ambiguous));
        assert_eq!(repaired, doc(&ambiguous));
        assert!(repairs.is_empty());

        let own_column = "- epic\n  - steps\n    1. ~~a~~\n    2. 🚧 [#subagent:b#1] b\n       - **Response [#r-aaaaaaaaaaaa]**\n         - **State:** Complete.\n";
        let (repaired, _) = repair_numbered_steps(&doc(own_column));
        assert_eq!(repaired, doc(own_column));

        let bullets = "- epic\n  - ~~a~~\n  - b\n    - **Response [#r-aaaaaaaaaaaa]**\n      - **State:** Complete.\n";
        let (repaired, _) = repair_numbered_steps(&doc(bullets));
        assert_eq!(repaired, doc(bullets));
    }

    #[test]
    fn only_a_half_closed_strike_is_closed() {
        for line in [
            "    1. ~~a~~\n",
            "    1. ~a~\n",
            "    1. ~~a~b~\n",
            "    1. plain~\n",
        ] {
            let body = format!("- epic\n{line}");
            let (repaired, repairs) = repair_numbered_steps(&doc(&body));
            assert_eq!(repaired, doc(&body), "{line}");
            assert!(repairs.is_empty(), "{line}");
        }
        let (repaired, _) = repair_numbered_steps(&doc("- epic\n  - ~~done~\n"));
        assert!(repaired.contains("  - ~~done~~\n"), "{repaired}");
    }
}
