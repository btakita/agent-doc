//! `#struckidtrail`: a retained strike rebased over a later editor cut must
//! not re-append the struck row's id after its closing `~~`.
//!
//! Live shape (agent-doc-bugs.md, 2026-11-23 02:41:29Z): `queue consume`
//! struck a reported row whose text ends in its generated `[#q-…]`. A claim
//! projection then marked the row's follow-up, and the controller rebased the
//! still-retained strike over that cut. The merge read the struck row as a
//! pre-admission copy of the live row (the id sits before the closing `~~`,
//! where the trailing-id parser does not look) and projected the id onto it
//! again, after the strike: `~~… [#q-x]~~ [#q-x]`. The row then read as live
//! and the pool re-offered it.

use super::*;

const ORIGIN: &str = "follow up on an action item from queue item \u{201c}` - **Guidance:** \u{1f64b} 3/5 done, 2 yours. Only you can finish this: 2 items for you\u{2026}\u{201d}: agent: check CI run 1001 once it finishes. [#q-0a1b2c3d4e5f]";
const FOLLOWUP: &str = "[#q-0a1b2c3d4e5f] follow-up: agent: re-check runs 1002 and 1003 once they finish. [#q-5f4e3d2c1b0a]";

fn doc(origin: &str, followup: &str) -> String {
    format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: dispatch\n\nDispatched.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- ### ~~earlier finished row [#q-111111111111]~~\n",
            "{}\n",
            "  - **Response [#r-0123456789ab]**\n",
            "    - **Summary:** All named commits passed CI.\n",
            "    - **State:** Complete.\n",
            "    - **Guidance:** \u{1f5d1}\u{fe0f} Safe to delete this tree. Its action items moved to follow-up rows.\n",
            "{}\n",
            "- later work [#q-222222222222]\n",
            "<!-- /agent:queue -->\n",
        ),
        origin, followup,
    )
}

fn claimed_origin() -> String {
    format!("- \u{1f6a7} [#subagent:cif#1] {ORIGIN}")
}

fn struck_origin() -> String {
    format!("- ### ~~{ORIGIN}~~")
}

/// The live sequence: the retained strike (base: claimed row, target: struck
/// row) is rebased over a cut that already holds the strike and a new claim
/// marker on the follow-up row. The cut is the fixed point.
#[test]
fn a_retained_strike_rebased_over_a_followup_claim_keeps_one_id_inside_the_strike() {
    let base = doc(&claimed_origin(), &format!("- {FOLLOWUP}"));
    let target = doc(&struck_origin(), &format!("- {FOLLOWUP}"));
    let cut = doc(
        &struck_origin(),
        &format!("- \u{1f7e1} [#subagent:ci3#1] {FOLLOWUP}"),
    );
    let rebased = rebase_retained_target_over_editor_cut(&base, &target, &cut).unwrap();
    assert_eq!(rebased, cut, "{rebased}");
    let again = rebase_retained_target_over_editor_cut(&base, &target, &rebased).unwrap();
    assert_eq!(again, rebased, "a second rebase is a no-op");
}

/// The same strike rebased over a cut equal to its target.
#[test]
fn a_retained_strike_rebased_over_its_own_target_is_unchanged() {
    let base = doc(&claimed_origin(), &format!("- {FOLLOWUP}"));
    let target = doc(&struck_origin(), &format!("- {FOLLOWUP}"));
    let rebased = rebase_retained_target_over_editor_cut(&base, &target, &target).unwrap();
    assert_eq!(rebased, target, "{rebased}");
}

/// The three-way merge underneath: one side unchanged, the other struck the
/// row. The strike wins with its one id, in either side order, and whatever
/// the row's text (the bug needed a work text of 24+ characters, the floor
/// for reading a live row as a continuation of an admitted one).
#[test]
fn merging_a_strike_against_an_unchanged_side_never_duplicates_the_id() {
    for work in [
        ORIGIN,
        "quoted `code` x: agent: check. [#q-0a1b2c3d4e5f]",
        "do the thing [#q-0a1b2c3d4e5f]",
    ] {
        let base = doc(&format!("- {work}"), "- other [#q-5f4e3d2c1b0a]");
        let target = doc(&format!("- ~~{work}~~"), "- other [#q-5f4e3d2c1b0a]");
        let state = agent_doc_merge::crdt::CrdtDoc::from_text(&base).encode_state();
        for (ours, theirs) in [(&base, &target), (&target, &base)] {
            let (merged, _) =
                agent_doc_merge::merge_contents_crdt(Some(&state), ours, theirs).unwrap();
            assert_eq!(merged, target, "{work}:\n{merged}");
        }
    }
}
