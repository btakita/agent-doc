//! # Module: plugin
//!
//! ## Spec
//! - Manages editor plugin lifecycle (install, update, list) for JetBrains IDEs and VS Code-family editors (VS Code, VSCodium, Cursor).
//! - `install(editor)` — fetches the latest GitHub Release for `btakita/agent-doc`, selects the appropriate asset (signed variant preferred), downloads it, and installs it.
//! - `install_local(editor)` — installs from a locally built artifact found by walking up from CWD to locate an `editors/` directory.
//! - `update(editor)` — for JetBrains, skips re-install if the installed plugin.xml version matches the latest release tag; for VS Code, always reinstalls (handled idempotently by the CLI).
//! - `list()` — scans JetBrains plugin directories for the versioned agent-doc JAR and queries `code --list-extensions` for the VS Code extension; prints found entries to stdout.
//! - JetBrains plugin directories are discovered from versioned IDE data roots (`~/.local/share/JetBrains/<Product><Version>/` on Linux, `~/Library/Application Support/JetBrains/<Product><Version>/` on macOS). Config roots and unrelated JetBrains service directories are excluded. Callers can select an exact target with `--plugins-dir`; ambiguous non-interactive discovery fails with rerun guidance instead of waiting on stdin.
//! - VS Code CLI detection order: `cursor` → `codium` → `code` (first that succeeds `--version`).
//! - Asset selection: prefers `<prefix>-signed.<ext>`, falls back to any `<prefix>*.<ext>` match. For local JetBrains installs, prefers `-signed.zip` over `.zip`. Local VS Code installs require the VSIX version to match `package.json` exactly so stale artifacts cannot be installed by accident.
//!
//! ## Agentic Contracts
//! - `install(editor)` — returns `Err` on network failure, missing asset, or CLI install failure.
//! - `install_local(editor)` — returns `Err` if no `editors/` directory is found or no artifact exists.
//! - `update(editor)` — returns `Ok(())` early (no-op) when the JetBrains plugin is already at the latest version.
//! - `list()` — always returns `Ok(())`; emits a stderr message when no plugins are found.
//! - Unrecognized `editor` strings return `Err` with a list of supported values.
//! - Old JetBrains plugin installation (`agent-doc-jetbrains/` directory) is removed before extracting the new zip.
//!
//! ## Evals
//! - install_unknown_editor: `install("emacs")` → Err containing "Unknown editor"
//! - update_already_current: JetBrains plugin at matching version → early Ok, no download
//! - list_no_plugins: no IDE dirs, `code` absent → stderr "No agent-doc editor plugins found", Ok
//! - detect_code_cmd: cursor available → returns "cursor"; only code available → returns "code"
//! - find_asset_prefers_signed: release with both signed and unsigned zip → signed asset selected
//! - find_local_zip_prefers_signed: dist dir with both zips → signed path returned
//! - find_local_vscode_vsix_requires_manifest_version: stale VSIX files are ignored and a missing current build fails closed
//! - jetbrains_discovery_excludes_config_and_service_roots: only versioned IDE data roots are candidates

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::io::{self, IsTerminal as _, Read as _, Write as _};
use std::path::{Path, PathBuf};

const GITHUB_REPO: &str = "btakita/agent-doc";

fn build_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_recv_body(Some(std::time::Duration::from_secs(30)))
        .timeout_send_body(Some(std::time::Duration::from_secs(10)))
        .build()
        .into()
}

fn fetch_latest_release() -> Result<Value> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = build_agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "agent-doc")
        .call()
        .context("Failed to fetch latest release from GitHub")?;
    let body: Value = resp
        .into_body()
        .read_json()
        .context("Failed to parse release JSON")?;
    Ok(body)
}

fn fetch_releases() -> Result<Vec<Value>> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases");
    let resp = build_agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "agent-doc")
        .call()
        .context("Failed to fetch releases from GitHub")?;
    let body: Vec<Value> = resp
        .into_body()
        .read_json()
        .context("Failed to parse releases JSON")?;
    Ok(body)
}

fn find_asset<'a>(release: &'a Value, prefix: &str, ext: &str) -> Result<(&'a str, &'a str)> {
    let assets = release["assets"]
        .as_array()
        .context("No assets in release")?;

    // Prefer signed variant
    let signed_name = format!("{prefix}-signed.{ext}");
    if let Some(asset) = assets
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n == signed_name))
    {
        let name = asset["name"].as_str().unwrap();
        let url = asset["browser_download_url"]
            .as_str()
            .context("No download URL for asset")?;
        return Ok((name, url));
    }

    // Fall back to any matching asset
    if let Some(asset) = assets.iter().find(|a| {
        a["name"]
            .as_str()
            .is_some_and(|n| n.starts_with(prefix) && n.ends_with(&format!(".{ext}")))
    }) {
        let name = asset["name"].as_str().unwrap();
        let url = asset["browser_download_url"]
            .as_str()
            .context("No download URL for asset")?;
        return Ok((name, url));
    }

    bail!("No {prefix}*.{ext} asset found in latest release");
}

fn has_asset(release: &Value, prefix: &str, ext: &str) -> bool {
    find_asset(release, prefix, ext).is_ok()
}

fn fetch_release_for_asset(prefix: &str, ext: &str) -> Result<Value> {
    let latest = fetch_latest_release()?;
    if has_asset(&latest, prefix, ext) {
        return Ok(latest);
    }

    let latest_tag = release_version(&latest).to_string();
    eprintln!(
        "Latest release {latest_tag} has no {prefix}*.{ext} asset; checking older releases..."
    );

    let releases = fetch_releases()?;
    for release in releases {
        if has_asset(&release, prefix, ext) {
            return Ok(release);
        }
    }

    bail!("No {prefix}*.{ext} asset found in latest release or any recent GitHub release");
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

fn jetbrains_plugin_dirs() -> Vec<PathBuf> {
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return vec![],
    };

    let search_roots = if cfg!(target_os = "macos") {
        vec![home.join("Library/Application Support/JetBrains")]
    } else {
        vec![
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("JetBrains"),
        ]
    };

    jetbrains_plugin_dirs_in_roots(&search_roots)
}

fn jetbrains_plugin_dirs_in_roots(search_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in search_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                if !path.is_dir() || !is_jetbrains_ide_data_dir(&name.to_string_lossy()) {
                    continue;
                }
                let plugins = path.join("plugins");
                if plugins.is_dir() {
                    dirs.push(plugins);
                } else {
                    // Modern IDEs commonly expose the product-version data root itself as
                    // `idea.plugins.path`.
                    dirs.push(path);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn is_jetbrains_ide_data_dir(name: &str) -> bool {
    const PRODUCTS: &[&str] = &[
        "Aqua",
        "CLion",
        "DataGrip",
        "GoLand",
        "IdeaIC",
        "IntelliJIdea",
        "PhpStorm",
        "PyCharm",
        "Rider",
        "RubyMine",
        "RustRover",
        "WebStorm",
    ];
    name.chars().any(|ch| ch.is_ascii_digit())
        && PRODUCTS.iter().any(|product| name.starts_with(product))
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

fn install_jetbrains_into(release: &Value, target_dir: &Path) -> Result<()> {
    let (asset_name, url) = find_asset(release, "agent-doc-jetbrains", "zip")?;
    eprintln!("Found asset: {asset_name}");
    fs::create_dir_all(target_dir).context("Failed to create JetBrains plugins directory")?;

    let tmp = download_to_temp(url)?;

    // Remove old installation if present
    let dest = target_dir.join("agent-doc-jetbrains");
    if dest.exists() {
        fs::remove_dir_all(&dest).context("Failed to remove old plugin")?;
    }

    // Extract zip
    let file = fs::File::open(tmp.path()).context("Failed to open downloaded zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let out_path = target_dir.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            io::copy(&mut entry, &mut outfile)?;
        }
    }

    let version = release_version(release);
    eprintln!("Plugin installed ({version}) to {}", target_dir.display());
    eprintln!("Restart your IDE to activate.");
    Ok(())
}

fn install_jetbrains(release: &Value, plugins_dir: Option<&Path>) -> Result<()> {
    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
    install_jetbrains_into(release, &target_dir)
}

// --- VS Code ---

fn detect_code_cmd() -> &'static str {
    // Check for cursor first, then codium, then code
    for cmd in ["cursor", "codium", "code"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return cmd;
        }
    }
    "code"
}

fn install_vscode(release: &Value) -> Result<()> {
    let (asset_name, url) = find_asset(release, "agent-doc", "vsix")?;
    eprintln!("Found asset: {asset_name}");

    let tmp = download_to_temp(url)?;
    let code = detect_code_cmd();

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
    match editor {
        "jetbrains" | "jb" | "idea" => {
            let release = fetch_release_for_asset("agent-doc-jetbrains", "zip")?;
            install_jetbrains(&release, plugins_dir)
        }
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains installs");
            }
            let release = fetch_release_for_asset("agent-doc", "vsix")?;
            install_vscode(&release)
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    }
}

pub fn install_local(editor: &str) -> Result<()> {
    install_local_with_plugins_dir(editor, None)
}

pub fn install_local_with_plugins_dir(editor: &str, plugins_dir: Option<&Path>) -> Result<()> {
    match editor {
        "jetbrains" | "jb" | "idea" => install_jetbrains_local(plugins_dir),
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains installs");
            }
            install_vscode_local()
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    }
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

fn install_jetbrains_local(plugins_dir: Option<&Path>) -> Result<()> {
    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
    let zip_path = local_jetbrains_zip()?;
    install_jetbrains_local_zip_into(&zip_path, &target_dir)?;

    eprintln!("Plugin installed to {}", target_dir.display());
    eprintln!("Restart your IDE to activate.");
    Ok(())
}

/// Install the current local JetBrains build into every IDE that already has
/// agent-doc installed. This is the non-interactive coherence path used by
/// `make install`: it updates all existing installations instead of choosing
/// one arbitrary IDE and silently leaving the others stale.
pub fn install_local_all_existing(editor: &str) -> Result<()> {
    match editor {
        "jetbrains" | "jb" | "idea" => install_jetbrains_local_all_existing(),
        _ => bail!("--all-installed is currently supported only for JetBrains installs"),
    }
}

fn install_jetbrains_local_all_existing() -> Result<()> {
    let targets = existing_jetbrains_agent_doc_dirs(&jetbrains_plugin_dirs());
    if targets.is_empty() {
        bail!(
            "No existing JetBrains agent-doc installation found; install one explicitly with \
             `agent-doc plugin install jetbrains --local --plugins-dir <PATH>`"
        );
    }

    let zip_path = local_jetbrains_zip()?;
    for target_dir in &targets {
        install_jetbrains_local_zip_into(&zip_path, target_dir)?;
        eprintln!("Plugin installed to {}", target_dir.display());
    }
    eprintln!(
        "Installed the matching local JetBrains package into {} existing IDE installation(s).",
        targets.len()
    );
    eprintln!("Restart running IDEs to activate the package generation.");
    Ok(())
}

fn existing_jetbrains_agent_doc_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.iter()
        .filter(|dir| installed_jetbrains_plugin_version(dir).is_some())
        .cloned()
        .collect()
}

fn local_jetbrains_zip() -> Result<PathBuf> {
    let project_root = find_local_build_dir()?;
    let dist_dir = project_root.join("editors/jetbrains/build/distributions");

    // Pick the newest built version overall; prefer signed only within the same version.
    find_best_local_zip(&dist_dir).with_context(|| {
        format!(
            "No agent-doc-jetbrains*.zip found in {}",
            dist_dir.display()
        )
    })
}

fn local_jetbrains_zip_version(zip_path: &Path) -> Result<String> {
    let name = zip_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Local JetBrains build has a non-UTF-8 filename")?;
    let base = name
        .strip_prefix("agent-doc-jetbrains-")
        .context("Local JetBrains build has an unexpected filename")?;
    base.strip_suffix("-signed.zip")
        .or_else(|| base.strip_suffix(".zip"))
        .map(str::to_owned)
        .context("Local JetBrains build has an unexpected filename")
}

fn install_jetbrains_local_zip_into(zip_path: &Path, target_dir: &Path) -> Result<()> {
    let expected_version = local_jetbrains_zip_version(zip_path)?;
    eprintln!("Installing from local build: {}", zip_path.display());
    fs::create_dir_all(target_dir).context("Failed to create JetBrains plugins directory")?;

    // Remove old installation if present
    let dest = target_dir.join("agent-doc-jetbrains");
    if dest.exists() {
        fs::remove_dir_all(&dest).context("Failed to remove old plugin")?;
    }

    // Extract zip
    let file = fs::File::open(zip_path).context("Failed to open zip")?;
    let mut archive = zip::ZipArchive::new(file).context("Failed to read zip archive")?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let out_path = target_dir.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            io::copy(&mut entry, &mut outfile)?;
        }
    }

    let installed_version = installed_jetbrains_plugin_version(target_dir).with_context(|| {
        format!(
            "JetBrains package verification failed in {}: no agent-doc plugin jar found",
            target_dir.display()
        )
    })?;
    if installed_version != expected_version {
        bail!(
            "JetBrains package verification failed in {}: built {}, installed {}",
            target_dir.display(),
            expected_version,
            installed_version
        );
    }
    Ok(())
}

fn install_vscode_local() -> Result<()> {
    let project_root = find_local_build_dir()?;
    let dist_dir = project_root.join("editors/vscode");
    let vsix = find_local_vscode_vsix(&dist_dir)?;

    eprintln!("Installing from local build: {}", vsix.display());

    let code = detect_code_cmd();
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
    let lib_dir = target_dir.join("agent-doc-jetbrains/lib");
    fs::read_dir(lib_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name
                .strip_prefix("agent-doc-jetbrains-")?
                .strip_suffix(".jar")?;
            let key: Vec<u32> = version
                .split('.')
                .map(str::parse::<u32>)
                .collect::<std::result::Result<_, _>>()
                .ok()?;
            Some((key, version.to_owned()))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, version)| version)
}

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
    match editor {
        "jetbrains" | "jb" | "idea" => {
            let dirs = jetbrains_plugin_dirs();
            let target_dir = choose_plugins_dir(&dirs, plugins_dir)?;
            let release = fetch_release_for_asset("agent-doc-jetbrains", "zip")?;
            let version = release_version(&release);
            if installed_jetbrains_plugin_version(&target_dir).as_deref()
                == Some(version.trim_start_matches('v'))
            {
                eprintln!("JetBrains plugin is already at {version}.");
                return Ok(());
            }
            install_jetbrains_into(&release, &target_dir)
        }
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            if plugins_dir.is_some() {
                bail!("--plugins-dir is only supported for JetBrains updates");
            }
            let release = fetch_release_for_asset("agent-doc", "vsix")?;
            // VS Code/Cursor handles update-in-place via --install-extension
            install_vscode(&release)
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        choose_plugins_dir_with_interactivity, existing_jetbrains_agent_doc_dirs, find_asset,
        find_best_local_zip, find_local_vscode_vsix, find_local_zip, has_asset,
        installed_jetbrains_plugin_version, is_jetbrains_ide_data_dir,
        jetbrains_plugin_dirs_in_roots, local_jetbrains_zip_version, release_version,
    };
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

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

    #[test]
    fn find_asset_prefers_signed_variant() {
        let release = json!({
            "tag_name": "v0.33.11",
            "assets": [
                {"name": "agent-doc-jetbrains-0.2.75.zip", "browser_download_url": "https://example.com/unsigned.zip"},
                {"name": "agent-doc-jetbrains-signed.zip", "browser_download_url": "https://example.com/signed.zip"}
            ]
        });

        let (name, url) = find_asset(&release, "agent-doc-jetbrains", "zip").unwrap();
        assert_eq!(name, "agent-doc-jetbrains-signed.zip");
        assert_eq!(url, "https://example.com/signed.zip");
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
}

pub fn list() -> Result<()> {
    let mut found = false;

    // JetBrains
    let dirs = jetbrains_plugin_dirs();
    for d in &dirs {
        if let Some(version) = installed_jetbrains_plugin_version(d) {
            println!("jetbrains  v{}  {}", version, d.display());
            found = true;
        }
    }

    // VS Code
    let code = detect_code_cmd();
    if let Ok(output) = std::process::Command::new(code)
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
