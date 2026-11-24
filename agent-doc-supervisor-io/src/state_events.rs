//! Typed supervisor coordination facts stored in the project `state.db`.

use std::path::{Path, PathBuf};

use agent_doc_sqlite::state_store::{
    StateEventInsert, count_state_events_for_document_in_db, insert_state_event_in_db,
    load_state_events_by_fact_types_for_document_from_db, open_state_db,
};
use agent_doc_state_backbone::{EventLedger, StateEvent};
use agent_doc_state_scope::LocalReadScope;
use anyhow::{Context, Result};

pub(crate) struct DocumentStateIdentity {
    pub project_root: PathBuf,
    pub canonical_file: PathBuf,
    pub document_hash: String,
}

pub(crate) fn document_state_identity(file: &Path) -> Result<Option<DocumentStateIdentity>> {
    let canonical_file = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(project_root) = agent_doc_fs::find_project_root(&canonical_file) else {
        return Ok(None);
    };
    let document_hash = agent_doc_fs::document_state_hash(&canonical_file)?;
    Ok(Some(DocumentStateIdentity {
        project_root,
        canonical_file,
        document_hash,
    }))
}

// `#ledgerscope`: run-scoped state-ledger cache.
//
// `load_ledger` opens `state.db`, reads EVERY state event, JSON-parses each one,
// and replays them into an `EventLedger` — and callers then project a single
// document out of the result. `load_startup_miss` does this per document, and
// `take_superseded_startup_miss` calls it again, so a sync pass reloaded the
// whole ledger several times per file.
//
// Measured on the agent-doc focus-sync hot path: this block was 995ms of a
// 1024ms per-document ownership loop — essentially the entire `ownership_proof`
// phase and ~40% of a 2.5s sync, for two documents.
//
// The ledger is one shared input per project, so a scope loads it once and every
// per-document projection derives from that. Same shape as the other observation
// caches: opt-in (nothing caches without a scope, so long-lived processes are
// unaffected), reference-counted, and invalidated by `append_event` so a writer
// never reads its own stale view.
thread_local! {
    static LEDGER_SCOPE: std::cell::RefCell<Option<LedgerScopeState>> =
        const { std::cell::RefCell::new(None) };
}

struct LedgerScopeState {
    /// The scope this cache's slots live in (`#stategraphjoin`). Its lifetime is
    /// exactly the ledger scope's depth-counted extent.
    scope: LocalReadScope,
    ledgers: lazily::ComputedMap<(PathBuf, String, LedgerFamily), Option<std::rc::Rc<EventLedger>>>,
    depth: usize,
}

impl LedgerScopeState {
    fn new() -> Self {
        let scope = LocalReadScope::new();
        let ledgers = lazily::ComputedMap::new(scope.ctx());
        Self {
            scope,
            ledgers,
            depth: 1,
        }
    }

    fn reset_entries(&mut self) {
        // Re-taking the scope IS the invalidation: the ledger memo must not outlive
        // its scope, so the scope boundary drops the whole graph rather than evicting
        // entries one at a time.
        self.scope = LocalReadScope::new();
        self.ledgers = lazily::ComputedMap::new(self.scope.ctx());
    }
}

/// RAII guard for a state-ledger read scope on this thread.
#[must_use = "the ledger is cached only while the guard is alive"]
pub struct StateLedgerScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

/// Open a command-scoped state-ledger cache. Nested calls are reference counted.
pub fn begin_state_ledger_scope() -> StateLedgerScope {
    LEDGER_SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        match scope.as_mut() {
            Some(state) => state.depth += 1,
            None => *scope = Some(LedgerScopeState::new()),
        }
    });
    StateLedgerScope {
        _not_send: std::marker::PhantomData,
    }
}

impl Drop for StateLedgerScope {
    fn drop(&mut self) {
        LEDGER_SCOPE.with(|scope| {
            let mut scope = scope.borrow_mut();
            let finished = match scope.as_mut() {
                Some(state) => {
                    state.depth -= 1;
                    state.depth == 0
                }
                None => false,
            };
            if finished {
                *scope = None;
            }
        });
    }
}

/// Drop any cached ledger. Called after a write so a reader in the same scope
/// never sees a pre-write view.
fn invalidate_ledger_cache() {
    LEDGER_SCOPE.with(|scope| {
        if let Some(state) = scope.borrow_mut().as_mut() {
            state.reset_entries();
        }
    });
}

/// The closed fact family a supervisor reader folds (`#q-c71d106debc4`,
/// dogfood key histreplay).
///
/// `#ledgerdocscope` narrowed these reads from the whole project to one
/// document, but one document can still carry an unbounded history: live
/// `haiven-dev` `sdk-dev.md` held 38,776 rows / 139MB of `payload_json` (112MB
/// of it `visible_write_commit_candidate_observed` images), so every
/// startup-miss or recycle read parsed all of it (~200-250ms per call) to look
/// at a handful of rows. Each reader projects exactly one field, and
/// `DocumentStateProjection::apply_fact` writes that field from its family
/// alone and reads no other state, so folding the family in append order gives
/// the same field as the whole-document replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum LedgerFamily {
    /// Writes `SupervisorProjection::startup_miss`.
    StartupMiss,
    /// Writes `SupervisorProjection::recycle`.
    Recycle,
}

impl LedgerFamily {
    pub(crate) fn fact_types(self) -> &'static [&'static str] {
        match self {
            Self::StartupMiss => &["startup_miss_recorded", "startup_miss_cleared"],
            Self::Recycle => &[
                "supervisor_recycle_requested",
                "supervisor_recycle_started",
                "supervisor_recycle_settled",
            ],
        }
    }
}

fn load_document_family_uncached(
    project_root: &Path,
    document_hash: &str,
    family: LedgerFamily,
) -> Result<EventLedger> {
    let conn = open_state_db(project_root)?;
    let mut ledger = EventLedger::new();
    for row in load_state_events_by_fact_types_for_document_from_db(
        &conn,
        document_hash,
        family.fact_types(),
    )? {
        let event: StateEvent = serde_json::from_str(&row.payload_json).with_context(|| {
            format!(
                "parse supervisor state event {} from state.db",
                row.event_id
            )
        })?;
        ledger.append(event);
    }
    Ok(ledger)
}

/// Shared per-document, per-family ledger read. Inside a scope a document's
/// family is loaded once and later reads return the same `Rc`, so the repeated
/// reads in one pass (`take_superseded_startup_miss` calls `load_startup_miss`
/// again) cost a map lookup instead of another query.
pub(crate) fn load_document_family_shared(
    project_root: &Path,
    document_hash: &str,
    family: LedgerFamily,
) -> Result<std::rc::Rc<EventLedger>> {
    let active = LEDGER_SCOPE.with(|scope| scope.borrow().is_some());
    if !active {
        return Ok(std::rc::Rc::new(load_document_family_uncached(
            project_root,
            document_hash,
            family,
        )?));
    }

    let key = (
        project_root.to_path_buf(),
        document_hash.to_string(),
        family,
    );
    let cached = LEDGER_SCOPE.with(|scope| {
        let scope = scope.borrow();
        let state = scope.as_ref()?;
        state.ledgers.get(state.scope.ctx(), &key).flatten()
    });
    if let Some(ledger) = cached {
        return Ok(ledger);
    }

    // A load failure is never cached — a transient `state.db` error must not be
    // remembered as "this document has no events".
    let loaded = std::rc::Rc::new(load_document_family_uncached(
        project_root,
        document_hash,
        family,
    )?);
    LEDGER_SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        if let Some(state) = scope.as_mut() {
            let value = loaded.clone();
            state
                .ledgers
                .get_or_insert_with(state.scope.ctx(), key, move |_, _| Some(value.clone()));
        }
    });
    Ok(loaded)
}

/// The document's epoch — `EventLedger::document_epoch` over every retained
/// event of the document — as an indexed `COUNT(*)` instead of a full replay.
/// Writers stamp it into their next event id.
pub(crate) fn document_epoch(project_root: &Path, document_hash: &str) -> Result<u64> {
    let conn = open_state_db(project_root)?;
    count_state_events_for_document_in_db(&conn, document_hash)
}

pub(crate) fn append_event(project_root: &Path, event: &StateEvent) -> Result<bool> {
    let conn = open_state_db(project_root)?;
    let payload_json = serde_json::to_string(event).context("serialize supervisor state event")?;
    let inserted = insert_state_event_in_db(
        &conn,
        &StateEventInsert {
            event_id: &event.event_id,
            document_hash: event.document_hash(),
            domain: event.domain().label(),
            fact_type: event.fact.label(),
            payload_json: &payload_json,
        },
    );
    // `#ledgerscope`: a write makes any cached ledger stale. Invalidate before
    // returning so a reader in the same scope cannot observe a pre-write view.
    invalidate_ledger_cache();
    inserted
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_sqlite::state_store::load_state_events_from_db;
    use agent_doc_state_backbone::StateFact;

    fn whole_document_ledger(project_root: &Path, document_hash: &str) -> EventLedger {
        let conn = open_state_db(project_root).unwrap();
        let mut ledger = EventLedger::new();
        for row in load_state_events_from_db(&conn, Some(document_hash)).unwrap() {
            ledger.append(serde_json::from_str(&row.payload_json).unwrap());
        }
        ledger
    }

    fn startup_miss(hash: &str, pane: &str, timestamp: u64) -> StateFact {
        StateFact::StartupMissRecorded {
            document_hash: hash.to_string(),
            file: "/doc.md".to_string(),
            pane_id: pane.to_string(),
            session_id: format!("session-{pane}"),
            harness: "claude".to_string(),
            timestamp,
            origin: "fresh_start".to_string(),
            cycle_baseline_id: None,
        }
    }

    fn startup_miss_cleared(hash: &str, pane: &str, timestamp: u64) -> StateFact {
        StateFact::StartupMissCleared {
            document_hash: hash.to_string(),
            pane_id: pane.to_string(),
            session_id: format!("session-{pane}"),
            timestamp,
        }
    }

    fn recycle(hash: &str, phase: &str, recycle_epoch: u64) -> StateFact {
        let (document_hash, reason, marked_secs) = (
            hash.to_string(),
            format!("reason-{recycle_epoch}"),
            recycle_epoch * 10,
        );
        match phase {
            "requested" => StateFact::SupervisorRecycleRequested {
                document_hash,
                reason,
                recycle_epoch,
                marked_secs,
            },
            "started" => StateFact::SupervisorRecycleStarted {
                document_hash,
                reason,
                recycle_epoch,
                marked_secs,
            },
            _ => StateFact::SupervisorRecycleSettled {
                document_hash,
                reason,
                recycle_epoch,
                marked_secs,
            },
        }
    }

    /// The family's SQL fact types are the facts' own labels, so a renamed
    /// label cannot silently empty a family read.
    #[test]
    fn family_fact_types_are_the_fact_labels() {
        assert_eq!(
            LedgerFamily::StartupMiss.fact_types(),
            [
                startup_miss("h", "%1", 1).label(),
                startup_miss_cleared("h", "%1", 1).label(),
            ]
        );
        assert_eq!(
            LedgerFamily::Recycle.fact_types(),
            [
                recycle("h", "requested", 1).label(),
                recycle("h", "started", 1).label(),
                recycle("h", "settled", 1).label(),
            ]
        );
    }

    /// `#q-c71d106debc4`: the family-scoped fold and the indexed epoch equal the
    /// whole-document replay they replace, with unrelated content-bearing facts,
    /// another document's facts, a stale identity-CAS clear, and an out-of-order
    /// recycle generation interleaved.
    #[test]
    fn family_reads_and_indexed_epoch_match_whole_document_replay() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let (doc, other) = ("doc-hash", "other-hash");
        let unrelated = |hash: &str, n: u64| StateFact::BaselineSaved {
            document_hash: hash.to_string(),
            cycle_id: format!("cycle-{n}"),
            baseline_hash: format!("hash-{n}"),
            baseline_path: Some("x".repeat(4096)),
        };
        let facts = [
            unrelated(doc, 1),
            startup_miss(doc, "%1", 100),
            recycle(doc, "requested", 1),
            unrelated(other, 2),
            startup_miss(other, "%9", 900),
            recycle(other, "requested", 50),
            unrelated(doc, 3),
            recycle(doc, "started", 1),
            startup_miss_cleared(doc, "%1", 999), // stale: wrong timestamp
            recycle(doc, "settled", 1),
            recycle(doc, "requested", 3),
            unrelated(doc, 4),
            recycle(doc, "settled", 2), // older generation, ignored
            startup_miss(doc, "%2", 200),
            startup_miss_cleared(doc, "%1", 100), // stale: superseded miss
        ];
        for (n, fact) in facts.into_iter().enumerate() {
            append_event(dir.path(), &StateEvent::new(format!("event-{n}"), fact)).unwrap();
        }

        let whole = whole_document_ledger(dir.path(), doc);
        let whole_projection = whole.project_document(doc).unwrap();
        let startup = load_document_family_shared(dir.path(), doc, LedgerFamily::StartupMiss)
            .unwrap()
            .project_document(doc)
            .unwrap();
        let recycle = load_document_family_shared(dir.path(), doc, LedgerFamily::Recycle)
            .unwrap()
            .project_document(doc)
            .unwrap();
        assert_eq!(
            startup.supervisor.startup_miss,
            whole_projection.supervisor.startup_miss
        );
        assert_eq!(
            startup.supervisor.startup_miss.as_ref().unwrap().pane_id,
            "%2"
        );
        assert_eq!(
            recycle.supervisor.recycle,
            whole_projection.supervisor.recycle
        );
        assert_eq!(recycle.supervisor.recycle.recycle_epoch, 3);

        assert_eq!(
            document_epoch(dir.path(), doc).unwrap(),
            whole.document_epoch(doc)
        );
        assert_eq!(document_epoch(dir.path(), doc).unwrap(), 12);
        assert_eq!(document_epoch(dir.path(), "absent").unwrap(), 0);

        // A re-emitted event id is not a new event: neither count moves.
        append_event(dir.path(), &StateEvent::new("event-0", unrelated(doc, 1))).unwrap();
        assert_eq!(document_epoch(dir.path(), doc).unwrap(), 12);
        assert_eq!(
            whole_document_ledger(dir.path(), doc).document_epoch(doc),
            12
        );
    }
}
