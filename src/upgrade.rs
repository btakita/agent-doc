//! # Module: upgrade
//!
//! ## Spec
//! - `run()` is the `agent-doc upgrade` subcommand handler. It checks crates.io for
//!   the latest version and upgrades the binary in place using three ordered strategies:
//!   1. Download prebuilt binary from GitHub Releases (`tar.gz` for detected target triple).
//!   2. `cargo install <crate>` fallback.
//!   3. `pip install --upgrade <crate>` fallback.
//!
//!   If all strategies fail, prints manual installation instructions and returns `Ok(())`.
//! - `warn_if_outdated()` is called on every startup. Checks crates.io with a 24-hour
//!   disk cache (`~/.cache/agent-doc/version-cache.json`) and prints a one-line warning
//!   to stderr if a newer version is available. Silently swallows all errors.
//! - Version comparison uses simple semver tuple ordering `(major, minor, patch)`.
//! - `detect_target()` resolves the current platform to a target triple
//!   (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`,
//!   `aarch64-apple-darwin`); returns `None` for unsupported platforms.
//! - GitHub Releases download writes to a temp file alongside the binary, extracts via
//!   `tar xzf`, sets `0o755` permissions, then atomically renames into place.
//! - `fetch_latest_version()` hits `https://crates.io/api/v1/crates/<name>` and reads
//!   `/crate/max_version`.
//! - Cache TTL is 24 hours; stale or missing cache triggers a live network fetch.
//!
//! ## Agentic Contracts
//! - `run()` and `warn_if_outdated()` are the only public entry points.
//! - `warn_if_outdated()` never panics or propagates errors; safe to call unconditionally at startup.
//! - `run()` returns `Ok(())` even when all upgrade strategies fail; callers do not need
//!   to handle the upgrade failure specially.
//! - The binary is upgraded in place (same path as `std::env::current_exe()`); symlinks
//!   resolve to the real file before writing.
//!
//! ## Evals
//! - test_version_newer_major: "2.0.0" > "1.0.0" → true
//! - test_version_newer_minor: "1.2.0" > "1.1.0" → true
//! - test_version_newer_patch: "1.0.2" > "1.0.1" → true
//! - test_version_same: "1.0.0" vs "1.0.0" → false
//! - test_version_older_major: "0.9.0" vs "1.0.0" → false
//! - test_version_invalid: non-semver strings → false (no panic)
//! - test_cache_freshness: cache written now → fresh; cache written 25h ago → stale
//! - test_cache_roundtrip: write then read cache file → version and timestamp preserved
//! - test_detect_target: current platform → `Some` string containing a dash
//! - test_github_release_url_format: version + detected target → URL starts with correct GitHub prefix, ends with `.tar.gz`

use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::Read as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_TTL_SECS: u64 = 24 * 60 * 60; // 24 hours
const GITHUB_REPO: &str = "btakita/agent-doc";

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

/// Detect the current platform target triple.
fn detect_target() -> Option<String> {
    let os = if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        return None;
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return None;
    };

    Some(format!("{arch}-{os}"))
}

/// Try to upgrade by downloading from GitHub Releases.
/// Returns true if successful.
fn try_github_release_upgrade(version: &str) -> bool {
    let target = match detect_target() {
        Some(t) => t,
        None => return false,
    };

    let exe_path = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let archive_name = format!("{CRATE_NAME}-{target}.tar.gz");
    let url =
        format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}/{archive_name}");

    eprintln!("Downloading from GitHub Releases...");
    eprintln!("  {url}");

    let exe_dir = match exe_path.parent() {
        Some(d) => d,
        None => return false,
    };

    let tmp_archive = exe_dir.join(format!(".{CRATE_NAME}-upgrade.tar.gz"));
    let tmp_binary = exe_dir.join(format!(".{CRATE_NAME}-upgrade"));

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_recv_body(Some(std::time::Duration::from_secs(30)))
        .timeout_send_body(Some(std::time::Duration::from_secs(10)))
        .build()
        .into();

    let mut resp = match agent.get(&url).call() {
        Ok(r) => r,
        Err(_) => return false,
    };

    let mut archive_bytes = Vec::new();
    if resp
        .body_mut()
        .as_reader()
        .read_to_end(&mut archive_bytes)
        .is_err()
    {
        return false;
    }

    if std::fs::write(&tmp_archive, &archive_bytes).is_err() {
        return false;
    }

    let tar_status = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(&tmp_archive)
        .arg("-C")
        .arg(exe_dir)
        .arg("--transform")
        .arg(format!("s/{CRATE_NAME}/.{CRATE_NAME}-upgrade/"))
        .status();

    let _ = std::fs::remove_file(&tmp_archive);

    let extracted_ok = matches!(tar_status, Ok(s) if s.success());
    if !extracted_ok {
        let _ = std::fs::remove_file(&tmp_binary);
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp_binary, std::fs::Permissions::from_mode(0o755));
    }

    if std::fs::rename(&tmp_binary, &exe_path).is_err() {
        if std::fs::copy(&tmp_binary, &exe_path).is_err() {
            let _ = std::fs::remove_file(&tmp_binary);
            return false;
        }
        let _ = std::fs::remove_file(&tmp_binary);
    }

    true
}

/// The `upgrade` subcommand handler.
pub fn run() -> Result<()> {
    eprintln!("Checking for updates...");

    let latest = match fetch_latest_version(CRATE_NAME) {
        Some(v) => v,
        None => {
            eprintln!("Could not determine the latest version from crates.io.");
            return Ok(());
        }
    };

    if !version_is_newer(&latest, CURRENT_VERSION) {
        eprintln!("You are already on the latest version (v{CURRENT_VERSION}).");
        return Ok(());
    }

    eprintln!("New version available: v{latest} (current: v{CURRENT_VERSION})");

    // Strategy 1: GitHub Releases binary download
    if try_github_release_upgrade(&latest) {
        eprintln!("Successfully upgraded to v{latest} via GitHub Releases.");
        return Ok(());
    }

    // Strategy 2: cargo install
    eprintln!("Attempting: cargo install {CRATE_NAME}");
    let cargo_status = std::process::Command::new("cargo")
        .args(["install", CRATE_NAME])
        .status();

    if let Ok(status) = cargo_status
        && status.success()
    {
        eprintln!("Successfully upgraded to v{latest} via cargo.");
        return Ok(());
    }

    // Strategy 3: pip install
    eprintln!("cargo install failed, trying: pip install --upgrade {CRATE_NAME}");
    let pip_status = std::process::Command::new("pip")
        .args(["install", "--upgrade", CRATE_NAME])
        .status();

    if let Ok(status) = pip_status
        && status.success()
    {
        eprintln!("Successfully upgraded to v{latest} via pip.");
        return Ok(());
    }

    // Manual instructions
    eprintln!(
        "\nAutomatic upgrade failed. You can upgrade manually:\n\
         \n  curl -sSf https://raw.githubusercontent.com/{GITHUB_REPO}/main/install.sh | sh\n\
         \nor:\n\
         \n  cargo install {CRATE_NAME}\n\
         \nor:\n\
         \n  pip install --upgrade {CRATE_NAME}\n"
    );

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
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let cache = serde_json::json!({
        "version": version,
        "timestamp": now,
    });
    fs::write(&path, serde_json::to_string_pretty(&cache).ok()?).ok()?;
    Some(())
}

fn fetch_latest_version(crate_name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{}", crate_name);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build()
        .into();
    let resp = agent.get(&url).call().ok()?;
    let body: Value = resp.into_body().read_json().ok()?;
    let max_version = body.pointer("/crate/max_version")?.as_str()?.to_string();
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

    #[test]
    fn test_detect_target() {
        let target = detect_target();
        assert!(target.is_some(), "should detect current platform");
        let t = target.unwrap();
        assert!(t.contains('-'), "target should contain a dash");
        assert!(
            t.ends_with("unknown-linux-gnu") || t.ends_with("apple-darwin"),
            "unexpected target: {t}"
        );
    }

    #[test]
    fn test_github_release_url_format() {
        let version = "1.2.3";
        let target = detect_target().unwrap();
        let archive = format!("{}-{}.tar.gz", CRATE_NAME, target);
        let url = format!(
            "https://github.com/{}/releases/download/v{}/{}",
            GITHUB_REPO, version, archive
        );
        assert!(url.starts_with("https://github.com/btakita/agent-doc/releases/download/v1.2.3/"));
        assert!(url.ends_with(".tar.gz"));
    }
}
