//! `#coinedid` — detect `#id` tags a response invented but never recorded.
//!
//! In agent-doc, `#tags` are the identifier system for tracked work: `do [#id]`
//! queue heads, `agent:backlog` entries, `--done <id>`. A turn that coins a NEW
//! tag and puts it in a commit message or a code comment creates an identifier
//! that *looks* tracked and resolves to nothing — a reader greps it later and
//! finds confident prose with no item, no status, and no way to tell whether the
//! work shipped, was reverted, or never existed.
//!
//! This happened live: a session coined `#orphandrain` for a feature it built and
//! then reverted, and `#superviserrsilent` for code that shipped. Neither was
//! ever in the document. The existing "recommendation-like items" guard is too
//! vague to catch it — it says "consider adding pending items" and was skimmed
//! past twice in the same session. Naming the exact coined id is what makes the
//! correction actionable.
//!
//! Pure by design: the caller supplies the response text and the document's known
//! id universe, so every rule is testable without a document or a repo.

use std::collections::BTreeSet;

/// Extract `#id`-shaped tags from response text.
///
/// Deliberately narrow. A tag is `#` followed by an identifier of at least two
/// characters made of lowercase alphanumerics, `-`, or `_`. Uppercase-leading
/// tokens (`#TODO`), pure numbers (`#123`, which is issue/PR syntax), and
/// markdown headings (`# Title`, `## Section`) are not tracked-work ids.
pub fn extract_tags(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != '#' {
            i += 1;
            continue;
        }
        // A markdown heading marker is `#`/`##` at line start followed by space.
        // Also skip `##`-runs so `## Section` never yields a tag.
        let at_line_start = i == 0 || bytes[i - 1] == '\n';
        let mut j = i + 1;
        if at_line_start {
            while j < bytes.len() && bytes[j] == '#' {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] == ' ' || bytes[j] == '\t' {
                i = j;
                continue;
            }
        }
        // A tag must not be glued to the end of a preceding word (`foo#bar`).
        if !at_line_start
            && i > 0
            && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_' || bytes[i - 1] == '-')
        {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_lowercase()
                || bytes[end].is_ascii_digit()
                || bytes[end] == '-'
                || bytes[end] == '_')
        {
            end += 1;
        }
        let tag: String = bytes[start..end].iter().collect();
        // Require >=2 chars and at least one letter, so `#1`, `#12`, and `#-`
        // (issue references and punctuation) are not treated as work ids.
        if tag.len() >= 2 && tag.chars().any(|c| c.is_ascii_lowercase()) {
            out.insert(tag);
        }
        i = end.max(i + 1);
    }
    out
}

/// Tags the response coined that no known id accounts for.
///
/// `known_ids` is the document's id universe (backlog, queue, done, review) plus
/// any tag already present in the repository. Anything left is an identifier this
/// turn invented with nothing behind it.
pub fn coined_ids(response_text: &str, known_ids: &BTreeSet<String>) -> Vec<String> {
    extract_tags(response_text)
        .into_iter()
        .filter(|tag| !known_ids.contains(tag))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    /// The live failure: a turn coins a tag for work it did, and nothing records it.
    #[test]
    fn a_coined_tag_with_no_known_id_is_reported() {
        let response = "Fixed it. `#orphandrain` — controller-side drain for documents.";
        assert_eq!(
            coined_ids(response, &known(&["fr79", "c8tb"])),
            vec!["orphandrain".to_string()]
        );
    }

    /// Referencing tracked work must stay silent, or the guard is noise.
    #[test]
    fn known_ids_are_not_reported() {
        let response = "Closing #fr79 and leaving #c8tb queued.";
        assert!(coined_ids(response, &known(&["fr79", "c8tb"])).is_empty());
    }

    /// Markdown headings are not tags. `### Re: topic` opens every response, so
    /// treating those as ids would fire on literally every turn.
    #[test]
    fn markdown_headings_are_not_tags() {
        let response = "### Re: some topic — opus-4-8\n\n# Title\n## Section\n\nBody.";
        assert!(coined_ids(response, &known(&[])).is_empty());
    }

    /// Issue/PR references (`#123`) are a different namespace.
    #[test]
    fn numeric_issue_references_are_not_work_ids() {
        assert!(coined_ids("See #123 and #4567.", &known(&[])).is_empty());
    }

    /// A tag glued to a word (`color#fff`, `sha#abc`) is not a tracked id.
    #[test]
    fn tags_glued_to_a_preceding_word_are_ignored() {
        assert!(coined_ids("value color#fff here", &known(&[])).is_empty());
    }

    /// Tags appear inside prose, backticks, and parentheses; all must be caught,
    /// since a coined id in a code comment is exactly the durable case.
    #[test]
    fn tags_are_found_in_prose_backticks_and_parens() {
        let response =
            "Landed (`#qstrikechurn`) and also #qbulletlesshead, plus `#superviserrsilent`.";
        let mut found = coined_ids(response, &known(&[]));
        found.sort();
        assert_eq!(
            found,
            vec![
                "qbulletlesshead".to_string(),
                "qstrikechurn".to_string(),
                "superviserrsilent".to_string()
            ]
        );
    }

    /// Underscores and digits are legal in ids (`#22a8`, `#spb1`, `#kp5z`).
    #[test]
    fn ids_may_contain_digits_hyphens_and_underscores() {
        let mut found = coined_ids("#22a8 #drain-no-defer #kp5z #some_id", &known(&[]));
        found.sort();
        assert_eq!(
            found,
            vec![
                "22a8".to_string(),
                "drain-no-defer".to_string(),
                "kp5z".to_string(),
                "some_id".to_string()
            ]
        );
    }

    /// The same coined tag repeated must be reported once, not once per mention.
    #[test]
    fn a_repeated_coined_tag_is_reported_once() {
        let response = "#orphandrain here, #orphandrain again, and #orphandrain once more.";
        assert_eq!(
            coined_ids(response, &known(&[])),
            vec!["orphandrain".to_string()]
        );
    }

    /// A lone `#` and a trailing `#` must not panic or yield an empty id.
    #[test]
    fn bare_hash_characters_are_ignored() {
        assert!(coined_ids("a # b #", &known(&[])).is_empty());
        assert!(coined_ids("#", &known(&[])).is_empty());
    }
}
