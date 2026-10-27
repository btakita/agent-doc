//! The JetBrains patch watcher's exchange prompt-prefix repair, behind the
//! hot-reloaded native boundary (`#jbshrinkrestarts`, the GH #326 lever).
//!
//! When the binary sends an editor patch with `normalize_prefix_lines`, the
//! plugin's `PatchWatcher` adds the `❯ ` prompt prefix to those operator lines
//! in the exchange's operator region (before the last boundary marker), skipping
//! lines that sit inside a response block until a line that looks like a fresh
//! prompt ends that block. Which lines look like a prompt, a response label, or
//! plain response prose is a heuristic word list, and every change to it was a
//! plugin generation, an IDE restart on the exact-263 modular package.
//!
//! The rule lives here now, as the `exchange_prefix_repair` derivation. It is a
//! byte-for-byte port of the pre-move Kotlin `normalizeExchangePrefixesUtil`,
//! not the binary's `agent_doc_element_exchange::normalize_exchange_prefixes_for_targets`:
//! the two differ (the plugin's repairs every occurrence of a target and does
//! not track prefixed response headings), and changing an editor's output is a
//! deliberate act. Converging them is now a native-only change. The port keeps
//! `java.util.regex`'s ASCII `\s`/`\d` and Kotlin's `trim` whitespace
//! (`Char.isWhitespace`), and splits lines at `\n` only, as the Kotlin did.
//!
//! The plugin keeps the Kotlin as a frozen floor for a library that cannot
//! answer. One parity fixture
//! (`editors/jetbrains/src/test/resources/editor-derivations/exchange_prefix_repair.json`)
//! is answered by both.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The document a patch produced and the prefix targets the patch names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangePrefixRepairEvidence {
    #[serde(default)]
    pub document: Option<String>,
    #[serde(default)]
    pub prefix_lines: Vec<String>,
}

/// The `exchange_prefix_repair` answer. `document` is present exactly when the
/// repair changed it, so an unchanged document does not cross back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangePrefixRepair {
    pub changed: bool,
    pub document: Option<String>,
}

const PROMPT_PREFIX: &str = "❯ ";
const CLOSE_TAG: &str = "<!-- /agent:exchange -->";

static OPEN_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!-- agent:exchange(?:[ \t\n\x0B\x0C\r][^>]*)? -->").unwrap());
static BOUNDARY_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->").unwrap());
static WHOLE_BOUNDARY_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A<!-- agent:boundary:[a-z0-9][a-z0-9:-]* -->\z").unwrap());
static PREFIXED_LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A❯[ \t\n\x0B\x0C\r]+(?:[-*+]|[0-9]+[.)])[ \t\n\x0B\x0C\r]+").unwrap()
});
static LIST_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?:[-*+]|[0-9]+[.)])[ \t\n\x0B\x0C\r]+").unwrap());

/// Kotlin's `Char.isWhitespace` on the JVM (`Character.isWhitespace(c) ||
/// Character.isSpaceChar(c)`); see `route_outcome`.
fn kotlin_is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | '\u{1C}'..='\u{1F}'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

fn trim(value: &str) -> &str {
    value.trim_matches(kotlin_is_whitespace)
}

fn trim_start(value: &str) -> &str {
    value.trim_start_matches(kotlin_is_whitespace)
}

fn trim_end(value: &str) -> &str {
    value.trim_end_matches(kotlin_is_whitespace)
}

fn is_blank(value: &str) -> bool {
    value.chars().all(kotlin_is_whitespace)
}

fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

const COMMAND_PROMPT_STARTS: &[&str] = &[
    "do #",
    "do [#",
    "fix #",
    "run ",
    "rerun ",
    "build ",
    "test ",
    "commit ",
    "push ",
    "verify ",
    "investigate ",
];

fn starts_prompt_run_after_response(trimmed: &str, is_target: bool) -> bool {
    let already_prefixed = trimmed.starts_with(PROMPT_PREFIX);
    if !already_prefixed && !is_target {
        return false;
    }
    let unprefixed = if already_prefixed {
        trim_start(&trimmed[PROMPT_PREFIX.len()..])
    } else {
        trimmed
    };
    if line_looks_like_plain_response_after_prompt(unprefixed) {
        return false;
    }
    // A blockquote inside an assistant response is quoted context, not a
    // fresh prompt transition.
    if unprefixed.starts_with('>') {
        return false;
    }
    if already_prefixed && !PREFIXED_LIST_ITEM.is_match(trimmed) {
        return true;
    }
    let lower = unprefixed.to_lowercase();
    lower.starts_with("please ")
        || lower.contains(" please ")
        || lower.starts_with("can you ")
        || lower.starts_with("could you ")
        || lower.starts_with("would you ")
        || lower.starts_with("need you to ")
        || lower == "go"
        || lower == "continue"
        || lower.starts_with('/')
        || COMMAND_PROMPT_STARTS
            .iter()
            .any(|start| lower.starts_with(start))
}

fn line_looks_like_fresh_prompt_after_response(trimmed: &str) -> bool {
    let lower = trim(trimmed.strip_prefix('❯').unwrap_or(trimmed)).to_lowercase();
    trimmed.starts_with('❯')
        || trimmed.ends_with('?')
        || lower == "go"
        || lower == "continue"
        || COMMAND_PROMPT_STARTS
            .iter()
            .any(|start| lower.starts_with(start))
}

fn line_looks_like_plain_response_after_prompt(trimmed: &str) -> bool {
    if is_blank(trimmed) {
        return false;
    }
    if line_has_known_response_label(trimmed) {
        return true;
    }
    if line_looks_like_fresh_prompt_after_response(trimmed) {
        return false;
    }
    if [
        "- ",
        "* ",
        "Plan:",
        "Verification",
        "What changed:",
        "Follow-up:",
        "Commit / push:",
        "Backlog:",
        "`#",
    ]
    .iter()
    .any(|start| trimmed.starts_with(start))
    {
        return true;
    }
    let lower = trimmed.to_lowercase();
    [
        "i updated ",
        "i fixed ",
        "i added ",
        "i implemented ",
        "i left ",
        "updated ",
        "fixed ",
        "added ",
        "implemented ",
    ]
    .iter()
    .any(|start| lower.starts_with(start))
}

fn line_has_known_response_label(line: &str) -> bool {
    let Some(normalized) = normalize_response_label_candidate(line) else {
        return false;
    };
    [
        "Plan:",
        "Verification:",
        "What changed:",
        "Follow-up:",
        "Commit / push:",
        "Backlog:",
    ]
    .iter()
    .any(|label| normalized.starts_with(label))
}

fn normalize_response_label_candidate(line: &str) -> Option<String> {
    let mut trimmed = trim(line);
    if is_blank(trimmed) {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = trim_start(rest);
    }
    if let Some(marker) = LIST_MARKER.find(trimmed) {
        trimmed = trim_start(&trimmed[marker.end()..]);
    }
    let stripped = strip_markdown_emphasis_pair(trimmed);
    let candidate = trim_start(&stripped);
    (!is_blank(candidate)).then(|| candidate.to_string())
}

fn strip_markdown_emphasis_pair(text: &str) -> String {
    for marker in ["***", "___", "**", "__", "*", "_"] {
        let Some(rest) = text.strip_prefix(marker) else {
            continue;
        };
        let Some(end) = rest.find(marker) else {
            continue;
        };
        let label = &rest[..end];
        let tail = &rest[end + marker.len()..];
        if !is_blank(label) && (trim_end(label).ends_with(':') || trim_start(tail).starts_with(':'))
        {
            return format!("{label}{tail}");
        }
    }
    text.to_string()
}

/// Add the `❯ ` prefix to the target operator lines of the exchange's
/// operator region (before its last boundary marker).
pub fn repair_exchange_prefixes(document: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return document.to_string();
    }
    let Some(open) = OPEN_TAG.find(document) else {
        return document.to_string();
    };
    let Some(close) = document[open.end()..]
        .find(CLOSE_TAG)
        .map(|index| open.end() + index)
    else {
        return document.to_string();
    };
    let before_exchange = &document[..open.end()];
    let exchange_content = &document[open.end()..close];
    let after_exchange = &document[close..];

    // Only the operator region before the LAST boundary marker: historical
    // cycles each leave a marker.
    let user_region_end = BOUNDARY_TAG
        .find_iter(exchange_content)
        .last()
        .map_or(exchange_content.len(), |boundary| boundary.start());
    let user_region = &exchange_content[..user_region_end];
    let agent_region = &exchange_content[user_region_end..];

    let targets: HashSet<&str> = prefix_lines
        .iter()
        .filter(|line| !is_blank(line))
        .map(|line| trim_end(line))
        .collect();

    let mut in_response_block = false;
    let normalized = user_region
        .split('\n')
        .map(|line| {
            let trimmed = trim(line);
            if WHOLE_BOUNDARY_TAG.is_match(trimmed) {
                in_response_block = false;
                return line.to_string();
            }
            if is_exchange_response_heading(trimmed) {
                in_response_block = true;
                return line.to_string();
            }
            let normalized = trim_end(line);
            let is_target = targets.contains(normalized);
            if in_response_block {
                if starts_prompt_run_after_response(trimmed, is_target) {
                    in_response_block = false;
                } else {
                    return line.to_string();
                }
            }
            if normalized.starts_with(PROMPT_PREFIX) {
                line.to_string()
            } else if is_target {
                format!("{PROMPT_PREFIX}{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{before_exchange}{normalized}{agent_region}{after_exchange}")
}

/// Answer one `exchange_prefix_repair` request.
pub fn derive_exchange_prefix_repair(
    evidence: &ExchangePrefixRepairEvidence,
) -> ExchangePrefixRepair {
    let document = evidence.document.as_deref().unwrap_or("");
    let repaired = repair_exchange_prefixes(document, &evidence.prefix_lines);
    if repaired == document {
        ExchangePrefixRepair {
            changed: false,
            document: None,
        }
    } else {
        ExchangePrefixRepair {
            changed: true,
            document: Some(repaired),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repair(document: &str, targets: &[&str]) -> String {
        let targets: Vec<String> = targets.iter().map(|line| line.to_string()).collect();
        repair_exchange_prefixes(document, &targets)
    }

    #[test]
    fn only_the_region_before_the_last_boundary_is_repaired() {
        let document = "<!-- agent:exchange -->\nfix it\n<!-- agent:boundary:a1 -->\nfix it\n<!-- agent:boundary:b2 -->\nfix it\n<!-- /agent:exchange -->\n";
        assert_eq!(
            repair(document, &["fix it"]),
            "<!-- agent:exchange -->\n❯ fix it\n<!-- agent:boundary:a1 -->\n❯ fix it\n<!-- agent:boundary:b2 -->\nfix it\n<!-- /agent:exchange -->\n"
        );
    }

    #[test]
    fn a_response_block_holds_until_a_fresh_prompt() {
        let document = "<!-- agent:exchange -->\n### Re: x\nfixed the bug\nplease run tests\n<!-- /agent:exchange -->";
        assert_eq!(
            repair(document, &["fixed the bug", "please run tests"]),
            "<!-- agent:exchange -->\n### Re: x\nfixed the bug\n❯ please run tests\n<!-- /agent:exchange -->"
        );
    }

    #[test]
    fn emphasis_labels_and_list_markers_are_response_labels() {
        assert!(line_has_known_response_label("❯ 1. **Plan:** do it"));
        assert!(line_has_known_response_label("- __Backlog__: none"));
        assert!(!line_has_known_response_label("**Plan** later"));
    }

    #[test]
    fn unchanged_documents_do_not_cross_back() {
        let answer = derive_exchange_prefix_repair(&ExchangePrefixRepairEvidence {
            document: Some("no exchange here".to_string()),
            prefix_lines: vec!["x".to_string()],
        });
        assert_eq!(
            answer,
            ExchangePrefixRepair {
                changed: false,
                document: None
            }
        );
    }
}
