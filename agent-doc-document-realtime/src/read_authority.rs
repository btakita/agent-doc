//! Pure editor-buffer versus disk read-authority policy.
//!
//! Given on-disk document content and an optional live editor-buffer snapshot,
//! decide which source is authoritative for a cycle read. This module owns only
//! deterministic policy; callers own sidecar IO, logging, and filesystem paths.

/// Which document source the cycle should treat as authoritative this read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocAuthority {
    /// The live editor buffer holds unsaved edits newer than disk.
    EditorBuffer,
    /// The on-disk file wins because no editor is attached or the buffer is saved.
    Disk,
}

impl DocAuthority {
    /// Stable label for ops/log markers and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            DocAuthority::EditorBuffer => "editor_buffer",
            DocAuthority::Disk => "disk",
        }
    }
}

/// A snapshot of the live editor buffer.
///
/// `dirty` is the authority signal: `true` means the buffer holds edits not yet
/// flushed to disk. `generation` orders successive snapshots from the same
/// source; it is not compared against disk mtime.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciliation {
    pub authority: DocAuthority,
    /// The authoritative content the cycle should read.
    pub content: String,
    /// `true` when a clean editor buffer differs from disk. Disk wins, but this
    /// is a drift signal worth logging.
    pub diverged: bool,
    /// Stable reason code for ops/log markers and diagnostics.
    pub reason: &'static str,
}

impl Reconciliation {
    /// The authoritative content the cycle should read.
    pub fn authoritative_content(&self) -> &str {
        &self.content
    }
}

/// Decide which document source is authoritative for an agent-doc cycle read.
pub fn reconcile_current_doc(disk: &str, buffer: Option<&BufferState>) -> Reconciliation {
    match buffer {
        None => Reconciliation {
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: false,
            reason: "editor_absent",
        },
        Some(buf) if buf.content == disk => Reconciliation {
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: false,
            reason: "in_sync",
        },
        Some(buf) if buf.dirty => Reconciliation {
            authority: DocAuthority::EditorBuffer,
            content: buf.content.clone(),
            diverged: false,
            reason: "editor_unsaved_newer",
        },
        Some(_clean_but_differs) => Reconciliation {
            authority: DocAuthority::Disk,
            content: disk.to_string(),
            diverged: true,
            reason: "buffer_clean_diverged_disk_newer",
        },
    }
}

/// Convenience: the content the cycle should read as "current document".
pub fn current_doc(disk: &str, buffer: Option<&BufferState>) -> String {
    reconcile_current_doc(disk, buffer).content
}

/// Whether a buffer snapshot at `next` generation supersedes one at `prev`.
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
        let buf = BufferState::new("buffer has newer edits", true, 12);
        let r = reconcile_current_doc("stale disk", Some(&buf));
        assert_eq!(r.authority, DocAuthority::EditorBuffer);
        assert_eq!(r.content, "buffer has newer edits");
        assert!(!r.diverged);
        assert_eq!(r.reason, "editor_unsaved_newer");
    }

    #[test]
    fn clean_buffer_diverged_from_disk_uses_disk_and_flags() {
        let buf = BufferState::new("editor last-saved text", false, 3);
        let r = reconcile_current_doc("disk changed after save", Some(&buf));
        assert_eq!(r.authority, DocAuthority::Disk);
        assert_eq!(r.content, "disk changed after save");
        assert!(r.diverged);
        assert_eq!(r.reason, "buffer_clean_diverged_disk_newer");
    }

    #[test]
    fn current_doc_preserves_buffer_only_queue_item() {
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
