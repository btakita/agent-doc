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
/// typing cut of the second row before the next existed, up to the finished
/// row (each cut replaced by the next: the operator typed on from it).
fn history() -> TypingHistory {
    let mut versions = vec![doc(PIPE)];
    for tail in [
        "",
        " Is this a way to declare ",
        " Is this a way to declare guis from agent-doc",
        " Is this a way to declare guis from agent-doc to present information?",
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

// ---- `#dupwrite2` ---------------------------------------------------------

const PIPE_ROW: &str = "I do like the pipe operator for commands as well.";

/// The live `agent:run` tail once preflight admitted the copies (agent-loop
/// superproject commit 313a3fa5c5 onward): every copy is a `### ` head with
/// its own generated `[#q-…]`, identical rows carry `⟦effect:N⟧`, and the two
/// bare copies typed before their `- ` were rewritten into `---` prompt
/// fences.
fn admitted_live() -> String {
    doc(&format!(
        concat!(
            "- ### 📥 {pipe} ⟦effect:1⟧ [#q-8536fadf0285]\n",
            "---\n",
            "📥 {snap1} [#q-217e769965d7]\n",
            "---\n",
            "- ### 📥 {snap2} ⟦effect:1⟧ [#q-0feb5aebf877]\n",
            "- ### 📥 {pipe} ⟦effect:2⟧ [#q-c26e3ea230ac]\n",
            "- ### ~~🗑️ do [#poolrow-8ac1]~~\n",
            "- ### 📥 {snap3} [#q-140c87b4f232]\n",
            "- ### 📥 {pipe} ⟦effect:3⟧ [#q-158f900202b6]\n",
            "---\n",
            "📥 {snap1} [#q-12d6f4ea6884]\n",
            "---\n",
            "- ### 📥 {snap2} ⟦effect:2⟧ [#q-37cd908220ef]\n",
            "- ### 📥 {pipe} ⟦effect:4⟧ [#q-9e92831a7369]\n",
            "- ### 📥 {finished} [#q-68b3244de244]\n",
        ),
        pipe = PIPE_ROW,
        snap1 = typed(""),
        snap2 = typed(" Is this a way to declare "),
        snap3 = typed(" Is this a way to declare guis from agent-doc"),
        finished = typed(" Is this a way to declare guis from agent-doc to present information?"),
    ))
}

fn unclaimed(_: &str) -> bool {
    false
}

/// Measured 2026-11-24: `queue sync` removed nothing on the live document.
/// The state ledger keeps only its newest merge baselines and retained-write
/// cuts, and by then every one of them held the damaged state; git had no
/// commit of the document between 12:03:43 (the operator mid-way through
/// typing `- I do like the pipe`) and 12:30:48 (every copy present). The
/// committed copy from 12:03 still proves the pipe row was ONE row typed
/// once, so its later copies go; nothing recorded ever held a Declare-lang
/// snapshot before the next one existed, so those stay and are reported.
#[test]
fn dupwrite2_committed_typing_prefix_proves_the_pipe_copies_but_not_unrecorded_snapshots() {
    // An unrelated version (no row of any candidate's lineage) is skipped
    // unparsed and changes nothing.
    let versions = [
        doc("- ship the release notes\n"),
        doc("- I do like the pipe\n"),
        damaged(),
        admitted_live(),
        admitted_live(),
    ];
    let mut relevant = |works: &[String]| TypingHistory::from_relevant_documents(&versions, works);
    assert_eq!(relevant(&[PIPE_ROW.to_string()]).len(), 4);
    let outcome = drop_stale_typing_copies_with(&admitted_live(), &mut relevant, &unclaimed);
    let expected = admitted_live()
        .replace(
            &format!("- ### 📥 {PIPE_ROW} ⟦effect:1⟧ [#q-8536fadf0285]\n"),
            "",
        )
        .replace(
            &format!("- ### 📥 {PIPE_ROW} ⟦effect:2⟧ [#q-c26e3ea230ac]\n"),
            "",
        )
        .replace(
            &format!("- ### 📥 {PIPE_ROW} ⟦effect:3⟧ [#q-158f900202b6]\n"),
            "",
        );
    assert_eq!(outcome.document, expected, "{:?}", outcome.repairs);
    assert_eq!(outcome.repairs.len(), 3, "{:?}", outcome.repairs);
    // The Declare-lang snapshots are candidates without proof: reported, kept.
    assert_eq!(outcome.unproven.len(), 5, "{:?}", outcome.unproven);
    assert!(
        outcome
            .unproven
            .iter()
            .all(|work| work.starts_with(LINK) && !work.ends_with("information?")),
        "{:?}",
        outcome.unproven
    );
}

/// With the operator's typing recorded, every copy goes, and a bare copy
/// that a rewrite fenced in `---` lines takes its fence with it: an orphan
/// `---` pair would parse as an empty prompt fence or a setext underline.
#[test]
fn dupwrite2_fenced_bare_copy_goes_with_its_fence() {
    let recorded = || {
        TypingHistory::from_documents(&[
            doc(&format!("- {PIPE_ROW}\n")),
            doc(&format!("- {PIPE_ROW}\n{}\n", typed(""))),
            doc(&format!("- {PIPE_ROW}\n- {}\n", typed(""))),
            doc(&format!(
                "- {PIPE_ROW}\n- {}\n",
                typed(" Is this a way to declare ")
            )),
            doc(&format!(
                "- {PIPE_ROW}\n- {}\n",
                typed(" Is this a way to declare guis from agent-doc")
            )),
            doc(&format!(
                "- {PIPE_ROW}\n- {}\n",
                typed(" Is this a way to declare guis from agent-doc to present information?")
            )),
            admitted_live(),
        ])
    };
    let outcome = drop_stale_typing_copies_with(&admitted_live(), &mut |_| recorded(), &unclaimed);
    let expected = doc(&format!(
        concat!(
            "- ### ~~🗑️ do [#poolrow-8ac1]~~\n",
            "- ### 📥 {pipe} ⟦effect:4⟧ [#q-9e92831a7369]\n",
            "- ### 📥 {finished} [#q-68b3244de244]\n",
        ),
        pipe = PIPE_ROW,
        finished = typed(" Is this a way to declare guis from agent-doc to present information?"),
    ));
    assert_eq!(outcome.document, expected, "{:?}", outcome.repairs);
    assert_eq!(outcome.repairs.len(), 8, "{:?}", outcome.repairs);
    assert!(outcome.unproven.is_empty(), "{:?}", outcome.unproven);
    // Idempotent.
    let again = drop_stale_typing_copies_with(&outcome.document, &mut |_| recorded(), &unclaimed);
    assert_eq!(again.document, outcome.document);
    assert!(again.repairs.is_empty());
}

/// A shorter row is a typing snapshot only when a recorded version shows
/// the operator typing on from it: the longer text present and the shorter
/// row gone. A row the operator kept beside a longer one added later (an
/// hour or a day apart in the committed history) is never one.
#[test]
fn dupwrite2_a_row_never_replaced_by_its_extension_is_not_a_typing_snapshot() {
    let short = "- release notes for 0.36\n";
    let long = "- release notes for 0.36 and the migration guide\n";
    let current = doc(&format!("{short}{long}"));
    let recorded = || TypingHistory::from_documents(&[doc(short), current.clone()]);
    let outcome = drop_stale_typing_copies_with(&current, &mut |_| recorded(), &unclaimed);
    assert_eq!(outcome.document, current, "{:?}", outcome.repairs);
    assert!(outcome.repairs.is_empty());
    assert_eq!(outcome.unproven, vec!["release notes for 0.36".to_string()]);
}

/// A copy a ledger claim holds is a worker's row: never removed, even with
/// proof (no claim may point at a removed row).
#[test]
fn dupwrite2_a_claimed_copy_is_never_removed() {
    let current = doc(&format!("- 📥 {PIPE_ROW}\n- 📥 {PIPE_ROW} ⟦effect:2⟧\n"));
    let recorded = || TypingHistory::from_documents(&[doc(&format!("- {PIPE_ROW}\n"))]);
    let claimed =
        |row: &str| row.contains("⟦effect:2⟧") || row.trim_start_matches("📥 ") == PIPE_ROW;
    let outcome = drop_stale_typing_copies_with(&current, &mut |_| recorded(), &claimed);
    assert_eq!(outcome.document, current, "{:?}", outcome.repairs);
    // Only the first copy claimed: the unclaimed duplicate goes.
    let first_only = |row: &str| !row.contains("⟦effect:2⟧");
    let outcome = drop_stale_typing_copies_with(&current, &mut |_| recorded(), &first_only);
    assert_eq!(outcome.document, doc(&format!("- 📥 {PIPE_ROW}\n")));
}

/// A copy whose generated id another queue row refers to stays, and is the
/// keeper of its duplicates.
#[test]
fn dupwrite2_a_copy_another_row_refers_to_is_never_removed() {
    let first = format!("- 📥 {PIPE_ROW} [#q-0000000000b1]\n");
    let rest =
        format!("- 📥 {PIPE_ROW} ⟦effect:2⟧ [#q-0000000000b2]\n- re [#q-0000000000b2]: yes\n");
    let current = doc(&format!("{first}{rest}"));
    let recorded = || TypingHistory::from_documents(&[doc(&format!("- {PIPE_ROW}\n"))]);
    let outcome = drop_stale_typing_copies_with(&current, &mut |_| recorded(), &unclaimed);
    assert_eq!(outcome.document, doc(&rest), "{:?}", outcome.repairs);
}
