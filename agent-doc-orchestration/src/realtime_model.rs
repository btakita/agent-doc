//! # Module: realtime_model
//!
//! ## Spec (`#rtwatch` — realtime editor-buffer ↔ disk read authority)
//! The agent-doc cycle (`preflight` / `write` / `finalize` / `session-check`)
//! currently sources "current document" by reading the **disk file** (and a
//! preflight snapshot/baseline). When an editor (IDEA / VS Code) holds **unsaved
//! edits**, its live buffer is *newer than disk* and is already treated as
//! `content_ours`-authoritative over the socket IPC apply path — so the cycle
//! reasons about a **staler** document than the one the user is editing, and an
//! agent write can clobber legitimate user queue/exchange content that only
//! exists in the buffer (the `#queue-user-edit-overwrite` / `test test` clobber
//! / `#ipcdrift` family).
//!
//! This module owns the **deterministic read-authority decision**: given the
//! on-disk content and an optional live editor-buffer snapshot, decide which is
//! authoritative for the agent to read, following the operator's stated model —
//! *"the editor buffer is the source of truth for the document state when the
//! editor is running... falling back to the file on disk."* The authority rule
//! keys off the buffer's **dirty** flag (unsaved edits not yet flushed to disk)
//! rather than comparing cross-source timestamps, so it is unambiguous and
//! deterministically testable without a live editor:
//!
//! - editor buffer absent (no editor / closed) → **disk** wins;
//! - buffer content equals disk (saved, in sync) → **disk** wins (canonical);
//! - buffer is dirty / unsaved (content differs from disk) → **editor buffer**
//!   wins (it holds edits newer than disk);
//! - buffer is clean (matches its last save) but disk content differs → **disk**
//!   wins and the result is flagged `diverged` (disk was changed after the
//!   editor's last save — a drift signal the caller logs).
//!
//! Per the Shared Foundation pattern (`CLAUDE.md` — FFI-first for editor
//! integration; all deterministic behavior in the binary), this lands the read
//! authority as a pure, seam-isolated primitive with deterministic evals,
//! mirroring how [`crate::document_watcher`] (`#pcpc4`) shipped its controller
//! gate independently of the live `notify` feed. Wiring the cycle read sites
//! (`preflight.rs` / `write.rs` / `session_check.rs`) to source current-doc
//! through [`reconcile_current_doc`], and feeding the durable editor-buffer
//! snapshot from the socket IPC layer, is the separate live-verify cutover rung.
//!
//! ## Evals
//! - `editor_absent_uses_disk`
//! - `in_sync_buffer_prefers_disk_canonical`
//! - `dirty_buffer_wins_over_disk`
//! - `clean_buffer_diverged_from_disk_uses_disk_and_flags`
//! - `current_doc_preserves_buffer_only_queue_item`
//! - `buffer_supersedes_is_monotonic`

/// Which document source the cycle should treat as authoritative this read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocAuthority {
    /// The live editor buffer (holds unsaved edits newer than disk).
    EditorBuffer,
    /// The on-disk file (no editor, or the buffer is saved/in sync).
    Disk,
}

impl DocAuthority {
    /// Stable label for `ops.log` markers.
    pub fn as_str(self) -> &'static str {
        match self {
            DocAuthority::EditorBuffer => "editor_buffer",
            DocAuthority::Disk => "disk",
        }
    }
}

/// A snapshot of the live editor buffer, reported by the editor plugin over the
/// socket IPC channel. `dirty` is the authority signal: `true` means the buffer
/// holds edits not yet flushed to disk (so it is newer than disk). `generation`
/// is a monotonic per-document editor-change counter used to order successive
/// buffer snapshots from the *same* source (staleness / durable delta ingest);
/// it is **not** compared against disk mtime.
#[derive(Debug, Clone)]
pub struct BufferState {
    pub content: String,
    pub dirty: bool,
    pub generation: u64,
}

impl BufferState {
    pub fn new(content: impl Into<String>, dirty: bool, generation: u64) -> Self {
        Self {
            content: content.into(),
            dirty,
            generation,
        }
    }
}

/// The resolved read authority for one cycle read.
#[derive(Debug, Clone)]
pub struct Reconciliation {
    pub authority: DocAuthority,
    /// The authoritative content the cycle should read.
    pub content: String,
    /// `true` when the editor buffer is clean (saved) yet disk content differs —
    /// disk wins, but this is a drift signal worth logging.
    pub diverged: bool,
    /// Stable reason code for `ops.log` markers.
    pub reason: &'static str,
}

impl Reconciliation {
    /// The authoritative content the cycle should read.
    pub fn authoritative_content(&self) -> &str {
        &self.content
    }
}

/// Decide which document source is authoritative for an agent-doc cycle read,
/// given the on-disk `disk` content and an optional live editor-buffer snapshot.
///
/// See the module spec for the authority rule. This is a pure function: same
/// inputs always yield the same decision, with no I/O or clock reads.
pub fn reconcile_current_doc(disk: &str, buffer: Option<&BufferState>) -> Reconciliation {
    match buffer {
        // No editor (or closed) — disk is the only source.
        None => Reconciliation {
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: false,
            reason: "editor_absent",
        },
        Some(buf) if buf.content == disk => Reconciliation {
            // Saved / in sync: disk is canonical.
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: false,
            reason: "in_sync",
        },
        Some(buf) if buf.dirty => Reconciliation {
            // Unsaved edits live only in the buffer — it is newer than disk.
            authority: DocAuthority::EditorBuffer,
            content: buf.content.clone(),
            diverged: false,
            reason: "editor_unsaved_newer",
        },
        Some(_clean_but_differs) => Reconciliation {
            // Buffer is clean (matches its last save) but disk differs — disk was
            // changed after the editor's last save, so disk is newer. Flag drift.
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: true,
            reason: "buffer_clean_diverged_disk_newer",
        },
    }
}

/// Convenience: the content the cycle should read as "current document",
/// reconciling disk against the optional live editor buffer. Equivalent to
/// `reconcile_current_doc(disk, buffer).content`.
pub fn current_doc(disk: &str, buffer: Option<&BufferState>) -> String {
    reconcile_current_doc(disk, buffer).content
}

/// Whether a buffer snapshot at `next` generation supersedes one at `prev`.
/// Used to order successive editor-buffer deltas from the same source so a
/// late-arriving stale snapshot cannot overwrite a newer one (durable delta
/// ingestion). Strictly monotonic: equal generations do not supersede.
pub fn buffer_supersedes(prev: u64, next: u64) -> bool {
    next > prev
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_absent_uses_disk() {
        let r = reconcile_current_doc("disk body", None);
        assert_eq!(r.authority, DocAuthority::Disk);
        assert_eq!(r.content, "disk body");
        assert!(!r.diverged);
        assert_eq!(r.reason, "editor_absent");
    }

    #[test]
    fn in_sync_buffer_prefers_disk_canonical() {
        let buf = BufferState::new("same", false, 7);
        let r = reconcile_current_doc("same", Some(&buf));
        assert_eq!(r.authority, DocAuthority::Disk);
        assert_eq!(r.content, "same");
        assert!(!r.diverged);
        assert_eq!(r.reason, "in_sync");
    }

    #[test]
    fn dirty_buffer_wins_over_disk() {
        // The core no-clobber fix: unsaved buffer edits are authoritative.
        let buf = BufferState::new("buffer has newer edits", true, 12);
        let r = reconcile_current_doc("stale disk", Some(&buf));
        assert_eq!(r.authority, DocAuthority::EditorBuffer);
        assert_eq!(r.content, "buffer has newer edits");
        assert!(!r.diverged);
        assert_eq!(r.reason, "editor_unsaved_newer");
    }

    #[test]
    fn clean_buffer_diverged_from_disk_uses_disk_and_flags() {
        // Editor saved earlier; another writer changed disk afterward → disk newer.
        let buf = BufferState::new("editor last-saved text", false, 3);
        let r = reconcile_current_doc("disk changed after save", Some(&buf));
        assert_eq!(r.authority, DocAuthority::Disk);
        assert_eq!(r.content, "disk changed after save");
        assert!(r.diverged);
        assert_eq!(r.reason, "buffer_clean_diverged_disk_newer");
    }

    #[test]
    fn current_doc_preserves_buffer_only_queue_item() {
        // Realistic #queue-user-edit-overwrite scenario: the user just typed a
        // queue item in IDEA without saving. Disk lacks it; the dirty buffer has
        // it. The cycle must read the buffer so the agent does not clobber it.
        let disk = "## Queue\n- do [#a]\n";
        let buffer_content = "## Queue\n- do [#a]\n- do [#rtwatch]\n";
        let buf = BufferState::new(buffer_content, true, 99);
        let current = current_doc(disk, Some(&buf));
        assert!(current.contains("#rtwatch"));
        assert_eq!(current, buffer_content);
    }

    #[test]
    fn buffer_supersedes_is_monotonic() {
        assert!(buffer_supersedes(1, 2));
        assert!(!buffer_supersedes(2, 2));
        assert!(!buffer_supersedes(3, 2));
    }
}
