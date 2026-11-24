//! `#composefault` root: admitting a row's generated `[#q-…]` re-keys its cell
//! (`queue:N:txt:<work>` to `queue:N:id:q-…`). The per-cell merge must still
//! read the admission as an update of that one row.
//!
//! Measured on haiven-dev `tasks/sdk/sdk-dev.md` (2026-11-24T21:45:24Z):
//! report-complete's retained target admitted the claimed row's id and marked
//! it `☑️`; the operator was typing elsewhere in the queue. The controller's
//! projection of the target over that cut (`retained_transition_projection_effect`)
//! kept the row twice, the `☑️ … [#q-…]` copy with its follow-ups and the
//! stale `🚧` copy after them, because the base row's identity and the
//! admitted row's promotion alias were spelled differently (one with the
//! `queue:N:` cell prefix, one without), so the alias never matched and the
//! `#keepqueueitems` rule restored the "omitted" operator row. The next
//! `queue consume --ack-text` struck that stale copy and attached its response
//! tree under it, after the follow-ups: the `#composefault` shapes.

use super::merge_3way;
use crate::crdt::{CrdtDoc, merge_by_component};

const WORK: &str = "fix the parser";
const ID: &str = "q-0123456789ab";
const OTHER: &str = "- ### update the changelog [#q-6f5e4d3c2b1a]\n";
const TYPED: &str = "- operator typed a new row\n";

fn doc(rows: &[&str]) -> String {
    format!(
        "<!-- agent:queue -->\n{}<!-- /agent:queue -->\n",
        rows.concat()
    )
}

fn rows_holding(text: &str, needle: &str) -> usize {
    text.lines().filter(|line| line.contains(needle)).count()
}

#[test]
fn an_admitted_id_with_a_lifecycle_marker_is_one_row_beside_operator_typing() {
    let claimed = format!("- 🚧 [#subagent:x#1] {WORK}\n");
    let reported = format!("- ☑️ [#subagent:x#1] {WORK} [#{ID}]\n");
    let followup = format!("- [#{ID}] follow-up: operator: choose one\n");
    let base = doc(&[&claimed, OTHER]);
    let ours = doc(&[&reported, &followup, OTHER]);
    let theirs = doc(&[&claimed, OTHER, TYPED]);
    let out = merge_3way(&base, &ours, &theirs);
    assert!(!out.fell_back, "{out:?}");
    assert_eq!(
        out.merged_text,
        doc(&[&reported, &followup, OTHER, TYPED]),
        "{}",
        out.merged_text
    );
}

#[test]
fn a_bare_id_admission_is_one_row_beside_operator_typing() {
    let bare = format!("- {WORK}\n");
    let admitted = format!("- {WORK} [#{ID}]\n");
    let base = doc(&[&bare, OTHER]);
    let ours = doc(&[&admitted, OTHER]);
    let theirs = doc(&[&bare, OTHER, TYPED]);
    let out = merge_3way(&base, &ours, &theirs);
    assert!(!out.fell_back, "{out:?}");
    assert_eq!(
        rows_holding(&out.merged_text, WORK),
        1,
        "{}",
        out.merged_text
    );
    assert_eq!(
        out.merged_text,
        doc(&[&admitted, OTHER, TYPED]),
        "{}",
        out.merged_text
    );
}

/// The measured shape: the row is a child of a PR heading row, beside a note.
#[test]
fn a_nested_admitted_id_with_a_lifecycle_marker_is_one_row_beside_operator_typing() {
    let pr = "- ### https://github.com/acme/api/pull/930\n";
    let note = "  - %% fix(auth): always issue durable credentials %%\n";
    let other = "- ### https://github.com/acme/api/pull/933\n";
    let typed = "- check the deploy log for\n";
    let claimed = format!("  - 🚧 [#subagent:x#1] {WORK}\n");
    let reported = format!("  - ☑️ [#subagent:x#1] {WORK} [#{ID}]\n");
    let f1 = format!("  - [#{ID}] follow-up: operator: choose one [operator-verify]\n");
    let f2 = format!("  - [#{ID}] follow-up: operator: hold #930 [operator-verify]\n");
    let base = doc(&[pr, note, &claimed, other]);
    let ours = doc(&[pr, note, &reported, &f1, &f2, other]);
    let theirs = doc(&[pr, note, &claimed, other, typed]);
    let out = merge_3way(&base, &ours, &theirs);
    assert!(!out.fell_back, "{out:?}");
    assert_eq!(
        out.merged_text,
        doc(&[pr, note, &reported, &f1, &f2, other, typed]),
        "{}",
        out.merged_text
    );
}

/// The measured documents byte for byte (work text with sentence punctuation,
/// the queue under a heading with marker attributes).
#[test]
fn the_measured_report_complete_admission_is_one_row_beside_operator_typing() {
    let frame = |rows: &str| {
        format!(
            "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\n❯ Keep going.\n<!-- /agent:exchange -->\n\n## Run <!-- agent:queue id=\"main\" subagents go -->\n- ### https://github.com/acme/api/pull/930\n  - %% fix(auth): always issue durable credentials %%\n{rows}- ### https://github.com/acme/api/pull/933\n"
        )
    };
    let work = "I think we should use stateless JWTs. What is the reason against it?";
    let claimed = format!("  - 🚧 [#subagent:jwtadopt#1] {work}\n");
    let reported = format!(
        "  - ☑\u{fe0f} [#subagent:jwtadopt#1] {work} [#q-2277fd38654a]\n  - [#q-2277fd38654a] follow-up: operator: choose one: (1) durable rows plus retention; (2) Model C, a stateless durable JWT with a denylist. [operator-verify]\n  - [#q-2277fd38654a] follow-up: operator: hold #930 until you choose. [operator-verify]\n"
    );
    let close = "<!-- /agent:queue -->\n";
    let typed = "- check the deploy log for\n";
    let base = format!("{}{close}", frame(&claimed));
    let ours = format!("{}{close}", frame(&reported));
    let theirs = format!("{}{typed}{close}", frame(&claimed));
    let expected = format!("{}{typed}{close}", frame(&reported));
    let out = merge_3way(&base, &ours, &theirs);
    assert!(!out.fell_back, "{out:?}");
    assert_eq!(out.merged_text, expected, "{}", out.merged_text);
    // The component CRDT merge the retained rebase falls back to agrees.
    let state = CrdtDoc::from_text(&base).encode_state();
    let merged = merge_by_component(Some(&state), &ours, &theirs).unwrap();
    assert_eq!(merged, expected, "{merged}");
}

/// The operator struck the row while the admission was in flight: one row,
/// and the strike stands (it lost to the admission while the two were
/// separate cells).
#[test]
fn an_admission_racing_the_operators_strike_keeps_one_struck_row() {
    let bare = format!("- {WORK}\n");
    let admitted = format!("- {WORK} [#{ID}]\n");
    let struck = format!("- ~~{WORK}~~\n");
    let base = doc(&[&bare, OTHER]);
    let ours = doc(&[&admitted, OTHER]);
    let theirs = doc(&[&struck, OTHER, TYPED]);
    let out = merge_3way(&base, &ours, &theirs);
    assert!(!out.fell_back, "{out:?}");
    assert_eq!(
        rows_holding(&out.merged_text, WORK),
        1,
        "{}",
        out.merged_text
    );
    assert!(
        out.merged_text.contains(&format!("- ~~{WORK}")),
        "{}",
        out.merged_text
    );
}
