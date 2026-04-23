//! # Module: plugin
//!
//! ## Spec
//! - Manages editor plugin lifecycle (install, update, list) for JetBrains IDEs and VS Code-family editors (VS Code, VSCodium, Cursor).
//! - `install(editor)` — fetches the latest GitHub Release for `btakita/agent-doc`, selects the appropriate asset (signed variant preferred), downloads it, and installs it.
//! - `install_local(editor)` — installs from a locally built artifact found by walking up from CWD to locate an `editors/` directory.
//! - `update(editor)` — for JetBrains, skips re-install if the installed plugin.xml version matches the latest release tag; for VS Code, always reinstalls (handled idempotently by the CLI).
//! - `list()` — scans JetBrains plugin directories for `agent-doc-jetbrains/META-INF/plugin.xml` and queries `code --list-extensions` for the VS Code extension; prints found entries to stdout.
//! - JetBrains plugin directories are discovered from OS-specific paths (`~/.local/share/JetBrains/*/plugins/` on Linux, `~/Library/Application Support/JetBrains/*/plugins/` on macOS). When multiple IDEs are found, the user is prompted interactively on stderr.
//! - VS Code CLI detection order: `cursor` → `codium` → `code` (first that succeeds `--version`).
//! - Asset selection: prefers `<prefix>-signed.<ext>`, falls back to any `<prefix>*.<ext>` match. For local JetBrains installs, prefers `-signed.zip` over `.zip`.
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

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::PathBuf;

const GITHUB_REPO: &str = "btakita/agent-doc";

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_read(std::time::Duration::from_secs(30))
        .timeout_write(std::time::Duration::from_secs(10))
        .build()
}

fn fetch_latest_release() -> Result<Value> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = build_agent()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "agent-doc")
        .call()
        .context("Failed to fetch latest release from GitHub")?;
    let body: Value = resp.into_json().context("Failed to parse release JSON")?;
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

fn download_to_temp(url: &str) -> Result<tempfile::NamedTempFile> {
    eprintln!("Downloading {url}");
    let resp = build_agent()
        .get(url)
        .set("User-Agent", "agent-doc")
        .call()
        .context("Download failed")?;
    let mut tmp = tempfile::NamedTempFile::new().context("Failed to create temp file")?;
    let mut bytes = Vec::new();
    resp.into_reader()
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
            home.join(".local/share/JetBrains"),
            home.join(".config/JetBrains"),
        ]
    };

    let mut dirs = Vec::new();
    for root in &search_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let plugins = path.join("plugins");
                if plugins.is_dir() {
                    dirs.push(plugins);
                } else if root.ends_with("JetBrains") && path.is_dir() {
                    // Some layouts put plugins directly in IDE dir
                    dirs.push(path);
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn choose_plugins_dir(dirs: &[PathBuf]) -> Result<&PathBuf> {
    if dirs.is_empty() {
        bail!(
            "No JetBrains IDE plugins directory found.\n\
             Expected locations:\n  \
             Linux: ~/.local/share/JetBrains/*/plugins/ or ~/.config/JetBrains/*/plugins/\n  \
             macOS: ~/Library/Application Support/JetBrains/*/plugins/"
        );
    }
    if dirs.len() == 1 {
        return Ok(&dirs[0]);
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
    Ok(&dirs[idx - 1])
}

fn install_jetbrains(release: &Value) -> Result<()> {
    let (asset_name, url) = find_asset(release, "agent-doc-jetbrains", "zip")?;
    eprintln!("Found asset: {asset_name}");

    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs)?;

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
    let release = fetch_latest_release()?;
    match editor {
        "jetbrains" | "jb" | "idea" => install_jetbrains(&release),
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => install_vscode(&release),
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    }
}

pub fn install_local(editor: &str) -> Result<()> {
    match editor {
        "jetbrains" | "jb" | "idea" => install_jetbrains_local(),
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => install_vscode_local(),
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

fn install_jetbrains_local() -> Result<()> {
    let project_root = find_local_build_dir()?;
    let dist_dir = project_root.join("editors/jetbrains/build/distributions");

    // Find the latest signed zip, fall back to unsigned
    let zip_path = find_local_zip(&dist_dir, true)
        .or_else(|| find_local_zip(&dist_dir, false))
        .with_context(|| {
            format!(
                "No agent-doc-jetbrains*.zip found in {}",
                dist_dir.display()
            )
        })?;

    eprintln!("Installing from local build: {}", zip_path.display());

    let dirs = jetbrains_plugin_dirs();
    let target_dir = choose_plugins_dir(&dirs)?;

    // Remove old installation if present
    let dest = target_dir.join("agent-doc-jetbrains");
    if dest.exists() {
        fs::remove_dir_all(&dest).context("Failed to remove old plugin")?;
    }

    // Extract zip
    let file = fs::File::open(&zip_path).context("Failed to open zip")?;
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

    eprintln!("Plugin installed to {}", target_dir.display());
    eprintln!("Restart your IDE to activate.");
    Ok(())
}

fn install_vscode_local() -> Result<()> {
    let project_root = find_local_build_dir()?;
    let dist_dir = project_root.join("editors/vscode");

    // Find .vsix file
    let vsix = fs::read_dir(&dist_dir)
        .context("Failed to read VS Code build directory")?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".vsix") {
                Some(e.path())
            } else {
                None
            }
        })
        .max_by_key(|p| p.metadata().ok().and_then(|m| m.modified().ok()))
        .with_context(|| format!("No .vsix file found in {}", dist_dir.display()))?;

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

fn find_local_zip(dist_dir: &std::path::Path, signed: bool) -> Option<PathBuf> {
    let entries = fs::read_dir(dist_dir).ok()?;
    let suffix = if signed { "-signed.zip" } else { ".zip" };

    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("agent-doc-jetbrains") && name.ends_with(suffix) {
                // If looking for unsigned, exclude signed
                if !signed && name.ends_with("-signed.zip") {
                    return None;
                }
                Some(e.path())
            } else {
                None
            }
        })
        // Pick the most recently modified
        .max_by_key(|p| p.metadata().ok().and_then(|m| m.modified().ok()))
}

pub fn update(editor: &str) -> Result<()> {
    let release = fetch_latest_release()?;
    let version = release_version(&release);

    match editor {
        "jetbrains" | "jb" | "idea" => {
            // Check if already installed at this version
            let dirs = jetbrains_plugin_dirs();
            for d in &dirs {
                let manifest = d.join("agent-doc-jetbrains/META-INF/plugin.xml");
                if manifest.exists()
                    && let Ok(content) = fs::read_to_string(&manifest)
                    && content.contains(&format!(
                        "<version>{}</version>",
                        version.trim_start_matches('v')
                    ))
                {
                    eprintln!("JetBrains plugin is already at {version}.");
                    return Ok(());
                }
            }
            install_jetbrains(&release)
        }
        "vscode" | "code" | "vscodium" | "codium" | "cursor" => {
            // VS Code/Cursor handles update-in-place via --install-extension
            install_vscode(&release)
        }
        _ => bail!("Unknown editor: {editor}. Supported: jetbrains, vscode, cursor"),
    }
}

pub fn list() -> Result<()> {
    let mut found = false;

    // JetBrains
    let dirs = jetbrains_plugin_dirs();
    for d in &dirs {
        let manifest = d.join("agent-doc-jetbrains/META-INF/plugin.xml");
        if manifest.exists() {
            let version = fs::read_to_string(&manifest)
                .ok()
                .and_then(|c| {
                    // Extract <version>...</version>
                    let start = c.find("<version>")? + 9;
                    let end = c[start..].find("</version>")? + start;
                    Some(c[start..end].to_string())
                })
                .unwrap_or_else(|| "unknown".into());
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
