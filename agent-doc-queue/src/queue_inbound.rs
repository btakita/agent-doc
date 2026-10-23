//! Pure inbound queue-head composition and dedupe (`#queueinbound`, GH #237).
//!
//! `agent-doc queue add <FILE> --source <name> [--key <k>] [--attach <PATH>] <TEXT>`
//! lets an external tool (an editor plugin, a play-mode bridge, a CI watcher)
//! hand work to a session. This module owns the deterministic half: the
//! attributed head shape, its validation, the dedupe rule, and the queue-body
//! insertion. The IO crate owns redaction, attachment storage, and the
//! binary-owned write.
//!
//! Head shape (one queue line, always single-line):
//!
//! ```text
//! - [from:<source>] <text>
//! - [from:<source> key=<key>] <text> (attachment: <path>)
//! ```
//!
//! The `[from:...]` tag carries no `#`, so it never reads as a tracked-work id
//! or a preset reference; the head classifies as an ordinary drainable
//! free-text head. Dedupe is visible in the document itself (no side ledger):
//! a LIVE (unstruck) queue head with the same `source` + `key` makes the add a
//! no-op. Without `--key`, a live head with byte-identical rendered text is the
//! duplicate. A struck (consumed) head never blocks a later add, so a recurring
//! failure re-enters the queue once its earlier head was handled.

use anyhow::{Result, bail};

use crate::document_queue::{self, QueueEntry};

/// Prefix of the attribution tag every inbound head starts with.
pub const INBOUND_TAG_PREFIX: &str = "[from:";

/// Parsed `[from:<source> key=<key>]` attribution of a queue head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundTag {
    pub source: String,
    pub key: Option<String>,
}

/// Outcome of planning an inbound append against the current document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundAppendPlan {
    /// The head is new; `content` is the full document with it appended.
    Append { content: String, head: String },
    /// A live head already carries this key (or identical text): no write.
    Duplicate { existing: String },
}

/// Insert queue-local follow-up heads immediately after the head that produced
/// them. Queue id and origin are both mandatory: a mirrored copy in another
/// queue must never redirect the insertion to the document's selected queue.
pub fn plan_queue_followups(
    content: &str,
    queue_id: &str,
    origin: &str,
    followups: &[String],
) -> Result<String> {
    if followups.is_empty() {
        bail!("queue follow-up needs at least one --item");
    }
    let components = agent_doc_element::element::parse(content)?;
    let queue = crate::queue_graph::component_for_id(content, &components, queue_id)?
        .ok_or_else(|| anyhow::anyhow!("agent:queue id `{queue_id}` not found"))?;
    let body = queue.content(content);
    let wanted = crate::queue_head_alignment::normalize_queue_head_text(origin);
    let mut matches = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let line_text = line.trim_end_matches(['\r', '\n']);
        let Some(prompt) = line_text
            .trim_start()
            .strip_prefix("- ")
            .filter(|text| !text.trim_start().starts_with("~~"))
        else {
            offset += line.len();
            continue;
        };
        if crate::queue_head_alignment::normalize_queue_head_text(prompt) == wanted {
            matches.push(offset + line.len());
        }
        offset += line.len();
    }
    let insertion_at = match matches.as_slice() {
        [offset] => queue.open_end + offset,
        [] => bail!("queue follow-up origin {origin:?} not found in queue id `{queue_id}`"),
        _ => bail!("queue follow-up origin {origin:?} is ambiguous in queue id `{queue_id}`"),
    };
    let mut insertion = String::new();
    for item in followups {
        let item = normalize_inbound_text(item)?;
        insertion.push_str("- ");
        insertion.push_str(&item);
        insertion.push('\n');
    }
    let mut updated = String::with_capacity(content.len() + insertion.len());
    updated.push_str(&content[..insertion_at]);
    updated.push_str(&insertion);
    updated.push_str(&content[insertion_at..]);
    Ok(updated)
}

fn valid_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/' | '@')
}

/// Validate a `--source` / `--key` token. Tokens are restricted so the tag
/// round-trips through [`parse_inbound_tag`] and can never smuggle a `#id`,
/// a bracket, or whitespace into the queue line.
pub fn validate_inbound_token(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("--{label} must not be empty");
    }
    if value.len() > 128 {
        bail!("--{label} must be at most 128 characters");
    }
    if let Some(bad) = value.chars().find(|ch| !valid_token_char(*ch)) {
        bail!(
            "--{label} {value:?} contains {bad:?}; allowed characters are ASCII letters, digits, and - _ . : / @"
        );
    }
    Ok(())
}

/// Collapse `text` to one queue line: every whitespace run (including
/// newlines) becomes a single space. Empty text is refused.
pub fn normalize_inbound_text(text: &str) -> Result<String> {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        bail!("queue add: the head text must not be empty");
    }
    Ok(collapsed)
}

/// Render the attributed queue-head text (without the `- ` list marker).
/// `text` must already be normalized and redacted; `attachments` are the
/// project-relative paths of already-redacted attachment copies.
pub fn render_inbound_head(
    source: &str,
    key: Option<&str>,
    text: &str,
    attachments: &[String],
) -> String {
    let mut head = String::from(INBOUND_TAG_PREFIX);
    head.push_str(source);
    if let Some(key) = key {
        head.push_str(" key=");
        head.push_str(key);
    }
    head.push_str("] ");
    head.push_str(text);
    for attachment in attachments {
        head.push_str(" (attachment: ");
        head.push_str(attachment);
        head.push(')');
    }
    head
}

/// Parse the leading `[from:<source>( key=<key>)?]` tag of a queue head.
pub fn parse_inbound_tag(head: &str) -> Option<InboundTag> {
    let rest = head.trim_start().strip_prefix(INBOUND_TAG_PREFIX)?;
    let close = rest.find(']')?;
    let inner = &rest[..close];
    let (source, key) = match inner.split_once(" key=") {
        Some((source, key)) => (source, Some(key)),
        None => (inner, None),
    };
    if source.is_empty() || !source.chars().all(valid_token_char) {
        return None;
    }
    if let Some(key) = key
        && (key.is_empty() || !key.chars().all(valid_token_char))
    {
        return None;
    }
    Some(InboundTag {
        source: source.to_string(),
        key: key.map(str::to_string),
    })
}

/// Live (unstruck) queue prompt texts of a queue body.
fn live_queue_prompts(body: &str) -> Result<Vec<String>> {
    Ok(document_queue::parse(body)?
        .into_iter()
        .filter_map(|entry| match entry {
            QueueEntry::Prompt(prompt) => Some(prompt.text.trim().to_string()),
            _ => None,
        })
        .collect())
}

/// Plan appending `head` (rendered by [`render_inbound_head`]) to the end of
/// the document's selected `agent:run` component (the first one unless a
/// queue set selects another).
///
/// The insertion is byte-preserving: every existing queue line, including
/// operator formatting the queue parser would normalize, stays exactly as
/// authored (`#qauthorder`); only the new `- <head>` line is added before the
/// closing marker.
pub fn plan_inbound_append(
    content: &str,
    head: &str,
    source: &str,
    key: Option<&str>,
) -> Result<InboundAppendPlan> {
    let components = agent_doc_element::element::parse(content)?;
    // The same selected queue block preflight reads (`queue_graph`), so the
    // head lands where the next preflight looks for queue work.
    let Some(queue) = crate::queue_graph::selected_component(content, &components)? else {
        bail!(
            "no agent:run component found. Add `<!-- agent:run -->` ... `<!-- /agent:run -->` to the document. (`agent:queue` remains a supported compatibility alias.)"
        );
    };
    let body = &content[queue.open_end..queue.close_start];
    for live in live_queue_prompts(body)? {
        let duplicate = match key {
            Some(key) => parse_inbound_tag(&live)
                .is_some_and(|tag| tag.source == source && tag.key.as_deref() == Some(key)),
            None => live == head,
        };
        if duplicate {
            return Ok(InboundAppendPlan::Duplicate { existing: live });
        }
    }
    let mut insertion = String::new();
    if !body.is_empty() && !body.ends_with('\n') {
        insertion.push('\n');
    }
    insertion.push_str("- ");
    insertion.push_str(head);
    insertion.push('\n');
    let mut updated = String::with_capacity(content.len() + insertion.len());
    updated.push_str(&content[..queue.close_start]);
    updated.push_str(&insertion);
    updated.push_str(&content[queue.close_start..]);
    Ok(InboundAppendPlan::Append {
        content: updated,
        head: head.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:queue go -->\n",
        "- current task\n",
        "- ~~done earlier~~\n",
        "<!-- /agent:queue -->\n",
    );

    #[test]
    fn renders_and_parses_attribution_round_trip() {
        let head = render_inbound_head(
            "unity",
            Some("test/PlayerSpec"),
            "PlayerSpec failed",
            &[".agent-doc/inbound/abc-log.txt".to_string()],
        );
        assert_eq!(
            head,
            "[from:unity key=test/PlayerSpec] PlayerSpec failed (attachment: .agent-doc/inbound/abc-log.txt)"
        );
        assert_eq!(
            parse_inbound_tag(&head),
            Some(InboundTag {
                source: "unity".into(),
                key: Some("test/PlayerSpec".into()),
            })
        );
        assert_eq!(
            parse_inbound_tag("[from:ci] build broke").unwrap().key,
            None
        );
        assert_eq!(parse_inbound_tag("plain operator prompt"), None);
    }

    #[test]
    fn tokens_reject_hash_brackets_and_whitespace() {
        assert!(validate_inbound_token("source", "ci-watcher").is_ok());
        for bad in ["", "a b", "x#y", "x]y", "[x"] {
            assert!(validate_inbound_token("source", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn text_collapses_to_one_line() {
        assert_eq!(
            normalize_inbound_text("  line one\n  line two\t end ").unwrap(),
            "line one line two end"
        );
        assert!(normalize_inbound_text(" \n ").is_err());
    }

    #[test]
    fn append_lands_before_close_marker_byte_preserving() {
        let head = render_inbound_head("ci", None, "fix the build", &[]);
        let InboundAppendPlan::Append { content, .. } =
            plan_inbound_append(DOC, &head, "ci", None).unwrap()
        else {
            panic!("expected append");
        };
        assert_eq!(
            content,
            DOC.replace(
                "- ~~done earlier~~\n",
                "- ~~done earlier~~\n- [from:ci] fix the build\n"
            )
        );
    }

    #[test]
    fn followups_stay_after_origin_in_exact_queue_when_head_is_mirrored() {
        let doc = concat!(
            "<!-- agent:queue id=main subagents -->\n",
            "- do [#httpsgithubcom]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues loop interval=1h -->\n",
            "- do [#httpsgithubcom]\n",
            "- existing tail\n",
            "<!-- /agent:queue -->\n",
        );
        let followups = vec![
            "#gh-fix https://github.com/btakita/agent-doc/issues/303".to_string(),
            "#gh-fix https://github.com/btakita/agent-doc/issues/304".to_string(),
            "#gh-fix https://github.com/btakita/agent-doc/issues/305".to_string(),
            "release + publish".to_string(),
        ];
        let updated =
            plan_queue_followups(doc, "issues", "do [#httpsgithubcom]", &followups).unwrap();
        assert_eq!(
            updated,
            concat!(
                "<!-- agent:queue id=main subagents -->\n",
                "- do [#httpsgithubcom]\n",
                "<!-- /agent:queue -->\n\n",
                "<!-- agent:queue id=issues loop interval=1h -->\n",
                "- do [#httpsgithubcom]\n",
                "- #gh-fix https://github.com/btakita/agent-doc/issues/303\n",
                "- #gh-fix https://github.com/btakita/agent-doc/issues/304\n",
                "- #gh-fix https://github.com/btakita/agent-doc/issues/305\n",
                "- release + publish\n",
                "- existing tail\n",
                "<!-- /agent:queue -->\n",
            )
        );
    }

    #[test]
    fn followups_fail_closed_on_wrong_queue_or_ambiguous_origin() {
        let doc = concat!(
            "<!-- agent:queue id=main -->\n",
            "- do [#same]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues -->\n",
            "- do [#same]\n",
            "- do [#same]\n",
            "<!-- /agent:queue -->\n",
        );
        let items = vec!["follow-up".to_string()];
        assert!(plan_queue_followups(doc, "missing", "do [#same]", &items).is_err());
        assert!(plan_queue_followups(doc, "issues", "do [#missing]", &items).is_err());
        assert!(plan_queue_followups(doc, "issues", "do [#same]", &items).is_err());
    }

    #[test]
    fn same_key_is_a_duplicate_but_a_struck_head_is_not() {
        let head = render_inbound_head("ci", Some("k1"), "first wording", &[]);
        let InboundAppendPlan::Append { content, .. } =
            plan_inbound_append(DOC, &head, "ci", Some("k1")).unwrap()
        else {
            panic!("expected append");
        };
        let again = render_inbound_head("ci", Some("k1"), "second wording", &[]);
        assert_eq!(
            plan_inbound_append(&content, &again, "ci", Some("k1")).unwrap(),
            InboundAppendPlan::Duplicate {
                existing: head.clone()
            }
        );
        // Another source with the same key is a different head.
        let other = render_inbound_head("unity", Some("k1"), "x", &[]);
        assert!(matches!(
            plan_inbound_append(&content, &other, "unity", Some("k1")).unwrap(),
            InboundAppendPlan::Append { .. }
        ));
        // Once the head is consumed (struck), the key may enter again.
        let struck = content.replace(&format!("- {head}"), &format!("- ~~{head}~~"));
        assert!(matches!(
            plan_inbound_append(&struck, &again, "ci", Some("k1")).unwrap(),
            InboundAppendPlan::Append { .. }
        ));
    }

    #[test]
    fn keyless_identical_text_is_a_duplicate() {
        let head = render_inbound_head("ci", None, "fix the build", &[]);
        let InboundAppendPlan::Append { content, .. } =
            plan_inbound_append(DOC, &head, "ci", None).unwrap()
        else {
            panic!("expected append");
        };
        assert!(matches!(
            plan_inbound_append(&content, &head, "ci", None).unwrap(),
            InboundAppendPlan::Duplicate { .. }
        ));
    }

    #[test]
    fn inbound_head_is_drainable_free_text() {
        let head = render_inbound_head("ci", Some("k"), "PlayerSpec failed in play mode", &[]);
        assert!(crate::queue_continuation::is_drainable_queue_head(&head));
        let doc = DOC.replace("- current task\n", &format!("- {head}\n"));
        assert_eq!(
            crate::queue_heads::classify_active_queue_head(&doc).unwrap(),
            crate::queue_heads::ActiveQueueHeadKind::FreeText
        );
    }

    #[test]
    fn missing_queue_component_is_refused() {
        let err = plan_inbound_append("# no queue\n", "[from:ci] x", "ci", None).unwrap_err();
        assert!(err.to_string().contains("no agent:run component"), "{err}");
    }
}
