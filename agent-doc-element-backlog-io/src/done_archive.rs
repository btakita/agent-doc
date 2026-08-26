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

/// Candidate `<stem>.done.md` archives for a document, walking up to the
/// project root.
///
/// Complements the `agent:done archive=` attribute rather than replacing it: a
/// document may declare an archive, sit beside one, or both. Callers take the
/// union, because missing an archive means reporting completed work as invented.
pub fn done_archive_candidates(file: &Path) -> Vec<PathBuf> {
    agent_doc_fs::done_archive_candidates(file)
}

/// Tracked-work ids recorded in this document's done archives.
///
/// `#coinedguardledgerasymmetry`: the single predicate both coined-id guards
/// read. They used to disagree — the `PreToolUse` guard ran a whole-text tag
/// scan over the archive while `session-check` read only entry ids — so the same
/// tag, the same document and the same archive produced "tracked" on one path
/// and "invented" on the other. That is how a verification probe passed for a
/// reason its author did not intend.
///
/// The narrow reading wins. An archive entry has the binary-owned shape
/// `- YYYY-MM-DD [#id] text`, and only that leading id names tracked work; a tag
/// merely *cited* in an entry's prose is a reference, not an item. Vouching for
/// citations launders a coined id into legitimacy forever, which is exactly the
/// rule `#hookhashanchortags` rejected when it chose curated instruction anchors
/// over "any id anyone typed". Anchors quoted in archived prose stay allowed
/// because `agent_doc_fs::instruction_surface_anchors` covers them by name.
///
/// Resolution is the UNION of both strategies the guards previously used —
/// declared `archive=` targets and sibling `<stem>.done.md` files walking up —
/// so unifying the predicate cannot lose an archive either guard used to find.
pub fn archived_tracked_ids(file: &Path, content: &str) -> Result<HashSet<String>> {
    let mut ids = external_done_archive_ids(file, content)?;
    for archive in done_archive_candidates(file) {
        if let Ok(archived) = std::fs::read_to_string(&archive) {
            ids.extend(
                agent_doc_element_backlog::backlog::extract_pending_ids_from_text(&archived),
            );
        }
    }
    Ok(ids)
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

/// Remove every archived entry whose own leading id matches `id` and recover
/// the newest matching item as an open tracked-work item.
///
/// Archive entries have the binary-owned shape
/// `- YYYY-MM-DD [#id] text`; indented continuation lines belong to that entry.
/// Prose citations and ids in continuations are not identities. Removing all
/// same-id archive entries prevents a later done-archive scan from immediately
/// classifying the explicitly reopened item as completed again.
pub fn take_done_archive_item(
    archive: &str,
    id: &str,
) -> Result<(String, agent_doc_element_backlog::backlog::PendingItem)> {
    let id = agent_doc_element_backlog::backlog::normalize_pending_id(id);
    anyhow::ensure!(!id.is_empty(), "reopened done id must not be empty");

    #[derive(Debug)]
    struct Entry {
        start: usize,
        first_line_end: usize,
        end: usize,
        id: String,
        text: String,
    }

    fn entry_header(line: &str) -> Option<(String, String)> {
        if !(line.starts_with("- ") || line.starts_with("* ")) {
            return None;
        }
        let marker = line.find("[#")?;
        let after = &line[marker + 2..];
        let close = after.find(']')?;
        let id = &after[..close];
        if !agent_doc_element_backlog::backlog::is_valid_pending_id(id) {
            return None;
        }
        Some((
            id.to_ascii_lowercase(),
            after[close + 1..].trim().to_string(),
        ))
    }

    let mut headers = Vec::new();
    let mut offset = 0usize;
    for raw in archive.split_inclusive('\n') {
        let without_newline = raw.strip_suffix('\n').unwrap_or(raw);
        let line = without_newline
            .strip_suffix('\r')
            .unwrap_or(without_newline);
        if let Some((entry_id, text)) = entry_header(line) {
            headers.push((offset, offset + raw.len(), entry_id, text));
        }
        offset += raw.len();
    }

    let entries = headers
        .iter()
        .enumerate()
        .map(|(index, (start, first_line_end, entry_id, text))| Entry {
            start: *start,
            first_line_end: *first_line_end,
            end: headers
                .get(index + 1)
                .map(|(next_start, ..)| *next_start)
                .unwrap_or(archive.len()),
            id: entry_id.clone(),
            text: text.clone(),
        })
        .collect::<Vec<_>>();

    let mut reopened = None;
    let mut rewritten = String::with_capacity(archive.len());
    let mut cursor = 0usize;
    for entry in entries {
        if entry.id != id {
            continue;
        }
        rewritten.push_str(&archive[cursor..entry.start]);
        cursor = entry.end;
        reopened = Some(agent_doc_element_backlog::backlog::PendingItem {
            marker: agent_doc_element_backlog::backlog::PendingListMarker::Bullet,
            id: entry.id,
            state: agent_doc_element_backlog::backlog::PendingState::Open,
            gate_type: None,
            in_progress: false,
            text: entry.text,
            continuation: archive[entry.first_line_end..entry.end].to_string(),
        });
    }
    rewritten.push_str(&archive[cursor..]);

    let item = reopened.with_context(|| format!("id not found in done archive: {id}"))?;
    Ok((rewritten, item))
}

/// Resolve the binary-owned external done archive configured by the document.
///
/// Commit closeout uses this typed path to keep the session document and the
/// archive mutation in the same private-index transaction. Returning `None`
/// means the document uses its inline `agent:done` body instead.
pub fn configured_external_done_archive(file: &Path, content: &str) -> Result<Option<PathBuf>> {
    let components = agent_doc_element::element::parse(content)?;
    let Some(archive_path) = components
        .into_iter()
        .find(|component| agent_doc_element::element::is_backlog_done_component(&component.name))
        .and_then(|component| component.attrs.get("archive").cloned())
    else {
        return Ok(None);
    };
    resolve_done_archive_target(file, &archive_path).map(Some)
}

pub(crate) fn resolve_done_archive_target(file: &Path, archive_path: &str) -> Result<PathBuf> {
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
    let deferred = crate::backlog_cmd::deferred_raw_content(target);
    let mut existing = match deferred {
        Some(content) => content,
        None => match std::fs::read_to_string(target) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                "# Agent Doc Completed Work\n\n".to_string()
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to read done archive {}", target.display()));
            }
        },
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
        if crate::backlog_cmd::stage_raw_write(target, existing.clone()) {
            return Ok(());
        }
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

    #[test]
    fn take_done_archive_item_removes_all_same_id_entries_and_preserves_continuation() {
        let archive = concat!(
            "# Agent Doc Completed Work\n\n",
            "- 2026-07-01 [#keep] keep this\n",
            "- 2026-07-02 [#reopen] original text\n",
            "  proof line\n",
            "- 2026-07-03 [#reopen] corrected text\n",
            "  latest proof\n",
        );
        let (rewritten, item) = take_done_archive_item(archive, "#REOPEN").unwrap();
        assert!(rewritten.contains("[#keep] keep this"));
        assert!(!rewritten.contains("[#reopen]"));
        assert_eq!(item.id, "reopen");
        assert_eq!(item.text, "corrected text");
        assert_eq!(item.continuation, "  latest proof\n");
        assert_eq!(
            item.state,
            agent_doc_element_backlog::backlog::PendingState::Open
        );
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

#[cfg(test)]
mod archived_tracked_ids_tests {
    use super::*;

    fn project(archive_body: &str, doc_body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/bugs.md");
        std::fs::write(&doc, doc_body).unwrap();
        std::fs::write(dir.path().join("tasks/bugs.done.md"), archive_body).unwrap();
        (dir, doc)
    }

    /// The leading id of an archive entry names completed tracked work, so
    /// citing it must stay allowed. This is the `#fr79` case: a real id blocked
    /// because it had been archived out of the live document.
    #[test]
    fn an_archived_entry_id_is_tracked() {
        let (_dir, doc) = project("- 2026-08-09 [#fr79] Shipped the thing.\n", "body\n");
        let ids = archived_tracked_ids(&doc, "body\n").unwrap();
        assert!(ids.contains("fr79"), "{ids:?}");
    }

    /// `#coinedguardledgerasymmetry`: a tag merely CITED inside an entry's prose
    /// is a reference, not an item. Vouching for it launders a coined id into
    /// legitimacy forever — the rule `#hookhashanchortags` rejected.
    #[test]
    fn a_tag_only_cited_in_archived_prose_is_not_tracked() {
        let (_dir, doc) = project(
            "- 2026-08-09 [#fr79] Shipped the thing, related to #neverwasanitem.\n",
            "body\n",
        );
        let ids = archived_tracked_ids(&doc, "body\n").unwrap();
        assert!(ids.contains("fr79"), "{ids:?}");
        assert!(
            !ids.contains("neverwasanitem"),
            "a citation must not vouch for itself: {ids:?}"
        );
    }

    /// Resolution is the union of both strategies the two guards used, so
    /// unifying the predicate cannot lose an archive either one used to find.
    /// Here the declared `archive=` target is the ONLY route to the file.
    #[test]
    fn a_declared_archive_target_is_read_even_without_a_sibling() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/nested")).unwrap();
        let doc = dir.path().join("tasks/nested/bugs.md");
        let body = "<!-- agent:done archive=tasks/elsewhere.done.md -->\n<!-- /agent:done -->\n";
        std::fs::write(&doc, body).unwrap();
        std::fs::write(
            dir.path().join("tasks/elsewhere.done.md"),
            "- 2026-08-09 [#declaredonly] Shipped.\n",
        )
        .unwrap();

        let ids = archived_tracked_ids(&doc, body).unwrap();
        assert!(ids.contains("declaredonly"), "{ids:?}");
    }

    /// And the sibling walk is the ONLY route here, with no `archive=` declared.
    #[test]
    fn a_sibling_archive_is_read_without_a_declared_target() {
        let (_dir, doc) = project("- 2026-08-09 [#siblingonly] Shipped.\n", "body\n");
        let ids = archived_tracked_ids(&doc, "body\n").unwrap();
        assert!(ids.contains("siblingonly"), "{ids:?}");
    }
}
