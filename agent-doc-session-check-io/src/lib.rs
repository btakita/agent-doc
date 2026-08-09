use anyhow::{Context, Result};
use std::cell::Cell;
use std::path::Path;

pub mod backlog_guards;
pub mod closeout_guards;
pub mod command;
pub mod detect;
pub mod guard_modes;
pub mod partial_staging;
pub mod profile;
pub(crate) use agent_doc_document_realtime_io::{
    invalidate_current_document_projection as invalidate_current_document_pass,
    with_current_document_projection_pass as with_current_document_pass,
};
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
            &format!("session-check:{source}"),
        );
    }
    // The shared pass projection is revision-aware: repeated guards reuse one
    // authority result, while a concurrent operator edit advances the Yrs
    // state vector (or detached disk hash) and invalidates before this read.
    agent_doc_document_realtime_io::try_resolve_current_document_with_source(
        file,
        &format!("session-check:{source}"),
    )
    .with_context(|| {
        format!(
            "session-check:{source}: resolve current document {}",
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
        &format!("session-check:{source}"),
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
            &format!("session-check:{source}"),
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
mod closeout_pass_guard {
    /// `#preflightprojpass`: BOTH session-check entry points must open the
    /// projection pass.
    ///
    /// `run_with_options` (the CLI) opened one; `enforce_clean_closeout` (what
    /// `respond` and `write --commit` run) did not, so the identical guard
    /// suite paid a full document resolve per guard. Measured 2026-08-09 on
    /// 0.35.204, once `pass=` made the outcome readable: 190 of 226 resolves
    /// were `uninstalled` against 12 genuine `miss`.
    ///
    /// A behavioural test cannot see this — an unwrapped path is merely SLOWER,
    /// never wrong — which is exactly why it survived. The entry points are
    /// few and named, so guard them structurally.
    #[test]
    fn every_session_check_entry_point_opens_the_projection_pass() {
        let command = include_str!("command.rs");
        let body = command
            .split_once("\n#[cfg(test)]")
            .map(|(before, _)| before)
            .unwrap_or(command);
        for entry in [
            "pub fn run_with_options(",
            "pub fn enforce_clean_closeout_with_force_disk(",
        ] {
            let start = body
                .find(entry)
                .unwrap_or_else(|| panic!("entry point `{entry}` moved or was renamed"));
            // The pass must be opened within the entry point itself, before it
            // delegates into the guard suite.
            let window = &body[start..(start + 1400).min(body.len())];
            assert!(
                window.contains("with_current_document_pass("),
                "`{entry}` must open the projection pass; without it every guard \
                 re-resolves the whole document"
            );
        }
    }
}

#[cfg(test)]
mod source_attribution_guard {
    /// `#passattrib`: `session-check` composed its resolve source as
    /// `format!("session-check {source}")` — WITH A SPACE — so every one of its
    /// resolutions parsed as the bare token `session-check` from a
    /// space-delimited ops line. Measured 2026-08-09: 62 resolutions that
    /// looked like one caller were really ten distinct guards, and the
    /// per-guard breakdown only appeared after re-parsing the field by hand.
    ///
    /// This is the same defect 0.35.199 fixed one level up, where a hardcoded
    /// `source=crdt_relay` hid every caller. An unparseable field is as good as
    /// a missing one.
    #[test]
    fn composed_resolve_sources_stay_single_token() {
        let source = include_str!("lib.rs");
        let body = source
            .split_once("#[cfg(test)]")
            .map(|(before, _)| before)
            .unwrap_or(source);
        assert!(
            !body.contains("\"session-check {source}\""),
            "a space in `source=` makes every session-check resolve unattributable"
        );
        assert!(
            body.contains("\"session-check:{source}\""),
            "compose the guard name onto the caller with a non-space separator"
        );
    }
}

#[cfg(test)]
mod current_document_pass_tests {
    use super::*;

    /// The pass shares unchanged reads but must not hide an operator edit.
    #[test]
    fn pass_reacts_to_an_operator_edit_between_guards() {
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

        assert_ne!(
            first, second,
            "a new disk revision must invalidate the pass"
        );
        assert_eq!(second, "second\n");
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

    /// A nested entry point reuses the outer graph, including its revision
    /// invalidation; nesting must not freeze the first document version.
    #[test]
    fn nested_pass_reuses_the_outer_reactive_graph() {
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

        assert_ne!(
            outer, inner,
            "the shared graph must observe the new revision"
        );
        assert_eq!(inner, "second\n");
    }
}
