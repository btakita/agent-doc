//! # Module: watch_authority
//!
//! ## Spec (`#pcpc5e2` / `#dsqa` / `#pcp7` — 08b cut-over residual phase 2)
//! Drives the `specs/08b-single-process-control-plane.md` migration for the
//! **filesystem-watch authority**: moving patch application off the editor
//! plugin's own `WatchService` (a second, autonomous filesystem watcher over
//! `.agent-doc/patches/`) so the single controller-owned watcher
//! ([`crate::document_watcher`], `#pcpc4`/`#pcp4`) is the *only* event source.
//!
//! This is the editor-side companion to [`crate::supervisor_authority`] and
//! [`crate::write_authority`]: same rollback-flag ladder discipline, applied to
//! *who watches the filesystem and applies patches*. The JetBrains/VS Code
//! plugin `PatchWatcher` runs its own NIO `WatchService` thread that detects
//! `<hash>.json` patch files and applies them via the Document API. That second
//! watcher mutates the live editor buffer between an agent finalize's preflight
//! and its commit, which is the root of the `live_prompt_drift_after_preflight`
//! / `ipc_socket_already_applied_live_buffer_diverged` drift family the
//! `#dav9` in-process hosting swap alone did **not** eliminate (the host moved,
//! but the *second writer* did not). Demoting that watcher to read-only buffer
//! reporting leaves the controller-owned watcher + socket IPC as the sole
//! writer, so finalize's candidate and the live buffer stop racing.
//!
//! Per 08b §"Migration gates", the cut-over follows a gated ladder, each rung
//! with a rollback flag that returns the previous authority:
//!
//! - **`active`** (default) — exactly today's behavior: the editor plugin's
//!   `WatchService` thread independently applies file-IPC patches it observes
//!   under `.agent-doc/patches/`. Shipped users are unchanged until an operator
//!   opts in. Rollback target for the read-only rung.
//! - **`read-only`** (`#dsqa` authority rung) — the plugin's `WatchService`
//!   file-apply path is demoted to read-only buffer reporting: it no longer
//!   applies patches it observes on disk. The controller-owned watcher
//!   (`#pcpc4`) plus the socket IPC command channel become the sole writer to
//!   the live editor buffer. This realizes `#pcp7`.
//!
//! ## Agentic Contracts
//! - [`current_mode`] reads `AGENT_DOC_PLUGIN_WATCH` on every call (cheap, no
//!   caching) so an operator can advance or roll back the gate without
//!   restarting a long-lived process. An unrecognized value logs one warning and
//!   falls back to `Active` (fail-safe to current behavior — a watch-authority
//!   decision must never strand a live editor session without an applier).
//! - The plugin queries this gate through the
//!   `agent_doc_plugin_watch_readonly` FFI export (see `crate::ffi` in the
//!   binary crate). The flag lives in the binary, not the plugin process, so the
//!   operator sets it once on the `agent-doc` session and every plugin instance
//!   honors it via FFI rather than depending on the IDE's inherited environment.
//!
//! ## Evals
//! - `mode_parses_known_values_and_defaults_active`
//! - `mode_unknown_value_falls_back_active`
//! - `mode_is_readonly_only_at_demotion_rung`

/// Environment variable that selects the active plugin-watch migration gate.
pub const ENV_VAR: &str = "AGENT_DOC_PLUGIN_WATCH";

/// The 08b plugin filesystem-watch migration gate (see module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginWatchMode {
    /// The editor plugin `WatchService` independently applies file-IPC patches
    /// (today's behavior). Default + rollback target.
    Active,
    /// The plugin `WatchService` file-apply path is demoted to read-only buffer
    /// reporting; the controller-owned watcher + socket IPC are the sole writer.
    ReadOnly,
}

impl PluginWatchMode {
    /// Parse a mode token. Accepts the canonical spellings plus a few
    /// punctuation/case variants. Returns `None` for unknown values so the
    /// caller can warn and fail safe.
    pub fn parse(s: &str) -> Option<PluginWatchMode> {
        match s.trim().to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "" | "active" | "on" | "apply" => Some(PluginWatchMode::Active),
            "read-only" | "readonly" | "ro" | "report" => Some(PluginWatchMode::ReadOnly),
            _ => None,
        }
    }

    /// Stable lowercase token for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginWatchMode::Active => "active",
            PluginWatchMode::ReadOnly => "read-only",
        }
    }

    /// Whether this rung demotes the plugin `WatchService` file-apply path to
    /// read-only (only the `read-only` authority rung; `active` keeps the plugin
    /// applying file-IPC patches itself).
    pub fn is_readonly(self) -> bool {
        matches!(self, PluginWatchMode::ReadOnly)
    }
}

/// Resolve the active mode from the environment on each call. An unrecognized
/// value warns once to stderr and falls back to `Active` (never demote a live
/// editor's only applier on a typo).
pub fn current_mode() -> PluginWatchMode {
    match std::env::var(ENV_VAR) {
        Ok(raw) => PluginWatchMode::parse(&raw).unwrap_or_else(|| {
            eprintln!("[watch-authority] WARNING: unrecognized {ENV_VAR}={raw:?}; falling back to active");
            PluginWatchMode::Active
        }),
        Err(_) => PluginWatchMode::Active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_parses_known_values_and_defaults_active() {
        assert_eq!(PluginWatchMode::parse(""), Some(PluginWatchMode::Active));
        assert_eq!(PluginWatchMode::parse("active"), Some(PluginWatchMode::Active));
        assert_eq!(PluginWatchMode::parse("ON"), Some(PluginWatchMode::Active));
        assert_eq!(
            PluginWatchMode::parse("read_only"),
            Some(PluginWatchMode::ReadOnly)
        );
        assert_eq!(
            PluginWatchMode::parse(" Read-Only "),
            Some(PluginWatchMode::ReadOnly)
        );
        assert_eq!(
            PluginWatchMode::parse("readonly"),
            Some(PluginWatchMode::ReadOnly)
        );
        assert_eq!(PluginWatchMode::Active.as_str(), "active");
        assert_eq!(PluginWatchMode::ReadOnly.as_str(), "read-only");
    }

    #[test]
    fn mode_unknown_value_falls_back_active() {
        assert_eq!(PluginWatchMode::parse("banana"), None);
    }

    #[test]
    fn mode_is_readonly_only_at_demotion_rung() {
        assert!(!PluginWatchMode::Active.is_readonly());
        assert!(PluginWatchMode::ReadOnly.is_readonly());
    }

    #[test]
    fn current_mode_reads_env_and_defaults_active() {
        let _lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        // Default (unset) is the rollback-safe active applier.
        unsafe { std::env::remove_var(ENV_VAR) };
        assert_eq!(current_mode(), PluginWatchMode::Active);
        // Operator opt-in demotes to read-only.
        unsafe { std::env::set_var(ENV_VAR, "read-only") };
        assert_eq!(current_mode(), PluginWatchMode::ReadOnly);
        // Unknown value fails safe back to active (never strand a live editor).
        unsafe { std::env::set_var(ENV_VAR, "banana") };
        assert_eq!(current_mode(), PluginWatchMode::Active);
        unsafe { std::env::remove_var(ENV_VAR) };
    }
}
