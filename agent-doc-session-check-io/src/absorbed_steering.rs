//! Operator steering absorbed into a commit baseline (`#steerbaselineabsorb`).
//!
//! ## Spec
//! - A turn-boundary steering report (`respond` / `write --commit` terminal
//!   output, the post-commit `session-check`, the hook after close) compares the
//!   live document with the cycle's PREFLIGHT baseline. Operator text typed while
//!   the closeout was committing can land inside that commit: it is then in the
//!   committed baseline, the report still calls it a new operator prompt
//!   ("answer it in the next cycle"), and the next preflight diffs the editor
//!   against the commit, finds nothing, and reports `no_changes: true`. The
//!   prompt was silently skipped (observed 2026-10-04 on agent-doc-bugs.md).
//! - When a reported item is an exchange prompt whose every non-blank line is
//!   already a line of the committed baseline, [`record_reported`] persists it in
//!   a per-document ledger (`state.db` runtime key
//!   `midturn_steering:absorbed:<doc>`), the same way `#chatprompt` (GH #125)
//!   persists a prompt the document diff cannot show.
//! - The next preflight reads [`unanswered_prompts`] and carries them like chat
//!   prompts: `absorbed_steering_prompts` in the contract, a `prompt_target` in
//!   `user_intent_prompt_changes`, and `no_changes: false`. An admitted cycle
//!   that carried them calls [`mark_carried`].
//! - An entry is consumed once a cycle carried it ([`mark_carried`]) or once a
//!   later committed response cycle (started at or after the entry was noted,
//!   and not the cycle that absorbed it) closed out. Consumed entries stay in
//!   the ledger until the TTL so every steering consumer can drop the same item
//!   ([`drop_consumed`]): the hook stops re-surfacing an answered prompt.
//!
//! ## Agentic Contracts
//! - Never fails a closeout or a hook: callers log and continue on error.
//! - Only exchange prompts are recorded; queue items stay with the queue drain,
//!   which reads the committed queue component directly.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use agent_doc_document_realtime::midturn_steering::{SteeringChange, SteeringItem, SteeringSource};

/// Seconds an entry stays in the ledger (pending or consumed). Mirrors the chat
/// prompt ledger TTL: a turn that never picked it up within this window is long
/// over.
pub const ABSORBED_STEERING_TTL_SECS: u64 =
    agent_doc_prompt_contract::chat_prompt::CHAT_PROMPT_LEDGER_TTL_SECS;

/// Entries kept per document; the newest survive.
pub const ABSORBED_STEERING_CAP: usize = 16;

/// `diff_type` a preflight contract reports when the document did not change
/// and its only work is absorbed steering.
pub const ABSORBED_STEERING_DIFF_TYPE: &str = "absorbed_steering";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsorbedStatus {
    /// Reported as pending steering; no cycle has carried or answered it yet.
    Pending,
    /// Carried by an admitted cycle or answered by a later committed response.
    Consumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsorbedSteeringEntry {
    /// The operator's text, verbatim as the steering report carried it.
    pub text: String,
    /// Epoch seconds the report noted it.
    pub noted_at: u64,
    /// The cycle whose closeout baseline absorbed it, when known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub absorbed_by_cycle: String,
    pub status: AbsorbedStatus,
    /// The cycle that carried or answered it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbsorbedSteeringLedger {
    pub entries: Vec<AbsorbedSteeringEntry>,
}

/// What a prune knows about the document's latest cycle.
#[derive(Debug, Clone, Default)]
pub struct LatestCommittedCycle {
    pub cycle_id: String,
    pub started_at: u64,
}

fn identity(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Does `baseline` (frontmatter excluded) carry every non-blank line of
/// `text` as a whole line?
pub fn text_in_baseline(baseline: &str, text: &str) -> bool {
    let (_, body) = agent_doc_frontmatter::frontmatter::split_frontmatter_parts(baseline);
    let lines: std::collections::BTreeSet<&str> = body.lines().map(str::trim).collect();
    let mut wanted = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .peekable();
    wanted.peek().is_some() && wanted.all(|line| lines.contains(line))
}

/// An item the ledger tracks: an exchange prompt that is still operator text
/// (not a removal).
fn is_absorbable(item: &SteeringItem) -> bool {
    item.source == SteeringSource::Exchange && item.change != SteeringChange::Deleted
}

impl AbsorbedSteeringLedger {
    /// Add every reported exchange prompt whose text is inside `baseline`.
    /// Returns the texts newly recorded. An item the ledger already knows (in
    /// either status) is not re-added: a consumed prompt must never re-open.
    pub fn record(
        &mut self,
        items: &[SteeringItem],
        baseline: &str,
        cycle_id: &str,
        now: u64,
    ) -> Vec<String> {
        let mut added = Vec::new();
        for item in items.iter().filter(|item| is_absorbable(item)) {
            let text = item.verbatim.trim();
            if text.is_empty() || !text_in_baseline(baseline, text) {
                continue;
            }
            let id = identity(text);
            if self.entries.iter().any(|entry| identity(&entry.text) == id) {
                continue;
            }
            self.entries.push(AbsorbedSteeringEntry {
                text: text.to_string(),
                noted_at: now,
                absorbed_by_cycle: cycle_id.to_string(),
                status: AbsorbedStatus::Pending,
                consumed_by: None,
            });
            added.push(text.to_string());
        }
        added
    }

    /// Hygiene: drop entries past the TTL, consume pending entries a later
    /// committed response cycle answered, keep the newest
    /// [`ABSORBED_STEERING_CAP`]. Returns whether anything changed.
    pub fn prune(&mut self, now: u64, latest: Option<&LatestCommittedCycle>) -> bool {
        let before = self.clone();
        self.entries
            .retain(|entry| now.saturating_sub(entry.noted_at) <= ABSORBED_STEERING_TTL_SECS);
        if let Some(latest) = latest {
            for entry in &mut self.entries {
                if entry.status == AbsorbedStatus::Pending
                    && latest.started_at >= entry.noted_at
                    && latest.cycle_id != entry.absorbed_by_cycle
                {
                    entry.status = AbsorbedStatus::Consumed;
                    entry.consumed_by = Some(latest.cycle_id.clone());
                }
            }
        }
        if self.entries.len() > ABSORBED_STEERING_CAP {
            let excess = self.entries.len() - ABSORBED_STEERING_CAP;
            self.entries.drain(..excess);
        }
        *self != before
    }

    /// Pending texts, oldest first.
    pub fn pending(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.status == AbsorbedStatus::Pending)
            .map(|entry| entry.text.clone())
            .collect()
    }

    /// Mark the pending entries matching `texts` consumed by `cycle_id`.
    pub fn consume(&mut self, texts: &[String], cycle_id: &str) -> bool {
        let ids: Vec<String> = texts.iter().map(|text| identity(text)).collect();
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.status == AbsorbedStatus::Pending && ids.contains(&identity(&entry.text)) {
                entry.status = AbsorbedStatus::Consumed;
                entry.consumed_by = Some(cycle_id.to_string());
                changed = true;
            }
        }
        changed
    }

    /// Whether `item` is a consumed entry: carried or answered already.
    pub fn is_consumed(&self, item: &SteeringItem) -> bool {
        if !is_absorbable(item) {
            return false;
        }
        let id = identity(&item.verbatim);
        self.entries
            .iter()
            .any(|entry| entry.status == AbsorbedStatus::Consumed && identity(&entry.text) == id)
    }
}

fn state_key(file: &Path) -> String {
    agent_doc_queue_io::subagent_dispatch::midturn_steering_state_key("absorbed", file)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The document's latest cycle when it committed a response.
fn latest_committed_cycle(file: &Path) -> Option<LatestCommittedCycle> {
    agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .filter(|cycle| {
            cycle.phase == agent_doc_turn::CyclePhase::Committed && cycle.response_sha256.is_some()
        })
        .map(|cycle| LatestCommittedCycle {
            cycle_id: cycle.cycle_id,
            started_at: cycle.started_at,
        })
}

fn open_conn(file: &Path) -> Result<Option<agent_doc_sqlite::state_store::Connection>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    Ok(Some(agent_doc_sqlite::state_store::open_state_db(&root)?))
}

fn load(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
) -> Result<AbsorbedSteeringLedger> {
    Ok(
        agent_doc_sqlite::state_store::load_project_runtime_state_from_db(conn, &state_key(file))?
            .map(|raw| serde_json::from_str(&raw).context("parse absorbed steering ledger"))
            .transpose()?
            .unwrap_or_default(),
    )
}

fn store(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
    ledger: &AbsorbedSteeringLedger,
) -> Result<()> {
    let key = state_key(file);
    if ledger.entries.is_empty() {
        agent_doc_sqlite::state_store::clear_project_runtime_state_in_db(conn, &key)?;
    } else {
        agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
            conn,
            &key,
            &serde_json::to_string(ledger)?,
            now_secs().saturating_mul(1000),
        )?;
    }
    Ok(())
}

/// Load and prune the ledger, persisting the pruned form.
fn load_pruned(
    conn: &agent_doc_sqlite::state_store::Connection,
    file: &Path,
) -> Result<AbsorbedSteeringLedger> {
    let mut ledger = load(conn, file)?;
    if ledger.prune(now_secs(), latest_committed_cycle(file).as_ref()) {
        store(conn, file, &ledger)?;
    }
    Ok(ledger)
}

/// The committed baseline the closeout recorded (HEAD when no snapshot).
fn committed_baseline(file: &Path) -> Option<String> {
    agent_doc_snapshot_io::load_document_baseline(file)
        .ok()
        .flatten()
        .or_else(|| agent_doc_git_io::revision::show_head(file).ok().flatten())
}

/// Persist reported steering whose text the committed baseline already carries
/// (`#steerbaselineabsorb`). Called where steering is reported as pending at
/// or after closeout. Returns the newly recorded texts.
pub fn record_reported(file: &Path, items: &[SteeringItem], cycle_id: &str) -> Result<Vec<String>> {
    if !items.iter().any(is_absorbable) {
        return Ok(Vec::new());
    }
    let Some(baseline) = committed_baseline(file) else {
        return Ok(Vec::new());
    };
    let Some(conn) = open_conn(file)? else {
        return Ok(Vec::new());
    };
    let mut ledger = load_pruned(&conn, file)?;
    let added = ledger.record(items, &baseline, cycle_id, now_secs());
    if !added.is_empty() {
        store(&conn, file, &ledger)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "absorbed_steering_recorded file={} cycle={cycle_id} count={} (#steerbaselineabsorb)",
                file.display(),
                added.len()
            ),
        );
    }
    Ok(added)
}

/// [`record_reported`] that never fails its caller (closeout / hook paths).
pub fn record_reported_logged(file: &Path, items: &[SteeringItem], cycle_id: &str) {
    if let Err(err) = record_reported(file, items, cycle_id) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "absorbed_steering_record_failed file={} error={err:#} (#steerbaselineabsorb)",
                file.display()
            ),
        );
    }
}

/// Absorbed steering no cycle has carried or answered yet, oldest first.
pub fn unanswered_prompts(file: &Path) -> Result<Vec<String>> {
    let Some(conn) = open_conn(file)? else {
        return Ok(Vec::new());
    };
    Ok(load_pruned(&conn, file)?.pending())
}

/// An admitted cycle carried `texts` in its contract: they are consumed.
pub fn mark_carried(file: &Path, texts: &[String], cycle_id: &str) -> Result<()> {
    if texts.is_empty() {
        return Ok(());
    }
    let Some(conn) = open_conn(file)? else {
        return Ok(());
    };
    let mut ledger = load(&conn, file)?;
    if ledger.consume(texts, cycle_id) {
        store(&conn, file, &ledger)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "absorbed_steering_carried file={} cycle={cycle_id} count={} (#steerbaselineabsorb)",
                file.display(),
                texts.len()
            ),
        );
    }
    Ok(())
}

/// Drop steering items the ledger already consumed, so an answered prompt is
/// never re-surfaced. Errors keep every item (fail open: loss is worse than a
/// repeat).
pub fn drop_consumed(file: &Path, items: &mut Vec<SteeringItem>) {
    if !items.iter().any(is_absorbable) {
        return;
    }
    let ledger = match open_conn(file).and_then(|conn| match conn {
        Some(conn) => load_pruned(&conn, file).map(Some),
        None => Ok(None),
    }) {
        Ok(Some(ledger)) => ledger,
        Ok(None) => return,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "absorbed_steering_filter_failed file={} error={err:#} (#steerbaselineabsorb)",
                    file.display()
                ),
            );
            return;
        }
    };
    items.retain(|item| !ledger.is_consumed(item));
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document_realtime::midturn_steering::SteeringDispatch;

    fn exchange(text: &str) -> SteeringItem {
        SteeringItem {
            source: SteeringSource::Exchange,
            change: SteeringChange::Added,
            dispatch: SteeringDispatch::AddressNow,
            current_item: false,
            verbatim: text.to_string(),
            previous: None,
            presets: Vec::new(),
            possibly_partial: false,
            owner: None,
            explicit: false,
        }
    }

    const BASELINE: &str = "---\nagent_doc_session: t\n---\n\n<!-- agent:exchange -->\ntesting to see if you pick this up.\n<!-- /agent:exchange -->\n";

    #[test]
    fn only_prompts_the_baseline_carries_whole_are_recorded() {
        let mut ledger = AbsorbedSteeringLedger::default();
        let added = ledger.record(
            &[
                exchange("testing to see if you pick this up."),
                exchange("testing to see"),
                exchange("not committed yet"),
            ],
            BASELINE,
            "cycle-1",
            100,
        );
        assert_eq!(
            added,
            vec!["testing to see if you pick this up.".to_string()]
        );
        // Re-reporting the same item does not add it twice.
        assert!(
            ledger
                .record(
                    &[exchange("testing  to see if you pick this up.")],
                    BASELINE,
                    "c",
                    101
                )
                .is_empty()
        );
        assert_eq!(ledger.pending().len(), 1);
    }

    #[test]
    fn a_later_committed_response_consumes_and_the_absorbing_cycle_does_not() {
        let mut ledger = AbsorbedSteeringLedger::default();
        ledger.record(
            &[exchange("testing to see if you pick this up.")],
            BASELINE,
            "cycle-1",
            100,
        );
        // The absorbing cycle itself never consumes, even in the same second.
        assert!(!ledger.prune(
            110,
            Some(&LatestCommittedCycle {
                cycle_id: "cycle-1".into(),
                started_at: 100,
            })
        ));
        // A cycle that started before the report did not answer it.
        ledger.prune(
            110,
            Some(&LatestCommittedCycle {
                cycle_id: "cycle-0".into(),
                started_at: 90,
            }),
        );
        assert_eq!(ledger.pending().len(), 1);
        assert!(ledger.prune(
            120,
            Some(&LatestCommittedCycle {
                cycle_id: "cycle-2".into(),
                started_at: 105,
            })
        ));
        assert!(ledger.pending().is_empty());
        assert!(ledger.is_consumed(&exchange("testing to see if you pick this up.")));
        // A consumed prompt never re-opens when reported again.
        assert!(
            ledger
                .record(
                    &[exchange("testing to see if you pick this up.")],
                    BASELINE,
                    "c",
                    130
                )
                .is_empty()
        );
        // The TTL finally drops it.
        assert!(ledger.prune(100 + ABSORBED_STEERING_TTL_SECS + 1, None));
        assert!(ledger.entries.is_empty());
    }

    #[test]
    fn carrying_consumes() {
        let mut ledger = AbsorbedSteeringLedger::default();
        ledger.record(
            &[exchange("testing to see if you pick this up.")],
            BASELINE,
            "cycle-1",
            100,
        );
        assert!(ledger.consume(
            &["testing to see if you pick this up.".to_string()],
            "cycle-2"
        ));
        assert!(ledger.pending().is_empty());
        assert_eq!(ledger.entries[0].consumed_by.as_deref(), Some("cycle-2"));
    }
}
