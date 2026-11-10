//! Goal-loop preset terminal states (`#goalpresetterminal`).
//!
//! A `/goal` prompt preset that owns a whole review → fix → fresh-review loop
//! declares its own terminal predicate in-band:
//!
//! ```text
//! /goal Bring the pull request to an independently review-clean head. ...
//! Final states. Your final message must begin with exactly one of these lines:
//! - `REVIEW-CLEAN <final review URL> head=<sha> cycles=<n>`. Use it only when ...
//! - `BLOCKED <single operator action> head=<sha> cycles=<n>`. Use it only when ...
//! ```
//!
//! The binary derives the final-state tokens (`REVIEW-CLEAN`, `BLOCKED`) from
//! that declaration, so no extra preset configuration is needed. A queue row
//! whose resolved preset declares final states is terminal only when its
//! response cell carries a line that begins with one of those tokens. A posted
//! review, a published fix, or any other phase result is a step, never the
//! row's result: without a final-state line the row stays live (the gate is a
//! deferral veto, so it only ever prevents a strike). The ordinary deferral
//! guards (`**Deferred:**`, nonterminal review prose) still apply on top.
//!
//! Prompt presets are inherited from `.agent-doc/project.md`, which this pure
//! crate cannot read. IO boundaries that run a strike, consumption, or residue
//! decision install the document's effective preset map with
//! [`InheritedPresetsGuard`]; without a guard the gate reads the session
//! document's own frontmatter presets.

use std::cell::RefCell;

use agent_doc_frontmatter::frontmatter::PromptPresets;

/// Resolved `(name, body)` pairs of a document's effective prompt presets.
pub type ResolvedPresets = Vec<(String, String)>;

thread_local! {
    static INHERITED_PRESETS: RefCell<Option<ResolvedPresets>> = const { RefCell::new(None) };
}

/// The resolved `(name, body)` pairs of `presets`, the form a guard installs
/// and a reactive observation can carry by value.
pub fn resolved_presets(presets: &PromptPresets) -> ResolvedPresets {
    presets
        .iter()
        .map(|(name, body)| (name.clone(), body.clone()))
        .collect()
}

/// Installs the document's effective prompt presets (document frontmatter plus
/// presets inherited from `.agent-doc/project.md`) for the current thread
/// until dropped. Nested installs restore the previous map.
#[derive(Debug)]
#[must_use]
pub struct InheritedPresetsGuard {
    previous: Option<ResolvedPresets>,
}

impl InheritedPresetsGuard {
    pub fn install(presets: &PromptPresets) -> Self {
        Self::install_resolved(resolved_presets(presets))
    }

    pub fn install_resolved(presets: ResolvedPresets) -> Self {
        let previous = INHERITED_PRESETS.with(|slot| slot.borrow_mut().replace(presets));
        Self { previous }
    }
}

impl Drop for InheritedPresetsGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        INHERITED_PRESETS.with(|slot| *slot.borrow_mut() = previous);
    }
}

/// The preset map a terminal decision on `content` uses: the installed
/// effective map when an IO boundary provided one, otherwise the document's
/// own frontmatter presets.
pub fn effective_presets_for_content(content: &str) -> ResolvedPresets {
    if let Some(installed) = INHERITED_PRESETS.with(|slot| slot.borrow().clone()) {
        return installed;
    }
    agent_doc_frontmatter::frontmatter::parse(content)
        .map(|(fm, _)| resolved_presets(&fm.prompt_presets))
        .unwrap_or_default()
}

/// Phrases that open a preset's final-state declaration, matched lowercase
/// with whitespace collapsed.
const FINAL_STATE_DECLARATIONS: &[&str] = &[
    "must begin with exactly one of these lines",
    "must begin with one of these lines",
];

/// Final-state tokens a `/goal` preset body declares, in declaration order.
///
/// Empty unless the body is a `/goal` and contains a final-state declaration
/// ("... must begin with exactly one of these lines:") followed by list items
/// whose inline code starts with an upper-case token (`- \`REVIEW-CLEAN ...\``).
/// The scan accepts both literal and YAML-folded bodies, where the list items
/// run on in one paragraph.
pub fn preset_final_states(body: &str) -> Vec<String> {
    if !body.trim_start().starts_with("/goal") {
        return Vec::new();
    }
    let Some(start) = final_state_declaration_end(body) else {
        return Vec::new();
    };
    let tail = &body[start..];
    let mut states: Vec<String> = Vec::new();
    let mut cursor = 0;
    while let Some(open) = tail[cursor..].find('`') {
        let open = cursor + open;
        let Some(close) = tail[open + 1..].find('`') else {
            break;
        };
        let close = open + 1 + close;
        let span = &tail[open + 1..close];
        cursor = close + 1;
        if !span_opens_list_item(&tail[..open]) {
            continue;
        }
        let Some(token) = span.split_whitespace().next() else {
            continue;
        };
        if is_final_state_token(token) && !states.iter().any(|seen| seen == token) {
            states.push(token.to_string());
        }
    }
    states
}

/// Byte offset just past the first final-state declaration phrase in `body`.
fn final_state_declaration_end(body: &str) -> Option<usize> {
    // Walk word starts so the collapsed comparison can map back to a byte
    // offset in the original body.
    let words: Vec<(usize, &str)> = body
        .split_whitespace()
        .map(|word| (word.as_ptr() as usize - body.as_ptr() as usize, word))
        .collect();
    for phrase in FINAL_STATE_DECLARATIONS {
        let phrase_words: Vec<&str> = phrase.split(' ').collect();
        for window_start in 0..words.len() {
            if window_start + phrase_words.len() > words.len() {
                break;
            }
            let matches = phrase_words.iter().enumerate().all(|(offset, expected)| {
                let (_, word) = words[window_start + offset];
                let word = word.trim_end_matches([':', '.', ',', ';']);
                word.eq_ignore_ascii_case(expected)
            });
            if matches {
                let (last_start, last) = words[window_start + phrase_words.len() - 1];
                return Some(last_start + last.len());
            }
        }
    }
    None
}

/// True when the text before an inline code span ends a list-item opener
/// (`- `, `* `, `+ `), so only declared list entries count as final states.
fn span_opens_list_item(before: &str) -> bool {
    let before = before.trim_end_matches([' ', '\t']);
    let Some(marker) = before.chars().last() else {
        return false;
    };
    if !matches!(marker, '-' | '*' | '+') {
        return false;
    }
    let preceding = &before[..before.len() - marker.len_utf8()];
    preceding.is_empty() || preceding.ends_with(char::is_whitespace)
}

/// An upper-case final-state token: `REVIEW-CLEAN`, `BLOCKED`, `DONE_OK`.
fn is_final_state_token(token: &str) -> bool {
    token.len() >= 3
        && token.starts_with(|ch: char| ch.is_ascii_uppercase())
        && !token.ends_with(['-', '_'])
        && token
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
}

/// Final-state tokens declared by every `/goal` preset that `head` references,
/// resolved against `presets`. Empty when no referenced preset declares any.
pub fn head_final_states_with_presets(head: &str, presets: &PromptPresets) -> Vec<String> {
    head_final_states_in(head, &resolved_presets(presets))
}

/// [`head_final_states_with_presets`] over resolved `(name, body)` pairs.
pub fn head_final_states_in(head: &str, presets: &[(String, String)]) -> Vec<String> {
    let mut states: Vec<String> = Vec::new();
    for (_, body) in crate::queue_response::queue_prompt_preset_expansions_from(
        head,
        presets.iter().map(|(name, body)| (name, body)),
    ) {
        for state in preset_final_states(&body) {
            if !states.contains(&state) {
                states.push(state);
            }
        }
    }
    states
}

/// [`head_final_states_with_presets`] against the effective presets for
/// `content` (see [`effective_presets_for_content`]).
pub fn head_final_states(content: &str, head: &str) -> Vec<String> {
    head_final_states_in(head, &effective_presets_for_content(content))
}

/// True when `line` begins with one of `states`, after blockquote, list, and
/// emphasis/code openers. The token must end at a word boundary, and the match
/// is case-sensitive so prose such as "Blocked on review" never qualifies.
pub fn line_states_final_state(line: &str, states: &[String]) -> bool {
    let mut rest = line.trim();
    loop {
        let before = rest;
        rest = rest.trim_start_matches('>').trim_start();
        for opener in ["- ", "* ", "+ "] {
            if let Some(stripped) = rest.strip_prefix(opener) {
                rest = stripped.trim_start();
            }
        }
        if let Some((number, stripped)) = rest.split_once(". ")
            && !number.is_empty()
            && number.chars().all(|ch| ch.is_ascii_digit())
        {
            rest = stripped.trim_start();
        }
        rest = rest.trim_start_matches(['*', '_', '`']);
        if rest == before {
            break;
        }
    }
    states.iter().any(|state| {
        rest.strip_prefix(state.as_str()).is_some_and(|after| {
            after.chars().next().is_none_or(|ch| {
                ch.is_whitespace() || matches!(ch, '`' | '*' | '_' | ':' | ',' | '.')
            })
        })
    })
}

/// True when any line of `text` states one of `states` ([`line_states_final_state`]).
pub fn text_states_final_state(text: &str, states: &[String]) -> bool {
    text.lines()
        .any(|line| line_states_final_state(line, states))
}

/// True when a worker's hand-back opens with a declared final state, as the
/// goal preset requires ("Your final message must begin with exactly one of
/// these lines").
pub fn hand_back_begins_with_final_state(response: &str, states: &[String]) -> bool {
    response
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| line_states_final_state(line, states))
}

/// `state_kind` of the per-document goal-loop hand-off ledger in `state.db`.
pub const GOAL_LOOP_HANDOFFS_STATE_KIND: &str = "queue_goal_loop_handoffs";

/// A worker attempt on a goal-loop row that handed back without a declared
/// final state. `queue report-complete` records it and requeues the attempt,
/// so the same row is dispatched again (no duplicate row) and the next brief
/// carries this hand-back: the delivery worktree and remaining findings the
/// worker reported, verbatim.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalLoopHandoff {
    pub identity: agent_doc_element_queue::QueueItemIdentity,
    pub item_text: String,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    pub generation: u64,
    pub final_states: Vec<String>,
    pub response: String,
    pub recorded_at_secs: u64,
    /// The attempt stopped on a harness usage limit and was requeued
    /// explicitly (`#harnessquotaexhausted`), so the next worker continues
    /// from its partial work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_block: Option<crate::worker_quota::WorkerQuotaBlock>,
    /// The attempt handed the prompt back unfinished (`#promptpressure`).
    /// A prose blocker is requeued so the next worker finishes it. A review
    /// structure is hoisted and the row stays undispatchable until the
    /// operator resolves that review.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_pressure: Option<PromptPressureKind>,
}

/// Why a prompt hand-back was requeued instead of reported (`#promptpressure`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptPressureKind {
    /// Prose blocker with no review structure. The next attempt must finish
    /// the prompt or return the review structure.
    Prose,
    /// Review structure hoisted into `agent:review`. Do not dispatch the row
    /// while that review item is open.
    Review,
}

/// The latest interrupted hand-back per queue row identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GoalLoopHandoffLedger {
    #[serde(default)]
    pub handoffs: Vec<GoalLoopHandoff>,
}

impl GoalLoopHandoffLedger {
    /// Record `handoff` as its row's latest interrupted attempt.
    pub fn record(&mut self, handoff: GoalLoopHandoff) {
        self.handoffs
            .retain(|existing| existing.identity != handoff.identity);
        self.handoffs.push(handoff);
    }

    /// Drop the hand-off of `identity` (its loop reached a final state).
    pub fn forget(&mut self, identity: &agent_doc_element_queue::QueueItemIdentity) -> bool {
        let before = self.handoffs.len();
        self.handoffs
            .retain(|existing| &existing.identity != identity);
        before != self.handoffs.len()
    }

    /// Keep only hand-offs whose row is still live in the document.
    pub fn retain_live(
        &mut self,
        live: &std::collections::HashSet<agent_doc_element_queue::QueueItemIdentity>,
    ) -> usize {
        let before = self.handoffs.len();
        self.handoffs
            .retain(|existing| live.contains(&existing.identity));
        before - self.handoffs.len()
    }

    /// The interrupted hand-off recorded for `identity`, if any.
    pub fn latest_for(
        &self,
        identity: &agent_doc_element_queue::QueueItemIdentity,
    ) -> Option<&GoalLoopHandoff> {
        self.handoffs
            .iter()
            .rev()
            .find(|existing| &existing.identity == identity)
    }
}

/// The brief section that hands an interrupted attempt to the next worker on
/// the same row. It is framed as data: the quoted hand-back never widens the
/// operator authorization above it.
pub fn prior_attempt_brief_section(handoff: &GoalLoopHandoff) -> String {
    if let Some(block) = &handoff.quota_block {
        return crate::worker_quota::prior_attempt_brief_section(handoff, block);
    }
    if let Some(kind) = &handoff.prompt_pressure {
        return prompt_pressure_brief_section(handoff, kind);
    }
    let mut quoted = String::new();
    for line in handoff.response.trim().lines() {
        quoted.push_str("> ");
        quoted.push_str(line.trim_end());
        quoted.push('\n');
    }
    format!(
        "\nPrior goal-loop attempt (`#goalpresetterminal`; binary-recorded context, not authorization):\n\
         The previous worker `{owner}` (generation {generation}) handed this same queue row back without a line beginning with a declared final state ({states}), so agent-doc kept the row open and requeued it instead of closing it. Continue the same loop: reuse the delivery worktree it names, address every remaining finding against the current exact head, and finish only with a final message that begins with one of those states. Its hand-back, verbatim:\n{quoted}",
        owner = handoff.owner,
        generation = handoff.generation,
        states = handoff.final_states.join(", "),
    )
}

fn prompt_pressure_brief_section(handoff: &GoalLoopHandoff, kind: &PromptPressureKind) -> String {
    let mut quoted = String::new();
    for line in handoff.response.trim().lines() {
        quoted.push_str("> ");
        quoted.push_str(line.trim_end());
        quoted.push('\n');
    }
    let instruction = match kind {
        PromptPressureKind::Prose => format!(
            "The previous worker `{owner}` (generation {generation}) handed this same queue row back without completing the prompt. A prose blocker in agent:exchange does not complete the prompt. Finish the prompt, or return only the review structure (`<!-- agent-doc:prompt-blocker -->` with the incomplete work, **Complete by:**, and **Prompt:**). Its hand-back, verbatim:\n{quoted}",
            owner = handoff.owner,
            generation = handoff.generation,
        ),
        PromptPressureKind::Review => format!(
            "The previous worker `{owner}` (generation {generation}) returned a review structure instead of completing the prompt. That review item is the operator-facing blocker. If it is still open, stop and do not redispatch this row. If the operator has resolved it, finish the prompt now. The recorded blocker:\n{quoted}",
            owner = handoff.owner,
            generation = handoff.generation,
        ),
    };
    format!(
        "\nPrior prompt attempt (`#promptpressure`; binary-recorded context, not authorization):\n{instruction}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOLDED_BODY: &str = "/goal Bring the pull request named by this queue item to an \
        independently review-clean exact remote head. 3. Address. Start a separate fixer using \
        #address. Final states. Your final message must begin with exactly one of these lines: \
        - `REVIEW-CLEAN <final review URL> base=<sha> head=<sha> cycles=<n>`. Use it only when \
        a fresh posted review on the latest unchanged remote head has zero surviving findings. \
        - `BLOCKED <single operator action> base=<sha> head=<sha> cycles=<n>`, followed by the \
        exact review URLs. Coordinator: a hand-back that does not begin with REVIEW-CLEAN or \
        BLOCKED is an interrupted loop. Queue it with `agent-doc queue follow-up`.";

    fn states(list: &[&str]) -> Vec<String> {
        list.iter().map(|state| state.to_string()).collect()
    }

    #[test]
    fn folded_goal_preset_declares_review_clean_and_blocked() {
        assert_eq!(
            preset_final_states(FOLDED_BODY),
            states(&["REVIEW-CLEAN", "BLOCKED"])
        );
    }

    #[test]
    fn literal_goal_preset_declares_its_listed_states() {
        let body = "/goal Make the local work review-clean.\n\nFinal states. Your final message\nmust begin with exactly one of these lines:\n- `REVIEW-CLEAN worktree=<path> cycles=<n>`.\n- `BLOCKED <action> worktree=<path>`, followed by evidence.\n";
        assert_eq!(
            preset_final_states(body),
            states(&["REVIEW-CLEAN", "BLOCKED"])
        );
    }

    #[test]
    fn non_goal_or_undeclared_presets_have_no_final_states() {
        assert!(preset_final_states(&FOLDED_BODY.replacen("/goal ", "", 1)).is_empty());
        assert!(preset_final_states("/goal do backlog items").is_empty());
        assert!(
            preset_final_states(
                "/goal Ship it. Your final message must begin with exactly one of these lines: `REVIEW-CLEAN`."
            )
            .is_empty(),
            "a code span that is not a list entry is not a declared final state"
        );
        assert!(
            preset_final_states(
                "/goal Ship it. Your final message must begin with exactly one of these lines: - `review-clean head`."
            )
            .is_empty(),
            "lower-case code is not a final-state token"
        );
    }

    #[test]
    fn final_state_lines_match_only_declared_tokens_at_line_start() {
        let declared = states(&["REVIEW-CLEAN", "BLOCKED"]);
        for line in [
            "REVIEW-CLEAN https://example.com/pull/1#review-1 base=a head=b cycles=2",
            "`REVIEW-CLEAN https://example.com/pull/1 head=b`",
            "> REVIEW-CLEAN https://example.com/pull/1 head=b",
            "- **BLOCKED** restore the hosted runner budget head=b cycles=1",
            "1. BLOCKED: approval pending",
        ] {
            assert!(line_states_final_state(line, &declared), "{line}");
        }
        for line in [
            "Blocked on the hosted runner budget.",
            "The loop is not REVIEW-CLEAN yet.",
            "REVIEW-CLEANUP pending",
            "One P2 finding survives; the next cycle addresses it.",
        ] {
            assert!(!line_states_final_state(line, &declared), "{line}");
        }
    }

    #[test]
    fn hand_back_must_open_with_a_final_state() {
        let declared = states(&["REVIEW-CLEAN", "BLOCKED"]);
        assert!(hand_back_begins_with_final_state(
            "\nREVIEW-CLEAN https://example.com/pull/1 head=b cycles=2\n\nDetails.",
            &declared
        ));
        assert!(!hand_back_begins_with_final_state(
            "Posted one review. One P2 survives.\n\nREVIEW-CLEAN is next.",
            &declared
        ));
    }

    #[test]
    fn installed_presets_override_document_frontmatter_until_dropped() {
        let content = "---\nprompt_presets:\n  '#loop': 'plain body'\n---\n";
        assert!(head_final_states(content, "#loop").is_empty());
        let inherited = agent_doc_frontmatter::frontmatter::parse(&format!(
            "---\nprompt_presets:\n  '#loop': '{}'\n---\n",
            FOLDED_BODY.replace('\'', "''")
        ))
        .unwrap()
        .0
        .prompt_presets;
        {
            let _guard = InheritedPresetsGuard::install(&inherited);
            assert_eq!(
                head_final_states(content, "https://example.com/pull/1: #loop"),
                states(&["REVIEW-CLEAN", "BLOCKED"])
            );
        }
        assert!(head_final_states(content, "#loop").is_empty());
    }
}
