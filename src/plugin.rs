//! # Module: plugin
//!
//! ## Spec
//! - Manages editor plugin lifecycle (install, update, list) for JetBrains IDEs and VS Code-family editors (VS Code, VSCodium, Cursor).
//! - `install(editor)` — fetches the latest GitHub Release for `btakita/agent-doc`, selects the appropriate asset (signed variant preferred), downloads it, and installs it.
//! - `install_local(editor)` — installs from a locally built artifact found by walking up from CWD to locate an `editors/` directory.
//! - `update(editor)` — for JetBrains, skips re-install if the installed plugin version matches the latest package asset; for VS Code, reinstalls through the editor CLI.
//! - `update_all_installed()` — release-watcher entry point that updates every existing agent-doc JetBrains/VS Code installation without installing into a new editor. Returns a `PluginReconcileReport` with one `PluginTargetOutcome` per target (GH #114), from which `PluginReconcileReport::summary` derives the upgrade's closing lines: separate hot-upgraded / installed / staged / restart-required / unchanged counts, each target that needs a restart by name, and no "if it does not reload on its own" hedge where a reload is known not to happen. A target whose proven platform build is outside every published package range is reported as `UnsupportedPlatform` (GH #233): left in place, one warning, no failure.
//! - `list()` — scans JetBrains plugin directories for the versioned agent-doc JAR and queries `code --list-extensions` for the VS Code extension; prints found entries to stdout.
//! - JetBrains plugin directories are discovered from versioned IDE data roots (`~/.local/share/JetBrains/<Product><Version>/` on Linux, `~/Library/Application Support/JetBrains/<Product><Version>/` on macOS). Config roots and unrelated JetBrains service directories are excluded. Callers can select an exact target with `--plugins-dir`; ambiguous non-interactive discovery fails with rerun guidance instead of waiting on stdin.
//! - VS Code CLI detection order: `cursor` → `codium` → `code` (first that succeeds `--version`). Absence is reported as a missing prerequisite before any download, never discarded and re-spawned as `code`.
//! - Asset selection matches complete versioned package shapes and prefers a signed variant. JetBrains targets prove their platform build from the versioned IDE data root: 242-261 select `agent-doc-jetbrains-<version>.zip`, exact 262 selects `agent-doc-jetbrains-262-<version>.zip`, and an unprovable/unsupported target fails before replacement. Local installs read the selected package's own `gradle.properties`; VS Code installs require the VSIX version to match `package.json` exactly.
//! - Downloaded editor packages are verified before installation against the release's `EDITOR-PACKAGES.sha256` manifest, falling back to GitHub's per-asset `digest`. A declared digest that disagrees with the bytes fails closed.
//!
//! ## Agentic Contracts
//! - `install(editor)` — returns `Err` on network failure, missing asset, or CLI install failure.
//! - `install_local(editor)` — returns `Err` if no `editors/` directory is found or no artifact exists.
//! - `update(editor)` — returns `Ok(())` early (no-op) when the JetBrains plugin is already at the latest version.
//! - `update_all_installed()` attempts editor families independently and reports all failures after the remaining installed targets have been attempted.
//! - A staged target is never counted as updated: it needs an IDE restart, and when agent-doc declined the restart-free upgrade on that JetBrains build the summary says the restart-free path is unavailable there (GH #114).
//! - `list()` — always returns `Ok(())`; emits a stderr message when no plugins are found.
//! - Unrecognized `editor` strings return `Err` with a list of supported values.
//! - Byte-identical local JetBrains packages are true no-ops: the installed tree is not
//!   rewritten, so a live IDE never maps an unlinked duplicate of the same generation.
//! - Changed JetBrains packages attach a system-classloader upgrade bridge to every live IDE,
//!   letting JetBrains unload, replace, and load the package through its dynamic-plugin API.
//!   Direct filesystem replacement is used only when no live IDE owns that installation.
//! - Every successful package replacement schedules all existing supervisors and controllers for
//!   safe-boundary recycle. Plugin updates use the full controller fleet, including idle roots and
//!   same-binary controllers, because their editor endpoints can still own the replaced package.
//!
//! ## Evals
//! - install_unknown_editor: `install("emacs")` → Err containing "Unknown editor"
//! - update_already_current: JetBrains plugin at matching version → early Ok, no download
//! - list_no_plugins: no IDE dirs, `code` absent → stderr "No agent-doc editor plugins found", Ok
//! - detect_code_cmd: cursor available → returns "cursor"; only code available → returns "code"; none available → `None`, and the caller fails with install guidance before downloading
//! - find_asset_prefers_signed: release with both signed and unsigned *versioned* zips → signed asset selected in either API order
//! - editor package integrity: a manifest or API digest that disagrees with the downloaded bytes refuses the install
//! - find_local_zip_prefers_signed: dist dir with both zips → signed path returned
//! - find_local_vscode_vsix_requires_manifest_version: stale VSIX files are ignored and a missing current build fails closed
//! - jetbrains_discovery_excludes_config_and_service_roots: only versioned IDE data roots are candidates
//! - release_search_skips_prereleases_and_drafts: the fallback walk matches `/releases/latest` stable-release semantics
//! - github_rate_limit_error_reports_reset_and_auth_guidance: exhausted API limits produce an actionable diagnostic

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_REPO: &str = "btakita/agent-doc";
const VSCODE_EXTENSION_ID: &str = "btakita.agent-doc";

fn build_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_recv_body(Some(std::time::Duration::from_secs(30)))
        .timeout_send_body(Some(std::time::Duration::from_secs(10)))
        .build()
        .into()
}

fn github_token_from(mut get_var: impl FnMut(&str) -> Option<String>) -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"]
        .into_iter()
        .find_map(|name| get_var(name).filter(|token| !token.trim().is_empty()))
}

fn github_token() -> Option<String> {
    github_token_from(|name| std::env::var(name).ok())
}

fn github_get_request(
    agent: &ureq::Agent,
    url: &str,
    token: Option<&str>,
) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    let request = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "agent-doc");
    if let Some(token) = token {
        request.header("Authorization", &format!("Bearer {token}"))
    } else {
        request
    }
}

fn ensure_github_api_success<T>(response: &ureq::http::Response<T>, context: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    if status.as_u16() == 403
        && response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
            == Some("0")
    {
        let reset = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|value| value.to_str().ok())
            .map(format_rate_limit_reset)
            .unwrap_or_default();
        bail!(
            "GitHub API rate limit exhausted{reset}; set GITHUB_TOKEN or GH_TOKEN to authenticate"
        );
    }

    bail!("{context}: GitHub returned HTTP {status}")
}

/// Render `x-ratelimit-reset` so the diagnostic is actionable on its own
/// (GH #57). A bare `Unix timestamp 1789999999` does not tell the operator
/// whether to wait one minute or fifty, so the wait is spelled out; the epoch
/// stays alongside it for scripted consumers.
fn format_rate_limit_reset(raw: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    format_rate_limit_reset_at(raw, now)
}

fn format_rate_limit_reset_at(raw: &str, now_epoch_secs: u64) -> String {
    let Ok(reset_epoch_secs) = raw.trim().parse::<u64>() else {
        // Unparseable header: report it verbatim rather than inventing a wait.
        return format!("; resets at Unix timestamp {raw}");
    };
    let remaining = reset_epoch_secs.saturating_sub(now_epoch_secs);
    if remaining == 0 {
        return format!("; the reset is due now (Unix timestamp {reset_epoch_secs})");
    }
    let minutes = remaining / 60;
    let seconds = remaining % 60;
    let human = if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    };
    format!("; resets in {human} (Unix timestamp {reset_epoch_secs})")
}

fn fetch_github(url: &str, context: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let agent = build_agent();
    let token = github_token();
    let response = github_get_request(&agent, url, token.as_deref())
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .with_context(|| context.to_string())?;
    ensure_github_api_success(&response, context)?;
    Ok(response)
}

fn fetch_latest_release() -> Result<Value> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = fetch_github(&url, "Failed to fetch latest release from GitHub")?;
    let body: Value = resp
        .into_body()
        .read_json()
        .context("Failed to parse release JSON")?;
    Ok(body)
}

/// `#pluginassetpaging`: releases per page requested from GitHub.
///
/// The releases endpoint defaults to 30 per page and this call used to send no
/// `per_page` and never paginate, so the asset search could only ever see the 30
/// most recent releases. Editor-plugin assets ship on their own cadence — as of
/// GH #53 exactly one release in the window carried
/// `agent-doc-jetbrains-*.zip` — so at this repository's release rate that
/// asset silently scrolls out of reach and `plugin install` / `plugin update`
/// start failing with "No agent-doc-jetbrains*.zip asset found", on a release
/// that is still published and still downloadable.
const RELEASES_PER_PAGE: usize = 100;

/// Bound on how far back the asset search walks. 5 × 100 is deep enough to
/// outlast any plausible gap between plugin-bearing releases while keeping a
/// missing asset a bounded failure rather than a walk of the entire history.
const RELEASE_SEARCH_MAX_PAGES: usize = 5;

fn releases_page_url(page: usize) -> String {
    format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
    )
}

fn fetch_releases_page(page: usize) -> Result<Vec<Value>> {
    let resp = fetch_github(
        &releases_page_url(page),
        "Failed to fetch releases from GitHub",
    )?;
    let body: Vec<Value> = resp
        .into_body()
        .read_json()
        .context("Failed to parse releases JSON")?;
    Ok(body)
}

/// Walk release pages newest-first and return the first release carrying the
/// asset. Stops at the first match, at a short (final) page, or at
/// [`RELEASE_SEARCH_MAX_PAGES`], whichever comes first.
///
/// `fetch_page` is a parameter so the paging decisions are testable without a
/// network.
fn find_release_with_asset(
    prefix: &str,
    ext: &str,
    fetch_page: impl FnMut(usize) -> Result<Vec<Value>>,
) -> Result<Value> {
    find_release_where(prefix, ext, |_| true, fetch_page).map_err(|miss| match miss {
        ReleaseSearchMiss::Fetch(error) => error,
        ReleaseSearchMiss::NotFound(scanned) => anyhow::anyhow!(
            "No {prefix}*.{ext} asset found in the {scanned} most recent GitHub releases"
        ),
    })
}

/// GH #113: like [`find_release_with_asset`], but skip every release newer
/// than `ceiling`, so `agent-doc upgrade` reconciles plugins to the release it
/// just installed instead of whatever "latest" says by the time the plugin
/// phase runs.
fn find_release_with_asset_at_or_below(
    prefix: &str,
    ext: &str,
    ceiling: &str,
    fetch_page: impl FnMut(usize) -> Result<Vec<Value>>,
) -> Result<Value> {
    let ceiling_key = numeric_dot_version(ceiling.trim_start_matches('v'))
        .with_context(|| format!("invalid pinned release version {ceiling:?}"))?;
    find_release_where(
        prefix,
        ext,
        |release| {
            numeric_dot_version(release_version(release).trim_start_matches('v'))
                .is_some_and(|key| key <= ceiling_key)
        },
        fetch_page,
    )
    .map_err(|miss| match miss {
        ReleaseSearchMiss::Fetch(error) => error,
        ReleaseSearchMiss::NotFound(scanned) => anyhow::anyhow!(
            "No {prefix}*.{ext} asset found at or below v{} in the {scanned} most recent GitHub releases",
            ceiling.trim_start_matches('v')
        ),
    })
}

/// Why [`find_release_where`] returned no release: a page fetch failed, or the
/// walk finished after scanning this many releases without a match (each caller
/// phrases its own miss message).
enum ReleaseSearchMiss {
    Fetch(anyhow::Error),
    NotFound(usize),
}

/// Shared newest-first page walk behind the asset searches.
fn find_release_where(
    prefix: &str,
    ext: &str,
    accept: impl Fn(&Value) -> bool,
    mut fetch_page: impl FnMut(usize) -> Result<Vec<Value>>,
) -> std::result::Result<Value, ReleaseSearchMiss> {
    let mut scanned = 0usize;
    for page in 1..=RELEASE_SEARCH_MAX_PAGES {
        let releases = fetch_page(page).map_err(ReleaseSearchMiss::Fetch)?;
        // A page shorter than the requested size is the last one. Checked
        // before the scan so a match on the final page still returns.
        let is_final_page = releases.len() < RELEASES_PER_PAGE;
        scanned += releases.len();
        for release in releases {
            if is_stable_release(&release) && accept(&release) && has_asset(&release, prefix, ext) {
                return Ok(release);
            }
        }
        if is_final_page {
            break;
        }
    }
    Err(ReleaseSearchMiss::NotFound(scanned))
}

fn is_stable_release(release: &Value) -> bool {
    !release["prerelease"].as_bool().unwrap_or(false)
        && !release["draft"].as_bool().unwrap_or(false)
}

/// One release asset resolved by [`find_asset`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReleaseAsset<'a> {
    name: &'a str,
    url: &'a str,
    /// GitHub's per-asset checksum, `sha256:<hex>` when the API reports one.
    digest: Option<&'a str>,
}

fn asset_matches(name: &str, prefix: &str, ext: &str) -> bool {
    // Match the complete package shape, not a prefix: `agent-doc-jetbrains`
    // also prefixes `agent-doc-jetbrains-262-<version>.zip`, and prefix-first
    // selection could hand the exact-262 artifact to a classic 242-261 install.
    // Which line a target receives is decided by the target-IDE build resolver.
    packaged_plugin_version(name, &format!("{prefix}-"), &format!(".{ext}")).is_some()
}

fn read_asset<'a>(asset: &'a Value) -> Result<ReleaseAsset<'a>> {
    let name = asset["name"]
        .as_str()
        .context("Release asset has no name")?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("No download URL for asset")?;
    Ok(ReleaseAsset {
        name,
        url,
        digest: asset["digest"].as_str(),
    })
}

fn find_asset<'a>(release: &'a Value, prefix: &str, ext: &str) -> Result<ReleaseAsset<'a>> {
    let assets = release["assets"]
        .as_array()
        .context("No assets in release")?;

    // Prefer a signed variant, matched by SHAPE (GH #55). The previous code
    // compared against the exact string `format!("{prefix}-signed.{ext}")`, but
    // every published asset is versioned — `agent-doc-jetbrains-0.2.386-signed.zip`
    // — so that equality could never fire and selection silently fell through to
    // whatever order the API happened to return. It read as a preference while
    // being unreachable.
    let signed_suffix = format!("-signed.{ext}");
    let mut first_match: Option<&'a Value> = None;
    for asset in assets {
        let Some(name) = asset["name"].as_str() else {
            continue;
        };
        if !asset_matches(name, prefix, ext) {
            continue;
        }
        if name.ends_with(&signed_suffix) {
            return read_asset(asset);
        }
        if first_match.is_none() {
            first_match = Some(asset);
        }
    }

    if let Some(asset) = first_match {
        return read_asset(asset);
    }

    bail!("No {prefix}*.{ext} asset found in latest release");
}

fn has_asset(release: &Value, prefix: &str, ext: &str) -> bool {
    find_asset(release, prefix, ext).is_ok()
}

/// `#editorpkgdigest` (GH #55): the editor packages are code loaded into an
/// IDE and were the only release assets carrying no published integrity value.
/// `SHA256SUMS` deliberately covers the platform archives only — it is consumed
/// by the PyPI bootstrap launcher and `make release-macos-assets` — so the
/// editor packages get a manifest of their own instead.
const EDITOR_PACKAGE_MANIFEST: &str = "EDITOR-PACKAGES.sha256";

/// Look up `name` in a `sha256sum`-format manifest (`<hex>  <name>`).
fn editor_package_manifest_digest<'a>(manifest: &'a str, name: &str) -> Option<&'a str> {
    manifest.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let digest = fields.next()?;
        // `sha256sum` marks binary-mode entries with a leading `*`.
        let raw_entry = fields.next()?;
        let entry = raw_entry.strip_prefix('*').unwrap_or(raw_entry);
        (entry == name && fields.next().is_none()).then_some(digest)
    })
}

/// Strip GitHub's `sha256:` prefix from a release asset `digest` field.
fn parse_asset_digest(digest: &str) -> Option<&str> {
    digest.strip_prefix("sha256:").filter(|hex| !hex.is_empty())
}

fn compare_digest(name: &str, expected: &str, actual: &str, source: &str) -> Result<()> {
    if expected.eq_ignore_ascii_case(actual) {
        eprintln!("Verified {name} against {source}.");
        return Ok(());
    }
    bail!(
        "Integrity check failed for {name}: {source} declares sha256 {expected}, but the downloaded bytes hash to {actual}. Refusing to install."
    )
}

fn editor_package_manifest_url(release: &Value) -> Option<&str> {
    release["assets"].as_array()?.iter().find_map(|asset| {
        if asset["name"].as_str()? == EDITOR_PACKAGE_MANIFEST {
            asset["browser_download_url"].as_str()
        } else {
            None
        }
    })
}

fn fetch_text(url: &str) -> Result<String> {
    let mut body = String::new();
    build_agent()
        .get(url)
        .header("User-Agent", "agent-doc")
        .call()
        .context("Download failed")?
        .body_mut()
        .as_reader()
        .read_to_string(&mut body)
        .context("Failed to read response")?;
    Ok(body)
}

/// Verify a downloaded editor package before it is extracted or handed to the
/// editor CLI. Transport integrity already comes from HTTPS to GitHub; this is
/// the defence-in-depth the platform archives have had all along.
fn verify_editor_package(release: &Value, asset: &ReleaseAsset<'_>, path: &Path) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let actual = agent_doc_hash::bytes_hash(&bytes);

    if let Some(url) = editor_package_manifest_url(release) {
        let manifest = fetch_text(url)
            .with_context(|| format!("Failed to fetch {EDITOR_PACKAGE_MANIFEST}"))?;
        let Some(expected) = editor_package_manifest_digest(&manifest, asset.name) else {
            bail!(
                "{EDITOR_PACKAGE_MANIFEST} on this release has no entry for {}; refusing to install an editor package the manifest does not cover",
                asset.name
            );
        };
        return compare_digest(asset.name, expected, &actual, EDITOR_PACKAGE_MANIFEST);
    }

    if let Some(expected) = asset.digest.and_then(parse_asset_digest) {
        return compare_digest(
            asset.name,
            expected,
            &actual,
            "the GitHub release asset digest",
        );
    }

    // Releases cut before `#editorpkgdigest` carry neither a manifest nor an API
    // digest. Warn rather than fail closed, so `plugin update`'s fallback walk
    // can still reach an older asset.
    eprintln!(
        "Warning: {} carries no published checksum (no {EDITOR_PACKAGE_MANIFEST} asset and no API digest); installing without an integrity check.",
        asset.name
    );
    Ok(())
}

fn fetch_release_for_asset(prefix: &str, ext: &str) -> Result<Value> {
    if let Some(pinned) = release_pin() {
        return find_release_with_asset_at_or_below(prefix, ext, &pinned, fetch_releases_page);
    }
    let latest = fetch_latest_release()?;
    if has_asset(&latest, prefix, ext) {
        return Ok(latest);
    }

    let latest_tag = release_version(&latest).to_string();
    eprintln!(
        "Latest release {latest_tag} has no {prefix}*.{ext} asset; checking older releases..."
    );

    find_release_with_asset(prefix, ext, fetch_releases_page)
}

fn download_to_temp(url: &str) -> Result<tempfile::NamedTempFile> {
    eprintln!("Downloading {url}");
    let mut resp = build_agent()
        .get(url)
        .header("User-Agent", "agent-doc")
        .call()
        .context("Download failed")?;
    let mut tmp = tempfile::NamedTempFile::new().context("Failed to create temp file")?;
    let mut bytes = Vec::new();
    resp.body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .context("Failed to read response")?;
    tmp.write_all(&bytes).context("Failed to write temp file")?;
    tmp.flush()?;
    Ok(tmp)
}

fn release_version(release: &Value) -> &str {
    release["tag_name"].as_str().unwrap_or("unknown")
}

// --- JetBrains ---

#[cfg(test)]
use agent_doc_fs::jetbrains_install::is_jetbrains_ide_data_dir;
pub(crate) use agent_doc_fs::jetbrains_install::jetbrains_plugin_dirs;
#[cfg(test)]
use agent_doc_fs::jetbrains_install::jetbrains_plugin_dirs_in_roots;

/// The two published JetBrains packages share a plugin ID but have disjoint
/// compatibility ranges. Artifact selection therefore belongs to the target
/// IDE, never to release-asset ordering or whichever local ZIP was built last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JetbrainsPackageRange {
    Classic242To261,
    Modular262,
}

impl JetbrainsPackageRange {
    fn asset_prefix(self) -> &'static str {
        match self {
            Self::Classic242To261 => "agent-doc-jetbrains",
            Self::Modular262 => "agent-doc-jetbrains-262",
        }
    }

    fn project_dir(self) -> &'static str {
        match self {
            Self::Classic242To261 => "editors/jetbrains",
            Self::Modular262 => "editors/jetbrains-262",
        }
    }

    fn build_command(self) -> &'static str {
        match self {
            Self::Classic242To261 => "./gradlew buildPlugin",
            Self::Modular262 => "gradle buildPlugin verifySplitArtifact",
        }
    }

    fn package_version(self, asset_name: &str) -> Option<String> {
        packaged_plugin_version(asset_name, &format!("{}-", self.asset_prefix()), ".zip")
    }
}

fn jetbrains_platform_build(target_dir: &Path) -> Option<u32> {
    agent_doc_fs::jetbrains_install::jetbrains_platform_build(target_dir)
}

fn jetbrains_package_range(target_dir: &Path) -> Result<JetbrainsPackageRange> {
    let label = jetbrains_target_label(target_dir);
    let build = jetbrains_platform_build(target_dir).with_context(|| {
        format!(
            "Cannot prove the JetBrains platform build for {}. Expected a versioned IDE data directory such as ~/.local/share/JetBrains/IntelliJIdea2026.2 or its plugins child; refusing to choose between the classic 242-261 and modular 262 artifacts. Re-run with `--plugins-dir <versioned IDE data dir>/plugins`, or install the matching ZIP manually (agent-doc-jetbrains-<version>.zip for 242-261, agent-doc-jetbrains-262-<version>.zip for 262)",
            target_dir.display()
        )
    })?;
    match build {
        242..=261 => Ok(JetbrainsPackageRange::Classic242To261),
        262 => Ok(JetbrainsPackageRange::Modular262),
        // GH #233: no remedy this binary can offer installs into such a build,
        // so the message names none; the installed plugin is left alone.
        _ => bail!(
            "JetBrains target {label} uses unsupported platform build {build}; supported artifact ranges are {}. No published agent-doc-jetbrains package declares build {build}, so nothing was installed and the plugin already in {label} was left in place",
            agent_doc_fs::jetbrains_install::JETBRAINS_SUPPORTED_RANGES
        ),
    }
}

/// The exact release asset a target IDE must receive. Both published ZIPs
/// share one plugin ID, so the target's proven platform build is the only
/// selector; asset order and signed/unsigned preference never cross ranges.
fn jetbrains_release_asset_for_target<'a>(
    release: &'a Value,
    target_dir: &Path,
) -> Result<(JetbrainsPackageRange, ReleaseAsset<'a>)> {
    let package_range = jetbrains_package_range(target_dir)?;
    let asset = find_asset(release, package_range.asset_prefix(), "zip")?;
    Ok((package_range, asset))
}

/// Which compatibility line an installed tree holds, read from its plugin jar:
/// the classic package ships `agent-doc-jetbrains-<v>.jar`, the modular 262
/// package ships `agent.doc-<v>.jar` (its module JARs are unversioned).
fn installed_jetbrains_package(target_dir: &Path) -> Option<(JetbrainsPackageRange, String)> {
    [JETBRAINS_PLUGIN_DIR, JETBRAINS_MODULAR_PLUGIN_DIR]
        .into_iter()
        .filter_map(|plugin_dir| fs::read_dir(target_dir.join(plugin_dir).join("lib")).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            [
                (
                    JetbrainsPackageRange::Classic242To261,
                    "agent-doc-jetbrains-",
                ),
                (JetbrainsPackageRange::Modular262, "agent.doc-"),
            ]
            .into_iter()
            .find_map(|(range, prefix)| {
                let version = name.strip_prefix(prefix)?.strip_suffix(".jar")?;
                let key = numeric_dot_version(version)?;
                Some((key, range, version.to_string()))
            })
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, range, version)| (range, version))
}

/// What a release reconciliation must do with one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JetbrainsReleaseDecision {
    /// The target already holds this compatibility line at or above the asset.
    Current,
    /// Install the asset. `cross_line` means the target holds the OTHER
    /// compatibility line (for example a classic package inside a 262 IDE);
    /// version numbers across lines are not comparable, so it is replaced
    /// regardless of which Marketplace update number is higher.
    Replace { cross_line: bool },
}

fn jetbrains_release_decision(
    installed: Option<(JetbrainsPackageRange, &str)>,
    target_range: JetbrainsPackageRange,
    available_version: &str,
) -> Result<JetbrainsReleaseDecision> {
    let Some((installed_range, installed_version)) = installed else {
        return Ok(JetbrainsReleaseDecision::Replace { cross_line: false });
    };
    if installed_range != target_range {
        return Ok(JetbrainsReleaseDecision::Replace { cross_line: true });
    }
    Ok(
        match jetbrains_version_cmp(installed_version, available_version)? {
            CmpOrdering::Less => JetbrainsReleaseDecision::Replace { cross_line: false },
            CmpOrdering::Equal | CmpOrdering::Greater => JetbrainsReleaseDecision::Current,
        },
    )
}

fn jetbrains_release_decision_for(
    target_dir: &Path,
    target_range: JetbrainsPackageRange,
    available_version: &str,
) -> Result<JetbrainsReleaseDecision> {
    let installed = installed_jetbrains_package(target_dir);
    jetbrains_release_decision(
        installed
            .as_ref()
            .map(|(range, version)| (*range, version.as_str())),
        target_range,
        available_version,
    )
}

fn choose_plugins_dir_with_interactivity(
    dirs: &[PathBuf],
    explicit: Option<&Path>,
    interactive: bool,
) -> Result<PathBuf> {
    if let Some(explicit) = explicit {
        return Ok(explicit.to_path_buf());
    }
    if dirs.is_empty() {
        bail!(
            "No JetBrains IDE plugins directory found.\n\
             Expected versioned IDE data roots under:\n  \
             Linux: ${{XDG_DATA_HOME:-~/.local/share}}/JetBrains/\n  \
             macOS: ~/Library/Application Support/JetBrains/"
        );
    }
    if dirs.len() == 1 {
        return Ok(dirs[0].clone());
    }
    if !interactive {
        let candidates = dirs
            .iter()
            .map(|dir| format!("  {}", dir.display()))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Multiple JetBrains IDE plugin directories found and stdin is non-interactive:\n{candidates}\nrerun with `--plugins-dir <PATH>`"
        );
    }

    eprintln!("Multiple JetBrains IDEs found. Choose a plugins directory:");
    for (i, d) in dirs.iter().enumerate() {
        eprintln!("  [{}] {}", i + 1, d.display());
    }
    eprint!("Enter number: ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let idx: usize = input.trim().parse().context("Invalid number")?;
    if idx == 0 || idx > dirs.len() {
        bail!("Selection out of range");
    }
    Ok(dirs[idx - 1].clone())
}

fn choose_plugins_dir(dirs: &[PathBuf], explicit: Option<&Path>) -> Result<PathBuf> {
    choose_plugins_dir_with_interactivity(dirs, explicit, io::stdin().is_terminal())
}

fn install_jetbrains_into(
    release: &Value,
    target_dir: &Path,
) -> Result<JetbrainsLocalInstallOutcome> {
    let (package_range, asset) = jetbrains_release_asset_for_target(release, target_dir)?;
    eprintln!("Found asset: {}", asset.name);
    fs::create_dir_all(target_dir).context("Failed to create JetBrains plugins directory")?;

    let tmp = download_to_temp(asset.url)?;
    verify_editor_package(release, &asset, tmp.path())?;

    let expected_version = jetbrains_zip_plugin_version(tmp.path())?;
    // The downgrade guard compares versions only within one compatibility
    // line. A classic package left inside a 262 IDE (or the reverse) is
    // incompatible whatever its update number, so it is always replaced.
    if let Some((installed_range, installed_version)) = installed_jetbrains_package(target_dir)
        && installed_range == package_range
        && jetbrains_version_cmp(&installed_version, &expected_version)? == CmpOrdering::Greater
    {
        eprintln!(
            "Installed JetBrains plugin v{installed_version} is newer than release asset v{expected_version}; refusing downgrade and keeping the installed generation. Use `agent-doc plugin install jetbrains --local` for the current checkout build."
        );
        return Ok(JetbrainsLocalInstallOutcome::Unchanged);
    }
    let outcome = install_jetbrains_zip_into(tmp.path(), target_dir, &expected_version)?;

    eprintln!(
        "{}",
        jetbrains_install_result_message(target_dir, &outcome, &expected_version)?
    );
    print_jetbrains_activation_outcome(outcome.clone());
    Ok(outcome)
}

/// GH #108: the headline for a finished release install. A staged package left
/// the plugin tree untouched by design, so it must not read "Plugin installed"
/// stamped with the on-disk (old) version; it names the staged version instead.
fn jetbrains_install_result_message(
    target_dir: &Path,
    outcome: &JetbrainsLocalInstallOutcome,
    staged_version: &str,
) -> Result<String> {
    if !matches!(
        outcome,
        JetbrainsLocalInstallOutcome::StagedForRestart { .. }
    ) {
        return jetbrains_install_success_message(target_dir);
    }
    let kept = installed_jetbrains_plugin_version(target_dir)
        .map(|version| format!("v{version}"))
        .unwrap_or_else(|| "the previous generation".to_string());
    Ok(format!(
        "Plugin v{staged_version} staged (not installed) for the next IDE start; {} still holds {kept}, which stays loaded until the IDE restarts.",
        target_dir.display()
    ))
}

fn jetbrains_install_success_message(target_dir: &Path) -> Result<String> {
    let version = installed_jetbrains_plugin_version(target_dir).with_context(|| {
        format!(
            "JetBrains package verification failed in {}: no agent-doc plugin jar found",
            target_dir.display()
        )
    })?;
    Ok(format!(
        "Plugin installed (v{version}) to {}",
        target_dir.display()
    ))
}

fn install_jetbrains(plugins_dir: Option<&Path>) -> Result<JetbrainsLocalInstallOutcome> {
    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
    let package_range = jetbrains_package_range(&target_dir)?;
    let release = fetch_release_for_asset(package_range.asset_prefix(), "zip")?;
    install_jetbrains_into(&release, &target_dir)
}

// --- VS Code ---

/// Resolve the VS Code-family CLI, or `None` when none is installed.
///
/// GH #57: this used to return `"code"` after proving all three candidates
/// absent, so the caller discarded the absence it had just measured and spawned
/// a binary known to be missing — surfacing as a bare `No such file or
/// directory (os error 2)` that reads like the *vsix* is missing.
fn detect_code_cmd() -> Option<&'static str> {
    available_code_cmds().into_iter().next()
}

fn available_code_cmds() -> Vec<&'static str> {
    ["cursor", "codium", "code"]
        .into_iter()
        .filter(|cmd| {
            std::process::Command::new(cmd)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .collect()
}

fn missing_code_cli_message() -> &'static str {
    "No VS Code-family CLI on PATH: tried `cursor`, `codium`, and `code`. \
Install the editor's shell command (VS Code / VSCodium: run `Shell Command: Install 'code' command in PATH` from the command palette; Cursor: `Install 'cursor' command`), or put the CLI on PATH, then re-run."
}

fn require_code_cmd() -> Result<&'static str> {
    detect_code_cmd().context(missing_code_cli_message())
}

fn vscode_extension_version_from_output(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let (id, version) = line.trim().split_once('@')?;
        (id.eq_ignore_ascii_case(VSCODE_EXTENSION_ID) && !version.is_empty())
            .then(|| version.to_owned())
    })
}

fn installed_vscode_extensions() -> (Vec<(&'static str, String)>, Vec<String>) {
    let mut installed = Vec::new();
    let mut errors = Vec::new();
    for code in available_code_cmds() {
        match std::process::Command::new(code)
            .args(["--list-extensions", "--show-versions"])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(version) = vscode_extension_version_from_output(&stdout) {
                    installed.push((code, version));
                }
            }
            Ok(output) => errors.push(format!(
                "`{code} --list-extensions --show-versions` exited with {}",
                output.status
            )),
            Err(error) => errors.push(format!(
                "Failed to run `{code} --list-extensions --show-versions`: {error}"
            )),
        }
    }
    (installed, errors)
}

fn packaged_plugin_version(name: &str, prefix: &str, extension: &str) -> Option<String> {
    let base = name.strip_prefix(prefix)?.strip_suffix(extension)?;
    let version = base.strip_suffix("-signed").unwrap_or(base);
    numeric_dot_version(version).map(|_| version.to_owned())
}

fn install_vscode(release: &Value) -> Result<()> {
    let code = require_code_cmd()?;
    install_vscode_with_cmd(release, code)
}

fn install_vscode_with_cmd(release: &Value, code: &str) -> Result<()> {
    let asset = find_asset(release, "agent-doc", "vsix")?;
    eprintln!("Found asset: {}", asset.name);
    let tmp = download_to_temp(asset.url)?;
    verify_editor_package(release, &asset, tmp.path())?;

    let status = std::process::Command::new(code)
        .args(["--install-extension"])
        .arg(tmp.path())
        .status()
        .with_context(|| format!("Failed to run `{code} --install-extension`"))?;

    if !status.success() {
        bail!("`{code} --install-extension` exited with {status}");
    }

    let version = release_version(release);
    eprintln!("Extension installed ({version}) via `{code}`.");
    Ok(())
}

// --- Public API ---

pub fn install(editor: &str) -> Result<()> {
    install_with_plugins_dir(editor, None)
}

pub fn install_with_plugins_dir(editor: &str, plugins_dir: Option<&Path>) -> Result<()> {
    let live_plugin_replaced = match editor {
        "jetbrains" | "jb" | "idea" => matches!(
            install_jetbrains(plugins_dir)?,
            JetbrainsLocalInstallOutcome::HotUpgraded { .. }
        ),
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains installs");
            }
            let release = fetch_release_for_asset("agent-doc", "vsix")?;
            install_vscode(&release)?;
            false
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    };
    if live_plugin_replaced {
        crate::runtime_update::recycle_existing_runtimes_after_live_plugin_update("plugin-install");
    }
    Ok(())
}

pub fn install_local(editor: &str) -> Result<()> {
    install_local_with_plugins_dir(editor, None)
}

pub fn install_local_with_plugins_dir(editor: &str, plugins_dir: Option<&Path>) -> Result<()> {
    let live_plugin_replaced = match editor {
        "jetbrains" | "jb" | "idea" => matches!(
            install_jetbrains_local(plugins_dir)?,
            JetbrainsLocalInstallOutcome::HotUpgraded { .. }
        ),
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains installs");
            }
            install_vscode_local()?;
            false
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    };
    if live_plugin_replaced {
        crate::runtime_update::recycle_existing_runtimes_after_live_plugin_update(
            "plugin-install-local",
        );
    }
    Ok(())
}

fn find_local_build_dir() -> Result<PathBuf> {
    // Walk up from CWD to find project root with editors/ directory
    let cwd = std::env::current_dir().context("Failed to get CWD")?;
    let mut dir = cwd.as_path();
    loop {
        let editors = dir.join("editors");
        if editors.is_dir() {
            return Ok(dir.to_path_buf());
        }
        // Also check if we're in the agent-doc submodule from a parent workspace
        let src_agent_doc = dir.join("src/agent-doc/editors");
        if src_agent_doc.is_dir() {
            return Ok(dir.join("src/agent-doc"));
        }
        dir = dir
            .parent()
            .context("Could not find project root with editors/ directory")?;
    }
}

fn install_jetbrains_local(plugins_dir: Option<&Path>) -> Result<JetbrainsLocalInstallOutcome> {
    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
    let zip_path = local_jetbrains_zip(&target_dir)?;
    let outcome = install_jetbrains_local_zip_into(&zip_path, &target_dir)?;
    match &outcome {
        JetbrainsLocalInstallOutcome::Installed => {
            eprintln!("Plugin installed to {}", target_dir.display());
            eprintln!("No live IDE owned this installation; the next IDE start loads it.");
        }
        JetbrainsLocalInstallOutcome::HotUpgraded { processes } => {
            eprintln!("Plugin dynamically upgraded in {processes} live JetBrains process(es).");
            eprintln!("No JetBrains restart is required.");
        }
        JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
            eprintln!("WARNING: {}", restart_required_message(&target_dir, reason));
        }
        JetbrainsLocalInstallOutcome::StagedForRestart { reason } => {
            eprintln!(
                "WARNING: {}",
                staged_for_restart_message(&target_dir, reason)
            );
        }
        JetbrainsLocalInstallOutcome::Unchanged => {
            eprintln!(
                "Plugin already byte-identical at {}; kept the live generation in place",
                target_dir.display()
            );
            eprintln!("No JetBrains restart is required; no installed plugin bytes changed.");
        }
    }
    Ok(outcome)
}

/// Install the current local JetBrains build into every IDE that already has
/// agent-doc installed. This is the non-interactive coherence path used by
/// `make install`: it updates all existing installations instead of choosing
/// one arbitrary IDE and silently leaving the others stale.
pub fn install_local_all_existing(editor: &str) -> Result<()> {
    let hot_upgraded = match editor {
        "jetbrains" | "jb" | "idea" => install_jetbrains_local_all_existing(),
        _ => bail!("--all-installed is currently supported only for JetBrains installs"),
    }?;
    if hot_upgraded > 0 {
        crate::runtime_update::recycle_existing_runtimes_after_live_plugin_update(
            "plugin-install-local",
        );
    }
    Ok(())
}

fn install_jetbrains_local_all_existing() -> Result<usize> {
    let targets = existing_jetbrains_agent_doc_dirs(&jetbrains_plugin_dirs());
    if targets.is_empty() {
        bail!(
            "No existing JetBrains agent-doc installation found; install one explicitly with \
             `agent-doc plugin install jetbrains --local --plugins-dir <PATH>`"
        );
    }

    // Resolve every target before replacing any installation. A custom or
    // future IDE directory whose platform build cannot be proven must not let
    // an arbitrary classic ZIP overwrite a modular installation.
    let project_root = find_local_build_dir()?;
    let targets = resolve_local_jetbrains_targets(&project_root, targets)?;
    let mut installed = 0usize;
    let mut hot_upgraded = 0usize;
    let mut unchanged = 0usize;
    let mut restart_pending = 0usize;
    for (target_dir, zip_path) in &targets {
        match install_jetbrains_local_zip_into(zip_path, target_dir)? {
            JetbrainsLocalInstallOutcome::Installed => {
                installed += 1;
                eprintln!(
                    "Plugin installed to {}; no live IDE runs it, so the next IDE start loads it",
                    target_dir.display()
                );
            }
            JetbrainsLocalInstallOutcome::HotUpgraded { processes } => {
                installed += 1;
                hot_upgraded += 1;
                eprintln!(
                    "Plugin dynamically upgraded in {processes} live JetBrains process(es) for {}",
                    target_dir.display()
                );
            }
            JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
                installed += 1;
                restart_pending += 1;
                eprintln!("WARNING: {}", restart_required_message(target_dir, &reason));
            }
            JetbrainsLocalInstallOutcome::StagedForRestart { reason } => {
                installed += 1;
                restart_pending += 1;
                eprintln!(
                    "WARNING: {}",
                    staged_for_restart_message(target_dir, &reason)
                );
            }
            JetbrainsLocalInstallOutcome::Unchanged => {
                unchanged += 1;
                eprintln!(
                    "Plugin already byte-identical at {}; kept the live generation in place",
                    target_dir.display()
                );
            }
        }
    }
    eprintln!(
        "JetBrains package convergence: {installed} updated, {unchanged} already current across {} existing IDE installation(s).",
        targets.len(),
    );
    eprintln!(
        "{}",
        jetbrains_convergence_restart_summary(installed, hot_upgraded, restart_pending)
    );
    Ok(hot_upgraded)
}

/// The closing line of a local JetBrains convergence. It used to say "no IDE
/// restart is required" whenever anything changed, including right after a
/// WARNING that the upgrade was staged for the next IDE start.
///
/// `#jbdynamicfalsereport`: it also claimed a dynamic replacement for a plain
/// `Installed` outcome -- files written while no live IDE ran the plugin -- so a
/// running IDE with no agent-doc plugin loaded was told no restart was needed.
/// Only `hot_upgraded` installs (proven live) may claim a dynamic replacement.
fn jetbrains_convergence_restart_summary(
    installed: usize,
    hot_upgraded: usize,
    restart_pending: usize,
) -> String {
    let cold = installed.saturating_sub(hot_upgraded + restart_pending);
    if restart_pending > 0 {
        format!(
            "{restart_pending} JetBrains installation(s) are not running the new plugin generation; restart those IDEs to load the new plugin."
        )
    } else if cold > 0 {
        format!(
            "{cold} JetBrains installation(s) were written with no live IDE running agent-doc from them; the new plugin loads at the next IDE start (a running IDE without the plugin loaded must be restarted)."
        )
    } else if hot_upgraded > 0 {
        "Changed live JetBrains packages were dynamically replaced and verified live; no IDE restart is required."
            .to_string()
    } else {
        "No JetBrains restart is required; no installed plugin bytes changed.".to_string()
    }
}

fn existing_jetbrains_agent_doc_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    existing_jetbrains_agent_doc_dirs_in(
        dirs,
        &agent_doc_fs::jetbrains_install::jetbrains_system_roots(),
    )
}

/// GH #115: an installation a failed staging destroyed has no jar left, but it
/// is still an existing agent-doc installation: reconciliation must reinstall
/// it rather than skip it as "never installed here".
fn existing_jetbrains_agent_doc_dirs_in(
    dirs: &[PathBuf],
    system_roots: &[PathBuf],
) -> Vec<PathBuf> {
    dirs.iter()
        .filter(|dir| {
            installed_jetbrains_plugin_version(dir).is_some()
                || matches!(
                    agent_doc_fs::jetbrains_install::staged_install_failure(dir, system_roots),
                    Some(agent_doc_fs::jetbrains_install::StagedInstallFailure::Destroyed { .. })
                )
        })
        .cloned()
        .collect()
}

/// Bind every existing installation to the local ZIP of its own compatibility
/// range BEFORE any tree is replaced. Every unresolvable target is reported
/// together, and none is installed: a missing 262 build or an unprovable IDE
/// directory must never fall back to the classic ZIP.
fn resolve_local_jetbrains_targets(
    project_root: &Path,
    targets: Vec<PathBuf>,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut resolved = Vec::with_capacity(targets.len());
    let mut failures = Vec::new();
    for target in targets {
        match local_jetbrains_zip_in(project_root, &target) {
            Ok(zip) => resolved.push((target, zip)),
            Err(error) => failures.push(format!("  {}: {error:#}", target.display())),
        }
    }
    if !failures.is_empty() {
        bail!(
            "Refusing to update any JetBrains installation: {} target(s) have no provable compatible local package (no installation was changed):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
    Ok(resolved)
}

fn local_jetbrains_zip(target_dir: &Path) -> Result<PathBuf> {
    let project_root = find_local_build_dir()?;
    local_jetbrains_zip_in(&project_root, target_dir)
}

fn jetbrains_plugin_version(
    project_root: &Path,
    package_range: JetbrainsPackageRange,
) -> Result<String> {
    let properties = project_root
        .join(package_range.project_dir())
        .join("gradle.properties");
    let content = fs::read_to_string(&properties)
        .with_context(|| format!("Failed to read {}", properties.display()))?;
    content
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "pluginVersion").then(|| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Missing pluginVersion in {}", properties.display()))
}

fn local_jetbrains_zip_in(project_root: &Path, target_dir: &Path) -> Result<PathBuf> {
    let package_range = jetbrains_package_range(target_dir)?;
    let plugin_project = project_root.join(package_range.project_dir());
    let dist_dir = plugin_project.join("build/distributions");
    let version = jetbrains_plugin_version(project_root, package_range).with_context(|| {
        format!(
            "{} needs the {} package, but this checkout has no buildable {} project; refusing to substitute the other compatibility line",
            jetbrains_target_label(target_dir),
            package_range.asset_prefix(),
            plugin_project.display()
        )
    })?;
    let prefix = package_range.asset_prefix();
    let signed = dist_dir.join(format!("{prefix}-{version}-signed.zip"));
    if signed.is_file() {
        return Ok(signed);
    }
    let unsigned = dist_dir.join(format!("{prefix}-{version}.zip"));
    if unsigned.is_file() {
        return Ok(unsigned);
    }
    bail!(
        "No JetBrains package matching gradle.properties pluginVersion {version} at {}; build the compatibility-ranged artifact in {} (`{}`), or run `make install-editor-plugins`",
        unsigned.display(),
        plugin_project.display(),
        package_range.build_command()
    )
}

fn local_jetbrains_zip_version(zip_path: &Path) -> Result<String> {
    let name = zip_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Local JetBrains build has a non-UTF-8 filename")?;
    [
        JetbrainsPackageRange::Modular262,
        JetbrainsPackageRange::Classic242To261,
    ]
    .into_iter()
    .find_map(|range| range.package_version(name))
    .context("Local JetBrains build has an unexpected filename")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JetbrainsLocalInstallOutcome {
    Installed,
    HotUpgraded {
        processes: usize,
    },
    /// GH #63: the files were replaced on disk while a live IDE still runs the
    /// previous generation; `reason` says why no restart-free upgrade happened.
    RestartRequired {
        reason: String,
    },
    /// `#jbstageonfail`: the dynamic upgrade failed, so the live IDE staged the
    /// package for its next start and the jars it maps were left in place.
    StagedForRestart {
        reason: String,
    },
    Unchanged,
}

/// What the restart-free upgrade did across the live JetBrains processes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum JetbrainsHotUpgrade {
    Upgraded {
        processes: usize,
    },
    /// No process accepted the hot-swap, but at least one staged the package
    /// through its own pending-install script.
    StagedForRestart {
        reason: String,
    },
    /// `#jbdynamicfalsereport`: an in-IDE install already rewrote the plugin
    /// tree, but no live process proved it loaded the new generation (or another
    /// live owner of the target runs none at all). The tree is not replaced
    /// again under a JVM that may map it; the caller is told to restart.
    InstalledUnverified {
        reason: String,
    },
    /// `#jbdynamicfalsereport`: a live IDE owns the target plugins directory but
    /// had no agent-doc generation loaded, so nothing could be dynamically
    /// replaced. The files are written for the next start, which must happen
    /// before the IDE runs the plugin.
    NotLoaded {
        reason: String,
    },
}

/// `#jbdynamicfalsereport`: one live JetBrains process's answer to the dynamic
/// upgrader, parsed from the launcher's stdout.
#[derive(Debug, Clone, PartialEq, Eq)]
enum JetbrainsUpgraderStatus {
    /// `ok:<version>[:<reattach receipt>]` -- the IDE reports a fresh generation.
    Ok { status_line: String },
    /// `staged:<version>:<reason>` -- the IDE staged the package for its next start.
    Staged { reason: String },
    /// `skip:plugin-not-loaded[:plugins-path=<dir>]` -- the IDE runs no agent-doc
    /// generation. `plugins_path` names the directory it loads plugins from when
    /// the upgrader reported it (older upgraders did not).
    PluginNotLoaded { plugins_path: Option<PathBuf> },
    /// `skip:different-plugin-root:...` or any other status: this process does
    /// not serve the target installation.
    NotOwner,
}

fn parse_jetbrains_upgrader_status(stdout: &str) -> JetbrainsUpgraderStatus {
    for line in stdout.lines().map(str::trim) {
        if line.starts_with("ok:") {
            return JetbrainsUpgraderStatus::Ok {
                status_line: line.to_string(),
            };
        }
        if let Some(reason) = staged_upgrade_reason(line) {
            return JetbrainsUpgraderStatus::Staged { reason };
        }
        if let Some(rest) = line.strip_prefix("skip:plugin-not-loaded") {
            let plugins_path = rest
                .strip_prefix(":plugins-path=")
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from);
            return JetbrainsUpgraderStatus::PluginNotLoaded { plugins_path };
        }
    }
    JetbrainsUpgraderStatus::NotOwner
}

/// How long the installer waits for a live process to map the replacement jar
/// after the upgrader reported `ok:`.
const JETBRAINS_LIVE_LOAD_PROOF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn same_filesystem_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.components().eq(right.components()),
    }
}

/// `#jbdynamicfalsereport`: whether a live process's mapped plugin jar proves it
/// is executing `expected_version` from `target_dir`. Only a still-linked jar at
/// `<target_dir>/agent-doc-jetbrains/lib/agent-doc-jetbrains-<expected>.jar`
/// (or the exact-262 `agent.doc-<expected>.jar`) counts; a deleted mapping, a different version or a different plugin root is
/// not a load of this install.
fn jetbrains_mapped_jar_proves_load(
    mapped: &agent_doc_fs::plugin_jar::MappedPluginJar,
    target_dir: &Path,
    expected_version: &str,
) -> bool {
    let agent_doc_fs::plugin_jar::MappedPluginJar::Current { path, .. } = mapped else {
        return false;
    };
    // The classic package ships `agent-doc-jetbrains-<v>.jar`; the modular 262
    // package ships `agent.doc-<v>.jar`. Both land in the canonical tree.
    let lib = target_dir.join(JETBRAINS_PLUGIN_DIR).join("lib");
    [
        format!("agent-doc-jetbrains-{expected_version}.jar"),
        format!("agent.doc-{expected_version}.jar"),
    ]
    .iter()
    .any(|jar| same_filesystem_path(Path::new(path), &lib.join(jar)))
}

/// Poll `pid`'s mapped plugin jar until it proves the expected generation is
/// loaded from `target_dir`, or `timeout` elapses.
#[cfg(all(not(test), target_os = "linux"))]
fn await_jetbrains_live_load(
    pid: u32,
    target_dir: &Path,
    expected_version: &str,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let mapped = agent_doc_fs::plugin_jar::probe_mapped_plugin_jar(
            pid,
            agent_doc_fs::plugin_jar::JETBRAINS_PLUGIN_JAR_STEMS,
        );
        if jetbrains_mapped_jar_proves_load(&mapped, target_dir, expected_version) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Without `/proc` there is no mapped-jar evidence; the upgrader's own `ok:`
/// receipt (a fresh descriptor of the expected version owning a new
/// classloader) is the only proof available.
#[cfg(all(not(test), not(target_os = "linux")))]
fn await_jetbrains_live_load(
    _pid: u32,
    _target_dir: &Path,
    _expected_version: &str,
    _timeout: std::time::Duration,
) -> bool {
    true
}

/// `#jbdynamicfalsereport`: fold every live process's upgrader status into one
/// verdict. A process counts as dynamically upgraded only when the upgrader
/// reported `ok:` AND `load_proof` then observed the new generation live in it.
/// A live owner of the target with no generation loaded is a fresh install that
/// only a restart loads -- never a dynamic replacement.
fn jetbrains_hot_upgrade_from_statuses(
    statuses: Vec<(u32, JetbrainsUpgraderStatus)>,
    target_dir: &Path,
    expected_version: &str,
    mut load_proof: impl FnMut(u32) -> bool,
) -> Option<JetbrainsHotUpgrade> {
    let mut upgraded = 0usize;
    let mut unverified: Option<String> = None;
    let mut not_loaded: Option<String> = None;
    let mut staged: Option<String> = None;
    for (pid, status) in statuses {
        match status {
            JetbrainsUpgraderStatus::Ok { status_line } => {
                if load_proof(pid) {
                    upgraded += 1;
                    if let Some(warning) = jetbrains_upgrade_reattach_warning(pid, &status_line) {
                        eprintln!("WARNING: {warning}");
                    }
                } else {
                    unverified.get_or_insert_with(|| {
                        format!(
                            "pid {pid}: the upgrader reported `{status_line}` but the IDE was not observed running the agent-doc {expected_version} plugin jar from {} within {}s",
                            target_dir.join(JETBRAINS_PLUGIN_DIR).display(),
                            JETBRAINS_LIVE_LOAD_PROOF_TIMEOUT.as_secs()
                        )
                    });
                }
            }
            JetbrainsUpgraderStatus::Staged { reason } => {
                staged.get_or_insert_with(|| format!("pid {pid}: {reason}"));
            }
            JetbrainsUpgraderStatus::PluginNotLoaded { plugins_path } => {
                if plugins_path
                    .as_deref()
                    .is_some_and(|path| !same_filesystem_path(path, target_dir))
                {
                    continue;
                }
                not_loaded.get_or_insert_with(|| {
                    format!(
                        "live JetBrains pid {pid} has no agent-doc plugin loaded, so there was no generation to replace dynamically; the IDE loads the installed plugin only after a restart"
                    )
                });
            }
            JetbrainsUpgraderStatus::NotOwner => {}
        }
    }
    jetbrains_hot_upgrade_verdict(upgraded, unverified, not_loaded, staged)
}

/// GH #113: the release `agent-doc upgrade` just installed. When set, plugin
/// assets resolve to the newest release at or below it rather than to whatever
/// GitHub reports as latest by the time the plugin phase runs.
static RELEASE_PIN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn set_release_pin(version: Option<&str>) {
    *RELEASE_PIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        version.map(|version| version.trim_start_matches('v').to_owned());
}

fn release_pin() -> Option<String> {
    RELEASE_PIN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// GH #63: `--no-dynamic` turns the restart-free upgrade off for this process.
static DYNAMIC_UPGRADE_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub fn set_dynamic_upgrade_enabled(enabled: bool) {
    DYNAMIC_UPGRADE_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn dynamic_upgrade_enabled() -> bool {
    DYNAMIC_UPGRADE_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

fn restart_required_message(target_dir: &Path, reason: &str) -> String {
    format!(
        "Plugin files replaced in {} but the running IDE is not running the new plugin generation ({reason}). Restart the IDE to load the new plugin.",
        target_dir.display()
    )
}

fn staged_for_restart_message(target_dir: &Path, reason: &str) -> String {
    let cause = if agent_doc_declined_dynamic_upgrade(reason) {
        "agent-doc declined the restart-free upgrade on this JetBrains build"
    } else {
        "The restart-free upgrade failed"
    };
    format!(
        "{cause}, so the running IDE staged the new plugin for its next start and the plugin files in {} were left in place ({reason}). Restart the IDE to load the new plugin.",
        target_dir.display()
    )
}

fn print_jetbrains_activation_outcome(outcome: JetbrainsLocalInstallOutcome) {
    match outcome {
        JetbrainsLocalInstallOutcome::Installed => {
            eprintln!("No live IDE owned this installation; the next IDE start loads it.");
        }
        JetbrainsLocalInstallOutcome::HotUpgraded { processes } => {
            eprintln!("Plugin dynamically upgraded in {processes} live JetBrains process(es).");
            eprintln!("No JetBrains restart is required.");
        }
        JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
            eprintln!("WARNING: restart the IDE to load the new plugin ({reason}).");
        }
        JetbrainsLocalInstallOutcome::StagedForRestart { reason } => {
            eprintln!(
                "WARNING: the new plugin is staged for the next IDE start; restart the IDE to load it ({reason})."
            );
        }
        JetbrainsLocalInstallOutcome::Unchanged => {
            eprintln!("No JetBrains restart is required; no installed plugin bytes changed.");
        }
    }
}

fn jetbrains_ide_pids_from_jcmd(output: &str) -> Vec<u32> {
    let mut pids = output
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(' ')?;
            crate::plugin_activation::jetbrains_ide_label(command)?;
            pid.parse().ok()
        })
        .collect::<Vec<_>>();
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(not(test))]
fn live_jetbrains_ide_pids() -> Result<Vec<u32>> {
    let mut pids = crate::plugin_activation::live_ide_processes()
        .into_iter()
        .map(|process| process.pid)
        .collect::<Vec<_>>();
    match Command::new("jcmd").arg("-l").output() {
        Ok(output) if output.status.success() => {
            pids.extend(jetbrains_ide_pids_from_jcmd(&String::from_utf8_lossy(
                &output.stdout,
            )));
        }
        Ok(output) if pids.is_empty() => bail!(
            "Cannot discover live JetBrains processes: `jcmd -l` exited with {}; refusing an uncoordinated package replacement",
            output.status
        ),
        Err(error) if pids.is_empty() => bail!(
            "Cannot discover live JetBrains processes: failed to run `jcmd -l`: {error}; refusing an uncoordinated package replacement"
        ),
        _ => {}
    }
    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}

#[cfg(not(test))]
fn extract_jetbrains_upgrade_launcher(zip_path: &Path) -> Result<tempfile::NamedTempFile> {
    let file = fs::File::open(zip_path).context("Failed to open JetBrains package")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read JetBrains package")?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let file_name = name
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if jetbrains_plugin_jar_version(file_name).is_some() {
            let mut launcher = tempfile::Builder::new()
                .prefix("agent-doc-jb-upgrader-")
                .suffix(".jar")
                .tempfile()
                .context("Failed to create temporary JetBrains upgrade launcher")?;
            io::copy(&mut entry, &mut launcher)?;
            launcher.flush()?;
            return Ok(launcher);
        }
    }
    bail!("JetBrains package has no agent-doc plugin jar to use as the upgrade launcher")
}

/// `#jb262dynupgrade`: stable prefix of the reason an exact-262 modular package
/// gives for not attempting a restart-free upgrade. The classic launcher
/// (`JetBrainsPluginUpgradeBootstrap`) unloads one classic plugin tree and
/// installs a ZIP rooted at `agent-doc-jetbrains/`; the 262 package is a Plugin
/// Model v2 split-mode distribution (root `agent-doc-jetbrains-262/`, a
/// descriptor-only `agent.doc-<v>.jar` plus `lib/modules/agent.doc.*.jar`, with a
/// frontend half that may live in a separate JetBrains Client process), and it
/// ships no upgrade launcher at all. That is a property of the package line, not
/// a legacy build, so it must never read as "predates restart-free support".
const JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE: &str =
    "the exact-262 modular JetBrains package has no restart-free dynamic upgrade entry point";

/// Which compatibility line a package ZIP holds, read from its versioned plugin
/// jar: classic `agent-doc-jetbrains-<v>.jar` or modular `agent.doc-<v>.jar`.
fn jetbrains_zip_package_range(zip_path: &Path) -> Result<JetbrainsPackageRange> {
    let file = fs::File::open(zip_path).context("Failed to open JetBrains package")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read JetBrains package")?;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let file_name = name
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if agent_doc_fs::plugin_jar::plugin_jar_version(file_name, &["agent.doc-"]).is_some() {
            return Ok(JetbrainsPackageRange::Modular262);
        }
        if agent_doc_fs::plugin_jar::plugin_jar_version(file_name, &["agent-doc-jetbrains-"])
            .is_some()
        {
            return Ok(JetbrainsPackageRange::Classic242To261);
        }
    }
    bail!("JetBrains package has no versioned agent-doc plugin jar")
}

/// `#jb262dynupgrade`: why a live IDE cannot be upgraded restart-free from a
/// package of `package_range`, or `None` when the package carries a usable
/// upgrade launcher. `launcher_has_main` is consulted only for the classic line;
/// the modular 262 package has no launcher to inspect.
fn jetbrains_dynamic_upgrade_entry_point_refusal(
    package_range: JetbrainsPackageRange,
    expected_version: &str,
    launcher_has_main: impl FnOnce() -> Result<bool>,
) -> Result<Option<String>> {
    match package_range {
        JetbrainsPackageRange::Modular262 => Ok(Some(format!(
            "{JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE} (v{expected_version} is a Plugin Model v2 split-mode package whose agent.doc-{expected_version}.jar carries no upgrade launcher), so a running 262 IDE keeps its current plugin generation until it restarts"
        ))),
        JetbrainsPackageRange::Classic242To261 => Ok((!launcher_has_main()?).then(|| {
            format!(
                "JetBrains package v{expected_version} predates restart-free dynamic upgrade support (its plugin JAR has no Main-Class); refusing to replace a package owned by a live IDE. Restart the IDE before installing this legacy package, or install a current local build with `agent-doc plugin install jetbrains --local`."
            )
        })),
    }
}

fn jetbrains_upgrade_launcher_has_main_manifest(jar_path: &Path) -> Result<bool> {
    let file = fs::File::open(jar_path).context("Failed to open JetBrains upgrade launcher")?;
    let mut archive =
        zip::ZipArchive::new(file).context("Failed to read JetBrains upgrade launcher as a JAR")?;
    let mut manifest = match archive.by_name("META-INF/MANIFEST.MF") {
        Ok(manifest) => manifest,
        Err(zip::result::ZipError::FileNotFound) => return Ok(false),
        Err(error) => return Err(error).context("Failed to inspect JetBrains upgrade manifest"),
    };
    let mut content = String::new();
    manifest
        .read_to_string(&mut content)
        .context("Failed to read JetBrains upgrade manifest")?;
    Ok(content.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("Main-Class") && !value.trim().is_empty()
        })
    }))
}

/// GH #63: JVM candidates for attaching to `pid`, most specific first.
///
/// The target IDE ships its own runtime, so the ambient environment is only a
/// fallback: the process executable itself when it is a `java` launcher, then a
/// `jbr/bin/java` beside any ancestor of that executable (the native `idea`
/// launcher lives in `<dist>/bin/`), then `JAVA_HOME`, then each `PATH` entry.
fn java_candidates_for_ide(
    ide_exe: Option<&Path>,
    java_home: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
) -> Vec<PathBuf> {
    let java_name = if cfg!(windows) { "java.exe" } else { "java" };
    let mut candidates = Vec::new();
    if let Some(exe) = ide_exe {
        if exe.file_name().and_then(|name| name.to_str()) == Some(java_name) {
            candidates.push(exe.to_path_buf());
        }
        for ancestor in exe.ancestors().skip(1) {
            candidates.push(ancestor.join("jbr").join("bin").join(java_name));
        }
    }
    if let Some(home) = java_home {
        candidates.push(home.join("bin").join(java_name));
    }
    if let Some(path_var) = path_var {
        candidates.extend(std::env::split_paths(path_var).map(|dir| dir.join(java_name)));
    }
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| seen.insert(candidate.clone()));
    candidates
}

fn resolve_java_for_ide(pid: u32, candidates: &[PathBuf]) -> Result<PathBuf> {
    if let Some(found) = candidates.iter().find(|candidate| candidate.is_file()) {
        return Ok(found.clone());
    }
    bail!(
        "no JVM found to run the JetBrains dynamic upgrader for pid {pid}; tried the IDE's bundled runtime, JAVA_HOME and PATH:\n{}",
        candidates
            .iter()
            .map(|candidate| format!("  {}", candidate.display()))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[cfg(not(test))]
fn java_executable_for_ide(pid: u32) -> Result<PathBuf> {
    let ide_exe = fs::read_link(format!("/proc/{pid}/exe")).ok();
    let java_home = std::env::var_os("JAVA_HOME").map(PathBuf::from);
    let path_var = std::env::var_os("PATH");
    let candidates = java_candidates_for_ide(
        ide_exe.as_deref(),
        java_home.as_deref(),
        path_var.as_deref(),
    );
    resolve_java_for_ide(pid, &candidates)
}

/// `#jbupgradereattach`: the dynamic upgrade verdict is the `ok:` status itself.
///
/// The IDE only writes `ok:<version>` after the replacement package is installed and a
/// fresh descriptor of that version owns a new classloader. Its trailing `documents=` /
/// `pending=` / `reattach_error=` fields are a receipt for open-document replica
/// re-registration, which converges against whichever controller owns each document's own
/// project root -- a property this install does not own. A shortfall there is therefore a
/// warning and never an install failure: the replacement bytes are already live, and the
/// editor keeps a bounded per-document retry armed. Aborting on it used to fail the whole
/// `make install` after the upgrade had landed, and the immediate retry then reported the
/// package byte-identical with no restart required.
fn jetbrains_upgrade_reattach_warning(pid: u32, status_line: &str) -> Option<String> {
    let receipt = status_line.trim();
    if let Some((_, error)) = receipt.split_once("reattach_error=") {
        return Some(format!(
            "JetBrains pid {pid} loaded the replacement plugin, but its open-document \
             reattach receipt was unavailable: {}. The upgrade is installed and live; \
             reopen an editor tab if one of its documents stops syncing.",
            error.trim()
        ));
    }
    // Pending paths are the receipt's last field, so a path containing `:` stays intact.
    let pending = receipt.split_once(":pending=")?.1.trim();
    let paths: Vec<&str> = pending.split(',').filter(|path| !path.is_empty()).collect();
    if paths.is_empty() {
        return None;
    }
    Some(format!(
        "JetBrains pid {pid} loaded the replacement plugin, but {} open document(s) had not \
         re-registered a replica yet: {}. The upgrade is installed and live; the editor \
         retries each document on its own bounded schedule.",
        paths.len(),
        paths.join(", ")
    ))
}

#[cfg(not(test))]
fn try_hot_upgrade_jetbrains(
    zip_path: &Path,
    target_dir: &Path,
    expected_version: &str,
) -> Result<Option<JetbrainsHotUpgrade>> {
    let pids = live_jetbrains_ide_pids()?;
    if pids.is_empty() {
        return Ok(None);
    }
    let package_range = jetbrains_zip_package_range(zip_path)?;
    let mut launcher = None;
    if let Some(refusal) =
        jetbrains_dynamic_upgrade_entry_point_refusal(package_range, expected_version, || {
            let extracted = extract_jetbrains_upgrade_launcher(zip_path)?;
            let has_main = jetbrains_upgrade_launcher_has_main_manifest(extracted.path())?;
            launcher = Some(extracted);
            Ok(has_main)
        })?
    {
        bail!("{refusal}");
    }
    let launcher = launcher.context("JetBrains upgrade launcher was not extracted")?;
    let archive = tempfile::Builder::new()
        .prefix("agent-doc-jb-package-")
        .suffix(".zip")
        .tempfile()
        .context("Failed to stage JetBrains package for dynamic install")?;
    fs::copy(zip_path, archive.path()).context("Failed to stage JetBrains package")?;
    let mut statuses = Vec::with_capacity(pids.len());
    for pid in pids {
        let java = java_executable_for_ide(pid)?;
        let output = Command::new(&java)
            .args(["--add-modules", "jdk.attach", "-jar"])
            .arg(launcher.path())
            .arg(pid.to_string())
            .arg(archive.path())
            .arg(target_dir)
            .arg(expected_version)
            .output()
            .with_context(|| {
                format!(
                    "Failed to launch JetBrains dynamic upgrader for pid {pid} with JVM {}",
                    java.display()
                )
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            bail!(
                "JetBrains dynamic upgrade failed for pid {pid}: {}{}",
                stdout.trim(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        statuses.push((pid, parse_jetbrains_upgrader_status(&stdout)));
    }
    Ok(jetbrains_hot_upgrade_from_statuses(
        statuses,
        target_dir,
        expected_version,
        |pid| {
            await_jetbrains_live_load(
                pid,
                target_dir,
                expected_version,
                JETBRAINS_LIVE_LOAD_PROOF_TIMEOUT,
            )
        },
    ))
}

/// The failure reason carried by a `staged:<version>:<reason>` upgrader status.
fn staged_upgrade_reason(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("staged:")?;
    let reason = rest.split_once(':').map_or("", |(_, reason)| reason).trim();
    Some(if reason.is_empty() {
        "the dynamic upgrade failed".to_string()
    } else {
        reason.to_string()
    })
}

/// Any in-IDE install (proven or not) already rewrote the shared plugin tree, so
/// it outranks a staging; it is a dynamic upgrade only when every such install
/// was proven live and no live owner of the target was left without the plugin.
/// Only when no in-IDE install ran does a staged package keep the tree as is,
/// and only when nothing staged either is a not-loaded owner reported.
fn jetbrains_hot_upgrade_verdict(
    upgraded: usize,
    unverified: Option<String>,
    not_loaded: Option<String>,
    staged: Option<String>,
) -> Option<JetbrainsHotUpgrade> {
    if upgraded > 0 || unverified.is_some() {
        return Some(match unverified.or(not_loaded) {
            Some(reason) => JetbrainsHotUpgrade::InstalledUnverified { reason },
            None => JetbrainsHotUpgrade::Upgraded {
                processes: upgraded,
            },
        });
    }
    if let Some(reason) = staged {
        return Some(JetbrainsHotUpgrade::StagedForRestart { reason });
    }
    not_loaded.map(|reason| JetbrainsHotUpgrade::NotLoaded { reason })
}

#[cfg(test)]
fn live_jetbrains_ide_pids() -> Result<Vec<u32>> {
    Ok(Vec::new())
}

#[cfg(test)]
fn try_hot_upgrade_jetbrains(
    _zip_path: &Path,
    _target_dir: &Path,
    _expected_version: &str,
) -> Result<Option<JetbrainsHotUpgrade>> {
    Ok(None)
}

fn jetbrains_zip_plugin_version(zip_path: &Path) -> Result<String> {
    let file = fs::File::open(zip_path).context("Failed to open JetBrains package")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read JetBrains package")?;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let file_name = name
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if let Some(version) = jetbrains_plugin_jar_version(file_name) {
            return Ok(version);
        }
    }
    bail!("JetBrains package has no versioned agent-doc plugin jar")
}

/// `#jbpluginvanish`: hidden work directory beside the plugin tree, on the same
/// filesystem, holding the staged extraction and the outgoing generation's
/// backup while a replacement is in flight. Its top level is neither a plugin
/// root (`lib/`) nor a descriptor, so the IDE never loads it.
const JETBRAINS_INSTALL_WORK_DIR: &str = ".agent-doc-jetbrains-install";
const JETBRAINS_PLUGIN_DIR: &str = "agent-doc-jetbrains";
const JETBRAINS_MODULAR_PLUGIN_DIR: &str = "agent-doc-jetbrains-262";

fn jetbrains_plugin_jar_version(file_name: &str) -> Option<String> {
    ["agent-doc-jetbrains-", "agent.doc-"]
        .into_iter()
        .find_map(|prefix| {
            let version = file_name.strip_prefix(prefix)?.strip_suffix(".jar")?;
            numeric_dot_version(version).map(|_| version.to_string())
        })
}

fn jetbrains_archive_relative(enclosed: &Path) -> Result<PathBuf> {
    [JETBRAINS_PLUGIN_DIR, JETBRAINS_MODULAR_PLUGIN_DIR]
        .into_iter()
        .find_map(|root| enclosed.strip_prefix(root).ok().map(Path::to_path_buf))
        .with_context(|| {
            format!(
                "Unexpected JetBrains package root: {} (expected {JETBRAINS_PLUGIN_DIR}/ or {JETBRAINS_MODULAR_PLUGIN_DIR}/)",
                enclosed.display()
            )
        })
}

/// Points in [`replace_jetbrains_plugin_tree_with`] where a test can inject a
/// failure (disk full, permission denied) to prove the old tree survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginTreeReplaceStep {
    /// About to write one extracted file into the staging tree.
    WriteStagedFile,
    /// The staged tree is verified; about to move the old tree aside.
    BackupOldTree,
    /// The old tree is aside; about to move the staged tree into place.
    SwapInStagedTree,
}

fn replace_jetbrains_plugin_tree(zip_path: &Path, target_dir: &Path) -> Result<()> {
    replace_jetbrains_plugin_tree_with(zip_path, target_dir, &mut |_| Ok(()))
}

/// `#jbpluginvanish`: replace `<target_dir>/agent-doc-jetbrains` atomically.
///
/// The old implementation removed the installed tree and then extracted the
/// package over the hole, so any failure in between (disk full, a truncated
/// package) left no plugin at all. Now the package is extracted into a staging
/// directory beside the tree, every file is fsynced and checked against the
/// package (size, CRC via the zip reader, a versioned plugin jar present), and
/// only then is the old tree renamed aside and the staged tree renamed in. A
/// failed swap renames the old tree back. The backup is removed only after the
/// new tree is in place.
fn replace_jetbrains_plugin_tree_with(
    zip_path: &Path,
    target_dir: &Path,
    fault: &mut dyn FnMut(PluginTreeReplaceStep) -> io::Result<()>,
) -> Result<()> {
    let dest = target_dir.join(JETBRAINS_PLUGIN_DIR);
    let work = target_dir.join(JETBRAINS_INSTALL_WORK_DIR);
    recover_interrupted_plugin_tree_swap(&dest, &work)?;
    fs::create_dir_all(&work).with_context(|| format!("Failed to create {}", work.display()))?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    );
    let staging = work.join(format!("staging-{nonce}"));
    let staged_tree = staging.join(JETBRAINS_PLUGIN_DIR);
    let backup = work.join(format!("backup-{nonce}"));

    let staged = extract_jetbrains_package_into(zip_path, &staging, fault)
        .and_then(|()| verify_staged_jetbrains_tree(zip_path, &staged_tree));
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir(&work);
        return Err(error.context(format!(
            "JetBrains plugin replacement aborted before touching {}; the installed plugin was kept",
            dest.display()
        )));
    }

    let had_old = dest.exists();
    if had_old {
        let moved =
            fault(PluginTreeReplaceStep::BackupOldTree).and_then(|()| fs::rename(&dest, &backup));
        if let Err(error) = moved {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_dir(&work);
            return Err(anyhow::Error::new(error).context(format!(
                "Failed to move the installed plugin {} aside; it was kept",
                dest.display()
            )));
        }
    }
    let swapped = fault(PluginTreeReplaceStep::SwapInStagedTree)
        .and_then(|()| fs::rename(&staged_tree, &dest));
    if let Err(error) = swapped {
        let restored = if had_old {
            fs::rename(&backup, &dest).map_err(|restore| {
                format!(
                    "; restoring the previous plugin from {} also failed: {restore}",
                    backup.display()
                )
            })
        } else {
            Ok(())
        };
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir(&work);
        return Err(anyhow::Error::new(error).context(format!(
            "Failed to move the new plugin into {}{}",
            dest.display(),
            restored
                .err()
                .unwrap_or_else(|| "; the previous plugin was restored".to_string())
        )));
    }
    let legacy_modular = target_dir.join(JETBRAINS_MODULAR_PLUGIN_DIR);
    if legacy_modular.exists() {
        fs::remove_dir_all(&legacy_modular).with_context(|| {
            format!(
                "Installed the selected package at {}, but failed to remove the obsolete modular plugin tree {}; remove it before restarting the IDE to avoid duplicate plugin IDs",
                dest.display(),
                legacy_modular.display()
            )
        })?;
    }
    let _ = fs::remove_dir_all(&staging);
    if had_old && let Err(error) = fs::remove_dir_all(&backup) {
        eprintln!(
            "[plugin] could not remove the previous plugin backup {}: {error}",
            backup.display()
        );
    }
    let _ = fs::remove_dir(&work);
    Ok(())
}

/// `#jbpluginvanish`: a process that died between moving the old tree aside and
/// moving the new one in leaves no plugin tree and a `backup-*` beside it. Put
/// the newest such backup back before anything else, then clear leftovers.
fn recover_interrupted_plugin_tree_swap(dest: &Path, work: &Path) -> Result<()> {
    let Ok(entries) = fs::read_dir(work) else {
        return Ok(());
    };
    let mut backups = Vec::new();
    let mut leftovers = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("backup-") && entry.path().join("lib").is_dir() {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            backups.push((modified, entry.path()));
        } else {
            leftovers.push(entry.path());
        }
    }
    backups.sort();
    if !dest.exists()
        && let Some((_, newest)) = backups.pop()
    {
        fs::rename(&newest, dest).with_context(|| {
            format!(
                "Failed to restore the interrupted plugin replacement's backup {} to {}",
                newest.display(),
                dest.display()
            )
        })?;
        eprintln!(
            "[plugin] restored {} from an interrupted replacement's backup",
            dest.display()
        );
    }
    for path in leftovers
        .into_iter()
        .chain(backups.into_iter().map(|(_, path)| path))
    {
        let _ = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
    }
    Ok(())
}

fn extract_jetbrains_package_into(
    zip_path: &Path,
    staging: &Path,
    fault: &mut dyn FnMut(PluginTreeReplaceStep) -> io::Result<()>,
) -> Result<()> {
    let file = fs::File::open(zip_path).context("Failed to open zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
    fs::create_dir_all(staging)
        .with_context(|| format!("Failed to create {}", staging.display()))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .with_context(|| format!("Unsafe path in JetBrains package: {}", entry.name()))?;
        let relative = jetbrains_archive_relative(&enclosed)?;
        // Normalize both published archive roots to the established on-disk
        // plugin directory. This also converges preview/manual modular installs
        // without teaching every activation/staging path a second live root.
        let out_path = staging.join(JETBRAINS_PLUGIN_DIR).join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .with_context(|| format!("Failed to create {}", out_path.display()))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        fault(PluginTreeReplaceStep::WriteStagedFile)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;
        let mut outfile = fs::File::create(&out_path)
            .with_context(|| format!("Failed to create {}", out_path.display()))?;
        // The zip reader verifies the entry's CRC once it is read to the end.
        let written = io::copy(&mut entry, &mut outfile)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;
        // Surface a delayed-allocation ENOSPC here, not after the swap.
        outfile
            .sync_all()
            .with_context(|| format!("Failed to flush {}", out_path.display()))?;
        if written != entry.size() {
            bail!(
                "Short write extracting {}: {written} of {} bytes",
                out_path.display(),
                entry.size()
            );
        }
    }
    Ok(())
}

/// The staged tree must hold every packaged file at its packaged size and a
/// versioned `lib/agent-doc-jetbrains-<v>.jar`; anything less never replaces
/// a working installation.
fn verify_staged_jetbrains_tree(zip_path: &Path, staged_tree: &Path) -> Result<()> {
    let file = fs::File::open(zip_path).context("Failed to open zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
    let mut packaged = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .with_context(|| format!("Unsafe path in JetBrains package: {}", entry.name()))?;
        let relative = jetbrains_archive_relative(&enclosed)?;
        let staged_len = fs::metadata(staged_tree.join(&relative))
            .map(|metadata| metadata.len())
            .with_context(|| format!("Staged plugin is missing {}", relative.display()))?;
        if staged_len != entry.size() {
            bail!(
                "Staged plugin file {} is {staged_len} bytes; the package holds {}",
                relative.display(),
                entry.size()
            );
        }
        packaged.insert(relative);
    }
    let has_plugin_jar = packaged.iter().any(|relative| {
        relative.parent() == Some(Path::new("lib"))
            && relative
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(jetbrains_plugin_jar_version)
                .is_some()
    });
    if !has_plugin_jar {
        bail!("Staged plugin has no versioned agent-doc plugin jar");
    }
    let mut staged = BTreeSet::new();
    collect_installed_plugin_files(staged_tree, staged_tree, &mut staged)?;
    if staged != packaged {
        bail!("Staged plugin files differ from the package's file list");
    }
    Ok(())
}

/// `#jbpluginvanish`: once the plugin tree holds a freshly installed generation
/// (`StagingPurge::All`), or before any install (`StagingPurge::Doomed`), drop
/// pending agent-doc stagings from the IDE's pending-install scripts so the
/// next IDE start cannot delete the plugin and fail to unzip its replacement.
fn purge_jetbrains_pending_stagings(
    target_dir: &Path,
    mode: agent_doc_fs::jetbrains_install::StagingPurge,
) {
    let roots = agent_doc_fs::jetbrains_install::jetbrains_system_roots();
    match agent_doc_fs::jetbrains_install::purge_pending_stagings_for(target_dir, &roots, mode) {
        Ok(purged) => {
            for staging in purged {
                let zips = staging
                    .zips
                    .iter()
                    .map(|zip| zip.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                eprintln!(
                    "[plugin] removed a pending JetBrains staging for {} from {} ({zips}): its next-start delete would have removed the plugin",
                    target_dir.display(),
                    staging.script.display()
                );
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_pending_staging_purged mode={mode:?} target={} script={} removed_lines={} zips={zips:?}",
                    target_dir.display(),
                    staging.script.display(),
                    staging.removed_lines
                ));
            }
        }
        Err(error) => eprintln!(
            "WARNING: could not purge pending JetBrains stagings for {}: {error:#}",
            target_dir.display()
        ),
    }
}

fn install_jetbrains_zip_into(
    zip_path: &Path,
    target_dir: &Path,
    expected_version: &str,
) -> Result<JetbrainsLocalInstallOutcome> {
    fs::create_dir_all(target_dir).context("Failed to create JetBrains plugins directory")?;
    // GH #115: serialize every install into this plugins directory. `agent-doc
    // upgrade` and a manual `plugin update` used to stage the same package two
    // minutes apart, and the second staging is what destroyed the install.
    let _install_lock = agent_doc_fs::jetbrains_install::lock_jetbrains_install(target_dir)?;
    // `#jbpluginvanish`: a staging whose package is gone can only delete the
    // plugin at the next IDE start; drop it before deciding anything else.
    purge_jetbrains_pending_stagings(
        target_dir,
        agent_doc_fs::jetbrains_install::StagingPurge::Doomed,
    );
    if jetbrains_local_zip_matches_installation(zip_path, target_dir)? {
        return Ok(JetbrainsLocalInstallOutcome::Unchanged);
    }
    if let Some(outcome) = already_staged_outcome(
        target_dir,
        &agent_doc_fs::jetbrains_install::jetbrains_system_roots(),
        expected_version,
    ) {
        return Ok(outcome);
    }
    let outcome = install_jetbrains_package_bytes(
        zip_path,
        target_dir,
        expected_version,
        dynamic_upgrade_enabled(),
        || try_hot_upgrade_jetbrains(zip_path, target_dir, expected_version),
        live_jetbrains_ide_pids,
    )?;
    // A staged package is applied by the IDE at its next start, so the plugin
    // tree still holds the live generation's bytes by design.
    if !matches!(
        outcome,
        JetbrainsLocalInstallOutcome::StagedForRestart { .. }
    ) && !jetbrains_local_zip_matches_installation(zip_path, target_dir)?
    {
        bail!(
            "JetBrains package verification failed in {}: installed bytes differ from the package",
            target_dir.display()
        );
    }
    // `#jbpluginvanish`: the tree now holds the new generation (restart-free
    // upgrade or direct replacement). Any staging still queued would run its
    // `delete:` over it at the next IDE start and either downgrade it or, with
    // its package gone, leave no plugin at all.
    if !matches!(
        outcome,
        JetbrainsLocalInstallOutcome::StagedForRestart { .. }
    ) {
        purge_jetbrains_pending_stagings(
            target_dir,
            agent_doc_fs::jetbrains_install::StagingPurge::All,
        );
    }
    Ok(outcome)
}

/// GH #115: when the IDE's pending-install queue already holds a viable staging
/// of `expected_version` for `target_dir`, staging it again is never useful (the
/// live JVM already declined the restart-free swap) and used to be destructive:
/// each staging appends its own delete+unzip block. Report the existing staging
/// instead, and make sure the restart-required marker names it.
fn already_staged_outcome(
    target_dir: &Path,
    system_roots: &[PathBuf],
    expected_version: &str,
) -> Option<JetbrainsLocalInstallOutcome> {
    let staging = agent_doc_fs::jetbrains_install::pending_stagings_for(target_dir, system_roots)
        .into_iter()
        .find(|staging| staging.zip_present && staging.version == expected_version)?;
    let marker = target_dir.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
    let base_reason = format!(
        "v{expected_version} is already staged for the next IDE start ({} in {}); not staging it again",
        staging.zip.display(),
        staging.script.display()
    );
    let recorded_marker = fs::read_to_string(&marker).ok();
    let recorded = recorded_marker
        .as_deref()
        .and_then(agent_doc_fs::jetbrains_install::staged_version_from_restart_marker);
    // GH #180: reusing a viable staging must retain why its restart-free
    // upgrade was unavailable. `PluginTargetOutcome` classifies permanence from
    // this reason, so replacing it with only "already staged" made summaries
    // regress to `restart_free_unavailable=false` on every subsequent run.
    let reason = if recorded.as_deref() == Some(expected_version) {
        recorded_marker
            .as_deref()
            .and_then(|body| {
                body.lines()
                    .find(|line| agent_doc_declined_dynamic_upgrade(line))
            })
            .map(|original| format!("{base_reason}; original staging reason: {original}"))
            .unwrap_or(base_reason)
    } else {
        base_reason
    };
    if recorded.as_deref() != Some(expected_version) {
        record_restart_required_marker(
            &marker,
            &format!("staged for restart: {reason}"),
            Some(expected_version),
            installed_jetbrains_plugin_version(target_dir).as_deref(),
        );
    }
    eprintln!("WARNING: JetBrains plugin {reason}. Restart the IDE to load it.");
    log_jetbrains_upgrade_decision(&format!(
        "plugin_dynamic_upgrade outcome=already_staged staged_version={expected_version} target={} zip={}",
        target_dir.display(),
        staging.zip.display()
    ));
    Some(JetbrainsLocalInstallOutcome::StagedForRestart { reason })
}

/// GH #63: the restart-free dynamic upgrade is an optimization, never the
/// update itself. When it cannot run (no JVM, attach refused, a platform
/// signature the upgrader cannot call) or `--no-dynamic` skips it, the package
/// is still replaced on disk -- staged beside the tree and swapped in by rename (`#jbpluginvanish`), so a
/// live IDE keeps reading the old inodes -- and the caller is told to restart.
fn install_jetbrains_package_bytes(
    zip_path: &Path,
    target_dir: &Path,
    expected_version: &str,
    dynamic: bool,
    hot_upgrade: impl FnOnce() -> Result<Option<JetbrainsHotUpgrade>>,
    live_pids: impl FnOnce() -> Result<Vec<u32>>,
) -> Result<JetbrainsLocalInstallOutcome> {
    let restart_marker =
        target_dir.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
    let restart_reason = if dynamic {
        match hot_upgrade() {
            Ok(Some(JetbrainsHotUpgrade::Upgraded { processes })) => {
                clear_restart_required_marker(&restart_marker);
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_dynamic_upgrade outcome=hot_upgraded processes={processes} version={expected_version} target={}",
                    target_dir.display()
                ));
                return Ok(JetbrainsLocalInstallOutcome::HotUpgraded { processes });
            }
            Ok(Some(JetbrainsHotUpgrade::StagedForRestart { reason })) => {
                // `#jbstageonfail`: never replace jars under the live JVM once its
                // own hot-swap failed. The restart verdict is still recorded so
                // preflight advises a restart rather than another install.
                eprintln!(
                    "WARNING: {}",
                    dynamic_upgrade_fallback_warning(
                        &reason,
                        "staged it for the next IDE start instead"
                    )
                );
                if agent_doc_declined_dynamic_upgrade(&reason) {
                    print_permanent_dynamic_upgrade_loss_once();
                }
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_dynamic_upgrade outcome=staged_for_restart declined_by={} staged_version={expected_version} target={} reason={reason:?}",
                    dynamic_upgrade_decliner(&reason),
                    target_dir.display()
                ));
                let reason = format!("dynamic upgrade unavailable, staged for restart: {reason}");
                // GH #87: name the staged version so preflight can tell a staged
                // install (restart is the remedy) from a plain stale one. Use the
                // version the caller already read from the package; never re-derive
                // it from `zip_path`'s filename -- release installs pass a
                // `NamedTempFile` (`.tmpXXXXXX`) whose name carries no version.
                // GH #115: also record the generation the staging replaces, so a
                // post-restart check can tell applied / not applied / destroyed.
                record_restart_required_marker(
                    &restart_marker,
                    &reason,
                    Some(expected_version),
                    installed_jetbrains_plugin_version(target_dir).as_deref(),
                );
                return Ok(JetbrainsLocalInstallOutcome::StagedForRestart { reason });
            }
            Ok(Some(JetbrainsHotUpgrade::InstalledUnverified { reason })) => {
                // `#jbdynamicfalsereport`: never claim a dynamic replacement the
                // live IDE did not prove. The in-IDE install already wrote the
                // tree; rewriting it again would unlink jars that JVM may map.
                let reason = format!("dynamic upgrade not verified live: {reason}");
                eprintln!("WARNING: {reason}");
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_dynamic_upgrade outcome=restart_required declined_by=unverified_live_load version={expected_version} target={} reason={reason:?}",
                    target_dir.display()
                ));
                if !jetbrains_local_zip_matches_installation(zip_path, target_dir)? {
                    replace_jetbrains_plugin_tree(zip_path, target_dir)?;
                }
                record_restart_required_marker(&restart_marker, &reason, None, None);
                return Ok(JetbrainsLocalInstallOutcome::RestartRequired { reason });
            }
            Ok(Some(JetbrainsHotUpgrade::NotLoaded { reason })) => {
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_dynamic_upgrade outcome=restart_required declined_by=plugin_not_loaded version={expected_version} target={} reason={reason:?}",
                    target_dir.display()
                ));
                Some(reason)
            }
            Ok(None) => None,
            Err(error) => {
                // GH #67: the reason is printed once, in the final restart message.
                let reason = format!("{error:#}");
                eprintln!(
                    "WARNING: {}",
                    dynamic_upgrade_fallback_warning(&reason, "replacing the plugin files instead")
                );
                log_jetbrains_upgrade_decision(&format!(
                    "plugin_dynamic_upgrade outcome=restart_required declined_by={} version={expected_version} target={} reason={reason:?}",
                    dynamic_upgrade_decliner(&reason),
                    target_dir.display()
                ));
                Some(format!("dynamic upgrade unavailable: {reason}"))
            }
        }
    } else {
        match live_pids() {
            Ok(pids) if pids.is_empty() => None,
            Ok(pids) => Some(format!(
                "--no-dynamic skipped the restart-free upgrade for live JetBrains pid(s) {}",
                pids.iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Err(error) => Some(format!(
                "--no-dynamic; live JetBrains processes could not be enumerated: {error:#}"
            )),
        }
    };
    replace_jetbrains_plugin_tree(zip_path, target_dir)?;
    match &restart_reason {
        // GH #67: record the refusal so preflight's `plugin_bytes_superseded` advises
        // a restart instead of prescribing another install that re-learns the same
        // refusal every cycle. It sits beside the plugin tree, never inside it.
        Some(reason) => record_restart_required_marker(&restart_marker, reason, None, None),
        None => clear_restart_required_marker(&restart_marker),
    }
    Ok(match restart_reason {
        Some(reason) => JetbrainsLocalInstallOutcome::RestartRequired { reason },
        None => JetbrainsLocalInstallOutcome::Installed,
    })
}

/// Stable prefix the JetBrains upgrade action puts on a genuine platform refusal
/// (`JetBrainsPluginUpgradeAction.DYNAMIC_UNLOAD_REFUSED`).
const JETBRAINS_DYNAMIC_UNLOAD_REFUSED: &str = "plugin cannot unload dynamically";

/// GH #108: stable prefix the JetBrains upgrade action puts on agent-doc's own
/// decision not to attempt the swap (`JetBrainsPluginUpgradeAction.DYNAMIC_UPGRADE_DECLINED`).
const JETBRAINS_DYNAMIC_UPGRADE_DECLINED: &str = "agent-doc declined the restart-free upgrade";

/// GH #108: the same decline as emitted by the 0.35.435-0.35.441 upgrader, which
/// wore the platform-refusal prefix although the IDE was never asked. Kept so a
/// launcher from an older package is still attributed correctly.
const LEGACY_ASYNC_RETIREMENT_DECLINE: &str =
    "verifies the outgoing classloader only after loading the replacement";

/// GH #108: did agent-doc itself decline the restart-free upgrade (an
/// asynchronous classloader-retirement platform), rather than the IDE refusing it?
fn agent_doc_declined_dynamic_upgrade(reason: &str) -> bool {
    reason.contains(JETBRAINS_DYNAMIC_UPGRADE_DECLINED)
        || reason.contains(LEGACY_ASYNC_RETIREMENT_DECLINE)
}

/// Who stopped the restart-free upgrade, as an `ops.log` field value.
fn dynamic_upgrade_decliner(reason: &str) -> &'static str {
    if agent_doc_declined_dynamic_upgrade(reason) {
        "agent-doc"
    } else if reason.contains(JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE) {
        "modular_package"
    } else if reason.contains(JETBRAINS_DYNAMIC_UNLOAD_REFUSED) {
        "ide"
    } else {
        "upgrader_failure"
    }
}

/// GH #80: say whether the platform declined the unload or the upgrade never
/// reached it. "Refused" used to cover both, so an upgrader that could not even
/// link `DynamicPlugins$UnloadPluginOptions` read as a platform policy -- the one
/// outcome where accepting the restart is the right conclusion. GH #108: a
/// decline agent-doc took before consulting the IDE names agent-doc.
fn dynamic_upgrade_fallback_warning(reason: &str, fallback: &str) -> String {
    let cause = if agent_doc_declined_dynamic_upgrade(reason) {
        "agent-doc declined the restart-free upgrade: this JetBrains build retires plugin classloaders asynchronously, so there is no safe synchronous swap point"
    } else if reason.contains(JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE) {
        // `#jb262dynupgrade`: an expected property of the 262 package line, not
        // a failure and not a legacy build.
        "the exact-262 modular plugin package has no restart-free upgrade entry point, so the running IDE keeps its current plugin generation until restart"
    } else if reason.contains(JETBRAINS_DYNAMIC_UNLOAD_REFUSED) {
        "the IDE refused the restart-free upgrade"
    } else {
        "the restart-free upgrade failed before the IDE could accept or refuse it"
    };
    format!("{cause}; {fallback}.")
}

/// GH #108: printed once per process, because the decline is a property of the
/// JetBrains build, not of this attempt -- retrying can never succeed there.
fn permanent_dynamic_upgrade_loss_note() -> &'static str {
    "NOTE: restart-free plugin upgrade is permanently unavailable on this JetBrains build (it retires plugin classloaders asynchronously); every future install will stage the same way. Use `agent-doc upgrade` (or the plugin install) and then restart the IDE; re-running the install will not load the plugin without a restart."
}

fn print_permanent_dynamic_upgrade_loss_once() {
    static PRINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !PRINTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("{}", permanent_dynamic_upgrade_loss_note());
    }
}

/// GH #108: the plugin-upgrade decision is the one path in the install flow
/// whose only record used to be terminal output. Write one durable `ops.log`
/// line for the project owning the working directory (best-effort).
#[cfg(not(test))]
fn log_jetbrains_upgrade_decision(message: &str) {
    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|cwd| agent_doc_project_root_io::project_root_containing(&cwd))
    else {
        return;
    };
    let _ = agent_doc_ops_log_io::append_ops_log_at_project(
        &root,
        message,
        agent_doc_ops_log_io::OpsLogTracking {
            doc_stem: None,
            session_id: None,
            turn_id: None,
        },
    );
}

#[cfg(test)]
thread_local! {
    static LOGGED_UPGRADE_DECISIONS: std::cell::RefCell<Vec<String>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn log_jetbrains_upgrade_decision(message: &str) {
    LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow_mut().push(message.to_string()));
}

/// The first line is the refusal reason; a staged install adds a
/// `staged_version=<v>` line (GH #87) and the version it replaces as
/// `previous_version=<v>` (GH #115).
fn record_restart_required_marker(
    marker: &Path,
    reason: &str,
    staged_version: Option<&str>,
    previous_version: Option<&str>,
) {
    let mut body = format!("{}\n", reason.replace('\n', " "));
    if let Some(version) = staged_version {
        body.push_str(&format!(
            "{}{version}\n",
            agent_doc_fs::jetbrains_install::STAGED_VERSION_MARKER_PREFIX
        ));
        if let Some(previous) = previous_version {
            body.push_str(&format!(
                "{}{previous}\n",
                agent_doc_fs::jetbrains_install::PREVIOUS_VERSION_MARKER_PREFIX
            ));
        }
    }
    if let Err(err) = fs::write(marker, body) {
        eprintln!(
            "[plugin] could not record the restart-required verdict at {}: {err}",
            marker.display()
        );
    }
}

fn clear_restart_required_marker(marker: &Path) {
    if let Err(err) = fs::remove_file(marker)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "[plugin] could not clear the restart-required verdict at {}: {err}",
            marker.display()
        );
    }
}

fn collect_installed_plugin_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("Failed to read {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_installed_plugin_files(root, &path, files)?;
        } else if file_type.is_file() {
            files.insert(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn jetbrains_local_zip_matches_installation(zip_path: &Path, target_dir: &Path) -> Result<bool> {
    let installed_root = target_dir.join("agent-doc-jetbrains");
    if !installed_root.is_dir() || target_dir.join(JETBRAINS_MODULAR_PLUGIN_DIR).exists() {
        return Ok(false);
    }

    let file = fs::File::open(zip_path).context("Failed to open zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;
    let mut packaged_files = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .with_context(|| format!("Unsafe path in JetBrains package: {}", entry.name()))?;
        let relative = jetbrains_archive_relative(&enclosed)?;
        let installed = installed_root.join(&relative);
        let mut packaged = Vec::new();
        entry.read_to_end(&mut packaged)?;
        if fs::read(&installed).ok().as_deref() != Some(packaged.as_slice()) {
            return Ok(false);
        }
        packaged_files.insert(relative);
    }

    let mut installed_files = BTreeSet::new();
    collect_installed_plugin_files(&installed_root, &installed_root, &mut installed_files)?;
    Ok(installed_files == packaged_files)
}

fn install_jetbrains_local_zip_into(
    zip_path: &Path,
    target_dir: &Path,
) -> Result<JetbrainsLocalInstallOutcome> {
    let expected_version = local_jetbrains_zip_version(zip_path)?;
    let packaged_version = jetbrains_zip_plugin_version(zip_path)?;
    if packaged_version != expected_version {
        bail!(
            "JetBrains package version mismatch: filename says {}, plugin jar says {}",
            expected_version,
            packaged_version
        );
    }
    eprintln!("Installing from local build: {}", zip_path.display());
    let outcome = install_jetbrains_zip_into(zip_path, target_dir, &expected_version)?;

    let installed_version = installed_jetbrains_plugin_version(target_dir).with_context(|| {
        format!(
            "JetBrains package verification failed in {}: no agent-doc plugin jar found",
            target_dir.display()
        )
    })?;
    verify_local_install_version(&outcome, target_dir, &expected_version, &installed_version)?;
    Ok(outcome)
}

/// A staged package is applied by the IDE at its next start, so the plugin tree
/// still holds the LIVE generation's jar by design (`#jbstageonfail`), the same
/// exemption `install_jetbrains_zip_into` makes for its byte comparison. Without
/// it, every staged `--local` install reported "built N, installed N-1" and
/// failed `make install-full` after the staging had in fact succeeded.
fn verify_local_install_version(
    outcome: &JetbrainsLocalInstallOutcome,
    target_dir: &Path,
    expected_version: &str,
    installed_version: &str,
) -> Result<()> {
    if matches!(
        outcome,
        JetbrainsLocalInstallOutcome::StagedForRestart { .. }
    ) || installed_version == expected_version
    {
        return Ok(());
    }
    bail!(
        "JetBrains package verification failed in {}: built {}, installed {}",
        target_dir.display(),
        expected_version,
        installed_version
    );
}

fn install_vscode_local() -> Result<()> {
    let code = require_code_cmd()?;
    let project_root = find_local_build_dir()?;
    let dist_dir = project_root.join("editors/vscode");
    let vsix = find_local_vscode_vsix(&dist_dir)?;

    eprintln!("Installing from local build: {}", vsix.display());

    let status = std::process::Command::new(code)
        .args(["--install-extension"])
        .arg(&vsix)
        .status()
        .with_context(|| format!("Failed to run `{code} --install-extension`"))?;

    if !status.success() {
        bail!("`{code} --install-extension` exited with {status}");
    }

    eprintln!("Extension installed via `{code}`.");
    Ok(())
}

fn find_local_vscode_vsix(dist_dir: &std::path::Path) -> Result<PathBuf> {
    let manifest_path = dist_dir.join("package.json");
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .with_context(|| format!("Missing version in {}", manifest_path.display()))?;
    let vsix = dist_dir.join(format!("agent-doc-{version}.vsix"));
    if !vsix.is_file() {
        bail!(
            "No VSIX matching package.json version {version} at {}; run `npm run package` in {}",
            vsix.display(),
            dist_dir.display()
        );
    }
    Ok(vsix)
}

fn installed_jetbrains_plugin_version(target_dir: &std::path::Path) -> Option<String> {
    [JETBRAINS_PLUGIN_DIR, JETBRAINS_MODULAR_PLUGIN_DIR]
        .into_iter()
        .filter_map(|plugin_dir| fs::read_dir(target_dir.join(plugin_dir).join("lib")).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = jetbrains_plugin_jar_version(&name)?;
            let key = numeric_dot_version(&version)?;
            Some((key, version))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, version)| version)
}

fn numeric_dot_version(version: &str) -> Option<Vec<u32>> {
    version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<_, _>>()
        .ok()
}

fn jetbrains_version_cmp(left: &str, right: &str) -> Result<CmpOrdering> {
    let left_key = numeric_dot_version(left)
        .with_context(|| format!("Invalid installed JetBrains plugin version {left:?}"))?;
    let right_key = numeric_dot_version(right)
        .with_context(|| format!("Invalid JetBrains package version {right:?}"))?;
    Ok(left_key.cmp(&right_key))
}

#[cfg(test)]
fn parse_local_jetbrains_zip_version(name: &str) -> Option<Vec<u32>> {
    let base = name.strip_prefix("agent-doc-jetbrains-")?;
    let version = base
        .strip_suffix("-signed.zip")
        .or_else(|| base.strip_suffix(".zip"))?;
    version
        .split('.')
        .map(|part| part.parse::<u32>().ok())
        .collect()
}

#[cfg(test)]
fn find_best_local_zip(dist_dir: &std::path::Path) -> Option<PathBuf> {
    fs::read_dir(dist_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("agent-doc-jetbrains-") || !name.ends_with(".zip") {
                return None;
            }
            let version = parse_local_jetbrains_zip_version(&name)?;
            let is_signed = name.ends_with("-signed.zip");
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some((version, is_signed, modified, e.path()))
        })
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        })
        .map(|(_, _, _, path)| path)
}

#[cfg(test)]
fn find_local_zip(dist_dir: &std::path::Path, signed: bool) -> Option<PathBuf> {
    fs::read_dir(dist_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let is_signed = name.ends_with("-signed.zip");
            if !name.starts_with("agent-doc-jetbrains-") || !name.ends_with(".zip") {
                return None;
            }
            if signed != is_signed {
                return None;
            }
            let version = parse_local_jetbrains_zip_version(&name)?;
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some((version, modified, e.path()))
        })
        // Prefer the highest plugin version; only fall back to mtime within the same version.
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, _, path)| path)
}

pub fn update(editor: &str) -> Result<()> {
    update_with_plugins_dir(editor, None)
}

pub fn update_with_plugins_dir(editor: &str, plugins_dir: Option<&Path>) -> Result<()> {
    let live_plugin_replaced = match editor {
        "jetbrains" | "jb" | "idea" => {
            let dirs = jetbrains_plugin_dirs();
            let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
            let package_range = jetbrains_package_range(&target_dir)?;
            let release = fetch_release_for_asset(package_range.asset_prefix(), "zip")?;
            let (_, asset) = jetbrains_release_asset_for_target(&release, &target_dir)?;
            let version = package_range
                .package_version(asset.name)
                .context("JetBrains release asset has no valid package version")?;
            match jetbrains_release_decision_for(&target_dir, package_range, &version)? {
                JetbrainsReleaseDecision::Current => {
                    eprintln!(
                        "JetBrains plugin ({}) is already at or above v{version}.",
                        package_range.asset_prefix()
                    );
                    return Ok(());
                }
                JetbrainsReleaseDecision::Replace { cross_line: true } => eprintln!(
                    "{} holds the other JetBrains compatibility line; replacing it with {}.",
                    jetbrains_target_label(&target_dir),
                    asset.name
                ),
                JetbrainsReleaseDecision::Replace { cross_line: false } => {}
            }
            matches!(
                install_jetbrains_into(&release, &target_dir)?,
                JetbrainsLocalInstallOutcome::HotUpgraded { .. }
            )
        }
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains updates");
            }
            let release = fetch_release_for_asset("agent-doc", "vsix")?;
            // VS Code/Cursor handles update-in-place via --install-extension
            install_vscode(&release)?;
            false
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    };
    if live_plugin_replaced {
        crate::runtime_update::recycle_existing_runtimes_after_live_plugin_update("plugin-update");
    }
    Ok(())
}

/// GH #114: which editor family a reconciled plugin target belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginEditorFamily {
    JetBrains,
    VsCode,
}

/// GH #114: what reconciliation did to one installed editor plugin target.
/// A bare "updated" count flattened these, so a staged target (nothing
/// installed until the IDE restarts) used to be reported as updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginTargetOutcome {
    /// Written to disk with no live editor holding the previous generation
    /// (JetBrains), or handed to the editor's own extension manager (VS Code).
    Installed,
    /// A live JetBrains IDE swapped the plugin without a restart.
    HotUpgraded,
    /// Already at (or ahead of) the release; nothing was written.
    Unchanged,
    /// A live IDE staged the package for its next start; the plugin tree still
    /// holds the previous generation. `permanent`: agent-doc declined the
    /// restart-free upgrade on this JetBrains build, so no retry can avoid the
    /// restart there.
    StagedForRestart { permanent: bool },
    /// The files were replaced on disk while a live IDE keeps the previous
    /// generation loaded.
    RestartRequired,
    /// GH #233: the target's proven platform build lies outside every
    /// published package range, so nothing was installed and the plugin
    /// already there (`version` on the report) stays in place. Not a failure:
    /// no retry or manual install can succeed until a release declares `build`.
    UnsupportedPlatform { build: u32 },
}

impl PluginTargetOutcome {
    fn from_jetbrains(outcome: &JetbrainsLocalInstallOutcome) -> Self {
        match outcome {
            JetbrainsLocalInstallOutcome::Installed => Self::Installed,
            JetbrainsLocalInstallOutcome::HotUpgraded { .. } => Self::HotUpgraded,
            JetbrainsLocalInstallOutcome::Unchanged => Self::Unchanged,
            JetbrainsLocalInstallOutcome::StagedForRestart { reason } => Self::StagedForRestart {
                permanent: agent_doc_declined_dynamic_upgrade(reason),
            },
            JetbrainsLocalInstallOutcome::RestartRequired { .. } => Self::RestartRequired,
        }
    }

    fn needs_restart(&self) -> bool {
        matches!(self, Self::StagedForRestart { .. } | Self::RestartRequired)
    }
}

/// GH #114: one reconciled target. `label` names it for the operator (the
/// JetBrains IDE data directory, e.g. `IntelliJIdea2026.3`, or the VS Code CLI);
/// `version` is the plugin package version it was reconciled against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTargetReport {
    pub family: PluginEditorFamily,
    pub label: String,
    pub version: String,
    pub outcome: PluginTargetOutcome,
}

/// GH #114: the per-target result of `update_all_installed`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginReconcileReport {
    pub targets: Vec<PluginTargetReport>,
}

impl PluginReconcileReport {
    fn count(&self, predicate: impl Fn(&PluginTargetOutcome) -> bool) -> usize {
        self.targets
            .iter()
            .filter(|target| predicate(&target.outcome))
            .count()
    }

    /// Targets whose bytes or staged package changed in this reconciliation.
    pub fn changed(&self) -> usize {
        self.count(|outcome| {
            !matches!(
                outcome,
                PluginTargetOutcome::Unchanged | PluginTargetOutcome::UnsupportedPlatform { .. }
            )
        })
    }

    /// GH #233: one warning per target left on its installed plugin because
    /// its platform build is outside every published range. Names the build,
    /// the supported ranges and the version left installed; offers no retry or
    /// manual-install remedy, because none can succeed.
    pub fn unsupported_platform_warnings(&self) -> Vec<String> {
        self.targets
            .iter()
            .filter_map(|target| match target.outcome {
                PluginTargetOutcome::UnsupportedPlatform { build } => Some(format!(
                    "WARNING: {label} runs JetBrains platform build {build}, outside every published agent-doc-jetbrains range ({ranges}); left plugin v{version} installed there unchanged. This is not an install failure: no agent-doc-jetbrains package declares build {build} yet.",
                    label = target.label,
                    version = target.version,
                    ranges = agent_doc_fs::jetbrains_install::JETBRAINS_SUPPORTED_RANGES,
                )),
                _ => None,
            })
            .collect()
    }

    /// Targets that proved a running editor replaced its plugin generation.
    pub fn hot_upgraded(&self) -> usize {
        self.count(|outcome| *outcome == PluginTargetOutcome::HotUpgraded)
    }

    fn counts(&self) -> [(usize, &'static str, &'static str); 6] {
        use PluginTargetOutcome as O;
        [
            (
                self.count(|o| *o == O::HotUpgraded),
                "hot-upgraded",
                "hot_upgraded",
            ),
            (self.count(|o| *o == O::Installed), "installed", "installed"),
            (
                self.count(|o| matches!(o, O::StagedForRestart { .. })),
                "staged for restart",
                "staged_for_restart",
            ),
            (
                self.count(|o| *o == O::RestartRequired),
                "replaced under a live IDE",
                "restart_required",
            ),
            (self.count(|o| *o == O::Unchanged), "unchanged", "unchanged"),
            (
                self.count(|o| matches!(o, O::UnsupportedPlatform { .. })),
                "unsupported platform",
                "unsupported_platform",
            ),
        ]
    }

    /// The closing lines of a plugin reconciliation, derived only from the
    /// per-target outcomes. A restart the code knows is needed is stated as an
    /// instruction; the conditional "if it does not pick it up on its own" is
    /// kept for VS Code, whose running window may genuinely reload by itself.
    pub fn summary(&self, release: &str) -> String {
        let summary = self.reconciled_summary(release);
        let unsupported = self.unsupported_platform_warnings();
        if unsupported.is_empty() {
            return summary;
        }
        let mut lines = vec![summary];
        lines.extend(unsupported);
        lines.join("\n")
    }

    fn reconciled_summary(&self, release: &str) -> String {
        if self.changed() == 0 {
            if self.targets.iter().any(|target| {
                matches!(
                    target.outcome,
                    PluginTargetOutcome::UnsupportedPlatform { .. }
                )
            }) {
                return format!(
                    "Installed editor plugins on supported IDE builds already match v{release}."
                );
            }
            return format!("Installed editor plugins already match v{release}.");
        }
        let counts = self
            .counts()
            .iter()
            .filter(|(count, _, _)| *count > 0)
            .map(|(count, label, _)| format!("{count} {label}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut lines = vec![format!(
            "Editor plugins reconciled with the v{release} release: {counts}."
        )];
        for target in &self.targets {
            let (label, version) = (&target.label, &target.version);
            let line = match (target.family, &target.outcome) {
                (_, PluginTargetOutcome::StagedForRestart { permanent: true }) => format!(
                    "Restart {label} to load plugin v{version}; restart-free upgrade is unavailable on this build."
                ),
                (_, PluginTargetOutcome::StagedForRestart { permanent: false }) => format!(
                    "Restart {label} to load plugin v{version}; the restart-free upgrade did not complete, so it was staged for the next start."
                ),
                (_, PluginTargetOutcome::RestartRequired) => format!(
                    "Restart {label} to load plugin v{version}; the running IDE keeps the previous plugin until then."
                ),
                (PluginEditorFamily::JetBrains, PluginTargetOutcome::Installed) => format!(
                    "{label}: plugin v{version} installed; no live IDE held it, so its next start loads it."
                ),
                (PluginEditorFamily::VsCode, PluginTargetOutcome::Installed) => format!(
                    "{label}: extension v{version} installed; reload the editor window if it does not pick it up on its own."
                ),
                (
                    _,
                    PluginTargetOutcome::HotUpgraded
                    | PluginTargetOutcome::Unchanged
                    | PluginTargetOutcome::UnsupportedPlatform { .. },
                ) => continue,
            };
            lines.push(line);
        }
        if self.targets.iter().any(|target| {
            target.family == PluginEditorFamily::JetBrains && target.outcome.needs_restart()
        }) {
            // The plugin reloads `libagent_doc` itself when the file's mtime
            // changes; this process has no receipt of that reload, so the line
            // describes the mechanism rather than claiming it happened.
            lines.push(
                "The native library is reloaded by the running IDE when its file changes; only the plugin needs the restart."
                    .to_string(),
            );
        }
        lines.join("\n")
    }

    /// One `ops.log` record of the reconciliation outcome.
    pub fn ops_log_line(&self, release: &str) -> String {
        let counts = self
            .counts()
            .iter()
            .map(|(count, _, key)| format!("{key}={count}"))
            .collect::<Vec<_>>()
            .join(" ");
        let restart_targets = self
            .targets
            .iter()
            .filter(|target| target.outcome.needs_restart())
            .map(|target| format!("{}@{}", target.label, target.version))
            .collect::<Vec<_>>()
            .join(",");
        let permanent = self.targets.iter().any(|target| {
            target.outcome == PluginTargetOutcome::StagedForRestart { permanent: true }
        });
        format!(
            "plugin_upgrade_summary release={release} {counts} restart_targets={restart_targets:?} restart_free_unavailable={permanent}"
        )
    }
}

/// GH #114: print the reconciliation summary and record it in `ops.log`
/// (best-effort, like the per-install upgrade decision).
pub fn report_reconcile_summary(report: &PluginReconcileReport, release: &str) {
    eprintln!("{}", report.summary(release));
    if !report.targets.is_empty() {
        log_jetbrains_upgrade_decision(&report.ops_log_line(release));
    }
}

/// The operator-facing name of a JetBrains plugins directory: the IDE data
/// directory (`IntelliJIdea2026.3`) rather than its `plugins` child.
fn jetbrains_target_label(target_dir: &Path) -> String {
    let named = if target_dir.file_name().is_some_and(|name| name == "plugins") {
        target_dir.parent()
    } else {
        Some(target_dir)
    };
    named
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| target_dir.display().to_string())
}

/// One JetBrains target in a release reconciliation, already bound to the
/// exact asset of its own compatibility range.
#[derive(Debug, Clone, PartialEq, Eq)]
struct JetbrainsReleasePlanEntry {
    target: PathBuf,
    range: JetbrainsPackageRange,
    asset_name: String,
    version: String,
    decision: JetbrainsReleaseDecision,
}

#[derive(Debug, Default)]
struct JetbrainsReleasePlan {
    releases: BTreeMap<JetbrainsPackageRange, Value>,
    entries: Vec<JetbrainsReleasePlanEntry>,
    /// GH #233: targets whose proven platform build no published package
    /// declares. They are left untouched and reported, never counted as errors.
    unsupported: Vec<(PathBuf, u32)>,
    errors: Vec<String>,
}

/// Bind every installed JetBrains target to its own range's release asset.
/// Each range fetches its own release (the newest one that carries that
/// range's asset), so a 262 target never waits on, or receives, the classic
/// line and vice versa. A target whose platform build cannot be proven is
/// reported as an error and left untouched; the others still converge.
fn plan_jetbrains_release_reconcile(
    targets: Vec<PathBuf>,
    mut fetch_release: impl FnMut(&str) -> Result<Value>,
) -> JetbrainsReleasePlan {
    let mut plan = JetbrainsReleasePlan::default();
    let mut targets_by_range = BTreeMap::<JetbrainsPackageRange, Vec<PathBuf>>::new();
    for target in targets {
        if let Some(build) =
            agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&target)
        {
            plan.unsupported.push((target, build));
            continue;
        }
        match jetbrains_package_range(&target) {
            Ok(range) => targets_by_range.entry(range).or_default().push(target),
            Err(error) => plan.errors.push(format!("{}: {error:#}", target.display())),
        }
    }
    for (range, targets) in targets_by_range {
        let release = match fetch_release(range.asset_prefix()) {
            Ok(release) => release,
            Err(error) => {
                plan.errors
                    .push(format!("JetBrains {}: {error:#}", range.asset_prefix()));
                continue;
            }
        };
        let asset = match find_asset(&release, range.asset_prefix(), "zip").and_then(|asset| {
            let version = range
                .package_version(asset.name)
                .context("JetBrains release asset has no valid package version")?;
            Ok((asset.name.to_string(), version))
        }) {
            Ok(asset) => asset,
            Err(error) => {
                plan.errors
                    .push(format!("JetBrains {}: {error:#}", range.asset_prefix()));
                continue;
            }
        };
        let (asset_name, version) = asset;
        for target in targets {
            match jetbrains_release_decision_for(&target, range, &version) {
                Ok(decision) => plan.entries.push(JetbrainsReleasePlanEntry {
                    target,
                    range,
                    asset_name: asset_name.clone(),
                    version: version.clone(),
                    decision,
                }),
                Err(error) => plan.errors.push(format!("{}: {error:#}", target.display())),
            }
        }
        plan.releases.insert(range, release);
    }
    plan
}

/// Update every already-installed editor plugin without installing into a new IDE.
///
/// Used by the release watcher. Each editor family is attempted independently so
/// one broken target cannot prevent the others from converging.
pub fn update_all_installed() -> Result<PluginReconcileReport> {
    let jetbrains_targets = existing_jetbrains_agent_doc_dirs(&jetbrains_plugin_dirs());
    let (vscode_targets, mut errors) = installed_vscode_extensions();
    let mut report = PluginReconcileReport::default();
    if jetbrains_targets.is_empty() && vscode_targets.is_empty() && errors.is_empty() {
        return Ok(report);
    }

    if !jetbrains_targets.is_empty() {
        let plan = plan_jetbrains_release_reconcile(jetbrains_targets, |prefix| {
            fetch_release_for_asset(prefix, "zip")
        });
        errors.extend(plan.errors);
        for (target, build) in plan.unsupported {
            report.targets.push(PluginTargetReport {
                family: PluginEditorFamily::JetBrains,
                label: jetbrains_target_label(&target),
                version: installed_jetbrains_plugin_version(&target)
                    .unwrap_or_else(|| "unknown".to_string()),
                outcome: PluginTargetOutcome::UnsupportedPlatform { build },
            });
        }
        for entry in plan.entries {
            let mut record = |outcome| {
                report.targets.push(PluginTargetReport {
                    family: PluginEditorFamily::JetBrains,
                    label: jetbrains_target_label(&entry.target),
                    version: entry.version.clone(),
                    outcome,
                })
            };
            if entry.decision == JetbrainsReleaseDecision::Current {
                record(PluginTargetOutcome::Unchanged);
                continue;
            }
            let release = &plan.releases[&entry.range];
            match install_jetbrains_into(release, &entry.target) {
                Ok(outcome) => record(PluginTargetOutcome::from_jetbrains(&outcome)),
                Err(error) => errors.push(format!("{}: {error:#}", entry.target.display())),
            }
        }
    }

    for (code, installed_version) in vscode_targets {
        match fetch_release_for_asset("agent-doc", "vsix") {
            Ok(release) => match find_asset(&release, "agent-doc", "vsix").and_then(|asset| {
                packaged_plugin_version(asset.name, "agent-doc-", ".vsix")
                    .context("VS Code release asset has no valid package version")
            }) {
                Ok(version) => {
                    let outcome = match (
                        numeric_dot_version(&installed_version),
                        numeric_dot_version(&version),
                    ) {
                        (Some(installed), Some(available)) if installed >= available => {
                            Some(PluginTargetOutcome::Unchanged)
                        }
                        (Some(_), Some(_)) => match install_vscode_with_cmd(&release, code) {
                            Ok(()) => Some(PluginTargetOutcome::Installed),
                            Err(error) => {
                                errors.push(format!("{code}: {error:#}"));
                                None
                            }
                        },
                        _ => {
                            errors.push(format!(
                                "{code}: invalid installed or packaged version ({installed_version:?}, {version:?})"
                            ));
                            None
                        }
                    };
                    if let Some(outcome) = outcome {
                        report.targets.push(PluginTargetReport {
                            family: PluginEditorFamily::VsCode,
                            label: code.to_string(),
                            version,
                            outcome,
                        });
                    }
                }
                Err(error) => errors.push(format!("{code}: {error:#}")),
            },
            Err(error) => errors.push(format!("{code}: {error:#}")),
        }
    }

    if report.hot_upgraded() > 0 {
        // Recycle even when a later target failed: an earlier target already
        // proved that it replaced a live plugin generation.
        crate::runtime_update::recycle_existing_runtimes_after_live_plugin_update(
            "plugin-update-all",
        );
    }
    if errors.is_empty() {
        Ok(report)
    } else {
        // GH #233: a real failure elsewhere bails before the summary prints;
        // keep the unsupported-build warnings visible on that path too.
        for line in report.unsupported_platform_warnings() {
            eprintln!("{line}");
        }
        bail!(
            "one or more installed plugins failed to update:\n{}",
            errors.join("\n")
        )
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        EDITOR_PACKAGE_MANIFEST, compare_digest, editor_package_manifest_digest,
        editor_package_manifest_url, format_rate_limit_reset_at, missing_code_cli_message,
        parse_asset_digest, verify_editor_package,
    };
    use super::{
        JetbrainsLocalInstallOutcome, JetbrainsPackageRange, RELEASE_SEARCH_MAX_PAGES,
        RELEASES_PER_PAGE, choose_plugins_dir_with_interactivity, ensure_github_api_success,
        existing_jetbrains_agent_doc_dirs, find_asset, find_best_local_zip, find_local_vscode_vsix,
        find_local_zip, find_release_with_asset, find_release_with_asset_at_or_below,
        github_get_request, github_token_from, has_asset, install_jetbrains_local_zip_into,
        installed_jetbrains_plugin_version, is_jetbrains_ide_data_dir,
        jetbrains_ide_pids_from_jcmd, jetbrains_install_success_message,
        jetbrains_local_zip_matches_installation, jetbrains_package_range,
        jetbrains_platform_build, jetbrains_plugin_dirs_in_roots,
        jetbrains_upgrade_launcher_has_main_manifest, jetbrains_upgrade_reattach_warning,
        jetbrains_version_cmp, local_jetbrains_zip_in, local_jetbrains_zip_version,
        packaged_plugin_version, release_version, releases_page_url, replace_jetbrains_plugin_tree,
        verify_local_install_version, vscode_extension_version_from_output,
    };
    use super::{install_jetbrains_package_bytes, java_candidates_for_ide, resolve_java_for_ide};
    use serde_json::json;
    use std::cmp::Ordering as CmpOrdering;
    use std::fs;
    use std::io::Write as _;
    use std::path::Path;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_test_jetbrains_zip(path: &std::path::Path, version: &str, plugin: &[u8]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive
            .start_file(
                format!("agent-doc-jetbrains/lib/agent-doc-jetbrains-{version}.jar"),
                options,
            )
            .unwrap();
        archive.write_all(plugin).unwrap();
        archive
            .start_file("agent-doc-jetbrains/lib/dependency.jar", options)
            .unwrap();
        archive.write_all(b"dependency").unwrap();
        archive.finish().unwrap();
    }

    fn write_test_modular_jetbrains_zip(path: &std::path::Path, version: &str, plugin: &[u8]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        archive
            .start_file(
                format!("agent-doc-jetbrains-262/lib/agent.doc-{version}.jar"),
                options,
            )
            .unwrap();
        archive.write_all(plugin).unwrap();
        archive
            .start_file(
                "agent-doc-jetbrains-262/lib/modules/agent.doc.backend.jar",
                options,
            )
            .unwrap();
        archive.write_all(b"backend").unwrap();
        archive.finish().unwrap();
    }

    #[test]
    fn installed_vscode_parser_matches_only_the_agent_doc_extension() {
        let output = "unrelated.agent-doc-helper@9.9.9\nbtakita.agent-doc@0.2.475\n";
        assert_eq!(
            vscode_extension_version_from_output(output).as_deref(),
            Some("0.2.475")
        );
        assert_eq!(
            vscode_extension_version_from_output("other.extension@1.0.0"),
            None
        );
    }

    #[test]
    fn packaged_plugin_versions_handle_signed_and_unsigned_assets() {
        assert_eq!(
            packaged_plugin_version(
                "agent-doc-jetbrains-0.2.475-signed.zip",
                "agent-doc-jetbrains-",
                ".zip"
            )
            .as_deref(),
            Some("0.2.475")
        );
        assert_eq!(
            packaged_plugin_version("agent-doc-0.2.77.vsix", "agent-doc-", ".vsix").as_deref(),
            Some("0.2.77")
        );
        assert_eq!(
            packaged_plugin_version("agent-doc-latest.vsix", "agent-doc-", ".vsix"),
            None
        );
    }

    /// `#pluginassetpaging`: a release with no assets at all.
    fn bare_release(tag: &str) -> serde_json::Value {
        json!({ "tag_name": tag, "assets": [] })
    }

    fn plugin_release(tag: &str) -> serde_json::Value {
        let package_version = tag.trim_start_matches('v');
        json!({
            "tag_name": tag,
            "assets": [{
                "name": format!("agent-doc-jetbrains-{package_version}-signed.zip"),
                "browser_download_url": format!("https://example.invalid/{tag}.zip"),
            }],
        })
    }

    fn nonstable_plugin_release(tag: &str, field: &str) -> serde_json::Value {
        let mut release = plugin_release(tag);
        release[field] = json!(true);
        release
    }

    /// Pages of `RELEASES_PER_PAGE` filler releases with the plugin-bearing one
    /// at `asset_index` in the flattened, newest-first sequence.
    fn paged_source(
        total: usize,
        asset_index: Option<usize>,
    ) -> impl FnMut(usize) -> anyhow::Result<Vec<serde_json::Value>> {
        move |page: usize| {
            let start = (page - 1) * RELEASES_PER_PAGE;
            Ok((start..total.min(start + RELEASES_PER_PAGE))
                .map(|i| {
                    if Some(i) == asset_index {
                        plugin_release(&format!("v0.{i}.0"))
                    } else {
                        bare_release(&format!("v0.{i}.0"))
                    }
                })
                .collect())
        }
    }

    #[test]
    fn releases_page_url_requests_a_full_page_and_the_asked_for_page() {
        // The bug was a bare `/releases`: GitHub's 30-per-page default, one
        // page only. Both parameters have to be on the wire.
        let url = releases_page_url(3);
        assert!(
            url.contains(&format!("per_page={RELEASES_PER_PAGE}")),
            "{url}"
        );
        assert!(url.contains("page=3"), "{url}");
        assert!(url.contains("/repos/btakita/agent-doc/releases?"), "{url}");
    }

    #[test]
    fn release_search_reaches_an_asset_beyond_the_first_page() {
        // GH #53: exactly one release in the window carried the JetBrains zip.
        // Under the old single-page scan anything past the first page was
        // unreachable even though the release was still published.
        let mut pages_fetched = Vec::new();
        let mut source = paged_source(260, Some(150));
        let found = find_release_with_asset("agent-doc-jetbrains", "zip", |page| {
            pages_fetched.push(page);
            source(page)
        })
        .unwrap();
        assert_eq!(release_version(&found), "v0.150.0");
        assert_eq!(pages_fetched, vec![1, 2]);
    }

    #[test]
    fn release_search_skips_prereleases_and_drafts() {
        let prerelease = nonstable_plugin_release("v0.3.0-rc.1", "prerelease");
        let draft = nonstable_plugin_release("v0.2.1", "draft");
        let stable = plugin_release("v0.2.0");

        let found = find_release_with_asset("agent-doc-jetbrains", "zip", |_| {
            Ok(vec![prerelease.clone(), draft.clone(), stable.clone()])
        })
        .unwrap();

        assert_eq!(release_version(&found), "v0.2.0");
    }

    /// GH #113: the upgrade pins the plugin phase to the release it installed,
    /// so a release published between the binary swap and the plugin phase is
    /// never picked up.
    #[test]
    fn pinned_release_search_skips_releases_newer_than_the_pin() {
        let newer = plugin_release("v0.35.443");
        let pinned = plugin_release("v0.35.442");
        let older = plugin_release("v0.35.441");
        let found =
            find_release_with_asset_at_or_below("agent-doc-jetbrains", "zip", "0.35.442", |_| {
                Ok(vec![newer.clone(), pinned.clone(), older.clone()])
            })
            .unwrap();
        assert_eq!(release_version(&found), "v0.35.442");

        let err =
            find_release_with_asset_at_or_below("agent-doc-jetbrains", "zip", "0.35.440", |_| {
                Ok(vec![newer.clone(), pinned.clone()])
            })
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("at or below v0.35.440"),
            "{err:#}"
        );
        assert!(
            find_release_with_asset_at_or_below("agent-doc-jetbrains", "zip", "bogus", |_| Ok(
                vec![]
            ))
            .is_err()
        );
    }

    #[test]
    fn github_token_prefers_github_token_and_falls_back_to_gh_token() {
        let preferred = github_token_from(|name| match name {
            "GITHUB_TOKEN" => Some("github".into()),
            "GH_TOKEN" => Some("gh".into()),
            _ => None,
        });
        assert_eq!(preferred.as_deref(), Some("github"));

        let fallback = github_token_from(|name| match name {
            "GH_TOKEN" => Some("gh".into()),
            _ => None,
        });
        assert_eq!(fallback.as_deref(), Some("gh"));
    }

    #[test]
    fn github_request_adds_bearer_authorization_when_token_present() {
        let agent = super::build_agent();
        let request = github_get_request(&agent, "https://example.invalid", Some("secret"));

        assert_eq!(
            request
                .headers_ref()
                .and_then(|headers| headers.get("Authorization"))
                .and_then(|value| value.to_str().ok()),
            Some("Bearer secret")
        );
    }

    #[test]
    fn github_rate_limit_error_reports_reset_and_auth_guidance() {
        let response = ureq::http::Response::builder()
            .status(403)
            .header("x-ratelimit-remaining", "0")
            .header("x-ratelimit-reset", "1789999999")
            .body(())
            .unwrap();

        let err = ensure_github_api_success(&response, "Failed to fetch releases from GitHub")
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("rate limit exhausted"), "{message}");
        assert!(message.contains("1789999999"), "{message}");
        assert!(message.contains("GITHUB_TOKEN or GH_TOKEN"), "{message}");
    }

    #[test]
    fn release_search_stops_at_the_first_match() {
        let mut pages_fetched = Vec::new();
        let mut source = paged_source(500, Some(7));
        let found = find_release_with_asset("agent-doc-jetbrains", "zip", |page| {
            pages_fetched.push(page);
            source(page)
        })
        .unwrap();
        assert_eq!(release_version(&found), "v0.7.0");
        assert_eq!(
            pages_fetched,
            vec![1],
            "a match on page 1 must not fetch page 2"
        );
    }

    #[test]
    fn release_search_stops_at_a_short_final_page() {
        // A short page is the end of the history; walking past it would burn
        // `RELEASE_SEARCH_MAX_PAGES` requests on empty responses.
        let mut pages_fetched = Vec::new();
        let mut source = paged_source(120, None);
        let err = find_release_with_asset("agent-doc-jetbrains", "zip", |page| {
            pages_fetched.push(page);
            source(page)
        })
        .unwrap_err();
        assert_eq!(pages_fetched, vec![1, 2]);
        assert!(
            err.to_string().contains("120 most recent GitHub releases"),
            "the failure must say how deep the search actually went: {err}"
        );
    }

    #[test]
    fn release_search_finds_an_asset_on_a_short_final_page() {
        let source = paged_source(120, Some(115));
        let found = find_release_with_asset("agent-doc-jetbrains", "zip", source).unwrap();
        assert_eq!(release_version(&found), "v0.115.0");
    }

    #[test]
    fn release_search_is_bounded_by_max_pages() {
        let mut pages_fetched = Vec::new();
        let mut source = paged_source(10_000, None);
        let err = find_release_with_asset("agent-doc-jetbrains", "zip", |page| {
            pages_fetched.push(page);
            source(page)
        })
        .unwrap_err();
        assert_eq!(pages_fetched.len(), RELEASE_SEARCH_MAX_PAGES);
        assert!(
            err.to_string().contains("No agent-doc-jetbrains*.zip"),
            "{err}"
        );
    }

    #[test]
    fn release_search_propagates_a_fetch_error_instead_of_reporting_no_asset() {
        let err = find_release_with_asset("agent-doc-jetbrains", "zip", |_| {
            anyhow::bail!("Failed to fetch releases from GitHub")
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("Failed to fetch releases"),
            "{err}"
        );
    }

    #[test]
    fn ambiguous_noninteractive_jetbrains_target_fails_with_rerun_guidance() {
        let dirs = vec![
            PathBuf::from("/tmp/IdeaIC/plugins"),
            PathBuf::from("/tmp/RustRover/plugins"),
        ];
        let err = choose_plugins_dir_with_interactivity(&dirs, None, false).unwrap_err();
        assert!(err.to_string().contains("stdin is non-interactive"));
        assert!(err.to_string().contains("--plugins-dir <PATH>"));
        assert!(err.to_string().contains("RustRover/plugins"));
    }

    #[test]
    fn explicit_jetbrains_target_is_deterministic_among_multiple_ides() {
        let dirs = vec![
            PathBuf::from("/tmp/IdeaIC/plugins"),
            PathBuf::from("/tmp/RustRover/plugins"),
        ];
        let explicit = PathBuf::from("/opt/jetbrains/plugins");
        assert_eq!(
            choose_plugins_dir_with_interactivity(&dirs, Some(&explicit), false).unwrap(),
            explicit
        );
    }

    /// Published assets are versioned, which is exactly what the old exact-name
    /// comparison could not match. Both API orders are asserted: before the fix
    /// the unsigned-first order passed only because the signed zip happened to
    /// come back first, so order is the discriminator between a real preference
    /// and an accident.
    #[test]
    fn find_asset_prefers_signed_variant() {
        let signed = json!({"name": "agent-doc-jetbrains-0.2.75-signed.zip", "browser_download_url": "https://example.com/signed.zip"});
        let unsigned = json!({"name": "agent-doc-jetbrains-0.2.75.zip", "browser_download_url": "https://example.com/unsigned.zip"});

        for assets in [
            json!([unsigned.clone(), signed.clone()]),
            json!([signed.clone(), unsigned.clone()]),
        ] {
            let release = json!({"tag_name": "v0.33.11", "assets": assets});
            let asset = find_asset(&release, "agent-doc-jetbrains", "zip").unwrap();
            assert_eq!(asset.name, "agent-doc-jetbrains-0.2.75-signed.zip");
            assert_eq!(asset.url, "https://example.com/signed.zip");
        }
    }

    #[test]
    fn find_asset_falls_back_to_the_unsigned_package_and_reads_its_digest() {
        let release = json!({
            "tag_name": "v0.35.417",
            "assets": [
                {"name": "SHA256SUMS", "browser_download_url": "https://example.com/sums"},
                {"name": "agent-doc-jetbrains-0.2.392.zip", "browser_download_url": "https://example.com/jb.zip", "digest": "sha256:abc123"}
            ]
        });

        let asset = find_asset(&release, "agent-doc-jetbrains", "zip").unwrap();
        assert_eq!(asset.name, "agent-doc-jetbrains-0.2.392.zip");
        assert_eq!(asset.digest, Some("sha256:abc123"));
        assert_eq!(parse_asset_digest(asset.digest.unwrap()), Some("abc123"));
        assert_eq!(parse_asset_digest("md5:abc123"), None);
        assert_eq!(parse_asset_digest("sha256:"), None);
    }

    #[test]
    fn find_asset_does_not_confuse_the_modular_262_zip_with_the_classic_zip() {
        let modular = json!({
            "name": "agent-doc-jetbrains-262-0.2.509.zip",
            "browser_download_url": "https://example.com/modular.zip"
        });
        let classic = json!({
            "name": "agent-doc-jetbrains-0.2.508.zip",
            "browser_download_url": "https://example.com/classic.zip"
        });

        for assets in [
            json!([modular.clone(), classic.clone()]),
            json!([classic.clone(), modular.clone()]),
        ] {
            let release = json!({"tag_name": "v0.35.481", "assets": assets});
            let asset = find_asset(&release, "agent-doc-jetbrains", "zip").unwrap();
            assert_eq!(asset.name, "agent-doc-jetbrains-0.2.508.zip");
        }

        let modular_only = json!({"tag_name": "v0.35.481", "assets": [modular]});
        assert!(find_asset(&modular_only, "agent-doc-jetbrains", "zip").is_err());
    }

    #[test]
    fn jetbrains_asset_selection_keeps_classic_and_modular_ranges_disjoint() {
        let modular = json!({
            "name": "agent-doc-jetbrains-262-0.2.511.zip",
            "browser_download_url": "https://example.com/modular.zip"
        });
        let classic = json!({
            "name": "agent-doc-jetbrains-0.2.510.zip",
            "browser_download_url": "https://example.com/classic.zip"
        });
        for assets in [
            json!([modular.clone(), classic.clone()]),
            json!([classic.clone(), modular.clone()]),
        ] {
            let release = json!({"assets": assets});
            assert_eq!(
                find_asset(&release, "agent-doc-jetbrains", "zip")
                    .unwrap()
                    .name,
                "agent-doc-jetbrains-0.2.510.zip"
            );
            assert_eq!(
                find_asset(&release, "agent-doc-jetbrains-262", "zip")
                    .unwrap()
                    .name,
                "agent-doc-jetbrains-262-0.2.511.zip"
            );
        }
    }

    #[test]
    fn editor_package_manifest_lookup_matches_sha256sum_output() {
        let manifest = "\
11112222333344445555666677778888999900001111222233334444555566ab  agent-doc-0.2.71.vsix
aaaabbbbccccddddeeeeffff00001111222233334444555566667777888899cd *agent-doc-jetbrains-0.2.392.zip
";
        assert_eq!(
            editor_package_manifest_digest(manifest, "agent-doc-0.2.71.vsix"),
            Some("11112222333344445555666677778888999900001111222233334444555566ab")
        );
        // Binary-mode `*` prefixes are part of the format, not part of the name.
        assert_eq!(
            editor_package_manifest_digest(manifest, "agent-doc-jetbrains-0.2.392.zip"),
            Some("aaaabbbbccccddddeeeeffff00001111222233334444555566667777888899cd")
        );
        assert_eq!(
            editor_package_manifest_digest(manifest, "agent-doc-0.2.70.vsix"),
            None
        );
    }

    #[test]
    fn editor_package_manifest_url_is_found_by_exact_name() {
        let release = json!({
            "assets": [
                {"name": "agent-doc-0.2.71.vsix", "browser_download_url": "https://example.com/vsix"},
                {"name": EDITOR_PACKAGE_MANIFEST, "browser_download_url": "https://example.com/manifest"}
            ]
        });
        assert_eq!(
            editor_package_manifest_url(&release),
            Some("https://example.com/manifest")
        );
        assert_eq!(editor_package_manifest_url(&json!({"assets": []})), None);
    }

    #[test]
    fn digest_mismatch_refuses_the_install() {
        compare_digest("agent-doc-0.2.71.vsix", "abcd", "abcd", "a manifest").unwrap();
        compare_digest("agent-doc-0.2.71.vsix", "ABCD", "abcd", "a manifest").unwrap();
        let err = compare_digest("agent-doc-0.2.71.vsix", "abcd", "ef01", "a manifest")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Integrity check failed"), "{err}");
        assert!(err.contains("Refusing to install"), "{err}");
    }

    /// A release with no manifest asset and no API digest still installs, but it
    /// must say so — silently skipping verification is how the gap survived.
    #[test]
    fn a_release_without_any_published_digest_warns_instead_of_verifying() {
        let dir = TempDir::new().unwrap();
        let package = dir.path().join("agent-doc-0.2.71.vsix");
        fs::write(&package, b"payload").unwrap();
        let release = json!({
            "assets": [{"name": "agent-doc-0.2.71.vsix", "browser_download_url": "https://example.com/vsix"}]
        });
        let asset = find_asset(&release, "agent-doc", "vsix").unwrap();
        assert!(asset.digest.is_none());
        verify_editor_package(&release, &asset, &package).unwrap();
    }

    #[test]
    fn an_api_digest_that_disagrees_with_the_bytes_refuses_the_install() {
        let dir = TempDir::new().unwrap();
        let package = dir.path().join("agent-doc-0.2.71.vsix");
        fs::write(&package, b"payload").unwrap();
        let truth = agent_doc_hash::bytes_hash(b"payload");

        let good = json!({
            "assets": [{"name": "agent-doc-0.2.71.vsix", "browser_download_url": "https://example.com/vsix", "digest": format!("sha256:{truth}")}]
        });
        let asset = find_asset(&good, "agent-doc", "vsix").unwrap();
        verify_editor_package(&good, &asset, &package).unwrap();

        let bad = json!({
            "assets": [{"name": "agent-doc-0.2.71.vsix", "browser_download_url": "https://example.com/vsix", "digest": "sha256:deadbeef"}]
        });
        let asset = find_asset(&bad, "agent-doc", "vsix").unwrap();
        let err = verify_editor_package(&bad, &asset, &package)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Integrity check failed"), "{err}");
    }

    #[test]
    fn the_rate_limit_reset_is_reported_as_a_wait_not_a_bare_epoch() {
        assert_eq!(
            format_rate_limit_reset_at("1789999999", 1_789_999_999 - 723),
            "; resets in 12m 3s (Unix timestamp 1789999999)"
        );
        assert_eq!(
            format_rate_limit_reset_at("1789999999", 1_789_999_999 - 9),
            "; resets in 9s (Unix timestamp 1789999999)"
        );
        assert_eq!(
            format_rate_limit_reset_at("1789999999", 1_789_999_999),
            "; the reset is due now (Unix timestamp 1789999999)"
        );
        // A clock already past the reset must not underflow into a huge wait.
        assert_eq!(
            format_rate_limit_reset_at("1789999999", 1_790_000_600),
            "; the reset is due now (Unix timestamp 1789999999)"
        );
        // Unparseable headers are reported verbatim rather than guessed at.
        assert_eq!(
            format_rate_limit_reset_at("soon", 0),
            "; resets at Unix timestamp soon"
        );
    }

    #[test]
    fn the_missing_code_cli_message_names_the_prerequisite_not_the_vsix() {
        let message = missing_code_cli_message();
        assert!(message.contains("cursor"), "{message}");
        assert!(message.contains("codium"), "{message}");
        assert!(message.contains("PATH"), "{message}");
        assert!(!message.contains("No such file or directory"), "{message}");
    }

    #[test]
    fn has_asset_returns_false_for_assetless_release() {
        let release = json!({
            "tag_name": "v0.33.16",
            "assets": []
        });

        assert!(!has_asset(&release, "agent-doc-jetbrains", "zip"));
        assert!(!has_asset(&release, "agent-doc", "vsix"));
        assert_eq!(release_version(&release), "v0.33.16");
    }

    #[test]
    fn has_asset_matches_versioned_vsix_name() {
        let release = json!({
            "tag_name": "v0.33.11",
            "assets": [
                {"name": "agent-doc-0.2.8.vsix", "browser_download_url": "https://example.com/agent-doc.vsix"}
            ]
        });

        assert!(has_asset(&release, "agent-doc", "vsix"));
    }

    #[test]
    fn find_local_zip_prefers_newest_version_even_if_only_older_build_is_signed() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path();
        fs::write(
            dist.join("agent-doc-jetbrains-0.2.80-signed.zip"),
            b"signed-old",
        )
        .unwrap();
        fs::write(dist.join("agent-doc-jetbrains-0.2.91.zip"), b"unsigned-new").unwrap();

        let signed = find_local_zip(dist, true).unwrap();
        let unsigned = find_local_zip(dist, false).unwrap();

        assert!(
            signed.ends_with("agent-doc-jetbrains-0.2.80-signed.zip"),
            "signed selection should still see the available signed artifact"
        );
        assert!(
            unsigned.ends_with("agent-doc-jetbrains-0.2.91.zip"),
            "unsigned selection should pick the newest unsigned artifact"
        );
    }

    #[test]
    fn find_best_local_zip_prefers_newest_version_over_older_signed_artifact() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path();
        fs::write(
            dist.join("agent-doc-jetbrains-0.2.80-signed.zip"),
            b"signed-old",
        )
        .unwrap();
        fs::write(dist.join("agent-doc-jetbrains-0.2.91.zip"), b"unsigned-new").unwrap();

        let chosen = find_best_local_zip(dist).unwrap();

        assert!(
            chosen.ends_with("agent-doc-jetbrains-0.2.91.zip"),
            "install path should pick the newest version even when only the older build is signed"
        );
    }

    #[test]
    fn find_best_local_zip_prefers_signed_artifact_when_versions_match() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path();
        fs::write(dist.join("agent-doc-jetbrains-0.2.91.zip"), b"unsigned").unwrap();
        fs::write(
            dist.join("agent-doc-jetbrains-0.2.91-signed.zip"),
            b"signed",
        )
        .unwrap();

        let chosen = find_best_local_zip(dist).unwrap();

        assert!(
            chosen.ends_with("agent-doc-jetbrains-0.2.91-signed.zip"),
            "signed artifact should win when both builds have the same version"
        );
    }

    #[test]
    fn jetbrains_target_build_selects_only_its_compatibility_range() {
        let classic = PathBuf::from("/tmp/JetBrains/IntelliJIdea2026.1/plugins");
        let modular = PathBuf::from("/tmp/JetBrains/IntelliJIdea2026.2/plugins");
        assert_eq!(jetbrains_platform_build(&classic), Some(261));
        assert_eq!(jetbrains_platform_build(&modular), Some(262));
        assert_eq!(
            jetbrains_package_range(&classic).unwrap(),
            JetbrainsPackageRange::Classic242To261
        );
        assert_eq!(
            jetbrains_package_range(&modular).unwrap(),
            JetbrainsPackageRange::Modular262
        );

        let unknown = PathBuf::from("/opt/jetbrains/plugins");
        let error = jetbrains_package_range(&unknown).unwrap_err().to_string();
        assert!(error.contains("Cannot prove the JetBrains platform build"));

        let future = PathBuf::from("/tmp/JetBrains/IntelliJIdea2026.3/plugins");
        let error = jetbrains_package_range(&future).unwrap_err().to_string();
        assert!(error.contains("unsupported platform build 263"));
        // GH #233: the explicit-install refusal names no remedy that cannot
        // succeed on the newest release.
        assert!(error.contains("left in place"), "{error}");
        assert!(!error.contains("Upgrade agent-doc"), "{error}");
        assert_eq!(
            agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&future),
            Some(263)
        );
        assert_eq!(
            agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&modular),
            None
        );
        assert_eq!(
            agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&unknown),
            None
        );
        let old = PathBuf::from("/tmp/JetBrains/IntelliJIdea2024.1/plugins");
        assert_eq!(
            agent_doc_fs::jetbrains_install::jetbrains_unsupported_platform_build(&old),
            Some(241)
        );
    }

    #[test]
    fn local_jetbrains_zip_requires_target_ranged_gradle_properties_version() {
        let tmp = TempDir::new().unwrap();
        let jetbrains = tmp.path().join("editors/jetbrains");
        let classic_dist = jetbrains.join("build/distributions");
        let modular = tmp.path().join("editors/jetbrains-262");
        let modular_dist = modular.join("build/distributions");
        fs::create_dir_all(&classic_dist).unwrap();
        fs::create_dir_all(&modular_dist).unwrap();
        fs::write(
            jetbrains.join("gradle.properties"),
            "pluginGroup = example.agentdoc\npluginName = agent-doc-jetbrains\npluginVersion = 0.2.91\n",
        )
        .unwrap();
        fs::write(
            modular.join("gradle.properties"),
            "pluginGroup = example.agentdoc\npluginName = agent-doc-jetbrains\npluginVersion = 0.2.92\n",
        )
        .unwrap();
        fs::write(
            classic_dist.join("agent-doc-jetbrains-0.2.90.zip"),
            b"stale",
        )
        .unwrap();
        fs::write(
            modular_dist.join("agent-doc-jetbrains-262-0.2.91.zip"),
            b"stale",
        )
        .unwrap();

        let classic_target = Path::new("/tmp/JetBrains/IntelliJIdea2026.1/plugins");
        let modular_target = Path::new("/tmp/JetBrains/IntelliJIdea2026.2/plugins");

        let error = local_jetbrains_zip_in(tmp.path(), classic_target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("pluginVersion 0.2.91"));
        let error = local_jetbrains_zip_in(tmp.path(), modular_target)
            .unwrap_err()
            .to_string();
        assert!(error.contains("pluginVersion 0.2.92"));

        let classic = classic_dist.join("agent-doc-jetbrains-0.2.91.zip");
        let modular = modular_dist.join("agent-doc-jetbrains-262-0.2.92.zip");
        fs::write(&classic, b"classic").unwrap();
        fs::write(&modular, b"modular").unwrap();
        assert_eq!(
            local_jetbrains_zip_in(tmp.path(), classic_target).unwrap(),
            classic
        );
        assert_eq!(
            local_jetbrains_zip_in(tmp.path(), modular_target).unwrap(),
            modular
        );
    }

    fn dual_range_release() -> serde_json::Value {
        // Deliberately interleaved and including signed variants: selection
        // must depend on the target's range, never on API order.
        json!({
            "tag_name": "v0.35.481",
            "assets": [
                {"name": "agent-doc-jetbrains-262-0.2.511.zip", "browser_download_url": "https://example.invalid/m.zip"},
                {"name": "agent-doc-jetbrains-0.2.510.zip", "browser_download_url": "https://example.invalid/c.zip"},
                {"name": "agent-doc-jetbrains-262-0.2.511-signed.zip", "browser_download_url": "https://example.invalid/ms.zip"},
                {"name": "agent-doc-jetbrains-0.2.510-signed.zip", "browser_download_url": "https://example.invalid/cs.zip"},
            ],
        })
    }

    fn install_test_jar(plugins: &Path, jar: &str) {
        let lib = plugins.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join(jar), b"installed").unwrap();
    }

    #[test]
    fn mixed_261_and_262_targets_choose_distinct_exact_release_assets() {
        let release = dual_range_release();
        let classic = Path::new("/home/u/.local/share/JetBrains/IntelliJIdea2026.1/plugins");
        let modular = Path::new("/home/u/.local/share/JetBrains/IntelliJIdea2026.2/plugins");
        let oldest = Path::new("/home/u/.local/share/JetBrains/PyCharm2024.2");

        let (range, asset) = super::jetbrains_release_asset_for_target(&release, classic).unwrap();
        assert_eq!(range, JetbrainsPackageRange::Classic242To261);
        assert_eq!(asset.name, "agent-doc-jetbrains-0.2.510-signed.zip");
        let (range, asset) = super::jetbrains_release_asset_for_target(&release, modular).unwrap();
        assert_eq!(range, JetbrainsPackageRange::Modular262);
        assert_eq!(asset.name, "agent-doc-jetbrains-262-0.2.511-signed.zip");
        let (range, asset) = super::jetbrains_release_asset_for_target(&release, oldest).unwrap();
        assert_eq!(range, JetbrainsPackageRange::Classic242To261);
        assert_eq!(asset.name, "agent-doc-jetbrains-0.2.510-signed.zip");

        // A release that carries only the classic line must not satisfy a 262
        // target, and vice versa.
        let classic_only = json!({"assets": [
            {"name": "agent-doc-jetbrains-0.2.510.zip", "browser_download_url": "https://example.invalid/c.zip"},
        ]});
        assert!(super::jetbrains_release_asset_for_target(&classic_only, modular).is_err());
        let modular_only = json!({"assets": [
            {"name": "agent-doc-jetbrains-262-0.2.511.zip", "browser_download_url": "https://example.invalid/m.zip"},
        ]});
        assert!(super::jetbrains_release_asset_for_target(&modular_only, classic).is_err());
    }

    #[test]
    fn update_all_plan_preserves_each_compatible_line_and_fails_closed_on_unknown() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("JetBrains");
        let classic_current = root.join("IntelliJIdea2026.1/plugins");
        install_test_jar(&classic_current, "agent-doc-jetbrains-0.2.510.jar");
        let modular_current = root.join("RustRover2026.2/plugins");
        install_test_jar(&modular_current, "agent.doc-0.2.511.jar");
        let modular_stale = root.join("PyCharm2026.2/plugins");
        install_test_jar(&modular_stale, "agent.doc-0.2.509.jar");
        // A classic package wrongly left in a 262 IDE carries a HIGHER update
        // number than the modular asset; it must still be replaced.
        let classic_in_262 = root.join("GoLand2026.2/plugins");
        install_test_jar(&classic_in_262, "agent-doc-jetbrains-0.2.600.jar");
        // And the reverse: a modular package inside a 261 IDE.
        let modular_in_261 = root.join("CLion2026.1/plugins");
        install_test_jar(&modular_in_261, "agent.doc-0.2.700.jar");
        let unknown = tmp.path().join("custom-ide/plugins");
        install_test_jar(&unknown, "agent-doc-jetbrains-0.2.400.jar");

        let mut fetched = Vec::new();
        let plan = super::plan_jetbrains_release_reconcile(
            vec![
                classic_current.clone(),
                modular_current.clone(),
                modular_stale.clone(),
                classic_in_262.clone(),
                modular_in_261.clone(),
                unknown.clone(),
            ],
            |prefix| {
                fetched.push(prefix.to_string());
                Ok(dual_range_release())
            },
        );
        fetched.sort();
        assert_eq!(fetched, ["agent-doc-jetbrains", "agent-doc-jetbrains-262"]);

        let decision = |target: &Path| {
            let entry = plan
                .entries
                .iter()
                .find(|entry| entry.target == target)
                .unwrap_or_else(|| panic!("no plan entry for {}", target.display()));
            (
                entry.asset_name.clone(),
                entry.version.clone(),
                entry.decision,
            )
        };
        use super::JetbrainsReleaseDecision::{Current, Replace};
        let classic_asset = "agent-doc-jetbrains-0.2.510-signed.zip".to_string();
        let modular_asset = "agent-doc-jetbrains-262-0.2.511-signed.zip".to_string();
        assert_eq!(
            decision(&classic_current),
            (classic_asset.clone(), "0.2.510".into(), Current)
        );
        assert_eq!(
            decision(&modular_current),
            (modular_asset.clone(), "0.2.511".into(), Current)
        );
        assert_eq!(
            decision(&modular_stale),
            (
                modular_asset.clone(),
                "0.2.511".into(),
                Replace { cross_line: false }
            )
        );
        assert_eq!(
            decision(&classic_in_262),
            (
                modular_asset,
                "0.2.511".into(),
                Replace { cross_line: true }
            )
        );
        assert_eq!(
            decision(&modular_in_261),
            (
                classic_asset,
                "0.2.510".into(),
                Replace { cross_line: true }
            )
        );

        assert!(plan.entries.iter().all(|entry| entry.target != unknown));
        assert_eq!(plan.errors.len(), 1, "{:?}", plan.errors);
        assert!(plan.errors[0].contains("Cannot prove the JetBrains platform build"));
        assert!(plan.errors[0].contains("--plugins-dir"));
        assert_eq!(plan.releases.len(), 2);
    }

    #[test]
    fn update_all_plan_reports_a_missing_range_release_without_crossing_lines() {
        let tmp = TempDir::new().unwrap();
        let classic = tmp.path().join("IntelliJIdea2026.1/plugins");
        let modular = tmp.path().join("IntelliJIdea2026.2/plugins");
        install_test_jar(&classic, "agent-doc-jetbrains-0.2.500.jar");
        install_test_jar(&modular, "agent.doc-0.2.501.jar");
        let plan = super::plan_jetbrains_release_reconcile(
            vec![classic.clone(), modular.clone()],
            |prefix| {
                if prefix == "agent-doc-jetbrains-262" {
                    anyhow::bail!("no release carries {prefix}")
                }
                Ok(json!({"assets": [
                    {"name": "agent-doc-jetbrains-0.2.510.zip", "browser_download_url": "https://example.invalid/c.zip"},
                    {"name": "agent-doc-jetbrains-262-0.2.511.zip", "browser_download_url": "https://example.invalid/m.zip"},
                ]}))
            },
        );
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].target, classic);
        assert_eq!(
            plan.entries[0].asset_name,
            "agent-doc-jetbrains-0.2.510.zip"
        );
        assert_eq!(plan.errors.len(), 1);
        assert!(plan.errors[0].contains("agent-doc-jetbrains-262"));
    }

    /// GH #233: a target whose proven build no published range declares is
    /// left in place and reported as unsupported, never as a failed install,
    /// and no release is fetched on its behalf.
    #[test]
    fn update_all_plan_leaves_an_unsupported_build_in_place_without_an_error() {
        let tmp = TempDir::new().unwrap();
        let supported = tmp.path().join("IntelliJIdea2026.2/plugins");
        install_test_jar(&supported, "agent.doc-0.2.509.jar");
        let future = tmp.path().join("IntelliJIdea2026.3/plugins");
        install_test_jar(&future, "agent-doc-jetbrains-0.2.508.jar");

        let mut fetched = Vec::new();
        let plan = super::plan_jetbrains_release_reconcile(
            vec![supported.clone(), future.clone()],
            |prefix| {
                fetched.push(prefix.to_string());
                Ok(dual_range_release())
            },
        );

        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert_eq!(plan.unsupported, vec![(future.clone(), 263)]);
        assert!(plan.entries.iter().all(|entry| entry.target != future));
        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].target, supported);
        assert_eq!(fetched, ["agent-doc-jetbrains-262"]);
        assert_eq!(
            super::installed_jetbrains_plugin_version(&future).as_deref(),
            Some("0.2.508")
        );
    }

    /// GH #233: the unsupported-build outcome is one warning that names the
    /// build, the supported ranges and the version left installed, offers no
    /// remedy that cannot succeed, and does not count as a change.
    #[test]
    fn reconcile_summary_reports_unsupported_build_once_without_retry_advice() {
        use super::{PluginEditorFamily::JetBrains, PluginTargetOutcome as O};
        let report = super::PluginReconcileReport {
            targets: vec![
                reconcile_target(JetBrains, "IntelliJIdea2026.2", "0.2.518", O::Unchanged),
                reconcile_target(
                    JetBrains,
                    "IntelliJIdea2026.3",
                    "0.2.508",
                    O::UnsupportedPlatform { build: 263 },
                ),
            ],
        };
        assert_eq!(report.changed(), 0);
        assert_eq!(report.hot_upgraded(), 0);
        let summary = report.summary("0.35.482");
        assert!(
            summary.starts_with(
                "Installed editor plugins on supported IDE builds already match v0.35.482."
            ),
            "{summary}"
        );
        let warnings = summary
            .lines()
            .filter(|line| line.starts_with("WARNING:"))
            .collect::<Vec<_>>();
        assert_eq!(warnings.len(), 1, "{summary}");
        let warning = warnings[0];
        for needle in [
            "IntelliJIdea2026.3",
            "platform build 263",
            "242-261",
            "262 (agent-doc-jetbrains-262-<version>.zip)",
            "left plugin v0.2.508 installed",
            "not an install failure",
        ] {
            assert!(warning.contains(needle), "missing {needle:?}: {warning}");
        }
        for remedy in [
            "re-run",
            "retry",
            "plugin install",
            "Upgrade agent-doc",
            "skewed",
        ] {
            assert!(!summary.contains(remedy), "offers {remedy:?}: {summary}");
        }
        let logged = report.ops_log_line("0.35.482");
        assert!(logged.contains("unsupported_platform=1"), "{logged}");
        assert!(logged.contains("unchanged=1"), "{logged}");
        assert!(logged.contains("restart_targets=\"\""), "{logged}");

        // A report with no unsupported target keeps its original wording.
        let plain = super::PluginReconcileReport {
            targets: vec![reconcile_target(
                JetBrains,
                "IntelliJIdea2026.2",
                "0.2.518",
                O::Unchanged,
            )],
        };
        assert_eq!(
            plain.summary("0.35.482"),
            "Installed editor plugins already match v0.35.482."
        );
    }

    fn write_local_dual_build(project: &Path, classic: &str, modular: &str) {
        for (dir, version) in [
            ("editors/jetbrains", classic),
            ("editors/jetbrains-262", modular),
        ] {
            let dist = project.join(dir).join("build/distributions");
            fs::create_dir_all(&dist).unwrap();
            fs::write(
                project.join(dir).join("gradle.properties"),
                format!("pluginVersion = {version}\n"),
            )
            .unwrap();
        }
        write_test_jetbrains_zip(
            &project.join(format!(
                "editors/jetbrains/build/distributions/agent-doc-jetbrains-{classic}.zip"
            )),
            classic,
            b"classic",
        );
        write_test_modular_jetbrains_zip(
            &project.join(format!(
                "editors/jetbrains-262/build/distributions/agent-doc-jetbrains-262-{modular}.zip"
            )),
            modular,
            b"modular",
        );
    }

    #[test]
    fn local_all_installs_each_target_from_its_own_compatible_package() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("checkout");
        write_local_dual_build(&project, "0.2.510", "0.2.511");
        let root = tmp.path().join("JetBrains");
        let classic = root.join("IntelliJIdea2026.1/plugins");
        install_test_jar(&classic, "agent-doc-jetbrains-0.2.500.jar");
        let modular = root.join("IntelliJIdea2026.2/plugins");
        install_test_jar(&modular, "agent.doc-0.2.501.jar");
        // A 262 IDE holding a newer-numbered classic package must still be
        // converged to the modular line, never kept or refreshed as classic.
        let classic_in_262 = root.join("WebStorm2026.2/plugins");
        install_test_jar(&classic_in_262, "agent-doc-jetbrains-0.2.600.jar");

        let resolved = super::resolve_local_jetbrains_targets(
            &project,
            vec![classic.clone(), modular.clone(), classic_in_262.clone()],
        )
        .unwrap();
        let zip_name = |target: &Path| {
            resolved
                .iter()
                .find(|(t, _)| t == target)
                .unwrap()
                .1
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(zip_name(&classic), "agent-doc-jetbrains-0.2.510.zip");
        assert_eq!(zip_name(&modular), "agent-doc-jetbrains-262-0.2.511.zip");
        assert_eq!(
            zip_name(&classic_in_262),
            "agent-doc-jetbrains-262-0.2.511.zip"
        );

        for (target, zip) in &resolved {
            install_jetbrains_local_zip_into(zip, target).unwrap();
        }
        assert_eq!(
            super::installed_jetbrains_package(&classic),
            Some((JetbrainsPackageRange::Classic242To261, "0.2.510".into()))
        );
        for target in [&modular, &classic_in_262] {
            assert_eq!(
                super::installed_jetbrains_package(target),
                Some((JetbrainsPackageRange::Modular262, "0.2.511".into())),
                "{}",
                target.display()
            );
            assert!(
                !target
                    .join("agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.600.jar")
                    .exists()
            );
            assert!(
                target
                    .join("agent-doc-jetbrains/lib/modules/agent.doc.backend.jar")
                    .is_file()
            );
        }
    }

    #[test]
    fn local_all_refuses_every_target_when_any_cannot_prove_its_range() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("checkout");
        write_local_dual_build(&project, "0.2.510", "0.2.511");
        let classic = tmp.path().join("JetBrains/IntelliJIdea2026.1/plugins");
        install_test_jar(&classic, "agent-doc-jetbrains-0.2.500.jar");
        let unknown = tmp.path().join("portable-ide/plugins");
        install_test_jar(&unknown, "agent.doc-0.2.501.jar");

        let error = super::resolve_local_jetbrains_targets(
            &project,
            vec![classic.clone(), unknown.clone()],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("no installation was changed"), "{error}");
        assert!(
            error.contains("Cannot prove the JetBrains platform build"),
            "{error}"
        );
        assert!(error.contains("--plugins-dir"), "{error}");
        assert!(error.contains("portable-ide"), "{error}");
        assert_eq!(
            super::installed_jetbrains_package(&classic),
            Some((JetbrainsPackageRange::Classic242To261, "0.2.500".into()))
        );
    }

    #[test]
    fn local_262_target_without_a_modular_build_never_falls_back_to_classic() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("checkout");
        write_local_dual_build(&project, "0.2.510", "0.2.511");
        fs::remove_dir_all(project.join("editors/jetbrains-262")).unwrap();
        let modular = tmp.path().join("JetBrains/IntelliJIdea2026.2/plugins");

        let error = super::resolve_local_jetbrains_targets(&project, vec![modular])
            .unwrap_err()
            .to_string();
        assert!(error.contains("agent-doc-jetbrains-262 package"), "{error}");
        assert!(
            error.contains("refusing to substitute the other compatibility line"),
            "{error}"
        );
    }

    #[test]
    fn modular_zip_converges_to_the_canonical_installed_tree() {
        let tmp = TempDir::new().unwrap();
        let package = tmp.path().join("agent-doc-jetbrains-262-0.2.511.zip");
        let plugins = tmp.path().join("IntelliJIdea2026.2/plugins");
        let legacy = plugins.join("agent-doc-jetbrains-262/lib");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("agent.doc-0.2.510.jar"), b"old").unwrap();
        write_test_modular_jetbrains_zip(&package, "0.2.511", b"new");

        replace_jetbrains_plugin_tree(&package, &plugins).unwrap();

        assert!(!plugins.join("agent-doc-jetbrains-262").exists());
        assert_eq!(
            fs::read(plugins.join("agent-doc-jetbrains/lib/agent.doc-0.2.511.jar")).unwrap(),
            b"new"
        );
        assert_eq!(
            installed_jetbrains_plugin_version(&plugins).as_deref(),
            Some("0.2.511")
        );
        assert!(super::jetbrains_local_zip_matches_installation(&package, &plugins).unwrap());
    }

    #[test]
    fn find_local_vscode_vsix_requires_manifest_version() {
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path();
        fs::write(dist.join("package.json"), r#"{"version":"0.2.50"}"#).unwrap();
        fs::write(dist.join("agent-doc-0.2.47.vsix"), b"stale").unwrap();

        let error = find_local_vscode_vsix(dist).unwrap_err().to_string();
        assert!(error.contains("package.json version 0.2.50"));

        let current = dist.join("agent-doc-0.2.50.vsix");
        fs::write(&current, b"current").unwrap();
        assert_eq!(find_local_vscode_vsix(dist).unwrap(), current);
    }

    #[test]
    fn jetbrains_discovery_excludes_config_and_service_roots() {
        let tmp = TempDir::new().unwrap();
        let data_root = tmp.path().join("share/JetBrains");
        fs::create_dir_all(data_root.join("IntelliJIdea2026.1")).unwrap();
        fs::create_dir_all(data_root.join("PrivacyPolicy")).unwrap();
        fs::create_dir_all(data_root.join("Daemon")).unwrap();
        fs::create_dir_all(data_root.join("Idea")).unwrap();

        let dirs = jetbrains_plugin_dirs_in_roots(&[data_root]);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("IntelliJIdea2026.1"));
        assert!(is_jetbrains_ide_data_dir("PyCharm2025.3"));
        assert!(!is_jetbrains_ide_data_dir("PrivacyPolicy"));
    }

    #[test]
    fn jcmd_discovery_selects_only_jetbrains_hosts_and_deduplicates_pids() {
        let output = "28053 com.intellij.idea.Main\n45130 com.intellij.ml.llm.matterhorn.MainKt\n\
                      28053 com.intellij.idea.MainImpl\n991 jdk.jcmd/sun.tools.jcmd.JCmd -l\n";
        assert_eq!(jetbrains_ide_pids_from_jcmd(output), vec![28053]);
    }

    #[test]
    fn installed_jetbrains_version_comes_from_current_plugin_jar() {
        let tmp = TempDir::new().unwrap();
        let lib = tmp.path().join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("lazily-kt-0.29.0.jar"), b"dependency").unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.252.jar"), b"plugin").unwrap();

        assert_eq!(
            installed_jetbrains_plugin_version(tmp.path()).as_deref(),
            Some("0.2.252")
        );
        assert_eq!(
            jetbrains_install_success_message(tmp.path()).unwrap(),
            format!("Plugin installed (v0.2.252) to {}", tmp.path().display())
        );
    }

    #[test]
    fn jetbrains_versions_compare_numerically_before_install_side_effects() {
        assert_eq!(
            jetbrains_version_cmp("0.2.419", "0.2.392").unwrap(),
            CmpOrdering::Greater
        );
        assert_eq!(
            jetbrains_version_cmp("0.2.419", "0.2.419").unwrap(),
            CmpOrdering::Equal
        );
        assert_eq!(
            jetbrains_version_cmp("0.2.420", "0.2.419").unwrap(),
            CmpOrdering::Greater
        );
    }

    /// `#jbupgradereattach`: a converged receipt is silent -- the operator sees nothing
    /// extra when the upgrade landed and every open document re-registered.
    #[test]
    fn a_converged_reattach_receipt_emits_no_warning() {
        assert_eq!(
            jetbrains_upgrade_reattach_warning(4242, "ok:0.2.427:documents=3/3"),
            None,
        );
        assert_eq!(
            jetbrains_upgrade_reattach_warning(4242, "ok:0.2.427:documents=0/0"),
            None,
        );
        assert_eq!(jetbrains_upgrade_reattach_warning(4242, "ok:0.2.427"), None);
    }

    /// The exact shape measured 2026-09-26: one open document from an unrelated project
    /// stayed pending while the replacement bytes were installed and live. It must warn
    /// and name the document, and it must NOT be representable as an install failure.
    #[test]
    fn a_pending_document_warns_and_names_it_without_failing_the_install() {
        let warning = jetbrains_upgrade_reattach_warning(
            3129637,
            "ok:0.2.427:documents=1/2:pending=/repo/src/sample-app/tasks/sampleorders.md",
        )
        .expect("a pending document must be reported");
        assert!(warning.contains("3129637"), "{warning}");
        assert!(
            warning.contains("/repo/src/sample-app/tasks/sampleorders.md"),
            "the operator needs the exact pending document: {warning}"
        );
        assert!(
            warning.contains("installed and live"),
            "the receipt must not read as a failed upgrade: {warning}"
        );
    }

    /// Pending paths are the receipt's last field, so a path containing `:` survives
    /// verbatim instead of being split into another field.
    #[test]
    fn pending_paths_keep_colons_and_are_counted() {
        let warning = jetbrains_upgrade_reattach_warning(
            7,
            "ok:0.2.427:documents=0/2:pending=/a/od:d/x.md,/b/y.md",
        )
        .expect("two pending documents must be reported");
        assert!(warning.contains("/a/od:d/x.md"), "{warning}");
        assert!(warning.contains("/b/y.md"), "{warning}");
        assert!(warning.contains("2 open document(s)"), "{warning}");
    }

    /// An unreachable receipt is still a landed upgrade: the bytes converged before the
    /// reattach step ran at all.
    #[test]
    fn an_unavailable_reattach_receipt_warns_instead_of_failing() {
        let warning = jetbrains_upgrade_reattach_warning(
            9,
            "ok:0.2.427:documents=0/0:reattach_error=CRDT replica manager was not initialized",
        )
        .expect("an unavailable receipt must be reported");
        assert!(
            warning.contains("CRDT replica manager was not initialized"),
            "{warning}"
        );
        assert!(
            warning.contains("installed and live"),
            "the receipt must not read as a failed upgrade: {warning}"
        );
    }

    #[test]
    fn jetbrains_upgrade_launcher_requires_a_main_class_manifest() {
        fn write_launcher(path: &Path, manifest: &str) {
            let file = fs::File::create(path).unwrap();
            let mut archive = zip::ZipWriter::new(file);
            archive
                .start_file(
                    "META-INF/MANIFEST.MF",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(manifest.as_bytes()).unwrap();
            archive.finish().unwrap();
        }

        let tmp = TempDir::new().unwrap();
        let legacy = tmp.path().join("legacy.jar");
        let current = tmp.path().join("current.jar");
        write_launcher(&legacy, "Manifest-Version: 1.0\n");
        write_launcher(
            &current,
            "Manifest-Version: 1.0\nMain-Class: com.example.Upgrade\n",
        );

        assert!(!jetbrains_upgrade_launcher_has_main_manifest(&legacy).unwrap());
        assert!(jetbrains_upgrade_launcher_has_main_manifest(&current).unwrap());
    }

    #[test]
    fn all_installed_selection_updates_only_existing_agent_doc_packages() {
        let tmp = TempDir::new().unwrap();
        let current = tmp.path().join("IntelliJIdea2026.1/plugins");
        let unrelated = tmp.path().join("PyCharm2026.1/plugins");
        fs::create_dir_all(current.join("agent-doc-jetbrains/lib")).unwrap();
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(
            current.join("agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.261.jar"),
            b"old plugin",
        )
        .unwrap();

        assert_eq!(
            existing_jetbrains_agent_doc_dirs(&[unrelated, current.clone()]),
            vec![current]
        );
    }

    #[test]
    fn local_jetbrains_package_version_is_exact_for_signed_and_unsigned_builds() {
        assert_eq!(
            local_jetbrains_zip_version(
                PathBuf::from("/tmp/agent-doc-jetbrains-0.2.263-signed.zip").as_path()
            )
            .unwrap(),
            "0.2.263"
        );
        assert_eq!(
            local_jetbrains_zip_version(
                PathBuf::from("/tmp/agent-doc-jetbrains-0.2.264.zip").as_path()
            )
            .unwrap(),
            "0.2.264"
        );
    }

    #[test]
    fn byte_identical_jetbrains_install_keeps_live_files_in_place() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        let plugin = lib.join("agent-doc-jetbrains-0.2.396.jar");
        fs::write(&plugin, b"plugin").unwrap();
        fs::write(lib.join("dependency.jar"), b"dependency").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.396.zip");
        write_test_jetbrains_zip(&zip, "0.2.396", b"plugin");

        assert!(jetbrains_local_zip_matches_installation(&zip, &target).unwrap());
        #[cfg(unix)]
        let inode_before = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&plugin).unwrap());

        assert_eq!(
            install_jetbrains_local_zip_into(&zip, &target).unwrap(),
            JetbrainsLocalInstallOutcome::Unchanged
        );
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::MetadataExt::ino(&fs::metadata(&plugin).unwrap()),
            inode_before,
            "a no-op install must not unlink the JAR mapped by a live IDE"
        );
    }

    #[test]
    fn changed_jetbrains_install_replaces_and_verifies_package() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.396.jar"), b"old").unwrap();
        fs::write(lib.join("stale.jar"), b"stale").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.397.zip");
        write_test_jetbrains_zip(&zip, "0.2.397", b"new");

        assert!(!jetbrains_local_zip_matches_installation(&zip, &target).unwrap());
        assert_eq!(
            install_jetbrains_local_zip_into(&zip, &target).unwrap(),
            JetbrainsLocalInstallOutcome::Installed
        );
        assert_eq!(
            fs::read(lib.join("agent-doc-jetbrains-0.2.397.jar")).unwrap(),
            b"new"
        );
        assert!(!lib.join("stale.jar").exists());
    }

    #[test]
    fn java_is_resolved_from_the_target_ide_before_the_ambient_environment() {
        // GH #63: remote-dev backend with JAVA_HOME unset and no java on PATH;
        // the IDE's own `<dist>/jbr/bin/java` must be found.
        let tmp = TempDir::new().unwrap();
        let dist = tmp.path().join("RemoteDev/dist/IU-262.9437.185");
        fs::create_dir_all(dist.join("bin")).unwrap();
        fs::create_dir_all(dist.join("jbr/bin")).unwrap();
        fs::write(dist.join("bin/idea"), b"").unwrap();
        fs::write(dist.join("jbr/bin/java"), b"").unwrap();
        let empty_path = tmp.path().join("empty-path");
        fs::create_dir_all(&empty_path).unwrap();

        let candidates = java_candidates_for_ide(
            Some(&dist.join("bin/idea")),
            None,
            Some(empty_path.as_os_str()),
        );
        assert_eq!(candidates[0], dist.join("bin/jbr/bin/java"));
        assert_eq!(
            resolve_java_for_ide(1506046, &candidates).unwrap(),
            dist.join("jbr/bin/java")
        );

        // A java-launched IDE uses its own executable first.
        let java = dist.join("jbr/bin/java");
        assert_eq!(java_candidates_for_ide(Some(&java), None, None)[0], java);
    }

    #[test]
    fn missing_jvm_names_every_candidate_tried() {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("no-jdk");
        let candidates = java_candidates_for_ide(None, Some(&home), None);
        let error = resolve_java_for_ide(7, &candidates)
            .unwrap_err()
            .to_string();
        assert!(error.contains("no JVM found"), "{error}");
        assert!(error.contains("pid 7"), "{error}");
        assert!(
            error.contains(&home.join("bin/java").display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn failed_dynamic_upgrade_falls_back_to_a_restart_required_file_replacement() {
        // GH #63: a failed hot-swap must not mean a failed update.
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.392.jar"), b"old").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.448.zip");
        write_test_jetbrains_zip(&zip, "0.2.448", b"new");

        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            true,
            || anyhow::bail!("checkCanUnloadWithoutRestart signature mismatch"),
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();

        match outcome {
            JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
                assert!(reason.contains("signature mismatch"), "{reason}");
            }
            other => panic!("expected RestartRequired, got {other:?}"),
        }
        assert_eq!(
            fs::read(lib.join("agent-doc-jetbrains-0.2.448.jar")).unwrap(),
            b"new"
        );
        assert!(jetbrains_local_zip_matches_installation(&zip, &target).unwrap());

        // GH #67: the refusal is recorded beside (not inside) the plugin tree, so the
        // byte-identity check above still passes and preflight can advise a restart.
        let marker = target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
        let recorded = fs::read_to_string(&marker).unwrap();
        assert!(recorded.contains("signature mismatch"), "{recorded}");
        assert_eq!(recorded.lines().count(), 1, "{recorded}");

        // A later install that converges without a restart clears it.
        install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            true,
            || Ok(Some(super::JetbrainsHotUpgrade::Upgraded { processes: 1 })),
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        assert!(!marker.exists());
    }

    #[test]
    fn fallback_warning_separates_a_platform_refusal_from_an_upgrader_failure() {
        // GH #80: a class the upgrader could not link is not the IDE refusing.
        let linkage = "JetBrains dynamic upgrade failed for pid 3814926: Exception in thread \"main\" \
             java.lang.IllegalStateException: error:java.lang.IllegalStateException:\
             agent-doc upgrader could not call the platform: java.lang.NoClassDefFoundError: \
             com/intellij/ide/plugins/DynamicPlugins$UnloadPluginOptions";
        let warning =
            super::dynamic_upgrade_fallback_warning(linkage, "replacing the plugin files instead");
        assert!(warning.contains("failed before the IDE"), "{warning}");
        assert!(!warning.contains("refused"), "{warning}");

        let refused = "JetBrains dynamic upgrade failed for pid 1: error:java.lang.IllegalStateException:\
             plugin cannot unload dynamically (the platform reported the plugin cannot unload without a restart)";
        let warning =
            super::dynamic_upgrade_fallback_warning(refused, "replacing the plugin files instead");
        assert!(warning.contains("IDE refused"), "{warning}");
        assert!(
            warning.ends_with("replacing the plugin files instead."),
            "{warning}"
        );
    }

    /// GH #108: the asynchronous-classloader guard is agent-doc declining before
    /// the IDE is consulted -- both as the current upgrader words it and as the
    /// 0.35.435-0.35.441 upgrader did under the platform-refusal prefix.
    #[test]
    fn async_retirement_decline_is_attributed_to_agent_doc_not_the_ide() {
        let current = "pid 427146: agent-doc declined the restart-free upgrade: this JetBrains build retires \
             plugin classloaders asynchronously (AwaitClassloaderUnloadAsyncPostReconfiguration), so there is \
             no safe synchronous swap point; restart-free upgrade is permanently unavailable on this build";
        let legacy = "pid 427146: plugin cannot unload dynamically: this JetBrains build verifies the outgoing \
             classloader only after loading the replacement; agent-doc staged the update before touching \
             the live plugin generation";
        for reason in [current, legacy] {
            let warning = super::dynamic_upgrade_fallback_warning(
                reason,
                "staged it for the next IDE start instead",
            );
            assert!(warning.starts_with("agent-doc declined"), "{warning}");
            assert!(!warning.contains("IDE refused"), "{warning}");
            assert!(
                warning.ends_with("staged it for the next IDE start instead."),
                "{warning}"
            );
            assert_eq!(super::dynamic_upgrade_decliner(reason), "agent-doc");
            let staged = super::staged_for_restart_message(Path::new("/p"), reason);
            assert!(staged.starts_with("agent-doc declined"), "{staged}");
            assert!(!staged.contains("upgrade failed"), "{staged}");
        }
        assert_eq!(
            super::dynamic_upgrade_decliner("plugin cannot unload dynamically: x"),
            "ide"
        );
        assert_eq!(
            super::dynamic_upgrade_decliner("NoClassDefFoundError"),
            "upgrader_failure"
        );
        assert!(super::permanent_dynamic_upgrade_loss_note().contains("permanently unavailable"));
    }

    /// GH #108: a staged install must not print "Plugin installed (v<old>)"; it
    /// names the staged version and the generation that stays loaded, and its
    /// decision reaches `ops.log` with pid, decliner, staged version and outcome.
    #[test]
    fn staged_install_reports_the_staged_version_and_logs_the_decision() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.479.jar"), b"live").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.480.zip");
        write_test_jetbrains_zip(&zip, "0.2.480", b"new");
        super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow_mut().clear());

        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.480",
            true,
            || {
                Ok(Some(super::JetbrainsHotUpgrade::StagedForRestart {
                    reason: "pid 427146: agent-doc declined the restart-free upgrade: async"
                        .to_string(),
                }))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();

        let headline =
            super::jetbrains_install_result_message(&target, &outcome, "0.2.480").unwrap();
        assert!(!headline.contains("Plugin installed"), "{headline}");
        assert!(
            headline.contains("v0.2.480 staged (not installed)"),
            "{headline}"
        );
        assert!(headline.contains("v0.2.479"), "{headline}");

        let logged = super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow().clone());
        assert_eq!(logged.len(), 1, "{logged:?}");
        let line = &logged[0];
        for field in [
            "plugin_dynamic_upgrade",
            "outcome=staged_for_restart",
            "declined_by=agent-doc",
            "staged_version=0.2.480",
            "pid 427146",
        ] {
            assert!(line.contains(field), "missing {field}: {line}");
        }

        let installed = super::jetbrains_install_result_message(
            &target,
            &JetbrainsLocalInstallOutcome::Installed,
            "0.2.480",
        )
        .unwrap();
        assert!(
            installed.starts_with("Plugin installed (v0.2.479)"),
            "{installed}"
        );
    }

    #[test]
    fn no_dynamic_replaces_files_and_requires_restart_only_under_a_live_ide() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.448.zip");
        write_test_jetbrains_zip(&zip, "0.2.448", b"new");

        let live = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            false,
            || panic!("--no-dynamic must not attach"),
            || Ok(vec![1506046]),
        )
        .unwrap();
        assert!(
            matches!(&live, JetbrainsLocalInstallOutcome::RestartRequired { reason } if reason.contains("1506046")),
            "{live:?}"
        );

        let idle = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            false,
            || panic!("--no-dynamic must not attach"),
            || Ok(Vec::new()),
        )
        .unwrap();
        assert_eq!(idle, JetbrainsLocalInstallOutcome::Installed);
    }

    /// `#jbstageonfail`: a failed hot-swap that the IDE staged for its next start
    /// leaves the live jars untouched instead of unlinking them under the JVM,
    /// and still records the restart verdict preflight reads.
    #[test]
    fn staged_dynamic_upgrade_leaves_live_jars_in_place_and_records_restart() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        let live_jar = lib.join("agent-doc-jetbrains-0.2.455.jar");
        fs::write(&live_jar, b"live").unwrap();
        let live_inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&live_jar).unwrap().ino()
        };
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.456.zip");
        write_test_jetbrains_zip(&zip, "0.2.456", b"new");

        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.456",
            true,
            || {
                Ok(Some(super::JetbrainsHotUpgrade::StagedForRestart {
                    reason: "pid 9: plugin cannot unload dynamically".to_string(),
                }))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();

        match &outcome {
            JetbrainsLocalInstallOutcome::StagedForRestart { reason } => {
                assert!(
                    reason.contains("plugin cannot unload dynamically"),
                    "{reason}"
                );
            }
            other => panic!("expected StagedForRestart, got {other:?}"),
        }
        assert_eq!(fs::read(&live_jar).unwrap(), b"live");
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&live_jar).unwrap().ino(), live_inode);
        }
        assert!(!lib.join("agent-doc-jetbrains-0.2.456.jar").exists());
        let marker = target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
        let recorded = fs::read_to_string(&marker).unwrap();
        assert!(recorded.contains("staged for restart"), "{recorded}");
        // GH #87: the staged version is recorded so preflight advises a restart.
        assert_eq!(
            agent_doc_fs::jetbrains_install::staged_version_from_restart_marker(&recorded)
                .as_deref(),
            Some("0.2.456"),
            "{recorded}"
        );
        // GH #115: and the generation it replaces.
        assert_eq!(
            agent_doc_fs::jetbrains_install::previous_version_from_restart_marker(&recorded)
                .as_deref(),
            Some("0.2.455"),
            "{recorded}"
        );
    }

    /// GH #115: a viable pending staging of the same version is reported, never
    /// staged a second time; a doomed or different-version one is re-staged.
    #[test]
    fn second_staging_of_a_pending_version_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("data/IntelliJIdea2026.3");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.480.jar"), b"live").unwrap();
        let system = tmp.path().join("cache");
        let script_dir = system.join("IntelliJIdea2026.3/plugins");
        fs::create_dir_all(&script_dir).unwrap();
        let roots = vec![system.clone()];
        assert!(super::already_staged_outcome(&target, &roots, "0.2.481").is_none());

        let zip = script_dir.join("agent-doc-jetbrains-0.2.481+0f.zip");
        fs::write(
            script_dir.join("action.script"),
            format!(
                "delete:{t}/agent-doc-jetbrains\ndelete:{t}/agent-doc-jetbrains\nunzip:{z}:{t}\ndelete:{z}\n",
                t = target.display(),
                z = zip.display()
            ),
        )
        .unwrap();
        // Doomed staging (package gone): re-staging is the repair, not a skip.
        assert!(super::already_staged_outcome(&target, &roots, "0.2.481").is_none());

        fs::write(&zip, b"pkg").unwrap();
        assert!(super::already_staged_outcome(&target, &roots, "0.2.482").is_none());
        fs::write(
            target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER),
            "pid 9: agent-doc declined the restart-free upgrade because it is permanently unavailable on this build\nstaged_version=0.2.481\nprevious_version=0.2.480\n",
        )
        .unwrap();
        let outcome = super::already_staged_outcome(&target, &roots, "0.2.481")
            .expect("the viable staging should be reused");
        assert_eq!(
            super::PluginTargetOutcome::from_jetbrains(&outcome),
            super::PluginTargetOutcome::StagedForRestart { permanent: true },
            "reusing a staging must preserve the marker's permanent decline"
        );
        match outcome {
            JetbrainsLocalInstallOutcome::StagedForRestart { reason } => {
                assert!(reason.contains("already staged"), "{reason}");
                assert!(reason.contains("agent-doc declined"), "{reason}");
            }
            other => panic!("expected the existing staging, got {other:?}"),
        }
        let recorded = fs::read_to_string(
            target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER),
        )
        .unwrap();
        assert_eq!(
            agent_doc_fs::jetbrains_install::staged_version_from_restart_marker(&recorded)
                .as_deref(),
            Some("0.2.481")
        );
        assert_eq!(
            agent_doc_fs::jetbrains_install::previous_version_from_restart_marker(&recorded)
                .as_deref(),
            Some("0.2.480")
        );
        assert!(
            fs::read_to_string(script_dir.join("action.script"))
                .unwrap()
                .matches("unzip:")
                .count()
                == 1,
            "the skip must not touch the pending script"
        );
    }

    /// GH #115: reconciliation reinstalls an installation a failed staging
    /// destroyed instead of skipping it for having no jar.
    #[test]
    fn destroyed_installation_is_still_a_reconcile_target() {
        let tmp = TempDir::new().unwrap();
        let destroyed = tmp.path().join("IntelliJIdea2026.3");
        let never = tmp.path().join("IntelliJIdea2026.2");
        fs::create_dir_all(&destroyed).unwrap();
        fs::create_dir_all(&never).unwrap();
        fs::write(
            destroyed.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER),
            "staged\nstaged_version=0.2.481\nprevious_version=0.2.480\n",
        )
        .unwrap();
        let dirs = vec![destroyed.clone(), never.clone()];
        assert_eq!(
            super::existing_jetbrains_agent_doc_dirs_in(&dirs, &[]),
            vec![destroyed]
        );
    }

    #[test]
    fn staged_release_install_records_expected_version_from_a_temp_named_package() {
        // GH #87 (reopened): `plugin update jetbrains` downloads the release asset to
        // a `NamedTempFile` (`.tmpXXXXXX`), so the staged version must come from the
        // version the caller read out of the package, never from the filename.
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        fs::create_dir_all(target.join("agent-doc-jetbrains/lib")).unwrap();
        let download = tempfile::NamedTempFile::new_in(tmp.path()).unwrap();
        let name = download.path().file_name().unwrap().to_str().unwrap();
        assert!(!name.starts_with("agent-doc-jetbrains-"), "{name}");
        assert!(!name.ends_with(".zip"), "{name}");
        write_test_jetbrains_zip(download.path(), "0.2.467", b"new");

        let outcome = install_jetbrains_package_bytes(
            download.path(),
            &target,
            "0.2.467",
            true,
            || {
                Ok(Some(super::JetbrainsHotUpgrade::StagedForRestart {
                    reason: "pid 9: plugin cannot unload dynamically".to_string(),
                }))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        assert!(
            matches!(
                outcome,
                JetbrainsLocalInstallOutcome::StagedForRestart { .. }
            ),
            "{outcome:?}"
        );

        let marker = target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
        let recorded = fs::read_to_string(&marker).unwrap();
        assert_eq!(
            agent_doc_fs::jetbrains_install::staged_version_from_restart_marker(&recorded)
                .as_deref(),
            Some("0.2.467"),
            "{recorded}"
        );
    }

    #[test]
    fn staged_status_parses_reason_and_a_converged_process_wins() {
        assert_eq!(
            super::staged_upgrade_reason("staged:0.2.456:plugin cannot unload dynamically: x")
                .as_deref(),
            Some("plugin cannot unload dynamically: x")
        );
        assert_eq!(
            super::staged_upgrade_reason("staged:0.2.456").as_deref(),
            Some("the dynamic upgrade failed")
        );
        assert_eq!(super::staged_upgrade_reason("ok:0.2.456"), None);

        assert_eq!(
            super::jetbrains_hot_upgrade_verdict(1, None, None, Some("pid 2: refused".into())),
            Some(super::JetbrainsHotUpgrade::Upgraded { processes: 1 })
        );
        assert_eq!(
            super::jetbrains_hot_upgrade_verdict(0, None, None, Some("pid 2: refused".into())),
            Some(super::JetbrainsHotUpgrade::StagedForRestart {
                reason: "pid 2: refused".into()
            })
        );
        assert_eq!(
            super::jetbrains_hot_upgrade_verdict(0, None, None, None),
            None
        );
        // `#jbdynamicfalsereport`: a proven upgrade beside a live owner that runs
        // no plugin is not a restart-free convergence.
        assert_eq!(
            super::jetbrains_hot_upgrade_verdict(1, None, Some("pid 3: not loaded".into()), None),
            Some(super::JetbrainsHotUpgrade::InstalledUnverified {
                reason: "pid 3: not loaded".into()
            })
        );
    }

    /// `#jbdynamicfalsereport`: the upgrader's statuses parse into owner classes,
    /// including the plugins directory a not-loaded IDE reports.
    #[test]
    fn upgrader_status_parses_ok_staged_and_not_loaded() {
        use super::JetbrainsUpgraderStatus as S;
        assert_eq!(
            super::parse_jetbrains_upgrader_status("noise\nok:0.2.448:documents=1/1\n"),
            S::Ok {
                status_line: "ok:0.2.448:documents=1/1".into()
            }
        );
        assert_eq!(
            super::parse_jetbrains_upgrader_status("staged:0.2.448:refused"),
            S::Staged {
                reason: "refused".into()
            }
        );
        assert_eq!(
            super::parse_jetbrains_upgrader_status("skip:plugin-not-loaded"),
            S::PluginNotLoaded { plugins_path: None }
        );
        assert_eq!(
            super::parse_jetbrains_upgrader_status(
                "skip:plugin-not-loaded:plugins-path=/home/u/.local/share/JetBrains/IntelliJIdea2026.1"
            ),
            S::PluginNotLoaded {
                plugins_path: Some("/home/u/.local/share/JetBrains/IntelliJIdea2026.1".into())
            }
        );
        assert_eq!(
            super::parse_jetbrains_upgrader_status("skip:different-plugin-root:/x"),
            S::NotOwner
        );
        assert_eq!(super::parse_jetbrains_upgrader_status(""), S::NotOwner);
    }

    /// `#jbdynamicfalsereport` regression (1): `make install` into a running IDE
    /// whose agent-doc plugin directory had vanished reported "dynamically
    /// replaced; no IDE restart is required". The upgrader said
    /// `skip:plugin-not-loaded`, which the installer discarded as "no live IDE".
    #[test]
    fn no_plugin_loaded_before_install_reports_restart_required() {
        use super::JetbrainsUpgraderStatus as S;
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        fs::create_dir_all(&target).unwrap();
        let no_proof = |_pid: u32| -> bool { panic!("a not-loaded IDE has nothing to prove") };

        for plugins_path in [None, Some(target.clone())] {
            let verdict = super::jetbrains_hot_upgrade_from_statuses(
                vec![(41, S::PluginNotLoaded { plugins_path })],
                &target,
                "0.2.448",
                no_proof,
            );
            assert!(
                matches!(
                    &verdict,
                    Some(super::JetbrainsHotUpgrade::NotLoaded { reason }) if reason.contains("pid 41")
                ),
                "{verdict:?}"
            );
        }
        // An IDE loading plugins from a different directory does not own this target.
        assert_eq!(
            super::jetbrains_hot_upgrade_from_statuses(
                vec![(
                    41,
                    S::PluginNotLoaded {
                        plugins_path: Some(tmp.path().join("other-ide"))
                    }
                )],
                &target,
                "0.2.448",
                no_proof,
            ),
            None
        );

        let zip = tmp.path().join("agent-doc-jetbrains-0.2.448.zip");
        write_test_jetbrains_zip(&zip, "0.2.448", b"new");
        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            true,
            || {
                Ok(super::jetbrains_hot_upgrade_from_statuses(
                    vec![(41, S::PluginNotLoaded { plugins_path: None })],
                    &target,
                    "0.2.448",
                    no_proof,
                ))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        match &outcome {
            JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
                assert!(reason.contains("no agent-doc plugin loaded"), "{reason}");
            }
            other => panic!("expected RestartRequired, got {other:?}"),
        }
        assert!(jetbrains_local_zip_matches_installation(&zip, &target).unwrap());
        let marker = target.join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER);
        assert!(marker.exists());
        let summary = super::jetbrains_convergence_restart_summary(1, 0, 1);
        assert!(!summary.contains("dynamically replaced"), "{summary}");
        assert!(summary.contains("restart"), "{summary}");
    }

    /// `#jbdynamicfalsereport` regression (2): an `ok:` receipt is not enough; with
    /// no post-replace load proof within the timeout the install must say restart.
    #[test]
    fn upgrader_ok_without_live_load_proof_requires_restart() {
        use super::JetbrainsUpgraderStatus as S;
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        fs::create_dir_all(&target).unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.448.zip");
        write_test_jetbrains_zip(&zip, "0.2.448", b"new");
        // The in-IDE install already wrote the tree.
        super::replace_jetbrains_plugin_tree(&zip, &target).unwrap();
        let jar = target.join("agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.448.jar");
        let before = fs::metadata(&jar).unwrap();

        let mut probed = Vec::new();
        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            true,
            || {
                Ok(super::jetbrains_hot_upgrade_from_statuses(
                    vec![(
                        42,
                        S::Ok {
                            status_line: "ok:0.2.448".into(),
                        },
                    )],
                    &target,
                    "0.2.448",
                    |pid| {
                        probed.push(pid);
                        false
                    },
                ))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        assert_eq!(probed, vec![42]);
        match &outcome {
            JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
                assert!(reason.contains("not verified live"), "{reason}");
                assert!(reason.contains("pid 42"), "{reason}");
            }
            other => panic!("expected RestartRequired, got {other:?}"),
        }
        // Not rewritten again under a JVM that may map it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&jar).unwrap().ino(), before.ino());
        }
        let _ = before;
        assert!(
            target
                .join(agent_doc_preflight_io::warnings::PLUGIN_RESTART_REQUIRED_MARKER)
                .exists()
        );
    }

    /// `#jbdynamicfalsereport` regression (3): an `ok:` receipt plus an observed
    /// live load is a dynamic replacement.
    #[test]
    fn observed_live_load_reports_dynamic_replacement() {
        use super::JetbrainsUpgraderStatus as S;
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        fs::create_dir_all(&target).unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.448.zip");
        write_test_jetbrains_zip(&zip, "0.2.448", b"new");
        super::replace_jetbrains_plugin_tree(&zip, &target).unwrap();
        let jar = target.join("agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.448.jar");

        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.448",
            true,
            || {
                Ok(super::jetbrains_hot_upgrade_from_statuses(
                    vec![(
                        43,
                        S::Ok {
                            status_line: "ok:0.2.448".into(),
                        },
                    )],
                    &target,
                    "0.2.448",
                    |_pid| {
                        super::jetbrains_mapped_jar_proves_load(
                            &agent_doc_fs::plugin_jar::MappedPluginJar::Current {
                                path: jar.to_string_lossy().into_owned(),
                                inode: 1,
                            },
                            &target,
                            "0.2.448",
                        )
                    },
                ))
            },
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        assert_eq!(
            outcome,
            JetbrainsLocalInstallOutcome::HotUpgraded { processes: 1 }
        );
        assert!(
            super::jetbrains_convergence_restart_summary(1, 1, 0).contains("dynamically replaced")
        );
    }

    #[test]
    fn mapped_jar_proves_load_only_for_the_expected_live_jar_in_the_target() {
        use agent_doc_fs::plugin_jar::MappedPluginJar as M;
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        let jar = lib.join("agent-doc-jetbrains-0.2.448.jar");
        fs::write(&jar, b"x").unwrap();
        let current = |path: &Path| M::Current {
            path: path.to_string_lossy().into_owned(),
            inode: 1,
        };
        assert!(super::jetbrains_mapped_jar_proves_load(
            &current(&jar),
            &target,
            "0.2.448"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &current(&jar),
            &target,
            "0.2.449"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &current(&jar),
            &tmp.path().join("other"),
            "0.2.448"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &M::Deleted {
                path: jar.to_string_lossy().into_owned()
            },
            &target,
            "0.2.448"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &M::Unknown,
            &target,
            "0.2.448"
        ));
    }

    /// `#jb262dynupgrade`: a live 262 IDE maps the modular root jar
    /// `agent.doc-<v>.jar`, which proves a load of that generation exactly as the
    /// classic jar does.
    #[test]
    fn mapped_jar_proves_load_for_the_modular_262_jar() {
        use agent_doc_fs::plugin_jar::MappedPluginJar as M;
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(lib.join("modules")).unwrap();
        let jar = lib.join("agent.doc-0.2.514.jar");
        fs::write(&jar, b"x").unwrap();
        let module = lib.join("modules/agent.doc.backend.jar");
        fs::write(&module, b"x").unwrap();
        let current = |path: &Path| M::Current {
            path: path.to_string_lossy().into_owned(),
            inode: 1,
        };
        assert!(super::jetbrains_mapped_jar_proves_load(
            &current(&jar),
            &target,
            "0.2.514"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &current(&jar),
            &target,
            "0.2.515"
        ));
        assert!(!super::jetbrains_mapped_jar_proves_load(
            &current(&module),
            &target,
            "0.2.514"
        ));
    }

    /// `#jb262dynupgrade`: the package line is read from the ZIP's versioned
    /// plugin jar, so the hot-upgrade path knows a 262 package before it looks
    /// for a launcher.
    #[test]
    fn jetbrains_zip_package_range_reads_the_plugin_jar_line() {
        let tmp = TempDir::new().unwrap();
        let classic = tmp.path().join("classic.zip");
        let modular = tmp.path().join("modular.zip");
        write_test_jetbrains_zip(&classic, "0.2.513", b"classic");
        write_test_modular_jetbrains_zip(&modular, "0.2.514", b"modular");
        assert_eq!(
            super::jetbrains_zip_package_range(&classic).unwrap(),
            JetbrainsPackageRange::Classic242To261
        );
        assert_eq!(
            super::jetbrains_zip_package_range(&modular).unwrap(),
            JetbrainsPackageRange::Modular262
        );
    }

    /// `#jb262dynupgrade`: a 262 package is refused for an accurate reason --
    /// the modular line has no upgrade launcher -- without inspecting a launcher
    /// and never with the "predates restart-free support" legacy wording. The
    /// classic line keeps its Main-Class check.
    #[test]
    fn dynamic_upgrade_entry_point_refusal_names_the_modular_262_package() {
        let modular = super::jetbrains_dynamic_upgrade_entry_point_refusal(
            JetbrainsPackageRange::Modular262,
            "0.2.514",
            || panic!("the modular package has no launcher to inspect"),
        )
        .unwrap()
        .expect("a 262 package has no restart-free entry point");
        assert!(
            modular.contains(super::JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE),
            "{modular}"
        );
        assert!(modular.contains("agent.doc-0.2.514.jar"), "{modular}");
        assert!(!modular.contains("predates"), "{modular}");
        assert!(!modular.contains("legacy"), "{modular}");

        let legacy = super::jetbrains_dynamic_upgrade_entry_point_refusal(
            JetbrainsPackageRange::Classic242To261,
            "0.2.300",
            || Ok(false),
        )
        .unwrap()
        .expect("a classic jar without Main-Class predates the launcher");
        assert!(legacy.contains("predates restart-free"), "{legacy}");

        assert_eq!(
            super::jetbrains_dynamic_upgrade_entry_point_refusal(
                JetbrainsPackageRange::Classic242To261,
                "0.2.513",
                || Ok(true),
            )
            .unwrap(),
            None
        );
        assert!(
            super::jetbrains_dynamic_upgrade_entry_point_refusal(
                JetbrainsPackageRange::Classic242To261,
                "0.2.513",
                || anyhow::bail!("unreadable launcher"),
            )
            .is_err()
        );
    }

    /// `#jb262dynupgrade`: a live 262 IDE still gets its package replaced on
    /// disk and a restart-required verdict, but the warning, decliner and
    /// recorded reason say why -- the modular line has no entry point -- instead
    /// of claiming an upgrader failure or a legacy package.
    #[test]
    fn modular_262_package_falls_back_to_restart_with_an_accurate_reason() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("plugins");
        let lib = target.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent.doc-0.2.513.jar"), b"old").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-262-0.2.514.zip");
        write_test_modular_jetbrains_zip(&zip, "0.2.514", b"new");
        let refusal = super::jetbrains_dynamic_upgrade_entry_point_refusal(
            JetbrainsPackageRange::Modular262,
            "0.2.514",
            || unreachable!(),
        )
        .unwrap()
        .unwrap();

        super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow_mut().clear());
        let outcome = install_jetbrains_package_bytes(
            &zip,
            &target,
            "0.2.514",
            true,
            || anyhow::bail!("{refusal}"),
            || panic!("the dynamic path does not enumerate pids"),
        )
        .unwrap();
        match &outcome {
            JetbrainsLocalInstallOutcome::RestartRequired { reason } => {
                assert!(
                    reason.contains(super::JETBRAINS_MODULAR_NO_DYNAMIC_UPGRADE),
                    "{reason}"
                );
            }
            other => panic!("expected RestartRequired, got {other:?}"),
        }
        assert_eq!(fs::read(lib.join("agent.doc-0.2.514.jar")).unwrap(), b"new");
        assert_eq!(super::dynamic_upgrade_decliner(&refusal), "modular_package");
        let warning =
            super::dynamic_upgrade_fallback_warning(&refusal, "replacing the plugin files instead");
        assert!(warning.contains("exact-262 modular"), "{warning}");
        assert!(!warning.contains("failed"), "{warning}");
        assert!(!warning.contains("refused"), "{warning}");
        let logged = super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow().clone());
        assert!(
            logged
                .iter()
                .any(|line| line.contains("declined_by=modular_package")),
            "{logged:?}"
        );
        // Not an agent-doc platform decline: no permanent-loss classification.
        assert!(!super::agent_doc_declined_dynamic_upgrade(&refusal));
    }

    /// A staged `--local` install leaves the live generation's jar in place, so
    /// "built N, installed N-1" is the expected state, not a verification failure.
    /// Any other outcome still requires the installed version to match.
    #[test]
    fn staged_local_install_accepts_the_live_generation_jar() {
        let dir = std::path::Path::new("/plugins");
        let staged = JetbrainsLocalInstallOutcome::StagedForRestart {
            reason: "dynamic upgrade failed".to_string(),
        };
        assert!(verify_local_install_version(&staged, dir, "0.2.468", "0.2.467").is_ok());
        let err = verify_local_install_version(
            &JetbrainsLocalInstallOutcome::Installed,
            dir,
            "0.2.468",
            "0.2.467",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("built 0.2.468, installed 0.2.467"),
            "{err}"
        );
        assert!(
            verify_local_install_version(
                &JetbrainsLocalInstallOutcome::Installed,
                dir,
                "0.2.468",
                "0.2.468"
            )
            .is_ok()
        );
    }

    /// A staged or restart-required install must not close with "no IDE restart
    /// is required".
    #[test]
    fn convergence_summary_names_a_pending_restart() {
        assert!(
            super::jetbrains_convergence_restart_summary(1, 0, 1).contains("restart those IDEs")
        );
        assert!(
            super::jetbrains_convergence_restart_summary(2, 2, 0)
                .contains("no IDE restart is required")
        );
        assert!(
            super::jetbrains_convergence_restart_summary(0, 0, 0)
                .contains("no installed plugin bytes changed")
        );
        // `#jbdynamicfalsereport`: a cold install never claims a dynamic replacement.
        let cold = super::jetbrains_convergence_restart_summary(1, 0, 0);
        assert!(!cold.contains("dynamically replaced"), "{cold}");
        assert!(!cold.contains("no IDE restart is required"), "{cold}");
    }

    fn reconcile_target(
        family: super::PluginEditorFamily,
        label: &str,
        version: &str,
        outcome: super::PluginTargetOutcome,
    ) -> super::PluginTargetReport {
        super::PluginTargetReport {
            family,
            label: label.to_string(),
            version: version.to_string(),
            outcome,
        }
    }

    /// GH #114: the reported 441 -> 442 run — one direct install, one target
    /// staged behind a JVM where agent-doc declined the restart-free upgrade.
    #[test]
    fn reconcile_summary_names_permanent_staged_restart_without_hedging() {
        use super::{PluginEditorFamily::JetBrains, PluginTargetOutcome as O};
        let report = super::PluginReconcileReport {
            targets: vec![
                reconcile_target(JetBrains, "IntelliJIdea2026.2", "0.2.481", O::Installed),
                reconcile_target(
                    JetBrains,
                    "IntelliJIdea2026.3",
                    "0.2.481",
                    O::StagedForRestart { permanent: true },
                ),
            ],
        };
        let summary = report.summary("0.35.442");
        assert!(
            summary.starts_with(
                "Editor plugins reconciled with the v0.35.442 release: 1 installed, 1 staged for restart."
            ),
            "{summary}"
        );
        assert!(
            summary.contains(
                "Restart IntelliJIdea2026.3 to load plugin v0.2.481; restart-free upgrade is unavailable on this build."
            ),
            "{summary}"
        );
        assert!(
            summary.contains("IntelliJIdea2026.2: plugin v0.2.481 installed; no live IDE held it"),
            "{summary}"
        );
        assert!(!summary.contains("on its own"), "{summary}");
        assert!(!summary.contains("Updated 2"), "{summary}");
        assert!(
            summary.contains("only the plugin needs the restart"),
            "{summary}"
        );
        assert_eq!(report.changed(), 2);

        super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow_mut().clear());
        super::report_reconcile_summary(&report, "0.35.442");
        let logged = super::LOGGED_UPGRADE_DECISIONS.with(|log| log.borrow().clone());
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(
            logged[0].contains("restart_targets=\"IntelliJIdea2026.3@0.2.481\"")
                && logged[0].contains("restart_free_unavailable=true"),
            "{logged:?}"
        );
    }

    #[test]
    fn reconcile_summary_counts_mixed_outcomes_and_restart_targets() {
        use super::{
            PluginEditorFamily::{JetBrains, VsCode},
            PluginTargetOutcome as O,
        };
        let report = super::PluginReconcileReport {
            targets: vec![
                reconcile_target(JetBrains, "IntelliJIdea2026.1", "0.2.481", O::HotUpgraded),
                reconcile_target(JetBrains, "PyCharm2026.2", "0.2.481", O::HotUpgraded),
                reconcile_target(JetBrains, "GoLand2026.2", "0.2.481", O::Unchanged),
                reconcile_target(JetBrains, "RustRover2026.2", "0.2.481", O::RestartRequired),
                reconcile_target(
                    JetBrains,
                    "WebStorm2026.2",
                    "0.2.481",
                    O::StagedForRestart { permanent: false },
                ),
                reconcile_target(VsCode, "code", "0.35.442", O::Installed),
            ],
        };
        let summary = report.summary("0.35.442");
        assert!(
            summary.starts_with(
                "Editor plugins reconciled with the v0.35.442 release: 2 hot-upgraded, 1 installed, 1 staged for restart, 1 replaced under a live IDE, 1 unchanged."
            ),
            "{summary}"
        );
        assert_eq!(report.changed(), 5);
        assert_eq!(report.hot_upgraded(), 2);
        // A transient decline still needs a restart now, but must not claim the
        // restart-free path is gone for good.
        assert!(
            summary.contains("Restart WebStorm2026.2 to load plugin v0.2.481; the restart-free upgrade did not complete"),
            "{summary}"
        );
        assert!(
            summary.contains("Restart RustRover2026.2 to load plugin v0.2.481; the running IDE keeps the previous plugin until then."),
            "{summary}"
        );
        assert!(!summary.contains("unavailable on this build"), "{summary}");
        // Hot-upgraded and unchanged targets need no action line.
        assert!(!summary.contains("IntelliJIdea2026.1"), "{summary}");
        assert!(!summary.contains("GoLand2026.2"), "{summary}");
        // The conditional wording survives only where a reload may still land.
        let hedged = summary
            .lines()
            .filter(|line| line.contains("on its own"))
            .collect::<Vec<_>>();
        assert_eq!(
            hedged,
            vec![
                "code: extension v0.35.442 installed; reload the editor window if it does not pick it up on its own."
            ],
            "{summary}"
        );

        let logged = report.ops_log_line("0.35.442");
        for field in [
            "plugin_upgrade_summary release=0.35.442",
            "hot_upgraded=2",
            "installed=1",
            "staged_for_restart=1",
            "restart_required=1",
            "unchanged=1",
            "restart_targets=\"RustRover2026.2@0.2.481,WebStorm2026.2@0.2.481\"",
            "restart_free_unavailable=false",
        ] {
            assert!(logged.contains(field), "missing {field}: {logged}");
        }
    }

    #[test]
    fn reconcile_summary_all_unchanged_has_no_restart_text() {
        use super::{PluginEditorFamily::JetBrains, PluginTargetOutcome as O};
        let report = super::PluginReconcileReport {
            targets: vec![
                reconcile_target(JetBrains, "IntelliJIdea2026.2", "0.2.481", O::Unchanged),
                reconcile_target(JetBrains, "IntelliJIdea2026.3", "0.2.481", O::Unchanged),
            ],
        };
        let summary = report.summary("0.35.442");
        assert_eq!(summary, "Installed editor plugins already match v0.35.442.");
        assert_eq!(report.changed(), 0);
        assert_eq!(report.hot_upgraded(), 0);
        assert_eq!(
            super::PluginReconcileReport::default().summary("0.35.442"),
            summary
        );
    }

    #[test]
    fn hot_upgrade_only_summary_has_no_restart_or_native_library_line() {
        use super::{PluginEditorFamily::JetBrains, PluginTargetOutcome as O};
        let report = super::PluginReconcileReport {
            targets: vec![reconcile_target(
                JetBrains,
                "IntelliJIdea2026.3",
                "0.2.481",
                O::HotUpgraded,
            )],
        };
        let summary = report.summary("0.35.442");
        assert_eq!(
            summary,
            "Editor plugins reconciled with the v0.35.442 release: 1 hot-upgraded."
        );
    }

    /// GH #114: the per-target outcome is derived from the install's real
    /// result, and only agent-doc's own decline is marked permanent.
    #[test]
    fn staged_install_outcome_carries_permanence_into_the_reconcile_report() {
        for (reason, permanent) in [
            (
                "pid 1: agent-doc declined the restart-free upgrade: async",
                true,
            ),
            ("pid 1: plugin cannot unload dynamically: busy", false),
        ] {
            let tmp = TempDir::new().unwrap();
            let target = tmp.path().join("plugins");
            let zip = tmp.path().join("agent-doc-jetbrains-0.2.481.zip");
            write_test_jetbrains_zip(&zip, "0.2.481", b"new");
            let outcome = install_jetbrains_package_bytes(
                &zip,
                &target,
                "0.2.481",
                true,
                || {
                    Ok(Some(super::JetbrainsHotUpgrade::StagedForRestart {
                        reason: reason.to_string(),
                    }))
                },
                || panic!("the dynamic path does not enumerate pids"),
            )
            .unwrap();
            assert_eq!(
                super::PluginTargetOutcome::from_jetbrains(&outcome),
                super::PluginTargetOutcome::StagedForRestart { permanent },
                "{reason}"
            );
        }
        assert_eq!(
            super::PluginTargetOutcome::from_jetbrains(
                &JetbrainsLocalInstallOutcome::RestartRequired {
                    reason: "x".to_string()
                }
            ),
            super::PluginTargetOutcome::RestartRequired
        );
    }

    #[test]
    fn jetbrains_target_label_names_the_ide_data_directory() {
        assert_eq!(
            super::jetbrains_target_label(Path::new(
                "/h/.local/share/JetBrains/IntelliJIdea2026.3/plugins"
            )),
            "IntelliJIdea2026.3"
        );
        assert_eq!(
            super::jetbrains_target_label(Path::new(
                "/h/.local/share/JetBrains/IntelliJIdea2026.3"
            )),
            "IntelliJIdea2026.3"
        );
    }
}

pub fn list() -> Result<()> {
    let mut found = false;

    // JetBrains
    let dirs = jetbrains_plugin_dirs();
    let system_roots = agent_doc_fs::jetbrains_install::jetbrains_system_roots();
    for d in &dirs {
        let failure = agent_doc_fs::jetbrains_install::staged_install_failure(d, &system_roots);
        if let Some(version) = installed_jetbrains_plugin_version(d) {
            println!("jetbrains  v{}  {}", version, d.display());
            found = true;
        } else if let Some(agent_doc_fs::jetbrains_install::StagedInstallFailure::Destroyed {
            staged,
            ..
        }) = &failure
        {
            println!(
                "jetbrains  MISSING (staged v{staged} deleted the plugin without installing it)  {}",
                d.display()
            );
            found = true;
        }
        // GH #115: say it loudly, with the remedy.
        if let Some(failure) = &failure {
            eprintln!(
                "WARNING: {}",
                agent_doc_fs::jetbrains_install::staged_install_failure_message(d, failure)
            );
        }
    }

    // VS Code. No CLI is not an error here — `list` reports what is installed,
    // and an absent editor simply contributes nothing.
    if let Some(code) = detect_code_cmd()
        && let Ok(output) = std::process::Command::new(code)
            .args(["--list-extensions", "--show-versions"])
            .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.to_lowercase().contains("agent-doc") {
                println!("vscode     {}", line);
                found = true;
            }
        }
    }

    if !found {
        eprintln!("No agent-doc editor plugins found.");
    }

    Ok(())
}

/// `#jbpluginvanish`: the JetBrains plugin tree is replaced atomically -- a
/// failure anywhere before the swap completes leaves the installed plugin in
/// place.
#[cfg(test)]
mod plugin_tree_replace_tests {
    use super::{
        JETBRAINS_INSTALL_WORK_DIR, PluginTreeReplaceStep, replace_jetbrains_plugin_tree,
        replace_jetbrains_plugin_tree_with,
    };
    use std::fs;
    use std::io::{self, Write as _};
    use std::path::Path;
    use tempfile::TempDir;

    fn write_package(path: &Path, version: &str, plugin: &[u8]) {
        let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        archive
            .start_file(
                format!("agent-doc-jetbrains/lib/agent-doc-jetbrains-{version}.jar"),
                options,
            )
            .unwrap();
        archive.write_all(plugin).unwrap();
        archive
            .start_file("agent-doc-jetbrains/lib/dependency.jar", options)
            .unwrap();
        archive.write_all(b"dependency").unwrap();
        archive.finish().unwrap();
    }

    /// A plugins dir holding an installed 0.2.487 and a 0.2.488 package.
    fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = TempDir::new().unwrap();
        let plugins = tmp.path().join("IntelliJIdea2026.1");
        let lib = plugins.join("agent-doc-jetbrains/lib");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("agent-doc-jetbrains-0.2.487.jar"), b"old plugin").unwrap();
        fs::write(lib.join("dependency.jar"), b"old dependency").unwrap();
        let zip = tmp.path().join("agent-doc-jetbrains-0.2.488.zip");
        write_package(&zip, "0.2.488", b"new plugin");
        (tmp, plugins, zip)
    }

    fn assert_old_plugin_intact(plugins: &Path) {
        let lib = plugins.join("agent-doc-jetbrains/lib");
        assert_eq!(
            fs::read(lib.join("agent-doc-jetbrains-0.2.487.jar")).unwrap(),
            b"old plugin"
        );
        assert_eq!(
            fs::read(lib.join("dependency.jar")).unwrap(),
            b"old dependency"
        );
        assert!(!lib.join("agent-doc-jetbrains-0.2.488.jar").exists());
        assert!(
            !plugins.join(JETBRAINS_INSTALL_WORK_DIR).exists(),
            "no staging or backup left beside the plugin"
        );
    }

    fn enospc() -> io::Error {
        io::Error::from_raw_os_error(28)
    }

    /// The 2026-10-04 shape: the disk fills while the new package is written.
    /// The old code had already deleted the installed plugin by then.
    #[test]
    fn disk_full_mid_extraction_keeps_the_installed_plugin() {
        let (_tmp, plugins, zip) = fixture();
        let mut writes = 0;
        let error = replace_jetbrains_plugin_tree_with(&zip, &plugins, &mut |step| {
            if step == PluginTreeReplaceStep::WriteStagedFile {
                writes += 1;
                if writes == 2 {
                    return Err(enospc());
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("installed plugin was kept"),
            "{error:#}"
        );
        assert_old_plugin_intact(&plugins);
    }

    #[test]
    fn failure_moving_the_old_tree_aside_keeps_it() {
        let (_tmp, plugins, zip) = fixture();
        replace_jetbrains_plugin_tree_with(&zip, &plugins, &mut |step| {
            if step == PluginTreeReplaceStep::BackupOldTree {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            Ok(())
        })
        .unwrap_err();
        assert_old_plugin_intact(&plugins);
    }

    /// The old tree is already aside when the swap fails: it is renamed back.
    #[test]
    fn failed_swap_restores_the_old_tree() {
        let (_tmp, plugins, zip) = fixture();
        let error = replace_jetbrains_plugin_tree_with(&zip, &plugins, &mut |step| {
            if step == PluginTreeReplaceStep::SwapInStagedTree {
                return Err(enospc());
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("previous plugin was restored"),
            "{error:#}"
        );
        assert_old_plugin_intact(&plugins);
    }

    /// A package without a versioned plugin jar never replaces a working tree.
    #[test]
    fn package_without_a_plugin_jar_keeps_the_installed_plugin() {
        let (tmp, plugins, _) = fixture();
        let zip = tmp.path().join("broken.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        archive
            .start_file(
                "agent-doc-jetbrains/lib/dependency.jar",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"dependency").unwrap();
        archive.finish().unwrap();
        replace_jetbrains_plugin_tree(&zip, &plugins).unwrap_err();
        assert_old_plugin_intact(&plugins);
    }

    #[test]
    fn successful_replacement_swaps_in_the_new_tree() {
        let (_tmp, plugins, zip) = fixture();
        replace_jetbrains_plugin_tree(&zip, &plugins).unwrap();
        let lib = plugins.join("agent-doc-jetbrains/lib");
        assert_eq!(
            fs::read(lib.join("agent-doc-jetbrains-0.2.488.jar")).unwrap(),
            b"new plugin"
        );
        assert_eq!(fs::read(lib.join("dependency.jar")).unwrap(), b"dependency");
        assert!(!lib.join("agent-doc-jetbrains-0.2.487.jar").exists());
        assert!(!plugins.join(JETBRAINS_INSTALL_WORK_DIR).exists());
        // A fresh install (no previous tree) works too.
        fs::remove_dir_all(plugins.join("agent-doc-jetbrains")).unwrap();
        replace_jetbrains_plugin_tree(&zip, &plugins).unwrap();
        assert!(lib.join("agent-doc-jetbrains-0.2.488.jar").is_file());
    }

    /// A process killed between moving the old tree aside and moving the new
    /// one in leaves only the backup; the next replacement restores it first,
    /// so even a failing retry ends with the plugin present.
    #[test]
    fn interrupted_swap_backup_is_restored_before_retrying() {
        let (_tmp, plugins, zip) = fixture();
        let backup = plugins.join(JETBRAINS_INSTALL_WORK_DIR).join("backup-dead");
        fs::create_dir_all(backup.parent().unwrap()).unwrap();
        fs::rename(plugins.join("agent-doc-jetbrains"), &backup).unwrap();
        fs::create_dir_all(
            plugins
                .join(JETBRAINS_INSTALL_WORK_DIR)
                .join("staging-dead/x"),
        )
        .unwrap();
        replace_jetbrains_plugin_tree_with(&zip, &plugins, &mut |step| {
            if step == PluginTreeReplaceStep::WriteStagedFile {
                return Err(enospc());
            }
            Ok(())
        })
        .unwrap_err();
        assert_old_plugin_intact(&plugins);
    }
}
