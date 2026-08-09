//! TTL freshness primitives for filesystem leases and request sidecars.
//!
//! This crate intentionally owns only the pure timestamp policy. Domain crates
//! still own their lease bodies, paths, environment knobs, and side effects.

use std::time::Duration;

/// Default tsift local-model lease registry path relative to a project root.
pub const DEFAULT_LOCAL_MODEL_LEASE_REGISTRY_RELATIVE: &str = ".tsift/gpu-lease.json";

/// Returns `true` while `observed_secs` is within `ttl` of `now_secs`.
///
/// Future timestamps are treated as fresh to tolerate small wall-clock skew
/// between cooperating processes.
pub fn timestamp_is_fresh(observed_secs: u64, now_secs: u64, ttl: Duration) -> bool {
    now_secs.saturating_sub(observed_secs) <= ttl.as_secs()
}

/// Build the `tsift local-model lease reap` arg vector.
pub fn local_model_reap_command_args(lease_file: Option<&str>, host: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "local-model".to_string(),
        "lease".to_string(),
        "reap".to_string(),
        "--unload-empty".to_string(),
    ];
    if let Some(path) = lease_file {
        args.push("--lease-file".to_string());
        args.push(path.to_string());
    }
    if let Some(host) = host {
        args.push("--host".to_string());
        args.push(host.to_string());
    }
    args
}

/// Scope key for the queue drain-owner coordination lease (`#kp5z` / `#qflood`).
///
/// Both the in-session loop that claims the lease and the controller that reads
/// it share this dependency-free definition, so ownership policy cannot drift.
pub const DRAIN_OWNER_SCOPE: &str = "queue_drain";

/// Env override for the drain-owner lease TTL.
pub const DRAIN_OWNER_TTL_SECS_ENV: &str = "AGENT_DOC_DRAIN_OWNER_TTL_SECS";

const DEFAULT_DRAIN_OWNER_TTL_SECS: u64 = 90;

/// TTL for the drain-owner lease. It is deliberately short and self-expiring so
/// a stopped loop returns ownership to the unattended drainer.
pub fn drain_owner_ttl() -> Duration {
    let secs = std::env::var(DRAIN_OWNER_TTL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_DRAIN_OWNER_TTL_SECS);
    Duration::from_secs(secs.max(1))
}

/// Env override for the drain-stall grace window.
pub const DRAIN_STALL_GRACE_SECS_ENV: &str = "AGENT_DOC_DRAIN_STALL_GRACE_SECS";

const DEFAULT_DRAIN_STALL_GRACE_SECS: u64 = 600;

/// How long a loop-owned drain lease still counts as evidence the loop is
/// continuing, for the STALL DIAGNOSTIC only (`#queuestallloopfalsepositive`).
///
/// Deliberately NOT [`drain_owner_ttl`]. Those windows answer different
/// questions, and reusing the ownership TTL for both made the diagnostic cry
/// wolf on every healthy cycle:
///
/// - **Ownership** must expire fast (90s) so a stopped loop hands the queue back
///   to the unattended drainer. That is correct and unchanged.
/// - **Continuation** is about whether a loop is still working through the queue.
///   A self-paced loop's cycle legitimately spans minutes — a full verification
///   suite, a build/install, then a scheduler wakeup with jitter — and nothing
///   refreshes the lease mid-cycle.
///
/// Measured on `tasks/agent-doc/agent-doc-bugs2.md` on 2026-08-09: successive
/// continuation-recorded -> next-preflight gaps of 122s and 187s against a 90s
/// TTL, so `queue_stall_detected` fired on three consecutive healthy cycles.
///
/// A genuinely stopped loop still trips the diagnostic once this longer window
/// lapses. Firing late is strictly better than firing every cycle: a warning
/// that is usually wrong trains its reader to ignore it, which costs exactly the
/// real stall it exists to catch.
pub fn drain_stall_grace() -> Duration {
    let secs = std::env::var(DRAIN_STALL_GRACE_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_DRAIN_STALL_GRACE_SECS);
    // Never shorter than ownership: a lease that still confers ownership must
    // always still count as continuation.
    Duration::from_secs(secs.max(1)).max(drain_owner_ttl())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#queuestallloopfalsepositive`: the stall grace must outlast a healthy
    /// self-paced cycle, and can never be shorter than ownership itself.
    #[test]
    fn drain_stall_grace_outlasts_ownership_and_a_healthy_cycle() {
        assert!(
            drain_stall_grace() >= drain_owner_ttl(),
            "a lease that still confers ownership must still count as continuation"
        );
        // The measured healthy gaps that produced the false positives.
        for observed_gap_secs in [122, 187] {
            assert!(
                drain_stall_grace() > Duration::from_secs(observed_gap_secs),
                "a {observed_gap_secs}s cycle gap is healthy, not a stall"
            );
            assert!(
                drain_owner_ttl() < Duration::from_secs(observed_gap_secs),
                "precondition: ownership TTL is shorter than the gap, which is why \
                 reusing it as continuation evidence false-fired"
            );
        }
    }

    #[test]
    fn timestamp_freshness_uses_ttl_window() {
        let ttl = Duration::from_secs(30);
        assert!(timestamp_is_fresh(1_000, 1_000, ttl), "same instant");
        assert!(timestamp_is_fresh(1_000, 1_030, ttl), "at the ttl edge");
        assert!(!timestamp_is_fresh(1_000, 1_031, ttl), "past the ttl");
        assert!(timestamp_is_fresh(2_000, 1_000, ttl), "future timestamp");
    }

    #[test]
    fn reap_command_args_defaults_to_unload_empty_without_optional_flags() {
        let args = local_model_reap_command_args(None, None);
        assert_eq!(
            args,
            vec![
                "local-model".to_string(),
                "lease".to_string(),
                "reap".to_string(),
                "--unload-empty".to_string(),
            ]
        );
    }

    #[test]
    fn reap_command_args_appends_lease_file_and_host_when_given() {
        let args = local_model_reap_command_args(
            Some(DEFAULT_LOCAL_MODEL_LEASE_REGISTRY_RELATIVE),
            Some("http://gpu-box:11434"),
        );
        assert!(args.contains(&"--lease-file".to_string()));
        assert!(args.contains(&DEFAULT_LOCAL_MODEL_LEASE_REGISTRY_RELATIVE.to_string()));
        assert!(args.contains(&"--host".to_string()));
        assert!(args.contains(&"http://gpu-box:11434".to_string()));
        let unload_idx = args.iter().position(|a| a == "--unload-empty").unwrap();
        let host_idx = args.iter().position(|a| a == "--host").unwrap();
        assert!(unload_idx < host_idx);
    }

    #[test]
    fn drain_owner_ttl_is_short_and_positive() {
        let ttl = drain_owner_ttl();
        assert!(ttl >= Duration::from_secs(1));
        assert!(
            ttl <= Duration::from_secs(600),
            "a long-lived drain lease would strand the queue when a loop stops"
        );
    }
}
