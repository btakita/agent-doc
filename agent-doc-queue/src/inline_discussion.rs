//! Inline pull-request discussion comments (`#addressinline`).
//!
//! `#address` and `#adversarial-pr` load every root inline discussion comment
//! (`gh api repos/<owner>/<repo>/pulls/<number>/comments`; html_url contains
//! `#discussion_r`), not only the review body. A later reply does not load the
//! root comment. The run is not complete while a root thread's written fix is
//! absent from the exact current head, and it does not dismiss a thread.
//!
//! The hand-back names each loaded root thread:
//!
//! ```text
//! THREAD-FIX-ON-HEAD <discussion-url> head=<sha> resolved
//! ```
//!
//! When the comments query returns no root comments:
//!
//! ```text
//! INLINE-THREADS-LOADED 0 head=<sha>
//! ```

use agent_doc_document::queue_projection::strip_priority_markers;

/// Binding closeout for `#address` and `#adversarial-pr` (`#addressinline`).
pub const INLINE_DISCUSSION_RULE: &str = "Inline discussion comments (#addressinline): #address and #adversarial-pr load every inline pull-request discussion comment, not only the review body. Before review or address, query `gh api repos/<owner>/<repo>/pulls/<number>/comments`. A root comment is one with no in_reply_to_id; its html_url contains `#discussion_r`. A `#pullrequestreview` body, an issue comment, and a later reply are not that root comment. Do not dismiss a thread. The run is not complete until every root thread's written fix is the code on the exact current head and that thread is addressed and resolved. The hand-back names each root thread on its own line: `THREAD-FIX-ON-HEAD <discussion-url> head=<sha> resolved`. Emit that line only when that head's code is the written fix and the thread was loaded, addressed, and resolved. When the comments query returns no root comments, the hand-back says `INLINE-THREADS-LOADED 0 head=<sha>` and names the query. A later reply on a thread does not stand in for loading the root comment.";

const GOAL_NAMES: &[&str] = &["#address", "#adversarial-pr"];

const UNLOADED_OR_ABSENT: &[&str] = &[
    "did not load",
    "never loaded",
    "was not loaded",
    "written fix is absent",
    "written fix is not on",
    "fix is not on",
    "does not have that fix",
    "do not have that fix",
    "dismissed the thread",
    "thread dismissed",
];

/// True when `name` is `#address` or `#adversarial-pr`.
pub fn preset_name_loads_inline_discussions(name: &str) -> bool {
    GOAL_NAMES
        .iter()
        .any(|goal| name.eq_ignore_ascii_case(goal))
}

/// True when `item` carries `#address` or `#adversarial-pr` as its own token.
pub fn item_mentions_inline_discussion_goal(item: &str) -> bool {
    let text = strip_priority_markers(item);
    let text = crate::queue_continuation::mask_inline_code(&text);
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .any(preset_name_loads_inline_discussions)
}

/// True when `head` is an `#address` or `#adversarial-pr` run under `presets`.
pub fn head_requires_inline_discussions(head: &str, presets: &[(String, String)]) -> bool {
    if item_mentions_inline_discussion_goal(head) {
        return true;
    }
    crate::queue_response::queue_prompt_preset_expansions_from(
        head,
        presets.iter().map(|(name, body)| (name, body)),
    )
    .iter()
    .any(|(name, _)| preset_name_loads_inline_discussions(name))
}

/// True when `response` shows every named root thread's written fix on a head,
/// or shows that the comments query returned no root comments.
///
/// Mentioning a `#discussion_r` URL, including to say a later reply resolved
/// it, does not load that thread. A receipt is required for every such URL.
/// Admitting the thread was not loaded, that its written fix is absent, or
/// that the thread was dismissed keeps the run incomplete.
pub fn inline_threads_settled(response: &str) -> bool {
    if admits_unloaded_or_absent(response) {
        return false;
    }
    let receipts = thread_fix_receipts(response);
    let mentioned = discussion_ids(response);
    if inline_threads_loaded_zero(response) {
        return receipts.is_empty() && mentioned.is_empty();
    }
    if receipts.is_empty() {
        return false;
    }
    mentioned
        .iter()
        .all(|id| receipts.iter().any(|receipt| &receipt.id == id))
}

fn admits_unloaded_or_absent(response: &str) -> bool {
    let prose = collapsed_prose(response);
    UNLOADED_OR_ABSENT
        .iter()
        .any(|phrase| prose.contains(phrase))
}

fn collapsed_prose(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

struct ThreadReceipt {
    id: String,
}

fn thread_fix_receipts(response: &str) -> Vec<ThreadReceipt> {
    response.lines().filter_map(line_thread_receipt).collect()
}

fn inline_threads_loaded_zero(response: &str) -> bool {
    response.lines().any(line_loaded_zero)
}

fn line_thread_receipt(line: &str) -> Option<ThreadReceipt> {
    let rest = line_payload(line);
    let rest = rest.strip_prefix("THREAD-FIX-ON-HEAD")?;
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let url = parts.next()?;
    let head = parts.next()?;
    let resolved = parts.next()?;
    let sha = head.strip_prefix("head=")?;
    if !is_sha(sha) || !token_word(resolved).eq_ignore_ascii_case("resolved") {
        return None;
    }
    let mut ids = discussion_ids(url);
    if ids.len() != 1 {
        return None;
    }
    Some(ThreadReceipt { id: ids.remove(0) })
}

fn line_loaded_zero(line: &str) -> bool {
    let rest = line_payload(line);
    let Some(rest) = rest.strip_prefix("INLINE-THREADS-LOADED") else {
        return false;
    };
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return false;
    }
    let mut parts = rest.split_whitespace();
    let Some(count) = parts.next() else {
        return false;
    };
    let Some(head) = parts.next() else {
        return false;
    };
    count == "0" && head.strip_prefix("head=").is_some_and(is_sha)
}

fn discussion_ids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let needle = b"discussion_r";
    let mut ids = Vec::new();
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if bytes[index..].starts_with(needle) {
            let mut end = index + needle.len();
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > index + needle.len() {
                let id = text[index..end].to_string();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            index = end;
        } else {
            index += 1;
        }
    }
    ids
}

fn is_sha(sha: &str) -> bool {
    let sha = sha.trim_end_matches([',', '.', ';', ')', '`']);
    (7..=40).contains(&sha.len()) && sha.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn token_word(token: &str) -> &str {
    token.trim_matches(|ch: char| matches!(ch, ',' | '.' | ';' | ':' | ')' | '(' | '`' | '*' | '_'))
}

fn line_payload(line: &str) -> &str {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        rest = rest.trim_start_matches(['*', '_', '`']);
        if rest == before {
            break;
        }
    }
    rest
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUST: &str = "https://github.com/haiven-dev/haiven-api/pull/869#discussion_r4532159601";
    const SHOULD: &str = "https://github.com/haiven-dev/haiven-api/pull/869#discussion_r4532069052";
    const HEAD: &str = "87afcacde5eaed85bc87372fad897903afdebacf";

    fn presets() -> Vec<(String, String)> {
        vec![
            (
                "#address".to_string(),
                "Address every actionable review item on the exact current head.".to_string(),
            ),
            (
                "#adversarial-pr".to_string(),
                "/goal Bring the pull request to a review-clean head. Start a fixer using #address."
                    .to_string(),
            ),
        ]
    }

    fn both_receipts() -> String {
        format!(
            "THREAD-FIX-ON-HEAD {MUST} head={HEAD} resolved\n\
             THREAD-FIX-ON-HEAD {SHOULD} head={HEAD} resolved\n"
        )
    }

    #[test]
    fn the_two_goals_load_inline_threads_and_a_review_goal_does_not() {
        let presets = presets();
        assert!(head_requires_inline_discussions(
            "https://github.com/haiven-dev/haiven-api/pull/869: #address",
            &presets,
        ));
        assert!(head_requires_inline_discussions(
            "#adversarial-pr",
            &presets
        ));
        assert!(!head_requires_inline_discussions(
            "https://github.com/haiven-dev/haiven-api/pull/869: #review",
            &presets,
        ));
        assert!(item_mentions_inline_discussion_goal("#address [#q-1]"));
        assert!(!item_mentions_inline_discussion_goal("#review"));
    }

    #[test]
    fn a_review_body_or_a_later_reply_does_not_load_the_threads() {
        let review_body = format!(
            "REVIEW-CLEAN https://github.com/haiven-dev/haiven-api/pull/869#pullrequestreview-5837752992 \
             head={HEAD} cycles=1\n\nThe review body is clean."
        );
        assert!(!inline_threads_settled(&review_body));
        let later = format!(
            "Later replies on {MUST} and {SHOULD} are marked resolved, so the threads were not reopened."
        );
        assert!(!inline_threads_settled(&later));
        assert!(discussion_ids(&later).len() == 2);
    }

    #[test]
    fn one_receipt_does_not_cover_a_second_named_thread() {
        let response = format!(
            "THREAD-FIX-ON-HEAD {MUST} head={HEAD} resolved\n\
             {SHOULD} still has a later reply.\n"
        );
        assert!(!inline_threads_settled(&response));
    }

    #[test]
    fn both_root_threads_on_the_head_settle_and_a_dismissal_does_not() {
        assert!(inline_threads_settled(&both_receipts()));
        let wording = format!("{}\nThe hung-FPE thread was dismissed.", both_receipts());
        assert!(
            inline_threads_settled(&wording),
            "\"was dismissed\" is not the admission phrase"
        );
        let explicit = format!("{}\nI dismissed the thread.", both_receipts());
        assert!(!inline_threads_settled(&explicit));
        let quoted_rule = format!("{}\n{}", INLINE_DISCUSSION_RULE, both_receipts());
        assert!(
            inline_threads_settled(&quoted_rule),
            "the binding rule is not an admission"
        );
        let absent = format!(
            "{}\nThe migration written fix is not on the current head.",
            both_receipts()
        );
        assert!(!inline_threads_settled(&absent));
    }

    #[test]
    fn an_empty_comments_query_settles_only_when_no_thread_is_named() {
        let zero = format!("INLINE-THREADS-LOADED 0 head={HEAD}\nQueried pulls/869/comments.");
        assert!(inline_threads_settled(&zero));
        let contradicted = format!("{zero}\nSee {MUST}.");
        assert!(!inline_threads_settled(&contradicted));
        assert!(!inline_threads_settled(
            "INLINE-THREADS-LOADED 2 head=bbbb222"
        ));
    }
}
