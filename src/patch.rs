//! # Module: patch
//!
//! ## Spec
//! - Replaces content in a named `<!-- agent:name -->` component within a markdown document by default.
//! - `--mode append|prepend` is an explicit override for callers that intentionally want cumulative mutation.
//! - Component config still contributes `timestamp`, `max_entries`, `pre_patch`, and `post_patch`, but the
//!   component's configured `patch=` mode no longer changes `agent-doc patch` default behavior.
//! - `append` mode concatenates new content after existing; `prepend` inserts new content before existing.
//! - Optional `timestamp: true` in component config prefixes each entry with an ISO-8601 UTC timestamp.
//! - Optional `max_entries` in component config trims to the last N non-empty lines after append/prepend.
//! - `pre_patch` shell hook: content piped to stdin, transformed stdout replaces the replacement string before writing. Receives `COMPONENT` and `FILE` env vars.
//! - `post_patch` shell hook: fire-and-forget after write, receives same env vars. Non-zero exit is logged as a warning only.
//! - After patching, the document is written through document authority and the typed ledger baseline is checkpointed.
//! - `run` reads replacement content from the `content` argument or stdin when `None`.
//! - A non-empty replacement is newline-terminated before the component close marker, including
//!   after append/prepend composition and hook transformation.
//!
//! ## Agentic Contracts
//! - `run(file, component_name, content)` — returns `Err` if the file is missing, the component is not found, or any hook fails.
//! - The typed baseline is always updated after a successful patch. Its filesystem snapshot sidecar is a downstream write-only crash effect.
//! - `pre_patch` hook failure (non-zero exit) aborts the patch and returns `Err`; no partial write occurs.
//! - `post_patch` hook failure never aborts the patch; stderr warning only.
//! - `trim_entries(content, max)` trims to the last `max` non-empty lines; returns content unchanged when under the limit.
//!
//! ## Evals
//! - replace_component: existing component + new content → old content replaced, surroundings preserved
//! - preserve_surrounding: content before and after component → unchanged after patch
//! - component_not_found: component name absent from doc → Err containing "not found"
//! - file_not_found: missing file path → Err containing "file not found"
//! - snapshot_updated_after_patch: after replace → snapshot file contains new content, not old
//! - append_mode_requires_explicit_override: append-mode component + bare patch → old content replaced
//! - explicit_append_mode: `--mode append` + second patch → both entries present in document
//! - explicit_prepend_mode: `--mode prepend` + new entry → new entry appears before existing
//! - replacement_without_trailing_newline: a one-line CLI argument remains structurally valid
//! - trim_entries_limits: 5-line content trimmed to 3 → oldest 2 lines removed
//! - pre_patch_hook_transforms: `pre_patch = "tr a-z A-Z"` → content uppercased before write
//! - post_patch_hook_runs: `post_patch = "touch <file>"` → marker file created after write

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Command;

use agent_doc_element::element;

use crate::PatchMode;
use agent_doc_frontmatter::project_config::ComponentConfig;
use agent_doc_project_config_io as project_config_io;
use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};

fn load_configs_with_context(
    file: &Path,
    rc: Option<&CycleContext>,
) -> Result<HashMap<String, ComponentConfig>> {
    if let Some(rc) = rc {
        return Ok(rc
            .project_config()
            .components
            .iter()
            .map(|(name, cfg)| (name.clone(), cfg.clone()))
            .collect());
    }
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            let cfg = project_config_io::load_project_from(&candidate);
            return Ok(cfg.components.into_iter().collect());
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    // Fall back to CWD-based resolution
    let proj_cfg = project_config_io::load_project();
    Ok(proj_cfg.components.into_iter().collect())
}

/// Patch a named component.
///
/// If `content` is None, reads replacement content from stdin.
/// Applies timestamp/max-entry hooks plus an explicit mode override.
pub fn run(
    file: &Path,
    component_name: &str,
    mode: PatchMode,
    content: Option<&str>,
) -> Result<()> {
    if !file.exists() {
        bail!("file not found: {}", file.display());
    }
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());

    let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "patch_command_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;

    let components = element::parse(&doc)
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

    let configs = load_configs_with_context(file, Some(&rc)).unwrap_or_default();
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

    let timestamp = config.is_some_and(|c| c.timestamp);
    let max_entries = config.map(|c| c.max_entries).unwrap_or(0);

    let mode_name = match mode {
        PatchMode::Replace => "replace",
        PatchMode::Append => "append",
        PatchMode::Prepend => "prepend",
    };

    let final_content = match mode {
        PatchMode::Append => {
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
        PatchMode::Prepend => {
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
        PatchMode::Replace => {
            if timestamp {
                format!("[{}] {}", iso_now(), replacement)
            } else {
                replacement
            }
        }
    };
    let final_content = terminate_component_content(final_content);

    let new_doc = comp.replace_content(&doc, &final_content);

    agent_doc_document_realtime_io::atomic_write_through_authority(file, &new_doc)
        .with_context(|| format!("failed to write {}", file.display()))?;

    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &new_doc,
        agent_doc_ops_log_io::log_op,
    )
    .with_context(|| format!("failed to checkpoint baseline for {}", file.display()))?;

    // Run post_patch hook (fire-and-forget)
    if let Some(script) = config.and_then(|c| c.post_patch.as_ref()) {
        run_post_hook(script, component_name, file);
    }

    eprintln!(
        "Patched component '{}' in {} (mode: {})",
        component_name,
        file.display(),
        mode_name
    );
    Ok(())
}

fn terminate_component_content(mut content: String) -> String {
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content
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
    String::from_utf8(output.stdout).context("pre_patch hook produced invalid UTF-8")
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
    /// focused snapshot IO work without `set_current_dir`.
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
        // Config lives at .agent-doc/config.toml with [components] section
        let config_path = dir.join(".agent-doc").join("config.toml");
        std::fs::write(config_path, content).unwrap();
    }

    #[test]
    fn replace_component() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "# Dashboard\n\n<!-- agent:status -->\nold content\n<!-- /agent:status -->\n\nFooter\n",
        );

        run(&doc, "status", PatchMode::Replace, Some("new content\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("new content"));
        assert!(!result.contains("old content"));
        assert!(result.contains("<!-- agent:status -->"));
        assert!(result.contains("<!-- /agent:status -->"));
        assert!(result.contains("Footer"));
    }

    #[test]
    fn replacement_without_trailing_newline_preserves_component_structure() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:queue -->\n- do truncated\n<!-- /agent:queue -->\n",
        );

        run(
            &doc,
            "queue",
            PatchMode::Replace,
            Some("- do complete prompt"),
        )
        .unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("- do complete prompt\n<!-- /agent:queue -->"));
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&result),
            None
        );
    }

    #[test]
    fn preserve_surrounding() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "BEFORE\n<!-- agent:x -->\nreplace me\n<!-- /agent:x -->\nAFTER\n",
        );

        run(&doc, "x", PatchMode::Replace, Some("replaced\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.starts_with("BEFORE\n"));
        assert!(result.ends_with("AFTER\n"));
        assert!(result.contains("replaced"));
    }

    #[test]
    fn component_not_found_error() {
        let dir = setup_project();
        let doc = write_doc(dir.path(), "test.md", "# No components\n");

        let err = run(&doc, "missing", PatchMode::Replace, Some("x")).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn file_not_found_error() {
        let err = run(
            Path::new("/nonexistent/file.md"),
            "s",
            PatchMode::Replace,
            Some("x"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn typed_baseline_drives_crash_projection_after_patch() {
        let dir = setup_project();
        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:s -->\nold\n<!-- /agent:s -->\n",
        );

        run(&doc, "s", PatchMode::Replace, Some("new\n")).unwrap();

        let baseline = agent_doc_snapshot_io::load_document_baseline(&doc)
            .unwrap()
            .expect("typed baseline");
        assert!(baseline.contains("new"));
        assert!(!baseline.contains("old"));
        let crash_projection =
            std::fs::read_to_string(agent_doc_fs::snapshot_path_for(&doc).unwrap()).unwrap();
        assert_eq!(crash_projection, baseline);
    }

    #[test]
    fn append_mode_requires_explicit_override() {
        let dir = setup_project();
        write_config(dir.path(), "[components.log]\npatch = \"append\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:log -->\nentry1\n<!-- /agent:log -->\n",
        );

        run(&doc, "log", PatchMode::Replace, Some("entry2\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("entry2"));
        assert!(!result.contains("entry1"));
    }

    #[test]
    fn explicit_append_mode() {
        let dir = setup_project();
        write_config(dir.path(), "[components.log]\npatch = \"append\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:log -->\nentry1\n<!-- /agent:log -->\n",
        );

        run(&doc, "log", PatchMode::Append, Some("entry2\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("entry1"));
        assert!(result.contains("entry2"));
    }

    #[test]
    fn explicit_prepend_mode() {
        let dir = setup_project();
        write_config(dir.path(), "[components.log]\npatch = \"prepend\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:log -->\nold\n<!-- /agent:log -->\n",
        );

        run(&doc, "log", PatchMode::Prepend, Some("new\n")).unwrap();

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

        run(&doc, "x", PatchMode::Replace, Some("new\n")).unwrap();

        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(result.contains("new"));
        assert!(!result.contains("old"));
    }

    #[test]
    fn pre_patch_hook_transforms_content() {
        let dir = setup_project();
        write_config(dir.path(), "[components.x]\npre_patch = \"tr a-z A-Z\"\n");

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:x -->\nold\n<!-- /agent:x -->\n",
        );

        run(&doc, "x", PatchMode::Replace, Some("hello world\n")).unwrap();

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
                "[components.x]\npost_patch = \"touch {}\"\n",
                marker.to_string_lossy()
            ),
        );

        let doc = write_doc(
            dir.path(),
            "test.md",
            "<!-- agent:x -->\nold\n<!-- /agent:x -->\n",
        );

        run(&doc, "x", PatchMode::Replace, Some("new\n")).unwrap();

        assert!(
            marker.exists(),
            "post_patch hook should have created marker file"
        );
    }
}
