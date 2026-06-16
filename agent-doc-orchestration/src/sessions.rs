//! # Module: sessions
//!
//! Session registry — maps canonical absolute document paths to tmux pane IDs
//! and supervisor metadata.
//!
//! Registry lives at `.agent-doc/sessions.json` relative to the project root.
//! Re-exports `Tmux`, `IsolatedTmux`, `RegistryEntry` (as `SessionEntry`),
//! `RegistryLock`, and `Registry` (as `SessionRegistry`) from `tmux-router`.
//! Agent-doc-specific operations (registry load/save with the hardcoded
//! `.agent-doc/sessions.json` path, pane/window queries, positional pane
//! resolution) live here. Thin tmux interaction shims delegate to tmux-router.
//!
//! ## Spec
//! - `registry_path()` returns the canonical `.agent-doc/sessions.json` path.
//! - `load()` deserialises the registry from disk; returns an empty map when the file
//!   does not exist. Not locked internally — read-only callers may call directly.
//! - `save(registry)` serialises the registry to disk with pretty-printing. Callers
//!   MUST hold a `RegistryLock` before calling.
//! - `register(session_id, pane_id, file)` acquires the lock, calls
//!   `register_with_pid` using the current process ID.
//! - `register_with_pid` queries the pane's window and delegates to `register_full`.
//! - `register_supervisor` records the authoritative supervisor PID +
//!   supervisor instance identity for a live pane owner.
//! - `register_full` enforces single-session-per-pane by evicting stale entries that
//!   share the same pane before inserting the new `SessionEntry`.
//! - When the same session UUID is rebound to a different pane, `register_full`
//!   best-effort appends `session_superseded ...` plus
//!   `session_end origin=registry_rebind ...` to the prior session log before the
//!   new pane registration lands.
//! - `lookup(session_id)` returns the pane ID for a session UUID, or `None` if not found.
//! - `lookup_entry(session_id)` returns the full `SessionEntry`, or `None`.
//! - `current_pane()` reads `TMUX_PANE` env var; falls back to querying tmux for the
//!   active pane (works from IDE processes outside a tmux shell).
//! - `pane_pid(pane_id)` queries the foreground process PID of a pane via
//!   `tmux display-message`.
//! - `pane_window(pane_id)` returns the tmux window ID (`@N`) for a pane.
//! - `capture_pane(tmux, pane_id)` returns visible pane content via
//!   `tmux capture-pane -p`; `capture_pane_with_ansi` preserves attributes
//!   for TUI prompt selection parsing.
//! - `Multiplexer` is the backend boundary for pane/window/session queries,
//!   capture, and submitted input. `Tmux` is the production implementation;
//!   unit tests can inject a mock backend without a live tmux server.
//! - `send_key(tmux, pane_id, key)` sends a single named key (e.g. `"Enter"`,
//!   `"Up"`) — does NOT append a newline, unlike `Tmux::send_keys`.
//! - `pane_by_position(position)` resolves a pane in the current window by
//!   positional hint: `left`, `right`, `top`, `bottom`.
//! - `pane_by_position_in_window(position, window)` is the same but scoped to a
//!   specific window ID.
//! - `in_tmux()` returns `true` when the `TMUX` env var is set.
//!
//! ## Agentic Contracts
//! - The registry file path is always `.agent-doc/sessions.json` relative to CWD at
//!   call time; callers must ensure CWD is the project root.
//! - `load` and `save` are NOT self-locking. Any read-modify-write cycle must acquire
//!   `RegistryLock` first; prefer `tmux_router::with_registry` for safe cycles.
//! - `register_full` guarantees at most one registry entry per pane ID; pre-existing
//!   entries pointing to the same pane are removed before the new entry is inserted.
//! - `capture_pane` and `send_key` propagate tmux errors as `anyhow::Error` — callers
//!   should treat failures as non-fatal warnings when used for display or navigation.
//! - `pane_by_position` errors on unrecognised position strings with an explicit
//!   message; valid values are exactly `left`, `right`, `top`, `bottom`.
//! - `current_pane` never returns an empty string; an empty tmux response is an error.
//!
//! ## Evals
//! - registry_roundtrip: save a `SessionEntry` → load from same dir → entry is
//!   preserved with identical fields.
//! - load_empty_returns_empty_map: load from a dir with no sessions.json → empty map.
//! - registry_multiple_sessions_isolated: two entries with distinct pane IDs round-trip
//!   independently without cross-contamination.
//! - registry_overwrite_existing_session: inserting the same session_id twice replaces
//!   the entry; registry length stays at 1.
//! - multiplexer_queries_support_mock_backend: active pane, pane PID, and window ID
//!   queries work through the trait without tmux.
//! - multiplexer_position_selection_is_backend_independent: position selection
//!   parses multiplexer pane geometry independent of the concrete backend.
//! - prune_removes_dead_panes: retain entries whose pane is alive removes fabricated
//!   dead pane IDs, leaving an empty registry.
//! - register_full_deduplicates_pane: seeding two sessions with the same pane then
//!   calling `register_full` with a third session leaves only the third in the registry.
//! - pane_alive_returns_false_for_nonexistent: `pane_alive("%99999")` returns `false`.
//! - tmux_create_session_and_verify: isolated server → `new_session` → pane ID starts
//!   with `%` and `pane_alive` returns `true`.
//! - tmux_auto_start_cascade: auto_start on no-server, no-session, and existing-session
//!   each produce a live pane in the correct session.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

// Re-export Tmux types from tmux-router.
#[cfg(test)]
pub use tmux_router::IsolatedTmux;
pub use tmux_router::PaneMoveOp;
pub use tmux_router::Tmux;
pub use tmux_router::{Registry as SessionRegistry, RegistryEntry as SessionEntry, RegistryLock};

const SESSIONS_FILE: &str = ".agent-doc/sessions.json";

/// Multiplexer operations agent-doc needs at the session/pane boundary.
///
/// The implementation is tmux-backed today, but callers that only need pane
/// state or geometry should depend on this trait instead of shelling out to
/// `tmux` directly. That keeps future backends such as Zellij from inheriting
/// tmux-specific command formatting throughout the codebase.
pub trait Multiplexer {
    fn display_message(&self, target: Option<&str>, format: &str) -> Result<String>;
    fn list_panes(&self, target: Option<&str>, format: &str) -> Result<String>;
    fn pane_alive(&self, pane_id: &str) -> bool;
    fn session_alive(&self, name: &str) -> bool;
    fn capture_pane_plain(&self, pane_id: &str) -> Result<String>;
    fn capture_pane_with_ansi(&self, pane_id: &str) -> Result<String>;
    fn send_key(&self, pane_id: &str, key: &str) -> Result<()>;
    fn send_submitted_text(&self, pane_id: &str, text: &str) -> Result<()>;
    fn send_submitted_text_for_harness(
        &self,
        pane_id: &str,
        text: &str,
        harness: &str,
    ) -> Result<()>;
}

impl Multiplexer for Tmux {
    fn display_message(&self, target: Option<&str>, format: &str) -> Result<String> {
        let mut cmd = self.cmd();
        cmd.arg("display-message");
        if let Some(target) = target {
            cmd.args(["-t", target]);
        }
        let output = cmd
            .args(["-p", format])
            .output()
            .context("failed to run multiplexer display-message")?;
        if !output.status.success() {
            anyhow::bail!(
                "multiplexer display-message failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn list_panes(&self, target: Option<&str>, format: &str) -> Result<String> {
        let mut cmd = self.cmd();
        cmd.arg("list-panes");
        if let Some(target) = target {
            cmd.args(["-t", target]);
        }
        let output = cmd
            .args(["-F", format])
            .output()
            .context("failed to run multiplexer list-panes")?;
        if !output.status.success() {
            anyhow::bail!(
                "multiplexer list-panes failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn pane_alive(&self, pane_id: &str) -> bool {
        Tmux::pane_alive(self, pane_id)
    }

    fn session_alive(&self, name: &str) -> bool {
        Tmux::session_alive(self, name)
    }

    fn capture_pane_plain(&self, pane_id: &str) -> Result<String> {
        Tmux::capture_pane(self, pane_id, None)
    }

    fn capture_pane_with_ansi(&self, pane_id: &str) -> Result<String> {
        let output = self
            .cmd()
            .args(["capture-pane", "-t", pane_id, "-p", "-e"])
            .output()
            .context("failed to run multiplexer capture-pane with ANSI")?;
        if !output.status.success() {
            anyhow::bail!(
                "multiplexer capture-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn send_key(&self, pane_id: &str, key: &str) -> Result<()> {
        crate::input_diag::log_key_event(
            None,
            "sessions.send_key",
            &format!("pane:{pane_id}"),
            "tmux_send_key",
            key,
            key.len(),
            crate::input_diag::KeyEventMeta::default(),
        );
        Tmux::send_key(self, pane_id, key)
    }

    fn send_submitted_text(&self, pane_id: &str, text: &str) -> Result<()> {
        let profile = tmux_submit_profile_for_harness("");
        crate::input_diag::log_text_submit(
            None,
            "sessions.send_submitted_text",
            &format!("pane:{pane_id}"),
            text,
            None,
            profile.transform(),
            profile.submit_key(),
        );
        send_submitted_text_with_profile(self, pane_id, text, profile)
            .with_context(|| format!("failed to submit input to pane {}", pane_id))
    }

    fn send_submitted_text_for_harness(
        &self,
        pane_id: &str,
        text: &str,
        harness: &str,
    ) -> Result<()> {
        let profile = tmux_submit_profile_for_harness(harness);
        crate::input_diag::log_text_submit(
            None,
            "sessions.send_submitted_text_for_harness",
            &format!("pane:{pane_id}"),
            text,
            Some(harness),
            profile.transform(),
            profile.submit_key(),
        );
        send_submitted_text_with_profile(self, pane_id, text, profile)
            .with_context(|| format!("failed to submit input to {harness} pane {pane_id}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxSubmitProfile {
    delivery: TmuxSubmitDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TmuxSubmitDelivery {
    HexTextCarriageReturn,
}

impl TmuxSubmitProfile {
    pub const fn mode(self) -> &'static str {
        match self.delivery {
            TmuxSubmitDelivery::HexTextCarriageReturn => "tmux_hex_text_cr",
        }
    }

    pub const fn transform(self) -> &'static str {
        match self.delivery {
            TmuxSubmitDelivery::HexTextCarriageReturn => "tmux_hex_text_cr",
        }
    }

    pub const fn submit_key(self) -> &'static str {
        match self.delivery {
            TmuxSubmitDelivery::HexTextCarriageReturn => "CR",
        }
    }

    pub const fn pending_draft_enter_resubmit(self) -> bool {
        matches!(self.delivery, TmuxSubmitDelivery::HexTextCarriageReturn)
    }
}

pub const fn tmux_submit_profile_for_harness(harness: &str) -> TmuxSubmitProfile {
    let _ = harness;
    TmuxSubmitProfile {
        delivery: TmuxSubmitDelivery::HexTextCarriageReturn,
    }
}

pub const fn tmux_submit_mode_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).mode()
}

pub const fn tmux_submit_transform_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).transform()
}

pub const fn tmux_submit_key_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).submit_key()
}

fn submitted_text_without_trailing_line_endings(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

fn text_submit_command_args(pane_id: &str, text: &str, profile: TmuxSubmitProfile) -> Vec<String> {
    let text = submitted_text_without_trailing_line_endings(text);
    let payload = format!("{text}\r");
    match profile.delivery {
        TmuxSubmitDelivery::HexTextCarriageReturn => {
            let mut args = vec![
                "send-keys".to_string(),
                "-t".to_string(),
                pane_id.to_string(),
                "-H".to_string(),
            ];
            args.extend(payload.as_bytes().iter().map(|byte| format!("{byte:02x}")));
            args
        }
    }
}

/// Submit text and the carriage-return byte in one tmux call. Codex/Claude TUI
/// panes can leave named submit keys as drafted input in raw mode; a literal CR
/// matches the supervisor PTY submit byte while still going through tmux.
fn send_text_with_submit_key(
    tmux: &Tmux,
    pane_id: &str,
    text: &str,
    profile: TmuxSubmitProfile,
) -> Result<()> {
    let status = tmux
        .cmd()
        .args(text_submit_command_args(pane_id, text, profile))
        .status()
        .context("failed to run tmux send-keys (text + submit key)")?;
    if !status.success() {
        anyhow::bail!("tmux send-keys text submit failed for pane {}", pane_id);
    }
    Ok(())
}

fn send_submitted_text_with_profile(
    tmux: &Tmux,
    pane_id: &str,
    text: &str,
    profile: TmuxSubmitProfile,
) -> Result<()> {
    match profile.delivery {
        TmuxSubmitDelivery::HexTextCarriageReturn => {
            send_text_with_submit_key(tmux, pane_id, text, profile)
        }
    }
}

/// Return the path to the sessions registry file (relative to CWD).
pub fn registry_path() -> PathBuf {
    PathBuf::from(SESSIONS_FILE)
}

/// Return the path to the sessions registry file under `base_dir`.
pub fn registry_path_in(base_dir: &Path) -> PathBuf {
    base_dir.join(SESSIONS_FILE)
}

pub fn canonical_registry_key_in(base_dir: &Path, file: &str) -> String {
    let path = Path::new(file);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    canonicalize_or_normalize(&joined)
        .to_string_lossy()
        .to_string()
}

fn canonicalize_or_normalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| normalize_path(path))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn entry_session_id<'a>(registry_key: &'a str, entry: &'a SessionEntry) -> &'a str {
    if entry.session_id.is_empty() {
        registry_key
    } else {
        entry.session_id.as_str()
    }
}

fn choose_preferred_entry(left: &SessionEntry, right: &SessionEntry) -> bool {
    if right.session_id.is_empty() != left.session_id.is_empty() {
        return !right.session_id.is_empty();
    }
    right.started >= left.started
}

fn normalize_registry(base_dir: &Path, registry: SessionRegistry) -> SessionRegistry {
    let mut normalized = SessionRegistry::new();
    for (legacy_key, mut entry) in registry {
        if entry.session_id.is_empty() {
            entry.session_id = legacy_key.clone();
        }
        let file_hint = if !entry.file.is_empty() {
            entry.file.clone()
        } else {
            legacy_key.clone()
        };
        let normalized_key = canonical_registry_key_in(base_dir, &file_hint);
        if let Some(existing) = normalized.get(&normalized_key)
            && !choose_preferred_entry(existing, &entry)
        {
            continue;
        }
        normalized.insert(normalized_key, entry);
    }
    normalized
}

fn find_registry_key_by_session_id(registry: &SessionRegistry, session_id: &str) -> Option<String> {
    registry
        .iter()
        .find_map(|(key, entry)| (entry_session_id(key, entry) == session_id).then(|| key.clone()))
}

// ---------------------------------------------------------------------------
// Agent-doc-specific tmux operations (not in tmux-router)
// ---------------------------------------------------------------------------

/// Capture the visible content of a tmux pane.
pub fn capture_pane(tmux: &Tmux, pane_id: &str) -> Result<String> {
    tmux.capture_pane_plain(pane_id)
}

/// Capture visible pane content while preserving ANSI attributes.
///
/// Prompt selection parsers need the highlighted option color; the normal
/// tmux-router capture helper intentionally returns plain text.
pub fn capture_pane_with_ansi(tmux: &Tmux, pane_id: &str) -> Result<String> {
    tmux.capture_pane_with_ansi(pane_id)
}

/// Join `src` beside `dst` only if both belong to `expected_session`.
///
/// Rescue paths use this to make a stashed pane visible again without
/// displacing another live pane into stash.
pub fn join_pane_guarded(
    tmux: &Tmux,
    src: &str,
    dst: &str,
    expected_session: &str,
    join_flag: &str,
) -> Result<()> {
    tmux.ensure_pane_in_session(src, expected_session)?;
    tmux.ensure_pane_in_session(dst, expected_session)?;
    PaneMoveOp::new(tmux, src, dst).join(join_flag)
}

/// Send a single key (not literal text) to a tmux pane.
///
/// Unlike `Tmux::send_keys` (which submits literal text), this sends
/// a single key name like "Up", "Down", "Enter" — used for TUI navigation.
pub fn send_key(tmux: &Tmux, pane_id: &str, key: &str) -> Result<()> {
    Multiplexer::send_key(tmux, pane_id, key)
}

/// Submit a single-line command through the default tmux submit profile.
///
/// This is the canonical live-pane submission helper for agent-doc-managed
/// harness commands such as routed reopen triggers and file-scoped `/clear`.
pub fn send_submitted_text(tmux: &Tmux, pane_id: &str, text: &str) -> Result<()> {
    tmux.send_submitted_text(pane_id, text)
}

/// Submit a single-line command through a harness-aware tmux submit path.
///
/// The submit profile is the single place that defines harness-specific tmux
/// key boundaries. Codex, Claude, OpenCode, and the default harness profile use
/// hex-encoded text plus the profile submit byte in one tmux `send-keys -H` operation.
pub fn send_submitted_text_for_harness(
    tmux: &Tmux,
    pane_id: &str,
    text: &str,
    harness: &str,
) -> Result<()> {
    tmux.send_submitted_text_for_harness(pane_id, text, harness)
}

// ---------------------------------------------------------------------------
// Free functions — registry operations and env-based checks
// ---------------------------------------------------------------------------

/// Load the session registry from disk. Returns empty map if file doesn't exist.
///
/// **Not locked internally.** Callers performing read-modify-write must acquire
/// `RegistryLock` first (or use `tmux_router::with_registry`). Read-only callers
/// can call this directly for a point-in-time snapshot.
pub fn load() -> Result<SessionRegistry> {
    load_in(&std::env::current_dir()?)
}

/// Load the session registry from `base_dir/.agent-doc/sessions.json`.
pub fn load_in(base_dir: &Path) -> Result<SessionRegistry> {
    let path = registry_path_in(base_dir);
    if !path.exists() {
        return Ok(SessionRegistry::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let registry: SessionRegistry = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(normalize_registry(base_dir, registry))
}

/// Save the session registry to disk.
///
/// **Not locked internally.** Callers MUST hold `RegistryLock` before calling.
/// Prefer `tmux_router::with_registry()` for safe read-modify-write cycles.
pub fn save(registry: &SessionRegistry) -> Result<()> {
    save_in(&std::env::current_dir()?, registry)
}

/// Save the session registry to `base_dir/.agent-doc/sessions.json`.
pub fn save_in(base_dir: &Path, registry: &SessionRegistry) -> Result<()> {
    let path = registry_path_in(base_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(registry)?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Register a session → pane mapping.
/// Get the PID of the foreground process in a tmux pane.
pub fn pane_pid(pane_id: &str) -> Result<u32> {
    pane_pid_with_mux(&Tmux::default_server(), pane_id)
}

pub fn pane_pid_with_mux(mux: &dyn Multiplexer, pane_id: &str) -> Result<u32> {
    let pid_str = mux
        .display_message(Some(pane_id), "#{pane_pid}")
        .with_context(|| format!("failed to query multiplexer pane PID for {}", pane_id))?;
    pid_str
        .parse::<u32>()
        .with_context(|| format!("invalid PID '{}' for pane {}", pid_str, pane_id))
}

pub fn register(session_id: &str, pane_id: &str, file: &str) -> Result<()> {
    register_with_pid(session_id, pane_id, file, std::process::id())
}

#[allow(dead_code)]
pub fn deregister(session_id: &str) -> Result<bool> {
    let registry_path = registry_path();
    let _lock = RegistryLock::acquire(&registry_path)?;
    let mut registry = load()?;
    let removed = find_registry_key_by_session_id(&registry, session_id)
        .and_then(|key| registry.remove(&key))
        .is_some();
    if removed {
        save(&registry)?;
    }
    Ok(removed)
}

pub fn register_with_pid(session_id: &str, pane_id: &str, file: &str, pid: u32) -> Result<()> {
    register_with_pid_internal(session_id, pane_id, file, pid, "register", "register")
}

fn register_with_pid_internal(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    // Query the window ID for this pane
    let window = pane_window(pane_id).unwrap_or_default();
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        &window,
        None,
        None,
        transition_caller,
        transition_reason,
    )
}

/// Like `register_with_pid` but accepts an explicit `cwd` override.
/// Used by `claim` to record the document's nearest git repo root as cwd,
/// avoiding superproject drift when claiming submodule-hosted documents.
pub fn register_with_pid_and_cwd(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    cwd: &str,
) -> Result<()> {
    let window = pane_window(pane_id).unwrap_or_default();
    register_full_with_cwd_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        &window,
        cwd,
        None,
        "claim",
        "claim_bind",
    )
}

#[allow(dead_code)]
pub fn attach_with_pid_and_cwd_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&registry_path_in(base_dir))?;
    let mut registry = load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "session",
        "manual_attach",
    )
}

pub fn attach_projection_only_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&registry_path_in(base_dir))?;
    let mut registry = load_in(base_dir)?;
    let started = chrono_now();
    let registry_key = canonical_registry_key_in(base_dir, file);
    let stale_keys: Vec<String> = registry
        .iter()
        .filter(|(key, entry)| entry.pane == pane_id && entry_session_id(key, entry) != session_id)
        .map(|(key, _)| key.clone())
        .collect();
    for key in stale_keys {
        eprintln!(
            "[registry] removing stale session {} (was pane {})",
            key, pane_id
        );
        registry.remove(&key);
    }
    registry.insert(
        registry_key,
        SessionEntry {
            pane: pane_id.to_string(),
            pid,
            cwd: cwd.to_string(),
            started,
            session_id: session_id.to_string(),
            file: file.to_string(),
            window: window.to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    save_in(base_dir, &registry)
}

pub fn register_supervisor(
    session_id: &str,
    pane_id: &str,
    file: &str,
    supervisor_pid: u32,
    supervisor_instance_id: &str,
) -> Result<()> {
    let window = pane_window(pane_id).unwrap_or_default();
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        supervisor_pid,
        &window,
        None,
        Some(supervisor_instance_id),
        "start",
        "supervisor_register",
    )
}

pub fn register_supervisor_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    supervisor_pid: u32,
    supervisor_instance_id: &str,
) -> Result<()> {
    let window = pane_window(pane_id).unwrap_or_default();
    register_full_with_cwd_and_instance_in(
        base_dir,
        session_id,
        pane_id,
        file,
        supervisor_pid,
        &window,
        &base_dir.to_string_lossy(),
        supervisor_instance_id,
        "sync",
        "recover_owner",
    )
}

#[cfg(test)]
#[allow(dead_code)]
pub fn register_full(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
) -> Result<()> {
    register_full_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        window,
        None,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full` but with an explicit `base_dir` for the registry.
#[cfg(test)]
pub fn register_full_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&registry_path_in(base_dir))?;
    let mut registry = load_in(base_dir)?;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        &cwd,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full` but uses the provided `cwd` instead of querying the process cwd.
#[cfg(test)]
pub fn register_full_with_cwd(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    register_full_with_cwd_internal_call(
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "test",
        "test_register",
    )
}

/// Like `register_full_with_cwd` but with an explicit `base_dir` for the registry.
pub fn register_full_with_cwd_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&registry_path_in(base_dir))?;
    let mut registry = load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        None,
        "route",
        "dispatch_bind",
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_with_cwd_and_instance_in(
    base_dir: &Path,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: &str,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let _lock = RegistryLock::acquire(&registry_path_in(base_dir))?;
    let mut registry = load_in(base_dir)?;
    register_full_internal(
        base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        Some(supervisor_instance_id),
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_internal_call(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: Option<&str>,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let base_dir = std::env::current_dir()?;
    let resolved_cwd = cwd
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| base_dir.to_string_lossy().to_string());
    let _lock = RegistryLock::acquire(&registry_path_in(&base_dir))?;
    let mut registry = load_in(&base_dir)?;
    register_full_internal(
        &base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        &resolved_cwd,
        supervisor_instance_id,
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_with_cwd_internal_call(
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let base_dir = std::env::current_dir()?;
    let _lock = RegistryLock::acquire(&registry_path_in(&base_dir))?;
    let mut registry = load_in(&base_dir)?;
    register_full_internal(
        &base_dir,
        &mut registry,
        session_id,
        pane_id,
        file,
        pid,
        window,
        cwd,
        supervisor_instance_id,
        transition_caller,
        transition_reason,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_full_internal(
    base_dir: &Path,
    registry: &mut SessionRegistry,
    session_id: &str,
    pane_id: &str,
    file: &str,
    pid: u32,
    window: &str,
    cwd: &str,
    supervisor_instance_id: Option<&str>,
    transition_caller: &'static str,
    transition_reason: &'static str,
) -> Result<()> {
    let started = chrono_now();
    let registry_key = canonical_registry_key_in(base_dir, file);
    let supervisor_instance_id = supervisor_instance_id.unwrap_or_default().to_string();

    // Enforce single session per pane: remove stale entries pointing to same pane
    let stale_keys: Vec<String> = registry
        .iter()
        .filter(|(k, e)| e.pane == pane_id && entry_session_id(k, e) != session_id)
        .map(|(k, _)| k.clone())
        .collect();
    for key in &stale_keys {
        eprintln!(
            "[registry] removing stale session {} (was pane {})",
            key, pane_id
        );
        registry.remove(key);
    }

    let mut controller_row_exists = false;
    if let Some(previous) = registry.get(&registry_key).cloned() {
        if transition_caller != "start" {
            let generations = crate::session_actor::project_binding_in(
                base_dir,
                file,
                session_id,
                pane_id,
                window,
                transition_caller,
                transition_reason,
            )?;
            log_session_rebind(
                base_dir,
                session_id,
                &previous,
                pane_id,
                window,
                transition_caller,
                transition_reason,
                generations,
            );
            controller_row_exists = true;
        }
    } else if transition_caller != "start" {
        let _ = crate::session_actor::project_binding_in(
            base_dir,
            file,
            session_id,
            pane_id,
            window,
            transition_caller,
            transition_reason,
        )?;
        controller_row_exists = true;
    } else if crate::project_controller::load_actor_record(base_dir, &registry_key)?.is_some() {
        controller_row_exists = true;
    }

    if controller_row_exists {
        let hint = crate::project_controller::SessionsProjectionHint {
            session_id: session_id.to_string(),
            pane_id: pane_id.to_string(),
            file: file.to_string(),
            pid,
            window_id: window.to_string(),
            cwd: cwd.to_string(),
            supervisor_instance_id: supervisor_instance_id.clone(),
        };
        crate::project_controller::project_sessions_projection_for_actor_with_hint(
            base_dir,
            &registry_key,
            Some(&hint),
        )?;
        return Ok(());
    }

    registry.insert(
        registry_key.clone(),
        SessionEntry {
            pane: pane_id.to_string(),
            pid,
            cwd: cwd.to_string(),
            started,
            session_id: session_id.to_string(),
            file: file.to_string(),
            window: window.to_string(),
            supervisor_instance_id,
        },
    );
    save_in(base_dir, registry)?;
    let _ =
        crate::project_controller::project_sessions_projection_for_actor(base_dir, &registry_key);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn log_session_rebind(
    base_dir: &Path,
    session_id: &str,
    previous: &SessionEntry,
    new_pane: &str,
    new_window: &str,
    transition_caller: &str,
    transition_reason: &str,
    generations: crate::session_actor::OwnershipGeneration,
) {
    if previous.pane == new_pane {
        return;
    }

    let log_file = resolve_log_file_path(base_dir, &previous.file);
    let old_window = if previous.window.is_empty() {
        "unknown"
    } else {
        previous.window.as_str()
    };
    let new_window = if new_window.is_empty() {
        "unknown"
    } else {
        new_window
    };

    let transition = crate::session_actor::format_transition_event(
        crate::session_actor::OwnershipTransitionEvent {
            caller: transition_caller,
            reason: transition_reason,
            prior_generation: generations.prior_generation,
            new_generation: generations.new_generation,
            old_pane: Some(previous.pane.as_str()),
            new_pane,
            old_window: Some(old_window),
            new_window: Some(new_window),
        },
    );
    if let Err(err) =
        crate::startup_miss::append_session_log_event(&log_file, session_id, &transition)
    {
        eprintln!(
            "[registry] warning: failed to append ownership transition for {}: {}",
            session_id, err
        );
        return;
    }

    let superseded = format!(
        "session_superseded old_pane={} new_pane={} old_window={} new_window={} prior_generation={} new_generation={}",
        previous.pane,
        new_pane,
        old_window,
        new_window,
        generations.prior_generation,
        generations.new_generation
    );
    if let Err(err) =
        crate::startup_miss::append_session_log_event(&log_file, session_id, &superseded)
    {
        eprintln!(
            "[registry] warning: failed to append session superseded event for {}: {}",
            session_id, err
        );
        return;
    }
    if let Err(err) = crate::startup_miss::append_session_log_event(
        &log_file,
        session_id,
        &format!(
            "session_end origin=registry_rebind pane={} next_pane={} generation={} next_generation={}",
            previous.pane, new_pane, generations.prior_generation, generations.new_generation
        ),
    ) {
        eprintln!(
            "[registry] warning: failed to append registry-rebind session_end for {}: {}",
            session_id, err
        );
    }
}

fn resolve_log_file_path(base_dir: &Path, file: &str) -> PathBuf {
    let file_path = Path::new(file);
    if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        base_dir.join(file_path)
    }
}

/// Query the tmux window ID for a pane.
pub fn pane_window(pane_id: &str) -> Result<String> {
    pane_window_with_mux(&Tmux::default_server(), pane_id)
}

pub fn pane_window_with_mux(mux: &dyn Multiplexer, pane_id: &str) -> Result<String> {
    mux.display_message(Some(pane_id), "#{window_id}")
        .with_context(|| format!("failed to query multiplexer window ID for {}", pane_id))
}

/// Look up the pane ID for a session.
pub fn lookup(session_id: &str) -> Result<Option<String>> {
    let registry = load()?;
    Ok(find_registry_key_by_session_id(&registry, session_id)
        .and_then(|key| registry.get(&key).map(|entry| entry.pane.clone())))
}

/// Look up the pane ID for a session in a specific base directory's registry.
pub fn lookup_in(base_dir: &Path, session_id: &str) -> Result<Option<String>> {
    let registry = load_in(base_dir)?;
    Ok(find_registry_key_by_session_id(&registry, session_id)
        .and_then(|key| registry.get(&key).map(|entry| entry.pane.clone())))
}

/// Look up a full registry entry by session ID.
pub fn lookup_entry(session_id: &str) -> Result<Option<SessionEntry>> {
    let registry = load()?;
    Ok(find_registry_key_by_session_id(&registry, session_id)
        .and_then(|key| registry.get(&key).cloned()))
}

/// Get the pane ID of the current pane.
/// Tries TMUX_PANE env var first, then falls back to querying tmux
/// for the active pane (works from outside tmux, e.g. IDE processes).
pub fn current_pane() -> Result<String> {
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        return Ok(pane);
    }
    current_pane_with_mux(&Tmux::default_server())
}

pub fn current_pane_with_mux(mux: &dyn Multiplexer) -> Result<String> {
    let pane = mux
        .display_message(None, "#{pane_id}")
        .context("failed to query multiplexer for active pane")?;
    if pane.is_empty() {
        anyhow::bail!("multiplexer returned empty pane ID");
    }
    Ok(pane)
}

/// Resolve a pane by positional hint (left, right, top, bottom).
/// Queries `tmux list-panes` for the current window and selects the pane
/// at the requested position based on coordinates.
pub fn pane_by_position(position: &str) -> Result<String> {
    pane_by_position_with_mux(&Tmux::default_server(), position)
}

pub fn pane_by_position_with_mux(mux: &dyn Multiplexer, position: &str) -> Result<String> {
    let text = mux
        .list_panes(
            None,
            "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}",
        )
        .context("failed to query multiplexer panes")?;
    select_pane_by_position(&text, position, "current multiplexer window")
}

fn select_pane_by_position(text: &str, position: &str, scope: &str) -> Result<String> {
    let mut panes: Vec<(String, u32, u32, u32, u32)> = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 {
            let id = parts[0].to_string();
            let left: u32 = parts[1].parse().unwrap_or(0);
            let top: u32 = parts[2].parse().unwrap_or(0);
            let width: u32 = parts[3].parse().unwrap_or(0);
            let height: u32 = parts[4].parse().unwrap_or(0);
            panes.push((id, left, top, width, height));
        }
    }
    if panes.is_empty() {
        anyhow::bail!("no panes found in {}", scope);
    }
    if panes.len() == 1 {
        return Ok(panes[0].0.clone());
    }
    let selected = match position {
        "left" => panes.iter().min_by_key(|p| p.1),
        "right" => panes.iter().max_by_key(|p| p.1 + p.3),
        "top" => panes.iter().min_by_key(|p| p.2),
        "bottom" => panes.iter().max_by_key(|p| p.2 + p.4),
        _ => anyhow::bail!(
            "invalid position '{}' — use left, right, top, or bottom",
            position
        ),
    };
    match selected {
        Some(pane) => Ok(pane.0.clone()),
        None => anyhow::bail!("could not resolve pane for position '{}'", position),
    }
}

/// Resolve a pane by positional hint within a specific tmux window.
/// Like `pane_by_position` but scoped to the given window ID (e.g. `@1`).
pub fn pane_by_position_in_window(position: &str, window: &str) -> Result<String> {
    pane_by_position_in_window_with_mux(&Tmux::default_server(), position, window)
}

pub fn pane_by_position_in_window_with_mux(
    mux: &dyn Multiplexer,
    position: &str,
    window: &str,
) -> Result<String> {
    let text = mux
        .list_panes(
            Some(window),
            "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}",
        )
        .with_context(|| format!("failed to query multiplexer panes for window {}", window))?;
    select_pane_by_position(&text, position, &format!("multiplexer window {}", window))
}

/// Check if we're inside tmux.
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Simple UTC timestamp without pulling in chrono.
fn chrono_now() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    #[derive(Default)]
    struct MockMultiplexer {
        active_pane: String,
        pane_pid: String,
        pane_window: String,
        list_panes: String,
        window_list_panes: String,
    }

    impl Multiplexer for MockMultiplexer {
        fn display_message(&self, target: Option<&str>, format: &str) -> Result<String> {
            match (target, format) {
                (None, "#{pane_id}") => Ok(self.active_pane.clone()),
                (Some(_), "#{pane_pid}") => Ok(self.pane_pid.clone()),
                (Some(_), "#{window_id}") => Ok(self.pane_window.clone()),
                _ => anyhow::bail!("unexpected display-message query: {target:?} {format}"),
            }
        }

        fn list_panes(&self, target: Option<&str>, _format: &str) -> Result<String> {
            if target.is_some() {
                Ok(self.window_list_panes.clone())
            } else {
                Ok(self.list_panes.clone())
            }
        }

        fn pane_alive(&self, _pane_id: &str) -> bool {
            true
        }

        fn session_alive(&self, _name: &str) -> bool {
            true
        }

        fn capture_pane_plain(&self, _pane_id: &str) -> Result<String> {
            Ok(String::new())
        }

        fn capture_pane_with_ansi(&self, _pane_id: &str) -> Result<String> {
            Ok(String::new())
        }

        fn send_key(&self, _pane_id: &str, _key: &str) -> Result<()> {
            Ok(())
        }

        fn send_submitted_text(&self, _pane_id: &str, _text: &str) -> Result<()> {
            Ok(())
        }

        fn send_submitted_text_for_harness(
            &self,
            _pane_id: &str,
            _text: &str,
            _harness: &str,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn multiplexer_queries_support_mock_backend() {
        let mux = MockMultiplexer {
            active_pane: "%active".to_string(),
            pane_pid: "4242".to_string(),
            pane_window: "@7".to_string(),
            ..Default::default()
        };
        let mux_ref: &dyn Multiplexer = &mux;

        assert_eq!(current_pane_with_mux(&mux).unwrap(), "%active");
        assert_eq!(pane_pid_with_mux(&mux, "%active").unwrap(), 4242);
        assert_eq!(pane_window_with_mux(&mux, "%active").unwrap(), "@7");
        assert!(mux_ref.pane_alive("%active"));
        assert!(mux_ref.session_alive("agent-doc"));
    }

    #[test]
    fn submit_profiles_keep_harness_submit_policy_in_one_place() {
        assert_eq!(tmux_submit_mode_for_harness("codex"), "tmux_hex_text_cr");
        assert_eq!(
            tmux_submit_transform_for_harness("codex"),
            "tmux_hex_text_cr"
        );
        assert_eq!(tmux_submit_key_for_harness("codex"), "CR");
        assert_eq!(
            submitted_text_without_trailing_line_endings("agent-doc plan.md\r\n"),
            "agent-doc plan.md"
        );
        assert_eq!(
            text_submit_command_args(
                "%7",
                "agent-doc plan.md\n",
                tmux_submit_profile_for_harness("codex")
            ),
            [
                "send-keys",
                "-t",
                "%7",
                "-H",
                "61",
                "67",
                "65",
                "6e",
                "74",
                "2d",
                "64",
                "6f",
                "63",
                "20",
                "70",
                "6c",
                "61",
                "6e",
                "2e",
                "6d",
                "64",
                "0d",
            ]
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            text_submit_command_args("%7", "\n", tmux_submit_profile_for_harness("codex")),
            ["send-keys", "-t", "%7", "-H", "0d"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(tmux_submit_mode_for_harness("claude"), "tmux_hex_text_cr");
        assert_eq!(
            tmux_submit_transform_for_harness("claude"),
            "tmux_hex_text_cr"
        );
        assert_eq!(tmux_submit_key_for_harness("claude"), "CR");
        assert_eq!(
            tmux_submit_mode_for_harness("unknown-harness"),
            "tmux_hex_text_cr"
        );
        assert_eq!(tmux_submit_mode_for_harness("opencode"), "tmux_hex_text_cr");
        assert_eq!(
            tmux_submit_transform_for_harness("opencode"),
            "tmux_hex_text_cr"
        );
        assert_eq!(tmux_submit_key_for_harness("opencode"), "CR");
    }

    #[test]
    fn multiplexer_position_selection_is_backend_independent() {
        let mux = MockMultiplexer {
            list_panes: "%left 0 0 80 24\n%right 120 0 80 24\n%bottom 0 24 160 24\n".to_string(),
            window_list_panes: "%top 0 0 160 12\n%low 0 12 160 36\n".to_string(),
            ..Default::default()
        };

        assert_eq!(pane_by_position_with_mux(&mux, "left").unwrap(), "%left");
        assert_eq!(pane_by_position_with_mux(&mux, "right").unwrap(), "%right");
        assert_eq!(
            pane_by_position_in_window_with_mux(&mux, "bottom", "@1").unwrap(),
            "%low"
        );
    }

    #[test]
    fn multiplexer_position_rejects_unknown_direction() {
        let err = select_pane_by_position("%0 0 0 80 24\n%1 80 0 80 24\n", "middle", "test")
            .unwrap_err()
            .to_string();

        assert!(err.contains("invalid position 'middle'"));
    }

    #[test]
    fn registry_roundtrip() {
        let dir = TempDir::new().unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(
            "test-session".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 12345,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "test-session".to_string(),
                file: "test.md".to_string(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        save_in(dir.path(), &reg).unwrap();
        let loaded = load_in(dir.path()).unwrap();
        let key = canonical_registry_key_in(dir.path(), "test.md");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&key].pane, "%42");
    }

    #[test]
    fn load_empty_returns_empty_map() {
        let dir = TempDir::new().unwrap();
        let reg = load_in(dir.path()).unwrap();
        assert!(reg.is_empty());
    }

    #[test]
    fn pane_alive_returns_false_for_nonexistent() {
        assert!(!Tmux::default_server().pane_alive("%99999"));
    }

    #[test]
    fn registry_multiple_sessions_isolated() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%10".to_string(),
                pid: 1000,
                cwd: "/tmp/a".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-a".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%20".to_string(),
                pid: 2000,
                cwd: "/tmp/b".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                session_id: "session-b".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        let json = serde_json::to_string_pretty(&reg).unwrap();
        let loaded: SessionRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded["session-a"].pane, "%10");
        assert_eq!(loaded["session-b"].pane, "%20");
        assert_ne!(loaded["session-a"].pane, loaded["session-b"].pane);
        assert_ne!(loaded["session-a"].pid, loaded["session-b"].pid);
    }

    #[test]
    fn registry_overwrite_existing_session() {
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%old".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-x".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-x".to_string(),
            SessionEntry {
                pane: "%new".to_string(),
                pid: 200,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:05:00Z".to_string(),
                session_id: "session-x".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        assert_eq!(reg.len(), 1);
        assert_eq!(reg["session-x"].pane, "%new");
        assert_eq!(reg["session-x"].pid, 200);
    }

    #[test]
    fn prune_removes_dead_panes_from_map() {
        let tmux = Tmux::default_server();
        let mut reg = SessionRegistry::new();
        reg.insert(
            "dead-session-1".to_string(),
            SessionEntry {
                pane: "%99998".to_string(),
                pid: 1,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "dead-session-1".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "dead-session-2".to_string(),
            SessionEntry {
                pane: "%99997".to_string(),
                pid: 2,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "dead-session-2".to_string(),
                file: String::new(),
                window: String::new(),
                supervisor_instance_id: String::new(),
            },
        );

        let before = reg.len();
        reg.retain(|_, entry| tmux.pane_alive(&entry.pane));
        let removed = before - reg.len();

        assert_eq!(removed, 2);
        assert!(reg.is_empty());
    }

    // -----------------------------------------------------------------------
    // Tmux isolation tests — use `-L` to create independent tmux servers
    // -----------------------------------------------------------------------

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_isolated_server_not_running_initially() {
        let t = IsolatedTmux::new("agent-doc-test-not-running");
        assert!(!t.running());
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_create_session_and_verify() {
        let t = IsolatedTmux::new("agent-doc-test-create-session");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test-session", tmp.path()).unwrap();
        assert!(!pane_id.is_empty(), "pane_id should not be empty");
        assert!(pane_id.starts_with('%'), "pane_id should start with %");

        assert!(t.running());
        assert!(t.session_exists("test-session"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_session_exists_returns_false_for_missing() {
        let t = IsolatedTmux::new("agent-doc-test-session-missing");
        let tmp = TempDir::new().unwrap();

        t.new_session("existing", tmp.path()).unwrap();

        assert!(t.session_exists("existing"));
        assert!(!t.session_exists("nonexistent"));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_new_window_creates_second_pane() {
        let t = IsolatedTmux::new("agent-doc-test-new-window");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("test", tmp.path()).unwrap();
        let pane2 = t.new_window("test", tmp.path()).unwrap();

        assert_ne!(pane1, pane2, "two windows should have different pane IDs");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_send_keys_to_pane() {
        let t = IsolatedTmux::new("agent-doc-test-send-keys");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        t.send_keys(&pane_id, "echo hello").unwrap();
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn opencode_submit_uses_hex_text_cr() {
        let t = IsolatedTmux::new("agent-doc-test-opencode-hex-cr");
        let tmp = TempDir::new().unwrap();
        let pane_id = t.new_session("test", tmp.path()).unwrap();
        let output_path = tmp.path().join("input.bin");
        let done_path = tmp.path().join("done");
        let ready_path = tmp.path().join("ready");
        let expected = b"agent-doc plan.md\r";

        let reader = format!(
            "sh -lc 'stty raw -echo; touch \"{}\"; dd bs=1 count={} of=\"{}\" 2>/dev/null; touch \"{}\"'",
            ready_path.display(),
            expected.len(),
            output_path.display(),
            done_path.display()
        );
        let setup_status = t
            .cmd()
            .args(["send-keys", "-t", &pane_id, "-l"])
            .arg(&reader)
            .status()
            .unwrap();
        assert!(setup_status.success(), "raw reader setup command failed");
        t.send_key(&pane_id, "Enter").unwrap();

        for _ in 0..60 {
            if ready_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            ready_path.exists(),
            "expected raw reader to enter raw mode before payload submit"
        );

        send_submitted_text_for_harness(&t, &pane_id, "agent-doc plan.md\n", "opencode").unwrap();

        for _ in 0..60 {
            if done_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(done_path.exists(), "expected raw reader to finish");
        assert_eq!(std::fs::read(output_path).unwrap(), expected);
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_pane_alive_returns_false_after_kill() {
        let t = IsolatedTmux::new("agent-doc-test-pane-kill");
        let tmp = TempDir::new().unwrap();

        let pane_id = t.new_session("test", tmp.path()).unwrap();
        assert!(t.pane_alive(&pane_id));

        // Create a second window so we can kill the first without killing the session
        let _pane2 = t.new_window("test", tmp.path()).unwrap();

        let _ = t.cmd().args(["kill-pane", "-t", &pane_id]).status();

        assert!(!t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_no_server() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-server");
        let tmp = TempDir::new().unwrap();

        assert!(!t.running());
        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(!pane_id.is_empty());
        assert!(t.running());
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_no_session() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-no-session");
        let tmp = TempDir::new().unwrap();

        t.new_session("other", tmp.path()).unwrap();
        assert!(t.running());
        assert!(!t.session_exists("claude"));

        let pane_id = t.auto_start("claude", tmp.path()).unwrap();
        assert!(t.session_exists("claude"));
        assert!(t.pane_alive(&pane_id));
    }

    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn tmux_auto_start_cascade_session_exists() {
        let t = IsolatedTmux::new("agent-doc-test-autostart-exists");
        let tmp = TempDir::new().unwrap();

        let pane1 = t.new_session("claude", tmp.path()).unwrap();

        let pane2 = t.auto_start("claude", tmp.path()).unwrap();
        assert_ne!(pane1, pane2, "should be a different pane (new window)");
        assert!(t.pane_alive(&pane1));
        assert!(t.pane_alive(&pane2));
    }

    // --- #tw4a: register_with_pid_and_cwd tests ---

    #[test]
    fn register_with_pid_and_cwd_stores_explicit_cwd() {
        // register_full_with_cwd_in should store the provided cwd string
        // rather than querying the process cwd.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let explicit_cwd = "/home/user/projects/my-submodule";
        register_full_with_cwd_in(
            dir.path(),
            "session-cwd-test",
            "%77",
            "plan.md",
            9999,
            "@5",
            explicit_cwd,
        )
        .unwrap();

        let loaded = load_in(dir.path()).unwrap();
        let key = canonical_registry_key_in(dir.path(), "plan.md");
        assert!(loaded.contains_key(&key), "session should be registered");
        let entry = &loaded[&key];
        assert_eq!(
            entry.cwd, explicit_cwd,
            "stored cwd must match the explicit value passed in"
        );
        assert_eq!(entry.pane, "%77");
        assert_eq!(entry.file, "plan.md");
        assert_eq!(entry.pid, 9999);
    }

    #[test]
    fn register_full_with_cwd_differs_from_process_cwd() {
        // The cwd stored by register_full_with_cwd_in must be the explicitly passed value,
        // even when it differs from the process cwd.
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let explicit_cwd = "/completely/different/path";
        register_full_with_cwd_in(
            dir.path(),
            "s-explicit",
            "%88",
            "doc.md",
            1234,
            "@1",
            explicit_cwd,
        )
        .unwrap();

        let loaded = load_in(dir.path()).unwrap();
        let key = canonical_registry_key_in(dir.path(), "doc.md");
        let entry = &loaded[&key];
        assert_eq!(entry.cwd, explicit_cwd);
        // Verify it differs from the actual process cwd
        let process_cwd = std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_ne!(
            entry.cwd, process_cwd,
            "stored cwd should differ from process cwd"
        );
    }

    #[test]
    fn register_full_deduplicates_pane() {
        // When a new session claims a pane, old sessions pointing to the same pane are removed
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        // Seed registry with two sessions pointing to the same pane
        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-a".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-a".to_string(),
                file: "old-file.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        reg.insert(
            "session-b".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: "/tmp".to_string(),
                started: "2026-01-01T00:01:00Z".to_string(),
                session_id: "session-b".to_string(),
                file: "another-old.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        save_in(dir.path(), &reg).unwrap();

        // Now register session-c with the same pane %42
        register_full_in(dir.path(), "session-c", "%42", "new-file.md", 200, "@1").unwrap();

        let loaded = load_in(dir.path()).unwrap();
        let new_key = canonical_registry_key_in(dir.path(), "new-file.md");
        let old_key_a = canonical_registry_key_in(dir.path(), "old-file.md");
        let old_key_b = canonical_registry_key_in(dir.path(), "another-old.md");
        // Only session-c should remain for pane %42
        assert!(loaded.contains_key(&new_key), "new session should exist");
        assert!(
            !loaded.contains_key(&old_key_a),
            "old session-a should be removed"
        );
        assert!(
            !loaded.contains_key(&old_key_b),
            "old session-b should be removed"
        );
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[&new_key].file, "new-file.md");
    }

    #[test]
    fn register_full_logs_session_rebind_before_overwriting_pane() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("tasks/rebind.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "# rebind\n").unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/logs/session-rebind.log"),
            "[1] session_start file=tasks/rebind.md pane=%42 session=session-rebind generation=1\n[2] codex_start mode=fresh restart_count=0\n",
        )
        .unwrap();

        let mut reg = SessionRegistry::new();
        reg.insert(
            "session-rebind".to_string(),
            SessionEntry {
                pane: "%42".to_string(),
                pid: 100,
                cwd: dir.path().display().to_string(),
                started: "2026-01-01T00:00:00Z".to_string(),
                session_id: "session-rebind".to_string(),
                file: "tasks/rebind.md".to_string(),
                window: "@7".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        save_in(dir.path(), &reg).unwrap();

        register_full_in(
            dir.path(),
            "session-rebind",
            "%84",
            "tasks/rebind.md",
            200,
            "@9",
        )
        .unwrap();

        let log =
            std::fs::read_to_string(dir.path().join(".agent-doc/logs/session-rebind.log")).unwrap();
        assert!(log.contains(
            "ownership_transition caller=test reason=test_register prior_generation=1 new_generation=2 old_pane=%42 new_pane=%84 old_window=@7 new_window=@9"
        ));
        assert!(
            log.contains(
                "session_superseded old_pane=%42 new_pane=%84 old_window=@7 new_window=@9 prior_generation=1 new_generation=2"
            )
        );
        assert!(log.contains(
            "session_end origin=registry_rebind pane=%42 next_pane=%84 generation=1 next_generation=2"
        ));
    }
}
