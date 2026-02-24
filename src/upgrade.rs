use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24 hours

/// Called on startup to print a warning if a newer version is available.
/// Silently returns on any error.
pub fn warn_if_outdated() {
    if let Some(latest) = check_for_update() {
        eprintln!(
            "Warning: {} v{} is available (you have v{}). Run `agent-doc upgrade` to update.",
            CRATE_NAME, latest, CURRENT_VERSION
        );
    }
}

/// The `upgrade` subcommand handler.
pub fn run() -> Result<()> {
    eprintln!("Checking for updates...");
    match check_for_update() {
        Some(latest) => {
            eprintln!(
                "New version available: v{} (current: v{})",
                latest, CURRENT_VERSION
            );
            eprintln!("Attempting upgrade via cargo install...");
            let status = std::process::Command::new("cargo")
                .args(["install", CRATE_NAME])
                .status();
            match status {
                Ok(s) if s.success() => {
                    eprintln!("Successfully upgraded to v{}.", latest);
                    return Ok(());
                }
                _ => {
                    eprintln!("cargo install failed. Trying pip install --upgrade...");
                }
            }
            let status = std::process::Command::new("pip")
                .args(["install", "--upgrade", CRATE_NAME])
                .status();
            match status {
                Ok(s) if s.success() => {
                    eprintln!("Successfully upgraded to v{} via pip.", latest);
                }
                _ => {
                    eprintln!(
                        "Automatic upgrade failed. Please upgrade manually:\n  \
                         cargo install {} --force\n  or\n  pip install --upgrade {}",
                        CRATE_NAME, CRATE_NAME
                    );
                }
            }
        }
        None => {
            eprintln!("You are running the latest version (v{}).", CURRENT_VERSION);
        }
    }
    Ok(())
}

/// Checks with 24h cache, returns latest version if newer than current.
fn check_for_update() -> Option<String> {
    // Try reading from cache first
    if let Some(cached) = read_cache() {
        if version_is_newer(&cached, CURRENT_VERSION) {
            return Some(cached);
        }
        return None;
    }

    // Fetch from network
    let latest = fetch_latest_version(CRATE_NAME)?;
    // Write to cache regardless of whether it's newer
    let _ = write_cache(&latest);
    if version_is_newer(&latest, CURRENT_VERSION) {
        Some(latest)
    } else {
        None
    }
}

fn cache_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".cache/agent-doc/version-cache.json"))
}

fn read_cache() -> Option<String> {
    let path = cache_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let cache: Value = serde_json::from_str(&content).ok()?;
    let timestamp = cache.get("timestamp")?.as_u64()?;
    let version = cache.get("version")?.as_str()?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(timestamp) < CACHE_TTL_SECS {
        Some(version.to_string())
    } else {
        None
    }
}

fn write_cache(version: &str) -> Option<()> {
    let path = cache_path()?;
    fs::create_dir_all(path.parent()?).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    let cache = serde_json::json!({
        "version": version,
        "timestamp": now,
    });
    fs::write(&path, serde_json::to_string_pretty(&cache).ok()?).ok()?;
    Some(())
}

fn fetch_latest_version(crate_name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{}", crate_name);
    let agent = ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(5))
        .timeout_write(std::time::Duration::from_secs(5))
        .build();
    let resp = agent.get(&url).call().ok()?;
    let body: Value = resp.into_json().ok()?;
    let max_version = body
        .pointer("/crate/max_version")?
        .as_str()?
        .to_string();
    Some(max_version)
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<(u64, u64, u64)> {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        Some((
            parts[0].parse().ok()?,
            parts[1].parse().ok()?,
            parts[2].parse().ok()?,
        ))
    };
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_newer_major() {
        assert!(version_is_newer("2.0.0", "1.0.0"));
    }

    #[test]
    fn test_version_newer_minor() {
        assert!(version_is_newer("1.2.0", "1.1.0"));
    }

    #[test]
    fn test_version_newer_patch() {
        assert!(version_is_newer("1.0.2", "1.0.1"));
    }

    #[test]
    fn test_version_same() {
        assert!(!version_is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_version_older_major() {
        assert!(!version_is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn test_version_older_minor() {
        assert!(!version_is_newer("1.0.0", "1.1.0"));
    }

    #[test]
    fn test_version_older_patch() {
        assert!(!version_is_newer("1.0.0", "1.0.1"));
    }

    #[test]
    fn test_version_invalid() {
        assert!(!version_is_newer("abc", "1.0.0"));
        assert!(!version_is_newer("1.0.0", "abc"));
        assert!(!version_is_newer("1.0", "1.0.0"));
    }

    #[test]
    fn test_cache_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = dir.path().join("version-cache.json");

        // Fresh cache (now)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let fresh = serde_json::json!({ "version": "9.9.9", "timestamp": now });
        fs::write(&cache_file, serde_json::to_string(&fresh).unwrap()).unwrap();

        let content = fs::read_to_string(&cache_file).unwrap();
        let cache: Value = serde_json::from_str(&content).unwrap();
        let ts = cache["timestamp"].as_u64().unwrap();
        assert!(now.saturating_sub(ts) < CACHE_TTL_SECS);

        // Stale cache (25 hours ago)
        let stale_ts = now - (25 * 60 * 60);
        let stale = serde_json::json!({ "version": "9.9.9", "timestamp": stale_ts });
        fs::write(&cache_file, serde_json::to_string(&stale).unwrap()).unwrap();

        let content = fs::read_to_string(&cache_file).unwrap();
        let cache: Value = serde_json::from_str(&content).unwrap();
        let ts = cache["timestamp"].as_u64().unwrap();
        assert!(now.saturating_sub(ts) >= CACHE_TTL_SECS);
    }

    #[test]
    fn test_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cache_file = dir.path().join("version-cache.json");

        // Write
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let data = serde_json::json!({ "version": "1.2.3", "timestamp": now });
        fs::write(&cache_file, serde_json::to_string_pretty(&data).unwrap()).unwrap();

        // Read back
        let content = fs::read_to_string(&cache_file).unwrap();
        let parsed: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["version"].as_str().unwrap(), "1.2.3");
        assert_eq!(parsed["timestamp"].as_u64().unwrap(), now);
    }
}
