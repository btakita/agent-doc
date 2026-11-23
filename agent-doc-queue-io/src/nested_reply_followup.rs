//! `#nestedreplyfollowup`: [`agent_doc_queue::nested_reply_followup`] with
//! its two gates read from durable state.
//!
//! - **Held:** the claim ledger, the lineage ledger, and the release-receipt
//!   ledger. A reply whose nest identity (or text) any of them records never
//!   moves, so a claimed or running attempt cannot turn `Lost`, and a reported
//!   receipt keeps matching its row. An unreadable ledger holds every reply.
//! - **Stable:** the reply's exact line is in the stable merge baseline the
//!   last binary write checkpointed (`#qstrikeexplain`), and it is not still
//!   being typed (`agent_doc_debounce::edit_settle`). No baseline moves
//!   nothing.
//!
//! The move is also refused unless disk agrees with the authority it was
//! planned on: the same plan over the disk text must yield the same moves.
//! Callers write the result through their binary-owned, CRDT-aware queue
//! write path and call [`record_moved_nested_replies`] once the authority
//! holds the moved rows.

use std::path::Path;

use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_queue::nested_reply_followup::{
    MovedNestedReply, NestedReplyGates, NestedReplyRow, move_nested_replies,
};

/// Everything a ledger says about past or present attempts: each record's
/// identities and the item texts it was keyed by.
#[derive(Default)]
struct HeldRecords {
    identities: Vec<QueueItemIdentity>,
    texts: Vec<String>,
    /// Identities a live claim or a pending release receipt holds: the row
    /// that owns a reply's response must not be one of them.
    live_identities: Vec<QueueItemIdentity>,
}

impl HeldRecords {
    fn push(&mut self, identity: &QueueItemIdentity, text: &str) {
        self.identities.push(identity.clone());
        self.texts.push(collapse_lower(text));
    }

    /// A record holds the row by identity, or by text: a record's text that
    /// contains the reply, or is contained in its nest vector. Short texts
    /// never match by text.
    fn holds(&self, row: &NestedReplyRow) -> bool {
        if self.identities.contains(&row.identity)
            || self.live_identities.contains(&row.owner_identity)
        {
            return true;
        }
        let reply = collapse_lower(&row.reply);
        let vector = collapse_lower(&row.nest_vector);
        let long = |text: &str| text.chars().count() >= 8;
        self.texts.iter().any(|text| {
            long(text)
                && ((long(&reply) && text.contains(&reply)) || vector.contains(text.as_str()))
        })
    }
}

fn load_held_records(file: &Path) -> Option<HeldRecords> {
    let mut held = HeldRecords::default();
    let claims = crate::queue_claim::load_ledger_read_only(file).ok()?;
    for claim in &claims.claims {
        held.push(&claim.identity, &claim.item_text);
        held.live_identities.push(claim.identity.clone());
    }
    let lineage = crate::queue_lineage::load_read_only(file).ok()?;
    for attempt in &lineage.attempts {
        held.push(&attempt.identity, &attempt.item_text);
        held.identities
            .extend(attempt.identity_history.iter().cloned());
        held.texts.extend(
            attempt
                .item_text_history
                .iter()
                .map(|text| collapse_lower(text)),
        );
    }
    let receipts = crate::claim_release_receipt::load_read_only(file).ok()?;
    for receipt in &receipts.receipts {
        held.push(&receipt.identity, &receipt.item_text);
        held.live_identities.push(receipt.identity.clone());
    }
    Some(held)
}

fn plan_with(
    file: &Path,
    content: &str,
    held: &HeldRecords,
    baseline: &str,
) -> Option<(String, Vec<MovedNestedReply>)> {
    let held_gate = |row: &NestedReplyRow| held.holds(row);
    let stable_gate = |row: &NestedReplyRow| {
        baseline.lines().any(|line| line == row.line)
            && agent_doc_debounce::edit_settle::completion_signal(&row.reply)
                != agent_doc_debounce::edit_settle::CompletionSignal::Incomplete
    };
    move_nested_replies(
        content,
        &agent_doc_hash::document_id_for_path(file),
        &NestedReplyGates {
            held: &held_gate,
            stable: &stable_gate,
        },
    )
}

/// Plan moving `content`'s eligible nested replies to top-level follow-up
/// rows. `baseline` is the stable merge baseline as the caller's run found
/// it, read before the run checkpointed anything ([`stable_baseline`]).
/// `None` when nothing moves or anything is uncertain.
pub fn move_nested_replies_for(
    file: &Path,
    content: &str,
    baseline: Option<&str>,
) -> Option<(String, Vec<MovedNestedReply>)> {
    if !content.contains("**Response [#r-") {
        return None;
    }
    let baseline = baseline?;
    // Cheap pre-check without the ledgers: nothing structurally movable and
    // stable means no state reads at all.
    plan_with(file, content, &HeldRecords::default(), baseline)?;
    let held = load_held_records(file)?;
    let planned = plan_with(file, content, &held, baseline)?;
    // Disk must agree with the authority on every move: a reply the editor
    // and disk do not both hold, unchanged, stays where it is.
    let disk = std::fs::read_to_string(file).ok()?;
    if disk != content {
        let on_disk = plan_with(file, &disk, &held, baseline)?;
        if on_disk.1 != planned.1 {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_nested_reply_followup_skipped file={} reason=editor_disk_disagree planned={} on_disk={} (#nestedreplyfollowup)",
                    file.display(),
                    planned.1.len(),
                    on_disk.1.len(),
                ),
            );
            return None;
        }
    }
    Some(planned)
}

/// The stable merge baseline the last binary write checkpointed, or `None`.
pub fn stable_baseline(file: &Path) -> Option<String> {
    agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
}

/// Record the moves the authority now holds: each new row's follow-up root
/// provenance, and the follow-up ledgers keyed on row text carried from the
/// reply's text to the new row (`#selfrepaircleanup` carry-over). A move
/// whose row is not in `authoritative` records nothing.
pub fn record_moved_nested_replies(file: &Path, authoritative: &str, moves: &[MovedNestedReply]) {
    let landed: Vec<&MovedNestedReply> = moves
        .iter()
        .filter(|moved| authoritative.contains(&moved.followup_row))
        .collect();
    if landed.is_empty() {
        return;
    }
    let rewrites: Vec<(String, String)> = landed
        .iter()
        .map(|moved| (moved.reply_row.clone(), moved.followup_row.clone()))
        .collect();
    crate::followup_provenance_ledger::carry_rewritten_rows(file, &rewrites);
    crate::followup_owner_ledger::carry_rewritten_rows(file, &rewrites);
    for moved in &landed {
        if !moved.linked_existing {
            crate::followup_provenance_ledger::record_followup_roots(
                file,
                authoritative,
                &[moved.owner_row.as_str()],
                std::slice::from_ref(&moved.followup_row),
            );
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_nested_reply_followup file={} origin={} response={} followup={} linked_existing={} reply_hash={} (#nestedreplyfollowup)",
                file.display(),
                moved.origin_id,
                moved.response_id,
                moved.followup_id,
                moved.linked_existing,
                agent_doc_hash::content_hash(&moved.reply),
            ),
        );
    }
}

fn collapse_lower(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(reply: &str, vector: &str) -> NestedReplyRow {
        NestedReplyRow {
            identity: agent_doc_queue::queue_claim::claim_identity(vector),
            nest_vector: vector.to_string(),
            reply: reply.to_string(),
            line: format!("    - {reply}"),
            owner_identity: QueueItemIdentity::FreeText("owner".into()),
        }
    }

    #[test]
    fn a_record_holds_a_row_by_identity_or_text() {
        let vector = "Fix the widget. [#q-aaaaaaaaaaaa]: Also check the gizmo.";
        let reply = row("Also check the gizmo.", vector);
        let mut held = HeldRecords::default();
        assert!(!held.holds(&reply));
        held.push(&reply.identity, "unrelated");
        assert!(held.holds(&reply), "same identity");

        let mut by_text = HeldRecords::default();
        by_text.push(
            &QueueItemIdentity::FreeText("other".into()),
            "fix the widget. [#q-aaaaaaaaaaaa]: also check  the gizmo.",
        );
        assert!(by_text.holds(&reply), "claim text contains the reply");

        let mut owner = HeldRecords::default();
        owner
            .live_identities
            .push(QueueItemIdentity::FreeText("owner".into()));
        assert!(
            owner.holds(&reply),
            "a live claim on the owner holds its replies"
        );

        let mut short = HeldRecords::default();
        short.push(&QueueItemIdentity::FreeText("x".into()), "go");
        assert!(
            !short.holds(&row("go", "Fix: go")),
            "short texts never match"
        );
    }

    const REPLY: &str = "Also check the gizmo.";

    fn session_doc() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("s.md");
        let content = format!(
            concat!(
                "<!-- agent:run id=\"main\" -->\n",
                "- ### ~~Fix the widget. [#q-aaaaaaaaaaaa]~~\n",
                "  - **Response [#r-56cf9a1ad146]**\n",
                "    - **Summary:** I fixed the widget.\n",
                "    - {REPLY}\n",
                "<!-- /agent:run -->\n",
            ),
            REPLY = REPLY
        );
        std::fs::write(&file, &content).unwrap();
        (dir, file, content)
    }

    #[test]
    fn a_live_claim_on_the_nested_reply_keeps_it_in_place() {
        let (_dir, file, content) = session_doc();
        assert!(
            move_nested_replies_for(&file, &content, Some(&content)).is_some(),
            "an unclaimed stable reply moves"
        );
        let vector = format!("Fix the widget. [#q-aaaaaaaaaaaa]: {REPLY}");
        crate::queue_claim::claim(&file, &vector, "subagent:gizmo", 3600).unwrap();
        assert_eq!(
            move_nested_replies_for(&file, &content, Some(&content)),
            None,
            "a claimed reply never moves"
        );
    }

    #[test]
    fn no_baseline_or_a_disagreeing_disk_moves_nothing() {
        let (_dir, file, content) = session_doc();
        assert_eq!(move_nested_replies_for(&file, &content, None), None);
        // The editor holds the reply; disk does not yet.
        let disk = content.replace(&format!("    - {REPLY}\n"), "");
        std::fs::write(&file, &disk).unwrap();
        assert_eq!(
            move_nested_replies_for(&file, &content, Some(&content)),
            None
        );
    }

    #[test]
    fn an_unfinished_line_is_still_being_typed() {
        let (_dir, file, content) = session_doc();
        let typing = content.replace(REPLY, "Also check the `gizmo");
        std::fs::write(&file, &typing).unwrap();
        assert_eq!(move_nested_replies_for(&file, &typing, Some(&typing)), None);
    }
}
