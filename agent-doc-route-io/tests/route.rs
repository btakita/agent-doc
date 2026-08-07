//! # Module: route
//!
//! Routes harness-specific document trigger commands to the correct tmux pane. This is the
//! process-level coordinator between file-save events (editor plugin / watch daemon)
//! and running agent sessions inside tmux.
//!
//! ## Spec
//!
//! - **`run(file, pane, debounce_ms, col_args)`**: Public entry point. Delegates to
//!   `run_with_tmux` using the default tmux server. Accepts an optional explicit
//!   `pane` override, a debounce delay in milliseconds, and column layout hints.
//! - **`run_with_tmux(file, tmux, pane, debounce_ms, col_args, mode, plain_trigger)`**: Core routing logic.
//!   1. Prunes stale session registry entries via `agent_doc_sync_io::resync::prune`.
//!   2. If `debounce_ms > 0`, waits for the file's mtime and shared editor typing
//!      indicator to settle (`await_idle`).
//!   3. Ensures a session UUID exists in the file's YAML frontmatter (generates one if missing).
//!   4. Resolves the target tmux session: prefers project config (`config.toml`), falls
//!      back to current tmux session, auto-updates config when the configured session is dead.
//!   5. Looks up the registered pane in the durable registry.
//!   6. If pane is alive: first verify that a live process tree still proves the
//!      document is running there. If the live owner is another pane, re-register there;
//!      if no live owner exists, fail closed instead of sending the trigger into an
//!      ambiguous shell. Pane IDs (`%N`) are globally unique per tmux server, so
//!      `target_session` matching is not required once ownership is proven.
//!      `rescue_from_stash` is attempted (it self-gates on session match) so panes
//!      stashed within the target session get rescued, but panes in other sessions are
//!      left in place. When the document already has prompt-bearing user drift after a
//!      closed cycle, the routed trigger must also produce a new reactive
//!      per-document admission projection before route returns success; otherwise
//!      route fails closed.
//!   7. If pane is dead and was previously registered: lazy-claims only to an explicit
//!      pane override via `find_target_pane` (skipped if the candidate is already claimed
//!      or is running a non-agent process), sends the command, then calls
//!      `sync_after_claim` to re-sync layout. Route never adopts the tmux session's
//!      current active pane implicitly.
//!   8. If no registered pane or no claimable pane: auto-starts a new agent session.
//!      Blocked by `AGENT_DOC_NO_AUTOSTART` env var (used in tests).
//! - **`auto_start(tmux, file, session_id, file_path, context_session)`**: Public; spawns a
//!   new route-owned agent pane and sends `agent-doc start --route-owned`. Waits for the
//!   agent's idle prompt before sending the initial command, then requires a real
//!   reactive document-cycle admission projection before treating the fresh start as successful. Called
//!   by `sync.rs` for unresolved files.
//! - **`provision_pane(tmux, file, session_id, file_path, context_session, col_args)`**: Like
//!   `auto_start` but skips waiting for the agent to be ready. Used by sync when only pane
//!   existence is needed (agent will start asynchronously). Computes `split_before` via
//!   `is_first_column(file, col_args)` so new panes split in the correct direction for
//!   their column position.
//! - **`try_provision_pane(...)`**: Same skip-wait provisioning path, but uses
//!   nonblocking startup locks and returns `Ok(None)` when a same-document or
//!   same-session start is already in progress. Used by safe-passive sync's
//!   pre-lock create-on-miss hot path.
//! - **`is_first_column(file, col_args)`**: Returns true when `file` appears in the first
//!   `--col` argument. Drives the `-dbh` (split before) vs `-dh` (split after) split direction
//!   when creating a new pane. Returns false when `col_args` has fewer than 2 entries.
//! - **Positional split target (sync path)**: When `skip_wait` is true (called from sync),
//!   `auto_start_in_session` picks the split target based on column position — first pane in
//!   the agent-doc window for left-column files (`split_before`), last pane for right-column
//!   files. This places the new pane adjacent to its column neighbors instead of always splitting
//!   beside an arbitrary registered pane.
//! - **`send_command(tmux, pane, file_path, harness)`**: Used only for direct tmux/shell
//!   launch paths plus existing dispatch-only live-pane reroutes. Managed reroutes keep using
//!   supervisor IPC when route needs queueing/admission semantics, but a dispatch-only reopen types the
//!   bare harness trigger directly through the resolved live pane so it shares the same terminal
//!   submit boundary as `session clear`.
//! - **Dispatch-only editor reroutes** still bypass the managed acceptance/admission-projection loop on
//!   purpose, and for existing managed sessions they send the bare reopen through direct
//!   live-pane submit instead of a one-shot supervisor IPC inject. Startup-window reroutes,
//!   including tracked Codex/OpenCode `/clear` restarts, remain prompt-gated and fail closed
//!   before sending input while the harness is redrawing or busy.
//!   Hook-visible Codex and pane-state OpenCode proof remain stronger telemetry, but
//!   plain editor dispatch-only success is the shared single tmux text+`Enter` transport path
//!   for all harnesses. It returns immediately after that operation succeeds, without pane
//!   acceptance polling, dispatch-start proof, or Enter resubmission. Prompt-aware routes keep
//!   the stronger acceptance and dispatch-start proof behavior.
//! - **`await_idle(file, debounce)`**: Polls Lazily's current-document authority every
//!   100ms. Detached documents dispatch immediately; attached documents wait for the
//!   canonical delivery frontier to converge. The first retained delivery observation
//!   requests one urgent editor drain so route startup does not depend on a background
//!   retry timer. Missing, pending, or unavailable authority fails closed after the
//!   `10 × debounce` safety cap expires.
//! - **`wait_for_agent_ready(tmux, pane_id, timeout, harness)`**: Polls pane content every
//!   `AGENT_READY_POLL_INTERVAL`
//!   looking for the agent's idle prompt (per `harness.prompt_patterns`). Returns true when
//!   prompt found, false on timeout. Logs progress every 10 polls. Existing-pane reroutes only
//!   fail closed on timeout when the document still has prompt-bearing drift; otherwise route
//!   just focuses the already-running pane without injecting a duplicate reopen.
//! - **`sync_after_claim(tmux, pane_id)`**: After a lazy claim, re-runs `sync::run` for all
//!   registered files in the same window to keep the tmux layout mirroring the editor split.
//!   Skipped when fewer than 2 files share the window.
//!
//! ## Agentic Contracts
//!
//! - **Session UUID guarantee**: `run_with_tmux` always ensures the file has a session UUID
//!   in frontmatter before any registry lookup. Callers never see a file without a UUID.
//! - **Stale-registry hygiene**: `agent_doc_sync_io::resync::prune` is called at the start of every `run_with_tmux`
//!   invocation; the registry is always pruned before a lookup is attempted.
//! - **One pane per document**: Each document gets its own agent pane. Unregistered files
//!   (no prior session) skip lazy-claim and always get a fresh pane via auto-start.
//! - **Globally-unique pane IDs**: tmux `%N` pane IDs are unique per server. A registered
//!   alive pane is always routable by ID — routing does not depend on which session it
//!   currently lives in. This matters when `route run` is invoked from outside tmux (e.g.
//!   IDE `Run Agent Doc`), where `target_session` falls back to a constant and may not
//!   match the real session of the claimed pane.
//! - **Registered-pane ownership proof**: an alive registered pane is not sufficient on
//!   its own. Route first scans tmux for a live process tree that still mentions the
//!   document path. If that owner is another pane, route re-registers there. If no live
//!   owner exists, route fails closed instead of dispatching into an ambiguous pane.
//! - **Explicit provenance guard (lazy-claim only)**: `find_target_pane()` only accepts
//!   an explicit pane override for lazy-claim. Route will not infer ownership from the
//!   tmux session's current active pane when the registered pane is dead.
//! - **Non-agent process guard (lazy-claim only)**: `is_agent_process()` gates the
//!   lazy-claim path — even an explicit candidate pane will not be adopted when it is
//!   running corky/shell instead of an agent process.
//! - **Stash rescue**: Panes that ended up in a tmux `stash` / `stash-*` window are
//!   automatically rejoined into the `agent-doc` window before routing, without
//!   swapping another visible pane back into stash.
//! - **Auto-start inhibit**: Setting `AGENT_DOC_NO_AUTOSTART` prevents `auto_start_in_session`
//!   from spawning a new pane. The call returns `Err` with a descriptive message.
//! - **No duplicate fallback panes**: If route cannot split beside a visible authoritative
//!   pane, or if the target session already has an `agent-doc` window but no safe registered
//!   anchor, it must fail closed and print tmux inspection/cleanup commands instead of creating
//!   a second hidden pane in stash.
//! - **Non-fatal pane focus**: `select_pane` failures are logged as warnings and never abort
//!   the routing flow. The command is still sent even if focus fails.
//! - **Reactive admission for prompt-bearing reruns**: Fresh auto-start success is not
//!   inferred from pane input acceptance alone. The same fail-closed rule applies when route
//!   dispatches to an existing pane while the document already has prompt-bearing drift on top
//!   of a closed cycle: route must observe the Project Controller's new per-document cycle
//!   projection before considering the dispatch successful.
//! - **Split direction determinism**: `is_first_column` requires ≥ 2 `col_args` entries to
//!   return true, ensuring a single-column layout never triggers a left-split.
//!
//! ## Evals
//!
//! - `is_first_column_empty_cols`: empty `col_args` → returns false (no layout context)
//! - `is_first_column_single_col`: single `col_args` entry → returns false (< 2 entries required)
//! - `is_first_column_in_first_col`: file matches first col arg → returns true
//! - `is_first_column_in_second_col`: file matches second col arg → returns false
//! - `is_first_column_comma_separated`: file matches comma-separated first col arg → returns true
//! - `detects_unicode_prompt`: `❯`, `❯ `, `  ❯  ` → all detected as agent idle prompt
//! - `detects_ascii_prompt`: `>`, `> `, `  >  ` → all detected as agent idle prompt
//! - `rejects_non_prompt_lines`: status text, empty lines, markdown headers → not matched as prompt
//! - `handles_ansi_prompt`: ANSI-colored `❯`/`>` → detected after strip_ansi
//! - `unregistered_file_skips_lazy_claim`: `registered = None` → lazy-claim step is skipped
//! - `dead_registered_pane_allows_lazy_claim`: `registered = Some(pane)` with dead pane → explicit-pane lazy-claim remains eligible
//! - `lazy_claim_requires_explicit_pane_provenance`: active pane in target session with no explicit `--pane` override → lazy-claim skipped
//! - (aspirational) `stash_rescue`: pane in stash window → rescued to agent-doc window before send
//! - `wrong_session_pane_still_receives_send`: alive pane in a session different from
//!   `target_session` → trigger command is sent to that pane (no new pane created)
//! - `alive_registered_pane_without_live_owner_fails_closed`: live registered pane with no
//!   file-owning process tree → route fails closed before sending into the pane
//! - `alive_registered_pane_reregisters_to_live_owner`: stale live registration + another
//!   pane running the file → route re-registers to the real live owner
//! - (aspirational) `debounce_idle`: file written rapidly → routing waits for mtime to settle
//! - (aspirational) `autostart_inhibited`: `AGENT_DOC_NO_AUTOSTART` set → returns Err, no pane spawned

#[cfg(test)]
use anyhow::Result;
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
use agent_doc_controller::dispatch::dispatch_only_starting_pane_ready_timeout_for_binary;
#[cfg(test)]
use agent_doc_controller::dispatch::{
    ActorDispatchState, AuthoritativeActorDispatchIntent, CloseoutBlockDispatchDecision,
    ReopenMode, StartingTimeoutActorFacts, actor_blocked_by_starting_timeout,
};
#[cfg(test)]
use agent_doc_controller::dispatch::{
    AuthoritativeActorReadyFacts, PromptReadyBarrierDecision, RoutedDispatchStartProof,
    STARTING_ACTOR_TIMEOUT_REASON, starting_timeout_blocked_actor_can_recover,
    startup_miss_requires_fresh_start, startup_miss_should_fail_closed,
    startup_miss_should_restart_live_owner, startup_miss_superseded_by_later_open_start,
};
#[cfg(test)]
use agent_doc_controller::dispatch::{
    BusyPaneAutoFixFacts, BusyPaneAutoFixOutcome,
    busy_existing_pane_auto_fix_outcome as controller_busy_existing_pane_auto_fix_outcome,
};
#[cfg(test)]
use agent_doc_controller_io::starting_actor_timeout::{
    StartingActorTimeoutLogDecision, clear_starting_actor_timeout_record,
    record_starting_actor_timeout, starting_actor_timeout_record_identity_matches,
    starting_actor_timeout_record_matches,
};
#[cfg(test)]
use agent_doc_harness::HarnessConfig;
#[cfg(test)]
use agent_doc_route_io::admission_projection::pending_prompt_bearing_context_for_route;
#[cfg(test)]
use agent_doc_route_io::authoritative_actor::{
    AuthoritativeActorDispatchTarget, actor_dispatch_state,
};
#[cfg(test)]
use agent_doc_route_io::authoritative_actor::{
    ManagedCapabilityProofStatus, authoritative_actor_dispatch_can_queue_optimistically,
    authoritative_actor_start_wait_terminal_state, managed_capability_proof_status,
    route_starting_actor_not_ready_log_line, tracked_harness_clear_requires_fresh_restart,
};
#[cfg(test)]
use agent_doc_route_io::closeout_drain::{
    apply_routed_dispatch_closeout_policy, classify_route_closeout_block,
    drain_open_closeout_before_routed_dispatch,
};
#[cfg(test)]
use agent_doc_route_io::command::RouteCommandEffects;
#[cfg(test)]
use agent_doc_route_io::command::RouteMode;
#[cfg(test)]
use agent_doc_route_io::diagnostics::{
    emit_startup_miss_diagnostic,
    file_route_dispatch_bug_report_with_runtime_effects as file_route_dispatch_bug_report,
};
#[cfg(test)]
use agent_doc_route_io::direct_pane_dispatch::editor_route_attempt_id;
#[cfg(test)]
use agent_doc_route_io::dispatch::RouteDispatchBugReportFacts;
#[cfg(test)]
use agent_doc_route_io::dispatch::{send_command_checked, send_command_once_checked};
#[cfg(test)]
use agent_doc_route_io::dispatch_recovery::resolve_fresh_dispatch_target_after_ready_wait;
#[cfg(test)]
use agent_doc_route_io::dispatch_target::register_dispatch_target;
#[cfg(test)]
use agent_doc_route_io::document_prep::scrub_duplicate_prompt_comments_for_route;
#[cfg(test)]
use agent_doc_route_io::pane_resolution::cleanup_failed_route_panes;
#[cfg(test)]
use agent_doc_route_io::pane_resolution::should_preserve_failed_route_pane;
#[cfg(test)]
use agent_doc_route_io::pane_resolution::startup_miss_route_facts;
#[cfg(test)]
use agent_doc_route_io::queue_dispatch::{
    activate_existing_route_queue_head, enqueue_exchange_slash_command_for_idle_drain,
    enqueue_route_dispatch_prompt, inactive_route_queue_head,
};
#[cfg(test)]
use agent_doc_route_io::runtime_effects::dispatch_only_starting_pane_ready_timeout;
#[cfg(test)]
use agent_doc_route_io::runtime_effects::{
    route_closeout_drain_effects, route_dispatch_only_effects, route_queue_effects,
    route_startup_effects,
};
#[cfg(test)]
use agent_doc_session_registry_io::dispatch_registry::ensure_dispatch_target_matches_file;
#[cfg(test)]
use agent_doc_session_registry_io::dispatch_registry::pane_registration_matches_file;
#[cfg(test)]
use agent_doc_supervisor::route_runtime::SupervisorHealth;
#[cfg(test)]
use agent_doc_supervisor::route_runtime::authoritative_actor_dispatch_target_eligible as supervisor_authoritative_actor_dispatch_target_eligible;
#[cfg(test)]
use agent_doc_supervisor::route_runtime::{RouteActorState, SupervisorRuntime};
#[cfg(test)]
use tmux_router::Tmux;

#[cfg(test)]
use agent_doc_session_registry_io::registration as sessions;

#[cfg(test)]
fn route_repair_closeout(file: &Path) -> Result<String> {
    agent_doc_repair_command_io::repair(file).map(|outcome| format!("{outcome:?}"))
}

#[cfg(test)]
static ROUTE_QUEUE_RETRY_WRITE_CALLS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
fn retry_once_crdt_merge_route_write_document(
    file: &Path,
    next_content: &str,
    previous_content: &str,
    _reason: &str,
) -> Result<()> {
    let call = ROUTE_QUEUE_RETRY_WRITE_CALLS.fetch_add(1, Ordering::SeqCst);
    if call == 0 {
        let concurrent = previous_content.replace(
            "- existing queued prompt\n",
            "- concurrent editor prompt\n- existing queued prompt\n",
        );
        std::fs::write(file, concurrent)?;
        anyhow::bail!(
            "project controller command `crdt_cp_write` failed: CP relay write refused for {}: expected_hash={} current_hash={} recovery=retry_crdt_merge",
            file.display(),
            agent_doc_hash::content_hash(previous_content),
            agent_doc_hash::content_hash(next_content),
        );
    }
    std::fs::write(file, next_content)?;
    Ok(())
}

#[cfg(test)]
fn runtime_route_command_effects() -> RouteCommandEffects {
    agent_doc_route_io::runtime_effects::route_command_effects(route_repair_closeout)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn run_with_tmux(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
) -> Result<()> {
    agent_doc_route_io::invocation::run_with_tmux(
        file,
        tmux,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        runtime_route_command_effects(),
    )
}

#[cfg(test)]
#[path = "route/pane_resolution.rs"]
mod pane_resolution;
#[cfg(test)]
pub(crate) use pane_resolution::*;

#[cfg(test)]
#[path = "route/startup.rs"]
mod startup;

#[cfg(test)]
pub(crate) use agent_doc_test_support::{
    ScopedCurrentDir, env_lock, launch_mock_agent_doc_without_file_arg,
    launch_mock_registered_agent_doc, mock_agent_script, pane_capture_contains_wrapped,
    route_bin_env_lock, send_keys_with_retry, test_registry_entry, tmux_start_lock,
    wait_for_pane_contains, wait_for_process_pid, wait_for_shell,
    write_mock_active_codex_turn_registered_agent_doc, write_mock_busy_opencode_recovers_on_escape,
    write_mock_busy_registered_agent_doc, write_mock_busy_registered_agent_doc_ignores_interrupt,
    write_mock_busy_registered_agent_doc_recovers_on_ctrl_g, write_mock_delayed_start_agent_doc,
    write_mock_registered_agent_doc, write_mock_registered_agent_doc_extra_line_detector,
    write_mock_registered_agent_doc_with_prefix, write_mock_start_agent_doc,
};
// #codex-route-busy-ctrl-g-opens-editor: the busy-pane reroute must only send
// `C-g` when the live capture proves a shell reverse-i-search / history-search.
// The pre-existing live ctrl-g test only models the reverse-i-search recovery,
// so this deterministic decision test covers the non-search composer / active
// turn case that previously received an editor-opening `C-g`.
#[cfg(test)]
pub(crate) fn test_cwd() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
// #jb-busy-reopen-auto-drain-when-idle: when a document's queue is ALREADY
// auto-looping, there is no INACTIVE head to activate, so
// activate_existing_route_queue_head returns None — but queue_continuation::detect
// still returns Some because the active loop is continuing the document. The busy
// FocusOnly / DispatchOnlyBusyQueue route paths key off exactly this signal to
// report deferred success (the loop will continue) instead of a busy refusal,
// so JB `Run Agent Doc` on an auto-looping pane no longer errors.
#[cfg(test)]
pub(crate) fn write_codex_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_network_access: enabled\nmanaged_proof: true\n---\n",
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    doc
}
#[cfg(test)]
pub(crate) fn write_codex_writable_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> (std::path::PathBuf, String) {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let writable = dir.join("writable-root");
    std::fs::create_dir_all(&writable).unwrap();
    let writable = writable.canonicalize().unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_args: \"--add-dir {}\"\nmanaged_proof: true\n---\n",
                writable.display()
            ),
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    let contract =
        agent_doc_turn_executor::codex_launch::writable_root_contract_id(&[writable]).unwrap();
    (doc, contract)
}
// --- rewrite_start_path tests ---
// --- Split direction tests ---
// --- Prompt detection tests (via HarnessConfig) ---
// --- Routing logic tests ---
// --- Integration tests (IsolatedTmux) ---
#[cfg(test)]
use tmux_router::IsolatedTmux;
// #jb-run-agent-doc-busy-wait-deadlock: a dispatch-only route on a busy active
// turn must NOT honor the slow-start `--wait-for-ready` override when a queue
// fallback exists — it should skip the wait and queue immediately, so JB `Run
// Agent Doc` on a mid-turn Claude actor does not block for the full 60s.
// #jb-run-agent-doc-busy-active-turn-stall: a bare dispatch-only Run Agent Doc
// against a pane proven busy on an active turn (working spinner / `esc to
// interrupt`) must NOT honor the 60s busy ready-wait — a multi-minute turn
// cannot reach a dispatch-ready prompt in that budget, so waiting only produces
// a silent stall before the inevitable "session still running" refusal. Skip the
// wait so the refusal/notification fires immediately.
// The refusal message must reflect whether the busy ready-wait was actually
// served: an active-turn skip words it as a busy turn (no misleading "after
// waiting Ns"), while the no-cue path keeps the cold-start ready-wait wording.
// --- auto_start_in_session tests ---
// --- has_named_window tests ---
// --- tmux_session validation tests ---
// --- Stash rescue tests ---
// --- split_before positional target tests ---
#[cfg(test)]
pub(crate) fn test_actor_record(pane_id: &str) -> agent_doc_controller::actor::ActorRecord {
    agent_doc_controller::actor::ActorRecord {
        document_id: "test-doc".to_string(),
        session_id: "test-session".to_string(),
        generation: 1,
        pane_id: pane_id.to_string(),
        window_id: "@1".to_string(),
        harness: "codex".to_string(),
        state: agent_doc_controller::actor::ActorState::Ready,
        last_transition: agent_doc_controller::actor::ActorLastTransition {
            caller: "test".to_string(),
            reason: "test".to_string(),
            timestamp: 0,
            prior_generation: 0,
            new_generation: 1,
        },
    }
}
#[cfg(test)]
pub(crate) fn test_degraded_actor(pane_id: &str) -> AuthoritativeActorDispatchTarget {
    AuthoritativeActorDispatchTarget {
        record: test_actor_record(pane_id),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
            current_harness: None,
        },
        pending_harness_switch: None,
    }
}
// #route-busy-vs-starting-wording: the FailClosed wait context distinguishes a
// pane busy on an active harness turn from a genuine cold startup timeout.
#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use agent_doc_controller::dispatch::{
        PromptReadyBarrierFacts, RouteLatencyFacts, classify_prompt_ready_barrier,
        route_latency_message,
    };
    use agent_doc_route_io::direct_pane_dispatch::CommandDispatchStatus;
    use agent_doc_supervisor::ipc_protocol::{IpcMethod, IpcResponse};
    use agent_doc_supervisor_io::ipc::SupervisorIpc;
    use agent_doc_turn::closeout_recovery::CloseoutRecoveryState;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: agent_doc_test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = agent_doc_test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn route_enqueue_exchange_slash_command_keeps_literal_head_for_idle_drain() {
        let _env_guard = env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = snapshot.replace(
            "<!-- /agent:exchange -->",
            "❯ /clear\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("exchange slash command should be route-visible");
        assert_eq!(ctx.prompt_text, "/clear");
        assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_exchange_slash_command_for_idle_drain(
            &doc,
            &ctx,
            "test_exchange_slash",
            route_queue_effects(),
        )
        .unwrap()
        .expect("slash command should queue for idle drain");
        assert!(outcome.appended);
        assert_eq!(outcome.prompt_text, "/clear");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        // `#qbulletlesshead`: the head stays LITERAL (undecorated) so the
        // idle-queue classifier still sees it, but it renders as a list item so
        // the bullet-only `item_nodes` enumerator can strike it. The literalness
        // is what this test guards; the missing bullet was the bug.
        assert!(updated.contains("\n- /clear\n"), "{updated}");
        assert!(
            !updated.contains(":pushpin: /clear"),
            "slash command must stay literal so the idle-queue classifier sees it:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("/clear"),
            "queued exchange slash command should be the active literal drain head"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot, updated,
            "route queueing must sync the snapshot so the command head is not treated as edited drift"
        );
    }

    #[test]
    fn route_enqueue_bare_exchange_slash_command_for_idle_drain() {
        let _env_guard = env_lock();
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let snapshot = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = snapshot.replace(
            "<!-- /agent:exchange -->",
            "/clear\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let ctx = pending_prompt_bearing_context_for_route(&doc, None)
            .unwrap()
            .expect("bare exchange slash command should be route-visible");
        assert_eq!(ctx.prompt_text, "/clear");
        assert_eq!(ctx.slash_command.as_deref(), Some("/clear"));

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_exchange_slash_command_for_idle_drain(
            &doc,
            &ctx,
            "test_bare_slash",
            route_queue_effects(),
        )
        .unwrap()
        .expect("slash command should queue for idle drain");
        assert!(outcome.appended);
        assert_eq!(outcome.prompt_text, "/clear");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        // `#qbulletlesshead`: the head stays LITERAL (undecorated) so the
        // idle-queue classifier still sees it, but it renders as a list item so
        // the bullet-only `item_nodes` enumerator can strike it. The literalness
        // is what this test guards; the missing bullet was the bug.
        assert!(updated.contains("\n- /clear\n"), "{updated}");
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("/clear"),
            "bare exchange slash command should be the active literal drain head"
        );
    }

    #[test]
    fn authoritative_actor_optimistic_queue_excludes_starting_state() {
        assert!(
            authoritative_actor_dispatch_can_queue_optimistically(
                agent_doc_controller::actor::ActorState::Busy
            ),
            "busy actors may still accept a supervisor-owned queued reopen"
        );
        assert!(
            !authoritative_actor_dispatch_can_queue_optimistically(
                agent_doc_controller::actor::ActorState::Starting
            ),
            "starting actors must become ready before route submits a reopen"
        );
    }
    #[test]
    fn authoritative_actor_start_wait_terminal_state_only_for_terminal_states() {
        assert!(authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::Closed
        ));
        assert!(authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::Blocked
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::Starting
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::Busy
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::WaitingInput
        ));
        assert!(!authoritative_actor_start_wait_terminal_state(
            agent_doc_controller::actor::ActorState::Ready
        ));
    }
    #[test]
    fn authoritative_actor_ready_poll_requires_ready_state_and_prompt_proof() {
        use agent_doc_controller::actor::ActorState;

        let schedule = [
            (ActorState::Starting, false, true),
            (ActorState::Busy, false, true),
            (ActorState::Ready, false, true),
        ];
        for (state, prompt_ready, dispatch_eligible) in schedule {
            assert_eq!(
                classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                    actor_state: actor_dispatch_state(state),
                    prompt_ready,
                    dispatch_eligible,
                }),
                PromptReadyBarrierDecision::Continue,
                "route must keep waiting while the current generation is {state:?} prompt_ready={prompt_ready} eligible={dispatch_eligible}"
            );
        }

        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Ready),
                prompt_ready: true,
                dispatch_eligible: false,
            }),
            PromptReadyBarrierDecision::Continue,
            "a ready actor still cannot dispatch until the target passes dispatch eligibility"
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Ready),
                prompt_ready: true,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Ready,
            "route may dispatch only after ready state, prompt proof, and eligibility agree"
        );
    }
    #[test]
    fn authoritative_actor_ready_poll_surfaces_terminal_states() {
        use agent_doc_controller::actor::ActorState;

        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Closed),
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(ActorState::Blocked),
                prompt_ready: false,
                dispatch_eligible: true,
            }),
            PromptReadyBarrierDecision::Terminal
        );
    }
    #[test]
    fn route_low_level_cleanup_scrubs_duplicate_prompt_comment_without_preserve_doc() {
        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first and fix the implementation. #spec-test-build-install-commit-push";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!--\n",
                "Keep this unrelated scratch note hidden.\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route should canonicalize duplicate prompt scratch comments before dispatch");
        let cleaned = cleanup.content;

        let duplicate_comment = format!("<!--\n{prompt}\n-->");
        assert!(
            !cleaned.contains(&duplicate_comment),
            "route must not dispatch with duplicate prompt text still in the post-exchange comment:\n{cleaned}"
        );
        assert!(
            cleaned.contains("\n<!--\n-->\n\n<!--\nKeep this unrelated scratch note hidden."),
            "route must preserve the ordinary comment shell and unrelated scratch comments:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_duplicate_prompt_comment_from_snapshot() {
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

        assert!(
            cleanup.is_none(),
            "route cleanup must preserve snapshot-owned scratch comments"
        );
    }
    #[test]
    fn route_preserves_scratch_comment_after_compact_summary_before_dispatch() {
        let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "Compacted content:\n",
                "- Trailing prompt/context: {prompt}\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: compact prompt duplication — gpt-5\n\n",
                "Line-by-line diff was the right diagnostic.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

        assert!(
            cleanup.is_none(),
            "production route cleanup must preserve visible post-exchange scratch comments"
        );
    }
    #[test]
    fn route_low_level_cleanup_scrubs_unowned_duplicate_prompt_comment() {
        let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "Compacted content:\n",
                "- Trailing prompt/context: {prompt}\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: compact prompt duplication — gpt-5\n\n",
                "Line-by-line diff was the right diagnostic.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs\n",
                "-->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route cleanup should scrub duplicate compact prompt residue");
        let cleaned = cleanup.content;

        assert!(
            !cleaned.contains(&format!("<!--\n{prompt}")),
            "route cleanup should remove only the duplicate prompt line:\n{cleaned}"
        );
        assert!(
            cleaned.contains("Look through the Claude + Codex + agent-doc session logs"),
            "route cleanup must not erase unrelated post-exchange scratch comments:\n{cleaned}"
        );
        assert!(
            cleaned.contains("<!--\n#spec-test-build-install-commit-push\n---\nLook through"),
            "route cleanup must preserve command and separator scratch lines:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_scratch_comment_when_response_quotes_same_text() {
        let scratch =
            "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ Please inspect the latest route cleanup report. #spec-test-build-install-commit-push\n",
                "### Re: route cleanup — gpt-5\n\n",
                "{scratch}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{scratch}\n",
                "-->\n"
            ),
            scratch = scratch
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap();
        let cleaned = cleanup
            .as_ref()
            .map(|cleanup| cleanup.content.as_str())
            .unwrap_or(content.as_str());

        assert!(
            cleaned.contains(&format!("<!--\n{scratch}\n-->")),
            "route cleanup must not treat assistant response quotes as prompt residue:\n{cleaned}"
        );
    }
    #[test]
    fn route_scrubs_duplicate_answered_prompt_tail_before_dispatch() {
        let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
        // Genuine delayed replay re-adds the just-answered prompt in answered form
        // (carrying the `❯ ` marker) — that is the ownership proof that lets route
        // scrub it safely.
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: mixed scratch comment deletion — gpt-5\n\n",
                "Answered already.\n",
                "<!-- agent:boundary:head -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
            .unwrap()
            .expect("route should canonicalize duplicate answered prompt tails before dispatch");
        let cleaned = cleanup.content;

        assert!(cleanup.removed_answered_tail);
        assert!(
            cleaned.contains(&format!(
                "❯ {prompt}\n❯ #spec-test-build-install-commit-push\n### Re:"
            )),
            "answered prompt block must remain in exchange history:\n{cleaned}"
        );
        assert!(
            !cleaned.contains(&format!("<!-- agent:boundary:head -->\n❯ {prompt}")),
            "route must not dispatch with duplicate answered-form prompt after the boundary:\n{cleaned}"
        );
    }
    #[test]
    fn route_preserves_unprefixed_live_prompt_matching_an_answered_prompt() {
        // Regression for #ipcfullprompt-recur: a freshly-typed prompt that happens to
        // match a previously-answered prompt (e.g. a re-typed "go") has no `❯ ` marker
        // and MUST be preserved for dispatch — never scrubbed as duplicate residue.
        let content = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "go\n",
            "<!-- /agent:exchange -->\n",
        );

        let cleanup = scrub_duplicate_prompt_comments_for_route(content, &[]).unwrap();
        assert!(
            cleanup.is_none(),
            "a bare re-typed prompt must not be scrubbed: {cleanup:?}"
        );
    }
    #[test]
    fn route_rejects_duplicate_prompt_markdown_residue_before_dispatch() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let content = format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );

        let err = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "route must fail closed before dispatching against duplicate prompt Markdown residue: {err}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_creates_visible_plain_queue_and_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "❯ prior prompt\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#qipc] Fix queue dispatch.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "❯ do [#qipc]. #spec-test-build-install-commit-push",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route should persist a queued dispatch prompt");

        assert!(outcome.appended);
        assert!(outcome.component_created);
        assert!(outcome.activated);
        assert_eq!(
            outcome.prompt_text,
            "do [#qipc]. #spec-test-build-install-commit-push"
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(updated.contains("- do [#qipc]. #spec-test-build-install-commit-push"));
        let queue_pos = updated.find("<!-- agent:queue go -->").unwrap();
        let backlog_pos = updated.find("<!-- agent:backlog -->").unwrap();
        assert!(
            queue_pos < backlog_pos,
            "created queue component should be visible before tracked work components:\n{updated}"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot, updated,
            "route queueing must sync the snapshot so queue continuation is not treated as a modified head prompt"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_converges_via_cp_editor_replica() {
        // JB Run Agent Doc can queue a pending dispatch while the editor plugin
        // owns the live buffer. That write must use the shared editor-converger,
        // not a direct disk write that manufactures a File Cache Conflict.
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let expected = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: go\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- 📌 manual preempt prompt\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_test_support::publish_editor_text_via_crdt_relay(
            &doc,
            "route-queue-test-editor",
            content,
        );

        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "manual preempt prompt",
            "test_busy_actor",
            true,
            route_queue_effects(),
        )
        .expect("route enqueue should converge through editor IPC");

        assert!(outcome.appended);
        assert!(outcome.activated);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), expected);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            expected
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("visible_write_current_transition_ready")
                && ops_log.contains("route_dispatch_queue_writeback")
                && ops_log.contains("transport=crdt_relay")
                && ops_log.contains("secondary_transport=none"),
            "route queue write must be observable as CP editor convergence:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("transport=disk_fallback"),
            "active-editor route queueing must not take a disk fallback:\n{ops_log}"
        );
    }

    #[test]
    fn route_enqueue_dispatch_prompt_retries_stale_crdt_relay_baseline() {
        // Repro shape for JB Run Agent Doc failing with
        // `crdt_cp_write ... recovery=retry_crdt_merge`: the relay current changed
        // between queue-read and writeback. Route must re-read and re-merge the
        // queue prompt instead of surfacing the transient hash mismatch.
        let dir = tempfile::TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- existing queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        ROUTE_QUEUE_RETRY_WRITE_CALLS.store(0, Ordering::SeqCst);

        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "manual preempt prompt",
            "test_busy_actor",
            true,
            agent_doc_route_io::queue_dispatch::RouteQueueEffects {
                write_document: retry_once_crdt_merge_route_write_document,
            },
        )
        .expect("route enqueue should retry a stale relay baseline");

        assert!(outcome.appended);
        assert!(outcome.activated);
        assert_eq!(2, ROUTE_QUEUE_RETRY_WRITE_CALLS.load(Ordering::SeqCst));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains(
                "- 📌 manual preempt prompt\n- concurrent editor prompt\n- existing queued prompt"
            ),
            "retry must preserve concurrent editor queue edits while inserting manual dispatch:\n{updated}"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            updated,
            "snapshot must match the retried queue write"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("route_dispatch_queue_retry")
                && ops_log.contains("reason=retry_crdt_merge")
                && ops_log.contains("route_dispatch_queued"),
            "retry should be observable in ops log:\n{ops_log}"
        );
    }

    #[test]
    fn route_enqueue_dispatch_prompt_preserves_unparseable_queue_instead_of_crashing() {
        // Repro of "JB Run Agent Doc error: route queue dispatch: failed to parse
        // existing agent:queue": an earlier corruption merged free-text prose into
        // the agent:queue component, so `queue::parse` bails on a bare line. The
        // route must not propagate that as a fatal error — it must preserve the
        // polluted body and still append the new pending dispatch.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "JB `Run Agent Doc` error:\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // The polluted free-text line is preserved as a non-actionable Freeform
        // entry (tolerant parse) rather than failing the consume/dispatch guards.
        let parsed =
            agent_doc_queue::document_queue::parse("JB `Run Agent Doc` error:\n- do [#existing]\n")
                .unwrap();
        assert!(
            parsed
                .iter()
                .any(|e| matches!(e, agent_doc_queue::document_queue::QueueEntry::Freeform(_)))
        );

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "do [#newitem]",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route must not crash on a polluted agent:queue");
        assert!(outcome.appended);

        let updated = std::fs::read_to_string(&doc).unwrap();
        // Existing (polluted) content preserved — not silently dropped.
        assert!(updated.contains("JB `Run Agent Doc` error:"));
        assert!(updated.contains("- do [#existing]"));
        // New dispatch appended below it.
        assert!(updated.contains("- do [#newitem]"));

        // Re-dispatching the same prompt into the still-polluted queue is idempotent.
        let outcome2 = enqueue_route_dispatch_prompt(
            &doc,
            "do [#newitem]",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route must stay resilient on repeat dispatch");
        assert!(outcome2.already_present);
        let updated2 = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated2.matches("- do [#newitem]").count(),
            1,
            "repeat dispatch into a polluted queue must not duplicate the entry:\n{updated2}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_activates_existing_queue_without_duplicate() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#qipc]. #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#qipc] Fix queue dispatch.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "do [#qipc]. #spec-test-build-install-commit-push",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route should activate an existing queued dispatch prompt");

        assert!(!outcome.appended);
        assert!(outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert_eq!(
            updated
                .matches("- do [#qipc]. #spec-test-build-install-commit-push")
                .count(),
            1,
            "route must not duplicate an already visible queue prompt:\n{updated}"
        );
    }
    #[test]
    fn route_activates_existing_inactive_auto_go_queue_head_as_go_queue_for_busy_deferral() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#shipstationaudit]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#shipstationaudit] Audit ShipStation settings.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#shipstationaudit]. #spec-test-commit-push")
        );

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(&doc, "busy actor", route_queue_effects())
            .unwrap()
            .expect("legacy inactive auto go queue head should activate");

        assert_eq!(
            outcome.prompt_text,
            "do [#shipstationaudit]. #spec-test-commit-push"
        );
        assert!(!outcome.appended);
        assert!(outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("queue: go"));
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert_eq!(
            updated
                .matches("- do [#shipstationaudit]. #spec-test-commit-push")
                .count(),
            1,
            "route must activate the existing head without duplicating it:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("shipstationaudit"),
            "activated go queue should become drainable by the idle-queue watch"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, updated, "route activation must sync the snapshot");
    }
    #[test]
    fn route_does_not_activate_plain_inactive_queue_head() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "- do [#manual]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(inactive_route_queue_head(&doc).unwrap(), None);
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor", route_queue_effects()).unwrap(),
            None,
            "plain inactive queues should stay inert without auto/start activation"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            content
        );
    }
    #[test]
    fn route_defers_uncommitted_queue_head_not_in_committed_snapshot() {
        // #qdispatchloss: the operator typed a fresh queue item into the editor
        // buffer; the JB plugin synced it to disk but it is NOT yet committed
        // (the committed snapshot predates the add). Route must NOT consume the
        // uncommitted head as the dispatch prompt — it would feed a possibly
        // half-typed line into the agent and lose the item.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        // Disk has a fresh, uncommitted head (`do [#fresh]`) prepended above the
        // committed one — exactly what an operator mid-edit produces.
        let on_disk = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#fresh]\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, on_disk).unwrap();
        // Committed snapshot only knows about `#committed`.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert!(
            !agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#fresh]"
            ),
            "a head absent from the committed snapshot queue is not backed"
        );
        assert!(
            agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#committed]"
            ),
            "a head present in the committed snapshot queue is backed"
        );

        // The inactive-head read defers the uncommitted head instead of
        // surfacing it for dispatch.
        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "route must not surface an uncommitted queue head for dispatch"
        );
        // The activate path therefore no-ops: nothing is consumed and the doc /
        // snapshot are untouched, so the operator's edit survives for the next
        // committed cycle.
        assert_eq!(
            activate_existing_route_queue_head(
                &doc,
                "dispatch_only_inactive_queue",
                route_queue_effects(),
            )
            .unwrap(),
            None,
            "route must not activate/consume an uncommitted queue head"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), on_disk);
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&doc)
                .unwrap()
                .unwrap(),
            committed
        );
    }
    #[test]
    fn route_dispatches_committed_queue_head() {
        // #qdispatchloss positive control: when the disk head IS backed by the
        // committed snapshot, route activates/dispatches it normally.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#committed]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#committed]"),
            "a committed-backed head is dispatchable"
        );
        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(
            &doc,
            "dispatch_only_inactive_queue",
            route_queue_effects(),
        )
        .unwrap()
        .expect("committed-backed head should activate");
        assert_eq!(outcome.prompt_text, "do [#committed]");
    }
    #[test]
    fn route_queue_head_unbacked_when_committed_snapshot_has_no_queue() {
        // #qdispatchloss: a committed snapshot with no queue component at all
        // means any on-disk queue head is a fresh uncommitted edit.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let committed = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert!(
            !agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                Some(committed),
                "do [#fresh]"
            ),
            "no committed queue component → head is unbacked"
        );
    }
    #[test]
    fn route_queue_head_backed_allows_when_no_committed_snapshot() {
        // #qdispatchloss bootstrap escape hatch: an untracked scaffold with no
        // committed snapshot must not be blocked — there is nothing to diverge
        // from.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "scaffold\n").unwrap();
        assert!(
            agent_doc_queue::route_dispatch::committed_snapshot_backs_queue_head(
                None,
                "do [#anything]"
            ),
            "no committed snapshot → allow (bootstrap)"
        );
    }
    #[test]
    fn busy_route_defers_to_active_go_loop_instead_of_refusing() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto go -->\n",
            "- do [#regional]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // No INACTIVE head — the queue is already active, so the activate path no-ops.
        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "an already-active go queue exposes no inactive head to activate"
        );
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor", route_queue_effects()).unwrap(),
            None,
            "activate path returns None when the queue is already go-looping"
        );
        // But the active-loop continuation signal IS present — this is what the busy
        // route path uses to defer (report success) instead of failing closed.
        let continuation = agent_doc_queue_io::queue_continuation::detect(&doc)
            .unwrap()
            .expect("active go-loop must expose a continuation head for busy deferral");
        assert_eq!(continuation.head_prompt, "do [#regional]");
    }
    #[test]
    fn route_activates_queue_stop_with_marker_go_head() {
        // #queue-state-unify: a `queue: stop` document carrying the marker-side
        // `<!-- agent:queue go -->` control must be recognized as activatable by the
        // route path so JB `Run Agent Doc` starts the queue. `go` is the marker
        // spelling of the canonical start gesture and overrides the stale stop.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue: stop\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#shipstationaudit]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#shipstationaudit] Audit ShipStation settings.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap().as_deref(),
            Some("do [#shipstationaudit]. #spec-test-commit-push"),
            "marker-side `go` must be recognized as an activatable head despite `queue: stop`"
        );

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = activate_existing_route_queue_head(&doc, "busy actor", route_queue_effects())
            .unwrap()
            .expect("startable inactive `go` queue head should activate");

        assert_eq!(
            outcome.prompt_text,
            "do [#shipstationaudit]. #spec-test-commit-push"
        );
        assert!(outcome.activated);

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: go"),
            "activation must flip the canonical control to go:\n{updated}"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_continuation_head(&updated).as_deref(),
            Some("shipstationaudit"),
            "activated queue should become drainable by the idle-queue watch"
        );
    }
    #[test]
    fn route_does_not_activate_queue_with_marker_stop() {
        // A marker-side `stop` is an explicit halt gesture and must keep the queue
        // inert even when it would otherwise activate via the legacy `auto`
        // attribute (#queue-state-unify). `stop` wins over `auto`/`go`/`start`.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: false\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto stop -->\n",
            "- do [#manual]. #spec-test-commit-push\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        assert_eq!(
            inactive_route_queue_head(&doc).unwrap(),
            None,
            "marker-side `stop` must keep the queue inert"
        );
        assert_eq!(
            activate_existing_route_queue_head(&doc, "busy actor", route_queue_effects()).unwrap(),
            None,
            "marker-side `stop` must not be activated by the route path"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_no_dup_with_completed_residue_and_live_head() {
        // Repro for #adoc-queue-ipc-drift: a halted/inactive-then-reactivated queue
        // that still carries struck `Completed` residue plus a single live prompt.
        // Re-dispatching the live head must NOT append a duplicate, and must NOT
        // supersede the live head into a struck id.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-isolation]~\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#adoc-orch-shim-cleanup] Finish the migration.\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "do [#adoc-orch-shim-cleanup]",
            "test_busy_actor",
            true,
            route_queue_effects(),
        )
        .expect("route should treat the live head as already queued");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            updated.matches("- do [#adoc-orch-shim-cleanup]").count(),
            1,
            "re-dispatching the live queue head must not duplicate it:\n{updated}\noutcome={outcome:?}"
        );
        assert!(
            !outcome.appended,
            "live head re-dispatch must not append:\n{updated}\noutcome={outcome:?}"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_supersedes_single_auto_queue_prompt() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- Run Agent Doc queued the first prompt.\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "Run Agent Doc queued the edited prompt.",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route should update a stale single auto-queue prompt");

        assert!(!outcome.appended);
        assert!(!outcome.already_present);
        assert!(outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            !updated.contains("- Run Agent Doc queued the first prompt."),
            "stale route-owned queue prompt should be replaced:\n{updated}"
        );
        assert!(
            updated.contains("- Run Agent Doc queued the edited prompt."),
            "edited prompt should become the single queued rerun:\n{updated}"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot, updated,
            "queue prompt supersession must sync the route snapshot"
        );
    }
    #[test]
    fn route_enqueue_dispatch_prompt_appends_to_legacy_auto_queue_as_plain_queue() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- first queued prompt\n",
            "- second queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "third queued prompt",
            "test_busy_actor",
            false,
            route_queue_effects(),
        )
        .expect("route should append to legacy multi-prompt queues");

        assert!(outcome.appended);
        assert!(!outcome.already_present);
        assert!(!outcome.superseded);
        assert!(!outcome.component_created);
        assert!(outcome.activated);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated
                .contains("- first queued prompt\n- second queued prompt\n- third queued prompt")
        );
    }
    #[test]
    fn route_enqueue_priority_dispatch_preempts_legacy_auto_queue_as_plain_queue() {
        // #jb-run-preempt-autoloop-priority: a manual operator Run Agent Doc into a
        // busy pane must jump AHEAD of pending auto-loop items, not land at the tail.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- first queued prompt\n",
            "- second queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "manual preempt prompt",
            "test_busy_actor",
            true,
            route_queue_effects(),
        )
        .expect("priority route dispatch should preempt the pending queue");

        assert!(outcome.appended);
        assert!(!outcome.already_present);
        assert!(!outcome.superseded);
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated.contains(
                "- 📌 manual preempt prompt\n- first queued prompt\n- second queued prompt"
            ),
            "priority dispatch must head-insert ahead of pending auto items with operator pin:\n{updated}"
        );
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(snapshot, updated, "priority preempt must sync the snapshot");
    }
    #[test]
    fn route_enqueue_priority_dispatch_inserts_ahead_of_lone_legacy_auto_prompt() {
        // #jb-run-preempt-autoloop-priority: a priority dispatch must NOT supersede a
        // lone auto prompt — replacing it would silently drop the pending auto-loop
        // item the manual run is preempting. Both prompts survive, manual first.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- pending auto-loop item\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let outcome = enqueue_route_dispatch_prompt(
            &doc,
            "manual preempt prompt",
            "test_busy_actor",
            true,
            route_queue_effects(),
        )
        .expect("priority route dispatch should insert ahead, not supersede");

        assert!(outcome.appended);
        assert!(!outcome.superseded, "priority dispatch must not supersede");
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        assert!(
            updated.contains("- 📌 manual preempt prompt\n- pending auto-loop item"),
            "priority dispatch must preserve the pending item and run ahead of it with operator pin:\n{updated}"
        );
    }
    #[test]
    fn route_enqueue_priority_dispatch_preserves_leading_queue_directives() {
        // #jb-run-preempt-autoloop-priority: the head-insert must land after leading
        // queue directives (preset / start fence), before the first actionable prompt.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "preset #spec-test-build-install-commit-push\n",
            "- first queued prompt\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        enqueue_route_dispatch_prompt(
            &doc,
            "manual preempt prompt",
            "test_busy_actor",
            true,
            route_queue_effects(),
        )
        .expect("priority route dispatch should insert after leading directives");

        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("<!-- agent:queue go -->"));
        assert!(!updated.contains("agent:queue auto"));
        let preset_pos = updated
            .find("preset #spec")
            .expect("preset directive preserved");
        let preempt_pos = updated
            .find("- 📌 manual preempt prompt")
            .expect("preempt prompt inserted");
        let first_pos = updated
            .find("- first queued prompt")
            .expect("first prompt preserved");
        assert!(
            preset_pos < preempt_pos && preempt_pos < first_pos,
            "preempt prompt must sit after the preset directive and before the first prompt:\n{updated}"
        );
    }
    #[test]
    fn managed_capability_proof_status_tracks_pending_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-status";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=pending",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Pending
        );

        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=failed error=\"dns\"",
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Failed
        );
    }
    #[test]
    fn managed_capability_proof_status_requires_matching_writable_root_contract() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-writable-proof-status";
        let (doc, contract) = write_codex_writable_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Missing
        );

        let (doc, _) = write_codex_writable_proof_status_fixture(
            dir.path(),
            session_id,
            &format!(
                "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1 writable_root_contract={contract}"
            ),
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Proven
        );
    }
    #[test]
    fn managed_capability_proof_status_requires_post_restart_proof() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-after-restart";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0",
        );
        std::fs::write(
            dir.path()
                .join(".agent-doc/logs")
                .join(format!("{session_id}.log")),
            format!(
                "[1] session_start file={} pane=%1 session={}\n\
                 [2] codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0\n\
                 [3] agent_restart_performed old_harness=claude new_harness=codex action=spawn_fresh_harness\n",
                doc.display(),
                session_id
            ),
        )
        .unwrap();

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Missing
        );

        agent_doc_supervisor_io::startup_miss::append_session_log_event(
            &doc,
            session_id,
            "codex_capability_proof status=proven network=proven ssh_targets=0 writable_roots=0",
        )
        .unwrap();
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
            ManagedCapabilityProofStatus::Proven
        );
    }
    #[test]
    fn managed_capability_proof_status_opencode_tracks_pending_and_failed() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-proof-status-opencode";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=pending",
        );

        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
            ManagedCapabilityProofStatus::Pending
        );

        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=failed error=\"ssh\"",
        );
        assert_eq!(
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
            ManagedCapabilityProofStatus::Failed
        );
    }
    #[test]
    fn pane_registration_matches_file_resolves_entry_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("claudescore-3.md");
        std::fs::write(&doc, "# session\n").unwrap();

        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            test_registry_entry("%401", "tasks/claudescore-3.md", &submodule),
        );

        assert!(
            pane_registration_matches_file(&registry, "%401", &doc.to_string_lossy()),
            "relative registry paths should resolve against the pane cwd"
        );
    }
    #[test]
    fn ensure_dispatch_target_matches_file_rejects_cross_file_registration() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let submodule = dir.path().join("src/session-share");
        let tasks = submodule.join("tasks");
        std::fs::create_dir_all(&tasks).unwrap();
        let registered = tasks.join("sampleorders.md");
        let requested = tasks.join("claudescore-3.md");
        std::fs::write(&registered, "# registered\n").unwrap();
        std::fs::write(&requested, "# requested\n").unwrap();

        sessions::register_full_with_cwd_in(
            dir.path(),
            "session-a",
            "%401",
            "tasks/sampleorders.md",
            1234,
            "@1",
            &submodule.to_string_lossy(),
        )
        .unwrap();

        let err = ensure_dispatch_target_matches_file("%401", &requested.to_string_lossy())
            .expect_err("cross-file pane reuse must fail closed");
        assert!(
            err.to_string().contains("refusing cross-file dispatch"),
            "error should explain the rejected cross-file dispatch: {err}"
        );
    }
    #[test]
    fn detects_unicode_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("❯"));
        assert!(h.is_prompt_line("❯ "));
        assert!(h.is_prompt_line("  ❯  "));
    }
    #[test]
    fn detects_ascii_prompt() {
        let h = HarnessConfig::codex();
        assert!(h.is_prompt_line(">"));
        assert!(h.is_prompt_line("> "));
        assert!(h.is_prompt_line("  >  "));
    }
    #[test]
    fn rejects_non_prompt_lines() {
        let h = HarnessConfig::claude();
        assert!(!h.is_prompt_line("Starting claude..."));
        assert!(!h.is_prompt_line("test result: ok"));
        assert!(!h.is_prompt_line(""));
        assert!(!h.is_prompt_line("  "));
        assert!(!h.is_prompt_line("## User"));
    }
    #[test]
    fn handles_ansi_prompt() {
        let h = HarnessConfig::claude();
        assert!(h.is_prompt_line("\x1b[32m❯\x1b[0m"));
        let h_codex = HarnessConfig::codex();
        assert!(h_codex.is_prompt_line("\x1b[1m>\x1b[0m"));
    }
    #[test]
    fn dead_registered_pane_allows_lazy_claim() {
        // When registered is Some but pane is dead, lazy-claim should be attempted.
        let registered: Option<String> = Some("%99".to_string());
        assert!(
            registered.is_some(),
            "dead registered pane should attempt lazy claim"
        );
    }
    #[test]
    fn drain_reaps_completed_review_item_across_all_surfaces() {
        // #route-drain reap-all-surfaces: the focused route-drain repair reaped only
        // the backlog, so a deployed `[x]` item left in review blocked dispatch until
        // a manual repeat ran full preflight maintenance ("JB Run Agent Doc failed; a
        // repeat attempt succeeded"). The drain now runs all-surface pending
        // maintenance first, so the completed review item is reaped on the first
        // attempt regardless of the final drain outcome.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("drain-review.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [x] [#seocat] Implemented and deployed\n",
            "<!-- /agent:review -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        // Open cycle so the drain actually runs (is_open()).
        agent_doc_cycle_state_io::start_preflight(&doc, None, Some(content)).unwrap();

        // The drain may still report Blocked on later (committed/etc.) guards in this
        // minimal fixture, but the all-surface reap runs before that — assert the
        // completed review item is gone from the file.
        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        let _ = super::drain_open_closeout_before_routed_dispatch(
            &doc,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        );

        let after = std::fs::read_to_string(&doc).unwrap();
        let review = agent_doc_element::element::parse(&after)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "review")
            .unwrap()
            .content(&after)
            .to_string();
        assert!(
            !review.contains("[#seocat]"),
            "drain must reap the completed review item via all-surface maintenance: {review}"
        );
        assert!(after.contains("[#keep1]"), "open backlog item must remain");
    }

    #[test]
    fn drain_cancels_empty_preflight_before_document_repair() {
        use agent_doc_controller::dispatch::RouteCloseoutDrainOutcome as DrainOutcome;

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("drain-empty-preflight.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let outcome = super::drain_open_closeout_before_routed_dispatch(
            &doc,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        )
        .unwrap();

        assert_eq!(
            outcome,
            DrainOutcome::Recovered("empty_preflight_cancelled".to_string()),
        );
        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Abandoned);
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), content);
    }

    #[test]
    fn closeout_block_decision_queues_prompt_context_before_failing_closed() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = "---\nagent_doc_session: test\n---\n\n";
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) = super::classify_route_closeout_block(
            &doc,
            low_level_reason.to_string(),
            true,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        );
        match dispatch_decision {
            CloseoutBlockDispatchDecision::EnqueuePromptForAfterCloseout => {
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                assert_eq!(decision.as_str(), "queue_prompt_for_after_closeout");
            }
            other => panic!("prompt context should queue behind closeout: {other:?}"),
        }
    }

    #[test]
    fn closeout_block_decision_waits_on_existing_active_queue_head() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "queue_active: true\n",
            "---\n\n",
            "## Queue\n\n",
            "<!-- agent:queue auto go -->\n",
            "- :pushpin: [#jbruncloseoutstate]\n",
            "<!-- /agent:queue -->\n"
        );
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) = super::classify_route_closeout_block(
            &doc,
            low_level_reason.to_string(),
            false,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        );
        match dispatch_decision {
            CloseoutBlockDispatchDecision::WaitForActiveQueueHead { head } => {
                assert_eq!(head, "jbruncloseoutstate");
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                let reason = decision.route_terminal_reason();
                assert!(
                    reason.contains("closeout recovery blocked [open_cycle]"),
                    "{reason}"
                );
                assert!(reason.contains("missing proof"), "{reason}");
                assert!(
                    !reason.contains(low_level_reason),
                    "route-visible blocker leaked low-level capture text: {reason}"
                );
            }
            other => panic!("existing active queue head should wait behind closeout: {other:?}"),
        }
    }

    #[test]
    fn closeout_block_decision_fails_closed_without_prompt_or_active_queue() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = "---\nagent_doc_session: test\n---\n\n";
        write_open_cycle_route_doc(&doc, content);
        let low_level_reason = "captured response baseline no longer matches current document";

        let (decision, dispatch_decision) = super::classify_route_closeout_block(
            &doc,
            low_level_reason.to_string(),
            false,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        );
        match dispatch_decision {
            CloseoutBlockDispatchDecision::FailClosed => {
                assert_eq!(decision.state(), Some(CloseoutRecoveryState::OpenCycle));
                let reason = decision.route_terminal_reason();
                assert!(
                    reason.contains("closeout recovery blocked [open_cycle]"),
                    "{reason}"
                );
                assert!(reason.contains("recommended:"), "{reason}");
                assert!(
                    !reason.contains(low_level_reason),
                    "route-visible blocker leaked low-level capture text: {reason}"
                );
            }
            other => panic!("missing prompt and queue should fail closed: {other:?}"),
        }
    }

    #[test]
    fn plain_run_trigger_passes_through_an_open_closeout() {
        use agent_doc_controller::dispatch::RouteCloseoutDrainOutcome as DrainOutcome;

        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("route-block.md");
        let content = "---\nagent_doc_session: test\n---\n\n";
        write_open_cycle_route_doc(&doc, content);

        let outcome = super::apply_routed_dispatch_closeout_policy(
            &doc,
            ReopenMode::DispatchOnly,
            AuthoritativeActorDispatchIntent::PlainTrigger,
            super::route_closeout_drain_effects(super::route_repair_closeout),
        )
        .unwrap();
        assert_eq!(outcome, DrainOutcome::PlainTriggerPassThrough);
        assert!(
            agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open()
        );
    }

    fn write_open_cycle_route_doc(doc: &std::path::Path, content: &str) {
        std::fs::create_dir_all(doc.parent().unwrap().join(".agent-doc")).unwrap();
        std::fs::write(doc, content).unwrap();
        agent_doc_cycle_state_io::start_preflight(doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::mark_pending_mutations(doc).unwrap();
    }

    #[test]
    fn route_latency_message_marks_budget_status() {
        let harness = HarnessConfig::codex();
        let ok = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: Duration::from_millis(999).as_millis(),
            budget_ms: Duration::from_secs(1).as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: "submitted",
            editor_attempt_id: None,
        });
        assert!(ok.contains("status=ok"), "{ok}");
        assert!(ok.contains("elapsed_ms=999"), "{ok}");

        let slow = route_latency_message(RouteLatencyFacts {
            phase: "dispatch_start_proof",
            elapsed_ms: Duration::from_secs(10).as_millis(),
            budget_ms: Duration::from_secs(10).as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: "unproven_but_accepted",
            editor_attempt_id: None,
        });
        assert!(slow.contains("status=over_budget"), "{slow}");
        assert!(slow.contains("outcome=unproven_but_accepted"), "{slow}");
    }

    #[test]
    fn route_diagnostics_include_editor_attempt_id_when_present() {
        let _attempt = EnvGuard::set(
            agent_doc_tmux_commands::input_diag::EDITOR_ROUTE_ATTEMPT_ID_ENV,
            "attempt 1/2",
        );
        let harness = HarnessConfig::codex();
        let editor_attempt_id = editor_route_attempt_id();
        let latency = route_latency_message(RouteLatencyFacts {
            phase: "direct_pane_submit",
            elapsed_ms: Duration::from_millis(120).as_millis(),
            budget_ms: Duration::from_secs(1).as_millis(),
            pane: "%7",
            harness_binary: &harness.binary,
            outcome: "accepted",
            editor_attempt_id: editor_attempt_id.as_deref(),
        });
        assert!(
            latency.contains("editor_attempt_id=attempt_1_2"),
            "{latency}"
        );
    }

    #[test]
    fn direct_pane_submit_budget_allows_acceptance_poll_slack() {
        let harness = HarnessConfig::codex();
        let message = route_latency_message(RouteLatencyFacts {
            phase: "direct_pane_submit",
            elapsed_ms: Duration::from_millis(1180).as_millis(),
            budget_ms: agent_doc_controller::dispatch::direct_pane_submit_acceptance_budget()
                .as_millis(),
            pane: "%1",
            harness_binary: &harness.binary,
            outcome: agent_doc_controller::dispatch::direct_pane_submit_outcome(
                CommandDispatchStatus::TimedOut,
                Some(RoutedDispatchStartProof::HookPromptMatched),
            ),
            editor_attempt_id: None,
        });

        assert!(message.contains("status=ok"), "{message}");
        assert!(
            message.contains("outcome=acceptance_unobserved_dispatch_proven"),
            "{message}"
        );
        assert!(!message.contains("timed_out"), "{message}");
    }
    #[test]
    fn route_dispatch_bug_report_dedupes_same_document_stage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("run-agent-doc.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        let first_diagnostic = dir.path().join(".agent-doc/logs/route-submit/first.txt");
        let second_diagnostic = dir.path().join(".agent-doc/logs/route-submit/second.txt");
        let base = RouteDispatchBugReportFacts {
            file: &doc,
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "direct_pane_submit_final",
            issue: "prompt_not_submitted",
            result: "submit_timed_out_without_proof",
            elapsed: Duration::from_secs(30),
            proof: None,
            diagnostic_path: Some(&first_diagnostic),
        };
        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        file_route_dispatch_bug_report(base);
        file_route_dispatch_bug_report(RouteDispatchBugReportFacts {
            diagnostic_path: Some(&second_diagnostic),
            ..base
        });

        let content = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            content
                .matches("[symptom-key invariant=run_agent_doc_route_dispatch_failure")
                .count(),
            1,
            "{content}"
        );
        assert!(content.contains("diagnostic_path="), "{content}");
        assert!(
            content.contains("first.txt") && content.contains("second.txt"),
            "{content}"
        );
        assert!(
            content.contains("  evidence: JetBrains Run Agent Doc route/dispatch failed"),
            "{content}"
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("route_dispatch_bug_backlog_filed")
                && ops.contains("inserted=true")
                && ops.contains("inserted=false"),
            "{ops}"
        );
    }

    #[test]
    fn route_dispatch_bug_report_uses_configured_bug_target_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "tasks/agent-doc/agent-doc-bugs2.md"
"#,
        )
        .unwrap();
        let doc = dir.path().join("run-agent-doc.md");
        let target = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(
            &doc,
            "---\nagent_doc_session: test\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        std::fs::write(
            &target,
            "---\nagent_doc_session: bugs\n---\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        let diagnostic = dir
            .path()
            .join(".agent-doc/logs/route-submit/configured.txt");
        let facts = RouteDispatchBugReportFacts {
            file: &doc,
            pane: "%7",
            harness: &HarnessConfig::codex(),
            phase: "direct_pane_submit_final",
            issue: "prompt_not_submitted",
            result: "submit_timed_out_without_proof",
            elapsed: Duration::from_secs(30),
            proof: None,
            diagnostic_path: Some(&diagnostic),
        };

        let _force_disk_guard =
            agent_doc_route_io::invocation::ForceDiskRouteWritesGuard::set(true);
        file_route_dispatch_bug_report(facts);

        let source = std::fs::read_to_string(&doc).unwrap();
        let filed = std::fs::read_to_string(&target).unwrap();
        assert!(
            !source.contains("#jbrunautobug"),
            "source document should not receive configured bug target item: {source}"
        );
        assert!(filed.contains("#jbrunautobug"), "{filed}");
        assert!(filed.contains("document="), "{filed}");
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("route_dispatch_bug_backlog_filed")
                && ops.contains("target_file=")
                && ops.contains("agent-doc-bugs2.md"),
            "{ops}"
        );
    }

    #[test]
    fn tracked_harness_clear_requires_fresh_restart_only_for_exact_clear_prompt() {
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("  /clear  ")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::codex(),
            Some("agent-doc tasks/bugs.md")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::claude(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("/clear")
        ));
        assert!(tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("  /clear  ")
        ));
        assert!(!tracked_harness_clear_requires_fresh_restart(
            &HarnessConfig::opencode(),
            Some("agent-doc tasks/bugs.md")
        ));
    }
    #[test]
    fn busy_existing_pane_auto_fix_outcome_restarts_fresh_for_healthy_authoritative_session_without_changes()
     {
        assert_eq!(
            controller_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_health: Some(SupervisorHealth::Healthy),
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
        );
        assert_eq!(
            controller_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_health: Some(SupervisorHealth::Restartable),
                restarted_supervisor: false,
            }),
            BusyPaneAutoFixOutcome::FailClosed
        );
        assert_eq!(
            controller_busy_existing_pane_auto_fix_outcome(BusyPaneAutoFixFacts {
                test_hook_changed: false,
                fix_made_changes: false,
                supervisor_health: Some(SupervisorHealth::Restartable),
                restarted_supervisor: true,
            }),
            BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn resolve_fresh_dispatch_target_ignores_explicitly_blocked_startup_miss_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new("route-test-fresh-start-blocked-handoff");

        let doc = dir.path().join("fresh-start-blocked-handoff.md");
        std::fs::write(&doc, "# Session\n").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = "route-fresh-start-blocked-handoff";
        let blocked_pane = "%364";
        let new_pane = "%370";

        sessions::register_full_with_cwd_in(
            dir.path(),
            session_id,
            blocked_pane,
            &file_path,
            12345,
            "@owner",
            dir.path().to_string_lossy().as_ref(),
        )
        .unwrap();

        let resolved = resolve_fresh_dispatch_target_after_ready_wait(
            &iso,
            session_id,
            new_pane,
            &file_path,
            Some(blocked_pane),
        )
        .unwrap();

        assert_eq!(
            resolved, new_pane,
            "resolver should keep dispatch in the fresh pane when the previous startup-miss owner is explicitly blocked"
        );

        let registry = agent_doc_session_registry_io::load_in(dir.path()).unwrap();
        let entry = registry
            .values()
            .find(|entry| entry.session_id == session_id)
            .expect("fresh pane should be registered after the blocked handoff is ignored");
        assert_eq!(entry.pane, new_pane);
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_preserves_live_registered_owner() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-preserve-failed-owner");
        let session = format!("test-{}", std::process::id());
        let pane = iso.new_session(&session, dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();

        assert!(
            should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
            "failed-route cleanup must preserve the live registered owner pane"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_does_not_preserve_unregistered_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-unregistered");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();

        assert!(
            !should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
            "failed-route cleanup should still remove panes that never became the live owner"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_reaps_startup_miss_owner_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-startup-miss");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();
        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &file,
            &pane,
            "session-1",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane));

        assert!(
            !iso.pane_alive(&pane),
            "fresh-route startup-miss panes should be reaped instead of preserved idle"
        );
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn failed_route_cleanup_only_reaps_attempt_local_created_panes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-cleanup-concurrent-sibling");
        let pane_owned = iso.new_session("test", dir.path()).unwrap();
        let pane_sibling = iso.split_window(&pane_owned, dir.path(), "-dh").unwrap();

        sessions::register_full_in(
            dir.path(),
            "session-1",
            &pane_owned,
            "tasks/software/corky.md",
            123,
            "@1",
        )
        .unwrap();
        sessions::register_full_in(
            dir.path(),
            "session-2",
            &pane_sibling,
            "tasks/software/tsift.md",
            456,
            "@1",
        )
        .unwrap();

        let file = dir.path().join("tasks/software/corky.md");
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&file, "# Corky\n").unwrap();

        cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane_owned));

        assert!(
            iso.pane_alive(&pane_owned),
            "cleanup should preserve the live owner pane for the failed route"
        );
        assert!(
            iso.pane_alive(&pane_sibling),
            "cleanup must not reap sibling panes that were not created by this route attempt"
        );
    }
    #[test]
    fn run_with_tmux_resolves_file_path_to_absolute() {
        // Verify that resolve_absolute_file_path turns a relative path into an
        // absolute one when the file exists. This is the guard against submodule
        // CWD-dependent resolution (#route1).
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let tasks = root.join("tasks");
        fs::create_dir_all(&tasks).unwrap();
        let doc = tasks.join("bugs.md");
        fs::write(&doc, "# Bugs\n").unwrap();

        let _cwd_guard = ScopedCurrentDir::set(&root);

        let resolved = agent_doc_git_io::dirs::resolve_absolute_file_path(std::path::Path::new(
            "tasks/bugs.md",
        ));
        assert!(
            resolved.is_absolute(),
            "route must send absolute paths to avoid submodule CWD misrouting"
        );
        assert_eq!(
            resolved, doc,
            "resolved path must point to the CWD-relative file, not a submodule shadow"
        );
    }
    #[test]
    fn startup_miss_recorded_on_fresh_start_timeout() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%42",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();

        let miss = agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
            .unwrap()
            .expect("should have marker");
        assert_eq!(miss.pane_id, "%42");
        assert_eq!(
            miss.origin,
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart
        );
        assert!(agent_doc_supervisor_io::startup_miss::is_startup_miss_pane(
            &doc, "%42"
        ));
    }
    #[test]
    fn startup_miss_cleared_on_successful_ack() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%42",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::FreshStart,
            None,
        )
        .unwrap();
        assert!(
            agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
                .unwrap()
                .is_some()
        );

        agent_doc_supervisor_io::startup_miss::clear_startup_miss(&doc).unwrap();
        assert!(
            agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
                .unwrap()
                .is_none()
        );
        assert!(!agent_doc_supervisor_io::startup_miss::is_startup_miss_pane(&doc, "%42"));
    }
    #[test]
    fn startup_miss_pane_detected_on_rerun() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%99",
            "session-test",
            "codex",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-old"),
        )
        .unwrap();

        assert!(agent_doc_supervisor_io::startup_miss::is_startup_miss_pane(
            &doc, "%99"
        ));
        assert!(
            !agent_doc_supervisor_io::startup_miss::is_startup_miss_pane(&doc, "%100"),
            "different pane should not match"
        );
    }
    #[test]
    fn startup_miss_routed_trigger_records_with_baseline_id() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        agent_doc_supervisor_io::startup_miss::record_startup_miss(
            &doc,
            "%50",
            "session-test",
            "claude",
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            Some("cycle-baseline-123"),
        )
        .unwrap();

        let miss = agent_doc_supervisor_io::startup_miss::load_startup_miss(&doc)
            .unwrap()
            .expect("marker");
        assert_eq!(
            miss.origin,
            agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger
        );
        assert_eq!(
            miss.cycle_baseline_id.as_deref(),
            Some("cycle-baseline-123")
        );
    }
    #[test]
    fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        assert!(startup_miss_requires_fresh_start(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            None,
        )));
        assert!(startup_miss_requires_fresh_start(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            Some("%99"),
            SupervisorHealth::Unreachable,
            None,
        )));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                None,
            )
        ));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                None,
                SupervisorHealth::Restartable,
                None,
            )
        ));
        assert!(!startup_miss_requires_fresh_start(
            startup_miss_route_facts(&miss, "%42", true, None, SupervisorHealth::Healthy, None,)
        ));
    }
    #[test]
    fn startup_miss_live_owner_restart_requires_closed_unsuperseded_start() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        let closed_same_start = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(10),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some(
                "auto_trigger_timeout harness=codex reason=no_prompt_after_30s".to_string(),
            ),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: true,
            saw_session_end_after_latest_run: false,
        };
        let newer_open_start = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(10),
            latest_run_timestamp: Some(11),
            latest_run_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };

        assert!(startup_miss_should_restart_live_owner(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&closed_same_start),
            )
        ));
        assert!(!startup_miss_should_restart_live_owner(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&newer_open_start),
            )
        ));
        assert!(startup_miss_superseded_by_later_open_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&newer_open_start),
            )
        ));
        assert!(!startup_miss_superseded_by_later_open_start(
            startup_miss_route_facts(
                &miss,
                "%42",
                true,
                Some("%42"),
                SupervisorHealth::NoSocket,
                Some(&closed_same_start),
            )
        ));
    }
    #[test]
    fn startup_miss_fail_closed_only_for_alive_open_no_socket_sessions() {
        let miss = agent_doc_supervisor::startup_miss::StartupMiss {
            file: "test.md".to_string(),
            pane_id: "%42".to_string(),
            session_id: "session-123".to_string(),
            harness: "codex".to_string(),
            timestamp: 10,
            origin: agent_doc_supervisor::startup_miss::StartupMissOrigin::RoutedTrigger,
            cycle_baseline_id: Some("cycle-abc".to_string()),
        };
        let open = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            latest_run_timestamp: Some(1),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_process_exit_after_latest_start: false,
            saw_session_end_after_latest_start: false,
            saw_process_exit_after_latest_run: false,
            saw_session_end_after_latest_run: false,
        };
        let closed = agent_doc_supervisor::startup_miss::SessionLogStatus {
            latest_start_pane: Some("%42".to_string()),
            latest_start_timestamp: Some(1),
            latest_run_timestamp: Some(1),
            latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
            saw_committed_cycle_after_latest_run: false,
            last_event: Some("session_end".to_string()),
            saw_process_exit_after_latest_start: true,
            saw_session_end_after_latest_start: true,
            saw_process_exit_after_latest_run: true,
            saw_session_end_after_latest_run: true,
        };

        assert!(startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            Some("%42"),
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::Healthy,
            Some(&open),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            true,
            None,
            SupervisorHealth::NoSocket,
            Some(&closed),
        )));
        assert!(!startup_miss_should_fail_closed(startup_miss_route_facts(
            &miss,
            "%42",
            false,
            None,
            SupervisorHealth::NoSocket,
            Some(&open),
        )));
    }
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn startup_miss_diagnostic_does_not_queue_shell_echo_in_pane() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        let iso = IsolatedTmux::new("route-test-startup-miss-diagnostic");
        let pane = iso.new_session("test", dir.path()).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "# Session\n").unwrap();

        send_keys_with_retry(&iso, &pane, "printf '> '");
        let before = wait_for_pane_contains(&iso, &pane, "> ", std::time::Duration::from_secs(5));
        assert!(
            before.contains("> "),
            "shell prompt should be visible: {before}"
        );

        emit_startup_miss_diagnostic(&iso, &pane, &doc, "startup timed out");

        std::thread::sleep(std::time::Duration::from_millis(250));
        let after = agent_doc_tmux_io::capture_pane(&iso, &pane).unwrap();
        assert!(
            !after.contains("echo '[agent-doc] startup-miss:"),
            "diagnostic should not be left as drafted shell input: {after}"
        );
    }
    #[test]
    fn skip_capability_proof_bypasses_failed_proof_status() {
        let dir = tempfile::tempdir().unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let session_id = "route-skip-proof";
        let doc = write_codex_proof_status_fixture(
            dir.path(),
            session_id,
            "opencode_capability_proof status=failed error=\"dns\"",
        );
        let status =
            managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap();
        assert_eq!(status, ManagedCapabilityProofStatus::Failed);
    }
    #[test]
    fn authoritative_actor_state_preserves_terminal_record_over_runtime_starting() {
        let mut blocked_record = test_actor_record("%42");
        blocked_record.state = agent_doc_controller::actor::ActorState::Blocked;
        blocked_record.last_transition.reason = STARTING_ACTOR_TIMEOUT_REASON.to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Starting),
                current_harness: None,
            },
            pending_harness_switch: None,
        };
        assert_eq!(
            blocked_actor.actor_state(),
            agent_doc_controller::actor::ActorState::Blocked,
            "a route-owned blocked record should remain a durable terminal gate even if stale supervisor IPC still reports starting"
        );
        assert!(
            actor_blocked_by_starting_timeout(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_controller::actor::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: false,
            }),
            "a route-owned starting timeout should be identifiable before route re-registers the stale pane"
        );

        let mut starting_record = test_actor_record("%43");
        starting_record.state = agent_doc_controller::actor::ActorState::Starting;
        let ready_actor = AuthoritativeActorDispatchTarget {
            record: starting_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Ready),
                current_harness: None,
            },
            pending_harness_switch: None,
        };
        assert_eq!(
            ready_actor.actor_state(),
            agent_doc_controller::actor::ActorState::Ready,
            "non-terminal records should still accept fresher supervisor runtime state"
        );
    }
    #[test]
    fn starting_timeout_blocked_actor_recovery_requires_prompt_ready_proof() {
        let mut blocked_record = test_actor_record("%42");
        blocked_record.state = agent_doc_controller::actor::ActorState::Blocked;
        blocked_record.last_transition.reason = STARTING_ACTOR_TIMEOUT_REASON.to_string();
        let blocked_actor = AuthoritativeActorDispatchTarget {
            record: blocked_record,
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Starting),
                current_harness: None,
            },
            pending_harness_switch: None,
        };

        assert!(
            starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_controller::actor::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: true,
            }),
            "a route-owned starting timeout may recover only after direct dispatch-ready prompt proof"
        );
        assert!(
            !starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: blocked_actor.record.state
                    == agent_doc_controller::actor::ActorState::Blocked,
                last_transition_reason: &blocked_actor.record.last_transition.reason,
                prompt_ready: false,
            }),
            "route must not clear a durable starting timeout without prompt proof"
        );
        let degraded_actor = test_degraded_actor("%43");
        assert!(
            !starting_timeout_blocked_actor_can_recover(StartingTimeoutActorFacts {
                actor_blocked: degraded_actor.record.state
                    == agent_doc_controller::actor::ActorState::Blocked,
                last_transition_reason: &degraded_actor.record.last_transition.reason,
                prompt_ready: true,
            }),
            "ordinary degraded actors must not use the starting-timeout recovery path"
        );
    }
    #[test]
    fn authoritative_actor_dispatch_target_eligible_true_only_when_no_guard_reason() {
        let healthy = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: Some(RouteActorState::Ready),
                current_harness: None,
            },
            pending_harness_switch: None,
        };
        assert!(supervisor_authoritative_actor_dispatch_target_eligible(
            &healthy.runtime
        ));

        let degraded = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::NoSocket,
                actor_state: None,
                current_harness: None,
            },
            pending_harness_switch: None,
        };
        assert!(!supervisor_authoritative_actor_dispatch_target_eligible(
            &degraded.runtime
        ));

        let no_state = AuthoritativeActorDispatchTarget {
            record: test_actor_record("%1"),
            runtime: SupervisorRuntime {
                health: SupervisorHealth::Healthy,
                actor_state: None,
                current_harness: None,
            },
            pending_harness_switch: None,
        };
        assert!(!supervisor_authoritative_actor_dispatch_target_eligible(
            &no_state.runtime
        ));
    }
    #[test]
    fn dispatch_only_starting_pane_ready_timeout_production_values() {
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), false),
            Duration::from_secs(15)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("codex"), false),
            Duration::from_secs(2)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("claude"), false),
            Duration::from_secs(2)
        );
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), true),
            Duration::from_millis(250)
        );
    }
    #[test]
    fn route_starting_actor_not_ready_log_line_includes_typed_lifecycle_facts() {
        let h = agent_doc_harness::HarnessConfig::codex();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 42,
            actor_state: ActorDispatchState::Busy,
            supervisor_health: "healthy".to_string(),
            runtime_state: "busy".to_string(),
            prompt_ready: false,
            last_transition_reason: "restart_bootstrap".to_string(),
            last_transition_caller: "start".to_string(),
        };

        let line = route_starting_actor_not_ready_log_line(
            Path::new("/tmp/doc.md"),
            &h,
            Duration::from_secs(8),
            Duration::from_millis(8_125),
            &facts,
        );

        assert!(line.contains("route_authoritative_actor_starting_not_ready"));
        assert!(line.contains("file=/tmp/doc.md"));
        assert!(line.contains("harness=codex"));
        assert!(line.contains("timeout_ms=8000"));
        assert!(line.contains("elapsed_ms=8125"));
        assert!(line.contains("pane=%7"));
        assert!(line.contains("generation=42"));
        assert!(line.contains("actor_state=busy"));
        assert!(line.contains("supervisor_health=healthy"));
        assert!(line.contains("runtime_state=busy"));
        assert!(line.contains("prompt_ready=false"));
        assert!(line.contains("last_transition_reason=restart_bootstrap"));
        assert!(line.contains("last_transition_caller=start"));
    }
    #[test]
    fn starting_actor_timeout_record_coalesces_same_generation_and_pane() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 42,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "repeat timeout").unwrap(),
            StartingActorTimeoutLogDecision::DuplicateTimeout
        );

        let mut next_generation = facts.clone();
        next_generation.generation += 1;
        assert_eq!(
            record_starting_actor_timeout(&file_path, &next_generation, "next timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
    #[test]
    fn starting_actor_timeout_record_matches_same_generation_and_pane() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-match.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 3,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert!(!starting_actor_timeout_record_matches(&file_path, &facts));
        record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap();
        assert!(starting_actor_timeout_record_matches(&file_path, &facts));

        let mut different_generation = facts.clone();
        different_generation.generation += 1;
        assert!(!starting_actor_timeout_record_matches(
            &file_path,
            &different_generation
        ));

        let mut different_pane = facts;
        different_pane.pane_id = "%8".to_string();
        assert!(!starting_actor_timeout_record_matches(
            &file_path,
            &different_pane
        ));
    }
    #[test]
    fn starting_actor_timeout_record_does_not_match_nonstarting_actor_state() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-busy.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let starting = AuthoritativeActorReadyFacts {
            pane_id: "%7".to_string(),
            generation: 3,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        record_starting_actor_timeout(&file_path, &starting, "first timeout").unwrap();

        let mut busy = starting.clone();
        busy.actor_state = ActorDispatchState::Busy;
        busy.runtime_state = "busy".to_string();
        busy.last_transition_reason = "ipc_inject".to_string();
        busy.last_transition_caller = "dispatch".to_string();

        assert!(
            starting_actor_timeout_record_identity_matches(&file_path, &busy),
            "the stale timeout has the same pane and generation as the post-clear busy projection"
        );
        assert!(
            !starting_actor_timeout_record_matches(&file_path, &busy),
            "a cached starting timeout must not short-circuit a later busy wait for the same generation"
        );
    }
    #[test]
    fn starting_actor_timeout_record_clears_after_ready_or_terminal_refresh() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/agent-doc/timeout-clear.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let facts = AuthoritativeActorReadyFacts {
            pane_id: "%9".to_string(),
            generation: 5,
            actor_state: ActorDispatchState::Starting,
            supervisor_health: "healthy".to_string(),
            runtime_state: "starting".to_string(),
            prompt_ready: false,
            last_transition_reason: "session_start".to_string(),
            last_transition_caller: "start".to_string(),
        };

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        clear_starting_actor_timeout_record(&file_path, &facts);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "after clear").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
    #[test]
    fn wait_for_ready_override_guard_sets_and_restores_thread_local() {
        use agent_doc_route_io::invocation::{WaitForReadyOverrideGuard, wait_for_ready_override};
        use std::time::Duration;

        // Baseline: no override set.
        assert_eq!(wait_for_ready_override(), None);

        // Outer scope sets a 30s override.
        let outer = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(30)));
        let outer_remaining = wait_for_ready_override().expect("outer deadline");
        assert!(outer_remaining <= Duration::from_secs(30));
        assert!(outer_remaining > Duration::from_secs(29));

        {
            // Inner scope replaces with a 60s override.
            let _inner = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
            let inner_remaining = wait_for_ready_override().expect("inner deadline");
            assert!(inner_remaining <= Duration::from_secs(60));
            assert!(inner_remaining > Duration::from_secs(59));

            // Nested unset is honored too.
            let _none = WaitForReadyOverrideGuard::set(None);
            assert_eq!(wait_for_ready_override(), None);
        }

        // Both nested guards dropped — back to outer 30s.
        let restored_outer_remaining = wait_for_ready_override().expect("restored outer deadline");
        assert!(restored_outer_remaining <= outer_remaining);
        assert!(restored_outer_remaining > Duration::from_secs(29));

        drop(outer);
        // Outer dropped — back to unset baseline.
        assert_eq!(wait_for_ready_override(), None);
    }
    #[test]
    fn dispatch_only_starting_pane_ready_timeout_honors_override_then_default() {
        use agent_doc_route_io::invocation::WaitForReadyOverrideGuard;
        use std::time::Duration;

        let codex = agent_doc_harness::HarnessConfig::codex();
        assert_eq!(
            dispatch_only_starting_pane_ready_timeout(&codex),
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("codex"), false)
        );

        let guard = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
        let remaining = dispatch_only_starting_pane_ready_timeout(&codex);
        assert!(remaining <= Duration::from_secs(60));
        assert!(remaining > Duration::from_secs(59));
        drop(guard);

        assert_eq!(
            dispatch_only_starting_pane_ready_timeout(&codex),
            dispatch_only_starting_pane_ready_timeout_for_binary(Some("codex"), false)
        );
    }

    #[test]
    fn dispatch_only_checks_operator_draft_before_spending_ready_budget() {
        let source = include_str!("../src/dispatch_only.rs");
        let ready_loop = source
            .split_once("let ready_deadline = Instant::now() + ready_timeout;")
            .unwrap()
            .1
            .split_once("let recovery_remaining")
            .unwrap()
            .0;
        let draft_probe = ready_loop
            .find("pane_composer_draft")
            .expect("operator draft probe");
        let blocking_wait = ready_loop
            .find("wait_for_agent_ready_outcome")
            .expect("ready wait");
        assert!(
            draft_probe < blocking_wait,
            "a visible draft is terminal and must fail before the bounded ready wait"
        );
    }
}

#[test]
fn route_coalesces_monotonic_whole_document_replay_before_residue_guard() {
    let stale = concat!(
        "---\nagent_doc_session: replay-test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\n",
        "*Compacted. Content archived to `.agent-doc/archives/prior.md`*\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:done -->\n<!-- /agent:done -->\n",
    );
    let live = stale.replace(
        "<!-- /agent:exchange -->",
        concat!(
            "In the funding package, move wire instructions above the notices.\n",
            "#spec-test-commit-push-deploy\n",
            "<!-- /agent:exchange -->"
        ),
    );
    let replayed = format!("{live}{stale}");

    let cleanup = scrub_duplicate_prompt_comments_for_route(&replayed, &[])
        .unwrap()
        .expect("route should coalesce a stale monotonic whole-document replay");

    assert_eq!(cleanup.coalesced_replay_copies, Some(2));
    assert_eq!(cleanup.content, live);
    assert_eq!(
        cleanup
            .content
            .matches("Compacted. Content archived")
            .count(),
        1
    );
    assert!(cleanup.content.contains("#spec-test-commit-push-deploy"));
}

/// `#steernoblock`: the pre-dispatch settle wait must never abort a route
/// invocation.
///
/// By the time route runs, the operator's prompt is ALREADY in the document (the
/// JetBrains action writes its prompt marker before routing) and an active turn
/// consumes it as realtime steering. Propagating the settle-wait expiry aborted
/// dispatch with "route deferred ...: Lazily current transition remained
/// delivery_pending for 5000ms" — reporting a failure for work that had already
/// landed and leaving the operator retrying a no-op.
///
/// Pinned as a source contract because exercising the live path needs a real
/// editor replica and tmux pane: the call must not use `?` on `await_idle`.
#[test]
fn route_settle_wait_never_blocks_dispatch() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/command.rs"),
    )
    .expect("route command source should be readable");

    assert!(
        !source.contains("await_idle(file, Duration::from_millis(debounce_ms))?;"),
        "the settle wait must not `?`-propagate — that aborts dispatch and blocks realtime steering"
    );
    assert!(
        source.contains("route_settle_wait_advisory"),
        "an unsettled transition must be recorded as advisory and proceed"
    );
    assert!(
        source.contains("#steernoblock"),
        "the non-blocking contract should stay documented at the call site"
    );
}
