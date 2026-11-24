use super::*;

fn body(cells: &[&str]) -> String {
    cells.concat()
}

const PROMPT_CELL: &str =
    "### Re: run [#q-1]\n> **Queue prompt:** do the thing\nPlain prose that no pass reads.\n";
const FOLLOWUP_CELL: &str =
    "### Re: follow ups\n> [#origin] follow-up: agent: check CI\nMore prose.\n";
const EMPTY_PROMPT_CELL: &str =
    "### Re: empty prompt\n> **Queue prompt:**\n>\nunrelated line\n> late quote\n";
const CODE_BLOCK_CELL: &str = "### Re: code\n```\nfn main() {}\n```\n";

/// The digest keeps exactly the lines a pass reads, every line while an
/// echo is awaited, and a chunk's heading only when it names an echo.
#[test]
fn the_digest_keeps_the_lines_passes_read() {
    let index = ExchangeIndex::build(&body(&[PROMPT_CELL, FOLLOWUP_CELL, EMPTY_PROMPT_CELL]));
    assert_eq!(
        index.digest(),
        format!(
            "{DIGEST_SENTINEL}### Re: run [#q-1]\n> **Queue prompt:** do the thing\n> [#origin] follow-up: agent: check CI\n### Re: empty prompt\n> **Queue prompt:**\n>\nunrelated line\n"
        )
    );
    assert!(index.neutral());
}

/// After any splice, the index equals the from-scratch index of the new
/// body, and the delta reads only the touched chunks.
#[test]
fn a_splice_reindexes_only_the_touched_chunks() {
    let cells = [
        PROMPT_CELL,
        FOLLOWUP_CELL,
        CODE_BLOCK_CELL,
        EMPTY_PROMPT_CELL,
        PROMPT_CELL,
    ];
    let old = body(&cells);
    let cases: Vec<(std::ops::Range<usize>, String)> = vec![
        // Append a cell.
        (old.len()..old.len(), FOLLOWUP_CELL.to_string()),
        // Compact: drop the first three cells behind a summary.
        (
            0..PROMPT_CELL.len() + FOLLOWUP_CELL.len() + CODE_BLOCK_CELL.len(),
            "### Compacted\n> Earlier exchange archived.\n".to_string(),
        ),
        // Edit inside one cell.
        (
            PROMPT_CELL.len() + 5..PROMPT_CELL.len() + 9,
            "XY".to_string(),
        ),
        // Break a heading so two chunks merge.
        (PROMPT_CELL.len()..PROMPT_CELL.len() + 1, String::new()),
        // Insert a heading in the middle of a cell, splitting it.
        (12..12, "\n### split\n".to_string()),
    ];
    for (removed, inserted) in cases {
        let mut next = old.clone();
        next.replace_range(removed.clone(), &inserted);
        let mut index = ExchangeIndex::build(&old);
        let delta = index.apply(removed.clone(), inserted.len(), &next);
        assert_eq!(
            index,
            ExchangeIndex::build(&next),
            "{removed:?} {inserted:?}"
        );
        assert!(
            delta.added_bytes() < next.len() || next.len() < 200,
            "{removed:?}: read {} of {}",
            delta.added_bytes(),
            next.len()
        );
    }
}

/// A compaction that only removes cells adds no evidence; an appended cell
/// with a queue prompt does.
#[test]
fn removing_evidence_is_not_new_evidence() {
    let old = body(&[PROMPT_CELL, FOLLOWUP_CELL, PROMPT_CELL]);
    let mut index = ExchangeIndex::build(&old);
    let summary = "### Compacted\nEarlier exchange archived.\n";
    let removed = 0..PROMPT_CELL.len() + FOLLOWUP_CELL.len();
    let mut next = old.clone();
    next.replace_range(removed.clone(), summary);
    let delta = index.apply(removed, summary.len(), &next);
    assert!(!delta.adds_evidence());
    assert!(!delta.structural());

    let appended = format!("{next}### Re: new\n> **Queue prompt:** another\n");
    let delta = index.apply(
        next.len()..next.len(),
        appended.len() - next.len(),
        &appended,
    );
    assert!(delta.adds_evidence());
}

/// An unclosed fence or marker text is structural.
#[test]
fn an_unclosed_fence_or_marker_is_structural() {
    let old = body(&[PROMPT_CELL]);
    for inserted in [
        "### Re: open\n```\nno close\n",
        "### Re: m\n`<!-- agent:queue -->`\n",
    ] {
        let mut index = ExchangeIndex::build(&old);
        let next = format!("{old}{inserted}");
        let delta = index.apply(old.len()..old.len(), inserted.len(), &next);
        assert!(delta.structural(), "{inserted:?}");
    }
    assert!(!has_marker_like_text("<!-- agent:boundary:abc -->"));
    assert!(!has_marker_like_text(
        "<!-- agent-doc-subagent-completion key=x -->"
    ));
}
