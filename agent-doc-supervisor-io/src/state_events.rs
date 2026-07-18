//! Typed supervisor coordination facts stored in the project `state.db`.

use std::path::{Path, PathBuf};

use agent_doc_sqlite::state_store::{
    StateEventInsert, insert_state_event_in_db, load_state_events_from_db, open_state_db,
};
use agent_doc_state_backbone::{EventLedger, StateEvent};
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
    ctx: lazily::Context,
    ledgers: lazily::SlotMap<(PathBuf, String), Option<std::rc::Rc<EventLedger>>>,
    depth: usize,
}

impl LedgerScopeState {
    fn new() -> Self {
        let ctx = lazily::Context::new();
        let ledgers = lazily::SlotMap::new(&ctx);
        Self {
            ctx,
            ledgers,
            depth: 1,
        }
    }

    fn reset_entries(&mut self) {
        self.ctx = lazily::Context::new();
        self.ledgers = lazily::SlotMap::new(&self.ctx);
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

/// `#ledgerdocscope`: load only ONE document's events.
///
/// The whole-project read is O(entire project history). Measured on this repo:
/// 12,482 events totalling ~512MB of `payload_json` — read, JSON-parsed, and
/// replayed in full just to answer a single-document question like "does this
/// document have a startup miss?". `load_state_events_from_db` already accepts a
/// `document_hash` filter (indexed in SQL); the callers simply passed `None`.
///
/// Scoping the query is strictly better than caching or re-encoding the
/// full-history read, because it removes the work instead of amortizing it — and
/// unlike the run-scoped cache it also helps the first read in every fresh
/// process.
fn load_document_ledger_uncached(project_root: &Path, document_hash: &str) -> Result<EventLedger> {
    let conn = open_state_db(project_root)?;
    let mut ledger = EventLedger::new();
    for row in load_state_events_from_db(&conn, Some(document_hash))? {
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

/// Shared per-document ledger read. Inside a scope a document's events are
/// loaded once and later reads return the same `Rc`, so the repeated reads in
/// one pass (`take_superseded_startup_miss` calls `load_startup_miss` again)
/// cost a map lookup instead of another query.
pub(crate) fn load_document_ledger_shared(
    project_root: &Path,
    document_hash: &str,
) -> Result<std::rc::Rc<EventLedger>> {
    let active = LEDGER_SCOPE.with(|scope| scope.borrow().is_some());
    if !active {
        return Ok(std::rc::Rc::new(load_document_ledger_uncached(
            project_root,
            document_hash,
        )?));
    }

    let key = (project_root.to_path_buf(), document_hash.to_string());
    let cached = LEDGER_SCOPE.with(|scope| {
        let scope = scope.borrow();
        let state = scope.as_ref()?;
        state.ledgers.get(&state.ctx, &key).flatten()
    });
    if let Some(ledger) = cached {
        return Ok(ledger);
    }

    // A load failure is never cached — a transient `state.db` error must not be
    // remembered as "this document has no events".
    let loaded = std::rc::Rc::new(load_document_ledger_uncached(project_root, document_hash)?);
    LEDGER_SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        if let Some(state) = scope.as_mut() {
            let value = loaded.clone();
            state
                .ledgers
                .get_or_insert_with(&state.ctx, key, move |_| Some(value.clone()));
        }
    });
    Ok(loaded)
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
