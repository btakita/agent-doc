use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use agent_doc_element::element;
use agent_doc_element::element::{is_backlog_component, is_tracked_work_component};

/// `#staleshow` — stable sentinel line inserted into the `<!-- agent:status -->`
/// component whenever the live route-owned supervisor/controller serving the
/// document is mapping a STALE agent-doc binary.
pub const STALE_SUPERVISOR_STATUS_MARKER: &str = "🔴 (restart/recycle your supervisor)";

/// Prefix for the binary-owned queue handoff summary projected into
/// `agent:status`. Reported queue rows (`📥`) are not stale or finished: their
/// worker result still needs coordinator integration, verification, and closeout.
pub const REPORTED_QUEUE_STATUS_PREFIX: &str = "📥 Queue handoff:";

/// Neutral delimiters for the single agent-managed status-sync projection.
///
/// These deliberately are not `agent:*` markers: the sync projection is nested
/// inside `agent:status`, but it is not an independently addressable component.
pub const STATUS_SYNC_OPEN_MARKER: &str = "<!-- status:sync -->";
pub const STATUS_SYNC_CLOSE_MARKER: &str = "<!-- /status:sync -->";

/// How an `agent:status sync="..."` update treats ordinary status prose.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusSyncMode {
    /// Upsert the managed sync block and preserve every byte outside it.
    #[default]
    Additive,
    /// Replace ordinary prose while retaining binary-owned operational lines.
    Replace,
}

/// Persistent status synchronization instruction surfaced by preflight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusSyncDirective {
    pub instruction: String,
    pub mode: StatusSyncMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ManagedStatusSyncBlock {
    block_start: usize,
    content_start: usize,
    content_end: usize,
    block_end: usize,
}

fn managed_status_sync_block(status_body: &str) -> Result<Option<ManagedStatusSyncBlock>> {
    let mut open: Option<(usize, usize)> = None;
    let mut found = None;
    let mut offset = 0usize;

    for segment in status_body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment).trim();
        if line == STATUS_SYNC_OPEN_MARKER {
            if open.is_some() || found.is_some() {
                bail!("agent:status contains duplicate or nested status:sync blocks");
            }
            open = Some((offset, offset + segment.len()));
        } else if line == STATUS_SYNC_CLOSE_MARKER {
            let Some((block_start, content_start)) = open.take() else {
                bail!("agent:status contains a status:sync close marker without an opener");
            };
            if found.is_some() {
                bail!("agent:status contains more than one status:sync block");
            }
            found = Some(ManagedStatusSyncBlock {
                block_start,
                content_start,
                content_end: offset,
                block_end: offset + segment.len(),
            });
        }
        offset += segment.len();
    }

    if open.is_some() {
        bail!("agent:status contains an unclosed status:sync block");
    }
    Ok(found)
}

/// Read and validate the persistent status-sync instruction on `agent:status`.
///
/// `sync-mode` accepts only `additive` (the default) and `replace`. Validation
/// also proves that any existing managed block is unique and balanced, so both
/// preflight and writeback fail before changing status content.
pub fn status_sync_directive(content: &str) -> Result<Option<StatusSyncDirective>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components
        .iter()
        .find(|component| component.name == "status")
    else {
        return Ok(None);
    };
    managed_status_sync_block(status.content(content))?;

    let marker = &content[status.open_start..status.open_end];
    let attrs = element::open_marker_attr_tokens(marker)
        .context("failed to parse agent:status opening marker attributes")?;
    let sync = attrs
        .iter()
        .filter(|(key, _)| key == "sync")
        .collect::<Vec<_>>();
    let modes = attrs
        .iter()
        .filter(|(key, _)| key == "sync-mode")
        .collect::<Vec<_>>();
    if sync.len() > 1 {
        bail!("agent:status must contain at most one sync attribute");
    }
    if modes.len() > 1 {
        bail!("agent:status must contain at most one sync-mode attribute");
    }
    let Some((_, instruction)) = sync.first() else {
        if !modes.is_empty() {
            bail!("agent:status sync-mode requires a sync instruction");
        }
        return Ok(None);
    };
    let instruction = instruction.as_deref().unwrap_or_default().trim();
    if instruction.is_empty() {
        bail!("agent:status sync attribute must contain a non-empty instruction");
    }
    let mode = match modes.first().map(|(_, value)| value.as_deref()) {
        None => StatusSyncMode::Additive,
        Some(Some("additive")) => StatusSyncMode::Additive,
        Some(Some("replace")) => StatusSyncMode::Replace,
        Some(Some(other)) => {
            bail!("unsupported agent:status sync-mode `{other}`; expected `additive` or `replace`")
        }
        Some(None) => bail!(
            "unsupported bare agent:status sync-mode; expected `additive` or `replace`"
        ),
    };
    Ok(Some(StatusSyncDirective {
        instruction: instruction.to_string(),
        mode,
    }))
}

fn normalized_status_sync_payload(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    }
}

fn render_status_sync_block(text: &str) -> String {
    format!(
        "{STATUS_SYNC_OPEN_MARKER}\n{}{STATUS_SYNC_CLOSE_MARKER}\n",
        normalized_status_sync_payload(text)
    )
}

fn is_protected_status_line(line: &str) -> bool {
    let line = line.trim();
    line == STALE_SUPERVISOR_STATUS_MARKER
        || line.starts_with(REPORTED_QUEUE_STATUS_PREFIX)
        || [
            "[agent-doc sync]",
            "[agent-doc merge]",
            "[agent-doc repair]",
            "[sync]",
            "[merge]",
            "[repair]",
            "sync ",
            "merge ",
            "repair ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn protected_status_prose(status_body: &str) -> String {
    let mut protected = Vec::new();
    let mut paragraph = Vec::new();
    let flush = |paragraph: &mut Vec<&str>, protected: &mut Vec<String>| {
        if paragraph.iter().any(|line| is_protected_status_line(line)) {
            protected.push(paragraph.join("\n"));
        }
        paragraph.clear();
    };

    for line in status_body.lines() {
        if line.trim().is_empty() {
            flush(&mut paragraph, &mut protected);
        } else {
            paragraph.push(line);
        }
    }
    flush(&mut paragraph, &mut protected);
    protected.join("\n\n")
}

fn apply_status_sync_body(status_body: &str, text: &str, mode: StatusSyncMode) -> Result<String> {
    let block = managed_status_sync_block(status_body)?;
    if mode == StatusSyncMode::Additive {
        if let Some(block) = block {
            let payload = normalized_status_sync_payload(text);
            if status_body[block.content_start..block.content_end] == payload {
                return Ok(status_body.to_string());
            }
            let mut updated = status_body.to_string();
            updated.replace_range(block.content_start..block.content_end, &payload);
            return Ok(updated);
        }

        let mut updated = status_body.to_string();
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        if !updated.trim().is_empty() && !updated.ends_with("\n\n") {
            updated.push('\n');
        }
        updated.push_str(&render_status_sync_block(text));
        return Ok(updated);
    }

    let ordinary_free = if let Some(block) = block {
        format!(
            "{}{}",
            &status_body[..block.block_start],
            &status_body[block.block_end..]
        )
    } else {
        status_body.to_string()
    };
    let protected = protected_status_prose(&ordinary_free);
    let managed = render_status_sync_block(text);
    if protected.is_empty() {
        Ok(managed)
    } else {
        Ok(format!("{protected}\n\n{managed}"))
    }
}

/// Apply `write --status` semantics while honoring an optional status sync
/// directive. Documents without `sync` retain the legacy destructive replace.
pub fn update_status_content(content: &str, text: &str) -> Result<String> {
    let Some(directive) = status_sync_directive(content)? else {
        return replace_status_content(content, text);
    };
    let components = element::parse(content).context("failed to parse components")?;
    let status = components
        .iter()
        .find(|component| component.name == "status")
        .context("document has no status component")?;
    let body = apply_status_sync_body(status.content(content), text, directive.mode)?;
    Ok(status.replace_content(content, &body))
}

/// Replace the `agent:status` component body with canonical status text.
pub fn replace_status_content(content: &str, text: &str) -> Result<String> {
    let components = element::parse(content).context("failed to parse components")?;
    let status = components
        .into_iter()
        .find(|component| component.name == "status")
        .context("document has no status component")?;
    let new_content = format!("\n{}\n", text);
    Ok(status.replace_content(content, &new_content))
}

fn first_live_backlog_id(content: &str) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    for comp in components {
        if !is_backlog_component(&comp.name) {
            continue;
        }
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(comp.content(content));
        if let Some(item) = items
            .into_iter()
            .find(|item| !item.id.is_empty() && !item.is_done())
        {
            return Ok(Some(item.id));
        }
    }
    Ok(None)
}

fn extract_status_top_backlog_id(status: &str) -> Option<String> {
    let marker = "Top backlog item:";
    let marker_idx = status.find(marker)?;
    let after_marker = &status[marker_idx + marker.len()..];
    let hash_idx = after_marker.find('#')?;
    let after_hash = &after_marker[hash_idx + 1..];
    let id = after_hash
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>();
    if id.is_empty() { None } else { Some(id) }
}

fn replace_top_backlog_sentence(line: &str, live_id: Option<&str>) -> String {
    let marker = "Top backlog item:";
    let Some(marker_idx) = line.find(marker) else {
        return line.to_string();
    };
    let sentence_end = line[marker_idx..]
        .find('.')
        .map(|idx| marker_idx + idx + 1)
        .unwrap_or(line.len());
    let before = line[..marker_idx].trim_end();
    let after = line[sentence_end..].trim_start();
    let replacement = live_id
        .map(|id| format!("Top backlog item: #{id}."))
        .unwrap_or_else(|| "No open backlog items.".to_string());

    match (before.is_empty(), after.is_empty()) {
        (true, true) => replacement,
        (false, true) => format!("{before} {replacement}"),
        (true, false) => format!("{replacement} {after}"),
        (false, false) => format!("{before} {replacement} {after}"),
    }
}

/// Reconcile a `Top backlog item: #id.` status sentence with the live backlog.
///
/// This is intentionally narrow: free-form status text is user-editable, so we
/// only touch the generated top-backlog sentence when it names a stale id.
pub fn reconcile_top_backlog_status_content(content: &str) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components.iter().find(|c| c.name == "status") else {
        return Ok(None);
    };
    if !components
        .iter()
        .any(|c| is_tracked_work_component(&c.name))
    {
        return Ok(None);
    }

    let status_body = status.content(content);
    let Some(status_id) = extract_status_top_backlog_id(status_body) else {
        return Ok(None);
    };
    let live_id = first_live_backlog_id(content)?;
    if live_id.as_deref() == Some(status_id.as_str()) {
        return Ok(None);
    }

    let mut replaced_any = false;
    let mut new_status = String::new();
    for segment in status_body.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        if line.contains("Top backlog item:") {
            new_status.push_str(&replace_top_backlog_sentence(line, live_id.as_deref()));
            replaced_any = true;
        } else {
            new_status.push_str(line);
        }
        new_status.push_str(newline);
    }
    if !replaced_any {
        return Ok(None);
    }
    Ok(Some(status.replace_content(content, &new_status)))
}

/// `#staleshow` — pure insert/remove of the [`STALE_SUPERVISOR_STATUS_MARKER`]
/// line in the status component body, idempotently, given whether the live
/// supervisor is currently running a stale binary.
pub fn apply_stale_supervisor_marker(status_body: &str, is_stale: bool) -> Option<String> {
    let has_marker = status_body
        .lines()
        .any(|line| line.trim() == STALE_SUPERVISOR_STATUS_MARKER);

    if is_stale {
        if has_marker {
            return None;
        }
        let trimmed = status_body.strip_prefix('\n').unwrap_or(status_body);
        let new_body = format!("\n{STALE_SUPERVISOR_STATUS_MARKER}\n{trimmed}");
        Some(new_body)
    } else {
        if !has_marker {
            return None;
        }
        let mut new_body = String::new();
        for segment in status_body.split_inclusive('\n') {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            if line.trim() == STALE_SUPERVISOR_STATUS_MARKER {
                continue;
            }
            new_body.push_str(segment);
        }
        Some(new_body)
    }
}

/// `#staleshow` — reconcile the stale-supervisor marker in the document's status
/// component against the live staleness signal.
pub fn reconcile_stale_supervisor_status_content(
    content: &str,
    is_stale: bool,
) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components.iter().find(|c| c.name == "status") else {
        return Ok(None);
    };
    let status_body = status.content(content);
    match apply_stale_supervisor_marker(status_body, is_stale) {
        Some(new_body) => Ok(Some(status.replace_content(content, &new_body))),
        None => Ok(None),
    }
}

fn reported_queue_status_line(reported_count: usize) -> String {
    let noun = if reported_count == 1 { "item" } else { "items" };
    format!(
        "{REPORTED_QUEUE_STATUS_PREFIX} {reported_count} reported {noun} awaiting coordinator integration, verification, and closeout."
    )
}

/// Reconcile the binary-owned reported-queue summary in a status body.
///
/// The line is projection-only telemetry. It is replaced rather than appended,
/// so changing counts cannot accumulate stale summaries, and count zero removes
/// it while preserving every operator-authored status line.
pub fn apply_reported_queue_status(
    status_body: &str,
    reported_count: usize,
) -> Option<String> {
    let expected = (reported_count > 0).then(|| reported_queue_status_line(reported_count));
    let mut retained = String::new();
    let mut existing = Vec::new();
    for segment in status_body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if line.trim().starts_with(REPORTED_QUEUE_STATUS_PREFIX) {
            existing.push(line.trim().to_string());
        } else {
            retained.push_str(segment);
        }
    }

    if existing.len() == usize::from(expected.is_some())
        && existing.first().map(String::as_str) == expected.as_deref()
    {
        return None;
    }

    let Some(expected) = expected else {
        return Some(retained);
    };
    let retained = retained.strip_prefix('\n').unwrap_or(&retained);
    Some(format!("\n{expected}\n{retained}"))
}

/// Reconcile the reported-queue handoff summary in the document's status
/// component without changing any operator-authored status prose.
pub fn reconcile_reported_queue_status_content(
    content: &str,
    reported_count: usize,
) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components.iter().find(|component| component.name == "status") else {
        return Ok(None);
    };
    match apply_reported_queue_status(status.content(content), reported_count) {
        Some(new_body) => Ok(Some(status.replace_content(content, &new_body))),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_stale_top_backlog_status_to_live_head() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Logged follow-ups. Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#new] New head\n",
            "<!-- /agent:backlog -->\n"
        );
        let out = reconcile_top_backlog_status_content(doc)
            .unwrap()
            .expect("stale status should be reconciled");
        assert!(out.contains("Logged follow-ups. Top backlog item: #new."));
        assert!(!out.contains("#old"));
    }

    #[test]
    fn replace_status_content_replaces_only_status_body() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "old status\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange -->\n",
            "keep me\n",
            "<!-- /agent:exchange -->\n"
        );

        let out = replace_status_content(doc, "new status").unwrap();
        let status = element::parse(&out)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "status")
            .unwrap();

        assert_eq!(status.content(&out), "\nnew status\n");
        assert!(!out.contains("old status"));
        assert!(out.contains("keep me"));
    }

    #[test]
    fn replace_status_content_requires_status_component() {
        let err = replace_status_content("# No status\n", "new").unwrap_err();

        assert!(err.to_string().contains("document has no status component"));
    }

    #[test]
    fn clears_stale_top_backlog_status_when_backlog_is_empty() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #old.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );
        let out = reconcile_top_backlog_status_content(doc)
            .unwrap()
            .expect("stale status should be cleared");
        assert!(out.contains("No open backlog items."));
        assert!(!out.contains("#old"));
    }

    #[test]
    fn preserves_status_when_top_backlog_is_current() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #live.\n",
            "<!-- /agent:status -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live] Live head\n",
            "<!-- /agent:backlog -->\n"
        );
        assert!(reconcile_top_backlog_status_content(doc).unwrap().is_none());
    }

    #[test]
    fn inserts_stale_supervisor_marker_when_stale() {
        let body = "\nSession ready. Top backlog item: #live.\n";
        let out = apply_stale_supervisor_marker(body, true)
            .expect("marker should be inserted when stale");
        assert!(out.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert_eq!(
            out.lines().find(|l| !l.trim().is_empty()),
            Some(STALE_SUPERVISOR_STATUS_MARKER)
        );
        assert!(out.contains("Session ready. Top backlog item: #live."));
    }

    #[test]
    fn does_not_duplicate_stale_supervisor_marker_on_repeat() {
        let body = "\nSession ready.\n";
        let once = apply_stale_supervisor_marker(body, true).expect("first insert");
        assert!(apply_stale_supervisor_marker(&once, true).is_none());
        assert_eq!(once.matches(STALE_SUPERVISOR_STATUS_MARKER).count(), 1);
    }

    #[test]
    fn removes_stale_supervisor_marker_when_fresh() {
        let body = format!("\n{STALE_SUPERVISOR_STATUS_MARKER}\nSession ready.\n");
        let out = apply_stale_supervisor_marker(&body, false)
            .expect("marker should be removed when fresh");
        assert!(!out.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(out.contains("Session ready."));
    }

    #[test]
    fn no_change_removing_marker_when_already_fresh() {
        let body = "\nSession ready.\n";
        assert!(apply_stale_supervisor_marker(body, false).is_none());
    }

    #[test]
    fn reconcile_inserts_and_removes_stale_supervisor_marker_in_document() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Session ready.\n",
            "<!-- /agent:status -->\n"
        );
        let stale = reconcile_stale_supervisor_status_content(doc, true)
            .unwrap()
            .expect("stale should insert marker");
        assert!(stale.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(stale.contains("Session ready."));
        assert!(
            reconcile_stale_supervisor_status_content(&stale, true)
                .unwrap()
                .is_none()
        );
        let fresh = reconcile_stale_supervisor_status_content(&stale, false)
            .unwrap()
            .expect("fresh should clear marker");
        assert!(!fresh.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(fresh.contains("Session ready."));
    }

    #[test]
    fn reconcile_is_noop_without_status_component() {
        let doc = "## Backlog\n\n<!-- agent:backlog -->\n<!-- /agent:backlog -->\n";
        assert!(
            reconcile_stale_supervisor_status_content(doc, true)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reported_queue_status_is_counted_updated_and_removed_without_touching_status_prose() {
        let doc = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Operator-authored summary.\n",
            "<!-- /agent:status -->\n"
        );

        let one = reconcile_reported_queue_status_content(doc, 1)
            .unwrap()
            .expect("reported work should add a status summary");
        assert!(one.contains("📥 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout."));
        assert!(one.contains("Operator-authored summary."));
        assert!(
            reconcile_reported_queue_status_content(&one, 1)
                .unwrap()
                .is_none()
        );

        let many = reconcile_reported_queue_status_content(&one, 3)
            .unwrap()
            .expect("a changed count should replace the summary");
        assert!(many.contains("📥 Queue handoff: 3 reported items awaiting coordinator integration, verification, and closeout."));
        assert_eq!(many.matches(REPORTED_QUEUE_STATUS_PREFIX).count(), 1);

        let cleared = reconcile_reported_queue_status_content(&many, 0)
            .unwrap()
            .expect("zero reported items should clear the summary");
        assert!(!cleared.contains(REPORTED_QUEUE_STATUS_PREFIX));
        assert!(cleared.contains("Operator-authored summary."));
    }

    #[test]
    fn status_sync_directive_defaults_to_additive_and_accepts_replace() {
        let additive = concat!(
            "<!-- agent:status patch=replace sync=\"Keep the published release current\" -->\n",
            "ordinary\n",
            "<!-- /agent:status -->\n"
        );
        assert_eq!(
            status_sync_directive(additive).unwrap(),
            Some(StatusSyncDirective {
                instruction: "Keep the published release current".to_string(),
                mode: StatusSyncMode::Additive,
            })
        );

        let replace = additive.replace(
            "sync=\"Keep the published release current\"",
            "sync=\"Keep the published release current\" sync-mode=replace",
        );
        assert_eq!(
            status_sync_directive(&replace).unwrap().unwrap().mode,
            StatusSyncMode::Replace
        );
    }

    #[test]
    fn status_sync_rejects_unknown_mode_without_changing_content() {
        let doc = concat!(
            "<!-- agent:status sync=\"Keep release current\" sync-mode=overwrite -->\n",
            "ordinary\n",
            "<!-- /agent:status -->\n"
        );
        let err = update_status_content(doc, "v1.2.3").unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported agent:status sync-mode")
        );
        assert_eq!(
            doc,
            concat!(
                "<!-- agent:status sync=\"Keep release current\" sync-mode=overwrite -->\n",
                "ordinary\n",
                "<!-- /agent:status -->\n"
            )
        );
    }

    #[test]
    fn status_sync_rejects_blank_duplicate_and_orphaned_attributes() {
        for (marker, expected) in [
            (
                "<!-- agent:status sync= -->",
                "sync attribute must contain a non-empty instruction",
            ),
            (
                "<!-- agent:status sync=release sync-mode -->",
                "unsupported bare agent:status sync-mode",
            ),
            (
                "<!-- agent:status sync=release sync-mode= -->",
                "unsupported agent:status sync-mode",
            ),
            (
                "<!-- agent:status sync=one sync=two -->",
                "at most one sync attribute",
            ),
            (
                "<!-- agent:status sync-mode=replace -->",
                "sync-mode requires a sync instruction",
            ),
        ] {
            let doc = format!("{marker}\nordinary\n<!-- /agent:status -->\n");
            let err = update_status_content(&doc, "v1.2.3").unwrap_err();
            assert!(
                err.to_string().contains(expected),
                "unexpected error for {marker}: {err:#}"
            );
        }
    }

    #[test]
    fn additive_status_sync_upserts_one_managed_block_and_preserves_other_bytes() {
        let doc = concat!(
            "before\n",
            "<!-- agent:status patch=replace sync=\"Keep release current\" custom=kept -->\n",
            "Operator summary.\n",
            "📥 Queue handoff: 1 reported item awaiting coordinator integration.\n",
            "<!-- /agent:status -->\n",
            "after\n"
        );
        let once = update_status_content(doc, "Published: v1.2.3").unwrap();
        assert!(once.starts_with("before\n<!-- agent:status patch=replace sync=\"Keep release current\" custom=kept -->\n"));
        assert!(once.contains("Operator summary."));
        assert!(once.contains(REPORTED_QUEUE_STATUS_PREFIX));
        assert_eq!(once.matches(STATUS_SYNC_OPEN_MARKER).count(), 1);
        assert!(once.contains("<!-- status:sync -->\nPublished: v1.2.3\n<!-- /status:sync -->"));
        assert!(once.ends_with("<!-- /agent:status -->\nafter\n"));

        let repeated = update_status_content(&once, "Published: v1.2.3").unwrap();
        assert_eq!(repeated, once, "same sync projection must be idempotent");

        let updated = update_status_content(&once, "Published: v1.2.4").unwrap();
        assert!(!updated.contains("Published: v1.2.3"));
        assert!(updated.contains("Published: v1.2.4"));
        assert_eq!(updated.matches(STATUS_SYNC_OPEN_MARKER).count(), 1);
        assert!(updated.contains("Operator summary."));
    }

    #[test]
    fn replace_status_sync_drops_ordinary_prose_but_preserves_operational_paragraphs() {
        let doc = concat!(
            "<!-- agent:status sync=\"Keep release current\" sync-mode=replace keep=yes -->\n",
            "Ordinary prose to replace.\n\n",
            "📥 Queue handoff: 2 reported items awaiting coordinator integration.\n\n",
            "[agent-doc repair] retained response warning.\n",
            "repair detail remains available.\n\n",
            "🔴 (restart/recycle your supervisor)\n",
            "<!-- /agent:status -->\n"
        );
        let updated = update_status_content(doc, "Published: v2.0.0").unwrap();
        assert!(!updated.contains("Ordinary prose to replace."));
        assert!(updated.contains(REPORTED_QUEUE_STATUS_PREFIX));
        assert!(updated.contains("[agent-doc repair] retained response warning."));
        assert!(updated.contains("repair detail remains available."));
        assert!(updated.contains(STALE_SUPERVISOR_STATUS_MARKER));
        assert!(updated.contains("<!-- status:sync -->\nPublished: v2.0.0\n<!-- /status:sync -->"));
        assert!(updated.contains("sync-mode=replace keep=yes"));
    }

    #[test]
    fn status_sync_rejects_duplicate_or_malformed_managed_blocks() {
        let duplicate = concat!(
            "<!-- agent:status sync=\"Keep release current\" -->\n",
            "<!-- status:sync -->\none\n<!-- /status:sync -->\n",
            "<!-- status:sync -->\ntwo\n<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n"
        );
        assert!(
            status_sync_directive(duplicate)
                .unwrap_err()
                .to_string()
                .contains("duplicate or nested status:sync blocks")
        );

        let unclosed = concat!(
            "<!-- agent:status sync=\"Keep release current\" -->\n",
            "<!-- status:sync -->\none\n",
            "<!-- /agent:status -->\n"
        );
        assert!(
            status_sync_directive(unclosed)
                .unwrap_err()
                .to_string()
                .contains("unclosed status:sync block")
        );
    }
}
