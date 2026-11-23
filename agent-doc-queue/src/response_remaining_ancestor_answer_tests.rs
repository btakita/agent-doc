//! `#answeredentry`: one answer resolves the action item in every response
//! that lists it, the response that owns the item and each enclosing one.

use super::*;

/// The live shape after `#tornsplice` self-repair, anonymized only by
/// omission: a struck step-two root whose response lists, besides its two
/// answered agent entries, the operator item a nested `#justdoit` response
/// (`[#r-6837aa93963f]`) owns. The operator answered that item under the
/// root's entry (`~~VSCodium not running.~~`), so the root reads `✅ 5/5
/// done.`, while the nested response still says `🙋 1/2 done, 1 yours.` for
/// the same item: a struck tree giving two answers about one item.
const ANCESTOR: &str = include_str!("../tests/fixtures/response_remaining_ancestor_answer.md");
const ROOT_RESPONSE: &str = "- **Response [#r-a4794e8fee5c]**";
const NESTED_RESPONSE: &str = "- **Response [#r-6837aa93963f]**";
const OUTER_ENTRY: &str =
    "      - ✅ operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n";
const NESTED_ENTRY: &str = "              - 🙋 operator: reload any open VSCodium windows <!-- agent-doc:response-detail -->\n";
const ANSWER_ROW: &str = "        - ~~🗑️ VSCodium not running. [#q-c4b5011d0725]~~\n";

/// The State and Guidance values of the response `marker`, and whether it
/// still has a Remaining list.
fn state_guidance(document: &str, marker: &str) -> (String, String, bool) {
    let start = document.find(marker).expect("response");
    let indent = document[..start].len() - document[..start].trim_end_matches(' ').len();
    let detail = format!("{}- ", " ".repeat(indent + 2));
    let mut state = None;
    let mut guidance = None;
    let mut remaining = false;
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
        } else if rest == REMAINING_LABEL {
            remaining = true;
        }
    }
    (
        state.expect("a State"),
        guidance.expect("a Guidance"),
        remaining,
    )
}

#[test]
fn an_answer_under_an_ancestor_entry_resolves_the_nested_item() {
    let (state, guidance, _) = state_guidance(ANCESTOR, NESTED_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_WAITING_ON_OPERATOR);
    assert!(guidance.starts_with("🙋 1/2 done, 1 yours."), "{guidance}");

    let (synced, _) =
        reconcile_response_remaining(ANCESTOR).expect("the nested response re-renders");
    let (state, guidance, remaining) = state_guidance(&synced, NESTED_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    assert_eq!(guidance, "✅ 2/2 done.", "{synced}");
    // The nested copy of the answered entry flips to ✅ with it.
    assert!(remaining, "{synced}");
    assert!(
        synced.contains(&NESTED_ENTRY.replacen(OPERATOR_OPEN_GLYPH, DONE_GLYPH, 1)),
        "{synced}"
    );
    assert!(!synced.contains(OPERATOR_OPEN_GLYPH), "{synced}");
    // The answer stays where the operator wrote it, under the root's entry.
    assert!(
        synced.contains(&format!("{OUTER_ENTRY}{ANSWER_ROW}")),
        "{synced}"
    );
    // Nothing in the tree says the item is still the operator's.
    assert!(!synced.contains("yours"), "{synced}");
    let (state, guidance, _) = state_guidance(&synced, ROOT_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE);
    assert!(guidance.starts_with("✅ 5/5 done."), "{guidance}");
    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op:\n{synced}"
    );
}

/// The full tree sync agrees: with every item answered, the struck root's
/// tree is deletable, so its glyph turns from 🔒 (keep) to 🗑️ and its
/// Guidance carries the delete indicator.
#[test]
fn the_struck_root_and_its_subtree_agree_after_the_full_sync() {
    let synced = crate::response_tree_sync::sync_response_trees_striking(ANCESTOR, &|_| false)
        .expect("the tree syncs")
        .document;
    assert!(
        synced.contains("- ### ~~🗑️ [#q-e9483473162d] follow-up:"),
        "{synced}"
    );
    let (state, guidance, _) = state_guidance(&synced, ROOT_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE);
    assert_eq!(
        guidance,
        format!("✅ 5/5 done. {NOTHING_REMAINS_ROW_GUIDANCE}"),
        "{synced}"
    );
    assert!(!synced.contains(OPERATOR_OPEN_GLYPH), "{synced}");
    assert_eq!(
        crate::response_tree_sync::sync_response_trees_striking(&synced, &|_| false),
        None,
        "re-sync is a no-op"
    );
}

/// The other direction: an answer under the nested response's own entry
/// resolves the same item in the root's list too.
#[test]
fn an_answer_under_the_owning_entry_resolves_the_ancestor_entry() {
    let answer_start = ANCESTOR.find(ANSWER_ROW).expect("answer");
    let answer_end = ANCESTOR[answer_start..]
        .find("    - **State:** Complete.\n    - **Guidance:** ✅ 5/5 done.")
        .map(|at| answer_start + at)
        .expect("root State");
    let answer = &ANCESTOR[answer_start..answer_end];
    let moved: String = answer
        .lines()
        .map(|line| format!("        {line}\n"))
        .collect();
    let document = ANCESTOR
        .replace(&format!("{OUTER_ENTRY}{answer}"), "")
        .replace(NESTED_ENTRY, &format!("{NESTED_ENTRY}{moved}"));
    assert!(!document.contains(OUTER_ENTRY), "{document}");

    let synced =
        reconcile_response_remaining(&document).map_or(document.clone(), |(synced, _)| synced);
    let (state, _, _) = state_guidance(&synced, NESTED_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    let (state, guidance, _) = state_guidance(&synced, ROOT_RESPONSE);
    assert_eq!(state, RESPONSE_STATE_COMPLETE, "{synced}");
    assert!(guidance.starts_with("✅ 5/5 done."), "{guidance}");
    assert!(!synced.contains("yours"), "{synced}");
    assert!(!synced.contains(OPERATOR_OPEN_GLYPH), "{synced}");
}

/// While the agent still works the answer (a live row under the root's
/// entry), every copy of the entry is agent work (`⏳`): the nested response
/// no longer counts the item as the operator's either.
#[test]
fn a_live_answer_under_an_ancestor_entry_makes_every_copy_agent_work() {
    let answer_start = ANCESTOR.find(ANSWER_ROW).expect("answer");
    let answer_end = ANCESTOR[answer_start..]
        .find("    - **State:** Complete.\n    - **Guidance:** ✅ 5/5 done.")
        .map(|at| answer_start + at)
        .expect("root State");
    let answer = &ANCESTOR[answer_start..answer_end];
    let document = ANCESTOR.replace(
        &format!("{OUTER_ENTRY}{answer}"),
        &format!("{OUTER_ENTRY}        - #justdoit reload it\n"),
    );

    let (synced, _) = reconcile_response_remaining(&document).expect("the tree re-renders");
    assert!(
        synced.contains(&NESTED_ENTRY.replacen(OPERATOR_OPEN_GLYPH, AGENT_OPEN_GLYPH, 1)),
        "{synced}"
    );
    assert!(
        synced.contains(&format!(
            "{}        - #justdoit reload it\n",
            OUTER_ENTRY.replacen(DONE_GLYPH, AGENT_OPEN_GLYPH, 1)
        )),
        "{synced}"
    );
    for marker in [NESTED_RESPONSE, ROOT_RESPONSE] {
        let (state, guidance, _) = state_guidance(&synced, marker);
        assert_eq!(state, RESPONSE_STATE_FOLLOW_UP, "{synced}");
        assert!(!guidance.contains("yours"), "{guidance}");
    }
    assert!(!synced.contains(OPERATOR_OPEN_GLYPH), "{synced}");
    assert_eq!(
        reconcile_response_remaining(&synced),
        None,
        "re-sync is a no-op:\n{synced}"
    );
}
