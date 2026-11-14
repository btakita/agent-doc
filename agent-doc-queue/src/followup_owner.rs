//! `#humanfollowupowner`: who owns a worker hand-back's action item.
//!
//! `queue report-complete` turns each **Action items** bullet into a follow-up
//! row (`follow up on an action item from …: <item>`,
//! [`crate::handback_followups`]). A row with no owner reads as agent work: it
//! paints `📥`, a `go` queue dispatches it with no review, and the Stop hook's
//! `#transientcloseout` gate refuses every turn end while it is pending. Many
//! action items are not agent work at all: another reviewer approving a pull
//! request, an org owner or repo admin changing a setting, an operator
//! decision. No agent can close those rows, so the session looped forever
//! (haiven-dev `tasks/sdk/sdk-dev.md`, 2026-11-13: 39 of 41 pending rows).
//!
//! # Owner
//!
//! A worker labels each bullet with its owner (the subagent brief asks for
//! it): `agent: …` is agent work, `operator: …`, `human: …`, and
//! `human (<who>): …` are not. A label may be bold (`**human (tmstorm):**`).
//! The label stays in the row text, so the owner the worker wrote is the one
//! every later reader sees.
//!
//! An unlabelled bullet (every hand-back written before the label existed) is
//! classified by [`human_owner_signal`]. The classifier is deliberately
//! conservative in one direction: any phrase that names an action only a
//! person can take makes the bullet human-owned. Misreading agent work as
//! human parks one row where the operator sees it and can hand it back;
//! misreading human work as agent work is the loop this module exists to end.
//! A word that is often the agent's own subject matter (`confirm`,
//! `authorization`, `approval`, `login`, `password`) counts only in a
//! human-action position (`Approve …` as an instruction, `an approval`,
//! `not authorized`, `operator authorization`, `approval required`, `must
//! authorize`, `account login`, `password prompt`).
//!
//! # Representation
//!
//! A human-owned row carries the existing `[operator-verify]` execution tag
//! after its work text (and before the binary's trailing `[#q-…]` id). That
//! tag already means "no agent drain scope runs this": free-text heads parse
//! it inline (`#ftimmediate`), so the row is never drainable or dispatchable,
//! preflight projects `⏳` awaiting operator onto it instead of `📥`
//! (`#queueawaitop`), and the `#transientcloseout` gate treats a pending row
//! that carries it as waiting on the operator (`#stopquiescentrows`). The
//! operator strikes the row once the action is done, or answers it inline
//! (`…: done`, `#opverifyanswered`) to hand the remaining work back to an
//! agent. The tag lives in the work text, so it is durable; the `⏳`/`📥`
//! glyph is only projection.
//!
//! # Existing documents
//!
//! [`reclassify_human_followup_rows`] tags the live, unstarted
//! `follow up on an action item` rows whose item is human-owned, in place and
//! idempotently. `agent-doc queue reclassify-followups <FILE>` runs it on
//! demand. Preflight queue maintenance runs it on every cycle, but tags each
//! row at most once: it records the [`followup_owner_key`] of every row that
//! carries the tag ([`followup_row_keys`]), and
//! [`reclassify_unseen_human_followup_rows`] skips a recorded row, so an
//! operator who deletes `[operator-verify]` to hand a row back to an agent is
//! not overruled on the next cycle. A row classified as agent work is never
//! recorded, so each cycle classifies it again and a later vocabulary fix
//! still reaches it.

use std::collections::HashSet;

use anyhow::Result;

use agent_doc_document::queue_projection::{self, QueueLifecycleMarker};
use agent_doc_element_backlog::backlog;

/// The work-text prefix of every row `report-complete` writes for an action
/// item ([`crate::handback_followups::action_item_followup_text`]).
pub const FOLLOWUP_ROW_PREFIX: &str = "follow up on an action item from ";

/// The execution tag a human-owned follow-up row carries.
pub const OPERATOR_OWNED_TAG: &str = backlog::OPERATOR_VERIFY_TAG;

/// Who owns an action item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionItemOwner {
    /// An agent can do it; the follow-up row stays dispatchable (`📥`).
    Agent,
    /// Only a person can do it; the row waits on the operator (`⏳`).
    Human,
}

/// Why an action item was classified as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerEvidence {
    /// The worker labelled the bullet (`agent:`, `operator:`, `human (x):`).
    Label,
    /// An unlabelled bullet named an action only a person can take.
    Signal(&'static str),
    /// An unlabelled bullet with no such phrase.
    Default,
}

/// The owner a worker wrote at the start of an action item, if any:
/// `agent:` → [`ActionItemOwner::Agent`]; `operator:`, `human:`, and
/// `human (<who>):` → [`ActionItemOwner::Human`]. Case-insensitive; the label
/// may be bold or italic (`**human (tmstorm):**`, `**Agent**:`).
pub fn explicit_owner_label(item: &str) -> Option<ActionItemOwner> {
    let emphasis = |c: char| c == '*' || c == '_';
    let rest = item.trim_start().trim_start_matches(emphasis);
    let word_end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let owner = match rest[..word_end].to_ascii_lowercase().as_str() {
        "agent" => ActionItemOwner::Agent,
        "operator" | "human" => ActionItemOwner::Human,
        _ => return None,
    };
    let mut tail = rest[word_end..].trim_start_matches(emphasis).trim_start();
    if let Some(inner) = tail.strip_prefix('(') {
        let close = inner.find(')')?;
        tail = inner[close + 1..].trim_start_matches(emphasis).trim_start();
    }
    tail.starts_with(':').then_some(owner)
}

/// Whole words (lowercase) that name an action only a person can take
/// wherever they appear. Matched against the item with inline code masked, so
/// a path such as `admin.rs` never matches. A word joined by a hyphen is one
/// word: `approval-flow` is not `approval`.
const HUMAN_WORDS: &[&str] = &[
    // Another person's review decision: an author can never approve their
    // own pull request, and a re-review is the reviewer's act.
    "re-review",
    "re-reviews",
    "rereview",
    "non-author",
    "maintainer",
    "maintainers",
    "approves",
    // Settings only an owner or admin can change.
    "admin",
    "admins",
    "administrator",
    // An operator decision.
    "decide",
    "decides",
    // A credential or check only a person can pass: single sign-on, a second
    // factor, a hardware key, a device confirmation.
    "sso",
    "2fa",
    "mfa",
    "otp",
    "totp",
    "passkey",
    "passkeys",
    "security-key",
    "security-keys",
    "yubikey",
    "captcha",
    "unrecognized-device",
];

/// Verbs (lowercase) that name an action only a person can take when they
/// open a clause, as an instruction: the item's first word, or the first word
/// after `.`, `;`, `:`, `,`, `(`, `→`, or after `then`, `and`, `or`, `please`
/// (`Approve and merge PR 9`, `Review and approve #240`). Elsewhere they are
/// usually what the agent works on (`the approve button`, `how the client
/// authorizes`), so they are not signals there.
const HUMAN_CLAUSE_VERBS: &[&str] = &[
    "approve",
    "authorize",
    "authenticate",
    "re-authenticate",
    "reauthenticate",
    "rehearse",
];

/// Words after which the next word opens a clause.
const CLAUSE_JOINERS: &[&str] = &["then", "and", "or", "please"];

/// Phrases (lowercase, single-spaced words) with the same meaning. A phrase
/// pins the human-action reading a bare word cannot: `an approval` and
/// `approval from` are another person's act, `the approval docs` is not;
/// `not authorized` is missing authority, `the authorization header` is code.
const HUMAN_PHRASES: &[&str] = &[
    // Another person's review decision.
    "someone other than",
    "someone else",
    "another reviewer",
    "an approval",
    "human approval",
    "approval from",
    "approvals from",
    "approval on",
    "cannot approve",
    "can not approve",
    "can t approve",
    // Settings only an owner or admin can change.
    "org owner",
    "organization owner",
    "repo owner",
    "repository owner",
    "code owner",
    // An operator decision or confirmation.
    "ask the operator",
    "operator confirms",
    "operator to confirm",
    "confirm with the operator",
    "operator decides",
    "operator to decide",
    // Authority the agent does not hold: missing, named as the operator's,
    // or required (`[operator authorization required] Create the repo`,
    // `this item did not authorize the push`). `authorization` and `approval`
    // alone stay the agent's subject matter (`Fix the authorization header`).
    "not authorized",
    "needs authorization",
    "need authorization",
    "requires authorization",
    "authorization from",
    "did not authorize",
    "didn t authorize",
    "operator authorization",
    "human authorization",
    "operator approval",
    "authorization required",
    "authorization needed",
    "approval required",
    "approval needed",
    // Signing in, or a credential check only a person can pass.
    "sign in",
    "signed in",
    "sign into",
    "log in",
    "logged in",
    "log into",
    "account login",
    "interactive login",
    "login prompt",
    "login checkpoint",
    "security key",
    "verification code",
    "one-time code",
    "one-time password",
    "authenticator app",
    "device confirmation",
    "unrecognized device",
    "new device",
    "password checkpoint",
    "password prompt",
    "enter the password",
    "enter your password",
    "enter a password",
];

/// One dependent form of another person's approval: a `target` word preceded,
/// within `reach` words of the same clause, by one of `leads` (`needs …
/// approval`, `get … approved`, `should approve`), or, when `leads` is empty,
/// followed by one of `follows` (`approved by`). A lead does not count when
/// one of `unless` stands within `unless_reach` words before it in the same
/// clause: `how to approve` describes the act, and `make X require approval`
/// configures a gate, so neither waits on anyone. The bare instruction
/// `Approve …` is [`HUMAN_CLAUSE_VERBS`]; these are the forms where the
/// approval is someone else's act or a wait on it, in any clause position
/// except a purpose clause ([`purpose_clause_words`]).
struct DependentApproval {
    targets: &'static [&'static str],
    leads: &'static [&'static str],
    reach: usize,
    follows: &'static [&'static str],
    unless: &'static [&'static str],
    unless_reach: usize,
    signal: &'static str,
}

/// The modal and helper words after which `approve` or `authorize` is
/// someone's act, not the agent's subject matter (`the operator should
/// approve`, `the operator must authorize the force-push`).
const APPROVE_MODALS: &[&str] = &[
    "should", "must", "will", "would", "can", "could", "may", "might", "shall",
];

/// Question words before `to approve` that make it a description of the act
/// (`Document how to approve a release`), not a wait on someone doing it.
const APPROVE_QUESTION_WORDS: &[&str] = &["how", "when", "where", "whether", "what", "which"];

/// Causative verbs before `require`/`need … approval` that make it a gate
/// the agent configures (`Make the environment require approval`), not an
/// approval a change is waiting on.
const GATE_CAUSATIVES: &[&str] = &["make", "makes", "made", "making"];

/// Dependent approval forms, and the modal form of `authorize`
/// (`#humanfollowupowner`). A word joined by a hyphen stays one word, so
/// `approval-flow` matches none of them.
const DEPENDENT_APPROVALS: &[DependentApproval] = &[
    DependentApproval {
        targets: &["approve"],
        leads: &["to"],
        reach: 1,
        follows: &[],
        unless: APPROVE_QUESTION_WORDS,
        unless_reach: 1,
        signal: "to approve",
    },
    DependentApproval {
        targets: &["approve"],
        leads: APPROVE_MODALS,
        reach: 1,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "<modal> approve",
    },
    DependentApproval {
        targets: &["authorize"],
        leads: APPROVE_MODALS,
        reach: 1,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "<modal> authorize",
    },
    DependentApproval {
        targets: &["approve"],
        leads: &["have", "has", "let", "lets", "ask", "asks"],
        reach: 3,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "have … approve",
    },
    DependentApproval {
        targets: &["approval", "approvals"],
        leads: &["need", "needs", "needed", "needing"],
        reach: 4,
        follows: &[],
        unless: GATE_CAUSATIVES,
        unless_reach: 4,
        signal: "needs … approval",
    },
    DependentApproval {
        targets: &["approval", "approvals"],
        leads: &["require", "requires", "required", "requiring"],
        reach: 4,
        follows: &[],
        unless: GATE_CAUSATIVES,
        unless_reach: 4,
        signal: "requires … approval",
    },
    DependentApproval {
        targets: &["approval", "approvals"],
        leads: &["await", "awaits", "awaiting", "wait", "waits", "waiting"],
        reach: 4,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "awaiting … approval",
    },
    DependentApproval {
        targets: &["approval", "approvals"],
        leads: &["s"],
        reach: 1,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "'s approval",
    },
    DependentApproval {
        targets: &["approved"],
        leads: &[],
        reach: 0,
        follows: &["by"],
        unless: &[],
        unless_reach: 0,
        signal: "approved by",
    },
    DependentApproval {
        targets: &["approved"],
        leads: &["has", "have", "had"],
        reach: 1,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "has approved",
    },
    DependentApproval {
        targets: &["approved"],
        leads: &["get", "gets", "got", "getting"],
        reach: 4,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "get … approved",
    },
    DependentApproval {
        targets: &["approved"],
        leads: &["be", "been", "being"],
        reach: 1,
        follows: &[],
        unless: &[],
        unless_reach: 0,
        signal: "be approved",
    },
];

/// Whether each of `words` (from [`clause_words`]) sits in a purpose clause:
/// after `so` (or `so that`) or `in order to`, up to the end of that clause.
/// A purpose clause names the outcome of the item's own work (`Fix the lint
/// failure so the PR can be approved`), so an approval there is not what the
/// item waits on. A `so` that opens its clause (`CI is green, so tmstorm
/// needs to approve`) states a consequence, not a purpose, and does not count.
fn purpose_clause_words(words: &[(&str, bool)]) -> Vec<bool> {
    let mut in_purpose = false;
    let mut flags = Vec::with_capacity(words.len());
    for (index, (word, opens_clause)) in words.iter().enumerate() {
        if *opens_clause {
            in_purpose = false;
        }
        flags.push(in_purpose);
        let in_order_to = *word == "order"
            && index > 0
            && words[index - 1].0 == "in"
            && words.get(index + 1).is_some_and(|(next, _)| *next == "to");
        if (*word == "so" && !opens_clause) || in_order_to {
            in_purpose = true;
        }
    }
    flags
}

/// The first [`DEPENDENT_APPROVALS`] form in `words` (from [`clause_words`])
/// outside a purpose clause. A lead counts only inside the target's clause:
/// the scan back from the target stops at the word that opens it.
fn dependent_approval_signal(words: &[(&str, bool)]) -> Option<&'static str> {
    let purpose = purpose_clause_words(words);
    for (index, (word, opens_clause)) in words.iter().enumerate() {
        if purpose[index] {
            continue;
        }
        for form in DEPENDENT_APPROVALS {
            if !form.targets.contains(word) {
                continue;
            }
            if let Some((next, next_opens)) = words.get(index + 1)
                && !next_opens
                && form.follows.contains(next)
            {
                return Some(form.signal);
            }
            if *opens_clause {
                continue;
            }
            for back in 1..=form.reach.min(index) {
                let lead_index = index - back;
                let (lead, lead_opens) = words[lead_index];
                if form.leads.contains(&lead) && !lead_is_cancelled(words, lead_index, form) {
                    return Some(form.signal);
                }
                if lead_opens {
                    break;
                }
            }
        }
    }
    None
}

/// Whether one of `form.unless` stands within `form.unless_reach` words
/// before the lead at `lead_index`, inside the lead's clause.
fn lead_is_cancelled(words: &[(&str, bool)], lead_index: usize, form: &DependentApproval) -> bool {
    if words[lead_index].1 {
        return false;
    }
    for back in 1..=form.unless_reach.min(lead_index) {
        let (word, opens_clause) = words[lead_index - back];
        if form.unless.contains(&word) {
            return true;
        }
        if opens_clause {
            break;
        }
    }
    false
}

/// The first phrase in an unlabelled action item that names an action only a
/// person can take, or `None` when it names none (`#humanfollowupowner`).
///
/// The vocabulary covers the shapes worker hand-backs use for human work:
/// another person's review decision (`re-review`, `non-author`, `maintainer`,
/// `X approves`, `Approve …` as an instruction, `an approval`, `approval
/// from`, `cannot approve`, `someone other than`, `another reviewer`, and the
/// dependent forms of [`DEPENDENT_APPROVALS`]: `to approve`, `should
/// approve`, `must authorize`, `have someone approve`,
/// `needs/requires/awaiting … approval`, `'s approval`, `approved by`, `has
/// approved`, `get … approved`, except in a purpose clause such as `so the PR
/// can be approved`), a setting only an owner or admin can change (`org
/// owner`, `repo owner`, `admin`), an operator decision (`decide`, `ask the
/// operator`, `operator confirms`), authority the agent does not hold (`not
/// authorized`, `did not authorize`, `operator authorization`/`approval`,
/// `authorization`/`approval required`, `Authorize …`), and a sign-in or
/// credential check (`sign in`, `log into`, `account login`, `SSO`, `2FA`,
/// `OTP`, `passkey`, `security key`, `device confirmation`, `password
/// prompt`). A word that is often the agent's own
/// subject matter is a signal only in a human-action position: `confirm`,
/// `authorization`, `approval`, `login`, and `password` alone are not
/// (`Confirm hosted CI is green`, `Fix the authorization header`, `Update
/// the approval-flow docs`), and neither is an identifier written without
/// backticks (`needs_approval`). Plain agent verbs (`merge`, `review`, `rebase`,
/// `fix`, `address`) are not signals: "address the review" and "rebase after
/// #240 merges" are agent work.
pub fn human_owner_signal(item: &str) -> Option<&'static str> {
    let masked = mask_code_spans(item).to_ascii_lowercase();
    let words = clause_words(&masked);
    for (word, opens_clause) in &words {
        if let Some(signal) = HUMAN_WORDS.iter().find(|signal| *signal == word) {
            return Some(signal);
        }
        if *opens_clause
            && let Some(signal) = HUMAN_CLAUSE_VERBS.iter().find(|signal| *signal == word)
        {
            return Some(signal);
        }
    }
    let joined = format!(
        " {} ",
        words
            .iter()
            .map(|(word, _)| *word)
            .collect::<Vec<_>>()
            .join(" ")
    );
    HUMAN_PHRASES
        .iter()
        .find(|phrase| joined.contains(&format!(" {phrase} ")))
        .copied()
        .or_else(|| dependent_approval_signal(&words))
}

/// The words of `text` (ASCII alphanumerics and inner hyphens and
/// underscores), each with whether it opens a clause ([`HUMAN_CLAUSE_VERBS`]).
/// An underscore joins a word like a hyphen, so an identifier written without
/// backticks (`needs_approval`, `approved_by`, `is_admin`) is one word that
/// matches no signal.
fn clause_words(text: &str) -> Vec<(&str, bool)> {
    let mut words = Vec::new();
    let mut opens_clause = true;
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            start.get_or_insert(index);
            continue;
        }
        if let Some(begin) = start.take() {
            let word = text[begin..index].trim_matches(|c| c == '-' || c == '_');
            if !word.is_empty() {
                words.push((word, opens_clause));
                opens_clause = CLAUSE_JOINERS.contains(&word);
            }
        }
        if matches!(ch, '.' | ';' | ':' | ',' | '(' | '!' | '?' | '\n' | '→') {
            opens_clause = true;
        }
    }
    words
}

/// The owner of an action item and why: the worker's label when present,
/// otherwise [`human_owner_signal`], otherwise agent.
pub fn classify_action_item(item: &str) -> (ActionItemOwner, OwnerEvidence) {
    if let Some(owner) = explicit_owner_label(item) {
        return (owner, OwnerEvidence::Label);
    }
    match human_owner_signal(item) {
        Some(signal) => (ActionItemOwner::Human, OwnerEvidence::Signal(signal)),
        None => (ActionItemOwner::Agent, OwnerEvidence::Default),
    }
}

/// [`classify_action_item`]'s owner alone.
pub fn action_item_owner(item: &str) -> ActionItemOwner {
    classify_action_item(item).0
}

/// Whether `text` already carries [`OPERATOR_OWNED_TAG`].
pub fn carries_operator_owned_tag(text: &str) -> bool {
    backlog::item_operator_verify_required(text)
}

/// `row` with [`OPERATOR_OWNED_TAG`] after its work text, before the
/// binary's trailing `[#q-…]` id annotation when it has one. A row that
/// already carries the tag is returned unchanged.
pub fn tag_operator_owned(row: &str) -> String {
    if carries_operator_owned_tag(row) {
        return row.to_string();
    }
    let base = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(row);
    if base == row {
        return format!("{} {OPERATOR_OWNED_TAG}", row.trim_end());
    }
    let mut split = row
        .char_indices()
        .zip(base.chars())
        .find(|((_, a), b)| a != b)
        .map_or(base.len().min(row.len()), |((index, _), _)| index);
    while !row.is_char_boundary(split) {
        split -= 1;
    }
    let (work, annotation) = row.split_at(split);
    format!(
        "{} {OPERATOR_OWNED_TAG} {}",
        work.trim_end(),
        annotation.trim_start()
    )
}

/// The follow-up row for `item`, tagged when the item is human-owned.
pub fn owned_followup_row(row: String, item: &str) -> String {
    match action_item_owner(item) {
        ActionItemOwner::Human => tag_operator_owned(&row),
        ActionItemOwner::Agent => row,
    }
}

/// The action item a `follow up on an action item from <origin>: <item>` work
/// text quotes. `<origin>` is `` `id` `` or `queue item “label”`.
pub fn followup_row_action_item(work: &str) -> Option<&str> {
    let rest = work.trim_start().strip_prefix(FOLLOWUP_ROW_PREFIX)?;
    let after_origin = if let Some(quoted) = rest.strip_prefix('`') {
        let close = quoted.find('`')?;
        &quoted[close + 1..]
    } else {
        let quoted = rest.strip_prefix("queue item “")?;
        let close = quoted.find('”')?;
        &quoted[close + '”'.len_utf8()..]
    };
    after_origin.strip_prefix(": ")
}

/// One follow-up row [`reclassify_human_followup_rows`] tagged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclassifiedFollowup {
    /// The row text before, without its list bullet.
    pub before: String,
    /// The row text after.
    pub after: String,
    /// Why the item is human-owned.
    pub evidence: OwnerEvidence,
}

/// The reclassified text of one live queue row (its text after the list
/// bullet), or `None` when it is not an unstarted human-owned follow-up row
/// missing its tag.
///
/// Only a row whose lifecycle is bare or `📥` qualifies: a claimed, running,
/// reported, or lost row has an attempt keyed by its text, and a `🙋` row is
/// already operator-owned. A `📥` row becomes `⏳`; preflight repaints the
/// same glyph from the tag.
pub fn reclassify_followup_row(text: &str) -> Option<(String, OwnerEvidence)> {
    if text.contains('\n') || text.trim_start().starts_with("~~") {
        return None;
    }
    let (heading, rest) = match text.strip_prefix("### ") {
        Some(rest) => ("### ", rest),
        None => ("", text),
    };
    let marker = queue_projection::lifecycle_marker(rest);
    if !matches!(marker, None | Some(QueueLifecycleMarker::Pending))
        || queue_projection::operator_action_marked(rest)
        || carries_operator_owned_tag(rest)
    {
        return None;
    }
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(rest);
    let item = followup_row_action_item(work)?;
    let item = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(item);
    let (owner, evidence) = classify_action_item(&item);
    if owner != ActionItemOwner::Human {
        return None;
    }
    let mut tagged = tag_operator_owned(rest);
    if marker == Some(QueueLifecycleMarker::Pending) {
        tagged = queue_projection::apply_awaiting_operator_marker(&tagged);
    }
    Some((format!("{heading}{tagged}"), evidence))
}

/// Tag every live, unstarted human-owned `follow up on an action item` row in
/// every queue block (`#humanfollowupowner`). Lines are rewritten in place;
/// every other byte is kept. Idempotent: a tagged row is never touched again,
/// so a second run returns `None`. This is the explicit repair
/// (`agent-doc queue reclassify-followups`); preflight uses
/// [`reclassify_unseen_human_followup_rows`] so an operator's removal of the
/// tag sticks.
pub fn reclassify_human_followup_rows(
    content: &str,
) -> Result<Option<(String, Vec<ReclassifiedFollowup>)>> {
    reclassify_unseen_human_followup_rows(content, &HashSet::new())
}

/// [`reclassify_human_followup_rows`], skipping every row whose
/// [`followup_owner_key`] is in `seen`.
///
/// Preflight records every follow-up row that carries the tag
/// ([`followup_row_keys`]). A row the operator hands back to an agent by
/// deleting its `[operator-verify]` tag keeps its key, so the next cycle
/// leaves it alone instead of re-adding the tag. A row that was never tagged
/// is never recorded, so it is classified again on every cycle.
pub fn reclassify_unseen_human_followup_rows(
    content: &str,
    seen: &HashSet<String>,
) -> Result<Option<(String, Vec<ReclassifiedFollowup>)>> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut rows = Vec::new();
    for (text_start, text) in followup_row_texts(content)? {
        if followup_owner_key(text).is_some_and(|key| seen.contains(&key)) {
            continue;
        }
        let Some((after, evidence)) = reclassify_followup_row(text) else {
            continue;
        };
        edits.push((text_start, text_start + text.len(), after.clone()));
        rows.push(ReclassifiedFollowup {
            before: text.to_string(),
            after,
            evidence,
        });
    }
    if edits.is_empty() {
        return Ok(None);
    }
    let mut updated = content.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        updated.replace_range(start..end, &replacement);
    }
    Ok(Some((updated, rows)))
}

/// The identity of a follow-up row for owner classification: its work text
/// without lifecycle projection, the binary's trailing `[#q-…]` id, or the
/// `[operator-verify]` tag, whitespace collapsed. Deleting the tag, or the
/// `📥`/`⏳` repaint, leaves the key unchanged. `None` for a row outside the
/// `follow up on an action item` shape.
pub fn followup_owner_key(text: &str) -> Option<String> {
    let text = text.strip_prefix("### ").unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    followup_row_action_item(work)?;
    let work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(work);
    let key = work
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case(OPERATOR_OWNED_TAG))
        .collect::<Vec<_>>()
        .join(" ");
    Some(key)
}

/// The follow-up rows of `content` by [`followup_owner_key`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FollowupRowKeys {
    /// Rows that carry the tag, in any lifecycle: the only rows where an
    /// operator's later deletion of the tag must stick. An untagged row is
    /// not here, so preflight classifies it again on every cycle.
    pub classified: HashSet<String>,
    /// Every live (unstruck) follow-up row, in any lifecycle.
    pub live: HashSet<String>,
}

/// [`FollowupRowKeys`] of every queue block of `content`. Preflight records
/// `classified` after it writes the document, and keeps a recorded key only
/// while its row is `live`.
pub fn followup_row_keys(content: &str) -> Result<FollowupRowKeys> {
    let mut keys = FollowupRowKeys::default();
    for (_, text) in followup_row_texts(content)? {
        if text.trim_start().starts_with("~~") {
            continue;
        }
        let Some(key) = followup_owner_key(text) else {
            continue;
        };
        let rest = text.strip_prefix("### ").unwrap_or(text);
        if carries_operator_owned_tag(rest) {
            keys.classified.insert(key.clone());
        }
        keys.live.insert(key);
    }
    Ok(keys)
}

/// `(byte offset, text)` of every single-line list row in a queue block that
/// mentions [`FOLLOWUP_ROW_PREFIX`]; `text` is the row after its `- ` bullet.
fn followup_row_texts(content: &str) -> Result<Vec<(usize, &str)>> {
    let components = agent_doc_element::element::parse(content)?;
    let mut rows = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body_start = component.open_end;
        let body = component.content(content);
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            let line_start = body_start + offset;
            offset += line.len();
            let content_line = line.trim_end_matches(['\n', '\r']);
            let indent = content_line.len() - content_line.trim_start().len();
            let Some(text) = content_line[indent..].strip_prefix("- ") else {
                continue;
            };
            if !text.contains(FOLLOWUP_ROW_PREFIX) {
                continue;
            }
            rows.push((line_start + indent + 2, text));
        }
    }
    Ok(rows)
}

/// `text` with the contents of inline code spans replaced by spaces.
fn mask_code_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for ch in text.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
        } else if in_code {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests;
