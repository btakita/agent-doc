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

#[cfg(test)]
mod tests {
    use super::*;

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
