use anyhow::{Context, Result};
use std::cell::Cell;
use std::path::Path;

pub mod backlog_guards;
pub mod closeout_guards;
pub mod command;
pub mod detect;
pub mod guard_modes;
pub mod current_document;
pub mod partial_staging;
pub mod profile;
pub(crate) use current_document::{invalidate_current_document_pass, with_current_document_pass};
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
    // `#sccurrentpass`: one derived resolution per sweep, re-taken only when a
    // step actually rewrote the document. See `current_document`.
    let owned_file = file.to_path_buf();
    let label = format!("session-check {source}");
    if let Some(hit) = crate::current_document::pass_resolved(file, move || {
        agent_doc_document_realtime_io::try_resolve_current_document_with_source(
            &owned_file,
            &label,
        )
        .ok()
    }) {
        return Ok(hit);
    }
    // No pass open, or the memoized resolve failed. Re-run uncached so the real
    // error surfaces instead of a cached absence.
    agent_doc_document_realtime_io::try_resolve_current_document_with_source(
        file,
        &format!("session-check {source}"),
    )
    .with_context(|| {
        format!(
            "session-check {source}: resolve current document {}",
            file.display()
        )
    })
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
    // The detectors above all funnel into this, so it is the one place where
    // authority resolution can be attributed once instead of per branch
    // (`#sessioncheckprofile`).
    profile::timed("resolve_current_document_content", || {
        Ok(resolve_current_document(file, source)?.into_content())
    })
}

pub(crate) fn resolve_disk_document_content(file: &Path, source: &str) -> Result<String> {
    profile::timed("resolve_disk_document_content", || {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(
            file,
            &format!("session-check {source}"),
        )
    })
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
