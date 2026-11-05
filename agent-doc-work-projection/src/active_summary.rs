//! The one-line summary an `agent:active` entry shows after its `` `[#id]` — `` prefix
//! (`#activesummaryurl`).
//!
//! The summary is the source text's first line, bounded to [`MAX_SUMMARY_CHARS`]. Two rules
//! keep a bare URL in it intact and keep the row soft-wrapping as one row:
//!
//! - The bound never cuts a bare URL: a cut that falls inside one moves to the URL's end, so
//!   the row never links to a truncated address such as `…/merge_reque…`.
//! - Every bare `http://` / `https://` URL is written as a CommonMark autolink `<url>`.
//!
//! The second rule works around IntelliJ's Markdown soft wrap (2026.2 and later,
//! `MarkdownLineWrapPositionStrategy.classifyForbidden`). When the best wrap point sits in
//! front of a GFM autolink, the strategy moves the wrap in front of the nearest `[` earlier on
//! the line, as if that `[` opened a `[label](url)` link, without checking that a `]` already
//! closed it. Every active row starts with its id's `[`, so a row whose summary has a bare URL
//! past the wrap width wrapped after `- `, then after the backtick, then mid-URL. Behind `<`
//! the best wrap point is an `<` token, which the strategy does not move, so the row wraps at
//! the space before the URL.

/// Characters of the source's first line an active summary keeps before it is elided.
pub(crate) const MAX_SUMMARY_CHARS: usize = 100;

/// Summarize `text` for an `agent:active` row: its first line, bounded, with bare URLs kept
/// whole and written as autolinks.
pub(crate) fn summarize_text(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let chars: Vec<char> = first.chars().collect();
    let mut cut = chars.len().min(MAX_SUMMARY_CHARS);
    if cut < chars.len() {
        for (start, end) in bare_url_spans(&chars) {
            if start < cut && cut < end {
                cut = end;
                break;
            }
        }
    }
    let bounded: String = chars[..cut].iter().collect();
    let delimited = delimit_bare_urls(&bounded);
    if cut < chars.len() {
        format!("{delimited}…")
    } else {
        delimited
    }
}

/// Write each bare URL in `text` as `<url>`, leaving trailing sentence punctuation outside.
fn delimit_bare_urls(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + 8);
    let mut next = 0;
    for (start, end) in bare_url_spans(&chars) {
        out.extend(&chars[next..start]);
        out.push('<');
        out.extend(&chars[start..end]);
        out.push('>');
        next = end;
    }
    out.extend(&chars[next..]);
    out
}

/// Char ranges of the bare URLs in `chars`: a whitespace-delimited token that starts with
/// `http://` or `https://` at the line start or after whitespace, outside a code span, less
/// any trailing punctuation GFM leaves out of an autolink. A token already inside `<…>`, a
/// link destination, or a code span is not bare, and neither is one holding `<`, `>` or a
/// backtick.
fn bare_url_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut in_code = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            in_code = !in_code;
            i += 1;
            continue;
        }
        let at_token_start = i == 0 || chars[i - 1].is_whitespace();
        if in_code || !at_token_start || !starts_with_scheme(&chars[i..]) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        let token = &chars[i..end];
        if !token.iter().any(|c| matches!(c, '<' | '>' | '`')) {
            let url_end = i + trimmed_url_len(token);
            if url_end > i + scheme_len(token) {
                spans.push((i, url_end));
            }
        }
        i = end;
    }
    spans
}

fn scheme_len(token: &[char]) -> usize {
    if token.starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/']) {
        8
    } else if token.starts_with(&['h', 't', 't', 'p', ':', '/', '/']) {
        7
    } else {
        0
    }
}

fn starts_with_scheme(chars: &[char]) -> bool {
    scheme_len(chars) > 0
}

/// Length of `token` once trailing punctuation is dropped, as GFM's extended autolink does:
/// `?`, `!`, `.`, `,`, `:`, `*`, `_`, `~`, quotes, `;`, the ellipsis, and a `)` the URL does
/// not open.
fn trimmed_url_len(token: &[char]) -> usize {
    let mut len = token.len();
    loop {
        let Some(&last) = token[..len].last() else {
            return len;
        };
        let drop = match last {
            '?' | '!' | '.' | ',' | ':' | ';' | '*' | '_' | '~' | '\'' | '"' | '…' => true,
            ')' => {
                let opens = token[..len].iter().filter(|c| **c == '(').count();
                let closes = token[..len].iter().filter(|c| **c == ')').count();
                closes > opens
            }
            _ => false,
        };
        if !drop {
            return len;
        }
        len -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MR: &str = "https://git.example.com/investment/tools/pmt2/-/merge_requests/1234";
    const LONG_MR: &str = "https://git.example.com/investment/pmtools/pmt2/-/merge_requests/1234/diffs?commit_id=0123abcd";

    #[test]
    fn short_plain_text_is_unchanged() {
        assert_eq!(
            summarize_text("  fix the parser  \nsecond line"),
            "fix the parser"
        );
    }

    #[test]
    fn long_plain_text_is_bounded_and_elided() {
        let text = "x".repeat(150);
        assert_eq!(summarize_text(&text), format!("{}…", "x".repeat(100)));
    }

    #[test]
    fn bare_url_becomes_an_autolink() {
        assert_eq!(
            summarize_text(&format!("Since we changed direction to {MR}.")),
            format!("Since we changed direction to <{MR}>.")
        );
    }

    #[test]
    fn a_cut_inside_a_url_moves_to_the_url_end() {
        // The operator's row: the 100-char bound used to land inside the merge request URL,
        // leaving a link to `…/merge_reque` plus the ellipsis.
        let prefix = "Since we changed direction to ";
        let text = format!("{prefix}{LONG_MR} we should rebase the rest of the stack");
        assert!(prefix.len() < MAX_SUMMARY_CHARS);
        assert!(prefix.len() + LONG_MR.len() > MAX_SUMMARY_CHARS);
        assert_eq!(summarize_text(&text), format!("{prefix}<{LONG_MR}>…"));
    }

    #[test]
    fn a_url_that_ends_the_text_gets_no_ellipsis() {
        let text = format!("Since we changed direction to the merge request at {MR}");
        assert!(text.chars().count() > MAX_SUMMARY_CHARS);
        assert!(text.len() - MR.len() < MAX_SUMMARY_CHARS);
        assert_eq!(
            summarize_text(&text),
            format!("Since we changed direction to the merge request at <{MR}>")
        );
    }

    #[test]
    fn urls_that_are_not_bare_are_left_alone() {
        for text in [
            format!("see <{MR}> first"),
            format!("see [the MR]({MR}) first"),
            format!("see `{MR}` first"),
            format!("see x{MR} first"),
            "see https:// alone".to_string(),
        ] {
            assert_eq!(summarize_text(&text), text, "{text}");
        }
    }

    #[test]
    fn trailing_punctuation_and_unopened_paren_stay_outside() {
        assert_eq!(
            summarize_text("(see https://example.com/a_(b)) and https://example.com/x?, ok"),
            "(see <https://example.com/a_(b)>) and <https://example.com/x>?, ok"
        );
    }

    #[test]
    fn rendering_is_idempotent() {
        let once = summarize_text(&format!("Since we changed direction to {MR}"));
        assert_eq!(summarize_text(&once), once);
    }
}
