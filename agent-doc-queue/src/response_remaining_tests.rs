//! Tests for [`super`] (`#guidancescope`).

use super::*;

/// The live shape, anonymized: a root row whose response (`[#r-2da6da4a7fd8]`)
/// says `Follow-up work remains.` although its one agent action item (a CI
/// check) is covered by the struck `~~Do the next actions.~~` beneath it; a
/// nested response (`[#r-8eb6516470b1]`) under its Guidance says the same
/// although both of its operator action items carry struck answers; the
/// operator's live `Complete the rest of the work…` row; and a live
/// `[operator-verify]` follow-up row at the end of the tree.
const SESSION: &str = include_str!("../tests/fixtures/response_remaining_session.md");

const LIVE_LEAF: &str = "        - Complete the rest of the work.";
const LIVE_OPERATOR_ROW: &str = "    - 🙋 follow up on an action item from queue item";
const KEEP_ANSWER: &str = "            - ~~keep after unlocking.~~\n";

/// Strike the live row starting with `prefix` (`- x` → `- ~~x~~`).
fn strike(document: &str, prefix: &str) -> String {
    let start = document.find(prefix).expect("row");
    let end = start + document[start..].find('\n').expect("line end");
    let line = &document[start..end];
    let indent = line.len() - line.trim_start().len();
    let body = line.trim_start().strip_prefix("- ").expect("list row");
    let body = body.strip_prefix("🙋 ").unwrap_or(body);
    format!(
        "{}{}- ~~{body}~~{}",
        &document[..start],
        " ".repeat(indent),
        &document[end..]
    )
}

/// The coordinator closed this item and the operator finished the last
/// `[operator-verify]` row: every row in the tree is struck.
fn all_resolved() -> String {
    strike(&strike(SESSION, LIVE_LEAF), LIVE_OPERATOR_ROW)
}

/// The `- <label> value` values of the response whose row is `marker`, in
/// order, and its Remaining entries.
fn response(document: &str, marker: &str) -> (String, String, Vec<String>) {
    let start = document.find(marker).expect("response");
    let indent = document[..start].len() - document[..start].trim_end_matches(' ').len();
    let detail = " ".repeat(indent + 2);
    let mut state = None;
    let mut guidance = None;
    let mut remaining = Vec::new();
    let mut in_remaining = false;
    for line in document[start..].lines().skip(1) {
        let line_indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && line_indent <= indent {
            break;
        }
        if let Some(rest) = line.strip_prefix(&format!("{detail}- ")) {
            in_remaining = rest == REMAINING_LABEL;
            if let Some(value) = rest.strip_prefix("**State:** ") {
                state.get_or_insert(value.to_string());
            }
            if let Some(value) = rest.strip_prefix("**Guidance:** ") {
                guidance.get_or_insert(value.to_string());
            }
        } else if in_remaining && let Some(rest) = line.strip_prefix(&format!("{detail}  - ")) {
            let rest = [AGENT_OPEN_GLYPH, OPERATOR_OPEN_GLYPH]
                .iter()
                .find_map(|glyph| rest.strip_prefix(&format!("{glyph} ")))
                .unwrap_or(rest);
            remaining.push(
                rest.strip_suffix(RESPONSE_DETAIL_MARKER)
                    .unwrap_or(rest)
                    .trim_end()
                    .to_string(),
            );
        } else if line_indent <= indent + 2 {
            in_remaining = false;
        }
    }
    (
        state.expect("state"),
        guidance.expect("guidance"),
        remaining,
    )
}

/// The guidance after its `#donetrack` tally.
fn tail(guidance: &str) -> &str {
    split_tally(guidance).unwrap_or_else(|| panic!("no tally: {guidance}"))
}

const ROOT: &str = "- **Response [#r-2da6da4a7fd8]**";
const NESTED: &str = "- **Response [#r-8eb6516470b1]**";

/// Every Guidance value in `document` names rows by readable text: no
/// `[#r-…]` response id appears in one.
fn assert_guidance_readable(document: &str) {
    for line in document.lines() {
        if let Some(value) = line.trim_start().strip_prefix("- **Guidance:** ") {
            assert!(
                !value.contains("[#r-"),
                "guidance names a bare response id: {value}"
            );
        }
    }
}

/// With every action item answered or covered and every row struck, the
/// root tree and the nested response say `Complete.` with no Remaining list,
/// and the Guidance says the tree can go.
#[test]
fn a_fully_resolved_tree_says_complete_with_nothing_remaining() {
    let document = all_resolved();
    assert_eq!(response(&document, ROOT).0, RESPONSE_STATE_FOLLOW_UP);

    let (synced, changed) = reconcile_response_remaining(&document).expect("trees synced");
    assert_eq!(changed, 2, "the root response and the nested one");
    let (state, guidance, remaining) = response(&synced, ROOT);
    assert_eq!(state, RESPONSE_STATE_COMPLETE);
    assert_eq!(tail(&guidance), NOTHING_REMAINS_ROW_GUIDANCE);
    assert!(guidance.starts_with(DONE_GLYPH), "{guidance}");
    assert!(remaining.is_empty());
    let (state, guidance, remaining) = response(&synced, NESTED);
    assert_eq!(state, RESPONSE_STATE_COMPLETE);
    assert_eq!(tail(&guidance), NOTHING_REMAINS_RESPONSE_GUIDANCE);
    assert!(remaining.is_empty());
    assert!(!synced.contains(REMAINING_LABEL));
    // Only the two State and two Guidance values changed.
    assert_eq!(synced.lines().count(), document.lines().count());
    let differing = synced
        .lines()
        .zip(document.lines())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 4);

    assert!(
        reconcile_response_remaining(&synced).is_none(),
        "a second pass changes nothing"
    );
}

/// The live document: the operator's leaf is being worked and one
/// `[operator-verify]` row is still open. The root lists both, one line each,
/// with the owner and a short imperative; the nested response lists the leaf
/// that sits in it.
#[test]
fn the_live_tree_lists_what_is_left() {
    let (synced, _) = reconcile_response_remaining(SESSION).expect("trees synced");
    let (state, guidance, remaining) = response(&synced, ROOT);
    assert_eq!(state, RESPONSE_STATE_FOLLOW_UP);
    assert_eq!(tail(&guidance), remaining_guidance(2));
    assert!(guidance.starts_with(AGENT_OPEN_GLYPH), "{guidance}");
    assert!(guidance.contains(", 1 yours."), "{guidance}");
    assert_eq!(
        remaining,
        vec![
            "agent: Complete the rest of the work".to_string(),
            "operator: in the editor, delete the misplaced copy under ` - ~~Complete the work. ⟦published⟧~~`"
                .to_string(),
        ]
    );
    let (state, guidance, remaining) = response(&synced, NESTED);
    assert_eq!(state, RESPONSE_STATE_FOLLOW_UP);
    assert_eq!(tail(&guidance), remaining_guidance(1));
    assert_eq!(
        remaining,
        vec!["agent: Complete the rest of the work".to_string()]
    );
    assert!(reconcile_response_remaining(&synced).is_none());

    // The coordinator strikes the leaf: the nested response is complete, the
    // root keeps only the operator row.
    let struck = strike(&synced, LIVE_LEAF);
    let (resynced, changed) = reconcile_response_remaining(&struck).expect("resynced");
    assert_eq!(changed, 2);
    assert_eq!(response(&resynced, NESTED).0, RESPONSE_STATE_COMPLETE);
    // Only the operator's row is left: the State says so and the Guidance
    // says exactly what to do.
    let (state, guidance, remaining) = response(&resynced, ROOT);
    assert_eq!(state, RESPONSE_STATE_WAITING_ON_OPERATOR);
    assert!(guidance.starts_with(OPERATOR_OPEN_GLYPH), "{guidance}");
    assert!(
        tail(&guidance).starts_with(&format!(
            "{ONLY_YOU_GUIDANCE_LEAD}in the editor, delete the misplaced copy"
        )),
        "{guidance}"
    );
    assert_eq!(remaining.len(), 1);
    assert!(remaining[0].starts_with("operator: in the editor, delete the misplaced copy"));
    assert!(reconcile_response_remaining(&resynced).is_none());
}

/// One operator action item without an answer: the root and the nested
/// response list exactly that item.
#[test]
fn one_unresolved_operator_item_is_a_one_item_list() {
    let document = all_resolved().replace(KEEP_ANSWER, "");
    let (synced, _) = reconcile_response_remaining(&document).expect("trees synced");
    let expected = vec![
        "operator: decide whether to keep `/home/sample/.local/bin/gh` after unlocking".to_string(),
    ];
    for marker in [ROOT, NESTED] {
        let (state, guidance, remaining) = response(&synced, marker);
        assert_eq!(state, RESPONSE_STATE_WAITING_ON_OPERATOR, "{marker}");
        assert!(
            tail(&guidance).starts_with(&format!("{ONLY_YOU_GUIDANCE_LEAD}decide whether to keep")),
            "{marker}: {guidance}"
        );
        assert_eq!(remaining, expected, "{marker}");
    }
    // The list is reserved response structure, never queue rows: the queue
    // tree is unchanged and a second pass is a no-op.
    let tree = |doc: &str| {
        crate::queue_tree::queue_tree(doc, &crate::queue_tree::QueueTreeOptions::default())
            .unwrap()
            .nodes
            .into_iter()
            .map(|node| (node.state, node.text))
            .collect::<Vec<_>>()
    };
    assert_eq!(tree(&synced), tree(&document));
    assert!(reconcile_response_remaining(&synced).is_none());

    // The operator answers it: the list goes and the trees are complete.
    let answered = {
        // Put the answer back under its item, where it was.
        let item = "remove with `rm ~/.local/bin/gh`). [operator-verify] <!-- agent-doc:response-detail -->\n";
        let at = synced.find(item).expect("item") + item.len();
        format!("{}{KEEP_ANSWER}{}", &synced[..at], &synced[at..])
    };
    let (resynced, _) = reconcile_response_remaining(&answered).expect("resynced");
    assert_eq!(response(&resynced, ROOT).0, RESPONSE_STATE_COMPLETE);
    assert!(!resynced.contains(REMAINING_LABEL));
}

/// An agent item with no later completion and no struck follow-up row stays
/// on the list; a struck follow-up row anywhere resolves it, a live one keeps
/// it listed once.
#[test]
fn an_agent_item_needs_a_completion_or_a_struck_followup_row() {
    // No `~~Do the next actions.~~`, and the operator's leaf still live: no
    // struck row follows the CI item in its response.
    let document = SESSION.replace("        - ~~Do the next actions.~~\n", "");
    let (synced, _) = reconcile_response_remaining(&document).expect("trees synced");
    let (_, guidance, remaining) = response(&synced, ROOT);
    assert_eq!(tail(&guidance), remaining_guidance(3));
    assert_eq!(
        remaining[0],
        "agent: check the agent-doc CI run for `d9190e1d6a` on `main`"
    );

    let item = "check the agent-doc CI run for `d9190e1d6a` on `main` (public API `https://api.github.com/repos/sample-org/sample-tool/actions/runs?head_sha=d9190e1d6a375c1b576a87489cd97102cb71b1a2` if `gh` still hangs).";
    let struck_row =
        format!("- ~~follow up on an action item from queue item “stop the copy”: agent: {item}~~");
    let with_row = synced.replace(
        "<!-- /agent:run -->",
        &format!("{struck_row}\n<!-- /agent:run -->"),
    );
    let (resynced, _) = reconcile_response_remaining(&with_row).expect("resynced");
    let (_, _, remaining) = response(&resynced, ROOT);
    assert_eq!(remaining.len(), 2, "{remaining:?}");
    assert!(!remaining.iter().any(|entry| entry.contains("CI run")));

    // `#treefinish`: a live follow-up row in another top-level tree carries
    // the item (delegated), so this tree no longer lists it.
    let delegated = resynced.replace(
        &struck_row,
        &format!("- follow up on an action item from queue item “stop the copy”: agent: {item}"),
    );
    assert!(reconcile_response_remaining(&delegated).is_none());
    let (_, _, remaining) = response(&delegated, ROOT);
    assert_eq!(remaining.len(), 2, "{remaining:?}");

    // A live follow-up row inside this tree keeps it open, listed once.
    let nested_row = format!(
        "    - follow up on an action item from queue item “stop the copy”: agent: {item}\n"
    );
    let live = synced.replacen(
        "    - 🙋 follow up on an action item",
        &format!("{nested_row}    - 🙋 follow up on an action item"),
        1,
    );
    assert_ne!(live, synced);
    let relive = reconcile_response_remaining(&live).map_or(live, |(document, _)| document);
    let (_, _, remaining) = response(&relive, ROOT);
    assert_eq!(remaining.len(), 3, "{remaining:?}");
    assert_eq!(
        remaining
            .iter()
            .filter(|entry| entry.contains("CI run"))
            .count(),
        1
    );
    assert!(reconcile_response_remaining(&relive).is_none());
}

/// The guidance names rows by short readable text, never by a `[#r-…]` id the
/// row only quotes; the old struck-child guidance did.
#[test]
fn guidance_names_rows_by_readable_text() {
    assert!(SESSION.contains("its root item [#r-92b7ccc53170] is still open"));
    let (synced, _, _) =
        crate::response_tree_sync::sync_response_trees(SESSION).expect("trees synced");
    assert_guidance_readable(&synced);
    // `#treefinish`: the finished child says only that it is done; the
    // root's response carries the tree's keep/delete indicator.
    assert!(
        synced.contains("                - **Guidance:** ✅ Done.\n"),
        "{synced}"
    );
    assert!(!synced.contains("This row is done"), "{synced}");
    assert!(crate::response_tree_sync::sync_response_trees(&synced).is_none());

    let (resolved, _, _) =
        crate::response_tree_sync::sync_response_trees(&all_resolved()).expect("trees synced");
    assert_guidance_readable(&resolved);
    assert!(crate::response_tree_sync::sync_response_trees(&resolved).is_none());
}

/// An operator-edited Remaining list (an unmarked line in it) is left alone,
/// with its State and Guidance.
#[test]
fn an_edited_remaining_list_stays() {
    let (synced, _) = reconcile_response_remaining(SESSION).expect("trees synced");
    let edited = synced.replace(
        "      - ⏳ agent: Complete the rest of the work <!-- agent-doc:response-detail -->\n",
        "      - ⏳ agent: Complete the rest of the work <!-- agent-doc:response-detail -->\n      - and also tidy the titles\n",
    );
    assert_ne!(edited, synced);
    let after = reconcile_response_remaining(&strike(&edited, LIVE_LEAF));
    let after = after.map_or_else(|| strike(&edited, LIVE_LEAF), |(doc, _)| doc);
    let (_, guidance, remaining) = response(&after, ROOT);
    assert_eq!(tail(&guidance), remaining_guidance(2));
    assert!(remaining.contains(&"and also tidy the titles".to_string()));
}

#[test]
fn short_text_is_the_owner_and_first_clause() {
    assert_eq!(
        short_row_text(
            "🙋 follow up on an action item from queue item “follow up on an action item from queue item 'x…”: operator: decide whether to keep `~/.local/bin/gh` after unlocking (harmless to keep). [operator-verify] [#q-cb3ac1276fd3]",
            80
        ),
        "operator: decide whether to keep `~/.local/bin/gh` after unlocking"
    );
    assert_eq!(
        short_imperative(
            "Complete the rest of the work. This response tree is large.",
            80
        ),
        "Complete the rest of the work"
    );
    let long = short_imperative(
        "check the `agent-doc` CI run for every commit that landed on main today and record each status in the table",
        40,
    );
    assert!(long.ends_with('…') && long.chars().count() <= 41, "{long}");
    assert_eq!(long.matches('`').count() % 2, 0);
}

/// `#answeredentry`: the live shape, anonymized. The operator answered both
/// 🙋 Remaining entries with `#justdoit` rows written under the entries; the
/// agent struck both (the first with a `Complete.` response, the second bare
/// through `--ack-text`), while the two `[operator-verify]` follow-up rows the
/// entries stand for are still live.
const ANSWERED: &str = include_str!("../tests/fixtures/response_remaining_answered_entries.md");
const ANSWERED_ROOT: &str = "- **Response [#r-ed4566860530]**";
const VSCODE_ENTRY: &str = "operator: decide whether to refresh the VS Code package";
const TITLES_ENTRY: &str =
    "operator: apply edits 1–3 above in the editor if you want the old titles cleaned";

/// The Remaining entry lines (glyph included) of the response `marker`.
fn entry_lines(document: &str, marker: &str) -> Vec<String> {
    let start = document.find(marker).expect("response");
    let indent = document[..start].len() - document[..start].trim_end_matches(' ').len();
    let entry_prefix = format!("{}- ", " ".repeat(indent + 4));
    let mut in_remaining = false;
    let mut out = Vec::new();
    for line in document[start..].lines().skip(1) {
        let line_indent = line.len() - line.trim_start().len();
        if !line.trim().is_empty() && line_indent <= indent {
            break;
        }
        if line_indent == indent + 2 {
            in_remaining = line.trim_start() == format!("- {REMAINING_LABEL}");
        } else if in_remaining && let Some(rest) = line.strip_prefix(&entry_prefix) {
            out.push(
                rest.strip_suffix(RESPONSE_DETAIL_MARKER)
                    .unwrap_or(rest)
                    .trim_end()
                    .to_string(),
            );
        }
    }
    out
}

/// Two struck answers under the two 🙋 entries finish the tree: the tally,
/// State and Guidance say so, both answers (and the first one's response)
/// stay under their entries marked ✅, and a second sync changes nothing.
#[test]
fn struck_answers_under_remaining_entries_finish_the_tally() {
    let (state, guidance, _) = response(ANSWERED, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_WAITING_ON_OPERATOR);
    assert!(guidance.starts_with("🙋 3/5 done, 2 yours."), "{guidance}");

    let (synced, changed) = reconcile_response_remaining(ANSWERED).expect("the tree re-renders");
    assert_eq!(changed, 1);
    let (state, guidance, _) = response(&synced, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    assert_eq!(guidance, "✅ 5/5 done.", "{synced}");
    assert_eq!(
        entry_lines(&synced, ANSWERED_ROOT),
        vec![
            format!("{DONE_GLYPH} {VSCODE_ENTRY}"),
            format!("{DONE_GLYPH} {TITLES_ENTRY}"),
        ]
    );
    // The answers and the first answer's response are kept verbatim.
    assert_eq!(synced.matches("        - ~~#justdoit~~\n").count(), 2);
    assert!(synced.contains("          - **Response [#r-4fa4a87420e5]**\n"));
    // Nothing else changes: only the two glyphs, the State and the Guidance.
    let expected = ANSWERED
        .replace(
            &format!("- {OPERATOR_OPEN_GLYPH} {VSCODE_ENTRY} "),
            &format!("- {DONE_GLYPH} {VSCODE_ENTRY} "),
        )
        .replace(
            &format!("- {OPERATOR_OPEN_GLYPH} {TITLES_ENTRY} "),
            &format!("- {DONE_GLYPH} {TITLES_ENTRY} "),
        )
        .replace(
            "    - **State:** Waiting on operator input.\n    - **Guidance:** 🙋 3/5 done, 2 yours. Only you can finish this: 2 items for you, listed under Remaining. Strike or answer each row when done.\n",
            "    - **State:** Complete.\n    - **Guidance:** ✅ 5/5 done.\n",
        );
    assert_eq!(synced, expected);

    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op"
    );
}

/// While the agent still works an answer (a live `#justdoit` under the
/// entry), the entry is agent work (`⏳`), not the operator's, and the answer
/// stays under it on every re-render.
#[test]
fn a_live_answer_under_an_entry_is_agent_work_and_stays() {
    let start = ANSWERED
        .find("        - ~~#justdoit~~\n          - **Response")
        .expect("answer");
    let end = ANSWERED[start..]
        .find("      - 🙋 operator: apply edits")
        .map(|at| start + at)
        .expect("next entry");
    let live = format!(
        "{}        - #justdoit\n{}",
        &ANSWERED[..start],
        &ANSWERED[end..]
    );

    let (synced, _) = reconcile_response_remaining(&live).expect("the tree re-renders");
    let (state, guidance, _) = response(&synced, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_FOLLOW_UP, "{synced}");
    assert_eq!(
        guidance,
        format!("⏳ 4/5 done. {}", remaining_guidance(1)),
        "{synced}"
    );
    assert_eq!(
        entry_lines(&synced, ANSWERED_ROOT),
        vec![
            format!("{AGENT_OPEN_GLYPH} {VSCODE_ENTRY}"),
            format!("{DONE_GLYPH} {TITLES_ENTRY}"),
        ]
    );
    assert!(synced.contains(&format!(
        "      - {AGENT_OPEN_GLYPH} {VSCODE_ENTRY} {RESPONSE_DETAIL_MARKER}\n        - #justdoit\n"
    )));
    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op"
    );

    // Striking the answer finishes it.
    let done = synced.replacen("        - #justdoit\n", "        - ~~#justdoit~~\n", 1);
    let (finished, _) = reconcile_response_remaining(&done).expect("the tree re-renders");
    let (state, guidance, _) = response(&finished, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{finished}");
    assert_eq!(guidance, "✅ 5/5 done.");
}

/// A struck answer whose own response is not `Complete.` does not finish the
/// entry it answers.
#[test]
fn a_struck_answer_with_an_unfinished_response_leaves_the_entry_open() {
    let blocked = ANSWERED.replacen(
        "            - **State:** Complete.\n",
        "            - **State:** Blocked.\n",
        1,
    );
    let (synced, _) = reconcile_response_remaining(&blocked).expect("the tree re-renders");
    let (state, guidance, _) = response(&synced, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_WAITING_ON_OPERATOR, "{synced}");
    assert!(guidance.starts_with("🙋 4/5 done, 1 yours."), "{guidance}");
    assert_eq!(
        entry_lines(&synced, ANSWERED_ROOT),
        vec![
            format!("{OPERATOR_OPEN_GLYPH} {VSCODE_ENTRY}"),
            format!("{DONE_GLYPH} {TITLES_ENTRY}"),
        ]
    );
    assert!(synced.contains("          - **Response [#r-4fa4a87420e5]**\n"));
    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op"
    );
}

/// The full sync (slim, what remains, finished-tree strike, guidance)
/// re-renders the answered tree once; a second sync is a no-op.
#[test]
fn the_full_sync_re_renders_the_answered_tree_once() {
    let synced = crate::response_tree_sync::sync_response_trees_striking(ANSWERED, &|_| false)
        .expect("the tree re-renders")
        .document;
    let (state, guidance, _) = response(&synced, ANSWERED_ROOT);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    assert!(guidance.starts_with("✅ 5/5 done."), "{guidance}");
    assert!(synced.contains("          - **Response [#r-4fa4a87420e5]**\n"));
    assert_eq!(
        crate::response_tree_sync::sync_response_trees_striking(&synced, &|_| false),
        None,
        "re-sync is a no-op"
    );
}
