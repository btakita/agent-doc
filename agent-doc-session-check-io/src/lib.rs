use anyhow::{Context, Result};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod backlog_guards;
pub mod closeout_guards;
pub mod command;
pub mod detect;
pub mod guard_modes;
pub mod partial_staging;
pub mod pending_capture;
pub mod pending_guards;
pub mod prompt_bearing;
pub mod queue_head_guards;
pub mod queue_head_provenance_guards;
pub mod response_guards;
pub mod write_pending_checks;

pub use backlog_guards::*;
pub use closeout_guards::*;
pub use command::*;
pub use detect::*;
pub use guard_modes::*;
pub use partial_staging::*;
pub use pending_capture::*;
pub use pending_guards::*;
pub use prompt_bearing::*;
pub use queue_head_guards::*;
pub use queue_head_provenance_guards::*;
pub use response_guards::*;
pub use write_pending_checks::*;

thread_local! {
    static FORCE_DISK_RESOLUTION: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn with_force_disk_resolution<T>(
    force_disk: bool,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if !force_disk {
        return f();
    }
    FORCE_DISK_RESOLUTION.with(|slot| {
        let previous = slot.replace(true);
        let result = f();
        slot.set(previous);
        result
    })
}

fn force_disk_resolution_enabled() -> bool {
    FORCE_DISK_RESOLUTION.with(Cell::get)
}

thread_local! {
    /// `#sccurrentpass`: pass-scoped memo of the resolved current document.
    ///
    /// `session-check` is a read-only sweep over one document: ~8 independent
    /// guards each called [`resolve_current_document`], and every call was a
    /// full controller round trip returning the whole document text. On a live
    /// editor-attached document that cost ~0.5-1s apiece, so one sweep spent
    /// several seconds re-fetching the same text — the dominant term in
    /// `Compact Exchange` wall time, which runs a sweep after its writeback.
    ///
    /// Worse, it was not only slow but *inconsistent*: because the operator can
    /// type mid-sweep, guards observed different document versions within a
    /// single pass (`text_len` 38865 -> 38866 -> 38932 in one recorded run), so
    /// two guards could disagree about the same document. A pass evaluates one
    /// document version now: the first resolve inside the scope wins and every
    /// later guard reads that same value.
    ///
    /// The memo is opened explicitly by the `session-check` entry points and is
    /// never global ambient state: outside a scope, resolution is unchanged.
    /// Force-disk resolution bypasses it entirely (different authority).
    static CURRENT_DOCUMENT_PASS: RefCell<Option<HashMap<PathBuf, agent_doc_document_realtime_io::CurrentDocument>>> =
        const { RefCell::new(None) };
}

/// Run `f` with a pass-scoped current-document memo active (`#sccurrentpass`).
///
/// Nested calls reuse the outer pass rather than installing a second memo, so a
/// sweep that delegates into another entry point still sees one document
/// version. Any document mutation performed inside a pass must call
/// [`invalidate_current_document_pass`]; `session-check` guards are read-only
/// over the document text, which is what makes this safe.
pub(crate) fn with_current_document_pass<T>(f: impl FnOnce() -> T) -> T {
    let installed = CURRENT_DOCUMENT_PASS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return false;
        }
        *slot = Some(HashMap::new());
        true
    });
    let result = f();
    if installed {
        CURRENT_DOCUMENT_PASS.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
    result
}

/// Drop `file` from the active pass memo so the next resolve re-reads it.
///
/// Call this after any path that can change the document's current text while a
/// pass is open.
pub(crate) fn invalidate_current_document_pass(file: &Path) {
    CURRENT_DOCUMENT_PASS.with(|slot| {
        if let Some(entries) = slot.borrow_mut().as_mut() {
            entries.remove(file);
        }
    });
}

fn current_document_pass_hit(
    file: &Path,
) -> Option<agent_doc_document_realtime_io::CurrentDocument> {
    CURRENT_DOCUMENT_PASS.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|entries| entries.get(file).cloned())
    })
}

fn record_current_document_pass(
    file: &Path,
    document: &agent_doc_document_realtime_io::CurrentDocument,
) {
    CURRENT_DOCUMENT_PASS.with(|slot| {
        if let Some(entries) = slot.borrow_mut().as_mut() {
            entries.insert(file.to_path_buf(), document.clone());
        }
    });
}

pub(crate) fn resolve_current_document(
    file: &Path,
    source: &str,
) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
    if force_disk_resolution_enabled() {
        return agent_doc_document_realtime_io::resolve_disk_current_document(
            file,
            &format!("session-check {source}"),
        );
    }
    if let Some(hit) = current_document_pass_hit(file) {
        return Ok(hit);
    }
    let resolved = agent_doc_document_realtime_io::try_resolve_current_document_with_source(
        file,
        &format!("session-check {source}"),
    )
    .with_context(|| {
        format!(
            "session-check {source}: resolve current document {}",
            file.display()
        )
    })?;
    record_current_document_pass(file, &resolved);
    Ok(resolved)
}

pub(crate) fn resolve_current_document_with_force_disk(
    file: &Path,
    source: &str,
    force_disk: bool,
) -> Result<agent_doc_document_realtime_io::CurrentDocument> {
    if !force_disk {
        return resolve_current_document(file, source);
    }
    agent_doc_document_realtime_io::resolve_disk_current_document(
        file,
        &format!("session-check {source}"),
    )
}

pub(crate) fn resolve_current_document_content(file: &Path, source: &str) -> Result<String> {
    Ok(resolve_current_document(file, source)?.into_content())
}

pub(crate) fn resolve_disk_document_content(file: &Path, source: &str) -> Result<String> {
    agent_doc_document_realtime_io::resolve_disk_current_document_content(
        file,
        &format!("session-check {source}"),
    )
}

pub(crate) fn resolve_current_document_content_with_force_disk(
    file: &Path,
    source: &str,
    force_disk: bool,
) -> Result<String> {
    Ok(resolve_current_document_with_force_disk(file, source, force_disk)?.into_content())
}

pub(crate) struct CapturedResponseGuardEvidence {
    pub response_body: String,
    pub capture_committed: bool,
}

pub(crate) fn captured_response_guard_evidence(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    capture_id: &str,
) -> Result<Option<CapturedResponseGuardEvidence>> {
    if let Some(projected) =
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?
        && state.response_sha256.as_deref() == Some(projected.response_sha256.as_str())
        && state.cycle_id == projected.cycle_id
    {
        return Ok(Some(CapturedResponseGuardEvidence {
            response_body: projected.response_body,
            capture_committed: state.phase == agent_doc_turn::CyclePhase::Committed,
        }));
    }

    Ok(None)
}

pub(crate) fn operator_live_buffer_contains_heading(file: &Path, heading: &str) -> bool {
    let heading = heading.trim();
    if heading.is_empty() {
        return false;
    }
    if let Ok(agent_doc_crdt_relay_io::CurrentText::Current {
        text: content,
        live_editors,
        ..
    }) = agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file)
        && live_editors > 0
    {
        let content_norm =
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&content);
        if content_norm.lines().any(|line| line.trim() == heading) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "session_check_committed_response_visible_in_current_document file={} heading={:?} authority=lazily_crdt",
                    file.display(),
                    heading,
                ),
            );
            return true;
        }
    }
    false
}

#[cfg(test)]
mod current_document_pass_tests {
    use super::*;

    /// A session-check sweep must evaluate ONE document version
    /// (`#sccurrentpass`). Before the pass memo, each guard independently
    /// resolved the current document, so an operator typing mid-sweep made two
    /// guards disagree about the same document — and each resolve cost a full
    /// controller round trip.
    #[test]
    fn pass_serves_one_document_version_to_every_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = tmp.path().join("task.md");
        std::fs::write(&doc, "first\n").unwrap();

        let (first, second) = with_current_document_pass(|| {
            let first = resolve_current_document_content(&doc, "guard_a").unwrap();
            // The operator edits mid-sweep.
            std::fs::write(&doc, "second\n").unwrap();
            let second = resolve_current_document_content(&doc, "guard_b").unwrap();
            (first, second)
        });

        assert_eq!(
            first, second,
            "guards inside one pass must observe the same document version"
        );
    }

    /// The memo is scoped, never ambient: outside a pass every resolve reads
    /// through, and a mutation inside a pass re-reads once invalidated.
    #[test]
    fn resolution_reads_through_outside_a_pass_and_after_invalidation() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = tmp.path().join("task.md");
        std::fs::write(&doc, "first\n").unwrap();

        let before = resolve_current_document_content(&doc, "outside_pass").unwrap();
        std::fs::write(&doc, "second\n").unwrap();
        let after = resolve_current_document_content(&doc, "outside_pass").unwrap();
        assert_ne!(
            before, after,
            "without a pass, resolution must not be memoized"
        );

        let (stale, fresh) = with_current_document_pass(|| {
            let stale = resolve_current_document_content(&doc, "guard_a").unwrap();
            // A self-heal rewrites the document mid-pass.
            std::fs::write(&doc, "third\n").unwrap();
            invalidate_current_document_pass(&doc);
            let fresh = resolve_current_document_content(&doc, "guard_b").unwrap();
            (stale, fresh)
        });
        assert_ne!(
            stale, fresh,
            "invalidation must drop the memo so a repaired document is re-read"
        );
    }

    /// A nested entry point reuses the outer pass instead of installing a
    /// second memo that could observe a different version.
    #[test]
    fn nested_pass_reuses_the_outer_document_version() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = tmp.path().join("task.md");
        std::fs::write(&doc, "first\n").unwrap();

        let (outer, inner) = with_current_document_pass(|| {
            let outer = resolve_current_document_content(&doc, "outer").unwrap();
            std::fs::write(&doc, "second\n").unwrap();
            let inner =
                with_current_document_pass(|| resolve_current_document_content(&doc, "inner"))
                    .unwrap();
            (outer, inner)
        });

        assert_eq!(
            outer, inner,
            "a nested pass must not resolve a second document version"
        );
    }
}
