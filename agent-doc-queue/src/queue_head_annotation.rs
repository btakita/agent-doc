//! Operator annotations on id-backed queue heads (`#qheadannotation`).
//!
//! A backlog-mirror queue head is canonically `do [#id]`. When the operator
//! appends their own text to it — `do [#id]: can the *.h files be generated
//! too?`, `[#id] and keep the old API` — that text is a directive in its own
//! right, not decoration. Id-backed consumption (`--done <id>`) strikes the head
//! by id, so without this module the appended directive was struck along with
//! the head even when the response never answered it (sdk.md, 2026-10-02:
//! `do [#sdkrestemitnative]: can the *.h files be generated from the contract as
//! well?` was quoted in the `> **Queue prompt:**` echo, the backlog item was
//! implemented, and the question was consumed unanswered).
//!
//! This module owns the pure policy:
//!
//! * [`queue_head_annotation`] separates the canonical directive part of a head
//!   from the operator annotation, reusing the shared queue identity
//!   ([`QueueItemIdentity`]) and marker set ([`strip_priority_markers`]).
//! * [`annotation_addressed_in`] is the deterministic response evidence: an
//!   `> **Operator note:** <annotation>` blockquote followed by answer prose.
//! * [`requeue_unaddressed_annotations`] is the never-drop backstop used by the
//!   post-capture consume paths: an unaddressed annotation on a head that is
//!   about to be consumed by id is re-queued verbatim as its own free-text line
//!   directly after the head.

use std::collections::HashSet;

use agent_doc_element_queue::{QueueItemIdentity, strip_priority_markers};
use agent_doc_markdown_ast::reply_row::WAITING_ITEM_MARKER;
use serde::{Deserialize, Serialize};

use crate::queue_response::{normalize_done_id, normalize_for_answer_match};

/// The literal label of the response evidence for a queue-head annotation.
pub const OPERATOR_NOTE_LABEL: &str = "**Operator note:**";

/// An operator annotation attached to an id-backed queue head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueHeadAnnotation {
    /// First tracked-work id the head names (normalized lowercase).
    pub id: String,
    /// Every id the head's leading directive names.
    pub ids: Vec<String>,
    /// The head text as it appears in the queue, priority/lifecycle markers
    /// stripped.
    pub head: String,
    /// The operator's text beyond the canonical directive, verbatim.
    pub annotation_verbatim: String,
}

/// Separate a queue head into its canonical id directive and an operator
/// annotation. Returns `None` for a canonical head (`do [#id]`, `[#id]`,
/// `do [#a] [#b]`), a free-text head, a `re [#id]` reference, a preset
/// invocation (`#gh-fix <url>`), and the inert-prose `[#id]: note` shape, which
/// is a free-text head answered through the `#ftstrike` quote path.
pub fn queue_head_annotation(head: &str) -> Option<QueueHeadAnnotation> {
    // `#queueoperatoranswer`: an operator answer row (`[#request-id]: <answer>`, or the
    // request id with the answer nested beneath it) is the operator's answer
    // to a request, not steering appended to tracked work.
    if agent_doc_document::queue_projection::operator_answer_row_request_id(head).is_some() {
        return None;
    }
    let stripped = strip_priority_markers(head);
    let visible_head = stripped.trim().trim_start_matches('❯').trim();
    // `⟦effect:N⟧` is binary-owned queue identity, not operator prose. Strip a
    // canonical trailing token before applying the authored annotation grammar;
    // this also preserves genuine prose that precedes the token.
    let authored = crate::document_queue::strip_queue_effect_annotation(visible_head);
    let text = authored.trim();
    if text.is_empty() || text.lines().next().is_some_and(|line| line.contains("~~")) {
        return None;
    }
    if !QueueItemIdentity::from_prompt(text).is_id_backed() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("re ") {
        return None;
    }
    let has_do = lower.starts_with("do ");
    let mut rest = if has_do { text[3..].trim_start() } else { text };
    let mut ids = Vec::new();
    loop {
        let (id, after) = if let Some(after_open) = rest.strip_prefix("[#") {
            let Some((id, after)) = after_open.split_once(']') else {
                break;
            };
            (id, after)
        } else if let Some(after_hash) = rest.strip_prefix('#') {
            let len = after_hash
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
                .unwrap_or(after_hash.len());
            (&after_hash[..len], &after_hash[len..])
        } else {
            break;
        };
        if id.is_empty()
            || !id
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        {
            break;
        }
        ids.push(id.to_ascii_lowercase());
        // A following id token must be whitespace-separated (`do [#a] [#b]`).
        let trimmed = after.trim_start();
        if trimmed.len() != after.len() && (trimmed.starts_with("[#") || trimmed.starts_with('#')) {
            rest = trimmed;
            continue;
        }
        rest = after;
        break;
    }
    let first = ids.first()?.clone();
    // `#itemtextasnote`: a generated Waiting-on-you row
    // (`[#id] <todo text> <!-- agent-doc:waiting-item -->`) mirrors its
    // `agent:todo` item. The text up to and including the waiting-item marker
    // is the binary's projection of that todo, not operator steering; only text
    // the operator appends after the marker is an annotation. Operator replies
    // added as child lines are separate rows the waiting tree preserves
    // (`#waitingoperatortext`).
    let first_line_len = rest.find('\n').unwrap_or(rest.len());
    let waiting_row = match rest[..first_line_len].find(WAITING_ITEM_MARKER) {
        Some(at) => {
            rest = &rest[at + WAITING_ITEM_MARKER.len()..];
            true
        }
        None => false,
    };
    // An operator can append steering after the binary has projected an effect
    // id (`do [#id] ⟦effect:1⟧: note`). Ignore the exact metadata token but
    // keep everything after it subject to the normal annotation contract.
    rest = strip_leading_effect_identity(rest);
    if rest.trim().is_empty() {
        return None;
    }
    // Optional-`do` grammar: a bare `[#id]: note` is inert prose (a free-text
    // head), not an annotated directive.
    if !has_do && rest.starts_with(':') && !waiting_row {
        return None;
    }
    let annotation = strip_leading_separator(rest.trim());
    if annotation.is_empty() {
        return None;
    }
    Some(QueueHeadAnnotation {
        id: first,
        ids,
        head: visible_head.to_string(),
        annotation_verbatim: annotation.to_string(),
    })
}

fn strip_leading_effect_identity(text: &str) -> &str {
    let trimmed = text.trim_start();
    let Some(after_prefix) =
        trimmed.strip_prefix(crate::document_queue::QUEUE_EFFECT_ANNOTATION_PREFIX)
    else {
        return text;
    };
    let Some((raw_effect, rest)) = after_prefix.split_once('⟧') else {
        return text;
    };
    let Ok(effect) = raw_effect.parse::<usize>() else {
        return text;
    };
    if effect == 0 {
        return text;
    }
    rest
}

fn strip_leading_separator(text: &str) -> &str {
    for separator in [":", "—", "–", "-", ",", ";", "."] {
        if let Some(rest) = text.strip_prefix(separator) {
            return rest.trim();
        }
    }
    text
}

/// Annotations on every unstruck head in `heads`, in order.
pub fn queue_head_annotations<S: AsRef<str>>(heads: &[S]) -> Vec<QueueHeadAnnotation> {
    heads
        .iter()
        .filter_map(|head| queue_head_annotation(head.as_ref()))
        .collect()
}

/// Annotations on the live, unstruck `agent:queue` heads of `content`.
pub fn live_queue_head_annotations(content: &str) -> anyhow::Result<Vec<QueueHeadAnnotation>> {
    if !agent_doc_element::element::parse(content)?
        .iter()
        .any(|component| component.name == "queue")
    {
        return Ok(Vec::new());
    }
    Ok(
        agent_doc_markdown_ast::mutations::item_nodes(content, "queue")?
            .into_iter()
            .filter(|node| !node.item.struck)
            .filter_map(|node| queue_head_annotation(&node.item.text))
            .collect(),
    )
}

/// True when `annotation` names one of `completion_ids`.
pub fn annotation_completed_by(
    annotation: &QueueHeadAnnotation,
    completion_ids: &[String],
) -> bool {
    let completed = completion_ids
        .iter()
        .map(|id| normalize_done_id(id))
        .collect::<HashSet<_>>();
    annotation.ids.iter().any(|id| completed.contains(id))
}

/// Deterministic evidence that `text` (a response body, or a document whose
/// exchange holds the response) addresses `annotation`: a blockquote whose
/// first line carries `**Operator note:**`, whose quoted text contains the
/// annotation (whitespace/punctuation/case-insensitive), followed by at least
/// one non-quote, non-heading line of answer prose before the next response
/// heading.
pub fn annotation_addressed_in(text: &str, annotation: &str) -> bool {
    let wanted = normalize_for_answer_match(annotation);
    if wanted.is_empty() {
        return true;
    }
    let lines = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_start();
        let Some(quoted) = line.strip_prefix('>') else {
            index += 1;
            continue;
        };
        let Some(after_label) = quoted.trim_start().strip_prefix(OPERATOR_NOTE_LABEL) else {
            index += 1;
            continue;
        };
        let mut note = after_label.to_string();
        index += 1;
        while index < lines.len() {
            let Some(more) = lines[index].trim_start().strip_prefix('>') else {
                break;
            };
            note.push(' ');
            note.push_str(more);
            index += 1;
        }
        if !normalize_for_answer_match(&note).contains(&wanted) {
            continue;
        }
        let answered = lines[index..]
            .iter()
            .map(|line| line.trim())
            .take_while(|line| !crate::queue_response::line_is_response_heading(line))
            .any(|line| !line.is_empty() && !line.starts_with('>') && !line.starts_with("<!--"));
        if answered {
            return true;
        }
    }
    false
}

/// The response shape that satisfies [`annotation_addressed_in`].
pub fn operator_note_evidence_example(annotation: &str) -> String {
    let quoted = annotation
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    format!("> {OPERATOR_NOTE_LABEL} {quoted}\n\n<your answer>")
}

/// Guidance surfaced in the cycle contract next to `queue_head_annotations`.
pub fn queue_head_annotation_guidance(annotations: &[QueueHeadAnnotation]) -> Option<String> {
    if annotations.is_empty() {
        return None;
    }
    let listed = annotations
        .iter()
        .map(|annotation| format!("#{}: {:?}", annotation.id, annotation.annotation_verbatim))
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "The operator annotated {} selected queue head(s) ({listed}). An annotation on a `do [#id]` \
         head is an operator directive for THIS turn, in addition to the backlog item: answer it in \
         the same response, quoting it as `> {OPERATOR_NOTE_LABEL} <annotation>` followed by your \
         answer. Closeout refuses to consume an id head whose annotation is not addressed that way.",
        annotations.len()
    ))
}

/// Annotated live heads that this closeout would consume by id
/// (`completion_ids`) but whose annotation `response` does not address.
pub fn unaddressed_completed_annotations(
    content: &str,
    response: &str,
    completion_ids: &[String],
) -> anyhow::Result<Vec<QueueHeadAnnotation>> {
    if completion_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(live_queue_head_annotations(content)?
        .into_iter()
        .filter(|annotation| annotation_completed_by(annotation, completion_ids))
        .filter(|annotation| !annotation_addressed_in(response, &annotation.annotation_verbatim))
        .collect())
}

/// Never-drop backstop for the id consume paths: for every live annotated head
/// among `consumed_heads` (the head texts a consume plan is about to complete)
/// whose annotation is not addressed by any of `evidence` (the captured
/// response and/or the document holding it), insert the annotation verbatim as
/// its own free-text queue line directly after the head. Returns `None` when
/// nothing needs re-queueing. A re-queue is skipped when the line right after
/// the head already carries that annotation, so a retried consume never
/// duplicates it.
pub fn requeue_unaddressed_annotations(
    content: &str,
    consumed_heads: &[String],
    evidence: &[&str],
) -> anyhow::Result<Option<(String, Vec<QueueHeadAnnotation>)>> {
    let consumed = consumed_heads
        .iter()
        .map(|head| strip_priority_markers(head).trim().to_string())
        .collect::<HashSet<_>>();
    if consumed.is_empty()
        || !agent_doc_element::element::parse(content)?
            .iter()
            .any(|component| component.name == "queue")
    {
        return Ok(None);
    }
    let nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue")?;
    let mut targets = Vec::new();
    for (position, node) in nodes.iter().enumerate() {
        if node.item.struck {
            continue;
        }
        let Some(annotation) = queue_head_annotation(&node.item.text) else {
            continue;
        };
        if !consumed.contains(strip_priority_markers(&node.item.text).trim())
            || evidence
                .iter()
                .any(|text| annotation_addressed_in(text, &annotation.annotation_verbatim))
        {
            continue;
        }
        let wanted = normalize_for_answer_match(&annotation.annotation_verbatim);
        let already_requeued = nodes.get(position + 1).is_some_and(|next| {
            normalize_for_answer_match(&strip_priority_markers(&next.item.text)) == wanted
        });
        if already_requeued {
            continue;
        }
        targets.push((node.node_key.clone(), annotation));
    }
    if targets.is_empty() {
        return Ok(None);
    }
    let mut updated = content.to_string();
    for (node_key, annotation) in &targets {
        let line = annotation
            .annotation_verbatim
            .lines()
            .collect::<Vec<_>>()
            .join("\n  ");
        updated = agent_doc_markdown_ast::mutations::enqueue_node(
            &updated,
            "queue",
            agent_doc_markdown_ast::mutations::MutationInsertPosition::After(node_key.clone()),
            &line,
        )?;
    }
    Ok(Some((
        updated,
        targets
            .into_iter()
            .map(|(_, annotation)| annotation)
            .collect(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotation(head: &str) -> Option<String> {
        queue_head_annotation(head).map(|a| a.annotation_verbatim)
    }

    #[test]
    fn detects_colon_annotation_on_do_directive() {
        let parsed = queue_head_annotation(
            "do [#sdkrestemitnative]: can the *.h files be generated from the contract as well?",
        )
        .unwrap();
        assert_eq!(parsed.id, "sdkrestemitnative");
        assert_eq!(
            parsed.annotation_verbatim,
            "can the *.h files be generated from the contract as well?"
        );
    }

    #[test]
    fn detects_trailing_prose_on_bare_and_do_directives() {
        assert_eq!(
            annotation("[#a] and keep the old API").as_deref(),
            Some("and keep the old API")
        );
        assert_eq!(
            annotation("do [#a] then ship it").as_deref(),
            Some("then ship it")
        );
        assert_eq!(
            annotation("do #a — but skip tests").as_deref(),
            Some("but skip tests")
        );
        assert_eq!(
            annotation("do [#a] [#b]: both, carefully").as_deref(),
            Some("both, carefully")
        );
        assert_eq!(
            queue_head_annotation("do [#a] [#b]: x").unwrap().ids,
            vec!["a", "b"]
        );
    }

    #[test]
    fn detects_marker_prefixed_variants() {
        for head in [
            "🚧 do [#a]: why?",
            ":pushpin: do [#a]: why?",
            "📌 🚧 do [#a]: why?",
            "**pin** do [#a]: why?",
            "❯ do [#a]: why?",
        ] {
            assert_eq!(annotation(head).as_deref(), Some("why?"), "{head}");
        }
    }

    #[test]
    fn canonical_and_non_directive_heads_carry_no_annotation() {
        for head in [
            "do [#a]",
            "[#a]",
            "🚧 do [#a]",
            ":pushpin: [#a]",
            "do #a",
            "do [#a] [#b]",
            "do [#a]:",
            "free text prompt about #a",
            "re [#a] context",
            "[#a]: inert prose stays a free-text head",
            "#gh-fix https://example.com/issues/1",
            "~~do [#a]: done~~",
        ] {
            assert_eq!(annotation(head), None, "{head}");
        }
    }

    #[test]
    fn binary_effect_identity_is_not_an_operator_annotation() {
        for head in [
            "do [#adtypingdupsiblings] ⟦effect:1⟧",
            "do [#adtypingdupsiblings] ⟦effect:2⟧",
            "🚧 do [#adtypingdupsiblings] ⟦effect:2⟧",
        ] {
            assert_eq!(annotation(head), None, "{head}");
        }
    }

    #[test]
    fn genuine_operator_annotation_survives_effect_identity() {
        assert_eq!(
            annotation("do [#a]: keep the old API ⟦effect:1⟧").as_deref(),
            Some("keep the old API")
        );
        assert_eq!(
            annotation("do [#a] ⟦effect:2⟧: keep the old API").as_deref(),
            Some("keep the old API")
        );
    }

    /// `#itemtextasnote` (agent-doc-bugs.md, 2026-11-04): a todo-mirrored
    /// Waiting-on-you row closed with `--done lazilyreleasedecision` was refused
    /// until its own item text was quoted back as an Operator note.
    #[test]
    fn waiting_row_item_text_is_not_an_operator_annotation() {
        for head in [
            "⟦you: 1 · agent: 0 · done: 0⟧ [#lazilyreleasedecision] Decide for lazily-rs 29fde5a8c6 (#lazilysqlitestoreopen): push it to lazily-rs main, and publish a lazily release (source: agent:backlog [#lazilysqlitestoreopen]) <!-- agent-doc:waiting-item -->",
            "[#lazilyreleasedecision] Decide for lazily-rs 29fde5a8c6 <!-- agent-doc:waiting-item -->",
            "⟦📥 you: 0 · agent: 1 · done: 1⟧ [#alpha] Approve alpha [#subagent-operator-request-x]. <!-- agent-doc:waiting-item --> ⟦effect:2⟧",
            "🚧 [#subagent:w#1] ⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Approve the ask. <!-- agent-doc:waiting-item -->",
            "do [#ask] Approve the ask. <!-- agent-doc:waiting-item -->",
        ] {
            assert_eq!(annotation(head), None, "{head}");
        }
    }

    #[test]
    fn operator_text_appended_after_a_waiting_row_marker_is_an_annotation() {
        assert_eq!(
            annotation(
                "⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Approve the ask. <!-- agent-doc:waiting-item --> but only after CI is green"
            )
            .as_deref(),
            Some("but only after CI is green")
        );
        assert_eq!(
            annotation(
                "[#ask] Approve the ask. <!-- agent-doc:waiting-item -->: only after CI ⟦effect:2⟧"
            )
            .as_deref(),
            Some("only after CI")
        );
        assert_eq!(
            annotation(
                "[#ask] Approve the ask. <!-- agent-doc:waiting-item --> ⟦effect:2⟧ — only after CI"
            )
            .as_deref(),
            Some("only after CI")
        );
    }

    #[test]
    fn multiline_annotation_keeps_continuation_lines() {
        assert_eq!(
            annotation("do [#a]: first line\n  second line").as_deref(),
            Some("first line\n  second line")
        );
    }

    #[test]
    fn operator_note_evidence_requires_quote_and_answer() {
        let note = "can the *.h files be generated from the contract as well?";
        let answered = "### Re: x\n\n> **Operator note:** can the *.h files be generated from the contract as well?\n\nYes: `emit_headers.py` now renders them.\n";
        assert!(annotation_addressed_in(answered, note));
        let quote_only = "### Re: x\n\n> **Operator note:** can the *.h files be generated from the contract as well?\n\n### Re: y\n\nother\n";
        assert!(!annotation_addressed_in(quote_only, note));
        // The sdk.md failure: the head quoted as a queue prompt is not evidence.
        let queue_prompt_echo = "### Re: x\n\n> **Queue prompt:**\n>\n> do [#sdkrestemitnative]: can the *.h files be generated from the contract as well?\n\nDone as draft PR #50.\n";
        assert!(!annotation_addressed_in(queue_prompt_echo, note));
        let wrapped = "> **Operator note:** can the *.h files be\n> generated from the contract as well?\n\nNo, they stay hand-written because...\n";
        assert!(annotation_addressed_in(wrapped, note));
    }

    fn doc(queue: &str) -> String {
        format!(
            "---\nqueue_active: true\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:backlog queue -->\n- [ ] [#a] item a\n- [ ] [#b] item b\n<!-- /agent:backlog -->\n"
        )
    }

    #[test]
    fn unaddressed_completed_annotations_only_for_completed_ids() {
        let content = doc("- do [#a]: why?\n- do [#b]: how?\n");
        let missing =
            unaddressed_completed_annotations(&content, "### Re: a\n\ndone\n", &["a".into()])
                .unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].id, "a");
        let addressed = "### Re: a\n\n> **Operator note:** why?\n\nBecause.\n";
        assert!(
            unaddressed_completed_annotations(&content, addressed, &["#a".into()])
                .unwrap()
                .is_empty()
        );
        assert!(
            unaddressed_completed_annotations(&content, "x", &[])
                .unwrap()
                .is_empty()
        );
    }

    fn waiting_doc(queue: &str) -> String {
        format!(
            "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:queue id=\"main\" -->\n{queue}<!-- /agent:queue -->\n\n<!-- agent:todo -->\n- [ ] [#ask] Approve the ask.\n<!-- /agent:todo -->\n\n<!-- agent:backlog queue -->\n- [ ] [#a] item a\n<!-- /agent:backlog -->\n"
        )
    }

    const WAITING_TREE: &str = "- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->\n  - ⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Approve the ask. <!-- agent-doc:waiting-item -->\n    - **Next:** you — reply under this item ▫️\n    - **Response:** Approve the ask. ▫️\n";

    /// The finalize pre-write gate shape: closing a waiting row by id needs no
    /// Operator note for the row's own text, while an annotated `do [#id]` head
    /// completed by the same closeout still fails closed until answered.
    #[test]
    fn closeout_gate_skips_waiting_row_text_but_still_blocks_annotated_heads() {
        let content = waiting_doc(&format!("- do [#a]: keep the old API\n{WAITING_TREE}"));
        let response = "### Re: ask\n\n`#ask`: done; closed in this closeout.\n";
        let missing = unaddressed_completed_annotations(
            &content,
            response,
            &["ask".to_string(), "a".to_string()],
        )
        .unwrap();
        assert_eq!(
            missing
                .iter()
                .map(|annotation| (
                    annotation.id.as_str(),
                    annotation.annotation_verbatim.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![("a", "keep the old API")],
            "only the operator annotation gates the closeout"
        );
        let answered = format!("{response}\n> **Operator note:** keep the old API\n\nKept.\n");
        assert!(
            unaddressed_completed_annotations(&content, &answered, &["ask".into(), "a".into()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn closeout_gate_still_blocks_an_operator_note_appended_to_a_waiting_row() {
        let tree = WAITING_TREE.replace(
            "<!-- agent-doc:waiting-item -->",
            "<!-- agent-doc:waiting-item --> but only after CI is green",
        );
        let content = waiting_doc(&tree);
        let missing =
            unaddressed_completed_annotations(&content, "### Re: ask\n\ndone\n", &["ask".into()])
                .unwrap();
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(missing[0].annotation_verbatim, "but only after CI is green");
        let answered = "### Re: ask\n\n> **Operator note:** but only after CI is green\n\nCI was green first.\n";
        assert!(
            unaddressed_completed_annotations(&content, answered, &["ask".into()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn requeue_never_copies_a_waiting_rows_own_text_into_the_queue() {
        let content = waiting_doc(WAITING_TREE);
        let head =
            "⟦you: 1 · agent: 0 · done: 0⟧ [#ask] Approve the ask. <!-- agent-doc:waiting-item -->"
                .to_string();
        assert!(
            requeue_unaddressed_annotations(
                &content,
                std::slice::from_ref(&head),
                &["no evidence"]
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn requeue_inserts_annotation_after_head_once() {
        let content = doc("- do [#a]: why is the sky blue?\n- do [#b]\n");
        // The consume plan reports the completed head without its 🚧 marker.
        let a_head = "🚧 do [#a]: why is the sky blue?".to_string();
        let (updated, requeued) = requeue_unaddressed_annotations(
            &content,
            std::slice::from_ref(&a_head),
            &["no evidence"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(requeued.len(), 1);
        assert!(
            updated
                .contains("- do [#a]: why is the sky blue?\n- why is the sky blue?\n- do [#b]\n"),
            "{updated}"
        );
        // A retried consume over the already-requeued document adds nothing.
        assert!(
            requeue_unaddressed_annotations(&updated, std::slice::from_ref(&a_head), &[])
                .unwrap()
                .is_none()
        );
        // Addressed evidence: nothing to requeue.
        let addressed = "> **Operator note:** why is the sky blue?\n\nRayleigh scattering.\n";
        assert!(
            requeue_unaddressed_annotations(&content, std::slice::from_ref(&a_head), &[addressed],)
                .unwrap()
                .is_none()
        );
        // Not consumed by this closeout: nothing to requeue.
        assert!(
            requeue_unaddressed_annotations(&content, &["do [#b]".to_string()], &[])
                .unwrap()
                .is_none()
        );
    }
}
