//! Binary and installed-editor-plugin release upgrades.
//!
//! Manual mode checks once and reconciles installed editor plugins on every
//! run (GH #107), failing loudly when they cannot be updated. Auto mode is a foreground watcher that checks
//! immediately, polls stable GitHub releases, retries transient failures, and
//! uses a per-user PID lock so only one watcher runs. Release archives are
//! installed only after verification against the release's `SHA256SUMS`.
//!
//! GH #113: replacing the executable on disk does not replace the running
//! process image, so after a binary upgrade the plugin reconcile runs as a child
//! of the freshly installed executable (`upgrade --reconcile-plugins-release
//! <VERSION>`), pinned to the installed release. Otherwise every fix a release
//! makes to the plugin-install path would be skipped by the upgrade delivering it.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
pub const DEFAULT_AUTO_INTERVAL_SECS: u64 = 15 * 60;
pub const MIN_AUTO_INTERVAL_SECS: u64 = 60;
const GITHUB_REPO: &str = "btakita/agent-doc";

/// GH #113: hidden `upgrade` flag that runs only the installed-plugin reconcile,
/// pinned to the given release. This is a cross-release contract: the binary an
/// upgrade installs is invoked with it by the binary being replaced, so future
/// releases must keep accepting it (and [`RECONCILE_PLUGINS_MODE_FLAG`]).
pub const RECONCILE_PLUGINS_RELEASE_FLAG: &str = "--reconcile-plugins-release";
pub const RECONCILE_PLUGINS_MODE_FLAG: &str = "--reconcile-plugins-mode";

/// Which upgrade flavor a plugin reconcile reports for: the one-shot command
/// fails loudly, the `--auto` watcher retries on its next poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum ReconcileMode {
    Once,
    Auto,
}

impl ReconcileMode {
    fn as_arg(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct AutoUpgradePlan {
    upgrade_binary: bool,
    reconcile_plugins: bool,
}

#[derive(Debug)]
struct AutoUpgradeState {
    effective_version: String,
    reconciled_release: Option<String>,
    /// The executable a previous cycle installed. Once set, this watcher's own
    /// image is stale for the rest of its life, so every later reconcile runs
    /// in this executable instead (GH #113).
    upgraded_exe: Option<PathBuf>,
}

impl AutoUpgradeState {
    fn new(version: &str) -> Self {
        Self {
            effective_version: version.to_owned(),
            reconciled_release: None,
            upgraded_exe: None,
        }
    }

    fn plan(&self, latest: &str) -> AutoUpgradePlan {
        AutoUpgradePlan {
            upgrade_binary: version_is_newer(latest, &self.effective_version),
            reconcile_plugins: self.reconciled_release.as_deref() != Some(latest),
        }
    }

    fn binary_upgraded(&mut self, version: &str, installed_exe: PathBuf) {
        self.effective_version = version.to_owned();
        self.upgraded_exe = Some(installed_exe);
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
///
/// `reconcile_plugins_release` is the hidden GH #113 child entry point: reconcile
/// installed editor plugins against that release in THIS (freshly installed)
/// process image, and do nothing else.
pub fn run(
    auto: bool,
    interval_seconds: Option<u64>,
    reconcile_plugins_release: Option<&str>,
    reconcile_mode: Option<ReconcileMode>,
) -> Result<()> {
    if let Some(release) = reconcile_plugins_release {
        crate::plugin::set_release_pin(Some(release));
        return reconcile_plugins_for_mode(release, reconcile_mode.unwrap_or(ReconcileMode::Once));
    }
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
    let upgraded_exe = if version_is_newer(&latest, CURRENT_VERSION) {
        eprintln!("New version available: v{latest} (current: v{CURRENT_VERSION})");
        let Some(installed_exe) = upgrade_binary(&latest) else {
            // Never move plugins ahead of a binary that stayed behind.
            print_manual_upgrade_instructions();
            return Ok(());
        };
        Some(installed_exe)
    } else {
        eprintln!("You are already on the latest version (v{CURRENT_VERSION}).");
        None
    };
    // GH #107: a release can split one fix across the binary and the editor
    // plugin, so the one-shot path reconciles installed plugins exactly like
    // `--auto` does — including when the binary is already current, which is
    // how a workspace left skewed by an older one-shot upgrade gets repaired.
    // GH #113: after a replacement it runs in the new executable's image.
    crate::plugin::set_release_pin(Some(&latest));
    reconcile_plugins_in_image(
        upgraded_exe.as_deref(),
        &latest,
        ReconcileMode::Once,
        || reconcile_plugins_for_mode(&latest, ReconcileMode::Once),
    )
}

/// Reconcile installed editor plugins in this process image, reporting the way
/// `mode`'s upgrade flavor does.
fn reconcile_plugins_for_mode(release: &str, mode: ReconcileMode) -> Result<()> {
    match mode {
        ReconcileMode::Once => {
            reconcile_installed_plugins_once(release, crate::plugin::update_all_installed)
        }
        ReconcileMode::Auto => {
            reconcile_installed_plugins_auto(release, crate::plugin::update_all_installed)
        }
    }
}

/// GH #113: run the plugin reconcile in the process image that matches the
/// installed release. With no replacement this image IS that release, so
/// `in_process` runs. After a replacement the freshly installed executable is
/// spawned (stdio inherited) and its exit status propagates; only when it
/// cannot be launched at all does `in_process` run, behind a warning that the
/// OLD release's plugin code is what ran.
fn reconcile_plugins_in_image(
    upgraded_exe: Option<&Path>,
    release: &str,
    mode: ReconcileMode,
    in_process: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let Some(exe) = upgraded_exe else {
        return in_process();
    };
    eprintln!(
        "Reconciling installed editor plugins with the upgraded v{release} binary ({}).",
        exe.display()
    );
    match std::process::Command::new(exe)
        .args(reconcile_child_args(release, mode))
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => bail!(
            "the installed editor plugin reconcile run by the upgraded v{release} binary ({}) \
             failed ({status}); see its output above",
            exe.display()
        ),
        Err(error) => {
            eprintln!("{}", child_spawn_fallback_warning(exe, release, &error));
            in_process()
        }
    }
}

fn reconcile_child_args(release: &str, mode: ReconcileMode) -> [String; 5] {
    [
        "upgrade".to_owned(),
        RECONCILE_PLUGINS_RELEASE_FLAG.to_owned(),
        release.to_owned(),
        RECONCILE_PLUGINS_MODE_FLAG.to_owned(),
        mode.as_arg().to_owned(),
    ]
}

fn child_spawn_fallback_warning(exe: &Path, release: &str, error: &std::io::Error) -> String {
    format!(
        "WARNING: could not launch the upgraded v{release} binary at {} ({error}); reconciling \
         installed editor plugins in this process instead, which runs the OLD v{CURRENT_VERSION} \
         plugin-install code. Plugin fixes shipped in v{release} did not apply to this run; \
         re-run `agent-doc upgrade` to reconcile with the new binary.",
        exe.display()
    )
}

/// `--auto`'s reconcile reporting: success is quiet unless something changed,
/// and failure propagates to the watcher, which retries on its next poll.
/// GH #114: the auto watcher reports only cycles that changed something, from
/// the same per-target outcomes as the one-shot summary.
fn reconcile_installed_plugins_auto(
    release: &str,
    reconcile: impl FnOnce() -> Result<crate::plugin::PluginReconcileReport>,
) -> Result<()> {
    let report = reconcile()?;
    if report.changed() > 0 {
        crate::plugin::report_reconcile_summary(&report, release);
    }
    Ok(())
}

/// Reconcile installed editor plugins for a one-shot upgrade and fail loudly
/// when they cannot be brought to `latest`, so a partially delivered release
/// never reports success (GH #107).
///
/// GH #114: the closing summary is derived from per-target outcomes, so a
/// staged target is reported as needing a restart rather than as updated.
fn reconcile_installed_plugins_once(
    latest: &str,
    reconcile: impl FnOnce() -> Result<crate::plugin::PluginReconcileReport>,
) -> Result<()> {
    match reconcile() {
        Ok(report) => {
            crate::plugin::report_reconcile_summary(&report, latest);
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
        if let Some(installed_exe) = upgrade_binary(latest) {
            // Replacing the executable does not update this running process's
            // compile-time version, so remember the effective version in memory.
            state.binary_upgraded(latest, installed_exe);
        } else {
            eprintln!("Binary auto-upgrade failed; retrying on the next poll.");
        }
    }
    if plan.reconcile_plugins {
        crate::plugin::set_release_pin(Some(latest));
        match reconcile_plugins_in_image(
            state.upgraded_exe.as_deref(),
            latest,
            ReconcileMode::Auto,
            || reconcile_plugins_for_mode(latest, ReconcileMode::Auto),
        ) {
            Ok(()) => state.plugins_reconciled(latest),
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

/// Install `version` and return the exact executable path that now holds it
/// (GH #113: the plugin reconcile is spawned from that path, never a PATH
/// lookup that could resolve to a different install).
fn upgrade_binary(version: &str) -> Option<PathBuf> {
    match try_github_release_upgrade(version) {
        Ok(installed_exe) => {
            eprintln!("Successfully upgraded to v{version} via GitHub Releases.");
            return Some(installed_exe);
        }
        Err(error) => eprintln!("GitHub binary upgrade failed: {error:#}"),
    }
    eprintln!("Trying: pip install --upgrade {CRATE_NAME}");
    if std::process::Command::new("pip")
        .args(["install", "--upgrade", CRATE_NAME])
        .status()
        .is_ok_and(|status| status.success())
    {
        if let Some(installed_exe) = current_executable_reporting_version(version) {
            eprintln!("Successfully upgraded to v{version} via pip.");
            return Some(installed_exe);
        }
        eprintln!(
            "pip completed but the running executable path does not report v{version}; refusing to mark the upgrade complete"
        );
    }
    None
}

fn version_from_cli_output(output: &str) -> Option<&str> {
    output.split_whitespace().last()
}

/// The current executable path, if running it reports exactly `expected`.
fn current_executable_reporting_version(expected: &str) -> Option<PathBuf> {
    let path = std::env::current_exe().ok()?;
    let output = std::process::Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let stdout = String::from_utf8(output.stdout).ok()?;
    (version_from_cli_output(&stdout) == Some(expected)).then_some(path)
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

/// Replace the running executable's file with `version` and return its
/// canonical path. The path is resolved BEFORE the replacement: on Linux,
/// `current_exe()` afterwards names the unlinked old inode (`... (deleted)`).
fn try_github_release_upgrade(version: &str) -> Result<PathBuf> {
    let target = detect_target().context("no prebuilt archive for this platform")?;
    let exe_path = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("failed to resolve the current executable")?;
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

    install_release_archive(&archive_bytes, &exe_path, version)?;
    Ok(exe_path)
}

/// Install a verified release archive beside `exe_path` (GH #132).
///
/// The archive was previously extracted straight into the install directory,
/// so tar wrote `libagent_doc.so` as a PLAIN FILE over the `lib-install`
/// symlink — non-atomically, with no versioned copy — and `gc-libs` could never
/// match "the installed library" again. Now everything is extracted into a
/// private staging directory on the same filesystem, the cdylib is installed
/// through [`crate::lib_install::install_versioned`] (versioned file + atomic
/// symlink swap, which also migrates a plain-file canonical library by
/// `rename(2)` over it, so there is never a moment with no library), the binary
/// is renamed into place, and superseded libraries are reaped. A reap failure
/// only warns; it never fails an install that already succeeded.
fn install_release_archive(archive_bytes: &[u8], exe_path: &Path, version: &str) -> Result<()> {
    let exe_dir = exe_path
        .parent()
        .context("current executable has no parent directory")?;
    let staging = exe_dir.join(format!(".{CRATE_NAME}-upgrade-{}.d", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to clear {}", staging.display()))?;
    }
    fs::create_dir(&staging).with_context(|| format!("failed to create {}", staging.display()))?;
    let outcome = install_from_staging(archive_bytes, exe_path, exe_dir, &staging, version);
    let _ = fs::remove_dir_all(&staging);
    let library_installed = outcome?;
    if library_installed {
        crate::lib_gc::gc_libs_after_install(exe_dir, "upgrade");
    }
    Ok(())
}

/// Returns whether the archive carried a shared library that was installed.
fn install_from_staging(
    archive_bytes: &[u8],
    exe_path: &Path,
    exe_dir: &Path,
    staging: &Path,
    version: &str,
) -> Result<bool> {
    let tmp_archive = staging.join("release.tar.gz");
    fs::write(&tmp_archive, archive_bytes)
        .with_context(|| format!("failed to write {}", tmp_archive.display()))?;
    let tar_status = std::process::Command::new("tar")
        .args(["xzf"])
        .arg(&tmp_archive)
        .arg("-C")
        .arg(staging)
        .status()
        .context("failed to run tar")?;
    if !tar_status.success() {
        bail!("tar exited with {tar_status}");
    }
    let staged_binary = staging.join(CRATE_NAME);
    if !staged_binary.is_file() {
        bail!("release archive is missing {CRATE_NAME}");
    }

    // Library first: a library failure aborts before the binary moves, so the
    // install is never left as a new binary paired with an old library.
    let staged_library = staging.join(crate::lib_install::platform_lib_name());
    let library_installed = if staged_library.is_file() {
        let installed = crate::lib_install::install_versioned(&staged_library, exe_dir, version)
            .context("failed to install the release shared library")?;
        eprintln!(
            "[upgrade] {} -> {} (symlink: {})",
            crate::lib_install::platform_lib_name(),
            installed.display(),
            crate::lib_install::platform_lib_name(),
        );
        true
    } else {
        false
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_binary, fs::Permissions::from_mode(0o755))?;
    }
    if fs::rename(&staged_binary, exe_path).is_err() {
        fs::copy(&staged_binary, exe_path).with_context(|| {
            format!(
                "failed to replace {} with {}",
                exe_path.display(),
                staged_binary.display()
            )
        })?;
    }
    Ok(library_installed)
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

    /// A gzip tarball shaped like a release asset: `agent-doc` + the cdylib.
    #[cfg(unix)]
    fn release_archive(dir: &Path, binary: &str, library: Option<&str>) -> Vec<u8> {
        let src = dir.join("archive-src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join(CRATE_NAME), binary).unwrap();
        let mut members = vec![CRATE_NAME.to_string()];
        if let Some(library) = library {
            let lib_name = crate::lib_install::platform_lib_name();
            fs::write(src.join(lib_name), library).unwrap();
            members.push(lib_name.to_string());
        }
        let archive = dir.join("release.tar.gz");
        let status = std::process::Command::new("tar")
            .arg("czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .args(&members)
            .status()
            .unwrap();
        assert!(status.success());
        fs::read(archive).unwrap()
    }

    #[cfg(unix)]
    fn install_dir(tmp: &Path) -> (PathBuf, PathBuf) {
        let bin = tmp.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(CRATE_NAME);
        fs::write(&exe, "old binary").unwrap();
        (bin, exe)
    }

    /// GH #132: the release path must produce the same shape as `lib-install`
    /// — a versioned library plus a canonical symlink — not a plain file.
    #[cfg(unix)]
    #[test]
    fn release_install_writes_versioned_library_and_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, exe) = install_dir(tmp.path());
        let archive = release_archive(tmp.path(), "new binary", Some("new library"));

        install_release_archive(&archive, &exe, "9.9.9").unwrap();

        let versioned = bin.join(crate::lib_install::versioned_lib_name("9.9.9"));
        let canonical = bin.join(crate::lib_install::platform_lib_name());
        assert_eq!(fs::read_to_string(&versioned).unwrap(), "new library");
        assert!(
            canonical.is_symlink(),
            "canonical library must be a symlink"
        );
        assert_eq!(
            fs::read_link(&canonical).unwrap(),
            PathBuf::from(crate::lib_install::versioned_lib_name("9.9.9"))
        );
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new binary");
        let leftovers: Vec<_> = fs::read_dir(&bin)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "staging debris: {leftovers:?}");
    }

    /// An install upgraded by the pre-fix path holds a PLAIN `libagent_doc.so`.
    /// The next upgrade must migrate it to the symlink layout and reap the
    /// superseded, unheld versioned pile — while a library a live process
    /// holds survives.
    #[cfg(unix)]
    #[test]
    fn release_install_migrates_plain_file_library_and_reaps_superseded() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, exe) = install_dir(tmp.path());
        let canonical = bin.join(crate::lib_install::platform_lib_name());
        fs::write(&canonical, "plain old library").unwrap();
        let stale = bin.join(crate::lib_install::versioned_lib_name("1.0.0"));
        fs::write(&stale, "v1").unwrap();
        let held = bin.join(crate::lib_install::versioned_lib_name("1.5.0"));
        fs::write(&held, "v1.5").unwrap();
        let held_lock = bin.join(format!(
            "{}.pid.{}",
            crate::lib_install::versioned_lib_name("1.5.0"),
            std::process::id()
        ));
        fs::write(&held_lock, "").unwrap();
        let archive = release_archive(tmp.path(), "new binary", Some("new library"));

        install_release_archive(&archive, &exe, "2.0.0").unwrap();

        assert!(canonical.is_symlink(), "plain file was not migrated");
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "new library");
        assert!(
            !stale.exists(),
            "auto gc-libs did not reap the superseded library"
        );
        assert!(
            held.exists(),
            "auto gc-libs reaped a library a live process holds"
        );
        assert!(held_lock.exists());
    }

    /// An archive without a library (a binary-only asset) still installs the
    /// binary and leaves the existing library alone.
    #[cfg(unix)]
    #[test]
    fn release_install_without_library_leaves_existing_library() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, exe) = install_dir(tmp.path());
        let canonical = bin.join(crate::lib_install::platform_lib_name());
        fs::write(&canonical, "existing").unwrap();
        let archive = release_archive(tmp.path(), "new binary", None);

        install_release_archive(&archive, &exe, "2.0.0").unwrap();

        assert_eq!(fs::read_to_string(&exe).unwrap(), "new binary");
        assert_eq!(fs::read_to_string(&canonical).unwrap(), "existing");
    }

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
            Ok(crate::plugin::PluginReconcileReport {
                targets: vec![crate::plugin::PluginTargetReport {
                    family: crate::plugin::PluginEditorFamily::JetBrains,
                    label: "IntelliJIdea2026.3".to_string(),
                    version: "0.2.481".to_string(),
                    outcome: crate::plugin::PluginTargetOutcome::HotUpgraded,
                }],
            })
        })
        .unwrap();
        assert!(
            called,
            "the one-shot upgrade must reconcile installed plugins"
        );
        reconcile_installed_plugins_once("0.35.441", || {
            Ok(crate::plugin::PluginReconcileReport::default())
        })
        .unwrap();
    }

    /// A stand-in "new binary": records its argv to `argv.txt` and exits with
    /// `exit_code`.
    #[cfg(unix)]
    fn fake_upgraded_binary(dir: &Path, exit_code: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.join("agent-doc");
        let record = dir.join("argv.txt");
        fs::write(
            &exe,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexit {exit_code}\n",
                record.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        exe
    }

    #[cfg(unix)]
    #[test]
    fn replaced_binary_reconciles_plugins_in_the_new_image_pinned_to_the_release() {
        for mode in [ReconcileMode::Once, ReconcileMode::Auto] {
            let dir = tempfile::tempdir().unwrap();
            let exe = fake_upgraded_binary(dir.path(), 0);
            let mut in_process_ran = false;
            reconcile_plugins_in_image(Some(&exe), "0.35.443", mode, || {
                in_process_ran = true;
                Ok(())
            })
            .unwrap();
            assert!(!in_process_ran, "the old image's reconcile must not run");
            let argv = fs::read_to_string(dir.path().join("argv.txt")).unwrap();
            assert_eq!(
                argv.lines().collect::<Vec<_>>(),
                [
                    "upgrade",
                    RECONCILE_PLUGINS_RELEASE_FLAG,
                    "0.35.443",
                    RECONCILE_PLUGINS_MODE_FLAG,
                    mode.as_arg(),
                ]
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn replaced_binary_reconcile_failure_propagates() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_upgraded_binary(dir.path(), 3);
        let mut in_process_ran = false;
        let error = reconcile_plugins_in_image(Some(&exe), "0.35.443", ReconcileMode::Once, || {
            in_process_ran = true;
            Ok(())
        })
        .unwrap_err();
        assert!(
            !in_process_ran,
            "a child failure must not be retried in the old image"
        );
        let message = format!("{error:#}");
        assert!(message.contains("upgraded v0.35.443 binary"), "{message}");
        assert!(
            message.contains('3'),
            "exit status must be reported: {message}"
        );
    }

    #[test]
    fn already_current_binary_reconciles_in_process() {
        let mut in_process_ran = false;
        reconcile_plugins_in_image(None, "0.35.443", ReconcileMode::Once, || {
            in_process_ran = true;
            Ok(())
        })
        .unwrap();
        assert!(in_process_ran);
        let error = reconcile_plugins_in_image(None, "0.35.443", ReconcileMode::Auto, || {
            Err(anyhow::anyhow!("download refused"))
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("download refused"));
    }

    #[test]
    fn unlaunchable_upgraded_binary_falls_back_in_process_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("vanished-agent-doc");
        let mut in_process_ran = false;
        reconcile_plugins_in_image(Some(&missing), "0.35.443", ReconcileMode::Once, || {
            in_process_ran = true;
            Ok(())
        })
        .unwrap();
        assert!(
            in_process_ran,
            "spawn failure must fall back to the in-process reconcile"
        );
        // The fallback's result is what propagates.
        assert!(
            reconcile_plugins_in_image(Some(&missing), "0.35.443", ReconcileMode::Once, || {
                Err(anyhow::anyhow!("old code failed"))
            })
            .is_err()
        );
        let warning = child_spawn_fallback_warning(
            &missing,
            "0.35.443",
            &std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        assert!(warning.starts_with("WARNING:"), "{warning}");
        assert!(
            warning.contains(&missing.display().to_string()),
            "{warning}"
        );
        assert!(
            warning.contains(&format!("OLD v{CURRENT_VERSION} plugin-install code")),
            "{warning}"
        );
    }

    #[test]
    fn auto_reconcile_reporting_propagates_failure() {
        let release = "0.35.442";
        reconcile_installed_plugins_auto(release, || {
            Ok(crate::plugin::PluginReconcileReport::default())
        })
        .unwrap();
        assert!(
            reconcile_installed_plugins_auto(release, || Err(anyhow::anyhow!("refused"))).is_err()
        );
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
        state.binary_upgraded("1.1.0", PathBuf::from("/opt/agent-doc"));
        assert_eq!(
            state.upgraded_exe.as_deref(),
            Some(Path::new("/opt/agent-doc")),
            "later cycles must reconcile in the installed executable, not this stale image"
        );
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
