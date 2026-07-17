//! Pure editor-buffer versus disk read-authority policy.
//!
//! Given on-disk document content and an optional live editor-buffer snapshot,
//! decide which source is authoritative for a cycle read. This module owns only
//! deterministic policy; callers own Lazily/controller IO, logging, and cold
//! recovery projection paths.

/// Which document source the cycle should treat as authoritative this read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocAuthority {
    /// The live editor buffer holds unsaved edits newer than disk.
    EditorBuffer,
    /// The on-disk file wins because no editor is attached or the buffer is saved.
    Disk,
}

/// Source selected while rebuilding document authority after reconnect.
///
/// Live editor buffers are always ahead of filesystem replicas in the
/// authority order. Multiple live editors converge through their shared CRDT;
/// disk and git are recovery sources only when no editor buffer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectAuthority {
    EditorCrdt,
    EditorBuffer,
    Disk,
    Git,
    Unavailable,
}

/// Select the source from which a reconnecting replica must be reset.
pub fn reconnect_authority(
    live_editor_buffers: usize,
    disk_available: bool,
    git_available: bool,
) -> ReconnectAuthority {
    match live_editor_buffers {
        2.. => ReconnectAuthority::EditorCrdt,
        1 => ReconnectAuthority::EditorBuffer,
        0 if disk_available => ReconnectAuthority::Disk,
        0 if git_available => ReconnectAuthority::Git,
        0 => ReconnectAuthority::Unavailable,
    }
}

/// Resolution of an explicit recovery/force-disk write observed by a live
/// editor. The binary retains the external target while the editor shows the
/// pre-write cut; it never silently merges that target into a divergent buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalDiskDecision {
    /// The editor loaded/accepted the exact external target.
    AcceptedInEditor,
    /// The editor still shows the cut from before the external write. Keep the
    /// target pending while the IDE asks the operator which version to keep.
    PendingUserDecision,
    /// The editor contains another cut. It is authoritative and the retained
    /// external target must be cleared.
    EditorSupersedes,
}

pub fn external_disk_decision(
    expected_editor_hash: &str,
    external_target_hash: &str,
    current_editor_hash: &str,
) -> ExternalDiskDecision {
    if current_editor_hash.eq_ignore_ascii_case(external_target_hash) {
        ExternalDiskDecision::AcceptedInEditor
    } else if expected_editor_hash.is_empty()
        || current_editor_hash.eq_ignore_ascii_case(expected_editor_hash)
    {
        ExternalDiskDecision::PendingUserDecision
    } else {
        ExternalDiskDecision::EditorSupersedes
    }
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
        Some(clean_but_differs) => Reconciliation {
            authority: DocAuthority::EditorBuffer,
            content: clean_but_differs.content.clone(),
            diverged: true,
            reason: "live_editor_diverged_disk_pending_reconciliation",
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
    fn clean_buffer_diverged_from_disk_keeps_live_editor_authority() {
        let buf = BufferState::new("editor last-saved text", false, 3);
        let r = reconcile_current_doc("disk changed after save", Some(&buf));
        assert_eq!(r.authority, DocAuthority::EditorBuffer);
        assert_eq!(r.content, "editor last-saved text");
        assert!(r.diverged);
        assert_eq!(r.reason, "live_editor_diverged_disk_pending_reconciliation");
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

    #[test]
    fn reconnect_authority_orders_editor_then_disk_then_git() {
        assert_eq!(
            reconnect_authority(2, true, true),
            ReconnectAuthority::EditorCrdt
        );
        assert_eq!(
            reconnect_authority(1, true, true),
            ReconnectAuthority::EditorBuffer
        );
        assert_eq!(reconnect_authority(0, true, true), ReconnectAuthority::Disk);
        assert_eq!(reconnect_authority(0, false, true), ReconnectAuthority::Git);
        assert_eq!(
            reconnect_authority(0, false, false),
            ReconnectAuthority::Unavailable
        );
    }

    #[test]
    fn external_disk_write_waits_for_user_or_yields_to_editor() {
        assert_eq!(
            external_disk_decision("base", "target", "base"),
            ExternalDiskDecision::PendingUserDecision
        );
        assert_eq!(
            external_disk_decision("base", "target", "target"),
            ExternalDiskDecision::AcceptedInEditor
        );
        assert_eq!(
            external_disk_decision("base", "target", "new-editor-cut"),
            ExternalDiskDecision::EditorSupersedes
        );
        assert_eq!(
            external_disk_decision("", "target", "unproven-editor-cut"),
            ExternalDiskDecision::PendingUserDecision
        );
    }
}
