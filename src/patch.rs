use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Command;

use crate::{component, snapshot};

const COMPONENTS_FILENAME: &str = ".agent-doc/components.toml";

/// Component configuration from `.agent-doc/components.toml`.
#[derive(Debug, Deserialize, Default)]
struct ComponentConfig {
    /// Patch mode: "replace" (default), "append", "prepend"
    #[serde(default = "default_mode")]
    mode: String,
    /// Auto-prefix entries with ISO timestamp (for append/prepend modes)
    #[serde(default)]
    timestamp: bool,
    /// Auto-trim old entries in append/prepend modes (0 = unlimited)
    #[serde(default)]
    max_entries: usize,
    /// Shell command to run before patching (stdin: content, stdout: transformed)
    #[serde(default)]
    pre_patch: Option<String>,
    /// Shell command to run after patching (fire-and-forget)
    #[serde(default)]
    post_patch: Option<String>,
}

fn default_mode() -> String {
    "replace".to_string()
}

/// Find the project root by walking up from a file path looking for `.agent-doc/`.
fn find_project_root(file: &Path) -> Option<std::path::PathBuf> {
    let canonical = file.canonicalize().ok()?;
    let mut dir = canonical.parent()?;
    loop {
        if dir.join(".agent-doc").is_dir() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

/// Load component configs from `.agent-doc/components.toml` relative to project root.
fn load_configs(file: &Path) -> Result<HashMap<String, ComponentConfig>> {
    let root = match find_project_root(file) {
        Some(r) => r,
        None => return Ok(HashMap::new()),
    };
    let path = root.join(COMPONENTS_FILENAME);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let configs: HashMap<String, ComponentConfig> = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(configs)
}

/// Replace content in a named component.
///
/// If `content` is None, reads replacement content from stdin.
/// Applies component config (mode, timestamp, max_entries) and shell hooks.
pub fn run(file: &Path, component_name: &str, content: Option<&str>) -> Result<()> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }

    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let comp = components
        .iter()
        .find(|c| c.name == component_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "component '{}' not found in {}",
                component_name,
                file.display()
            )
        })?;

    let configs = load_configs(file).unwrap_or_default();
    let config = configs.get(component_name);

    let mut replacement = match content {
        Some(text) => text.to_string(),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("failed to read from stdin")?;
            buf
        }
    };

    // Run pre_patch hook (transforms content)
    if let Some(script) = config.and_then(|c| c.pre_patch.as_ref()) {
        replacement = run_pre_hook(script, component_name, file, &replacement)?;
    }

    // Apply mode
    let mode = config.map(|c| c.mode.as_str()).unwrap_or("replace");
    let timestamp = config.is_some_and(|c| c.timestamp);
    let max_entries = config.map(|c| c.max_entries).unwrap_or(0);

    let final_content = match mode {
        "append" => {
            let existing = comp.content(&doc);
            let entry = if timestamp {
                format!("[{}] {}", iso_now(), replacement)
            } else {
                replacement
            };
            let mut combined = format!("{}{}", existing, entry);
            if max_entries > 0 {
                combined = trim_entries(&combined, max_entries);
            }
            combined
        }
        "prepend" => {
            let existing = comp.content(&doc);
            let entry = if timestamp {
                format!("[{}] {}", iso_now(), replacement)
            } else {
                replacement
            };
            let mut combined = format!("{}{}", entry, existing);
            if max_entries > 0 {
                combined = trim_entries(&combined, max_entries);
            }
            combined
        }
        _ => {
            // "replace" (default)
            if timestamp {
                format!("[{}] {}", iso_now(), replacement)
            } else {
                replacement
            }
        }
    };

    let new_doc = comp.replace_content(&doc, &final_content);

    std::fs::write(file, &new_doc)
        .with_context(|| format!("failed to write {}", file.display()))?;

    // Save snapshot relative to project root (not CWD) for thread safety
    let snap_rel = snapshot::path_for(file)?;
    if let Some(root) = find_project_root(file) {
        let snap_abs = root.join(&snap_rel);
        if let Some(parent) = snap_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create snapshot dir for {}", file.display()))?;
        }
        std::fs::write(&snap_abs, &new_doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
    } else {
        // Fallback to CWD-relative (original behavior)
        snapshot::save(file, &new_doc)
            .with_context(|| format!("failed to update snapshot for {}", file.display()))?;
    }

    // Run post_patch hook (fire-and-forget)
    if let Some(script) = config.and_then(|c| c.post_patch.as_ref()) {
        run_post_hook(script, component_name, file);
    }

    eprintln!(
        "Patched component '{}' in {} (mode: {})",
        component_name,
        file.display(),
        mode
    );
    Ok(())
}

/// Run a pre_patch hook. Passes content on stdin, returns transformed content from stdout.
fn run_pre_hook(script: &str, component_name: &str, file: &Path, content: &str) -> Result<String> {
    let mut child = Command::new("sh")
        .args(["-c", script])
        .env("COMPONENT", component_name)
        .env("FILE", file.to_string_lossy().as_ref())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to run pre_patch hook: {}", script))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(content.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "pre_patch hook failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            script
        );
    }
    String::from_utf8(output.stdout)
        .context("pre_patch hook produced invalid UTF-8")
}

/// Run a post_patch hook (fire-and-forget).
fn run_post_hook(script: &str, component_name: &str, file: &Path) {
    let result = Command::new("sh")
        .args(["-c", script])
        .env("COMPONENT", component_name)
        .env("FILE", file.to_string_lossy().as_ref())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();
    if let Err(e) = result {
        eprintln!("Warning: post_patch hook failed: {}", e);
    }
}

/// Trim to the last `max` non-empty lines.
fn trim_entries(content: &str, max: usize) -> String {
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() <= max {
        return content.to_string();
    }
    let trimmed: Vec<&str> = lines[lines.len() - max..].to_vec();
    let mut result = trimmed.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Simple UTC timestamp.
fn iso_now() -> String {
    let output = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a temp dir with `.agent-doc/snapshots/` so `find_project_root` and
    /// `snapshot::save` work without `set_current_dir`.
    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    fn write_doc(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn write_config(dir: &Path, content: &str) {
        std::fs::write(dir.join(COMPONENTS_FILENAME), content).unwrap();
    }

    #[test]
    fn replace_component() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "# Dashboard\n\n<!-- agent:status -->\nold content\n<!-- /agent:status -->\n\nFooter\n",
        );

        run(&doc, "status", Some("new content\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("new content"));
        assert!(!result.contains("old content"));
        assert!(result.contains("<!-- agent:status -->"));
        assert!(result.contains("<!-- /agent:status -->"));
        assert!(result.contains("Footer"));
    }

    #[test]
    fn preserve_surrounding() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "BEFORE\n<!-- agent:x -->\nreplace me\n<!-- /agent:x -->\nAFTER\n",
        );

        run(&doc, "x", Some("replaced\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.starts_with("BEFORE\n"));
        assert!(result.ends_with("AFTER\n"));
        assert!(result.contains("replaced"));
    }

    #[test]
    fn component_not_found_error() {
        let dir = setup_project();
        let doc = write_doc(dir.path(), "test.md", "# No components\n");

        let err = run(&doc, "missing", Some("x")).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn file_not_found_error() {
        let err = run(Path::new("/nonexistent/file.md"), "s", Some("x")).unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn snapshot_updated_after_patch() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:s -->\nold\n<!-- /agent:s -->\n",
        );

        run(&doc, "s", Some("new\n")).unwrap();

        // Snapshot should be readable from the project's .agent-doc/snapshots/
        let snap_path = dir.path().join(snapshot::path_for(&doc).unwrap());
        let snap = std::fs::read_to_string(snap_path).unwrap();
        assert!(snap.contains("new"));
        assert!(!snap.contains("old"));
    }

    #[test]
    fn append_mode() {
        let dir = setup_project();
        write_config(dir.path(), "[log]\nmode = \"append\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:log -->\nentry1\n<!-- /agent:log -->\n",
        );

        run(&doc, "log", Some("entry2\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("entry1"));
        assert!(result.contains("entry2"));
    }

    #[test]
    fn prepend_mode() {
        let dir = setup_project();
        write_config(dir.path(), "[log]\nmode = \"prepend\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:log -->\nold\n<!-- /agent:log -->\n",
        );

        run(&doc, "log", Some("new\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        let new_pos = result.find("new").unwrap();
        let old_pos = result.find("old").unwrap();
        assert!(new_pos < old_pos);
    }

    #[test]
    fn trim_entries_limits() {
        let content = "line1\nline2\nline3\nline4\nline5\n";
        let trimmed = trim_entries(content, 3);
        assert!(!trimmed.contains("line1"));
        assert!(!trimmed.contains("line2"));
        assert!(trimmed.contains("line3"));
        assert!(trimmed.contains("line4"));
        assert!(trimmed.contains("line5"));
    }

    #[test]
    fn trim_entries_noop_when_under_limit() {
        let content = "line1\nline2\n";
        assert_eq!(trim_entries(content, 5), content);
    }

    #[test]
    fn no_config_defaults_to_replace() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:x -->\nold\n<!-- /agent:x -->\n",
        );

        run(&doc, "x", Some("new\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("new"));
        assert!(!result.contains("old"));
    }

    #[test]
    fn pre_patch_hook_transforms_content() {
        let dir = setup_project();
        write_config(dir.path(), "[x]\npre_patch = \"tr a-z A-Z\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:x -->\nold\n<!-- /agent:x -->\n",
        );

        run(&doc, "x", Some("hello world\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("HELLO WORLD"));
    }

    #[test]
    fn post_patch_hook_runs() {
        let dir = setup_project();
        let marker = dir.path().join("hook-ran");
        write_config(
            dir.path(),
            &format!(
                "[x]\npost_patch = \"touch {}\"\n",
                marker.to_string_lossy()
            ),
        );

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:x -->\nold\n<!-- /agent:x -->\n",
        );

        run(&doc, "x", Some("new\n")).unwrap();

        assert!(marker.exists(), "post_patch hook should have created marker file");
    }
}
