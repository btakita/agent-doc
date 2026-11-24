use super::*;

fn doc(run: &str) -> String {
    format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Run <!-- agent:run id=\"main\" subagents go -->\n",
            "- ### https://github.com/acme/widget/issues/7\n",
            "  - %% widget crashes on start %%\n",
            "{}",
            "<!-- /agent:run -->\n\n",
            "## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
        ),
        run
    )
}

const PIPE: &str = "- I do like the pipe operator for commands as well.\n";
const LINK: &str = "[Declare lang](https://example.invalid/declare/)";

fn typed(tail: &str) -> String {
    format!("{LINK} looks interesting.{tail}")
}

/// The damaged `agent:run` of 2026-11-24T17:19Z, in shape: four copies of
/// one row (two with the `📥` intake projection), four typing snapshots of
/// another (two before its `- ` list marker), the finished row last, and
/// claim/struck rows of one finished item.
fn damaged() -> String {
    doc(&format!(
        concat!(
            "- 📥 I do like the pipe operator for commands as well.\n",
            "📥 {snap1}\n",
            "- 📥 {snap2}\n",
            "- 📥 I do like the pipe operator for commands as well.\n",
            "- ### ☑️ [#subagent:poolrow#1] do [#poolrow-8ac1]\n",
            "- ### ⚠️ [#subagent:poolrow#1] do [#poolrow-8ac1]\n",
            "- {snap3}\n",
            "- ### ~~do [#poolrow-8ac1]~~\n",
            "{pipe}",
            "📥 {snap1}\n",
            "- 📥 {snap2}\n",
            "- ### ~~do [#poolrow-8ac1]~~\n",
            "{pipe}",
            "- {finished}\n",
            "- operator note: agent-doc duplicated my rows.\n",
        ),
        snap1 = typed(""),
        snap2 = typed(" Is this a way to declare "),
        snap3 = typed(" Is this a way to declare guis from agent-doc"),
        finished = typed(" Is this a way to declare guis from agent-doc to present information?"),
        pipe = PIPE,
    ))
}

/// Recorded versions while the operator typed: the pipe row once, then each
/// typing cut of the second row before the next existed.
fn history() -> TypingHistory {
    let mut versions = vec![doc(PIPE)];
    for tail in [
        "",
        " Is this a way to declare ",
        " Is this a way to declare guis from agent-doc",
    ] {
        versions.push(doc(&format!("{PIPE}{}\n", typed(tail))));
    }
    TypingHistory::from_documents(&versions)
}

#[test]
fn dupwrite_self_repair_removes_proven_stale_copies_and_keeps_the_newest_rows() {
    let (repaired, repairs) = drop_stale_typing_copies(&damaged(), &mut history);
    let expected = doc(&format!(
        concat!(
            "- ### ☑️ [#subagent:poolrow#1] do [#poolrow-8ac1]\n",
            "- ### ⚠️ [#subagent:poolrow#1] do [#poolrow-8ac1]\n",
            "- ### ~~do [#poolrow-8ac1]~~\n",
            "- ### ~~do [#poolrow-8ac1]~~\n",
            "{pipe}",
            "- {finished}\n",
            "- operator note: agent-doc duplicated my rows.\n",
        ),
        finished = typed(" Is this a way to declare guis from agent-doc to present information?"),
        pipe = PIPE,
    ));
    assert_eq!(repaired, expected, "{repaired}");
    assert_eq!(repairs.len(), 8, "{repairs:?}");
    assert!(
        repairs
            .iter()
            .all(|repair| repair.kind == SelfRepairKind::StaleTypingCopy)
    );
    // Idempotent.
    let (again, more) = drop_stale_typing_copies(&repaired, &mut history);
    assert_eq!(again, repaired);
    assert!(more.is_empty());
}

#[test]
fn dupwrite_self_repair_needs_recorded_proof() {
    // No recorded version: nothing is proven, nothing is removed.
    let (repaired, repairs) = drop_stale_typing_copies(&damaged(), &mut TypingHistory::default);
    assert_eq!(repaired, damaged());
    assert!(repairs.is_empty());

    // The operator keeps a short row and a longer one on purpose: every
    // recorded version holds both, so the short row was never a typing cut
    // of the long one. Two identical rows recorded twice from the start are
    // the operator's too.
    let short = "- release notes for 0.36\n";
    let long = "- release notes for 0.36 and the migration guide\n";
    let both = doc(&format!("{short}{long}{PIPE}{PIPE}"));
    let mut recorded = || TypingHistory::from_documents(&[both.clone(), both.clone()]);
    let (repaired, repairs) = drop_stale_typing_copies(&both, &mut recorded);
    assert_eq!(repaired, both);
    assert!(repairs.is_empty());
}

#[test]
fn dupwrite_self_repair_never_touches_rows_with_work_attached() {
    // A row with children, an id, a claim, a strike, a short row, and rows
    // under different parents are never copies.
    let current = doc(concat!(
        "- write the release notes\n",
        "  - include the migration guide\n",
        "- write the release notes and the changelog\n",
        "- do [#relnotes]\n",
        "- do [#relnotes] now\n",
        "- 🚧 [#subagent:notes#1] #rebase\n",
        "- 🚧 [#subagent:notes#1] #rebase now\n",
        "- ~~write the docs~~\n",
        "- ~~write the docs~~ today\n",
        "- go\n",
        "- go ahead\n",
        "- ### https://github.com/acme/widget/issues/8\n",
        "  - merge it after CI\n",
        "- ### https://github.com/acme/widget/issues/9\n",
        "  - merge it after CI passes\n",
        "- ### https://github.com/acme/widget/issues/10\n",
        "- ### https://github.com/acme/widget/issues/10\n",
    ));
    let mut recorded = || TypingHistory::from_documents(&[doc("")]);
    let (repaired, repairs) = drop_stale_typing_copies(&current, &mut recorded);
    assert_eq!(repaired, current, "{repairs:?}");
    // Even with every row once recorded alone, nothing qualifies.
    let mut recorded = || {
        TypingHistory::from_documents(&[doc(concat!(
            "- write the release notes\n",
            "- do [#relnotes]\n",
            "- ~~write the docs~~\n",
            "- go\n",
            "- ### https://github.com/acme/widget/issues/8\n",
            "  - merge it after CI\n",
        ))])
    };
    let (repaired, repairs) = drop_stale_typing_copies(&current, &mut recorded);
    assert_eq!(repaired, current, "{repairs:?}");
}

/// After preflight admitted the copies as queue heads (`### ` heading,
/// generated `[#q-…]` ids, `⟦effect:N⟧` ordinals on identical rows), they
/// are still the operator's free-text rows and still proven copies.
#[test]
fn dupwrite_self_repair_sees_through_admitted_heads() {
    let current = doc(&format!(
        concat!(
            "- ### 📥 I do like the pipe operator for commands as well. [#q-0000000000a1]\n",
            "📥 {snap1} [#q-0000000000a2]\n",
            "- ### 📥 {snap3} [#q-0000000000a3]\n",
            "- ### 📥 I do like the pipe operator for commands as well. ⟦effect:2⟧ [#q-0000000000a4]\n",
            "- ### 📥 {finished} [#q-0000000000a5]\n",
        ),
        snap1 = typed(""),
        snap3 = typed(" Is this a way to declare guis from agent-doc"),
        finished = typed(" Is this a way to declare guis from agent-doc to present information?"),
    ));
    let (repaired, repairs) = drop_stale_typing_copies(&current, &mut history);
    assert_eq!(
        repaired,
        doc(&format!(
            concat!(
                "- ### 📥 I do like the pipe operator for commands as well. [#q-0000000000a1]\n",
                "- ### 📥 {finished} [#q-0000000000a5]\n",
            ),
            finished =
                typed(" Is this a way to declare guis from agent-doc to present information?"),
        )),
        "{repairs:?}"
    );
    assert_eq!(repairs.len(), 3);
}
