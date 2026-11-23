//! # Module: preflight
//!
//! ## Spec
//! - `run(file)`: executes the full pre-agent preparation sequence for a
//!   session document and emits a single JSON object to stdout.
//! - Bails immediately if the file does not exist.
//! - Normal CLI execution publishes typed document observations to one
//!   controller-owned per-document `Computed`; the short-lived preflight process
//!   emits output from that projection. Probe mode uses an actorless local
//!   projection so it remains side-effect free.
//! - Effects are admitted by typed signals in dependency order:
//!   repair -> prior-cycle commit -> derived reads -> baseline checkpoint ->
//!   cycle-open. The terminal gate rejects skipped prerequisites. Cycle-open is
//!   still a durable exactly-once identity, not computed state.
//! - The layout check calls `check_layout()` to detect tmux structural problems
//!   (window index, session drift, and registered panes split across visible
//!   windows). Index/window drift is repaired immediately before the check is
//!   repeated; remaining issues are included in output but do not abort the run.
//! - The interrupted-cycle guard inspects persisted cycle state.
//!   For any open prior cycle, preflight auto-attempts `agent_doc_repair_io::run` +
//!   `git::commit(file)` before diffing again. For an open `preflight_started`
//!   cycle with no recoverable response and unresolved prompt-bearing drift,
//!   preflight fails closed while fresh; once stale, repair abandons the empty
//!   cycle and preserves the unanswered prompt as an uncommitted, actionable
//!   diff for the next run. Abandonment does not authorize a recovery commit.
//!   Non-prompt drift may still use the narrow no-op closeout
//!   that stages the snapshot only and leaves later live working-tree edits
//!   uncommitted; if that closeout still cannot prove the prior cycle is durable,
//!   preflight fails closed instead of diffing again.
//! - Step 1 — repair: calls `agent_doc_repair_io::run` to detect and apply any
//!   orphaned pending agent responses from a previous interrupted cycle.
//! - Step 2 — commit: calls `git::commit(file)` to record the previous
//!   exchange cycle; failure is downgraded to a warning, not a hard error.
//! - Step 3 — claims: reads `.agent-doc/claims.log` line-by-line via
//!   `read_and_truncate_claims`, then truncates the log to empty; claims are
//!   returned to the caller in the JSON output.
//! - Step 3b — debounce: waits up to 3 seconds (polling every 100 ms) for
//!   both the file mtime to be at least 500 ms old and the cross-process
//!   typing indicator to be inactive before proceeding to the diff step.
//! - Step 3c — linked docs: calls `check_linked_docs(file)` to inspect
//!   `links` from frontmatter. For local file links, compares git commit
//!   times against the snapshot mtime. For URL links (`http://`/`https://`),
//!   fetches content via `ureq`, converts HTML to markdown via `htmd`
//!   (stripping script/style/nav/footer/noscript/svg), caches in
//!   `.agent-doc/links_cache/<sha256(url)>.txt`, and reports changes by
//!   comparing against the cached content.
//! - Step 4 - diff: calls `agent_doc_diff_io::compute(...)` to compare the current
//!   document against the last snapshot; `no_changes=true` when they match.
//! - Also emits a bounded `session_accretion` advisory when local exchange/log
//!   heuristics detect churn-heavy growth or restart-heavy reopen patterns.
//! - Serializes `PreflightOutput` as pretty JSON to stdout; all diagnostic
//!   messages go to stderr.
//! - `check_layout()`: inspects the current tmux session for structural issues:
//!   missing window index 0 (base-index compliance), session drift, duplicate
//!   `agent-doc` windows, and registered panes scattered across non-stash
//!   windows. Stash windows may have non-idle panes (backgrounded sessions).
//!   Read-only; no mutations.
//!   Returns an empty vec when not inside tmux (silent).
//! - `read_and_truncate_claims(file)`: locates `.agent-doc/claims.log` relative
//!   to the project root, collects non-empty lines, truncates the file to empty,
//!   and returns the lines. Returns empty vec if the log is absent or unreadable.
//!
//! ## Agentic Contracts
//! - All output intended for the SKILL workflow is on stdout as valid JSON;
//!   callers must not parse stderr.
//! - `no_changes=true` in the output means the SKILL workflow should skip
//!   sending to the agent; `diff` will be `null` in this case.
//! - `layout_issues` reports structural tmux issues that remain after any
//!   immediate pre-diff layout repair has run.
//! - The claims log is consumed (truncated) exactly once per `preflight` call;
//!   a second call in the same cycle will return empty claims.
//! - Recovery (`recovered=true`) means the document was modified before the
//!   diff step; the `diff` and `document` fields reflect post-recovery state.
//! - Debounce waits for user typing to settle before computing the diff;
//!   if the 3-second timeout expires, `run` proceeds and logs a warning to
//!   stderr — it never blocks indefinitely.
//! - `check_layout` is always safe to call outside tmux; it returns `[]`.
//!
//! ## Evals
//! - `preflight_produces_valid_json`: document with matching snapshot →
//!   `run` returns `Ok(())` and emits parseable JSON with `no_changes=true`.
//! - `preflight_file_not_found`: missing path → `Err` containing "file not found".
//! - `preflight_detects_diff`: snapshot saved at original content, document
//!   updated with new content → `diff::compute` returns `Some(_)` (non-null diff).
//! - `preflight_claims_read_and_truncated`: claims.log with two entries →
//!   `read_and_truncate_claims` returns both lines and the log is empty afterwards.
//! - `preflight_no_claims_log_returns_empty`: no claims.log present →
//!   `read_and_truncate_claims` returns an empty vec without error.
//! - `preflight_output_serializes_correctly`: `PreflightOutput` with known
//!   values serializes to JSON with correct field names and types.
//! - `preflight_output_null_diff_when_no_changes`: `diff=None` + `no_changes=true`
//!   → JSON has `"diff": null` and `"no_changes": true`.
//! - `check_layout_returns_empty_outside_tmux`: `TMUX` env var unset →
//!   `check_layout()` returns empty vec without invoking tmux.
//! - `check_layout_detects_session_drift`: two alive registered panes in
//!   different sessions → `layout_issues` contains a "session drift" entry.
//! - `preflight_output_includes_layout_issues`: `PreflightOutput` with one
//!   layout issue → JSON `layout_issues` array has length 1 with correct text.
//! - `preflight_output_slash_commands_from_diff`: diff containing `+/clear` →
//!   `builtin_commands` array has one entry `"/clear"` (built-in, not in `slash_commands`).
//! - `is_url_detects_http`: `http://` and `https://` prefixes → true;
//!   relative paths and empty strings → false.
//! - `is_html_content_detects_html`: `text/html` and `application/xhtml` → true;
//!   `application/json` and `text/plain` → false.
//! - `html_to_markdown_converts_basic_html`: `<h1>` and `<strong>` → markdown
//!   heading and bold syntax.
//! - `html_to_markdown_strips_script_and_style`: script/style content removed
//!   from output, visible content preserved.
//! - `html_to_markdown_strips_nav_and_footer`: nav/footer content removed,
//!   main content preserved.
//! - `url_cache_path_is_deterministic`: same URL → same path; different URL →
//!   different path; extension is `.txt`.
//! - `links_cache_dir_creates_directory`: creates `.agent-doc/links_cache/` and
//!   returns `Some(path)` when `.agent-doc/` exists.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agent_doc_preflight_io::{
    PreflightOutput, PreflightWarning,
    layout::{check_layout, maybe_auto_repair_base_index, maybe_auto_resync_on_drift},
};
#[cfg(test)]
use agent_doc_preflight_io::{PreflightResponseContract, layout::detect_duplicate_claims};
use agent_doc_preflight_runtime_io::PREFLIGHT_MAINTENANCE_WRITE_EFFECTS;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionReport;

const TEST_BACKGROUND_PENDING_MAINTENANCE_DELAY_MS_ENV: &str =
    "AGENT_DOC_TEST_BACKGROUND_PENDING_MAINTENANCE_DELAY_MS";
/// Integration-only barrier (`#releasegateflake`): the detached worker parks
/// inside its maintenance body, holding the apply flock, the coordination
/// lease, and the claimed request, until this path exists. It touches
/// `<path>.parked` once parked.
const TEST_BACKGROUND_PENDING_MAINTENANCE_RELEASE_FILE_ENV: &str =
    "AGENT_DOC_TEST_BACKGROUND_PENDING_MAINTENANCE_RELEASE_FILE";
/// Integration-only interleaving (`#releasegateflake`): the admitting process
/// waits after spawning the worker until the worker has parked (the
/// `<path>.parked` marker of the barrier above), so the worker owns every
/// maintenance resource before admission finishes its dispatch and exits.
const TEST_PREFLIGHT_AWAIT_PARKED_WORKER_ENV: &str = "AGENT_DOC_TEST_PREFLIGHT_AWAIT_PARKED_WORKER";
const TEST_BARRIER_MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(120);
const TEST_BACKGROUND_MAINTENANCE_CRASH_AFTER_LOCK_ENV: &str =
    "AGENT_DOC_TEST_BACKGROUND_MAINTENANCE_CRASH_AFTER_LOCK";
const TEST_BACKGROUND_MAINTENANCE_PANIC_AFTER_EFFECT_ENV: &str =
    "AGENT_DOC_TEST_BACKGROUND_MAINTENANCE_PANIC_AFTER_EFFECT";
const BACKGROUND_QUEUE_MAINTENANCE_TOKEN_ENV: &str = "AGENT_DOC_BACKGROUND_QUEUE_MAINTENANCE_TOKEN";
const BACKGROUND_QUEUE_MAINTENANCE_AUTHORITY_HASH_ENV: &str =
    "AGENT_DOC_BACKGROUND_QUEUE_MAINTENANCE_AUTHORITY_HASH";

fn new_background_queue_maintenance_token() -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(
        "{}{}-{nanos}-{sequence}",
        agent_doc_queue_io::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX,
        std::process::id()
    )
}

/// Opt-out of controller-hosted maintenance: `process` keeps every request on
/// the detached worker process (`#q-e9483473162d`).
const BACKGROUND_MAINTENANCE_HOST_ENV: &str = "AGENT_DOC_BACKGROUND_MAINTENANCE_HOST";

/// Whether this admission may hand its request to the project controller.
///
/// The integration barriers above (`AGENT_DOC_TEST_*MAINTENANCE*`) are read by
/// the worker from its own environment, which only a worker this process
/// spawns inherits, so a test that sets one keeps the detached worker.
fn controller_hosted_maintenance_allowed() -> bool {
    if std::env::var(BACKGROUND_MAINTENANCE_HOST_ENV)
        .is_ok_and(|host| host.trim().eq_ignore_ascii_case("process"))
    {
        return false;
    }
    !std::env::vars_os().any(|(name, _)| {
        name.to_str().is_some_and(|name| {
            (name.starts_with("AGENT_DOC_TEST_") && name.contains("MAINTENANCE"))
                || name == TEST_PREFLIGHT_AWAIT_PARKED_WORKER_ENV
        })
    })
}

/// Dispatch comprehensive queue reconciliation outside the admission process.
///
/// The admitting process publishes the document's request (one latest row,
/// claimed by generation and fenced by `admitted_authority_hash`) and returns.
/// `#q-e9483473162d`: when a project controller is running, it owns the
/// request and runs it on its own per-document worker thread, coalesced and
/// debounced per document ([`agent_doc_controller_io::document_maintenance`]),
/// so the hook creates no process. Without one (or with one that refuses),
/// this spawns the detached worker as before: the child re-observes editor
/// authority before every mutation, and the admitting process only pays for
/// process creation. Using the binary selected by the controller's launch
/// policy also keeps embedded/plugin callers from trying to execute their host
/// process as `agent-doc`.
///
/// Returns the pid that owns the request: the controller's, or the child's.
pub fn spawn_background_queue_maintenance(
    file: &Path,
    commit_if_mutated: bool,
    admitted_authority_hash: &str,
) -> Result<u32> {
    let _request = agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
        file,
        admitted_authority_hash,
        commit_if_mutated,
    )?;
    if controller_hosted_maintenance_allowed() {
        let started = std::time::Instant::now();
        match agent_doc_controller_io::project_controller::enqueue_document_maintenance(file) {
            Ok(receipt) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_controller_enqueued file={} controller_pid={} generation={} outcome={} elapsed_us={} (#q-e9483473162d)",
                        file.display(),
                        receipt.controller_pid,
                        receipt.controller_generation,
                        receipt.outcome.as_str(),
                        started.elapsed().as_micros(),
                    ),
                );
                return Ok(receipt.controller_pid);
            }
            Err(error) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_controller_unavailable file={} fallback=process elapsed_us={} error={} (#q-e9483473162d)",
                    file.display(),
                    started.elapsed().as_micros(),
                    format!("{error:#}").replace('\n', " | "),
                ),
            ),
        }
    }
    spawn_background_queue_maintenance_process(file, commit_if_mutated, admitted_authority_hash)
}

/// The detached-process fallback of [`spawn_background_queue_maintenance`];
/// the request row is already published.
fn spawn_background_queue_maintenance_process(
    file: &Path,
    commit_if_mutated: bool,
    admitted_authority_hash: &str,
) -> Result<u32> {
    let binary = agent_doc_controller_io::project_controller::current_binary_identity()?.path;
    let token = new_background_queue_maintenance_token();
    // Claim before process creation. Concurrent admissions race in one SQLite
    // statement, so only the winner launches a child at all. The unique token
    // fences PID reuse and is passed to the child for atomic adoption.
    if !agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
        &file.to_string_lossy(),
        &token,
        std::process::id(),
    )
    .context("failed to reserve background queue maintenance ownership")?
    {
        let holder_pid = agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(
            &file.to_string_lossy(),
        )
        .map(|lease| lease.holder_pid)
        .unwrap_or_default();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_background_dispatch_coalesced file={} holder_pid={holder_pid}",
                file.display(),
            ),
        );
        return Ok(holder_pid);
    }
    let mut command = Command::new(&binary);
    command
        .arg("preflight")
        .arg(file)
        .arg("--queue-maintenance-only")
        .env(BACKGROUND_QUEUE_MAINTENANCE_TOKEN_ENV, &token)
        .env(
            BACKGROUND_QUEUE_MAINTENANCE_AUTHORITY_HASH_ENV,
            admitted_authority_hash,
        );
    if commit_if_mutated {
        command.arg("--commit-queue-maintenance");
    }
    let pid = match spawn_detached_queue_effect(&mut command) {
        Ok(pid) => pid,
        Err(error) => {
            agent_doc_queue_io::queue_edit_owner::clear_queue_edit_owner_lease_if_holder(
                &file.to_string_lossy(),
                &token,
            );
            return Err(anyhow::anyhow!(
                "failed to launch background queue maintenance with {}: {error}",
                binary.display()
            ));
        }
    };
    await_parked_worker_for_test(file, pid);
    // The reservation starts with the dispatcher's PID so a dispatcher crash
    // before process creation is recoverable. Once spawn succeeds, hand the
    // same token to the child PID immediately. This also makes a child crash
    // before `adopt_reserved` promptly recoverable while a long-lived plugin
    // host continues running.
    match agent_doc_queue_io::queue_edit_owner::refresh_queue_edit_owner_lease_if_holder(
        &file.to_string_lossy(),
        &token,
        pid,
    ) {
        Ok(_) => {}
        Err(error) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_background_pid_handoff_failed file={} pid={} error={}",
                file.display(),
                pid,
                format!("{error:#}").replace('\n', " | "),
            ),
        ),
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_background_dispatched file={} pid={} binary={}",
            file.display(),
            pid,
            binary.display(),
        ),
    );
    Ok(pid)
}

fn parked_marker(release: &Path) -> PathBuf {
    let mut marker = release.as_os_str().to_owned();
    marker.push(".parked");
    PathBuf::from(marker)
}

/// See [`TEST_PREFLIGHT_AWAIT_PARKED_WORKER_ENV`]. Forces the interleaving in
/// which the detached worker already holds the apply flock, the lease, and the
/// claim while admission still has its post-spawn handoff, its log append,
/// and process exit ahead of it. Admission must still finish on its own.
fn await_parked_worker_for_test(file: &Path, pid: u32) {
    let Some(release) = std::env::var_os(TEST_PREFLIGHT_AWAIT_PARKED_WORKER_ENV) else {
        return;
    };
    let marker = parked_marker(Path::new(&release));
    let started = std::time::Instant::now();
    while !marker.exists() && started.elapsed() < TEST_BARRIER_MAX_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_background_test_awaited_parked_worker file={} pid={} parked={} waited_ms={}",
            file.display(),
            pid,
            marker.exists(),
            started.elapsed().as_millis(),
        ),
    );
}

/// See [`TEST_BACKGROUND_PENDING_MAINTENANCE_RELEASE_FILE_ENV`].
fn park_worker_for_test(file: &Path) -> Result<()> {
    let Some(release) = std::env::var_os(TEST_BACKGROUND_PENDING_MAINTENANCE_RELEASE_FILE_ENV)
    else {
        return Ok(());
    };
    let release = PathBuf::from(release);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "pending_maintenance_background_test_parked file={} pid={}",
            file.display(),
            std::process::id(),
        ),
    );
    std::fs::write(parked_marker(&release), std::process::id().to_string())
        .context("failed to publish the parked-worker test marker")?;
    let started = std::time::Instant::now();
    while !release.exists() && started.elapsed() < TEST_BARRIER_MAX_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "pending_maintenance_background_test_released file={} released={} parked_ms={}",
            file.display(),
            release.exists(),
            started.elapsed().as_millis(),
        ),
    );
    Ok(())
}

fn spawn_detached_queue_effect(command: &mut Command) -> std::io::Result<u32> {
    detach_queue_effect(command);
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child.id();
    // Dropping `Child` intentionally leaves the independently owned worker
    // running; the short-lived preflight process must not join it.
    drop(child);
    Ok(pid)
}

#[cfg(unix)]
fn detach_queue_effect(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // The admitting CLI can exit immediately after spawning this worker. Start
    // a new session so a hook runner, test harness, terminal, or controller
    // process-group cleanup cannot kill comprehensive maintenance with its
    // short-lived parent.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn detach_queue_effect(_command: &mut Command) {}

/// A detached maintenance worker's ownership of one document's queue
/// maintenance. Ownership is the durable Lazily claim
/// (`agent_doc_queue_io::queue_maintenance_claim`): this worker reconnects as
/// the claim's newest actor generation, and each admitted request is one
/// claimed Lazily epoch. The OS apply flock and coordination-lease row held by
/// `QueueEditGuard` are the backend lock behind the claim; the heartbeat keeps
/// that lease fresh and reports its loss to Lazily as supersession.
struct BackgroundQueueMaintenanceClaim {
    file: PathBuf,
    token: String,
    claim: agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim,
    stop: Option<std::sync::mpsc::Sender<()>>,
    heartbeat: Option<std::thread::JoinHandle<()>>,
    guard: agent_doc_queue_io::queue_edit_owner::QueueEditGuard,
}

impl BackgroundQueueMaintenanceClaim {
    fn adopt_reserved(file: &Path, token: &str) -> Result<Option<Self>> {
        // A concurrent preflight can acquire the apply flock immediately after
        // this worker's reservation probe releases it, then observe the live
        // reservation and back out. Do not abandon the sole reserved worker in
        // that short lock-before-lease-check window. Wait only while the exact
        // token is still ours; losing the reservation is an immediate stop.
        let started = std::time::Instant::now();
        let deadline = started + std::time::Duration::from_secs(5);
        let guard = loop {
            if let Some(guard) =
                agent_doc_queue_io::queue_edit_owner::QueueEditGuard::adopt_reserved(file, token)?
            {
                break guard;
            }
            let lease = agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(
                &file.to_string_lossy(),
            );
            if !lease.as_ref().is_some_and(|lease| lease.holder == token) {
                return Ok(None);
            }
            if std::time::Instant::now() >= deadline {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_background_adoption_timed_out file={} pid={} waited_ms={}",
                        file.display(),
                        std::process::id(),
                        started.elapsed().as_millis(),
                    ),
                );
                return Ok(None);
            }
            let _ = agent_doc_queue_io::queue_edit_owner::refresh_queue_edit_owner_lease_if_holder(
                &file.to_string_lossy(),
                token,
                std::process::id(),
            )?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        if started.elapsed() >= std::time::Duration::from_millis(2) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_background_adoption_waited file={} pid={} waited_ms={}",
                    file.display(),
                    std::process::id(),
                    started.elapsed().as_millis(),
                ),
            );
        }
        // Holding the flock and the lease row makes this worker the only live
        // owner; reconnecting fences any crashed predecessor's late receipts
        // and requeues its retained request under the same effect identity.
        let claim =
            agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(file)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_background_claim_reconnected file={} pid={} generation={} retained={}",
                file.display(),
                std::process::id(),
                claim.generation(),
                claim.retains_work().unwrap_or(true),
            ),
        );
        let file = file.to_path_buf();
        let heartbeat_file = file.clone();
        let heartbeat_token = token.to_string();
        let heartbeat_claim = claim.clone();
        let (stop, stopped) = std::sync::mpsc::channel();
        let heartbeat = match std::thread::Builder::new()
            .name("queue-maintenance-claim-heartbeat".to_string())
            .spawn(move || loop {
                match stopped.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        let refreshed = agent_doc_queue_io::queue_edit_owner::refresh_queue_edit_owner_lease_if_holder(
                            &heartbeat_file.to_string_lossy(),
                            &heartbeat_token,
                            std::process::id(),
                        );
                        let lost = match refreshed {
                            Ok(true) => continue,
                            Ok(false) => "superseded".to_string(),
                            Err(error) => format!("refresh failed: {error:#}"),
                        };
                        eprintln!(
                            "[preflight] background maintenance lease {lost} for {}",
                            heartbeat_file.display(),
                        );
                        if let Err(error) = heartbeat_claim.record_backend_lock_lost() {
                            eprintln!(
                                "[preflight] could not record lease loss on the maintenance claim for {}: {error:#}",
                                heartbeat_file.display(),
                            );
                        }
                        break;
                    }
                }
            }) {
            Ok(heartbeat) => Some(heartbeat),
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!(
                        "queue_maintenance_background_lease_heartbeat_spawn_failed file={} error={error}",
                        file.display(),
                    ),
                );
                None
            }
        };
        Ok(Some(Self {
            file,
            token: token.to_string(),
            claim,
            stop: heartbeat.as_ref().map(|_| stop),
            heartbeat,
            guard,
        }))
    }

    /// Whether this worker still owns the document's maintenance: its actor
    /// generation is current and the backend lease still names it.
    fn owns(&self) -> Result<bool> {
        if self.heartbeat.is_none() {
            // Without a heartbeat thread, keep the backend lease fresh here.
            let _ = agent_doc_queue_io::queue_edit_owner::refresh_queue_edit_owner_lease_if_holder(
                &self.file.to_string_lossy(),
                &self.token,
                std::process::id(),
            )?;
        }
        let holder = agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(
            &self.file.to_string_lossy(),
        );
        if !holder.is_some_and(|lease| lease.holder == self.token) {
            self.claim.record_backend_lock_lost()?;
            return Ok(false);
        }
        self.claim.is_current_actor()
    }

    fn apply_guard(&self) -> &agent_doc_queue_io::queue_edit_owner::QueueEditGuard {
        &self.guard
    }

    fn claim_request(
        &self,
        request: &agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    ) -> Result<agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaimed> {
        self.claim.claim_request(request)
    }

    /// Verify the claim still holds `request` and the backend lock is ours.
    fn verify(
        &self,
        request: &agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    ) -> Result<()> {
        if !self.owns()? {
            return Err(
                agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaimSuperseded::new(
                    "background queue maintenance reservation was superseded",
                )
                .into(),
            );
        }
        self.claim.verify(request)
    }

    fn complete_request(
        &mut self,
        request: &agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    ) -> Result<bool> {
        self.claim.complete(&mut self.guard, request)
    }

    /// The receipted plan progress a crashed predecessor left for exactly
    /// `request`. An unreadable image is reported and ignored: the plan then
    /// starts from the admission, which is the pre-resume behaviour.
    fn plan_progress(
        &self,
        file: &Path,
        request: &agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    ) -> Option<agent_doc_preflight_io::queue_maintenance_effect_cas::QueueMaintenancePlanProgress>
    {
        let decoded = self.claim.load_plan_progress(request).and_then(|stored| {
            stored
                .map(|stored| serde_json::from_str(&stored).map_err(anyhow::Error::from))
                .transpose()
        });
        match decoded {
            Ok(progress) => progress,
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_plan_progress_unreadable file={} error={}",
                        file.display(),
                        format!("{error:#}").replace('\n', " | "),
                    ),
                );
                None
            }
        }
    }

    /// Persist the receipted progress of `request`'s plan, fenced by this
    /// worker's claim on exactly that request.
    fn store_plan_progress(
        &self,
        request: &agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
        progress: &agent_doc_preflight_io::queue_maintenance_effect_cas::QueueMaintenancePlanProgress,
    ) -> Result<()> {
        self.claim
            .store_plan_progress(request, &serde_json::to_string(progress)?)
    }
}

impl Drop for BackgroundQueueMaintenanceClaim {
    fn drop(&mut self) {
        // Unfinished claimed work keeps the coordination lease durable. This
        // runs during ordinary errors and panic unwinding, so the incomplete
        // attempt stays visible to session-check and recoverable by the next
        // worker, which reconnects and resumes the retained Lazily intent.
        if self.claim.retains_work().unwrap_or(true) {
            self.guard.retain_incomplete_attempt_on_drop();
        }
        self.stop.take();
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
    }
}

/// Fence for one detached maintenance request: the durable Lazily claim must
/// still hold the exact request, and every effect crosses one step of the
/// request's ordered Lazily effect plan against the canonical document
/// authority (`#implementlazilyowned`).
///
/// The plan's receipted progress is persisted in `state.db` beside the claim
/// after every step that applies an effect or receipts a document cut (plan
/// step 9). A worker that resumes the same request after a crash reloads it:
/// its plan expects the last receipted cut instead of the stale admission and
/// skips every replayable effect that already has a receipt.
struct BackgroundRequestEffectFence<'a> {
    owner: &'a BackgroundQueueMaintenanceClaim,
    request: &'a agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    plan: agent_doc_preflight_io::queue_maintenance_effect_cas::QueueMaintenanceEffectPlan,
    /// dogfood `maintlockhold`: every fence crossing observes the authority hash.
    /// Keyed by the relay's compact revision, an unchanged canonical is not
    /// re-materialized under the hub lock on each crossing.
    authority: agent_doc_document_realtime_io::authority_hash_memo::AuthorityHashMemo,
}

impl<'a> BackgroundRequestEffectFence<'a> {
    fn new(
        file: &Path,
        owner: &'a BackgroundQueueMaintenanceClaim,
        request: &'a agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequest,
    ) -> Self {
        let progress = owner.plan_progress(file, request);
        if let Some(progress) = progress
            .as_ref()
            .filter(|progress| progress.admitted == request.admitted_authority_hash)
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_plan_resumed file={} pid={} applied_through={} receipts={}",
                    file.display(),
                    std::process::id(),
                    progress.applied_through,
                    progress.receipts.len(),
                ),
            );
        }
        Self {
            owner,
            request,
            plan:
                agent_doc_preflight_io::queue_maintenance_effect_cas::QueueMaintenanceEffectPlan::resume(
                    file,
                    &request.admitted_authority_hash,
                    progress,
                ),
            authority: agent_doc_document_realtime_io::authority_hash_memo::AuthorityHashMemo::new(),
        }
    }

    fn observe_authority(&self, file: &Path, label: &str) -> Result<String> {
        self.authority.observe_hash(file, label)
    }

    fn fence_effect(&self, file: &Path) -> Result<()> {
        agent_doc_preflight_io::PendingMaintenanceEffectFence::check(self, file)
    }

    /// Persist the plan's receipted progress after a step that changed it.
    fn store_progress(&self) -> Result<()> {
        self.owner
            .store_plan_progress(self.request, &self.plan.progress())
    }
}

impl agent_doc_preflight_io::PendingMaintenanceEffectFence for BackgroundRequestEffectFence<'_> {
    fn check(&self, file: &Path) -> Result<()> {
        self.apply_step(
            file,
            agent_doc_preflight_io::QueueMaintenanceEffectStep::Verify,
        )
    }

    fn apply_step(
        &self,
        file: &Path,
        step: agent_doc_preflight_io::QueueMaintenanceEffectStep,
    ) -> Result<()> {
        self.owner.verify(self.request)?;
        let observe = || self.observe_authority(file, "background_queue_maintenance_effect_fence");
        self.plan.cross(step, &observe)
    }

    fn apply_effect_step(
        &self,
        file: &Path,
        step: agent_doc_preflight_io::QueueMaintenanceEffectStep,
        effect: &mut dyn FnMut() -> Result<String>,
    ) -> Result<()> {
        self.owner.verify(self.request)?;
        let observe = || self.observe_authority(file, "background_queue_maintenance_effect_fence");
        self.plan.apply_effect(step, &observe, effect)?;
        self.store_progress()
    }

    fn apply_replayable_effect_step(
        &self,
        file: &Path,
        step: agent_doc_preflight_io::QueueMaintenanceEffectStep,
        intent: &str,
        effect: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        self.owner.verify(self.request)?;
        let observe = || self.observe_authority(file, "background_queue_maintenance_effect_fence");
        self.plan
            .apply_replayable_effect(step, intent, &observe, effect)?;
        self.store_progress()
    }

    fn record_applied_content(&self, file: &Path, content: &str) -> Result<()> {
        self.owner.verify(self.request)?;
        let observe = || self.observe_authority(file, "background_queue_maintenance_applied_cut");
        self.plan
            .record_document_cut(agent_doc_hash::content_hash(content), &observe)?;
        self.store_progress()
    }

    fn expected_authority_hash(&self) -> Option<String> {
        self.plan.expected_authority_hash()
    }

    fn stage_document_cut(
        &self,
        _file: &Path,
        cut: agent_doc_preflight_io::queue_maintenance_effect_cas::StagedDocumentCut,
    ) -> Result<()> {
        self.owner.verify(self.request)?;
        self.plan.stage_cut(cut);
        self.store_progress()
    }

    fn staged_document_cut(
        &self,
    ) -> Option<agent_doc_preflight_io::queue_maintenance_effect_cas::StagedDocumentCut> {
        self.plan.staged_cut()
    }

    fn finish_document_cut(&self, _file: &Path) -> Result<()> {
        if self.plan.staged_cut().is_none() {
            return Ok(());
        }
        self.owner.verify(self.request)?;
        self.plan.finish_cut();
        self.store_progress()
    }

    fn observed_authority_hash(&self, file: &Path) -> Result<Option<String>> {
        self.observe_authority(file, "background_queue_maintenance_staged_cut")
            .map(Some)
    }

    fn record_applied_cut_hash(&self, file: &Path, cut_hash: &str) -> Result<()> {
        self.owner.verify(self.request)?;
        let observe = || self.observe_authority(file, "background_queue_maintenance_staged_cut");
        self.plan
            .record_document_cut(cut_hash.to_string(), &observe)?;
        self.store_progress()
    }
}

/// Hidden child-process entrypoint for comprehensive queue reconciliation.
///
/// The queue mutation lease coalesces overlapping admissions: a worker that
/// observes another live worker simply defers through the existing maintenance
/// fence. All authority and persistence checks remain in the ordinary queue
/// maintenance implementation, so moving the effect off the admission lane
/// does not introduce a disk-authority escape hatch.
pub fn run_background_queue_maintenance(file: &Path, commit_if_mutated: bool) -> Result<()> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_background_worker_entered file={} pid={}",
            file.display(),
            std::process::id(),
        ),
    );
    let token = (|| -> Result<String> {
        if cfg!(test) {
            // Unit tests invoke the worker body directly instead of going
            // through `spawn_background_queue_maintenance`, so reserve the test
            // process as the worker owner before exercising the same adoption
            // path.
            let token = new_background_queue_maintenance_token();
            let _ = agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &file.to_string_lossy(),
                &token,
                std::process::id(),
            )?;
            let content = std::fs::read_to_string(file)?;
            agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
                file,
                &agent_doc_hash::content_hash(&content),
                commit_if_mutated,
            )?;
            Ok(token)
        } else {
            std::env::var(BACKGROUND_QUEUE_MAINTENANCE_TOKEN_ENV)
                .context("background queue maintenance reservation token is missing")
        }
    })();
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            log_queue_maintenance_worker_exit(
                file,
                QueueMaintenanceWorkerMode::Detached,
                &Err(anyhow::anyhow!("{error:#}")),
            );
            return Err(error);
        }
    };
    run_queue_maintenance_worker_logged(file, &token, QueueMaintenanceWorkerMode::Detached)
}

/// `#q-e9483473162d`: reserve `file`'s maintenance for a worker thread of
/// this (controller) process, as [`spawn_background_queue_maintenance`]
/// reserves it for a child. `Ok(None)`: no request is pending, or another live
/// owner holds the reservation and will consume the latest request.
pub fn reserve_controller_queue_maintenance(file: &Path) -> Result<Option<String>> {
    if agent_doc_queue_io::queue_edit_owner::load_queue_maintenance_request(file)?.is_none() {
        return Ok(None);
    }
    let token = new_background_queue_maintenance_token();
    if agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
        &file.to_string_lossy(),
        &token,
        std::process::id(),
    )
    .context("failed to reserve controller queue maintenance ownership")?
    {
        return Ok(Some(token));
    }
    let holder_pid =
        agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(&file.to_string_lossy())
            .map(|lease| lease.holder_pid)
            .unwrap_or_default();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_controller_coalesced file={} holder_pid={holder_pid} (#q-e9483473162d)",
            file.display(),
        ),
    );
    Ok(None)
}

/// `#q-e9483473162d`: run `file`'s pending maintenance request on the calling
/// controller thread under `token` (from
/// [`reserve_controller_queue_maintenance`]). The worker body is the detached
/// child's; this process outlives the run, so the reservation is always
/// released afterwards. An unfinished claim keeps its retained intent and the
/// request row, so a closeout or the next admission recovers it.
pub fn run_controller_queue_maintenance(file: &Path, token: &str) -> Result<()> {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_background_worker_entered file={} pid={} mode=controller",
            file.display(),
            std::process::id(),
        ),
    );
    let outcome =
        run_queue_maintenance_worker_logged(file, token, QueueMaintenanceWorkerMode::Controller);
    release_controller_queue_maintenance(file, token);
    outcome
}

/// Release `token` if it is still `file`'s maintenance reservation.
pub fn release_controller_queue_maintenance(file: &Path, token: &str) {
    agent_doc_queue_io::queue_edit_owner::clear_queue_edit_owner_lease_if_holder(
        &file.to_string_lossy(),
        token,
    );
}

/// How a queue-maintenance worker body came to own its reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueMaintenanceWorkerMode {
    /// The detached child preflight dispatched.
    Detached,
    /// A closeout that adopted an orphaned request in process
    /// (`#deadmaintworker`).
    Adopted,
    /// A worker thread of the project controller (`#q-e9483473162d`).
    Controller,
}

impl QueueMaintenanceWorkerMode {
    fn label(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::Adopted => "adopted",
            Self::Controller => "controller",
        }
    }
}

fn panic_payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}

/// `#deadmaintworker`: every worker exit leaves one durable line. The detached
/// child runs with stderr on `/dev/null`, so an error it returned (a failed
/// authority read, a pending-maintenance error, a claim-store error) used to
/// vanish: the claim stayed in flight and the request stayed pending with no
/// trace of why the worker stopped.
fn log_queue_maintenance_worker_exit(
    file: &Path,
    mode: QueueMaintenanceWorkerMode,
    outcome: &Result<()>,
) {
    let detail = match outcome {
        Ok(()) => "outcome=ok".to_string(),
        Err(error) => format!(
            "outcome=error error={}",
            format!("{error:#}").replace('\n', " | ")
        ),
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_background_exited file={} pid={} mode={} {detail}",
            file.display(),
            std::process::id(),
            mode.label(),
        ),
    );
}

/// Run the worker body, converting a panic into a logged error. Unwinding
/// still runs the claim's `Drop`, so an incomplete attempt stays retained.
fn run_queue_maintenance_worker_logged(
    file: &Path,
    token: &str,
    mode: QueueMaintenanceWorkerMode,
) -> Result<()> {
    let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_queue_maintenance_worker(file, token, mode)
    })) {
        Ok(outcome) => outcome,
        Err(payload) => {
            let message = panic_payload_text(payload.as_ref());
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_background_panicked file={} pid={} mode={} panic={}",
                    file.display(),
                    std::process::id(),
                    mode.label(),
                    message.replace('\n', " | "),
                ),
            );
            Err(anyhow::anyhow!(
                "background queue maintenance panicked: {message}"
            ))
        }
    };
    log_queue_maintenance_worker_exit(file, mode, &outcome);
    outcome
}

/// What [`recover_orphaned_queue_maintenance`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanedQueueMaintenanceRecovery {
    /// No request is pending, or a live owner will run it.
    NotOrphaned,
    /// Another owner reserved the request first; it runs it.
    Coalesced { holder_pid: u32 },
    /// This process adopted the orphaned request and ran it to completion.
    Recovered,
}

/// `#deadmaintworker`: adopt and run an admitted maintenance request whose
/// worker is gone.
///
/// Only preflight dispatched maintenance, so a worker that crashed, panicked,
/// or returned an error after claiming its request left the request pending
/// until the next turn's preflight: every session-check in between withheld
/// terminal closeout. A closeout that owns write intent calls this before it
/// takes the apply guard. It reserves the request exactly as a dispatcher
/// does (a dead holder's lease is replaced at once, without the TTL), then
/// runs the same worker body in process: the claim reconnects at a newer
/// generation, resumes the retained intent, and either applies the admitted
/// cut or, when the document has moved on, consumes it as superseded. A live
/// owner is never displaced.
///
/// The caller must not hold the document's apply guard.
pub fn recover_orphaned_queue_maintenance(
    file: &Path,
    source: &str,
) -> Result<OrphanedQueueMaintenanceRecovery> {
    let agent_doc_queue_io::queue_edit_owner::QueueMaintenanceRequestOwner::Orphaned {
        dead_holder,
    } = agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_owner(file)?
    else {
        return Ok(OrphanedQueueMaintenanceRecovery::NotOrphaned);
    };
    let file_text = file.to_string_lossy();
    let token = new_background_queue_maintenance_token();
    if !agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
        &file_text,
        &token,
        std::process::id(),
    )? {
        let holder_pid =
            agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(&file_text)
                .map(|lease| lease.holder_pid)
                .unwrap_or_default();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_orphan_recovery_coalesced file={} source={source} holder_pid={holder_pid}",
                file.display(),
            ),
        );
        return Ok(OrphanedQueueMaintenanceRecovery::Coalesced { holder_pid });
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_maintenance_orphan_adopted file={} source={source} pid={} dead_holder={} dead_holder_pid={}",
            file.display(),
            std::process::id(),
            dead_holder
                .as_ref()
                .map_or("none", |lease| lease.holder.as_str()),
            dead_holder.as_ref().map_or(0, |lease| lease.holder_pid),
        ),
    );
    let outcome =
        run_queue_maintenance_worker_logged(file, &token, QueueMaintenanceWorkerMode::Adopted);
    if outcome.is_err() {
        // A failed detached worker leaves its token behind so its dead PID
        // marks the attempt recoverable. This process stays alive to finish
        // its closeout, so a retained token would read as a live reservation
        // and stall that closeout's own apply guard. Release it: the request
        // row and the retained claim intent keep the attempt visible and
        // recoverable.
        agent_doc_queue_io::queue_edit_owner::clear_queue_edit_owner_lease_if_holder(
            &file_text, &token,
        );
    }
    outcome.map(|()| OrphanedQueueMaintenanceRecovery::Recovered)
}

/// The worker body shared by the detached child and an adopting closeout.
fn run_queue_maintenance_worker(
    file: &Path,
    token: &str,
    mode: QueueMaintenanceWorkerMode,
) -> Result<()> {
    let token = token.to_string();
    let Some(mut queue_mutation) = BackgroundQueueMaintenanceClaim::adopt_reserved(file, &token)?
    else {
        let lease = agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(
            &file.to_string_lossy(),
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_background_adoption_lost file={} pid={} token_matches={} holder_pid={}",
                file.display(),
                std::process::id(),
                lease.as_ref().is_some_and(|lease| lease.holder == token),
                lease
                    .as_ref()
                    .map(|lease| lease.holder_pid)
                    .unwrap_or_default(),
            ),
        );
        eprintln!(
            "[preflight] background maintenance reservation is no longer owned for {} (child_pid={})",
            file.display(),
            std::process::id(),
        );
        return Ok(());
    };
    let detached = mode == QueueMaintenanceWorkerMode::Detached;
    if detached && std::env::var_os(TEST_BACKGROUND_MAINTENANCE_CRASH_AFTER_LOCK_ENV).is_some() {
        // Integration-only crash barrier: process::exit intentionally skips
        // Drop, modeling a hard worker crash after flock acquisition and
        // durable adoption but before any maintenance success/effect.
        std::process::exit(91);
    }
    loop {
        let Some(request) =
            agent_doc_queue_io::queue_edit_owner::load_queue_maintenance_request(file)?
        else {
            return Ok(());
        };
        match queue_mutation.claim_request(&request) {
            Ok(claimed) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "queue_maintenance_background_claimed file={} pid={} generation={} claim={claimed:?}",
                    file.display(),
                    std::process::id(),
                    request.generation,
                ),
            ),
            Err(error)
                if error
                    .downcast_ref::<agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaimSuperseded>()
                    .is_some() =>
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "queue_maintenance_background_claim_superseded file={} pid={} error={}",
                        file.display(),
                        std::process::id(),
                        format!("{error:#}").replace('\n', " | "),
                    ),
                );
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let request_started = std::time::Instant::now();
        let effect_fence = BackgroundRequestEffectFence::new(file, &queue_mutation, &request);
        let result = (|| -> Result<()> {
            // `#implementlazilyowned` plan step 9a: a predecessor that died
            // between its document cut and the effect phase behind it left the
            // phase staged; finish it before this worker's first step.
            agent_doc_preflight_io::queue_maintenance_effect_phase::resume_staged_queue_maintenance_cut(
                file,
                &effect_fence,
            )?;
            effect_fence.fence_effect(file)?;
            if let Some(delay) = std::env::var(TEST_BACKGROUND_PENDING_MAINTENANCE_DELAY_MS_ENV)
                .ok()
                .and_then(|raw| raw.trim().parse::<u64>().ok())
                .filter(|delay| *delay > 0)
            {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "pending_maintenance_background_test_delay file={} delay_ms={delay}",
                        file.display(),
                    ),
                );
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            park_worker_for_test(file)?;
            effect_fence.fence_effect(file)?;
            // `#q-e9483473162d`: the reconcile passes preflight admission no
            // longer runs inline, applied to the admitted cut before pending
            // and queue maintenance read it.
            let reconcile_cut = agent_doc_document_realtime_io::peek_current_document_content(
                file,
                "background_admission_reconcile_cut",
            )?;
            effect_fence.fence_effect(file)?;
            agent_doc_ops_log_io::log_op(
                file,
                &agent_doc_markdown_lossless::shadow_audit_ops_log_line(
                    &reconcile_cut,
                    "preflight_initial",
                ),
            );
            agent_doc_preflight_io::admission_reconcile::run_deferred_admission_reconcile(
                file,
                &reconcile_cut,
                &effect_fence,
            )?;
            effect_fence.fence_effect(file)?;
            // The worker must derive its admitted cut without publishing an
            // authority observation. A human edit can land while a routed
            // resolver is inside controller recovery; its later fence would
            // reject the stale cut, but the resolver's state event would
            // already be durable. `peek` is intrinsically effect-free and the
            // authentic request fence immediately above/below owns rejection.
            let admitted_content = agent_doc_document_realtime_io::peek_current_document_content(
                file,
                "background_queue_maintenance_admitted_cut",
            )?;
            let pending_report = match agent_doc_preflight_io::run_pending_maintenance_fenced(
                file,
                &admitted_content,
                &PREFLIGHT_MAINTENANCE_WRITE_EFFECTS,
                &effect_fence,
                queue_mutation.apply_guard(),
            ) {
                Ok(report) => report,
                Err(error) if !queue_mutation.owns()? => {
                    eprintln!(
                        "[preflight] background pending maintenance stopped after lease loss for {}: {error:#}",
                        file.display(),
                    );
                    return Ok(());
                }
                Err(error) => return Err(error),
            };
            effect_fence.fence_effect(file)?;
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "pending_maintenance_background_completed file={} reordered={} backlog_gated={} review_count={} todo_signals={}",
                    file.display(),
                    pending_report.reordered,
                    pending_report.backlog_gated_count,
                    pending_report.review_count,
                    pending_report.todo_signals.len(),
                ),
            );
            if detached
                && std::env::var_os(TEST_BACKGROUND_MAINTENANCE_PANIC_AFTER_EFFECT_ENV).is_some()
            {
                panic!("injected background maintenance panic after pending effect");
            }
            effect_fence.fence_effect(file)?;
            let queue_authority_hash =
                agent_doc_preflight_io::PendingMaintenanceEffectFence::expected_authority_hash(
                    &effect_fence,
                )
                .context("background maintenance fence lost its authority cut")?;
            let outcome =
            agent_doc_preflight_io::run_queue_maintenance_for_admitted_authority_under_apply_guard(
                file,
                None,
                &queue_authority_hash,
                &effect_fence,
                queue_mutation.apply_guard(),
            );
            match outcome {
                Ok(state) => {
                    if let Some(authoritative) = state.authoritative_content.as_deref() {
                        agent_doc_preflight_io::PendingMaintenanceEffectFence::record_applied_content(
                        &effect_fence,
                        file,
                        authoritative,
                    )?;
                    }
                    effect_fence.fence_effect(file)?;
                    agent_doc_cycle_state_io::record_queue_mirrored_ids_or_warn(
                        file,
                        &state.mirrored_queue_ids,
                        "background_queue_maintenance",
                    );
                    let committed = match request.commit_if_mutated {
                        true => match effect_fence
                            .fence_effect(file)
                            .and_then(|()| agent_doc_commit_io::commit(file))
                        {
                            Ok(committed) => committed,
                            Err(error) => {
                                effect_fence.fence_effect(file)?;
                                agent_doc_ops_log_io::log_op(
                                    file,
                                    &format!(
                                        "queue_maintenance_background_failed file={} stage=commit error={}",
                                        file.display(),
                                        format!("{error:#}").replace('\n', " | "),
                                    ),
                                );
                                return Err(error);
                            }
                        },
                        false => false,
                    };
                    effect_fence.fence_effect(file)?;
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_maintenance_background_completed file={} mutated={} committed={} drainable={}",
                            file.display(),
                            state.maintenance_mutated,
                            committed,
                            state.queue_drainable_head_count,
                        ),
                    );
                    Ok(())
                }
                Err(error)
                    if error
                        .downcast_ref::<agent_doc_preflight_io::QueueMaintenanceEffectSuperseded>()
                        .is_some() =>
                {
                    effect_fence.fence_effect(file)?;
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_maintenance_background_superseded_authority file={} error={}",
                            file.display(),
                            format!("{error:#}").replace('\n', " | "),
                        ),
                    );
                    Ok(())
                }
                Err(error) => {
                    effect_fence.fence_effect(file)?;
                    agent_doc_ops_log_io::log_op(
                        file,
                        &format!(
                            "queue_maintenance_background_failed file={} error={}",
                            file.display(),
                            format!("{error:#}").replace('\n', " | "),
                        ),
                    );
                    Err(error)
                }
            }
        })();
        // dogfood `maintlockhold`: how long this request held the maintenance claim
        // and how many fence observations actually materialized the document.
        let fence_counts = effect_fence.authority.counts();
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "queue_maintenance_background_claim_held file={} pid={} generation={} held_ms={} fence_observations={} fence_materializations={} outcome={} (maintlockhold)",
                file.display(),
                std::process::id(),
                request.generation,
                request_started.elapsed().as_millis(),
                fence_counts.observations,
                fence_counts.materializations,
                if result.is_ok() { "ok" } else { "error" },
            ),
        );
        drop(effect_fence);

        match result {
            Ok(()) => {
                if queue_mutation.complete_request(&request)? {
                    return Ok(());
                }
                // A newer admitted generation arrived while this one ran. The
                // incumbent retains the flock/lease and immediately owns it.
            }
            Err(error) => {
                let current =
                    agent_doc_queue_io::queue_edit_owner::load_queue_maintenance_request(file)?;
                if current.as_ref().is_some_and(|current| current != &request) {
                    continue;
                }
                if error.to_string().contains("authority was superseded")
                    || error.to_string().contains("request was superseded")
                {
                    if queue_mutation.complete_request(&request)? {
                        return Ok(());
                    }
                    continue;
                }
                return Err(error);
            }
        }
    }
}

/// Run the preflight sequence for a session document.
///
/// Steps (in order):
/// 0. Check tmux layout health (`check_layout`)
/// 1. Repair orphaned pending response (`repair::run`)
/// 2. Commit previous cycle (`git::commit`)
/// 3. Check claims log (read + truncate `.agent-doc/claims.log`)
/// 4. Compute diff (`diff::compute`)
/// 5. Read document HEAD from disk
///
/// Outputs JSON to stdout. Progress/diagnostic messages go to stderr.
fn enforce_cycle_completion(
    file: &Path,
    preserve_fresh_owned_pane_reentry: bool,
) -> Result<(bool, bool)> {
    let apply_guard = match agent_doc_queue_io::queue_edit_owner::QueueEditGuard::try_acquire(file)?
    {
        Some(guard) => guard,
        None if !preflight_cycle_completion_may_mutate(file)? => return Ok((false, false)),
        None => acquire_preflight_apply_guard(file)?,
    };
    let repair_effects =
        agent_doc_closeout_runtime_io::RuntimeRepairIoEffects::new(file, &apply_guard)?;
    let session_check_effects =
        agent_doc_closeout_runtime_io::session_check_effects_under_apply_guard(file, &apply_guard)?;
    let mode = if preserve_fresh_owned_pane_reentry {
        agent_doc_preflight_io::PreflightCycleCompletionMode::PreserveFreshOwnedPaneReentry
    } else {
        agent_doc_preflight_io::PreflightCycleCompletionMode::RecoverInterrupted
    };
    agent_doc_preflight_io::enforce_cycle_completion_with_mode(
        file,
        &agent_doc_preflight_runtime_io::preflight_cycle_completion_effects(
            agent_doc_repair_runtime_io::repair_coordinator_effects(
                &repair_effects,
                &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
            ),
            session_check_effects,
        ),
        mode,
    )
}

fn preflight_cycle_completion_may_mutate(file: &Path) -> Result<bool> {
    if agent_doc_document_realtime_io::pending_document_write(file).is_some() {
        return Ok(true);
    }
    if agent_doc_cycle_state_io::load_with_closeout_projection(file)?
        .is_some_and(|state| state.is_open())
    {
        return Ok(true);
    }
    Ok(agent_doc_ops_log_io::detect_write_completed_commit_missing(file)?.is_some())
}

pub(crate) fn preflight_repair_may_mutate(file: &Path) -> Result<bool> {
    Ok(
        agent_doc_repair_io::pending::load_active_pending_response(file)?.is_some()
            || agent_doc_capture_io::load_active(file)?.is_some()
            || agent_doc_cycle_state_io::load_with_closeout_projection(file)?
                .is_some_and(|state| state.is_open()),
    )
}

/// Serialize the short foreground admission transactions without turning a
/// simultaneous preflight into a spurious failed turn. Comprehensive
/// maintenance remains detached; this bounded wait is only for another
/// foreground holder already inside one of the admission's atomic effects.
pub(crate) fn acquire_preflight_apply_guard(
    file: &Path,
) -> Result<agent_doc_queue_io::queue_edit_owner::QueueEditGuard> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1_250);
    loop {
        if let Some(guard) =
            agent_doc_queue_io::queue_edit_owner::QueueEditGuard::try_acquire(file)?
        {
            return Ok(guard);
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "queue mutation deferred: another owner is applying maintenance for {}; retry after it completes",
                file.display()
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub mod progress;
mod run;
pub use run::*;

#[cfg(test)]
mod th {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;
    // The source-repo locator accepts the document's git root when it is the
    // `agent-doc` crate, the `src/agent-doc` dogfood submodule layout, and
    // returns `None` (silent no-op) when no `agent-doc` Cargo.toml is present.
    // #per-cycle-protocol-output-overhead: empty Vec fields must not spend
    // per-cycle context bytes. A healthy/default PreflightOutput omits the empty
    // `claims` and `layout_issues` arrays from its JSON, and still round-trips
    // back to empty Vecs (serde default) so consumers reading the struct are safe.
    pub(crate) struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: parking_lot::MutexGuard<'static, ()>,
    }
    impl EnvGuard {
        pub(crate) fn set(key: &'static str, value: &str) -> Self {
            let lock = agent_doc_harness::prompt_source::TEST_ENV_LOCK.lock();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }
    /// Set up a minimal project directory with .agent-doc/ structure and a git repo.
    pub(crate) fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

        // Initialize a bare git repo so `git commit` doesn't fail fatally.
        Command::new("git")
            .current_dir(dir.path())
            .args(["init"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.email", "test@test.com"])
            .output()
            .ok();
        Command::new("git")
            .current_dir(dir.path())
            .args(["config", "user.name", "Test"])
            .output()
            .ok();

        dir
    }
    pub(crate) fn commit_all(root: &Path, message: &str, commit_date: Option<&str>) {
        Command::new("git")
            .current_dir(root)
            .args(["add", "."])
            .output()
            .unwrap();
        let mut commit = Command::new("git");
        commit
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"]);
        if let Some(date) = commit_date {
            commit
                .env("GIT_COMMITTER_DATE", date)
                .env("GIT_AUTHOR_DATE", date);
        }
        let output = commit.output().unwrap();
        assert!(
            output.status.success(),
            "git commit {message:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    pub(crate) fn initialize_git_head(root: &Path) {
        let readme = root.join("README.md");
        std::fs::write(&readme, "# project\n").unwrap();
        commit_all(root, "initial", None);
    }
    pub(crate) fn write_committed_doc(
        root: &Path,
        rel: &str,
        content: &str,
        message: &str,
        commit_date: Option<&str>,
    ) -> PathBuf {
        let doc = root.join(rel);
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        commit_all(root, message, commit_date);
        doc
    }
    pub(crate) fn write_session_registry(root: &Path, entries: &[(&str, &str, &Path, &str, &str)]) {
        let mut registry = tmux_router::Registry::new();
        for (session_id, pane, file, window, started) in entries {
            registry.insert(
                (*session_id).to_string(),
                tmux_router::RegistryEntry {
                    pane: (*pane).to_string(),
                    pid: 9999,
                    cwd: root.to_string_lossy().to_string(),
                    started: (*started).to_string(),
                    session_id: (*session_id).to_string(),
                    file: file
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                    window: (*window).to_string(),
                    supervisor_instance_id: String::new(),
                },
            );
        }
        agent_doc_session_registry_io::save_in(root, &registry).unwrap();
    }
    pub(crate) fn age_cycle_state(file: &Path, age_secs: u64) {
        agent_doc_cycle_state_io::age_current_cycle_for_tests(file, age_secs).unwrap();
    }
    pub(crate) fn write_cycles_log(doc: &Path, entries: &[agent_doc_ops_log_io::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }
    // #opsproof-samecycle-add: a gated review/backlog item added THIS cycle (its
    // text legitimately cites a shipped dependency commit) must NOT be ops-proof
    // auto-completed on the same cycle it first appears — even though the
    // write/finalize path already re-synced the on-disk snapshot to include it,
    // which defeats the snapshot-only same-cycle guard. cycle_state records the
    // added id; the reap must honor it.
    // #opsproof-falsepos: an open actionable backlog item whose completion
    // marker only describes already-landed *dependency* work (a cited commit
    // hash in mid-sentence prose) must NOT be auto-reaped. Only a marker that is
    // the item's own leading status verb proves the item itself is done.
    // #opsproofgate: a live-verify / operator-drive gate that cites a shipped
    // commit hash (e.g. "Code SHIPPED 1edb20d2") in its text must NOT be
    // auto-completed on evidence=commit — even when it has existed for several
    // cycles (not a same-cycle add). Only an anchored structured ops.log marker
    // driven live by the operator may close it.
    // #opsproof-falsepos: never auto-archive an item on the same cycle it is
    // added. A brand-new add is absent from the cycle-start snapshot, so even a
    // leading-status completion marker must not reap it this cycle.
    pub(crate) fn write_ops_log(dir: &TempDir, body: &str) {
        let logs = dir.path().join(".agent-doc/logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("ops.log"), body).unwrap();
    }
    // --- Fix 5: cross-document sweep ---
    // --- #cce5: resolve_agent_model / short_model_name tests ---
}
#[cfg(test)]
pub(crate) use th::*;

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    use std::io::Write;
    use std::process::Command;
    use tempfile::TempDir;

    #[cfg(unix)]
    #[test]
    fn slow_comprehensive_queue_effect_does_not_block_admission_dispatch() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1"]);
        let started = std::time::Instant::now();

        let pid = spawn_detached_queue_effect(&mut command).unwrap();

        assert!(pid > 0);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "dispatch waited for the comprehensive effect: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn reserved_background_worker_waits_out_transient_apply_lock_contention() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body\n").unwrap();
        let token = "reserved-background-worker";
        assert!(
            agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &doc.to_string_lossy(),
                token,
                std::process::id(),
            )
            .unwrap()
        );

        // Model the exact race from two simultaneous preflights: after the
        // reservation probe unlocks, the other preflight briefly acquires the
        // flock before it observes the live reservation and backs out.
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let lock_path = dir
            .path()
            .join(".agent-doc/locks/queue-apply")
            .join(format!("{document_hash}.lock"));
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();

        let worker_doc = doc.clone();
        let (adopted_tx, adopted_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            adopted_tx
                .send(BackgroundQueueMaintenanceClaim::adopt_reserved(
                    &worker_doc,
                    token,
                ))
                .unwrap();
        });
        assert!(
            adopted_rx
                .recv_timeout(std::time::Duration::from_millis(25))
                .is_err(),
            "worker must still be waiting while the transient flock is held"
        );

        fs2::FileExt::unlock(&lock).unwrap();
        let lease = adopted_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("worker did not adopt after transient contention")
            .unwrap()
            .expect("the exact reserved token should remain adoptable");
        assert!(!lease.claim.retains_work().unwrap());
        drop(lease);
        worker.join().unwrap();
        assert!(
            agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(
                &doc.to_string_lossy()
            )
            .is_none()
        );
    }

    /// `#deadmaintworker`: the 2026-11-15 worker claimed its request, then
    /// exited with no completion line. Its token stayed behind with a dead
    /// PID, the claim kept a retained in-flight attempt, and only the next
    /// preflight could redispatch it. A closeout now adopts the request: it
    /// reserves past the dead holder at once, reconnects the claim at a newer
    /// generation, runs the worker body, and consumes the request.
    #[test]
    fn closeout_adopts_and_runs_a_request_whose_worker_died() {
        use agent_doc_queue_io::queue_edit_owner::{
            QueueMaintenanceRequestOwner, queue_maintenance_request_owner,
        };
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = "body\n";
        std::fs::write(&doc, content).unwrap();
        let doc_text = doc.to_string_lossy().to_string();
        let request = agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
            &doc,
            &agent_doc_hash::content_hash(content),
            false,
        )
        .unwrap();
        let dead_token = format!(
            "{}dead-worker",
            agent_doc_queue_io::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX
        );
        assert!(
            agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &doc_text,
                &dead_token,
                4_000_000,
            )
            .unwrap()
        );
        // The dead worker had reconnected and claimed the request in flight.
        let crashed =
            agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(&doc)
                .unwrap();
        crashed.claim_request(&request).unwrap();
        assert!(matches!(
            queue_maintenance_request_owner(&doc).unwrap(),
            QueueMaintenanceRequestOwner::Orphaned {
                dead_holder: Some(_)
            }
        ));

        let outcome = recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap();

        assert_eq!(outcome, OrphanedQueueMaintenanceRecovery::Recovered);
        assert!(
            !agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_pending(&doc),
            "the adopted request must be consumed"
        );
        assert!(
            agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(&doc_text).is_none(),
            "the dead holder's token must not survive recovery"
        );
        assert!(
            !crashed.is_current_actor().unwrap(),
            "recovery must fence the dead worker's generation"
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_maintenance_orphan_adopted")
                && ops.contains("source=unit_test")
                && ops.contains(&format!("dead_holder={dead_token}"))
                && ops.contains("dead_holder_pid=4000000"),
            "{ops}"
        );
        assert!(
            ops.contains("queue_maintenance_background_exited")
                && ops.contains("mode=adopted outcome=ok"),
            "every worker exit is logged: {ops}"
        );
        assert_eq!(
            recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap(),
            OrphanedQueueMaintenanceRecovery::NotOrphaned
        );
    }

    /// `#implementlazilyowned` plan step 9: a worker that died after its
    /// document cut left the cut and its earlier effect receipts in
    /// `state.db`. Its successor resumes the same request from the receipted
    /// cut instead of rejecting it as a superseded admission, does not repeat
    /// an effect that already has a receipt, and drops the progress when it
    /// consumes the request.
    #[test]
    fn successor_resumes_a_dead_workers_plan_from_its_receipted_cut() {
        use agent_doc_preflight_io::QueueMaintenanceEffectStep;
        use agent_doc_preflight_io::queue_maintenance_effect_cas::QueueMaintenanceEffectPlan;
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let admitted = "body\n";
        std::fs::write(&doc, admitted).unwrap();
        let doc_text = doc.to_string_lossy().to_string();
        let admitted_hash = agent_doc_hash::content_hash(admitted);
        let request = agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
            &doc,
            &admitted_hash,
            false,
        )
        .unwrap();
        let dead_token = format!(
            "{}dead-worker",
            agent_doc_queue_io::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX
        );
        assert!(
            agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &doc_text,
                &dead_token,
                4_000_000,
            )
            .unwrap()
        );
        let crashed =
            agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(&doc)
                .unwrap();
        crashed.claim_request(&request).unwrap();

        // The dead worker's plan: it receipted the queue-maintenance skip
        // evidence, then cut the document, then died before completing.
        let skip_line = agent_doc_queue::backlog_sync::QueueMaintenanceSkip::NoQueueComponent
            .log_line(&doc.display().to_string(), None);
        let plan = QueueMaintenanceEffectPlan::admit(&doc, &admitted_hash);
        let mut receipted_effect_ran = false;
        plan.apply_replayable_effect(
            QueueMaintenanceEffectStep::Ops,
            &format!("ops:{}", agent_doc_hash::content_hash(&skip_line)),
            &|| Ok(admitted_hash.clone()),
            &mut || {
                receipted_effect_ran = true;
                Ok(())
            },
        )
        .unwrap();
        assert!(receipted_effect_ran);
        let cut = "body\nmaintained\n";
        std::fs::write(&doc, cut).unwrap();
        let cut_hash = agent_doc_hash::content_hash(cut);
        plan.record_document_cut(cut_hash.clone(), &|| Ok(cut_hash.clone()))
            .unwrap();
        crashed
            .store_plan_progress(&request, &serde_json::to_string(&plan.progress()).unwrap())
            .unwrap();
        drop(crashed);

        let outcome = recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap();

        assert_eq!(outcome, OrphanedQueueMaintenanceRecovery::Recovered);
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_maintenance_plan_resumed")
                && ops.contains("applied_through=2 receipts=2"),
            "the successor must reload the dead worker's receipts: {ops}"
        );
        assert!(
            ops.contains("queue_maintenance_background_completed")
                && !ops.contains("queue_maintenance_background_superseded_authority"),
            "the receipted cut, not the stale admission, is the resumed authority: {ops}"
        );
        assert!(
            !ops.contains(&skip_line),
            "an effect that already had a receipt ran again: {ops}"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), cut);
        assert!(
            !agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_pending(&doc),
            "the resumed request must be consumed"
        );
        let successor =
            agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(&doc)
                .unwrap();
        assert_eq!(
            successor.load_plan_progress(&request).unwrap(),
            None,
            "consuming the request drops its plan progress"
        );
    }

    /// Publish a request and leave it claimed by a dead worker whose plan
    /// staged `cut` with one ops evidence line and one mirror-provenance write
    /// (`#implementlazilyowned` plan step 9a). Returns the staged line.
    fn crash_after_staging_a_cut(doc: &Path, admitted: &str, cut: &str) -> String {
        use agent_doc_preflight_io::queue_maintenance_effect_cas::{
            QueueMaintenanceEffectPlan, StagedDocumentCut,
        };
        use agent_doc_preflight_io::queue_maintenance_effect_phase::DeferredMaintenanceEffect;
        let doc_text = doc.to_string_lossy().to_string();
        let admitted_hash = agent_doc_hash::content_hash(admitted);
        let request = agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
            doc,
            &admitted_hash,
            false,
        )
        .unwrap();
        let dead_token = format!(
            "{}dead-worker",
            agent_doc_queue_io::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX
        );
        assert!(
            agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &doc_text,
                &dead_token,
                4_000_000,
            )
            .unwrap()
        );
        let crashed =
            agent_doc_queue_io::queue_maintenance_claim::QueueMaintenanceClaim::reconnect(doc)
                .unwrap();
        crashed.claim_request(&request).unwrap();
        let staged_line = format!("staged_cut_effect_test file={}", doc.display());
        let effects = vec![
            DeferredMaintenanceEffect::Ops {
                line: staged_line.clone(),
            },
            DeferredMaintenanceEffect::MirroredQueueHeads {
                identities: vec!["stagedhead".to_string()],
            },
        ];
        let plan = QueueMaintenanceEffectPlan::admit(doc, &admitted_hash);
        plan.stage_cut(StagedDocumentCut {
            cut: agent_doc_hash::content_hash(cut),
            effects: serde_json::to_string(&effects).unwrap(),
        });
        crashed
            .store_plan_progress(&request, &serde_json::to_string(&plan.progress()).unwrap())
            .unwrap();
        drop(crashed);
        staged_line
    }

    fn staged_mirror_provenance(doc: &Path) -> std::collections::HashSet<String> {
        let root = agent_doc_project_root_io::project_root_containing(doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(&root).unwrap();
        agent_doc_sqlite::state_store::load_mirrored_queue_head_identities_from_db(
            &conn,
            &doc.display().to_string(),
        )
        .unwrap()
    }

    /// `#implementlazilyowned` plan step 9a: a worker that died after writing
    /// its document cut but before receipting it is resumed, not refused: the
    /// authority still equals the staged cut, so the successor receipts the
    /// cut, applies the staged effect phase once, and continues from the cut.
    #[test]
    fn successor_finishes_the_effect_phase_of_a_landed_unreceipted_cut() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let admitted = "body\n";
        let cut = "body\nmaintained\n";
        std::fs::write(&doc, admitted).unwrap();
        let staged_line = crash_after_staging_a_cut(&doc, admitted, cut);
        std::fs::write(&doc, cut).unwrap();

        let outcome = recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap();

        assert_eq!(outcome, OrphanedQueueMaintenanceRecovery::Recovered);
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_maintenance_plan_cut_resumed") && ops.contains("effects=2"),
            "the successor must finish the staged phase: {ops}"
        );
        assert_eq!(
            ops.matches(&staged_line).count(),
            1,
            "the staged evidence must land exactly once: {ops}"
        );
        assert!(
            ops.contains("queue_maintenance_background_completed")
                && !ops.contains("queue_maintenance_background_superseded_authority"),
            "the landed cut, not the stale admission, is the resumed authority: {ops}"
        );
        assert!(
            staged_mirror_provenance(&doc).contains("stagedhead"),
            "the staged ledger effect must land"
        );
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), cut);
        assert!(!agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_pending(&doc));
    }

    /// `#implementlazilyowned` plan step 9a: a staged cut whose write never
    /// landed is dropped, its effects never run, and the request runs from its
    /// admission.
    #[test]
    fn successor_drops_the_effect_phase_of_a_cut_that_never_landed() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let admitted = "body\n";
        std::fs::write(&doc, admitted).unwrap();
        let staged_line = crash_after_staging_a_cut(&doc, admitted, "body\nmaintained\n");

        let outcome = recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap();

        assert_eq!(outcome, OrphanedQueueMaintenanceRecovery::Recovered);
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains("queue_maintenance_plan_cut_dropped") && ops.contains("landed=no"),
            "{ops}"
        );
        assert!(
            !ops.contains(&staged_line),
            "a cut that never landed applied its staged effects: {ops}"
        );
        assert!(
            ops.contains("queue_maintenance_background_completed"),
            "{ops}"
        );
        assert!(!staged_mirror_provenance(&doc).contains("stagedhead"));
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), admitted);
    }

    /// `#deadmaintworker`: recovery never displaces a live owner; a live
    /// reservation that is about to adopt keeps the request.
    #[test]
    fn closeout_recovery_leaves_a_live_worker_request_alone() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body\n").unwrap();
        let doc_text = doc.to_string_lossy().to_string();
        agent_doc_queue_io::queue_edit_owner::publish_queue_maintenance_request(
            &doc, "hash", false,
        )
        .unwrap();
        let live_token = format!(
            "{}live-worker",
            agent_doc_queue_io::queue_edit_owner::BACKGROUND_QUEUE_EDIT_OWNER_PREFIX
        );
        assert!(
            agent_doc_queue_io::queue_edit_owner::try_reserve_queue_edit_owner(
                &doc_text,
                &live_token,
                std::process::id(),
            )
            .unwrap()
        );

        assert_eq!(
            recover_orphaned_queue_maintenance(&doc, "unit_test").unwrap(),
            OrphanedQueueMaintenanceRecovery::NotOrphaned
        );
        assert!(agent_doc_queue_io::queue_edit_owner::queue_maintenance_request_pending(&doc));
        assert_eq!(
            agent_doc_queue_io::queue_edit_owner::read_queue_edit_owner_lease(&doc_text)
                .unwrap()
                .holder,
            live_token
        );
    }

    /// `#deadmaintworker`: a worker error is logged with its cause instead of
    /// vanishing into the detached child's `/dev/null` stderr.
    #[test]
    fn worker_exit_line_carries_the_error() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body\n").unwrap();
        log_queue_maintenance_worker_exit(
            &doc,
            QueueMaintenanceWorkerMode::Detached,
            &Err(anyhow::anyhow!("controller read timed out\nsecond line")),
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains(
                "mode=detached outcome=error error=controller read timed out | second line"
            ),
            "{ops}"
        );
    }

    #[test]
    fn pane_authority_precedes_every_preflight_mutation_boundary() {
        let source = include_str!("run.rs");
        let authority = source
            .find("require_pane_authority_in(&preflight_scope")
            .expect("preflight must have a pane-authority admission gate");
        for mutation in [
            "recycle_stale_supervisor_for_turn_stage",
            "ensure_controller_running_for_file",
            "prepare_recoverable_response_replay_recovery_for_file",
            "retire_redundant_doubled_document_write_intents",
            "retain_preflight_controller_projection(",
            "wait_for_lazily_current_before_mutation",
            "enforce_cycle_completion(file",
            "run_pending_maintenance(",
            "start_preflight(file",
        ] {
            let mutation = source
                .find(mutation)
                .unwrap_or_else(|| panic!("missing preflight mutation boundary {mutation}"));
            assert!(
                authority < mutation,
                "pane authority must reject before {mutation} can mutate state"
            );
        }
    }

    #[test]
    fn preflight_output_omits_empty_claims_and_layout_issues() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        assert!(
            !json.contains("\"claims\""),
            "empty claims must be omitted from per-cycle output: {json}"
        );
        assert!(
            !json.contains("\"layout_issues\""),
            "empty layout_issues must be omitted from per-cycle output: {json}"
        );
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert!(round_trip.claims.is_empty());
        assert!(round_trip.layout_issues.is_empty());

        // Non-empty values are still emitted and round-trip intact.
        let populated = PreflightOutput {
            claims: vec!["claimed pane %1".to_string()],
            layout_issues: vec!["stash overflow".to_string()],
            ..PreflightOutput::default()
        };
        let json = serde_json::to_string(&populated).unwrap();
        assert!(json.contains("\"claims\""), "{json}");
        assert!(json.contains("\"layout_issues\""), "{json}");
        let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip.claims, vec!["claimed pane %1".to_string()]);
        assert_eq!(round_trip.layout_issues, vec!["stash overflow".to_string()]);
    }
    #[test]
    fn preflight_detects_diff() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();

        // Save snapshot of original, then add new content.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        std::fs::write(
            &doc,
            "---\nsession: test\n---\n\n## User\n\nHello\n\nNew question here.\n",
        )
        .unwrap();

        // diff::compute should detect changes → no_changes = false.
        let diff_result = agent_doc_diff_io::compute(
            &agent_doc_snapshot_io::DiffBaselineStore::new(agent_doc_ops_log_io::log_op),
            &doc,
        )
        .unwrap();
        assert!(diff_result.is_some(), "diff should detect new content");
    }
    #[test]
    fn append_latest_ipc_dogfood_note_reads_matching_ops_log_entry() {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let canonical = doc.canonicalize().unwrap();
        let diagnostic = format!(
            "ipc_proof_insufficient file={} source=socket_visible_write patch_id=abc invariant=live_prompt_drift_after_preflight recovery=visible_repair_required",
            canonical.display()
        );
        write_ops_log(
            &dir,
            &format!(
                "older irrelevant line\n[2026-06-23T00:00:00Z] {}\n",
                diagnostic
            ),
        );

        // Gated entry point must NOT append into a non-dogfood document (a user
        // doc that merely sits in a superproject): the diagnostic stays in ops.log.
        assert!(!agent_doc_preflight_io::append_latest_ipc_dogfood_note(&doc).unwrap());
        assert!(
            !std::fs::read_to_string(&doc)
                .unwrap()
                .contains("IPC proof issue dogfood log")
        );

        // The underlying appender (reached only for genuine agent-doc dogfood
        // docs) still folds the diagnostic into the exchange and dedups a repeat.
        assert!(
            agent_doc_preflight_io::append_ipc_dogfood_note_for_diagnostic(&doc, &diagnostic)
                .unwrap()
        );
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("IPC proof issue dogfood log"));
        assert!(updated.contains(&diagnostic));

        assert!(
            !agent_doc_preflight_io::append_ipc_dogfood_note_for_diagnostic(&doc, &diagnostic)
                .unwrap()
        );
    }

    /// #drained-done-queue-clear: a standalone no-diff preflight that drains a
    /// fully-resolved auto-queue writes the drained shape to disk + snapshot
    /// but leaves HEAD on the active-queue commit. The next preflight commit
    /// step must self-heal that pure queue-maintenance drift via the route
    /// queue commit-boundary recovery instead of stranding it for manual
    /// `agent-doc commit`. The drained snapshot has no active prompts, so this
    /// shape recovers only because HEAD proves the prior active auto-queue and
    /// nothing but queue state differs.
    #[test]
    fn route_queue_commit_boundary_recovers_drained_queue_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            active,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Standalone maintenance drained the queue: queue_active cleared, auto
        // stripped, body emptied — on disk and in the snapshot — but HEAD still
        // carries the active auto-queue.
        let drained = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            drained,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(active),
            Some(active),
        )
        .unwrap();

        assert!(matches!(
            agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        ));
        let rc = agent_doc_run_context_io::cycle_context(doc.clone());
        assert!(
            agent_doc_preflight_runtime_io::detect_route_queue_snapshot_commit_boundary_recoverable(
                &doc, &rc
            )
            .unwrap(),
            "drained-queue maintenance drift must be recoverable"
        );

        assert!(
            agent_doc_preflight_runtime_io::recover_route_queue_snapshot_commit_boundary(&doc, &rc)
                .unwrap()
        );
        assert!(
            matches!(
                agent_doc_snapshot_io::verify_snapshot_committed(&doc).unwrap(),
                agent_doc_snapshot_io::SnapshotCommitStatus::Committed
            ),
            "drained queue must be committed after recovery"
        );
        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        let head = String::from_utf8_lossy(&show.stdout);
        assert!(head.contains("queue_active: false"), "HEAD: {head}");
        assert!(!head.contains("agent:queue auto"), "HEAD: {head}");
    }
    /// #drained-done-queue-clear guard: the route queue commit-boundary
    /// recovery must NOT fire when a real user edit rides alongside the queue
    /// drain. Only pure queue-state churn is auto-committable.
    #[test]
    fn route_queue_commit_boundary_skips_drained_queue_with_user_edit() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");

        let active = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: true\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue auto -->\n",
            "- do [#alpha]\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, active).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            active,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "active queue", "--no-verify"])
            .output()
            .unwrap();

        // Drained queue PLUS an unrelated exchange edit — must not auto-commit.
        let drained_plus_edit = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "queue_active: false\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n\nAn extra user line.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );
        std::fs::write(&doc, drained_plus_edit).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            drained_plus_edit,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
            &doc,
            "commit_success",
            Some(active),
            Some(active),
        )
        .unwrap();

        let rc = agent_doc_run_context_io::cycle_context(doc.clone());
        assert!(
            !agent_doc_preflight_runtime_io::detect_route_queue_snapshot_commit_boundary_recoverable(
                &doc, &rc
            )
            .unwrap(),
            "a user edit alongside the drain must block auto-commit"
        );
    }
    #[test]
    fn preflight_resumes_commit_when_write_landed_without_open_cycle_state() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
        std::fs::write(&doc, original).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            original,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let patched =
            "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nRecovered answer\n";
        std::fs::write(&doc, patched).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            patched,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let ops = root.join(".agent-doc/logs/ops.log");
        std::fs::write(
            &ops,
            format!(
                "[100] ipc_write_consumed file={} patches=1\n",
                doc.display(),
            ),
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc, false).unwrap();
        assert!(
            !recovered,
            "no replay should be needed when file already has the response"
        );
        assert!(
            committed,
            "commit boundary should resume and create a commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);

        let show = Command::new("git")
            .current_dir(root)
            .args(["show", "HEAD:session.md"])
            .output()
            .unwrap();
        assert!(show.status.success(), "git show HEAD:session.md failed");
        let committed_doc = String::from_utf8_lossy(&show.stdout);
        assert!(
            committed_doc.contains("Recovered answer"),
            "HEAD should include the resumed response closeout:\n{committed_doc}"
        );

        let log = std::fs::read_to_string(ops).unwrap();
        assert!(
            log.contains("resume_commit_success file="),
            "resume commit success should be logged:\n{log}"
        );
    }
    #[test]
    fn archive_pending_done_inserts_canonical_done_component() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:done -->"));
        assert!(archived.contains("<!-- /agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(!archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }
    #[test]
    fn archive_pending_done_ignores_removed_pending_done_alias() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:pending-done -->\n",
            "<!-- /agent:pending-done -->\n"
        );
        std::fs::write(&file, content).unwrap();
        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                text: "completed item".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        assert!(archived.contains("<!-- agent:pending-done -->"));
        assert!(archived.contains("<!-- agent:done -->"));
        assert!(!archived.contains("<!-- agent:backlog-done -->"));
        assert!(archived.contains("[#done1] completed item"));
    }
    #[test]
    fn archive_pending_done_appends_to_external_done_archive() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let content = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=document:session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, content).unwrap();

        let archived = agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            content,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();

        let external = std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert!(external.contains("[#done1] completed externally"));
        assert!(!archived.contains("[#done1]"));
        assert!(archived.contains("completed work archived in document:session.done.md"));

        agent_doc_element_backlog_io::done_archive::archive_pending_done(
            &file,
            &archived,
            &[agent_doc_element_backlog::backlog::PendingItem {
                marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
                id: "done1".to_string(),
                state: agent_doc_element_backlog::backlog::PendingState::Done,
                gate_type: None,
                text: "completed externally".to_string(),
                continuation: String::new(),
            }],
        )
        .unwrap()
        .unwrap();
        let external_after =
            std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
        assert_eq!(external_after.matches("[#done1]").count(), 1);
    }
    #[test]
    fn archive_pending_done_rejects_invalid_external_archive_paths() {
        let dir = setup_project();
        let file = dir.path().join("session.md");
        let item = agent_doc_element_backlog::backlog::PendingItem {
            marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: agent_doc_element_backlog::backlog::PendingState::Done,
            gate_type: None,
            text: "completed item".to_string(),
            continuation: String::new(),
        };
        for archive_path in [
            "/tmp/session.done.md",
            "../session.done.md",
            "tasks/session.md",
        ] {
            let content = format!(
                "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
                archive_path
            );
            std::fs::write(&file, &content).unwrap();
            let err = agent_doc_element_backlog_io::done_archive::archive_pending_done(
                &file,
                &content,
                std::slice::from_ref(&item),
            )
            .unwrap_err()
            .to_string();
            assert!(
                err.contains("agent:done archive="),
                "unexpected error for {archive_path}: {err}"
            );
        }
    }
    #[test]
    fn external_done_archive_ids_satisfy_dropped_history_guard() {
        let dir = setup_project();
        let file = dir.path().join("tasks/session.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            dir.path().join("tasks/session.done.md"),
            "# Agent Doc Completed Work\n\n- 2026-05-13 [#item1] Was open\n",
        )
        .unwrap();
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=document:session.done.md -->\n",
            "<!-- /agent:done -->\n"
        );
        std::fs::write(&file, current).unwrap();

        let external_ids =
            agent_doc_element_backlog_io::done_archive::external_done_archive_ids(&file, current)
                .unwrap();
        let report =
            agent_doc_element_backlog::backlog::detect_dropped_from_history_with_extra_current_ids(
                current,
                baseline,
                &std::collections::HashSet::new(),
                &external_ids,
            )
            .unwrap();

        assert!(report.dropped.is_empty());
    }
    #[test]
    fn preflight_closes_response_captured_cycle_when_snapshot_already_matches_head() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let committed = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        std::fs::write(&doc, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let visible_snapshot = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            visible_snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
        std::fs::write(&doc, &with_user_edit).unwrap();
        agent_doc_cycle_state_io::start_preflight(
            &doc,
            Some(visible_snapshot),
            Some(&with_user_edit),
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &doc,
            "response_captured",
            Some(visible_snapshot),
            Some(&with_user_edit),
            "sha256",
            None,
        )
        .unwrap();

        let (recovered, committed) = enforce_cycle_completion(&doc, false).unwrap();
        assert!(
            recovered,
            "the missing commit boundary should be recovered from already-committed HEAD"
        );
        assert!(
            !committed,
            "HEAD-current closeout should not create a duplicate git commit"
        );

        let state = agent_doc_cycle_state_io::load(&doc).unwrap().unwrap();
        assert_eq!(state.phase, agent_doc_turn::CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_already_current");

        let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            log.contains("commit_already_current file="),
            "preflight should record the no-op closeout instead of failing:\n{log}"
        );
        assert!(
            !log.contains("commit_failed"),
            "preflight should not log a false commit_failed for HEAD-current closeout:\n{log}"
        );
    }
    #[test]
    fn preflight_warns_on_prompt_preset_text_inside_post_exchange_html_comment() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "Scratch note while testing.\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .expect("known prompt preset in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(
            warning
                .message
                .contains("#spec-test-build-install-commit-push")
        );
        assert!(warning.message.contains("non-executable user note"));
    }
    #[test]
    fn preflight_comment_prompt_preset_warning_ignores_agent_components() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:done -->\n",
            "<!-- archived #spec-test-build-install-commit-push -->\n",
            "<!-- /agent:done -->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();

        assert!(
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .is_none(),
            "agent-owned queue directives remain executable state, not ordinary scratch comments"
        );
    }
    #[test]
    fn auto_on_backlog_does_not_activate_queue() {
        // #backlog-auto-marker-misfire regression: the auto-loop reads `auto`
        // only from the queue component, never from the backlog.
        let content = concat!(
            "<!-- agent:queue -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog auto -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = agent_doc_element::element::parse(content).unwrap();
        let queue = components.iter().find(|c| c.name == "queue").unwrap();
        assert!(
            !agent_doc_queue::document_queue::has_auto_attr(&queue.attrs),
            "queue has no auto attribute"
        );
        let backlog = components.iter().find(|c| c.name == "backlog").unwrap();
        assert!(
            agent_doc_queue::document_queue::has_auto_attr(&backlog.attrs),
            "backlog carries the misplaced auto attribute"
        );
        let body = &content[queue.open_end..queue.close_start];
        let entries = agent_doc_queue::document_queue::parse(body).unwrap();
        // Activation is driven solely by the queue component's auto flag.
        let activation =
            agent_doc_queue::document_queue::resolve_activation(&entries, false, false, false);
        assert!(
            !activation.active,
            "backlog `auto` must never activate the auto-loop"
        );
    }
    #[test]
    fn preflight_warns_on_dispatch_text_inside_post_exchange_html_comment_without_presets() {
        let content = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "dispatch #manual-review\n",
            "/clear\n",
            "-->\n",
        );
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        let warning =
            agent_doc_workflow::preflight_policy::post_exchange_comment_prompt_preset_warning(
                "session.md",
                content,
                &fm.prompt_presets,
            )
            .expect("dispatch-looking text in ordinary post-exchange comment should warn");

        assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
        assert!(warning.message.contains("dispatch #manual-review"));
        assert!(warning.message.contains("/clear"));
    }
    #[test]
    fn preflight_preserves_duplicate_prompt_comment_from_snapshot() {
        let dir = setup_project();
        let root = dir.path();
        let doc = root.join("session.md");
        let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
        let snapshot = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );
        std::fs::write(&doc, &snapshot).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &snapshot,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let rc = agent_doc_run_context_io::cycle_context(doc.clone());
        let changed = agent_doc_preflight_runtime_io::remove_post_exchange_duplicate_prompt_comments_for_preflight(
            &doc, &rc,
        )
        .unwrap();

        let file_after = std::fs::read_to_string(&doc).unwrap();
        assert!(
            !changed,
            "preflight cleanup should not rewrite baseline-owned scratch comments"
        );
        assert!(
            file_after.contains(&format!("<!--\n{prompt}\n-->")),
            "preflight must not scrub post-exchange scratch text that already existed in HEAD:\n{file_after}"
        );
    }
    #[test]
    fn preflight_output_serializes_correctly() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec!["foo".to_string()],
            diff: Some("+new line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["recovered"], false);
        assert_eq!(parsed["committed"], true);
        assert_eq!(parsed["claims"][0], "foo");
        assert_eq!(parsed["no_changes"], false);
        assert!(parsed["diff"].as_str().is_some());
        assert!(
            parsed.get("document").is_none(),
            "document field must be absent"
        );
    }

    #[test]
    fn strict_template_response_contract_is_explicit_and_inline_docs_omit_it() {
        let template = concat!(
            "---\n",
            "agent_doc_session: test\n",
            "agent_doc_format: template\n",
            "---\n\n",
            "# Session\n",
        );
        let contract = response_contract_for_content(template)
            .expect("template documents must publish their closeout contract");
        assert_eq!(contract, PreflightResponseContract::strict_template());

        let output = PreflightOutput {
            response_contract: Some(contract),
            ..Default::default()
        };
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["response_contract"]["mode"], "strict_template");
        assert_eq!(json["response_contract"]["required_option"], "--template");
        assert_eq!(
            json["response_contract"]["opening_marker"],
            "<!-- patch:exchange -->"
        );
        assert_eq!(
            json["response_contract"]["closing_marker"],
            "<!-- /patch:exchange -->"
        );
        assert_eq!(json["response_contract"]["plain_stdin_allowed"], false);
        assert_eq!(
            json["response_contract"]["recap"]["opening_marker"],
            "<!-- patch:recap -->"
        );
        assert_eq!(
            json["response_contract"]["recap"]["closing_marker"],
            "<!-- /patch:recap -->"
        );
        assert_eq!(
            json["response_contract"]["recap"]["format_runbook"],
            "runbooks/recap.md"
        );
        assert_eq!(json["response_contract"]["recap"]["required"], true);

        let inline = "---\nagent_doc_session: test\nagent_doc_format: append\n---\n\n# Session\n";
        assert!(response_contract_for_content(inline).is_none());
        let inline_json = serde_json::to_value(PreflightOutput::default()).unwrap();
        assert!(inline_json.get("response_contract").is_none());
    }
    #[test]
    fn preflight_output_includes_orchestration_request() {
        let output = PreflightOutput {
            no_changes: false,
            orchestration_request: Some(agent_doc_diff::OrchestrationRequest {
                mode: agent_doc_diff::OrchestrationRequestMode::Sequential,
                trigger_text: "Synchronous orcestra.".to_string(),
                task_count: 5,
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["orchestration_request"]["mode"], "sequential");
        assert_eq!(parsed["orchestration_request"]["task_count"], 5);
        assert_eq!(
            parsed["orchestration_request"]["trigger_text"],
            "Synchronous orcestra."
        );
    }
    #[test]
    fn preflight_output_omits_orchestration_request_when_absent() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("orchestration_request").is_none(),
            "orchestration_request should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_includes_prompt_presets_requested() {
        let output = PreflightOutput {
            no_changes: false,
            prompt_presets_requested: vec!["#1".to_string(), "release-check".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["prompt_presets_requested"][0], "#1");
        assert_eq!(parsed["prompt_presets_requested"][1], "release-check");
    }
    #[test]
    fn preflight_output_omits_prompt_presets_requested_when_empty() {
        let output = PreflightOutput {
            no_changes: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("prompt_presets_requested").is_none(),
            "prompt_presets_requested should be omitted when empty"
        );
    }
    #[test]
    fn codex_network_access_warning_for_non_codex_harness() {
        let content = "---\nagent_doc_session: test\nagent: opencode\ncodex_network_access: enabled\n---\n\ntest\n";
        let (fm, _) = agent_doc_frontmatter::frontmatter::parse(content).unwrap();
        assert!(
            fm.codex_network_access.is_some(),
            "frontmatter should have codex_network_access"
        );
        let active = "opencode";
        assert_ne!(
            agent_doc_model_tier::canonical_harness_name(active).as_deref(),
            Some("codex"),
            "opencode should not be canonical codex"
        );
        assert!(
            agent_doc_model_tier::canonical_harness_name(active).is_some(),
            "opencode is a known harness"
        );
        let has_guard = agent_doc_model_tier::canonical_harness_name("codex").as_deref()
            == Some("codex")
            && agent_doc_model_tier::canonical_harness_name(active).as_deref() != Some("codex")
            && fm.codex_network_access.is_some();
        assert!(
            has_guard,
            "guard condition should fire for opencode + codex_network_access: enabled"
        );
    }
    #[test]
    fn preflight_output_includes_warnings() {
        let output = PreflightOutput {
            warnings: vec![PreflightWarning {
                code: "harness_mismatch".to_string(),
                message: "Document declares agent: codex but active harness is claude-code."
                    .to_string(),
                document_agent: Some("codex".to_string()),
                active_harness: Some("claude-code".to_string()),
            }],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["warnings"][0]["code"], "harness_mismatch");
        assert_eq!(parsed["warnings"][0]["document_agent"], "codex");
        assert_eq!(parsed["warnings"][0]["active_harness"], "claude-code");
    }
    #[test]
    fn preflight_output_omits_warnings_when_empty() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("warnings").is_none(),
            "warnings should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_null_diff_when_no_changes() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["diff"].is_null());
        assert_eq!(parsed["no_changes"], true);
    }
    #[test]
    fn check_layout_returns_empty_outside_tmux() {
        // When TMUX env var is not set (typical in CI / test), check_layout
        // should return an empty vec silently.
        let _env_guard = agent_doc_test_support::env_lock();
        let saved = std::env::var("TMUX").ok();
        // SAFETY: test is single-threaded; we restore the value immediately after.
        unsafe { std::env::remove_var("TMUX") };
        let issues = check_layout();
        // Restore if it was set.
        if let Some(val) = saved {
            unsafe { std::env::set_var("TMUX", val) };
        }
        assert!(
            issues.is_empty(),
            "expected no issues outside tmux, got: {:?}",
            issues
        );
    }
    #[test]
    fn preflight_output_includes_layout_issues() {
        let output = PreflightOutput {
            layout_issues: vec!["window index 0 missing".to_string()],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["layout_issues"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["layout_issues"][0], "window index 0 missing");
    }
    #[test]
    fn maybe_auto_repair_base_index_noop_without_issue() {
        let dir = tempfile::tempdir().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
        let file = dir.path().join("session.md");
        std::fs::write(&file, "---\n---\n").unwrap();
        let issues: Vec<String> = vec![];
        maybe_auto_repair_base_index(&file, &issues);
        let counter_path = agent_doc_dir.join("state/base-index-repair.count");
        assert!(
            !counter_path.exists(),
            "no counter file should be created when no base-index issue"
        );
    }
    #[test]
    fn detect_duplicate_claims_empty_registry() {
        let registry = tmux_router::Registry::new();
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn detect_duplicate_claims_no_duplicates() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/foo.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/bar.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn detect_duplicate_claims_two_sessions_same_file() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: "tasks/shared.md".to_string(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        let issues = detect_duplicate_claims(&registry);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("duplicate claims"));
        assert!(issues[0].contains("tasks/shared.md"));
        assert!(issues[0].contains("session-a"));
        assert!(issues[0].contains("session-b"));
    }
    #[test]
    fn detect_duplicate_claims_skips_empty_file_entries() {
        let mut registry = tmux_router::Registry::new();
        registry.insert(
            "session-a".to_string(),
            tmux_router::RegistryEntry {
                pane: "%1".to_string(),
                pid: 100,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-a".to_string(),
                file: String::new(), // legacy entry — no file
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        registry.insert(
            "session-b".to_string(),
            tmux_router::RegistryEntry {
                pane: "%2".to_string(),
                pid: 101,
                cwd: "/work".to_string(),
                started: "2026-01-01".to_string(),
                session_id: "session-b".to_string(),
                file: String::new(),
                window: "@1".to_string(),
                supervisor_instance_id: String::new(),
            },
        );
        assert!(detect_duplicate_claims(&registry).is_empty());
    }
    #[test]
    fn is_url_detects_http() {
        assert!(agent_doc_workflow::preflight_policy::is_url(
            "http://example.com"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_url(
            "https://example.com/path"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(
            "../relative/path.md"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(
            "tasks/software/agent-doc.md"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_url(""));
    }
    #[test]
    fn is_html_content_detects_html() {
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "text/html; charset=utf-8"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "text/html"
        ));
        assert!(agent_doc_workflow::preflight_policy::is_html_content(
            "application/xhtml+xml"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_html_content(
            "application/json"
        ));
        assert!(!agent_doc_workflow::preflight_policy::is_html_content(
            "text/plain"
        ));
    }
    #[test]
    fn html_to_markdown_converts_basic_html() {
        let html = "<h1>Title</h1><p>Hello <strong>world</strong>.</p>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Title"), "should contain heading text");
        assert!(md.contains("**world**"), "should convert bold");
    }
    #[test]
    fn html_to_markdown_strips_script_and_style() {
        let html =
            "<p>Visible</p><script>alert('xss')</script><style>.foo{}</style><p>Also visible</p>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Visible"));
        assert!(md.contains("Also visible"));
        assert!(!md.contains("alert"), "script content should be stripped");
        assert!(!md.contains(".foo"), "style content should be stripped");
    }
    #[test]
    fn html_to_markdown_strips_nav_and_footer() {
        let html =
            "<nav><a href='/'>Home</a></nav><main><p>Content</p></main><footer>Copyright</footer>";
        let md = agent_doc_workflow::preflight_policy::html_to_markdown(html);
        assert!(md.contains("Content"));
        assert!(!md.contains("Home"), "nav content should be stripped");
        assert!(
            !md.contains("Copyright"),
            "footer content should be stripped"
        );
    }
    #[test]
    fn url_cache_path_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let p1 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://example.com");
        let p2 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://example.com");
        assert_eq!(p1, p2, "same URL should produce same cache path");

        let p3 =
            agent_doc_workflow::preflight_policy::url_cache_path(dir.path(), "https://other.com");
        assert_ne!(
            p1, p3,
            "different URLs should produce different cache paths"
        );
        assert!(p1.extension().unwrap() == "txt");
    }
    #[test]
    fn preflight_output_includes_diff_type_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+go\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            diff_type: Some("approval".to_string()),
            diff_type_reason: Some("single approval word: \"go\"".to_string()),
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["diff_type"], "approval");
        assert!(parsed["diff_type_reason"].as_str().unwrap().contains("go"));
    }
    #[test]
    fn preflight_output_omits_diff_type_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("diff_type").is_none(),
            "diff_type should be omitted when None"
        );
        assert!(
            parsed.get("diff_type_reason").is_none(),
            "diff_type_reason should be omitted when None"
        );
    }
    #[test]
    fn preflight_output_includes_annotated_diff_when_set() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: true,
            claims: vec![],
            diff: Some("+line\n".to_string()),
            no_changes: false,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: Some("[user+] line".to_string()),
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["annotated_diff"], "[user+] line");
    }
    #[test]
    fn preflight_output_omits_annotated_diff_when_none() {
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("annotated_diff").is_none(),
            "annotated_diff should be omitted when None"
        );
    }
    #[test]
    fn preflight_output_includes_semantic_diff_when_set() {
        let output = PreflightOutput {
            semantic_diff: Some(agent_doc_diff::semantic::SemanticDiffSummary {
                schema_version: 2,
                changed_components: vec!["queue".to_string()],
                node_events: vec![agent_doc_diff::semantic::SemanticNodeEvent {
                    component: "queue".to_string(),
                    node_key: "queue:0:task:0".to_string(),
                    op: "insert".to_string(),
                    tracked_item_id: Some("task".to_string()),
                    before_index: None,
                    after_index: Some(0),
                    before_preview: None,
                    after_preview: Some("- do [#task]".to_string()),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["semantic_diff"]["schema_version"], 2);
        assert_eq!(parsed["semantic_diff"]["changed_components"][0], "queue");
        assert_eq!(parsed["semantic_diff"]["node_events"][0]["op"], "insert");
    }
    #[test]
    fn preflight_output_omits_semantic_diff_when_none() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("semantic_diff").is_none(),
            "semantic_diff should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_includes_inline_annotations() {
        let output = PreflightOutput {
            inline_annotations: vec![
                "This is wrong, fix it".to_string(),
                "Broaden the gate".to_string(),
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let anns = parsed["inline_annotations"].as_array().unwrap();
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0], "This is wrong, fix it");
        assert_eq!(anns[1], "Broaden the gate");
    }
    #[test]
    fn preflight_output_omits_inline_annotations_when_empty() {
        let output = PreflightOutput {
            inline_annotations: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("inline_annotations").is_none(),
            "inline_annotations should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_includes_user_intent_prompt_changes() {
        let output = PreflightOutput {
            user_intent_prompt_changes: vec![
                agent_doc_diff::PromptBearingChange {
                    kind: agent_doc_diff::PromptBearingChangeKind::PromptTarget,
                    text: "❯ Why was this missed?".to_string(),
                },
                agent_doc_diff::PromptBearingChange {
                    kind: agent_doc_diff::PromptBearingChangeKind::ContentEdit,
                    text: "This line should say 503, not 401.".to_string(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let changes = parsed["user_intent_prompt_changes"].as_array().unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0]["kind"], "prompt_target");
        assert_eq!(changes[0]["text"], "❯ Why was this missed?");
        assert_eq!(changes[1]["kind"], "content_edit");
    }
    #[test]
    fn preflight_output_omits_user_intent_prompt_changes_when_empty() {
        let output = PreflightOutput {
            user_intent_prompt_changes: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("user_intent_prompt_changes").is_none(),
            "user_intent_prompt_changes should be omitted when empty"
        );
    }
    #[test]
    fn preflight_output_includes_session_accretion_when_present() {
        let output = PreflightOutput {
            session_accretion: Some(SessionAccretionReport {
                level: SessionAccretionLevel::Warn,
                exchange_lines: 220,
                response_sections: 9,
                recent_committed_cycles: 7,
                recent_noop_closeouts: 2,
                recent_restart_count: 0,
                recent_session_loss_count: 0,
                startup_miss_active: false,
                clear_threshold: 50,
                reasons: vec!["exchange has grown".to_string()],
                guidance: vec!["Run `agent-doc compact session.md --commit`.".to_string()],
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["session_accretion"]["level"], "warn");
        assert_eq!(parsed["session_accretion"]["exchange_lines"], 220);
    }
    #[test]
    fn preflight_output_omits_session_accretion_when_absent() {
        let output = PreflightOutput::default();
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("session_accretion").is_none(),
            "session_accretion should be omitted when absent"
        );
    }
    #[test]
    fn preflight_output_slash_commands_from_diff() {
        // /clear is a built-in command — goes to builtin_commands, not slash_commands
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/clear\n";
        let parsed_cmds = agent_doc_diff::parse_slash_commands_classified(diff);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(diff.to_string()),
            no_changes: false,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: parsed_cmds.skill_commands,
            builtin_commands: parsed_cmds.builtin_commands,
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // /clear is a built-in — appears in builtin_commands, not slash_commands
        assert_eq!(parsed["builtin_commands"][0], "/clear");
        assert!(
            parsed["slash_commands"].is_null()
                || parsed["slash_commands"]
                    .as_array()
                    .is_none_or(|a| a.is_empty())
        );
    }
    #[test]
    fn preflight_output_no_document_field() {
        // The `document` field was removed — it must not appear in serialized JSON.
        // Having it would send full document content to the agent every cycle,
        // wasting tokens on every invocation.
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: None,
            no_changes: true,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed.get("document").is_none(),
            "document key must be absent from preflight JSON — it would waste tokens on every cycle"
        );
    }
    #[test]
    fn preflight_output_no_large_content() {
        // Regression: preflight JSON must not embed document content.
        // Any field containing the full file body would be sent to the agent
        // on every cycle, burning tokens proportional to document size.
        let large_content = "x".repeat(10_000);
        let output = PreflightOutput {
            layout_issues: vec![],
            recovered: false,
            committed: false,
            claims: vec![],
            diff: Some(format!("+{large_content}")), // diff can include content
            no_changes: false,
            linked_changes: vec![],
            diff_type: None,
            diff_type_reason: None,
            annotated_diff: None,
            slash_commands: vec![],
            builtin_commands: vec![],
            ..Default::default()
        };
        let json = serde_json::to_string(&output).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        // Only `diff` may contain the large content (it's the actual user change).
        // No OTHER field should contain it.
        let diff_str = parsed["diff"].as_str().unwrap_or("");
        for (key, val) in parsed.as_object().unwrap() {
            if key == "diff" {
                continue;
            }
            let val_str = val.to_string();
            assert!(
                !val_str.contains(&large_content),
                "field `{key}` contains large content — this would waste tokens on every preflight cycle"
            );
            assert!(
                val_str.len() < 1_000 || key == "annotated_diff",
                "field `{key}` is suspiciously large ({} bytes) — preflight should not embed document content",
                val_str.len()
            );
        }
        // Diff itself is allowed to contain the content
        assert!(diff_str.contains(&large_content));
    }
}
