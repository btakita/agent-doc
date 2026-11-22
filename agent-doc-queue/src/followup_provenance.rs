//! `#followuprootprovenance`: the tracked item a follow-up row descends from.
//!
//! `queue report-complete` and `queue follow-up` turn a worker's action items
//! into free-text rows (`[#<origin>] follow-up: <item>`, `#followupshorttitle`;
//! older rows read `follow up on an action item from …: <item>`). The row
//! names only its immediate origin, and that reference is never counted as
//! the origin's identity or as a reference to tracked work, so a nested
//! follow-up names a `[#q-…]` row id. The row text therefore cannot say which
//! backlog item the work belongs to.
//!
//! The go-mode backlog mirror used to see only that text. Once the item's
//! `do [#id]` head was acknowledged, struck and reaped, the item was still
//! open and no queue row referenced it, so the mirror re-added a pending
//! `do [#id]` head while a worker was already running its follow-up. The
//! duplicate head then held closeout (`#transientcloseout`) until a second
//! worker was dispatched onto the same work.
//!
//! So the creating command records each new row's **root tracked-item id** in
//! a typed ledger ([`FollowupRootLedger`]), keyed by the row's work identity
//! ([`followup_row_key`]). The root is the originating head's id, or, when the
//! origin is itself a recorded follow-up row, that row's root, so it carries
//! through any depth of nesting. The mirrors read the ledger through
//! [`live_followup_root_ids`]: an open item with a live (unstruck) descendant
//! row is already in the queue. Once every descendant is struck or gone, the
//! mirror may re-add the head as before. A row with no
//! recorded provenance contributes nothing, so older documents keep today's
//! behaviour.

use std::collections::{BTreeMap, HashSet};

use agent_doc_element_queue::QueueItemIdentity;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::document_queue::{self, QueueEntry};

/// Root provenance of one follow-up row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowupRoot {
    /// The tracked item (backlog/todo id) the row's work belongs to,
    /// normalized (lowercase, no `#`).
    pub root_item_id: String,
}

/// Follow-up root provenance for one document, keyed by [`followup_row_key`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowupRootLedger {
    #[serde(default)]
    pub rows: BTreeMap<String, FollowupRoot>,
}

impl FollowupRootLedger {
    /// The recorded root of the row whose text is `row`, if any.
    pub fn root_of_row(&self, row: &str) -> Option<&str> {
        let key = followup_row_key(row)?;
        self.rows.get(&key).map(|root| root.root_item_id.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The work identity of a queue row for provenance: its text without the
/// list heading, lifecycle and attempt projection, the binary's trailing
/// `[#q-…]` id, a report-state or effect annotation, or the
/// `[operator-verify]` tag, whitespace collapsed. Every repaint preflight
/// applies to a follow-up row leaves this key unchanged. A struck row keys
/// to its struck work text. `None` for an empty row.
pub fn followup_row_key(row: &str) -> Option<String> {
    let text = row.trim();
    let text = text.strip_prefix("### ").unwrap_or(text).trim();
    let text = struck_inner(text).unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    let mut work = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(work);
    loop {
        let trimmed = work.trim_end();
        let stripped = agent_doc_element_queue::strip_queue_report_state_annotation(trimmed);
        let stripped = strip_trailing_annotation(stripped);
        let stripped =
            agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(stripped);
        if stripped.trim_end() == trimmed {
            break;
        }
        work = stripped;
    }
    let key = work
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case(crate::followup_owner::OPERATOR_OWNED_TAG))
        .collect::<Vec<_>>()
        .join(" ");
    (!key.is_empty()).then_some(key)
}

/// `text` without one trailing binary-owned `⟦…⟧` annotation.
fn strip_trailing_annotation(text: &str) -> &str {
    let trimmed = text.trim_end();
    if let Some(prefix) = trimmed.strip_suffix('⟧')
        && let Some(open) = prefix.rfind(" ⟦")
    {
        return trimmed[..open].trim_end();
    }
    trimmed
}

/// The work text inside a `~~…~~` strike, without any trailing strike note.
fn struck_inner(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("~~")?;
    let close = rest.find("~~")?;
    Some(rest[..close].trim())
}

/// The root tracked-item id for follow-ups created from `origin_row`.
///
/// A recorded follow-up origin passes its own root on, so a follow-up of a
/// follow-up keeps the item it ultimately serves. Otherwise an id-backed
/// origin (`do [#id]`) is the root. An unrecorded free-text origin has no
/// root, and neither does a row whose only id is the binary's generated
/// `[#q-…]` correlation id.
pub fn origin_root_item_id(origin_row: &str, ledger: &FollowupRootLedger) -> Option<String> {
    if let Some(root) = ledger.root_of_row(origin_row) {
        return Some(root.to_string());
    }
    let text = origin_row.trim();
    let text = text.strip_prefix("### ").unwrap_or(text);
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(text);
    match agent_doc_element_queue::queue_head_identity(work) {
        QueueItemIdentity::Id(id) => {
            let id = id.trim().trim_start_matches('#').to_ascii_lowercase();
            (!id.is_empty() && !is_generated_queue_prompt_id(&id)).then_some(id)
        }
        QueueItemIdentity::FreeText(_) => None,
    }
}

/// `q-` followed by hex digits: the binary's generated row id, never a
/// tracked item.
fn is_generated_queue_prompt_id(id: &str) -> bool {
    id.strip_prefix("q-")
        .is_some_and(|hex| !hex.is_empty() && hex.chars().all(|ch| ch.is_ascii_hexdigit()))
}

/// The ledger after rows `inserted` were created from `origin_rows` (the
/// origin's row text and, when it differs, the claim's item text), with
/// recorded rows that no queue of `content` holds any more (live or struck)
/// dropped. `content` is the document the creating command wrote.
///
/// Returns `None` when nothing changes: no origin resolves to a root and no
/// recorded row was dropped.
pub fn plan_followup_root_record(
    ledger: &FollowupRootLedger,
    content: &str,
    origin_rows: &[&str],
    inserted: &[String],
) -> Result<Option<(FollowupRootLedger, Option<String>)>> {
    let root = origin_rows
        .iter()
        .find_map(|origin| origin_root_item_id(origin, ledger));
    let present = queue_row_keys(content)?;
    let mut next = FollowupRootLedger {
        rows: ledger
            .rows
            .iter()
            .filter(|(key, _)| present.contains(*key))
            .map(|(key, root)| (key.clone(), root.clone()))
            .collect(),
    };
    if let Some(root) = &root {
        for row in inserted {
            if let Some(key) = followup_row_key(row) {
                next.rows.insert(
                    key,
                    FollowupRoot {
                        root_item_id: root.clone(),
                    },
                );
            }
        }
    }
    Ok((next != *ledger).then_some((next, root)))
}

/// Root ids of every live follow-up row in `content`'s queues.
///
/// Live means the row is not struck, in any lifecycle: a pending, claimed,
/// running, lost or operator-held row still owns its root's work, and a
/// reported (`☑️`) row awaits the coordinator's acknowledgement exactly as a
/// reported `do [#id]` head does, which also keeps its id out of the mirror
/// until it is struck. These ids count as already queued for every
/// backlog→queue mirror.
pub fn live_followup_root_ids(
    content: &str,
    ledger: &FollowupRootLedger,
) -> Result<HashSet<String>> {
    let mut roots = HashSet::new();
    if ledger.is_empty() {
        return Ok(roots);
    }
    for text in queue_row_texts(content)? {
        let rest = text.strip_prefix("### ").unwrap_or(text).trim_start();
        if rest.starts_with("~~") {
            continue;
        }
        if let Some(root) = ledger.root_of_row(rest) {
            roots.insert(root.to_string());
        }
    }
    Ok(roots)
}

/// The ids of `roots` that `entries` does not already reference.
///
/// A mirror pass treats these as present elsewhere. An id the target queue
/// already references keeps its own row: in `queue=sync` mode an id marked
/// as present elsewhere would have its canonical row replaced by nothing.
pub fn roots_absent_from(roots: &HashSet<String>, entries: &[QueueEntry]) -> HashSet<String> {
    if roots.is_empty() {
        return HashSet::new();
    }
    let referenced: HashSet<String> = entries
        .iter()
        .flat_map(document_queue::queue_entry_reference_ids)
        .collect();
    roots
        .iter()
        .filter(|id| !referenced.contains(*id))
        .cloned()
        .collect()
}

/// [`followup_row_key`] of every row, live or struck, in `content`'s queues.
fn queue_row_keys(content: &str) -> Result<HashSet<String>> {
    Ok(queue_row_texts(content)?
        .into_iter()
        .filter_map(followup_row_key)
        .collect())
}

/// Text after the `- ` bullet of every single-line list row in every queue
/// component of `content`.
fn queue_row_texts(content: &str) -> Result<Vec<&str>> {
    let components = agent_doc_element::element::parse(content)?;
    let mut rows = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        for line in component.content(content).lines() {
            let trimmed = line.trim_start();
            if let Some(text) = trimmed.strip_prefix("- ") {
                rows.push(text.trim_end());
            }
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog_sync::{self, FollowUpQueuePlacement, MirrorOutcome, MirrorRefusal};
    use crate::document_queue::BacklogQueueSyncMode;
    use crate::handback_followups::plan_handback_followups;

    const HANDBACK: &str = concat!(
        "**Summary:** migrated the first family.\n",
        "**Verification:** make check exit=0\n",
        "**Action items:**\n",
        "- agent: Continue the remaining 271 pending-migration rows\n",
    );

    const NESTED_HANDBACK: &str = concat!(
        "**Summary:** migrated the second family.\n",
        "**Verification:** make check exit=0\n",
        "**Action items:**\n",
        "- agent: Continue the remaining 254 pending-migration rows\n",
    );

    /// The `tasks/software/lazily.md` shape: a go-mode `agent:run` with
    /// subagents, mirrored from a `queue priority` backlog whose item stays
    /// open across many slices.
    fn doc(run: &str) -> String {
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\nqueue: go\n---\n\n\
             <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
             ## Run <!-- agent:run subagents priority go -->\n{run}<!-- /agent:run -->\n\n\
             ## Backlog <!-- agent:backlog queue priority -->\n\
             - [ ] [#implementlazilyowned] migrate every pending row onto lazily\n\
             - [ ] [#other] unrelated open work\n\
             <!-- /agent:backlog -->\n"
        )
    }

    fn run_body(content: &str) -> &str {
        let components = agent_doc_element::element::parse(content).unwrap();
        backlog_sync::mirror_queue_component(&components)
            .unwrap()
            .content(content)
    }

    /// What `queue report-complete` does for `origin`: insert the hand-back's
    /// rows and record their root, as the IO wrapper does after the write.
    fn report(
        content: &str,
        ledger: &FollowupRootLedger,
        origin: &str,
        handback: &str,
    ) -> (String, FollowupRootLedger, Vec<String>) {
        let (updated, outcome) = plan_handback_followups(content, origin, handback);
        assert_eq!(outcome.refused, None, "{updated}");
        assert!(!outcome.inserted.is_empty(), "{updated}");
        let origin_row = crate::queue_response::queue_row_text_for_head(content, origin)
            .unwrap_or_else(|| origin.to_string());
        let next = plan_followup_root_record(
            ledger,
            &updated,
            &[origin_row.as_str(), origin],
            &outcome.inserted,
        )
        .unwrap()
        .map(|(next, _)| next)
        .unwrap_or_else(|| ledger.clone());
        (updated, next, outcome.inserted)
    }

    /// The closeout same-cycle go mirror for `ids`.
    fn closeout_mirror(content: &str, ledger: &FollowupRootLedger, ids: &[&str]) -> MirrorOutcome {
        let roots = live_followup_root_ids(content, ledger).unwrap();
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        backlog_sync::enqueue_actionable_ids_reporting_with_followup_roots(
            content,
            &ids,
            FollowUpQueuePlacement::Prepend,
            &[],
            &roots,
        )
        .unwrap()
    }

    /// The preflight queue-maintenance mirror over the whole open backlog.
    fn preflight_mirror(content: &str, ledger: &FollowupRootLedger) -> Option<Vec<QueueEntry>> {
        let entries = document_queue::parse(run_body(content)).unwrap();
        let roots = live_followup_root_ids(content, ledger).unwrap();
        document_queue::sync_backlog_into_queue_with_external_ids(
            &entries,
            &["implementlazilyowned".to_string(), "other".to_string()],
            BacklogQueueSyncMode::Prepend,
            &roots_absent_from(&roots, &entries),
        )
    }

    fn live_heads(entries: &[QueueEntry], id: &str) -> usize {
        entries
            .iter()
            .filter(|entry| matches!(entry, QueueEntry::Prompt(_)))
            .filter(|entry| {
                crate::queue_projection::queue_entry_do_id(entry).as_deref() == Some(id)
            })
            .count()
    }

    /// The ack/strike plus the later reap: the origin's `do [#id]` row is gone
    /// and only the follow-up rows remain.
    fn reap_origin(content: &str, origin_row: &str) -> String {
        let line = format!("- {origin_row}\n");
        assert!(content.contains(&line), "{content}");
        content.replacen(&line, "", 1)
    }

    fn running(content: &str, row: &str) -> String {
        let line = format!("- {row}\n");
        assert!(content.contains(&line), "{content}");
        content.replacen(&line, &format!("- 🚧 [#subagent:lzowned6#1] {row}\n"), 1)
    }

    fn strike(content: &str, row: &str) -> String {
        let live = content
            .lines()
            .find(|line| line.contains(row) && !line.contains("~~"))
            .unwrap_or_else(|| panic!("{row:?} missing: {content}"))
            .to_string();
        content.replacen(&format!("{live}\n"), &format!("- ~~{row}~~\n"), 1)
    }

    /// (a) A running follow-up of an open item stops both mirrors from
    /// re-adding the item's `do [#id]` head after that head was struck and
    /// reaped.
    #[test]
    fn a_live_followup_keeps_its_open_root_item_out_of_the_mirror() {
        let content = doc("- ☑️ [#subagent:lzowned5#1] do [#implementlazilyowned]\n");
        let (reported, ledger, inserted) = report(
            &content,
            &FollowupRootLedger::default(),
            "do [#implementlazilyowned]",
            HANDBACK,
        );
        assert_eq!(
            ledger.root_of_row(&inserted[0]),
            Some("implementlazilyowned"),
            "{ledger:?}"
        );
        let content = reap_origin(
            &reported,
            "☑️ [#subagent:lzowned5#1] do [#implementlazilyowned]",
        );
        let content = running(&content, &inserted[0]);
        assert!(
            !run_body(&content).contains("do [#implementlazilyowned]"),
            "{content}"
        );

        assert_eq!(
            closeout_mirror(&content, &ledger, &["implementlazilyowned"]),
            MirrorOutcome::Refused(MirrorRefusal::AlreadyQueued),
        );
        let synced = preflight_mirror(&content, &ledger).expect("`other` still mirrors");
        assert_eq!(live_heads(&synced, "implementlazilyowned"), 0, "{synced:?}");
        assert_eq!(live_heads(&synced, "other"), 1, "{synced:?}");
    }

    /// (b) Once the follow-up is struck the item has no queue presence, so the
    /// mirror re-adds its head as before. A reported follow-up still holds the
    /// item until the coordinator strikes it, like a reported `do [#id]` head.
    #[test]
    fn a_struck_followup_lets_the_mirror_readd_the_head() {
        let content = doc("- do [#implementlazilyowned]\n");
        let (reported, ledger, inserted) = report(
            &content,
            &FollowupRootLedger::default(),
            "do [#implementlazilyowned]",
            HANDBACK,
        );
        let content = reap_origin(&reported, "do [#implementlazilyowned]");

        let struck = strike(&content, &inserted[0]);
        let MirrorOutcome::Mirrored(next) =
            closeout_mirror(&struck, &ledger, &["implementlazilyowned"])
        else {
            panic!("a struck follow-up must not hold the head out: {struck}");
        };
        assert!(next.contains("- do [#implementlazilyowned]\n"), "{next}");
        let synced = preflight_mirror(&struck, &ledger).unwrap();
        assert_eq!(live_heads(&synced, "implementlazilyowned"), 1, "{synced:?}");

        let reported_row = content.replacen(
            &format!("- {}\n", inserted[0]),
            &format!("- ☑️ [#subagent:lzowned6#1] {} ⟦published⟧\n", inserted[0]),
            1,
        );
        assert_eq!(
            live_followup_root_ids(&reported_row, &ledger).unwrap(),
            HashSet::from(["implementlazilyowned".to_string()])
        );
        let reported_struck = strike(&reported_row, &inserted[0]);
        let synced = preflight_mirror(&reported_struck, &ledger).unwrap();
        assert_eq!(live_heads(&synced, "implementlazilyowned"), 1, "{synced:?}");
    }

    /// (c) A follow-up of a follow-up keeps the root item, even though its
    /// quote of the origin truncates the id away, and it holds the item out
    /// of the mirror after its own parent is struck.
    #[test]
    fn a_nested_followup_keeps_the_root_item_id() {
        let content = doc("- do [#implementlazilyowned]\n");
        let (reported, ledger, first) = report(
            &content,
            &FollowupRootLedger::default(),
            "do [#implementlazilyowned]",
            HANDBACK,
        );
        let content = reap_origin(&reported, "do [#implementlazilyowned]");
        let content = running(&content, &first[0]);
        // The claim names the row by its work text; the queue row carries the
        // attempt projection.
        let (nested_doc, ledger, nested) = report(&content, &ledger, &first[0], NESTED_HANDBACK);
        assert_eq!(
            ledger.root_of_row(&nested[0]),
            Some("implementlazilyowned"),
            "{ledger:?}"
        );

        // A third generation, created from the nested row while it is running.
        let nested_running = running(&nested_doc, &nested[0]);
        let third_origin = format!("🚧 [#subagent:lzowned6#1] {}", nested[0]);
        let (third_doc, ledger, third) = report(
            &nested_running,
            &ledger,
            &third_origin,
            "**Summary:** s\n**Action items:**\n- agent: Continue the last family\n",
        );
        assert!(
            !third[0].contains("implementlazilyowned"),
            "the nested quote truncates the id away: {third:?}"
        );
        assert_eq!(ledger.root_of_row(&third[0]), Some("implementlazilyowned"));

        let first_struck = strike(&third_doc, &first[0]);
        let nested_struck = strike(&first_struck, &nested[0]);
        assert_eq!(
            closeout_mirror(&nested_struck, &ledger, &["implementlazilyowned"]),
            MirrorOutcome::Refused(MirrorRefusal::AlreadyQueued),
            "{nested_struck}"
        );
        let all_struck = strike(&nested_struck, &third[0]);
        assert!(matches!(
            closeout_mirror(&all_struck, &ledger, &["implementlazilyowned"]),
            MirrorOutcome::Mirrored(_)
        ));
    }

    /// (d) A follow-up row with no recorded provenance (written before the
    /// ledger existed) leaves both mirrors exactly as they were.
    #[test]
    fn rows_without_provenance_keep_todays_mirror_behaviour() {
        let content = doc(
            "- 🚧 [#subagent:lzowned6#1] follow up on an action item from queue item “follow up on an action item from `i…”: agent: Continue the remaining rows [#q-790972e748e7]\n",
        );
        let ledger = FollowupRootLedger::default();
        assert!(
            live_followup_root_ids(&content, &ledger)
                .unwrap()
                .is_empty()
        );
        let MirrorOutcome::Mirrored(next) =
            closeout_mirror(&content, &ledger, &["implementlazilyowned"])
        else {
            panic!("an unrecorded row must not hold the head out");
        };
        assert!(next.contains("- do [#implementlazilyowned]\n"), "{next}");
        let synced = preflight_mirror(&content, &ledger).unwrap();
        assert_eq!(live_heads(&synced, "implementlazilyowned"), 1, "{synced:?}");
    }

    /// The key survives every repaint preflight applies to a follow-up row.
    #[test]
    fn the_row_key_ignores_projection_ids_tags_and_strikes() {
        let row = "follow up on an action item from `x`: A non-author approves PR 9";
        let key = followup_row_key(row).unwrap();
        for painted in [
            format!("📥 {row}"),
            format!("### 🚧 [#subagent:a#1] {row} [#q-0123456789ab]"),
            format!("⏳ {row} [operator-verify] [#q-0123456789ab]"),
            format!("☑️ [#subagent:a#1] {row} ⟦published⟧"),
            format!("~~{row}~~ — acknowledged"),
        ] {
            assert_eq!(
                followup_row_key(&painted).as_deref(),
                Some(key.as_str()),
                "{painted}"
            );
        }
    }

    /// Only a tracked id is a root: a free-text origin or a generated row id
    /// is not, and a recorded origin passes its root on.
    #[test]
    fn origin_roots() {
        let empty = FollowupRootLedger::default();
        assert_eq!(
            origin_root_item_id("### 🚧 [#subagent:a#1] do [#Alpha]", &empty).as_deref(),
            Some("alpha")
        );
        assert_eq!(origin_root_item_id("later work", &empty), None);
        assert_eq!(origin_root_item_id("do [#q-0123456789ab]", &empty), None);
        assert_eq!(
            origin_root_item_id("later work [#q-0123456789ab]", &empty),
            None
        );
        let mut ledger = FollowupRootLedger::default();
        ledger.rows.insert(
            "later work".to_string(),
            FollowupRoot {
                root_item_id: "alpha".to_string(),
            },
        );
        assert_eq!(
            origin_root_item_id("🚧 [#subagent:a#1] later work [#q-0123456789ab]", &ledger)
                .as_deref(),
            Some("alpha")
        );
    }

    /// Recording drops rows the document no longer holds, keeps struck ones,
    /// and records nothing for a rootless origin.
    #[test]
    fn recording_prunes_vanished_rows_and_skips_rootless_origins() {
        let mut ledger = FollowupRootLedger::default();
        for key in ["gone row", "struck row"] {
            ledger.rows.insert(
                key.to_string(),
                FollowupRoot {
                    root_item_id: "alpha".to_string(),
                },
            );
        }
        let content = doc("- ~~struck row~~\n- later work\n- new row\n");
        let (next, root) =
            plan_followup_root_record(&ledger, &content, &["later work"], &["new row".to_string()])
                .unwrap()
                .unwrap();
        assert_eq!(root, None);
        assert_eq!(
            next.rows.keys().cloned().collect::<Vec<_>>(),
            vec!["struck row".to_string()]
        );
    }
}
