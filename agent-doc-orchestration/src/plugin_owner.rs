//! Single live IntelliJ plugin-consumer owner lease (#8bfz / #fcconeowner).
//!
//! Each live IntelliJ plugin process registers its own consumer id
//! `jetbrains-<pid>-<uuid>` (see `editors/jetbrains/.../TypingTracker.kt`) and
//! every instance that has the project open watches `.agent-doc/patches/` and
//! consumes patches independently. With `N` windows open that is `N` live
//! consumers applying patches + `saveDocument` for one document — the race
//! behind the File Cache Conflict / cross-buffer drift family. `#fccreap`
//! (0.34.8) reaps *dead*-pid consumer patch files but does nothing about
//! concurrent *live* ones.
//!
//! This module is the single-owner tie-break: exactly one live consumer per
//! document holds the owner lease and applies/saves; non-owners defer. The
//! election is a filesystem lease (mirrors [`crate::drain_owner`]) with an
//! atomic `create_new` (O_EXCL) claim so two instances racing for an unowned or
//! stale lease cannot both win. Ownership is sticky while the owner keeps
//! refreshing (it calls [`try_acquire_plugin_owner`] on every patch event), and
//! self-healing: a stale heartbeat OR a provably-dead owner pid hands ownership
//! to the next live consumer, so closing the owner window never wedges patch
//! application. Closing the owner explicitly via [`release_plugin_owner`] hands
//! ownership over immediately.
//!
//! Fail-open is deliberate: any IO error during election returns "apply" (the
//! pre-#8bfz behavior) so a single-instance setup can never be worse off than
//! before — the lease only *suppresses* redundant non-owners.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Directory (relative to the project root) holding per-document plugin-owner
/// leases. Mirrors the sibling `.agent-doc/drain-owner` sidecar layout.
const PLUGIN_OWNER_DIR: &str = ".agent-doc/plugin-owner";

/// Default lease freshness window. Long enough that a single live owner stays
/// owner across a bursty agent response (patch events arrive well within it),
/// short enough that a window which stops watching the document hands ownership
/// back to another live instance quickly.
const DEFAULT_PLUGIN_OWNER_TTL_SECS: u64 = 30;
const PLUGIN_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_PLUGIN_OWNER_TTL_SECS";

/// Bounded retry budget for resolving `create_new` contention between racing
/// consumers before falling back to fail-open.
const ELECTION_MAX_RETRIES: usize = 4;

/// Persisted plugin-owner lease body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginOwnerLease {
    /// The owning consumer id (`jetbrains-<pid>-<uuid>`).
    pub consumer_id: String,
    /// The owning IntelliJ process pid (used for dead-owner self-heal).
    pub pid: u32,
    /// Unix seconds of the last heartbeat / claim.
    pub heartbeat_secs: u64,
}

/// Resolve the lease TTL, honoring the `AGENT_DOC_PLUGIN_OWNER_TTL_SECS` override.
pub fn plugin_owner_ttl() -> Duration {
    let secs = std::env::var(PLUGIN_OWNER_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_PLUGIN_OWNER_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

/// Pure freshness predicate: a lease is fresh while its heartbeat is within
/// `ttl` of `now`. Side-effect free for deterministic unit tests.
pub fn plugin_owner_lease_is_fresh(heartbeat_secs: u64, now: u64, ttl: Duration) -> bool {
    now.saturating_sub(heartbeat_secs) <= ttl.as_secs()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Compute the plugin-owner lease path for a document. Mirrors
/// [`crate::drain_owner`]: hash the document path and land the sidecar in the
/// nearest ancestor `.agent-doc/` directory.
fn plugin_owner_lease_path(file: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    file.hash(&mut hasher);
    let hash = hasher.finish();
    let mut dir = PathBuf::from(file);
    dir.pop();
    loop {
        if dir.join(".agent-doc").is_dir() {
            return dir.join(PLUGIN_OWNER_DIR).join(format!("{hash:016x}.json"));
        }
        if !dir.pop() {
            let parent = PathBuf::from(file)
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            return parent
                .join(PLUGIN_OWNER_DIR)
                .join(format!("{hash:016x}.json"));
        }
    }
}

fn read_lease_at(path: &Path) -> Option<PluginOwnerLease> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_lease_at(path: &Path, lease: &PluginOwnerLease) -> std::io::Result<()> {
    let body = serde_json::to_string(lease).unwrap_or_default();
    std::fs::write(path, body)
}

/// Read the raw plugin-owner lease for `file` (regardless of freshness).
pub fn read_plugin_owner_lease(file: &str) -> Option<PluginOwnerLease> {
    read_lease_at(&plugin_owner_lease_path(file))
}

/// Real liveness check shared with `#fccreap`'s consumer reaper.
fn pid_is_live(pid: u32) -> bool {
    crate::hooks::pid_is_live(pid)
}

/// `#6b5h`: true when a **live** editor plugin process owns this document — i.e.
/// there is a real editor buffer that the "editor IPC listener active → refuse
/// direct disk write" guard is actually protecting.
///
/// The disk-write guard exists to stop the binary from raw-writing the session
/// document behind a live editor buffer (the `File Cache Conflict` /
/// cross-buffer-drift family). But a pure-CLI session (no JetBrains/VS Code
/// plugin attached) can still observe a *connectable* IPC socket — the project
/// controller hosts a listener that returns error acks with nothing behind it —
/// so [`crate::ipc_socket::is_listener_active`] reports `true` while there is no
/// editor buffer to protect. Every `finalize` then wedges on `no_ack` forever
/// (`#6b5h`) and only `--force-disk` succeeds, because the guard cannot tell a
/// transiently-wedged real editor apart from a CLI-only dead-end listener.
///
/// This is the positive "a real editor is attached" signal that distinguishes
/// the two cases: every shipped plugin maintains a per-document owner lease
/// (`#8bfz`) recording its IDE process pid; a CLI-only session never wrote one.
///
/// Keys off **pid liveness**, not heartbeat freshness: the plugin refreshes the
/// lease heartbeat only on patch events, so an idle editor with the document
/// open keeps a live pid but a stale heartbeat. Gating on freshness would
/// misclassify an idle real editor as CLI-only and route a disk write straight
/// into its live buffer — the exact File Cache Conflict the guard prevents.
///
/// Fail-safe bias: returns `true` (treat as editor-attached → keep the guard)
/// whenever a lease with a live pid exists. Only an absent lease, or a lease
/// whose owner pid is provably dead, reads as "no live editor endpoint".
pub fn live_editor_endpoint_attached(file: &str) -> bool {
    editor_endpoint_attached_for_lease(read_plugin_owner_lease(file), pid_is_live)
}

/// Testable core of [`live_editor_endpoint_attached`]: a live editor is attached
/// iff an owner lease exists and its owner pid is still live. Pure given the
/// injected liveness predicate so the decision is unit-testable without spawning
/// a real IDE process.
pub fn editor_endpoint_attached_for_lease(
    lease: Option<PluginOwnerLease>,
    is_pid_live: impl Fn(u32) -> bool,
) -> bool {
    match lease {
        Some(lease) => is_pid_live(lease.pid),
        None => false,
    }
}

/// Testable election core. Returns `true` if `consumer_id` (running as `pid`)
/// holds or acquires ownership of the document at `path`, `false` if it should
/// defer to a live owner. Side effects (lease read/write/remove) are real but
/// the clock and liveness predicate are injected for deterministic tests.
///
/// Fail-open: any unexpected IO error resolves to `true` (apply) so the gate can
/// never regress a single-instance setup below the pre-#8bfz behavior.
fn try_acquire_plugin_owner_at(
    path: &Path,
    consumer_id: &str,
    pid: u32,
    now: u64,
    ttl: Duration,
    is_pid_live: impl Fn(u32) -> bool,
) -> bool {
    for _ in 0..ELECTION_MAX_RETRIES {
        match read_lease_at(path) {
            Some(lease) if lease.consumer_id == consumer_id => {
                // We already own it — refresh the heartbeat and stay owner. A
                // failed refresh still keeps ownership (fail-open).
                let _ = write_lease_at(
                    path,
                    &PluginOwnerLease {
                        consumer_id: consumer_id.to_string(),
                        pid,
                        heartbeat_secs: now,
                    },
                );
                return true;
            }
            Some(lease) => {
                // Someone else's lease. Defer only while it is fresh AND its pid
                // is live; otherwise the owner is gone — contend for takeover.
                if plugin_owner_lease_is_fresh(lease.heartbeat_secs, now, ttl)
                    && is_pid_live(lease.pid)
                {
                    return false;
                }
                // Stale or dead owner: best-effort remove, then atomically claim.
                if std::fs::remove_file(path).is_err() {
                    // Another consumer may have removed/replaced it first; the
                    // create_new below resolves the race.
                }
            }
            None => {
                // No lease yet — fall through to the atomic claim.
            }
        }

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let lease = PluginOwnerLease {
                    consumer_id: consumer_id.to_string(),
                    pid,
                    heartbeat_secs: now,
                };
                let body = serde_json::to_string(&lease).unwrap_or_default();
                // A short write failure still elects us (fail-open): the worst
                // case is a malformed lease another consumer treats as stale.
                let _ = file.write_all(body.as_bytes());
                return true;
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // Lost the create_new race; re-read and re-evaluate ownership.
                continue;
            }
            Err(_) => {
                // Unexpected IO error — fail open so a patch is never dropped.
                return true;
            }
        }
    }
    // Persistent contention (every retry lost the race): fail open rather than
    // silently defer and drop a patch.
    true
}

/// Claim or refresh the plugin-owner lease for `file`. Returns `true` if this
/// consumer owns the document (apply the patch + `saveDocument`), `false` if a
/// live owner already holds it (defer; leave the patch for the owner).
pub fn try_acquire_plugin_owner(file: &str, consumer_id: &str, pid: u32) -> bool {
    let path = plugin_owner_lease_path(file);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        // Can't even create the lease dir — fail open (apply as before).
        return true;
    }
    try_acquire_plugin_owner_at(
        &path,
        consumer_id,
        pid,
        now_secs(),
        plugin_owner_ttl(),
        pid_is_live,
    )
}

/// Best-effort release of the plugin-owner lease, but only if `consumer_id`
/// still owns it — never stomp a successor that already took over. Called when
/// the owning instance stops watching the document (project/file close).
pub fn release_plugin_owner(file: &str, consumer_id: &str) {
    let path = plugin_owner_lease_path(file);
    match read_lease_at(&path) {
        Some(lease) if lease.consumer_id == consumer_id => {
            if let Err(err) = std::fs::remove_file(&path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!(
                    "[agent-doc] warning: failed to release plugin-owner lease {}: {err}",
                    path.display()
                );
            }
        }
        _ => {
            // Not ours (or already gone) — leave it for the current owner.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_in(dir: &Path) -> String {
        std::fs::create_dir_all(dir.join(".agent-doc")).unwrap();
        let file = dir.join("plan.md");
        std::fs::write(&file, "body").unwrap();
        file.to_string_lossy().to_string()
    }

    fn lease_with_pid(pid: u32) -> PluginOwnerLease {
        PluginOwnerLease {
            consumer_id: format!("jetbrains-{pid}-uuid"),
            pid,
            heartbeat_secs: 0,
        }
    }

    #[test]
    fn editor_endpoint_attached_requires_lease_with_live_pid() {
        // #6b5h: no lease at all → CLI-only session, no editor buffer to protect.
        assert!(!editor_endpoint_attached_for_lease(None, |_| true));
        // Lease present + owner pid live → a real editor is attached.
        assert!(editor_endpoint_attached_for_lease(
            Some(lease_with_pid(4242)),
            |pid| pid == 4242
        ));
        // Lease present but owner pid dead (closed editor) → no live editor.
        assert!(!editor_endpoint_attached_for_lease(
            Some(lease_with_pid(4242)),
            |_| false
        ));
    }

    #[test]
    fn editor_endpoint_attached_ignores_heartbeat_staleness() {
        // #6b5h regression guard: an idle real editor refreshes the lease only on
        // patch events, so a stale heartbeat with a live pid must still read as
        // editor-attached (never route a disk write into a live idle buffer).
        let stale_but_live = PluginOwnerLease {
            consumer_id: "jetbrains-7-uuid".to_string(),
            pid: 7,
            heartbeat_secs: 0, // far in the past relative to any real `now`
        };
        assert!(editor_endpoint_attached_for_lease(
            Some(stale_but_live),
            |pid| pid == 7
        ));
    }

    #[test]
    fn freshness_predicate_uses_ttl_window() {
        let ttl = Duration::from_secs(30);
        assert!(plugin_owner_lease_is_fresh(1_000, 1_000, ttl), "same instant");
        assert!(plugin_owner_lease_is_fresh(1_000, 1_030, ttl), "at the ttl edge");
        assert!(!plugin_owner_lease_is_fresh(1_000, 1_031, ttl), "past the ttl");
        // Clock skew (heartbeat in the future) saturates to fresh.
        assert!(plugin_owner_lease_is_fresh(2_000, 1_000, ttl));
    }

    #[test]
    fn first_consumer_wins_second_defers_while_owner_is_live() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = doc_in(dir.path());
        let path = plugin_owner_lease_path(&file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let ttl = Duration::from_secs(30);
        let alive = |_pid: u32| true;

        // First consumer acquires.
        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-1-aaa", 1, 1_000, ttl, alive));
        // Second consumer defers while the owner stays fresh + live.
        assert!(!try_acquire_plugin_owner_at(&path, "jetbrains-2-bbb", 2, 1_000, ttl, alive));
        // Owner re-acquires (refresh) and keeps ownership.
        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-1-aaa", 1, 1_010, ttl, alive));
        let lease = read_lease_at(&path).unwrap();
        assert_eq!(lease.consumer_id, "jetbrains-1-aaa");
        assert_eq!(lease.heartbeat_secs, 1_010, "owner heartbeat refreshed");
    }

    #[test]
    fn dead_owner_pid_hands_ownership_to_next_consumer() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = doc_in(dir.path());
        let path = plugin_owner_lease_path(&file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let ttl = Duration::from_secs(30);

        // Owner 1 claims while live.
        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-1-aaa", 1, 1_000, ttl, |_| true));
        // Owner 1's pid is now dead; consumer 2 takes over even though the
        // heartbeat is still fresh.
        let only_2_live = |pid: u32| pid == 2;
        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-2-bbb", 2, 1_005, ttl, only_2_live));
        assert_eq!(read_lease_at(&path).unwrap().consumer_id, "jetbrains-2-bbb");
    }

    #[test]
    fn stale_heartbeat_hands_ownership_to_next_consumer() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = doc_in(dir.path());
        let path = plugin_owner_lease_path(&file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let ttl = Duration::from_secs(30);
        let alive = |_pid: u32| true;

        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-1-aaa", 1, 1_000, ttl, alive));
        // Long past the TTL with no refresh: a different consumer takes over even
        // though pid 1 is still live (owner stopped watching the doc).
        assert!(try_acquire_plugin_owner_at(&path, "jetbrains-2-bbb", 2, 9_000, ttl, alive));
        assert_eq!(read_lease_at(&path).unwrap().consumer_id, "jetbrains-2-bbb");
    }

    #[test]
    fn release_only_removes_own_lease() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = doc_in(dir.path());

        assert!(try_acquire_plugin_owner(&file, "jetbrains-1-aaa", 1));
        // A non-owner release is a no-op.
        release_plugin_owner(&file, "jetbrains-2-bbb");
        assert!(read_plugin_owner_lease(&file).is_some(), "non-owner must not release");
        // The owner releases successfully.
        release_plugin_owner(&file, "jetbrains-1-aaa");
        assert!(read_plugin_owner_lease(&file).is_none());
        // Idempotent: releasing an absent lease must not panic.
        release_plugin_owner(&file, "jetbrains-1-aaa");
    }

    #[test]
    fn end_to_end_real_paths_elect_single_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = doc_in(dir.path());
        // First live consumer wins via the public (real-clock) path.
        assert!(try_acquire_plugin_owner(&file, "jetbrains-1-aaa", std::process::id()));
        // A second consumer whose pid is THIS live process defers.
        assert!(!try_acquire_plugin_owner(&file, "jetbrains-2-bbb", std::process::id()));
    }
}
