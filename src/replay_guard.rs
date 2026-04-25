//! # Module: replay_guard
//!
//! Shared shape validation for replaying assistant payloads through recovery
//! paths such as the Codex Stop hook and `agent-doc repair`.
//!
//! ## Spec
//! - `classify_replay_payload(message)` trims the candidate payload and
//!   classifies it as empty, replayable, or blocked.
//! - Block transcript/full-document shaped payloads that should never be
//!   replayed automatically: agent component dumps, transcript prompt lines,
//!   `## User` / `## Assistant` headings, multiple `### Re:` headings, or
//!   malformed patch payloads.
//! - Patch-bearing payloads are replayable only when `template::parse_patches`
//!   returns at least one patch, no unmatched transcript text, and at least one
//!   `patch:exchange` block.
//!
//! ## Agentic Contracts
//! - The validator is conservative: ambiguous transcript-shaped payloads are
//!   blocked instead of auto-extracted.
//! - Replayable payloads are returned as the trimmed original string so callers
//!   can persist or replay the exact closeout body.
//!
//! ## Evals
//! - `classify_empty_payload`
//! - `classify_plain_response_body_as_replayable`
//! - `classify_single_exchange_patch_as_replayable`
//! - `classify_multiple_clean_patches_as_replayable`
//! - `classify_blocks_agent_component_dump`
//! - `classify_blocks_prompt_lines`
//! - `classify_blocks_multiple_response_headings`
//! - `classify_blocks_patch_payload_with_unmatched_transcript`

pub(crate) enum ReplayPayloadClassification<'a> {
    Empty,
    Replayable(&'a str),
    Blocked(String),
}

pub(crate) fn classify_replay_payload(message: &str) -> ReplayPayloadClassification<'_> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return ReplayPayloadClassification::Empty;
    }

    if trimmed.contains("<!-- agent:") || trimmed.contains("<!-- /agent:") {
        return ReplayPayloadClassification::Blocked(
            "it contained agent component markers from a full document component dump".to_string(),
        );
    }

    let prompt_lines = trimmed
        .lines()
        .filter(|line| line.trim_start().starts_with('❯'))
        .count();
    if prompt_lines > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained transcript prompt lines".to_string(),
        );
    }

    let user_headings = trimmed
        .lines()
        .filter(|line| {
            let line = line.trim();
            line == "## User" || line.starts_with("## User ")
        })
        .count();
    if user_headings > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained `## User` transcript headings".to_string(),
        );
    }

    let assistant_headings = trimmed
        .lines()
        .filter(|line| {
            let line = line.trim();
            line == "## Assistant" || line.starts_with("## Assistant ")
        })
        .count();
    if assistant_headings > 0 {
        return ReplayPayloadClassification::Blocked(
            "it contained `## Assistant` transcript headings".to_string(),
        );
    }

    let response_headings = trimmed
        .lines()
        .filter(|line| line.trim_start().starts_with("### Re:"))
        .count();
    if response_headings > 1 {
        return ReplayPayloadClassification::Blocked(
            "it contained multiple assistant response headings".to_string(),
        );
    }

    let has_patch_markers = trimmed.contains("<!-- patch:") || trimmed.contains("<!-- /patch:");
    if has_patch_markers {
        match crate::template::parse_patches(trimmed) {
            Ok((patches, unmatched)) => {
                if patches.is_empty() {
                    return ReplayPayloadClassification::Blocked(
                        "it contained malformed patch markers without a replayable patch block"
                            .to_string(),
                    );
                }
                if !unmatched.trim().is_empty() {
                    return ReplayPayloadClassification::Blocked(
                        "it contained extra transcript content around patch blocks".to_string(),
                    );
                }
                if !patches.iter().any(|patch| patch.name == "exchange") {
                    return ReplayPayloadClassification::Blocked(
                        "it did not include a `patch:exchange` closeout".to_string(),
                    );
                }
            }
            Err(err) => {
                return ReplayPayloadClassification::Blocked(format!(
                    "it contained malformed patch markers: {err}"
                ));
            }
        }
    }

    ReplayPayloadClassification::Replayable(trimmed)
}

#[cfg(test)]
mod tests {
    use super::{ReplayPayloadClassification, classify_replay_payload};

    fn assert_replayable(payload: &str) {
        match classify_replay_payload(payload) {
            ReplayPayloadClassification::Replayable(actual) => {
                assert_eq!(actual, payload.trim());
            }
            ReplayPayloadClassification::Empty => panic!("expected replayable payload, got empty"),
            ReplayPayloadClassification::Blocked(reason) => {
                panic!("expected replayable payload, got blocked: {reason}")
            }
        }
    }

    fn assert_blocked(payload: &str, needle: &str) {
        match classify_replay_payload(payload) {
            ReplayPayloadClassification::Blocked(reason) => {
                assert!(
                    reason.contains(needle),
                    "expected reason to contain `{needle}`, got `{reason}`"
                );
            }
            ReplayPayloadClassification::Empty => panic!("expected blocked payload, got empty"),
            ReplayPayloadClassification::Replayable(actual) => {
                panic!("expected blocked payload, got replayable: {actual}")
            }
        }
    }

    #[test]
    fn classify_empty_payload() {
        assert!(matches!(
            classify_replay_payload("   \n"),
            ReplayPayloadClassification::Empty
        ));
    }

    #[test]
    fn classify_plain_response_body_as_replayable() {
        assert_replayable("### Re: topic — gpt-5\n\nBody\n");
    }

    #[test]
    fn classify_single_exchange_patch_as_replayable() {
        assert_replayable(
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->\n",
        );
    }

    #[test]
    fn classify_multiple_clean_patches_as_replayable() {
        assert_replayable(
            "<!-- patch:status -->\nDone.\n<!-- /patch:status -->\n<!-- patch:exchange -->\n### Re: topic — gpt-5\n\nBody\n<!-- /patch:exchange -->\n",
        );
    }

    #[test]
    fn classify_blocks_agent_component_dump() {
        assert_blocked(
            "<!-- agent:exchange patch=append -->\n❯ hi\n### Re: topic — gpt-5\n<!-- /agent:exchange -->\n",
            "component",
        );
    }

    #[test]
    fn classify_blocks_prompt_lines() {
        assert_blocked("❯ do #task\n### Re: topic — gpt-5\n", "prompt lines");
    }

    #[test]
    fn classify_blocks_multiple_response_headings() {
        assert_blocked(
            "### Re: first — gpt-5\none\n### Re: second — gpt-5\ntwo\n",
            "multiple assistant response headings",
        );
    }

    #[test]
    fn classify_blocks_patch_payload_with_unmatched_transcript() {
        assert_blocked(
            "<!-- patch:exchange -->\n### Re: topic — gpt-5\nBody\n<!-- /patch:exchange -->\nextra transcript",
            "extra transcript content",
        );
    }
}
