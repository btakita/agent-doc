//! # Module: watch_authority
//!
//! ## Spec (`#dsqa` / `#pcp7` — 08b filesystem-watch authority, post-cutover end state)
//! Realizes the `specs/08b-single-process-control-plane.md` filesystem-watch
//! authority: the editor plugin's own NIO `WatchService` is **unconditionally**
//! demoted to read-only buffer reporting. The single controller-owned watcher
//! ([`crate::document_watcher`], `#pcpc4`/`#pcp4`) plus the socket IPC command
//! channel are the sole writer to the live editor buffer. This removes the
//! second-watcher race where the plugin mutated the live buffer between an agent
//! finalize's preflight and commit — the `live_prompt_drift_after_preflight` /
//! `ipc_socket_already_applied_live_buffer_diverged` drift family that the
//! `#dav9` in-process hosting swap alone did not fix (the host moved, the second
//! writer did not).
//!
//! ## History
//! This shipped through the 08b migration gate ladder behind the
//! `AGENT_DOC_PLUGIN_WATCH` rollback flag (`active → read-only`). The cutover is
//! now **complete**: the flag and the `active` (plugin-applies) path were removed
//! at the removal rung, so the plugin's WatchService file-apply path is always
//! read-only. The plugin queries this end state through the
//! `agent_doc_plugin_watch_readonly` FFI export (see `crate::ffi` in the binary
//! crate), which now always reports read-only and emits a structured
//! `plugin_watch_readonly` `ops.log` marker. The plugin's socket IPC apply path
//! (the controller's writer arm into the editor) stays active.

/// Whether the editor plugin's own `WatchService` file-apply path is demoted to
/// read-only buffer reporting. Always `true` post-cutover — the plugin never
/// autonomously applies file-IPC patches it observes on disk; the
/// controller-owned watcher + socket IPC are the sole writer.
pub fn plugin_watch_is_readonly() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_watch_is_always_readonly_post_cutover() {
        assert!(plugin_watch_is_readonly());
    }
}
