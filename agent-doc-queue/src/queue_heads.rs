//! Active queue-head projection from document text.
//!
//! This module owns pure `agent:queue` head classification. Callers provide
//! document text; file IO, cycle-state persistence, and closeout guards stay in
//! orchestration.

use agent_doc_document::queue_projection::{strip_in_progress_marker, strip_priority_markers};
use anyhow::{Context, Result};

use crate::queue_response::{
    display_queue_prompt_text, free_text_head_answered_by_response, normalize_done_id,
    queue_head_is_free_text_prompt, queue_prompt_done_id, queue_prompt_text_is_free_text,
};

/// Extract queue prompt head texts from a document's `agent:queue` component.
pub fn active_queue_heads(doc: &str) -> Vec<String> {
    queue_prompt_heads(doc)
}

/// Extract free-text (non-id-backed) queue prompt head texts from a document's
/// `agent:queue` component.
pub fn active_free_text_queue_heads(doc: &str) -> Vec<String> {
    queue_prompt_heads(doc)
        .into_iter()
        .map(|text| strip_priority_markers(&text))
        .filter(|text| {
            !text.is_empty() && !is_do_directive(text) && queue_prompt_text_is_free_text(doc, text)
        })
        .collect()
}

/// True when a queue head is id-backed: explicit `do [#id]` / `do #id`, or the
/// optional-`do` bare leading `[#id]` / `#id` form.
pub fn is_do_directive(text: &str) -> bool {
    let lower = strip_priority_markers(text).to_ascii_lowercase();
    lower.starts_with("do [#") || lower.starts_with("do #") || leads_with_bare_id_directive(&lower)
}

/// Return the currently active queue head text when frontmatter marks the queue
/// active and the document has a first prompt in `agent:queue`.
pub fn active_queue_head_text(content: &str) -> Result<Option<String>> {
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content)?;
    if fm.queue_active != Some(true) {
        return Ok(None);
    }
    let components = agent_doc_element::element::parse(content)?;
    let queue_set = crate::queue_set::QueueSet::parse(content, &components)?;
    let Some(queue) = queue_set.selected().map(|block| block.component) else {
        if !queue_set.blocks().is_empty() {
            return Ok(None);
        }
        return Err(anyhow::anyhow!(
            "queue consume guard: queue_active is true but document has no agent:queue component"
        ));
    };
    let entries = crate::document_queue::parse(queue.content(content))
        .context("queue consume guard: failed to parse document queue")?;
    Ok(crate::document_queue::first_prompt(&entries).map(|prompt| prompt.text.clone()))
}

/// Return the first prompt that should drive dispatch when the document has no
/// fresh diff but its queue is already active.
pub fn active_queue_prompt(content: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let queue_component = crate::queue_set::selected_component(content, &components)
        .ok()
        .flatten()?;
    let entries = crate::document_queue::parse(queue_component.content(content)).ok()?;
    let has_auto = crate::document_queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).ok()?;
    let activation = crate::document_queue::resolve_activation(
        &entries,
        has_auto,
        false,
        fm.queue_active.unwrap_or(false),
    );
    if !activation.active {
        return None;
    }
    crate::document_queue::prompts(&activation.entries_after)
        .first()
        .map(|prompt| strip_in_progress_marker(&prompt.text))
}

/// True when document frontmatter explicitly parks queue execution.
///
/// The only hold is `queue: pause` (`#queuestopremove`).
///
/// This is distinct from a queue that is merely not activated yet: Codex Stop
/// recovery still treats an ordinary manual queue head as pending document
/// work, but must not reopen a committed turn for an operator-stopped queue.
pub fn queue_is_explicitly_stopped(content: &str) -> bool {
    agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(frontmatter, _)| frontmatter.queue_active)
        == Some(false)
}

/// True when the current diff activates the document queue for prompt
/// extraction, including explicit `do queue` / `run queue` triggers.
pub fn queue_is_active_for_diff(content: &str, diff_text: &str) -> bool {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return false;
    };
    let Ok(Some(queue_component)) = crate::queue_set::selected_component(content, &components)
    else {
        return false;
    };
    let Ok(entries) = crate::document_queue::parse(queue_component.content(content)) else {
        return false;
    };
    let has_auto = crate::document_queue::has_auto_attr(&queue_component.attrs);
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap_or_default();
    crate::document_queue::resolve_activation(
        &entries,
        has_auto,
        agent_doc_diff::detect_queue_trigger(diff_text),
        fm.queue_active.unwrap_or(false),
    )
    .active
}

/// Classification of the leading prompt in an `agent:queue` component for
/// explicit operator-driven consumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveQueueHeadKind {
    /// No queue component, or no live prompt to strike.
    None,
    /// Live queue rows exist, but the frontmatter activation projection is
    /// false or absent. Captured-response recovery may heal this torn state;
    /// blind free-text consumption must not.
    Inactive,
    /// A free-text head (a plain question/instruction) that can be struck by the
    /// explicit consume command. Also covers bare registered `prompt_presets`
    /// token heads that have no tracked-work reap path.
    FreeText,
    /// An id-backed head (`#id`, `[#id]`, `do [#id]`, or a queue trigger) that
    /// names tracked work and must be reaped or acknowledged through id-aware
    /// paths instead of struck blindly.
    IdBacked,
}

/// Classify the leading `agent:queue` prompt for explicit queue consumption.
///
/// This intentionally checks for a queue prompt before consulting the canonical
/// free-text detector. When a document has queued prompts but the queue is not
/// active, the free-text detector returns false, preserving the explicit
/// consume command's historical fail-closed behavior for inactive queued text.
pub fn classify_active_queue_head(content: &str) -> Result<ActiveQueueHeadKind> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(queue) = crate::queue_set::selected_component(content, &components)? else {
        return Ok(ActiveQueueHeadKind::None);
    };
    let entries = crate::document_queue::parse(queue.content(content))?;
    if crate::document_queue::prompts(&entries).is_empty() {
        return Ok(ActiveQueueHeadKind::None);
    }
    let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(content)?;
    if frontmatter.queue_active != Some(true) {
        return Ok(ActiveQueueHeadKind::Inactive);
    }
    if queue_head_is_free_text_prompt(content)? {
        Ok(ActiveQueueHeadKind::FreeText)
    } else {
        Ok(ActiveQueueHeadKind::IdBacked)
    }
}

/// Operator-facing diagnostic explaining why active queue-head consumption was
/// skipped for this document content.
pub fn queue_skip_diagnostic_for_content(content: &str) -> Result<String> {
    const GENERIC: &str =
        "[queue] skipped consumption because the active prompt did not target the queue head";

    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(GENERIC.to_string());
    };
    let queue_head_display = display_queue_prompt_text(&queue_head);
    if queue_prompt_text_is_free_text(content, &queue_head) {
        return Ok(format!(
            "[queue] kept free-text head `{queue_head_display}` because free-text heads are consumed only when this cycle's response quotes that exact queue prompt. Add a `> **Queue prompt:**` echo for this head, or leave it queued."
        ));
    }
    if let Some(id) = queue_prompt_done_id(&queue_head) {
        if let Some(suggestion) = untracked_head_id_suggestion(content, &id) {
            return Ok(format!(
                "[queue] kept head `{queue_head_display}`: #{id} is not a tracked item in this document, so no `--done {id}` can ever match it. The closest tracked id is #{suggestion} — the queue line is likely a typo for it. Work #{suggestion} and close it with `--done {suggestion}`, then correct the queue line to `#{suggestion}` or strike the typo head with `agent-doc queue consume <FILE> --id {id}`. (#queueidtypo)"
            ));
        }
        return Ok(format!(
            "[queue] kept head `{queue_head_display}` because the response did not record a completion outcome for #{id}. Reap it with `--done {id}`, gate it with `--pending-gate {id}`, resolve review with `--review-resolve {id}`, or keep/narrow it with `--pending-edit \"{id}=...\"`. (missing proof: no done/gate/review-resolve/reap recorded for #{id} this cycle)"
        ));
    }
    Ok(GENERIC.to_string())
}

/// Operator-facing closeout diagnostic that names the response predicate which
/// kept a free-text or resolved preset head queued.
pub fn queue_skip_diagnostic_for_response(content: &str, response: &str) -> Result<String> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return queue_skip_diagnostic_for_content(content);
    };
    let expansions = crate::queue_response::queue_prompt_preset_expansions(content, &queue_head);
    if expansions.is_empty() {
        return queue_skip_diagnostic_for_content(content);
    }
    let queue_head_display = display_queue_prompt_text(&queue_head);
    if crate::queue_response::response_defers_free_text_head(response, &queue_head) {
        return Ok(format!(
            "[queue] kept prompt-preset head `{queue_head_display}` because the response explicitly defers that quoted head; remove the adjacent deferral marker only after the work is complete."
        ));
    }
    if crate::queue_response::queue_head_answered_by_response(content, response, &queue_head) {
        return Ok(format!(
            "[queue] kept prompt-preset head `{queue_head_display}` even though its canonical response evidence matched; another closeout guard vetoed consumption."
        ));
    }
    Ok(format!(
        "[queue] kept prompt-preset head `{queue_head_display}` because the response supplied neither an exact `> **Queue prompt:**` echo nor every resolved preset expansion."
    ))
}

/// `#queueidtypo`: an id-backed head whose id is not an active tracked item,
/// where exactly one active tracked id is a near spelling of it.
///
/// Observed 2026-09-30 on `sampleorders.md`: the operator queued
/// `do [#event-adapter-impl-tourne9yx]` for backlog item
/// `#event-adapter-impl-tourneyx`. The kept-head diagnostic told the agent to
/// `--done event-adapter-impl-tourne9yx`, which names nothing, so the head sat
/// undrained while the loop skipped past it. Suggest only; never rewrite the
/// operator's line (`#qauthorder`). A unique match within edit distance 2, on
/// ids long enough that two edits are a typo rather than a different word.
pub fn untracked_head_id_suggestion(content: &str, id: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    if id.chars().count() < 6 {
        return None;
    }
    let tracked = crate::queue_continuation::active_tracked_ids(content);
    if tracked.contains(&id) {
        return None;
    }
    let mut near = tracked
        .into_iter()
        .filter(|candidate| edit_distance(&id, candidate) <= 2);
    let first = near.next()?;
    near.next().is_none().then_some(first)
}

/// Levenshtein distance over chars.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != *cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()]
}

/// True when a closeout flag in this cycle explicitly names the active
/// id-backed queue head.
pub fn queue_head_has_explicit_completion_signal(
    content: &str,
    completion_ids: &[String],
) -> Result<bool> {
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    let names_head = |raw: &str| {
        let id = raw.split_once('=').map(|(id, _)| id).unwrap_or(raw);
        normalize_done_id(id) == head_id
    };
    Ok(completion_ids.iter().any(|raw| names_head(raw)))
}

/// Collect every explicit closeout id spelling that can authorize queue-head
/// completion.
///
/// AUTHORIZATION ONLY (`#donestrikeextra`). `pending_edit` belongs here because
/// narrowing the active head's text proves this cycle addressed that head, which
/// is enough to let the head be consumed. It must NOT be used to decide which
/// ids get STRUCK — see [`explicit_queue_resolution_ids`].
pub fn explicit_queue_completion_ids(
    pending_done: &[String],
    pending_gate: &[String],
    pending_edit: &[String],
    review_resolve: &[String],
) -> Vec<String> {
    pending_done
        .iter()
        .chain(pending_gate.iter())
        .chain(pending_edit.iter())
        .chain(review_resolve.iter())
        .map(|raw| {
            raw.split_once('=')
                .map(|(id, _)| id)
                .unwrap_or(raw.as_str())
        })
        .map(str::to_string)
        .collect()
}

/// Ids this cycle actually RESOLVED, and may therefore strike from the queue.
///
/// `#donestrikeextra` — a single `--done A` alongside a `--backlog-edit B=...`
/// struck B's queue head while the B backlog item correctly stayed `[ ]` open,
/// so an unfinished item silently left the drain and the operator could only
/// notice by diffing the queue. The cause is scope: the strike path reused
/// [`explicit_queue_completion_ids`], whose whole job is the *authorization*
/// question ("did this cycle address the head?"), for the very different
/// *resolution* question ("which ids are finished?").
///
/// `--backlog-edit` is explicitly the "keep/narrow it" flag — the queue-skip
/// diagnostic offers it as the alternative to reaping — so an edited id is by
/// definition still open and must never be struck. Only `--done` (completed),
/// `--pending-gate` (code-complete, awaiting review), and `--review-resolve`
/// (review cleared) resolve an item.
pub fn explicit_queue_resolution_ids(
    pending_done: &[String],
    pending_gate: &[String],
    review_resolve: &[String],
) -> Vec<String> {
    pending_done
        .iter()
        .chain(pending_gate.iter())
        .chain(review_resolve.iter())
        .map(|raw| {
            raw.split_once('=')
                .map(|(id, _)| id)
                .unwrap_or(raw.as_str())
        })
        .map(str::to_string)
        .collect()
}

/// True when `done_ids` names the active id-backed queue head.
pub fn queue_head_matches_done_ids(content: &str, done_ids: &[String]) -> Result<bool> {
    if done_ids.is_empty() {
        return Ok(false);
    }
    let Some(queue_head) = active_queue_head_text(content)? else {
        return Ok(false);
    };
    let Some(head_id) = queue_prompt_done_id(&queue_head) else {
        return Ok(false);
    };
    Ok(done_ids.iter().any(|id| normalize_done_id(id) == head_id))
}

/// Normalized identity for matching free-text queue heads across queue rows and
/// response echoes. Priority markers are cosmetic and do not affect identity.
pub fn free_text_queue_head_identity(text: &str) -> String {
    strip_priority_markers(text).trim().to_ascii_lowercase()
}

/// True when the supplied document content still has an active free-text queue
/// prompt with the same identity as `head`.
pub fn committed_queue_contains_free_text_head(content: &str, head: &str) -> bool {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return false;
    };
    let Ok(Some(queue)) = crate::queue_set::selected_component(content, &components) else {
        return false;
    };
    let Ok(entries) = crate::document_queue::parse(queue.content(content)) else {
        return false;
    };
    let target = free_text_queue_head_identity(head);
    if target.is_empty() {
        return false;
    }
    crate::document_queue::prompts(&entries)
        .into_iter()
        .any(|prompt| {
            let text = prompt.text.trim();
            queue_prompt_text_is_free_text(content, text)
                && free_text_queue_head_identity(text) == target
        })
}

/// True when the committed queue still holds a free-text prompt that *extends*
/// `head`: the recorded head is a strict prefix of a longer queued prompt
/// (`#qheadcomposing`).
///
/// This is the shape of a head preflight recorded while the operator was still
/// typing it: the operator finished the line after the cycle started, so the
/// recorded fragment no longer matches any queued prompt exactly. The item was
/// not removed and the fragment's answer did not consume it; it stays queued.
pub fn committed_queue_extends_free_text_head(content: &str, head: &str) -> bool {
    let target = free_text_queue_head_identity(head);
    if target.is_empty() {
        return false;
    }
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return false;
    };
    let Ok(Some(queue)) = crate::queue_set::selected_component(content, &components) else {
        return false;
    };
    let Ok(entries) = crate::document_queue::parse(queue.content(content)) else {
        return false;
    };
    crate::document_queue::prompts(&entries)
        .into_iter()
        .any(|prompt| {
            let text = prompt.text.trim();
            let identity = free_text_queue_head_identity(text);
            queue_prompt_text_is_free_text(content, text)
                && identity.len() > target.len()
                && identity.starts_with(&target)
        })
}

/// True when a non-recurring free-text queue head is still queued even though
/// committed exchange text contains a queue-prompt response echo for it.
pub fn free_text_queue_head_is_completed_residue(
    content: &str,
    exchange_text: &str,
    head: &str,
) -> bool {
    free_text_queue_head_is_completed_residue_with_claim(content, exchange_text, head, false)
}

/// [`free_text_queue_head_is_completed_residue`] for a head an active worker
/// claim may hold (`#ftstrikeclaimedmention`). A claimed head is completed
/// residue only on an exact `> **Queue prompt:**` echo of it
/// ([`crate::queue_response::free_text_head_has_explicit_answer_evidence`]),
/// the same proof the claimed-head strike requires, so the residue set and the
/// strike set stay equal (`#claimstrike`).
pub fn free_text_queue_head_is_completed_residue_with_claim(
    content: &str,
    exchange_text: &str,
    head: &str,
    claimed: bool,
) -> bool {
    if crate::queue_continuation::is_recurring_imperative_head(head) {
        return false;
    }
    let answered = if claimed {
        crate::queue_response::free_text_head_has_explicit_answer_evidence(exchange_text, head)
    } else {
        free_text_head_answered_by_response(exchange_text, head)
    };
    committed_queue_contains_free_text_head(content, head)
        && answered
        // `#deferstrike`: an echo followed by a deferral keeps the head queued
        // on purpose; it is not completed residue.
        && !crate::queue_response::latest_free_text_head_echo_is_deferral(exchange_text, head)
}

fn queue_prompt_heads(doc: &str) -> Vec<String> {
    let Ok(components) = agent_doc_element::element::parse(doc) else {
        return Vec::new();
    };
    let Ok(Some(queue)) = crate::queue_set::selected_component(doc, &components) else {
        return Vec::new();
    };
    let Ok(entries) = crate::document_queue::parse(queue.content(doc)) else {
        return Vec::new();
    };
    crate::document_queue::prompts(&entries)
        .into_iter()
        .map(|prompt| prompt.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Optional-`do` grammar: a queue head that leads with a bare id token (`[#id]`
/// or `#id`) is id-backed. A trailing `:` (`[#id]: note`) keeps the line inert
/// as prose annotation rather than a directive.
fn leads_with_bare_id_directive(lower: &str) -> bool {
    let (rest, bracketed) = if let Some(r) = lower.strip_prefix("[#") {
        (r, true)
    } else if let Some(r) = lower.strip_prefix('#') {
        (r, false)
    } else {
        return false;
    };
    let id_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .count();
    if id_len == 0 {
        return false;
    }
    let after = &rest[id_len..];
    if bracketed {
        match after.strip_prefix(']') {
            Some(tail) => !tail.starts_with(':'),
            None => false,
        }
    } else {
        after.is_empty() || after.starts_with([' ', '\t', '.'])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_typoed_head_id_names_the_tracked_item_it_meant() {
        let content = "---\nqueue_active: true\n---\n\n<!-- agent:queue -->\n- do [#event-adapter-impl-tourne9yx]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->\n- [ ] [#event-adapter-impl-tourneyx] Implement the TourneyX adapter.\n- [ ] [#event-adapter-impl-wa-wdfw] Implement the WDFW adapter.\n<!-- /agent:backlog -->\n";
        assert_eq!(
            super::untracked_head_id_suggestion(content, "event-adapter-impl-tourne9yx").as_deref(),
            Some("event-adapter-impl-tourneyx")
        );
        let message = super::queue_skip_diagnostic_for_content(content).unwrap();
        assert!(
            message.contains("#event-adapter-impl-tourne9yx is not a tracked item"),
            "{message}"
        );
        assert!(
            message.contains("`--done event-adapter-impl-tourneyx`"),
            "{message}"
        );
        assert!(
            !message.contains("Reap it with `--done event-adapter-impl-tourne9yx`"),
            "{message}"
        );
    }

    #[test]
    fn a_tracked_or_ambiguous_head_id_gets_no_suggestion() {
        let content = "<!-- agent:backlog -->\n- [ ] [#adapter-one] A.\n- [ ] [#adapter-two] B.\n<!-- /agent:backlog -->\n";
        assert_eq!(
            super::untracked_head_id_suggestion(content, "adapter-one"),
            None
        );
        assert_eq!(
            super::untracked_head_id_suggestion(content, "adapter-onx"),
            Some("adapter-one".to_string())
        );
        assert_eq!(
            super::untracked_head_id_suggestion(content, "adapter-tw"),
            Some("adapter-two".to_string())
        );
        assert_eq!(
            super::untracked_head_id_suggestion(content, "adapter-xxx"),
            None,
            "two candidates at distance <= 3 but none <= 2 is no match"
        );
        let both = "<!-- agent:backlog -->\n- [ ] [#adapter-ab] A.\n- [ ] [#adapter-ac] B.\n<!-- /agent:backlog -->\n";
        assert_eq!(
            super::untracked_head_id_suggestion(both, "adapter-ad"),
            None,
            "ambiguous"
        );
        assert_eq!(
            super::untracked_head_id_suggestion(content, "ab"),
            None,
            "too short"
        );
    }

    use super::*;

    const HALT_QUEUE_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange -->\n",
        "### Re: #foo halt\n\nCannot complete it safely yet.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#foo]\n",
        "<!-- /agent:queue -->\n"
    );

    #[test]
    fn is_do_directive_accepts_do_and_bare_id_forms() {
        // Back-compat: explicit `do` forms.
        assert!(is_do_directive("do [#opt]"));
        assert!(is_do_directive("do #opt"));
        assert!(is_do_directive("DO [#opt]. trailing note"));
        assert!(is_do_directive("🚧 do [#opt]"));
        assert!(is_do_directive(":pushpin: 🚧 do [#opt]"));
        // Optional-`do`: bare id token is id-backed.
        assert!(is_do_directive("[#opt]"));
        assert!(is_do_directive("🚧 [#opt]"));
        assert!(is_do_directive("[#opt]. do the small fix"));
        assert!(is_do_directive("#opt"));
        assert!(is_do_directive("#opt do the thing"));
        // Inert: prose annotation (`[#id]:`), references, plain prose, headings.
        assert!(!is_do_directive("[#opt]: just a note"));
        assert!(!is_do_directive("#opt: just a note"));
        assert!(!is_do_directive("re [#opt]"));
        assert!(!is_do_directive("see [#opt] for context"));
        assert!(!is_do_directive("# heading"));
        assert!(!is_do_directive("just a free-text prompt"));
    }

    #[test]
    fn active_queue_heads_split_id_backed_and_free_text_prompts() {
        let doc = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:queue -->\n",
            "- do [#build]\n",
            "- 🚧 [#running]\n",
            "- [#bare] do the small fix\n",
            "- [#note]: this is annotation prose\n",
            "- 🚧 write an active status summary\n",
            "- write a status summary\n",
            "<!-- /agent:queue -->\n"
        );

        assert_eq!(
            active_queue_heads(doc),
            vec![
                "do [#build]".to_string(),
                "🚧 [#running]".to_string(),
                "[#bare] do the small fix".to_string(),
                "[#note]: this is annotation prose".to_string(),
                "🚧 write an active status summary".to_string(),
                "write a status summary".to_string(),
            ]
        );
        assert_eq!(
            active_free_text_queue_heads(doc),
            vec![
                "[#note]: this is annotation prose".to_string(),
                "write an active status summary".to_string(),
                "write a status summary".to_string(),
            ]
        );
    }

    #[test]
    fn active_queue_heads_tolerate_missing_or_malformed_queue() {
        assert!(active_queue_heads("plain document").is_empty());
        assert!(active_free_text_queue_heads("plain document").is_empty());
    }

    #[test]
    fn active_queue_head_text_requires_active_queue_and_returns_first_prompt() {
        assert_eq!(
            active_queue_head_text(HALT_QUEUE_DOC).unwrap(),
            Some("do [#foo]".to_string())
        );
        let inactive = HALT_QUEUE_DOC.replace("queue_active: true", "queue_active: false");
        assert_eq!(active_queue_head_text(&inactive).unwrap(), None);
    }

    #[test]
    fn active_queue_prompt_returns_first_active_auto_prompt() {
        let doc = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#build]\n",
            "- write the status note\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(active_queue_prompt(doc), Some("do [#build]".to_string()));
    }

    #[test]
    fn active_queue_prompt_honors_persisted_queue_active() {
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n",
            "- write the status note\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(
            active_queue_prompt(doc),
            Some("write the status note".to_string())
        );
    }

    #[test]
    fn active_queue_prompt_strips_in_progress_marker() {
        let doc = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue -->\n",
            "- 🚧 write the status note\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(
            active_queue_prompt(doc),
            Some("write the status note".to_string())
        );
    }

    #[test]
    fn active_queue_prompt_returns_none_for_inactive_queue() {
        let doc = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "- write the status note\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(active_queue_prompt(doc), None);
    }

    #[test]
    fn queue_is_explicitly_stopped_distinguishes_stop_from_default_inactive() {
        let stopped = "---\nqueue: pause\n---\n\n<!-- agent:queue -->\n- #advance-review\n<!-- /agent:queue -->\n";
        // `#queuestopremove`: legacy `stop` shapes are no control, not a hold.
        let legacy_stopped = "---\nqueue_active: false\n---\n\n<!-- agent:queue -->\n- #advance-review\n<!-- /agent:queue -->\n";
        let retired_stop = "---\nqueue: stop\n---\n\n<!-- agent:queue -->\n- #advance-review\n<!-- /agent:queue -->\n";
        let default_inactive = "---\nsession: sid\n---\n\n<!-- agent:queue -->\n- #advance-review\n<!-- /agent:queue -->\n";
        let active = "---\nqueue: go\n---\n\n<!-- agent:queue -->\n- #advance-review\n<!-- /agent:queue -->\n";

        assert!(queue_is_explicitly_stopped(stopped));
        assert!(!queue_is_explicitly_stopped(legacy_stopped));
        assert!(!queue_is_explicitly_stopped(retired_stop));
        assert!(!queue_is_explicitly_stopped(default_inactive));
        assert!(!queue_is_explicitly_stopped(active));
    }

    #[test]
    fn queue_is_active_for_diff_accepts_exchange_trigger() {
        let doc = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "- write the status note\n",
            "<!-- /agent:queue -->\n",
        );
        let diff = concat!(
            "diff --git a/tasks/doc.md b/tasks/doc.md\n",
            "@@\n",
            "+do queue\n",
        );

        assert!(queue_is_active_for_diff(doc, diff));
    }

    #[test]
    fn queue_is_active_for_diff_returns_false_for_missing_queue() {
        let diff = concat!(
            "diff --git a/tasks/doc.md b/tasks/doc.md\n",
            "@@\n",
            "+do queue\n",
        );

        assert!(!queue_is_active_for_diff("plain document", diff));
    }

    #[test]
    fn classify_active_queue_head_distinguishes_free_text_and_id_backed() {
        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- answer this question\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            classify_active_queue_head(free_text).unwrap(),
            ActiveQueueHeadKind::FreeText
        );

        let id_backed = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#admin-recover] Fix it\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue go -->\n",
            "- [#admin-recover]\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            classify_active_queue_head(id_backed).unwrap(),
            ActiveQueueHeadKind::IdBacked
        );

        let product_name_prose = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- sampleorders.md has queue items for already done tasks. ",
            "Please investigate why agent-doc did not strike the done items and whether ",
            "the cause(s) were fixed in previous versions or not. Fix all remaining ",
            "contributing factors.\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            classify_active_queue_head(product_name_prose).unwrap(),
            ActiveQueueHeadKind::FreeText,
            "a hyphenated product name in prose is not an id-backed directive"
        );
    }

    #[test]
    fn classify_active_queue_head_preserves_inactive_queue_fail_closed_behavior() {
        let inactive = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- answer this later\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            classify_active_queue_head(inactive).unwrap(),
            ActiveQueueHeadKind::Inactive
        );
    }

    #[test]
    fn explicit_completion_signal_names_active_queue_head() {
        assert!(!queue_head_has_explicit_completion_signal(HALT_QUEUE_DOC, &[]).unwrap());
        assert!(
            queue_head_has_explicit_completion_signal(HALT_QUEUE_DOC, &["foo".to_string()])
                .unwrap()
        );
        assert!(
            queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &["foo=rewritten text".to_string()],
            )
            .unwrap()
        );
        assert!(
            !queue_head_has_explicit_completion_signal(
                HALT_QUEUE_DOC,
                &[
                    "bar".to_string(),
                    "baz".to_string(),
                    "qux=text".to_string(),
                    "other-review".to_string(),
                ],
            )
            .unwrap()
        );
        let inactive = HALT_QUEUE_DOC.replace("queue_active: true", "queue_active: false");
        assert!(
            !queue_head_has_explicit_completion_signal(&inactive, &["foo".to_string()]).unwrap()
        );
    }

    #[test]
    fn explicit_queue_completion_ids_strip_edit_payloads() {
        assert_eq!(
            explicit_queue_completion_ids(
                &["done".to_string()],
                &["gate".to_string()],
                &["edit=rewritten".to_string()],
                &["review".to_string()],
            ),
            vec![
                "done".to_string(),
                "gate".to_string(),
                "edit".to_string(),
                "review".to_string(),
            ]
        );
    }

    /// `#donestrikeextra` — the exact regression the bug report asks to pin:
    /// `--backlog-edit` on id B must never put B in the strike set when only A
    /// is `--done`.
    #[test]
    fn explicit_queue_resolution_ids_exclude_backlog_edits() {
        let resolution = explicit_queue_resolution_ids(&["fzmutloss".to_string()], &[], &[]);
        assert_eq!(resolution, vec!["fzmutloss".to_string()]);
        assert!(
            !resolution.contains(&"patchretryidem".to_string()),
            "an edited-but-open id must never be struck from the queue"
        );

        // The authorization set still sees the edit — that scope is unchanged.
        let authorization = explicit_queue_completion_ids(
            &["fzmutloss".to_string()],
            &[],
            &["patchretryidem=rewritten".to_string()],
            &[],
        );
        assert!(authorization.contains(&"patchretryidem".to_string()));
    }

    /// Gate and review-resolve DO resolve an item, so they stay in the strike
    /// set: a gated `[/]` item is code-complete and legitimately leaves the drain.
    #[test]
    fn explicit_queue_resolution_ids_keep_gate_and_review_resolve() {
        assert_eq!(
            explicit_queue_resolution_ids(
                &["done".to_string()],
                &["gate".to_string()],
                &["review".to_string()],
            ),
            vec!["done".to_string(), "gate".to_string(), "review".to_string(),]
        );
    }

    #[test]
    fn queue_head_matches_done_ids_compares_normalized_ids() {
        assert!(queue_head_matches_done_ids(HALT_QUEUE_DOC, &[" [#Foo] ".to_string()]).unwrap());
        assert!(!queue_head_matches_done_ids(HALT_QUEUE_DOC, &["bar".to_string()]).unwrap());
        assert!(!queue_head_matches_done_ids(HALT_QUEUE_DOC, &[]).unwrap());
    }

    #[test]
    fn committed_queue_contains_free_text_head_matches_cosmetic_markers_and_case() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- :pushpin: Explain The Queue Churn\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(
            free_text_queue_head_identity(":pushpin: Explain The Queue Churn"),
            "explain the queue churn"
        );
        assert!(committed_queue_contains_free_text_head(
            doc,
            "explain the queue churn"
        ));
        assert!(committed_queue_contains_free_text_head(
            doc,
            ":pushpin: explain the queue churn"
        ));
    }

    #[test]
    fn committed_queue_contains_free_text_head_rejects_id_backed_and_trigger_heads() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#build] build it\n",
            "- [ ] [#bare] bare id work\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#build]\n",
            "- [#bare]\n",
            "- do queue\n",
            "<!-- /agent:queue -->\n",
        );

        assert!(!committed_queue_contains_free_text_head(doc, "do [#build]"));
        assert!(!committed_queue_contains_free_text_head(doc, "[#bare]"));
        assert!(!committed_queue_contains_free_text_head(doc, "do queue"));
    }

    #[test]
    fn free_text_queue_head_is_completed_residue_detects_answered_active_head() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- explain the queue churn\n",
            "<!-- /agent:queue -->\n",
        );
        let exchange = concat!(
            "### Re: explain the queue churn\n\n",
            "> **Queue prompt:**\n>\n> explain the queue churn\n\n",
            "The churn comes from stale convergence.\n",
        );

        assert!(free_text_queue_head_is_completed_residue(
            doc,
            exchange,
            "explain the queue churn"
        ));
    }

    #[test]
    fn deferstrike_latest_deferred_echo_is_not_completed_residue() {
        // A non-imperative head: `release + publish` is already exempt as a
        // recurring imperative, which would make this test vacuous.
        let head = "close the GH 127 128 129 issues after integration lands";
        let content = format!("<!-- agent:queue go -->\n- {head}\n<!-- /agent:queue -->\n");
        let exchange_deferred = format!(
            "> **Queue prompt:** {head}\n\n\
             Dispatched to a subagent; it waits for the integration batch. Keep this head open.\n"
        );
        assert!(crate::queue_response::free_text_head_answered_by_response(
            &exchange_deferred,
            head
        ));
        assert!(!free_text_queue_head_is_completed_residue(
            &content,
            &exchange_deferred,
            head
        ));
        // A later cycle that answers the head makes it residue again.
        let exchange_answered = format!(
            "{exchange_deferred}\n> **Queue prompt:** {head}\n\nClosed all three after the merge.\n"
        );
        assert!(free_text_queue_head_is_completed_residue(
            &content,
            &exchange_answered,
            head
        ));
    }

    #[test]
    fn free_text_queue_head_is_completed_residue_exempts_recurring_imperative() {
        let doc = concat!(
            "<!-- agent:queue -->\n",
            "- deploy\n",
            "<!-- /agent:queue -->\n",
        );
        let exchange = concat!(
            "### Re: deploy\n\n",
            "> **Queue prompt:**\n>\n> deploy\n\n",
            "Deployment completed.\n",
        );

        assert!(!free_text_queue_head_is_completed_residue(
            doc, exchange, "deploy"
        ));
    }

    #[test]
    fn queue_skip_diagnostic_names_head_shape_and_repair_path() {
        let id_message = queue_skip_diagnostic_for_content(HALT_QUEUE_DOC).unwrap();
        assert!(id_message.contains("[queue] kept head `do #foo`"));
        assert!(id_message.contains("`--done foo`"));
        assert!(id_message.contains("`--pending-gate foo`"));
        assert!(id_message.contains("`--review-resolve foo`"));
        assert!(id_message.contains("`--pending-edit \"foo=...\"`"));
        assert!(id_message.contains("missing proof"));

        let free_text = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue auto -->\n",
            "- Review the queue diagnostics\n",
            "<!-- /agent:queue -->\n",
        );
        let free_text_message = queue_skip_diagnostic_for_content(free_text).unwrap();
        assert!(
            free_text_message
                .contains("[queue] kept free-text head `Review the queue diagnostics`")
        );
        assert!(free_text_message.contains("`> **Queue prompt:**` echo"));
    }

    #[test]
    fn prompt_preset_skip_diagnostic_names_the_actual_response_contract() {
        let content = concat!(
            "---\nqueue_active: true\nprompt_presets:\n",
            "  '#upgrade': Upgrade agent-doc and check the current issues.\n",
            "---\n\n",
            "<!-- agent:queue go -->\n",
            "- #upgrade\n",
            "<!-- /agent:queue -->\n",
        );
        let missing = queue_skip_diagnostic_for_response(
            content,
            "### Re: status\n\nChecked an unrelated item.\n",
        )
        .unwrap();
        assert!(missing.contains("kept prompt-preset head `#upgrade`"));
        assert!(missing.contains("exact `> **Queue prompt:**` echo"));
        assert!(missing.contains("resolved preset expansion"));
        assert!(!missing.contains("Add a `> **Queue prompt:**` echo"));

        let deferred = queue_skip_diagnostic_for_response(
            content,
            concat!(
                "> **Queue prompt:** #upgrade\n\n",
                "**Deferred:** waiting for the release build.\n",
            ),
        )
        .unwrap();
        assert!(deferred.contains("explicitly defers that quoted head"));
    }

    #[test]
    fn active_head_advances_across_dependent_queue_blocks() {
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue id=release-a -->\n",
            "~~- publish A~~\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b after=release-a subagents -->\n",
            "- organize B\n",
            "<!-- /agent:queue -->\n",
        );
        assert_eq!(
            active_queue_head_text(content).unwrap().as_deref(),
            Some("organize B")
        );
        assert_eq!(active_queue_prompt(content).as_deref(), Some("organize B"));
        assert_eq!(active_queue_heads(content), vec!["organize B"]);
    }
}
