use agent_doc_document::transient_markers::strip_guard_markers;
use anyhow::Result;
use std::path::Path;

pub trait GuardMarkerCleanupEffects {
    fn load_snapshot(&self, file: &Path) -> Result<Option<String>>;
    fn save_snapshot(&self, file: &Path, content: &str) -> Result<()>;
    fn read_to_string(&self, file: &Path) -> Result<String>;
    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()>;
}

/// Strip ephemeral guard markers from the snapshot and working-tree file on disk.
/// Best-effort: logs warnings on failure but does not propagate errors.
pub fn strip_guard_markers_from_disk(effects: &impl GuardMarkerCleanupEffects, file: &Path) {
    if let Ok(Some(ref content)) = effects.load_snapshot(file) {
        let cleaned = strip_guard_markers(content);
        if cleaned != *content
            && let Err(e) = effects.save_snapshot(file, &cleaned)
        {
            eprintln!("[commit] warning: failed to strip guard markers from snapshot: {e}");
        }
    }
    if let Ok(content) = effects.read_to_string(file) {
        let cleaned = strip_guard_markers(&content);
        // #fccaudit: route the working-tree guard-marker strip through the
        // editor-IPC converge gate so it never writes behind a live JB editor
        // buffer (File Cache Conflict). The stripped markers
        // (`<!-- no-pending-capture -->`, `<!-- no-pending-done-guard -->`) are
        // ephemeral directives, not `(HEAD)` annotations, so the editor buffer
        // should drop them too. With no listener this falls back to the same
        // byte-for-byte disk write as before.
        if cleaned != content
            && let Err(e) =
                effects.converge_or_disk_write(file, &content, &cleaned, "strip_guard_markers")
        {
            eprintln!("[commit] warning: failed to strip guard markers from file: {e}");
        }
    }
}
