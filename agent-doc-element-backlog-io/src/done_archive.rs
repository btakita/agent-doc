use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Archive reaped pending items to `agent:done`.
///
/// When the archive component is absent, create a visible
/// `## Completed / Reaped` section after the tracked work components before
/// appending the entries. Returns `Some(new_content)` when archival happened,
/// `None` only when there is no tracked-work anchor to place the archive.
pub fn archive_pending_done(
    file: &Path,
    content: &str,
    removed: &[agent_doc_element_backlog::backlog::PendingItem],
) -> Result<Option<String>> {
    if removed.is_empty() {
        return Ok(None);
    }
    let mut content_with_archive = content.to_string();
    let components = agent_doc_element::element::parse(&content_with_archive)?;
    if !components
        .iter()
        .any(|c| agent_doc_element::element::is_backlog_done_component(&c.name))
    {
        content_with_archive =
            agent_doc_element_done::insert_done_component_after_tracked_work(&content_with_archive)
                .context("failed to insert agent:done component")?;
    }
    let components = agent_doc_element::element::parse(&content_with_archive)?;
    let archive = components
        .into_iter()
        .find(|c| agent_doc_element::element::is_backlog_done_component(&c.name))
        .context("document is missing agent:done component")?;
    let existing_body = &content_with_archive[archive.open_end..archive.close_start];

    let today = agent_doc_log_time::current_local_date_ymd();

    if let Some(archive_path) = archive.attrs.get("archive") {
        let target = resolve_done_archive_target(file, archive_path)?;
        append_external_done_archive(&target, &today, removed)?;
        let pointer = format!("\n<!-- completed work archived in {} -->\n", archive_path);
        let new_body = if existing_body.trim().is_empty() || existing_body.trim() == pointer.trim()
        {
            pointer
        } else {
            existing_body.to_string()
        };
        return Ok(Some(
            archive.replace_content(&content_with_archive, &new_body),
        ));
    }

    let mut new_body = existing_body.to_string();
    if !new_body.is_empty() && !new_body.ends_with('\n') {
        new_body.push('\n');
    }
    for item in removed {
        new_body.push_str(&agent_doc_element_done::render_done_archive_entry(
            &today,
            &item.id,
            &item.text,
            &item.continuation,
        ));
    }

    Ok(Some(
        archive.replace_content(&content_with_archive, &new_body),
    ))
}

pub fn external_done_archive_ids(file: &Path, content: &str) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    let components = agent_doc_element::element::parse(content)?;
    for archive in components
        .iter()
        .filter(|c| agent_doc_element::element::is_backlog_done_component(&c.name))
    {
        let Some(archive_path) = archive.attrs.get("archive") else {
            continue;
        };
        let target = resolve_done_archive_target(file, archive_path)?;
        let archive_content = match std::fs::read_to_string(&target) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to read done archive {}", target.display()));
            }
        };
        ids.extend(
            agent_doc_element_backlog::backlog::extract_pending_ids_from_text(&archive_content),
        );
    }
    Ok(ids)
}

fn resolve_done_archive_target(file: &Path, archive_path: &str) -> Result<PathBuf> {
    if archive_path.trim().is_empty() {
        bail!("agent:done archive= must not be empty");
    }
    if !archive_path.ends_with(".done.md") {
        bail!(
            "agent:done archive={} must point to a .done.md file",
            archive_path
        );
    }
    let relative = Path::new(archive_path);
    if relative.is_absolute() {
        bail!(
            "agent:done archive={} must be repo-relative, not absolute",
            archive_path
        );
    }
    if relative.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!(
            "agent:done archive={} must not escape the repository",
            archive_path
        );
    }

    let canonical_file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root =
        agent_doc_project_root_io::project_root_containing(&canonical_file).with_context(|| {
            format!(
                "failed to find repository root for done archive resolution from {}",
                file.display()
            )
        })?;
    let target = root.join(relative);
    if let Ok(canonical_target) = target.canonicalize() {
        if !canonical_target.starts_with(&root) {
            bail!(
                "agent:done archive={} resolves outside the repository",
                archive_path
            );
        }
    } else if let Some(parent) = target.parent()
        && let Ok(canonical_parent) = parent.canonicalize()
        && !canonical_parent.starts_with(&root)
    {
        bail!(
            "agent:done archive={} resolves outside the repository",
            archive_path
        );
    }
    Ok(target)
}

fn append_external_done_archive(
    target: &Path,
    today: &str,
    removed: &[agent_doc_element_backlog::backlog::PendingItem],
) -> Result<()> {
    let mut existing = match std::fs::read_to_string(target) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            "# Agent Doc Completed Work\n\n".to_string()
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read done archive {}", target.display()));
        }
    };
    let mut changed = false;
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
        changed = true;
    }
    for item in removed {
        let first_line = format!("- {} [#{}] {}", today, item.id, item.text);
        if existing.lines().any(|line| line == first_line) {
            continue;
        }
        existing.push_str(&agent_doc_element_done::render_done_archive_entry(
            today,
            &item.id,
            &item.text,
            &item.continuation,
        ));
        changed = true;
    }
    if changed || !target.exists() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create done archive directory {}",
                    parent.display()
                )
            })?;
        }
        agent_doc_fs::write_atomic(target, existing.as_bytes())
            .with_context(|| format!("failed to write done archive {}", target.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pull the `archive` attribute out of a real document the way production
    /// does — parse the marker, read `attrs` — so the test exercises the same
    /// path that broke.
    fn archive_attr(marker: &str) -> String {
        // Markers must sit at column 0 — an indented marker is not recognised.
        let content = format!(
            "## Completed / Reaped\n\n<!-- agent:done {marker} -->\n<!-- /agent:done -->\n"
        );
        let components = agent_doc_element::element::parse(&content).expect("parse");
        components
            .into_iter()
            .find(|c| agent_doc_element::element::is_backlog_done_component(&c.name))
            .expect("agent:done component")
            .attrs
            .get("archive")
            .expect("archive attribute")
            .clone()
    }

    /// The end-to-end regression.
    ///
    /// agent-doc's own compact wrote `archive="tasks/software/lazily.done.md"`.
    /// The attribute value kept its quotes, so the `.done.md` suffix check could
    /// never match, and every preflight and session-check on that document died
    /// with `must point to a .done.md file`. The document was unusable until the
    /// quotes were hand-stripped out of the marker.
    #[test]
    fn quoted_archive_attribute_resolves() {
        let quoted = archive_attr("archive=\"tasks/software/lazily.done.md\"");
        assert_eq!(
            quoted, "tasks/software/lazily.done.md",
            "the parsed attribute must not carry its quotes"
        );

        // The resolver is what actually bailed. Run it inside this repo, where a
        // project root exists.
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/done_archive.rs");
        resolve_done_archive_target(&here, &quoted)
            .expect("a quoted archive attribute must resolve, not fail the document");
    }

    /// Quoted and unquoted spellings must behave identically — that they did not
    /// is the entire defect.
    #[test]
    fn quoted_and_unquoted_archive_agree() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/done_archive.rs");

        let quoted = archive_attr("archive=\"tasks/x.done.md\"");
        let bare = archive_attr("archive=tasks/x.done.md");
        assert_eq!(quoted, bare, "both spellings must parse to the same value");

        assert_eq!(
            resolve_done_archive_target(&here, &quoted).expect("quoted resolves"),
            resolve_done_archive_target(&here, &bare).expect("bare resolves"),
            "both spellings must resolve to the same archive path"
        );
    }

    /// The suffix check still rejects a genuinely wrong path — the fix must not
    /// have widened it into accepting anything.
    #[test]
    fn non_done_md_archive_is_still_rejected() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/done_archive.rs");
        for attr in ["archive=\"tasks/x.md\"", "archive=tasks/x.md"] {
            let value = archive_attr(attr);
            assert!(
                resolve_done_archive_target(&here, &value).is_err(),
                "{attr} must still be rejected: only .done.md is a valid archive"
            );
        }
    }
}
