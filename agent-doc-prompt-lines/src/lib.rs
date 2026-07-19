//! Pure line-level prompt/response classifiers shared across agent-doc crates.

const IMPERATIVE_LEADING_VERBS: &[&str] = &[
    "add",
    "audit",
    "benchmark",
    "build",
    "check",
    "clean",
    "close",
    "commit",
    "document",
    "fix",
    "harden",
    "implement",
    "install",
    "investigate",
    "note",
    "preserve",
    "push",
    "record",
    "refactor",
    "remove",
    "repair",
    "rerun",
    "revisit",
    "run",
    "test",
    "trace",
    "update",
    "verify",
    "write",
];

/// Approval words (case-insensitive after caller normalization).
pub const APPROVAL_WORDS: &[&str] = &[
    "go", "yes", "do", "ok", "continue", "approve", "approved", "y", "yep", "yeah", "sure",
    "proceed", "lgtm",
];

pub fn parse_slash_command_candidate(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix('/')?;
    let token_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let token = &rest[..token_end];
    if token.is_empty() {
        return None;
    }
    let mut chars = token.chars();
    let first = chars.next()?;
    let command_like = first.is_ascii_lowercase()
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, ':' | '_' | '-'));
    command_like.then(|| trimmed.to_string())
}

pub fn line_looks_like_slash_command(line: &str) -> bool {
    parse_slash_command_candidate(line).is_some()
}

fn line_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    let normalized_imperative = normalize_imperative_candidate(trimmed)
        .is_some_and(|normalized| looks_like_imperative_directive(&normalized));
    let slash_command = line_looks_like_slash_command(trimmed);
    let prompt_prefixed = trimmed.starts_with('❯') && !line_looks_like_markdown_list_item(trimmed);
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !line_has_known_response_label_after_prompt(trimmed)
        && (slash_command || prompt_prefixed || trimmed.ends_with('?') || normalized_imperative)
}

pub fn text_line_looks_like_prompt_target(line: &str) -> bool {
    line_looks_like_prompt_target(line)
}

pub fn normalized_prompt_preview_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !text_line_looks_like_prompt_target(trimmed) {
        return None;
    }
    Some(trimmed.trim_start_matches('❯').trim().to_string())
}

pub fn line_looks_like_fresh_prompt_after_response(trimmed: &str) -> bool {
    if line_looks_like_plain_response_after_prompt(trimmed) {
        return false;
    }

    let unprefixed = trimmed.trim_start_matches('❯').trim();
    let lower = unprefixed.to_ascii_lowercase();
    trimmed.starts_with('❯')
        || unprefixed.ends_with('?')
        || line_looks_like_slash_command(unprefixed)
        || lower == "go"
        || lower == "continue"
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

pub fn line_looks_like_soft_prompt_request(trimmed: &str) -> bool {
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("please ")
        || lower.contains(" please ")
        || lower.starts_with("can you ")
        || lower.starts_with("could you ")
        || lower.starts_with("would you ")
        || lower.starts_with("need you to ")
}

pub fn line_looks_like_prompt_prefix_repair_start(trimmed: &str, is_target: bool) -> bool {
    let unprefixed = strip_prompt_prefix(trimmed);

    if unprefixed.is_empty() || line_looks_like_plain_response_after_prompt(unprefixed) {
        return false;
    }

    is_target
        || line_looks_like_fresh_prompt_after_response(unprefixed)
        || line_looks_like_soft_prompt_request(unprefixed)
}

pub fn line_looks_like_targeted_prompt_prefix_repair_start(trimmed: &str, is_target: bool) -> bool {
    if !is_target {
        return false;
    }

    let unprefixed = strip_prompt_prefix(trimmed);

    if unprefixed.is_empty() || line_looks_like_plain_response_after_prompt(unprefixed) {
        return false;
    }

    if (trimmed.starts_with('❯') && !line_looks_like_markdown_list_item(trimmed))
        || line_looks_like_soft_prompt_request(unprefixed)
    {
        return true;
    }

    let lower = unprefixed.to_ascii_lowercase();
    lower == "go"
        || lower == "continue"
        || line_looks_like_slash_command(unprefixed)
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

pub fn line_looks_like_targeted_or_prefixed_prompt_repair_start(
    trimmed: &str,
    is_target: bool,
) -> bool {
    line_looks_like_targeted_prompt_prefix_repair_start(
        trimmed,
        is_target || trimmed.trim_start().starts_with('❯'),
    )
}

pub fn line_looks_like_plain_response_after_prompt(trimmed: &str) -> bool {
    if trimmed.is_empty() {
        return false;
    }

    if line_has_known_response_label_after_prompt(trimmed) {
        return true;
    }

    if normalized_prompt_preview_line(trimmed).is_some() {
        return false;
    }

    if line_looks_like_markdown_list_item(trimmed)
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("Plan:")
        || trimmed.starts_with("Verification")
        || trimmed.starts_with("What changed:")
        || trimmed.starts_with("Follow-up:")
        || trimmed.starts_with("Backlog:")
        || trimmed.starts_with("`#")
    {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("i updated ")
        || lower.starts_with("i fixed ")
        || lower.starts_with("i added ")
        || lower.starts_with("i implemented ")
        || lower.starts_with("i left ")
        || lower.starts_with("i dry-ran ")
        || lower.starts_with("recovered ")
        || lower.starts_with("confirmed ")
        || lower.starts_with("no code files changed")
        || lower.starts_with("this closeout ")
        || lower.starts_with("updated ")
        || lower.starts_with("fixed ")
        || lower.starts_with("added ")
        || lower.starts_with("implemented ")
}

fn line_has_known_response_label_after_prompt(line: &str) -> bool {
    let Some(normalized) = normalize_response_label_candidate(line) else {
        return false;
    };
    matches!(
        normalized.as_str(),
        s if s.starts_with("Plan:")
            || s.starts_with("Verification:")
            || s.starts_with("What changed:")
            || s.starts_with("Follow-up:")
            || s.starts_with("Commit / push:")
            || s.starts_with("Backlog:")
    )
}

fn normalize_response_label_candidate(line: &str) -> Option<String> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        trimmed = rest.trim_start();
    } else {
        let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            let rest = &trimmed[digits..];
            if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                trimmed = rest.trim_start();
            }
        }
    }

    let stripped = strip_markdown_emphasis_pair(trimmed);
    let normalized = stripped.trim_start();
    (!normalized.is_empty()).then(|| normalized.to_string())
}

fn strip_markdown_emphasis_pair(text: &str) -> String {
    for marker in ["***", "___", "**", "__", "*", "_"] {
        if let Some(rest) = text.strip_prefix(marker)
            && let Some(end) = rest.find(marker)
        {
            let label = &rest[..end];
            let tail = &rest[end + marker.len()..];
            if !label.trim().is_empty()
                && (label.trim_end().ends_with(':') || tail.trim_start().starts_with(':'))
            {
                return format!("{}{}", label, tail);
            }
        }
    }
    text.to_string()
}

pub fn looks_like_imperative_directive(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let compact = lower.trim();
    let exact = compact.trim_end_matches(|c: char| c.is_ascii_punctuation());
    if APPROVAL_WORDS.contains(&exact) {
        return true;
    }
    compact.starts_with("do #")
        || compact.starts_with("do [#")
        || compact.starts_with("fix #")
        || compact.starts_with("fix this")
        || compact.contains(" run tests")
        || compact.starts_with("run tests")
        || compact.contains(" run benchmarks")
        || compact.starts_with("run benchmarks")
        || compact.contains(" build + install")
        || compact.starts_with("build + install")
        || compact.contains(" build and install")
        || compact.starts_with("build and install")
        || compact.contains(" commit + push")
        || compact.starts_with("commit + push")
        || compact.contains(" commit and push")
        || compact.starts_with("commit and push")
        || starts_with_imperative_verb(compact)
}

pub fn normalize_imperative_candidate(line: &str) -> Option<String> {
    let mut trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("- ") {
        trimmed = rest.trim_start();
    }

    trimmed = strip_pending_checkbox_prefix(trimmed);

    if let Some(rest) = trimmed.strip_prefix("[#")
        && let Some(close) = rest.find(']')
    {
        let id = &rest[..close];
        if agent_doc_element_backlog::backlog::is_valid_pending_id(id) {
            trimmed = rest[close + 1..].trim_start();
        }
    }

    let trimmed = strip_politeness_prefix(trimmed.trim_start_matches('❯').trim_start());

    let normalized = trimmed
        .trim_end_matches(|c: char| c.is_ascii_punctuation() && c != ']')
        .trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

/// Strip a leading politeness marker so the imperative verb underneath is seen
/// (`#politeimperative`).
///
/// `looks_like_imperative_directive` keys off the FIRST word, so "Add a backup
/// command" classified as a directive while "Please add a backup command" did
/// not — the operator's politeness silently demoted their own instruction. In a
/// queue that meant a free-text head was never admitted as tracked backlog work,
/// and an agent had to hand-rewrite the line to `do [#id]` (observed live
/// 2026-07-18 on `equityfundingsource.md`). Politeness is not a mood change;
/// "Please do X" is exactly as imperative as "Do X".
///
/// Only leading markers are stripped, so a mid-sentence "please" is untouched.
fn strip_politeness_prefix(line: &str) -> &str {
    // Longest-first so "can you please" wins over "can you".
    const POLITENESS_PREFIXES: &[&str] = &[
        "could you please ",
        "would you please ",
        "can you please ",
        "could you ",
        "would you ",
        "can you ",
        "please ",
        "kindly ",
        "pls ",
    ];
    let mut current = line;
    // Loop so "Please can you fix X" also normalizes; bounded by the fact that
    // every iteration consumes at least one prefix's worth of bytes.
    loop {
        let lower = current.to_ascii_lowercase();
        let Some(prefix) = POLITENESS_PREFIXES
            .iter()
            .find(|prefix| lower.starts_with(**prefix))
        else {
            return current;
        };
        current = current[prefix.len()..].trim_start();
    }
}

pub fn strip_pending_checkbox_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prefix in ["[ ]", "[/]", "[x]", "[X]"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim_start();
        }
    }
    if let Some(inner) = trimmed.strip_prefix("[/")
        && let Some(close) = inner.find(']')
    {
        let gate_type = &inner[..close];
        if !gate_type.is_empty()
            && gate_type
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return inner[close + 1..].trim_start();
        }
    }
    trimmed
}

fn starts_with_imperative_verb(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if IMPERATIVE_LEADING_VERBS.contains(&first) {
        return true;
    }
    matches!((first, words.next()), ("clean", Some("up")))
}

pub fn parse_markdown_list_item(line: &str) -> Option<&str> {
    let trimmed = strip_prompt_prefix(line.trim_start());
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let rest = rest.trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            let rest = rest.trim();
            return (!rest.is_empty()).then_some(rest);
        }
    }

    None
}

pub fn line_looks_like_markdown_list_item(line: &str) -> bool {
    parse_markdown_list_item(line).is_some()
}

/// True when `trimmed` opens with a markdown list bullet. This intentionally
/// treats an empty bullet marker as a list item for directive-filtering paths.
pub fn trimmed_line_looks_like_markdown_list_item(trimmed: &str) -> bool {
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        return true;
    }
    if let Some(dot) = trimmed.find(". ") {
        let head = &trimmed[..dot];
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

fn strip_prompt_prefix(line: &str) -> &str {
    line.strip_prefix("❯ ")
        .or_else(|| line.strip_prefix('❯'))
        .map(str::trim_start)
        .unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#politeimperative`: a polite instruction is still an instruction. The
    /// live regression was a queue head that never became tracked backlog work
    /// because the operator wrote "Please add ..." instead of "Add ...".
    #[test]
    fn politeness_prefixes_do_not_demote_an_imperative() {
        let polite = "Please add a backup command to backup the TMO system. Test backup on the sandbox.";
        assert!(
            text_line_looks_like_prompt_target(polite),
            "a polite instruction must still classify as a prompt target"
        );
        assert!(text_line_looks_like_prompt_target(&format!("- {polite}")));

        for line in [
            "Please fix the flaky test",
            "pls run tests",
            "Kindly commit and push",
            "Can you add a retry?",
            "Could you please build the index",
            "Would you update the changelog",
        ] {
            assert!(
                text_line_looks_like_prompt_target(line),
                "must classify as a prompt target: {line}"
            );
        }

        // The politeness marker is stripped from the normalized directive text.
        assert_eq!(
            normalize_imperative_candidate("Please add a backup command").as_deref(),
            Some("add a backup command")
        );
        // Stacked markers collapse.
        assert_eq!(
            normalize_imperative_candidate("Please can you fix the guard").as_deref(),
            Some("fix the guard")
        );
        // A mid-sentence "please" is untouched, and a non-directive stays one.
        assert_eq!(
            normalize_imperative_candidate("The retry will please nobody").as_deref(),
            Some("The retry will please nobody")
        );
        assert!(!text_line_looks_like_prompt_target(
            "The retry will please nobody"
        ));
    }

    #[test]
    fn slash_command_prompt_lines_are_prompt_targets() {
        assert!(line_looks_like_fresh_prompt_after_response("/clear"));
        assert!(!text_line_looks_like_prompt_target(
            "/home/brian/work/foo.md"
        ));
    }

    #[test]
    fn prefixed_markdown_response_labels_are_not_prompt_targets() {
        for line in [
            "❯ **Verification:** Both redirects confirmed via `curl`.",
            "❯ Commit / push:",
            "❯ **Commit / push:**",
            "❯ - **Verification:** `cargo test` passed.",
            "❯ 1. **What changed:** normalized response labels.",
        ] {
            assert!(
                !text_line_looks_like_prompt_target(line),
                "assistant response label must not be classified as a prompt target: {line}"
            );
            assert!(
                line_looks_like_plain_response_after_prompt(line),
                "assistant response label must remain response prose: {line}"
            );
        }
    }

    #[test]
    fn prefixed_user_followup_after_response_still_starts_prompt() {
        for line in [
            "❯ verify deploy status",
            "❯ Verification failed; what next?",
            "❯ do [#respfx]. spec-test-build-install-commit-push",
        ] {
            assert!(
                text_line_looks_like_prompt_target(line),
                "real user follow-up must stay prompt-bearing: {line}"
            );
            assert!(
                line_looks_like_fresh_prompt_after_response(line),
                "real user follow-up must start a new prompt run: {line}"
            );
        }
    }
}
