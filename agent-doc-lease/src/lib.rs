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
}
