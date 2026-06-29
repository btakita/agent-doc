//! Pure response text policy for turn closeout and write guards.

/// Strip leading `## Assistant` and trailing `## User` headings from append-mode
/// response text.
///
/// The append writer adds its own `## Assistant` prefix and `## User` suffix, so
/// echoed transcript headings are removed before the response is persisted.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

const IMPERATIVE_STATUS_ONLY_SIGNALS: &[&str] = &[
    "in progress",
    "continuing",
    "starting",
    "working on it",
    "still working",
    "next i'll",
    "next i will",
    "i'll update",
    "i will update",
    "i'm going to",
    "i am going to",
    "let me do that",
];

const IMPERATIVE_META_REFUSAL_SIGNALS: &[&str] = &[
    "because you asked me to run agent-doc",
    "treated that text as document content",
    "not to execute",
    "say do #",
    "repeat the instruction in chat",
    "i stayed on the first layer",
    "operate on the session document",
];

const IMPERATIVE_BLOCKER_SIGNALS: &[&str] = &[
    "blocked",
    "blocker",
    "failed",
    "error",
    "cannot",
    "can't",
    "unable",
    "missing",
    "permission denied",
    "requires approval",
    "needs approval",
    "lock file",
    "timed out",
];

const IMPERATIVE_EVIDENCE_LABELS: &[&str] = &[
    "what changed:",
    "verification:",
    "commit / push:",
    "outcome:",
    "root cause:",
    "blocked:",
    "blocker:",
];

/// Decide whether a response satisfies an executable imperative directive.
///
/// This is the pure response half of the binary backstop. Diff extraction,
/// ops-log emission, and fail-closed error formatting stay in orchestration.
pub fn response_satisfies_imperative_contract(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    if contains_any_signal(&lower, IMPERATIVE_BLOCKER_SIGNALS) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_META_REFUSAL_SIGNALS) {
        return false;
    }
    if contains_execution_evidence(response, &lower) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_STATUS_ONLY_SIGNALS) {
        return false;
    }
    false
}

fn contains_any_signal(haystack: &str, signals: &[&str]) -> bool {
    signals.iter().any(|signal| haystack.contains(signal))
}

fn contains_execution_evidence(response: &str, lower: &str) -> bool {
    if response.contains("```") || response.contains("~~~") {
        return true;
    }
    if IMPERATIVE_EVIDENCE_LABELS
        .iter()
        .any(|label| lower.contains(label))
    {
        return true;
    }
    if lower.contains("implemented and verified")
        || lower.contains("built and installed")
        || lower.contains("added regression coverage")
        || lower.contains("pushed to ")
    {
        return true;
    }
    response.lines().any(|line| {
        has_commandish_backticks(line)
            || has_code_path(line)
            || contains_commit_hash(line)
            || line.trim_start().starts_with("- `")
    })
}

fn has_commandish_backticks(line: &str) -> bool {
    if !line.contains('`') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("git ")
        || lower.contains("make ")
        || lower.contains("npm ")
        || lower.contains("pnpm ")
        || lower.contains("yarn ")
        || lower.contains("pytest")
        || lower.contains("uv run")
        || lower.contains("agent-doc ")
        || line.contains('/')
}

fn has_code_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("src/")
        || line.contains("tests/")
        || line.contains("specs/")
        || line.contains("runbooks/")
        || lower.contains(".rs")
        || lower.contains(".md")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".sh")
        || lower.contains(".kt")
        || lower.contains(".ts")
}

fn contains_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_leading_space_before_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("\n\n## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_trailing_user_heading_and_keeps_newline() {
        assert_eq!(
            strip_assistant_heading("Done.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn strips_both_echoed_headings() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn leaves_plain_response_unchanged() {
        let response = "Done.\n\nDetails.";
        assert_eq!(strip_assistant_heading(response), response.to_string());
    }

    #[test]
    fn imperative_contract_rejects_status_only_response() {
        assert!(!response_satisfies_imperative_contract(
            "### Re: task - gpt-5\nIn progress. Continuing now."
        ));
    }

    #[test]
    fn imperative_contract_rejects_meta_refusal() {
        assert!(!response_satisfies_imperative_contract(
            "I treated that text as document content and not to execute it."
        ));
    }

    #[test]
    fn imperative_contract_allows_concrete_blocker() {
        assert!(response_satisfies_imperative_contract(
            "### Re: blocked - gpt-5\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed."
        ));
    }

    #[test]
    fn imperative_contract_allows_execution_evidence() {
        assert!(response_satisfies_imperative_contract(
            "### Re: done - gpt-5\nVerification:\n- `cargo test --manifest-path src/agent-doc/Cargo.toml`\nCommit / push:\n- `abc1234`\n"
        ));
    }

    #[test]
    fn imperative_contract_allows_code_path_or_commit_hash_evidence() {
        assert!(response_satisfies_imperative_contract(
            "Updated agent-doc-turn/src/response_text.rs and pushed 1b215b7."
        ));
    }
}
