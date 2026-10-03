//! Built-in subagent-intent vocabulary (`#subagentintent`).
//!
//! `#subagents` (and its spellings `#subagent`, `#sub-agents`, `#sub-agent`)
//! asks the agent to dispatch a queue item to a background subagent. The tag
//! carries that meaning on its own, whether or not the document registers it
//! in `prompt_presets`. Every consumer that classifies queue text — steering
//! dispatch, preflight's `queue_subagent_dispatch`, and queue drainability —
//! reads this one list, so a spelling recognised by one is recognised by all.
//!
//! The live miss this fixes (agent-doc-bugs.md, 2026-10-03): the document
//! registered only `'#subagents'`. The operator queued
//! `#subagent: https://…/issues/116`. Steering classified it `subagent`, but
//! queue drainability read `#subagent` as a reference to a tracked backlog item
//! `subagent`, found no such item, and judged every such head non-drainable, so
//! the idle supervisor never woke the session.

/// The canonical subagent-intent tag names, lowercased and without `#`.
pub const SUBAGENT_INTENT_TAGS: [&str; 4] = ["subagents", "subagent", "sub-agents", "sub-agent"];

/// True when `name` (with or without a leading `#`, any case) is a built-in
/// subagent-intent tag.
pub fn is_subagent_intent_tag(name: &str) -> bool {
    let name = name.trim().trim_start_matches('#').to_ascii_lowercase();
    SUBAGENT_INTENT_TAGS.contains(&name.as_str())
}

/// True when free text (a preset expansion body) asks for subagent dispatch.
pub fn text_requests_subagents(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    ["subagent", "sub-agent", "sub agent"]
        .iter()
        .any(|needle| body.contains(needle))
}

/// True when any `#word` token in `text` is a subagent-intent tag.
pub fn carries_subagent_intent_tag(text: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .filter(|token| token.starts_with('#'))
        .any(is_subagent_intent_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_is_recognised() {
        for tag in ["#subagents", "#subagent", "#Sub-Agent", "sub-agents"] {
            assert!(is_subagent_intent_tag(tag), "{tag}");
        }
        assert!(!is_subagent_intent_tag("#subagentx"));
        assert!(!is_subagent_intent_tag("#gh-fix"));
    }

    #[test]
    fn tag_is_found_inside_queue_text() {
        assert!(carries_subagent_intent_tag(
            "#subagent: https://github.com/btakita/agent-doc/issues/118"
        ));
        assert!(carries_subagent_intent_tag("do [#a] #sub-agents"));
        assert!(!carries_subagent_intent_tag("release + publish"));
    }
}
