//! Binary and installed-editor-plugin release upgrades.
//!
//! Manual mode checks once and reconciles installed editor plugins on every
//! run (GH #107), failing loudly when they cannot be updated. Auto mode is a foreground watcher that checks
//! immediately, polls stable GitHub releases, retries transient failures, and
//! uses a per-user PID lock so only one watcher runs. Release archives are
//! installed only after verification against the release's `SHA256SUMS`.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
pub const DEFAULT_AUTO_INTERVAL_SECS: u64 = 15 * 60;
pub const MIN_AUTO_INTERVAL_SECS: u64 = 60;
const GITHUB_REPO: &str = "btakita/agent-doc";

#[derive(Debug, PartialEq, Eq)]
struct AutoUpgradePlan {
    upgrade_binary: bool,
    reconcile_plugins: bool,
}

#[derive(Debug)]
struct AutoUpgradeState {
    effective_version: String,
    reconciled_release: Option<String>,
}

impl AutoUpgradeState {
    fn new(version: &str) -> Self {
        Self {
            effective_version: version.to_owned(),
            reconciled_release: None,
        }
    }

    fn plan(&self, latest: &str) -> AutoUpgradePlan {
        AutoUpgradePlan {
            upgrade_binary: version_is_newer(latest, &self.effective_version),
            reconcile_plugins: self.reconciled_release.as_deref() != Some(latest),
        }
    }

    fn binary_upgraded(&mut self, version: &str) {
        self.effective_version = version.to_owned();
    }

    fn plugins_reconciled(&mut self, release: &str) {
        self.reconciled_release = Some(release.to_owned());
    }
}

struct AutoUpgradeLock {
    path: PathBuf,
    owner: String,
}

impl Drop for AutoUpgradeLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).ok().as_deref() == Some(self.owner.as_str()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Print a best-effort startup warning at most once per cache TTL.
pub fn warn_if_outdated() {
    if let Some(latest) = check_for_update() {
        eprintln!(
            "Warning: {} v{} is available (you have v{}). Run `agent-doc upgrade` to update.",
            CRATE_NAME, latest, CURRENT_VERSION
        );
    }
}

/// Run one upgrade check, or watch releases continuously when `auto` is true.
pub fn run(auto: bool, interval_seconds: Option<u64>) -> Result<()> {
    if auto {
        run_auto(interval_seconds.unwrap_or(DEFAULT_AUTO_INTERVAL_SECS))
    } else {
        run_once()
    }
}

fn run_once() -> Result<()> {
    eprintln!("Checking for updates...");
    let latest = match fetch_latest_release_version() {
        Ok(version) => version,
        Err(error) => {
            eprintln!("Could not determine the latest version from GitHub Releases: {error:#}");
            return Ok(());
        }
    };
    if version_is_newer(&latest, CURRENT_VERSION) {
        eprintln!("New version available: v{latest} (current: v{CURRENT_VERSION})");
        if !upgrade_binary(&latest) {
            // Never move plugins ahead of a binary that stayed behind.
            print_manual_upgrade_instructions();
            return Ok(());
        }
    } else {
        eprintln!("You are already on the latest version (v{CURRENT_VERSION}).");
    }
    // GH #107: a release can split one fix across the binary and the editor
    // plugin, so the one-shot path reconciles installed plugins exactly like
    // `--auto` does — including when the binary is already current, which is
    // how a workspace left skewed by an older one-shot upgrade gets repaired.
    reconcile_installed_plugins_once(&latest, crate::plugin::update_all_installed)
}

/// Reconcile installed editor plugins for a one-shot upgrade and fail loudly
/// when they cannot be brought to `latest`, so a partially delivered release
/// never reports success (GH #107).
fn reconcile_installed_plugins_once(
    latest: &str,
    reconcile: impl FnOnce() -> Result<usize>,
) -> Result<()> {
    match reconcile() {
        Ok(0) => {
            eprintln!("Installed editor plugins already match v{latest}.");
            Ok(())
        }
        Ok(updated) => {
            eprintln!(
                "Updated {updated} installed editor plugin target(s) to the v{latest} release. \
                 Restart the editor if it does not reload the plugin on its own."
            );
            Ok(())
        }
        Err(error) => bail!(
            "v{latest} is NOT fully installed: installed editor plugin reconciliation failed: \
             {error:#}. The binary and plugin are now skewed; re-run `agent-doc upgrade` to retry \
             the plugin step, or install manually with `agent-doc plugin install <editor>`."
        ),
    }
}

fn run_auto(interval_seconds: u64) -> Result<()> {
    validate_auto_interval(interval_seconds)?;
    let _lock = acquire_auto_upgrade_lock()?;
    let mut state = AutoUpgradeState::new(CURRENT_VERSION);
    eprintln!("Watching stable GitHub releases every {interval_seconds}s (Ctrl-C to stop).");
    loop {
        match fetch_latest_release_version() {
            Ok(latest) => run_auto_cycle(&latest, &mut state),
            Err(error) => {
                eprintln!("Auto-upgrade check failed: {error:#}. Retrying on the next poll.")
            }
        }
        std::thread::sleep(Duration::from_secs(interval_seconds));
    }
}

fn run_auto_cycle(latest: &str, state: &mut AutoUpgradeState) {
    let plan = state.plan(latest);
    if plan.upgrade_binary {
        eprintln!(
            "New version available: v{latest} (effective: v{})",
            state.effective_version
        );
        if upgrade_binary(latest) {
            // Replacing the executable does not update this running process's
            // compile-time version, so remember the effective version in memory.
            state.binary_upgraded(latest);
        } else {
            eprintln!("Binary auto-upgrade failed; retrying on the next poll.");
        }
    }
    if plan.reconcile_plugins {
        match crate::plugin::update_all_installed() {
            Ok(updated) => {
                if updated > 0 {
                    eprintln!("Updated {updated} installed editor plugin target(s).");
                }
                state.plugins_reconciled(latest);
            }
            Err(error) => eprintln!(
                "Installed editor plugin reconciliation failed: {error:#}. Retrying on the next poll."
            ),
        }
    }
}

fn validate_auto_interval(interval_seconds: u64) -> Result<()> {
    if interval_seconds < MIN_AUTO_INTERVAL_SECS {
        bail!(
            "--interval-seconds must be at least {MIN_AUTO_INTERVAL_SECS} to avoid excessive GitHub API traffic"
        );
    }
    Ok(())
}

fn upgrade_binary(version: &str) -> bool {
    match try_github_release_upgrade(version) {
        Ok(()) => {
            eprintln!("Successfully upgraded to v{version} via GitHub Releases.");
            return true;
        }
        Err(error) => eprintln!("GitHub binary upgrade failed: {error:#}"),
    }
    eprintln!("Trying: pip install --upgrade {CRATE_NAME}");
    if std::process::Command::new("pip")
        .args(["install", "--upgrade", CRATE_NAME])
        .status()
        .is_ok_and(|status| status.success())
    {
        if current_executable_reports_version(version) {
            eprintln!("Successfully upgraded to v{version} via pip.");
            return true;
        }
        eprintln!(
            "pip completed but the running executable path does not report v{version}; refusing to mark the upgrade complete"
        );
    }
    false
}

fn version_from_cli_output(output: &str) -> Option<&str> {
    output.split_whitespace().last()
}

fn current_executable_reports_version(expected: &str) -> bool {
    std::env::current_exe()
        .and_then(|path| std::process::Command::new(path).arg("--version").output())
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let stdout = String::from_utf8(output.stdout).ok()?;
            version_from_cli_output(&stdout).map(str::to_owned)
        })
        .as_deref()
        == Some(expected)
}

fn print_manual_upgrade_instructions() {
    eprintln!(
        "\nAutomatic upgrade failed. You can upgrade manually:\n\
         \n  curl -sSf https://raw.githubusercontent.com/{GITHUB_REPO}/main/install.sh | sh\n\
         \nor:\n\
         \n  pip install --upgrade {CRATE_NAME}\n"
    );
}

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

fn release_asset_url(version: &str, asset_name: &str) -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}/{asset_name}")
}

fn download_bytes(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let mut response = agent
        .get(url)
        .header("User-Agent", CRATE_NAME)
        .call()
        .with_context(|| format!("failed to download {url}"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {url}"))?;
    Ok(bytes)
}

fn checksum_from_manifest<'a>(manifest: &'a str, asset_name: &str) -> Option<&'a str> {
    manifest.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let digest = fields.next()?;
        let name = fields.next()?.trim_start_matches('*');
        (fields.next().is_none() && name == asset_name).then_some(digest)
    })
}

fn verify_release_archive(asset_name: &str, bytes: &[u8], manifest: &str) -> Result<()> {
    let expected = checksum_from_manifest(manifest, asset_name)
        .with_context(|| format!("SHA256SUMS has no entry for {asset_name}"))?;
    let actual = agent_doc_hash::bytes_hash(bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        bail!(
            "integrity check failed for {asset_name}: expected {expected}, got {actual}; refusing to install"
        );
    }
    Ok(())
}

fn try_github_release_upgrade(version: &str) -> Result<()> {
    let target = detect_target().context("no prebuilt archive for this platform")?;
    let exe_path = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("failed to resolve the current executable")?;
    let exe_dir = exe_path
        .parent()
        .context("current executable has no parent directory")?;
    let archive_name = format!("{CRATE_NAME}-{target}.tar.gz");
    let archive_url = release_asset_url(version, &archive_name);
    let manifest_url = release_asset_url(version, "SHA256SUMS");
    eprintln!("Downloading from GitHub Releases...\n  {archive_url}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .timeout_send_body(Some(Duration::from_secs(10)))
        .build()
        .into();
    let archive_bytes = download_bytes(&agent, &archive_url)?;
    let manifest_bytes = download_bytes(&agent, &manifest_url)?;
    let manifest = std::str::from_utf8(&manifest_bytes).context("SHA256SUMS is not UTF-8")?;
    verify_release_archive(&archive_name, &archive_bytes, manifest)?;

    let tmp_archive = exe_dir.join(format!(".{CRATE_NAME}-upgrade.tar.gz"));
    let tmp_binary = exe_dir.join(format!(".{CRATE_NAME}-upgrade"));
    fs::write(&tmp_archive, &archive_bytes)
        .with_context(|| format!("failed to write {}", tmp_archive.display()))?;
    let tar_status = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(&tmp_archive)
        .arg("-C")
        .arg(exe_dir)
        .arg("--transform")
        .arg(format!("s/{CRATE_NAME}/.{CRATE_NAME}-upgrade/"))
        .status();
    let _ = fs::remove_file(&tmp_archive);
    let tar_status = tar_status.context("failed to run tar")?;
    if !tar_status.success() {
        let _ = fs::remove_file(&tmp_binary);
        bail!("tar exited with {tar_status}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_binary, fs::Permissions::from_mode(0o755))?;
    }
    if fs::rename(&tmp_binary, &exe_path).is_err() {
        fs::copy(&tmp_binary, &exe_path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                exe_path.display(),
                tmp_binary.display()
            )
        })?;
        let _ = fs::remove_file(&tmp_binary);
    }
    Ok(())
}

fn agent_doc_cache_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("XDG_CACHE_HOME")
        && !path.trim().is_empty()
    {
        return Some(PathBuf::from(path).join("agent-doc"));
    }
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".cache/agent-doc"))
}

fn cache_path() -> Option<PathBuf> {
    Some(agent_doc_cache_dir()?.join("version-cache.json"))
}

fn acquire_auto_upgrade_lock() -> Result<AutoUpgradeLock> {
    let path = agent_doc_cache_dir()
        .context("auto-upgrade requires HOME or XDG_CACHE_HOME")?
        .join("auto-upgrade.lock");
    fs::create_dir_all(path.parent().expect("lock path has parent"))?;
    let owner = std::process::id().to_string();
    for _ in 0..3 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(owner.as_bytes())?;
                file.sync_all()?;
                return Ok(AutoUpgradeLock { path, owner });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read_to_string(&path).unwrap_or_default();
                if existing
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .is_some_and(process_is_alive)
                {
                    bail!(
                        "an agent-doc auto-upgrade watcher is already running (PID {})",
                        existing.trim()
                    );
                }
                let _ = fs::remove_file(&path);
            }
            Err(error) => return Err(error).context("failed to create auto-upgrade lock"),
        }
    }
    bail!("could not acquire the agent-doc auto-upgrade watcher lock")
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > libc::pid_t::MAX as u32 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .any(|field| field == pid.to_string())
        })
}

#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

fn check_for_update() -> Option<String> {
    if let Some(cached) = read_cache() {
        return version_is_newer(&cached, CURRENT_VERSION).then_some(cached);
    }
    let latest = fetch_latest_release_version().ok()?;
    let _ = write_cache(&latest);
    version_is_newer(&latest, CURRENT_VERSION).then_some(latest)
}

fn read_cache() -> Option<String> {
    let content = fs::read_to_string(cache_path()?).ok()?;
    let cache: Value = serde_json::from_str(&content).ok()?;
    let timestamp = cache.get("timestamp")?.as_u64()?;
    let version = cache.get("version")?.as_str()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    (now.saturating_sub(timestamp) < CACHE_TTL_SECS).then(|| version.to_owned())
}

fn write_cache(version: &str) -> Option<()> {
    let path = cache_path()?;
    fs::create_dir_all(path.parent()?).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let cache = serde_json::json!({ "version": version, "timestamp": now });
    fs::write(path, serde_json::to_string_pretty(&cache).ok()?).ok()?;
    Some(())
}

fn release_version_from_response(body: &Value) -> Option<String> {
    let version = body.get("tag_name")?.as_str()?.trim_start_matches('v');
    (!version.is_empty()).then(|| version.to_owned())
}

fn fetch_latest_release_version() -> Result<String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let request = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", CRATE_NAME);
    let request = if let Some(token) = ["GITHUB_TOKEN", "GH_TOKEN"].into_iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        request.header("Authorization", &format!("Bearer {token}"))
    } else {
        request
    };
    let response = request
        .call()
        .context("GitHub latest-release request failed")?;
    let body: Value = response
        .into_body()
        .read_json()
        .context("failed to parse GitHub latest-release response")?;
    release_version_from_response(&body).context("GitHub release has no valid tag_name")
}

fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |version: &str| -> Option<(u64, u64, u64)> {
        let mut parts = version.split('.');
        let parsed = (
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        );
        parts.next().is_none().then_some(parsed)
    };
    matches!((parse(latest), parse(current)), (Some(left), Some(right)) if left > right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_shot_plugin_reconciliation_fails_loudly_on_error() {
        let error = reconcile_installed_plugins_once("0.35.441", || {
            Err(anyhow::anyhow!("JetBrains: download refused"))
        })
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("v0.35.441 is NOT fully installed"),
            "{message}"
        );
        assert!(message.contains("JetBrains: download refused"), "{message}");
    }

    #[test]
    fn one_shot_plugin_reconciliation_runs_and_succeeds() {
        let mut called = false;
        reconcile_installed_plugins_once("0.35.441", || {
            called = true;
            Ok(1)
        })
        .unwrap();
        assert!(
            called,
            "the one-shot upgrade must reconcile installed plugins"
        );
        reconcile_installed_plugins_once("0.35.441", || Ok(0)).unwrap();
    }

    #[test]
    fn version_comparison_orders_semver_triples() {
        assert!(version_is_newer("2.0.0", "1.0.0"));
        assert!(version_is_newer("1.2.0", "1.1.0"));
        assert!(version_is_newer("1.0.2", "1.0.1"));
        assert!(!version_is_newer("1.0.0", "1.0.0"));
        assert!(!version_is_newer("0.9.0", "1.0.0"));
        assert!(!version_is_newer("abc", "1.0.0"));
        assert!(!version_is_newer("1.0", "1.0.0"));
    }

    #[test]
    fn cli_version_parser_reads_the_exact_reported_version() {
        assert_eq!(
            version_from_cli_output("agent-doc 0.35.437\n"),
            Some("0.35.437")
        );
        assert_eq!(version_from_cli_output(""), None);
    }

    #[test]
    fn auto_plan_does_not_repeat_work_in_the_old_running_process() {
        let mut state = AutoUpgradeState::new("1.0.0");
        assert_eq!(
            state.plan("1.1.0"),
            AutoUpgradePlan {
                upgrade_binary: true,
                reconcile_plugins: true,
            }
        );
        state.binary_upgraded("1.1.0");
        state.plugins_reconciled("1.1.0");
        assert_eq!(
            state.plan("1.1.0"),
            AutoUpgradePlan {
                upgrade_binary: false,
                reconcile_plugins: false,
            }
        );
        assert!(state.plan("1.2.0").upgrade_binary);
        assert!(state.plan("1.2.0").reconcile_plugins);
    }

    #[test]
    fn auto_interval_has_a_rate_limit_floor() {
        assert!(validate_auto_interval(MIN_AUTO_INTERVAL_SECS).is_ok());
        assert!(validate_auto_interval(MIN_AUTO_INTERVAL_SECS - 1).is_err());
    }

    #[test]
    fn checksum_manifest_accepts_text_and_binary_markers() {
        let manifest = "abcd  agent-doc-linux.tar.gz\nef01 *agent-doc-macos.tar.gz\n";
        assert_eq!(
            checksum_from_manifest(manifest, "agent-doc-linux.tar.gz"),
            Some("abcd")
        );
        assert_eq!(
            checksum_from_manifest(manifest, "agent-doc-macos.tar.gz"),
            Some("ef01")
        );
        assert_eq!(checksum_from_manifest(manifest, "missing"), None);
    }

    #[test]
    fn archive_verification_fails_closed() {
        let name = "agent-doc-linux.tar.gz";
        let digest = agent_doc_hash::bytes_hash(b"payload");
        let manifest = format!("{digest}  {name}\n");
        verify_release_archive(name, b"payload", &manifest).unwrap();
        assert!(verify_release_archive(name, b"tampered", &manifest).is_err());
        assert!(verify_release_archive(name, b"payload", "").is_err());
    }

    #[test]
    fn detects_supported_target_and_formats_release_url() {
        let target = detect_target().expect("test platform should have a release archive");
        let name = format!("{CRATE_NAME}-{target}.tar.gz");
        let url = release_asset_url("1.2.3", &name);
        assert!(url.starts_with("https://github.com/btakita/agent-doc/releases/download/v1.2.3/"));
        assert!(url.ends_with(".tar.gz"));
    }

    #[test]
    fn release_version_normalizes_a_leading_v() {
        assert_eq!(
            release_version_from_response(&serde_json::json!({ "tag_name": "v1.2.3" })).as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn stale_lock_owned_by_dead_pid_is_recoverable() {
        assert!(!process_is_alive(u32::MAX));
    }

    #[test]
    fn lock_guard_only_removes_its_own_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        fs::write(&path, "other").unwrap();
        drop(AutoUpgradeLock {
            path: path.clone(),
            owner: "owner".to_string(),
        });
        assert_eq!(fs::read_to_string(path).unwrap(), "other");
    }
}
