use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use agent_doc_element::element;
use agent_doc_element::element::{is_backlog_component, is_tracked_work_component};

/// `#staleshow` — stable sentinel line inserted into the `<!-- agent:status -->`
/// component whenever the live route-owned supervisor/controller serving the
/// document is mapping a STALE agent-doc binary.
pub const STALE_SUPERVISOR_STATUS_MARKER: &str = "🔴 (restart/recycle your supervisor)";

/// Prefix for the binary-owned queue handoff summary projected into
/// `agent:status`. Reported queue rows (`☑️`) mark the worker complete, but are not
/// terminally integrated: their
/// worker result still needs coordinator integration, verification, and closeout.
pub const REPORTED_QUEUE_STATUS_PREFIX: &str = "📤 Queue handoff:";
const LEGACY_REPORTED_QUEUE_STATUS_PREFIX: &str = "📥 Queue handoff:";

/// `agent:status` opening-marker attribute that turns the per-document queue
/// marker legend off (`queue-legend=off`, `#queuemarkerlegend`).
pub const STATUS_QUEUE_LEGEND_ATTR: &str = "queue-legend";

/// Whether a `queue-legend` attribute value turns the legend off.
pub fn queue_legend_attr_disables(value: Option<&str>) -> bool {
    matches!(
        value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("off" | "false" | "no" | "0")
    )
}

/// Whether `queue-legend` carries a recognized value (bare, on/off, true/false,
/// yes/no, 1/0).
pub fn is_valid_queue_legend_attr_value(value: Option<&str>) -> bool {
    match value.map(|value| value.trim().to_ascii_lowercase()) {
        None => true,
        Some(value) => matches!(
            value.as_str(),
            "on" | "off" | "true" | "false" | "yes" | "no" | "1" | "0"
        ),
    }
}

/// Whether the document's `agent:status` component should carry the
/// binary-owned queue marker legend line (`#queuemarkerlegend`): the document
/// has a runnable-work component (`agent:run`, `agent:loop`, or legacy
/// `agent:queue`) and the status marker does not opt out with
/// `queue-legend=off`.
pub fn status_queue_legend_wanted(content: &str, status: &element::Component) -> bool {
    let marker = &content[status.open_start..status.open_end];
    let opted_out = element::open_marker_attr_tokens(marker)
        .map(|attrs| {
            attrs.iter().any(|(key, value)| {
                key == STATUS_QUEUE_LEGEND_ATTR && queue_legend_attr_disables(value.as_deref())
            })
        })
        .unwrap_or(false);
    if opted_out {
        return false;
    }
    element::parse(content)
        .map(|components| {
            components.iter().any(|component| {
                element::canonical_component_name(&component.name) == element::QUEUE_COMPONENT
            })
        })
        .unwrap_or(false)
}

/// A fenced-code opener: up to three spaces, then three or more backticks or
/// tildes (a backtick info string may not contain a backtick).
fn fence_opener(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let ch = trimmed
        .chars()
        .next()
        .filter(|ch| matches!(ch, '`' | '~'))?;
    let run = trimmed.chars().take_while(|c| *c == ch).count();
    if run < 3 || (ch == '`' && trimmed[run..].contains('`')) {
        return None;
    }
    Some((ch, run))
}

fn closes_fence(line: &str, (ch, run): (char, usize)) -> bool {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return false;
    }
    let closing = trimmed.chars().take_while(|c| *c == ch).count();
    closing >= run && trimmed[closing * ch.len_utf8()..].trim().is_empty()
}

/// Reconcile the binary-owned queue marker legend line in a status body
/// (`#queuemarkerlegend`).
///
/// Every legend line outside fenced code is removed, then, when `wanted`, one
/// canonical line rendered from the installed palette is appended as the last
/// paragraph. The result is a fixed point: applying it again changes nothing,
/// so a status write never accumulates legends and an unchanged legend never
/// churns the document. When the body ends inside an unterminated fence the
/// legend is not appended, because it would render as code. A body with no
/// legend line and `wanted == false` is returned byte-for-byte.
pub fn apply_queue_marker_legend(status_body: &str, wanted: bool) -> String {
    let mut kept = String::with_capacity(status_body.len());
    let mut removed = false;
    let mut fence: Option<(char, usize)> = None;
    for segment in status_body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        match fence {
            Some(open) => {
                if closes_fence(line, open) {
                    fence = None;
                }
                kept.push_str(segment);
            }
            None => {
                if let Some(open) = fence_opener(line) {
                    fence = Some(open);
                    kept.push_str(segment);
                } else if agent_doc_frontmatter::frontmatter::is_queue_marker_legend_line(line) {
                    removed = true;
                } else {
                    kept.push_str(segment);
                }
            }
        }
    }
    if !wanted {
        return if removed {
            collapse_trailing_blank_lines(&kept)
        } else {
            status_body.to_string()
        };
    }
    if fence.is_some() {
        return if removed {
            kept
        } else {
            status_body.to_string()
        };
    }
    let legend = agent_doc_frontmatter::frontmatter::queue_marker_legend_line();
    let prose = kept.trim_end_matches(['\n', ' ', '\t']);
    if prose.trim().is_empty() {
        format!("\n{legend}\n")
    } else {
        format!("{prose}\n\n{legend}\n")
    }
}

fn collapse_trailing_blank_lines(body: &str) -> String {
    let trimmed = body.trim_end_matches(['\n', ' ', '\t']);
    if trimmed.trim().is_empty() {
        "\n".to_string()
    } else {
        format!("{trimmed}\n")
    }
}

/// Apply [`apply_queue_marker_legend`] to a status component body about to be
/// written into `content`.
pub fn reconcile_status_body_queue_legend(
    content: &str,
    status: &element::Component,
    status_body: &str,
) -> String {
    apply_queue_marker_legend(status_body, status_queue_legend_wanted(content, status))
}

use crate::status_sync_tree::{
    ManagedStatusSyncBlock, ensure_well_formed_output, managed_status_sync_block,
    normalize_status_sync_body, normalized_status_sync_payload, parse_status_write,
    render_status_sync_block,
};
pub use crate::status_sync_tree::{STATUS_SYNC_CLOSE_MARKER, STATUS_SYNC_OPEN_MARKER};
/// Minimum opt-in cadence for owner-turn status-sync checkpoints.
pub const STATUS_SYNC_MIN_INTERVAL_MS: u64 = 60_000;

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
    /// Validated opt-in checkpoint cadence from `interval`, in milliseconds.
    /// `None` keeps synchronization at preflight only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
}

/// Read and validate the persistent status-sync instruction on `agent:status`.
///
/// `sync-mode` accepts only `additive` (the default) and `replace`. An optional
/// `interval` enables repeated mid-turn checkpoints using the queue duration
/// grammar and cannot be less than one minute. Validation also proves that any
/// existing managed block is unique and balanced, so both preflight and
/// writeback fail before changing status content.
pub fn status_sync_directive(content: &str) -> Result<Option<StatusSyncDirective>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components
        .iter()
        .find(|component| component.name == "status")
    else {
        return Ok(None);
    };
    managed_status_sync_block(status.content(content))?;
    status_sync_directive_attrs(content, status)
}

/// Parse only the `sync`, `sync-mode`, and `interval` attributes of the status
/// opening marker. Writers use this instead of [`status_sync_directive`] so a
/// malformed managed structure is repaired by the write rather than refused.
fn status_sync_directive_attrs(
    content: &str,
    status: &element::Component,
) -> Result<Option<StatusSyncDirective>> {
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
    let intervals = attrs
        .iter()
        .filter(|(key, _)| key == "interval")
        .collect::<Vec<_>>();
    if sync.len() > 1 {
        bail!("agent:status must contain at most one sync attribute");
    }
    if modes.len() > 1 {
        bail!("agent:status must contain at most one sync-mode attribute");
    }
    if intervals.len() > 1 {
        bail!("agent:status must contain at most one interval attribute");
    }
    let Some((_, instruction)) = sync.first() else {
        if !modes.is_empty() {
            bail!("agent:status sync-mode requires a sync instruction");
        }
        if !intervals.is_empty() {
            bail!("agent:status interval requires a sync instruction");
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
        Some(None) => {
            bail!("unsupported bare agent:status sync-mode; expected `additive` or `replace`")
        }
    };
    let interval_ms = match intervals.first().map(|(_, value)| value.as_deref()) {
        None => None,
        Some(Some(raw)) => Some(parse_status_sync_interval_ms(raw)?),
        Some(None) => bail!(
            "unsupported bare agent:status interval; expected a duration such as `1m`, `5m`, or `1h30m`"
        ),
    };
    Ok(Some(StatusSyncDirective {
        instruction: instruction.to_string(),
        mode,
        interval_ms,
    }))
}

fn parse_status_sync_interval_ms(raw: &str) -> Result<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("agent:status interval must contain a duration");
    }
    let seconds = if let Ok(seconds) = raw.parse::<u64>() {
        seconds
    } else {
        let mut total = 0u64;
        let mut digits = String::new();
        for ch in raw.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            let unit = match ch.to_ascii_lowercase() {
                'd' => 86_400u64,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => bail!(
                    "agent:status interval `{raw}` is not a duration; use e.g. `60`, `1m`, `5m`, `1h30m`, or `1d`"
                ),
            };
            let value = digits.parse::<u64>().with_context(|| {
                format!(
                    "agent:status interval `{raw}` is not a duration; use e.g. `60`, `1m`, `5m`, `1h30m`, or `1d`"
                )
            })?;
            digits.clear();
            let segment = value
                .checked_mul(unit)
                .with_context(|| format!("agent:status interval `{raw}` is too long"))?;
            total = total
                .checked_add(segment)
                .with_context(|| format!("agent:status interval `{raw}` is too long"))?;
        }
        if !digits.is_empty() {
            bail!(
                "agent:status interval `{raw}` mixes a unit-less number into a duration; use e.g. `1h30m`"
            );
        }
        total
    };
    let interval_ms = seconds
        .checked_mul(1_000)
        .with_context(|| format!("agent:status interval `{raw}` is too long"))?;
    if interval_ms < STATUS_SYNC_MIN_INTERVAL_MS {
        bail!("agent:status interval `{raw}` is below the minimum `1m`");
    }
    Ok(interval_ms)
}

fn is_protected_status_line(line: &str) -> bool {
    let line = line.trim();
    line == STALE_SUPERVISOR_STATUS_MARKER
        || line.starts_with(REPORTED_QUEUE_STATUS_PREFIX)
        || line.starts_with(LEGACY_REPORTED_QUEUE_STATUS_PREFIX)
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

/// Rebuild a status body as: the existing leading framing, the binary-owned
/// operational text from outside the managed region, optional caller prose, and
/// exactly one managed region around `payload`.
///
/// - `prose == None` is `sync-mode=replace`: ordinary prose is dropped and whole
///   operational paragraphs are retained.
/// - `prose == Some(..)` is a full status body supplied by the caller (its text
///   carried its own region). The caller owns every ordinary line, so only the
///   operational *lines* are retained, and caller lines that are themselves
///   operational or repeat a retained line are dropped — the projection owns
///   those, and keeping both would duplicate them.
fn compose_full_status_body(
    status_body: &str,
    block: Option<ManagedStatusSyncBlock>,
    prose: Option<&str>,
    payload: &str,
) -> String {
    let outside = match block {
        Some(block) => format!(
            "{}{}",
            &status_body[..block.block_start],
            &status_body[block.block_end..]
        ),
        None => status_body.to_string(),
    };
    let leading_len = status_body.len() - status_body.trim_start_matches('\n').len();
    let mut parts = Vec::new();
    match prose {
        None => {
            let protected = protected_status_prose(&outside);
            if !protected.is_empty() {
                parts.push(protected);
            }
        }
        Some(prose) => {
            let protected_lines = outside
                .lines()
                .filter(|line| is_protected_status_line(line))
                .map(str::trim_end)
                .collect::<Vec<_>>();
            let known = protected_lines
                .iter()
                .map(|line| line.trim())
                .collect::<std::collections::HashSet<_>>();
            if !protected_lines.is_empty() {
                parts.push(protected_lines.join("\n"));
            }
            let mut kept: Vec<&str> = Vec::new();
            for line in prose.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    if kept.last().is_some_and(|last| !last.trim().is_empty()) {
                        kept.push("");
                    }
                    continue;
                }
                if is_protected_status_line(line) || known.contains(trimmed) {
                    continue;
                }
                kept.push(line.trim_end());
            }
            let prose = kept.join("\n");
            let prose = prose.trim();
            if !prose.is_empty() {
                parts.push(prose.to_string());
            }
        }
    }
    let mut body = status_body[..leading_len].to_string();
    for part in parts {
        body.push_str(&part);
        body.push_str("\n\n");
    }
    body.push_str(&render_status_sync_block(payload));
    body
}

/// Self-heal a nested, duplicated, unclosed, or orphaned `status:sync`
/// structure in the document's `agent:status` component.
///
/// Returns `Ok(None)` when there is no status component or its managed
/// structure is already well-formed. Otherwise the status body is folded into
/// exactly one region: the latest innermost payload is kept, free-form prose
/// that leaked inside a wrapping region moves back outside (dropping lines the
/// outside prose already has), superseded payloads and orphaned markers are
/// removed, and every byte outside the status component is unchanged.
pub fn normalize_status_sync_content(content: &str) -> Result<Option<String>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components
        .iter()
        .find(|component| component.name == "status")
    else {
        return Ok(None);
    };
    Ok(normalize_status_sync_body(status.content(content))?
        .map(|body| status.replace_content(content, &body)))
}

fn apply_status_sync_body(status_body: &str, text: &str, mode: StatusSyncMode) -> Result<String> {
    let updated = compose_status_sync_body(status_body, text, mode)?;
    ensure_well_formed_output(&updated)?;
    Ok(updated)
}

fn compose_status_sync_body(status_body: &str, text: &str, mode: StatusSyncMode) -> Result<String> {
    // Self-heal first: a nested/duplicated/unclosed structure left by an older
    // writer is folded into one region before this write lands, so the write
    // never propagates (or wraps) a malformed shape.
    let healed = normalize_status_sync_body(status_body)?;
    let status_body = healed.as_deref().unwrap_or(status_body);
    let block = managed_status_sync_block(status_body)?;
    // The incoming text is parsed structurally, never spliced in as opaque
    // bytes: markers it carries are consumed, and prose it places outside its
    // own region makes it a full status body.
    let write = parse_status_write(text)?;

    if let Some(prose) = write.prose.as_deref() {
        return Ok(compose_full_status_body(
            status_body,
            block,
            Some(prose),
            &write.payload,
        ));
    }

    if mode == StatusSyncMode::Additive {
        let payload = normalized_status_sync_payload(&write.payload);
        if let Some(block) = block {
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
        updated.push_str(&render_status_sync_block(&write.payload));
        return Ok(updated);
    }

    Ok(compose_full_status_body(
        status_body,
        block,
        None,
        &write.payload,
    ))
}

/// What the authoritative document's `agent:status` holds right now, for a
/// status-sync checkpoint to compare against (`#statuspatchdrop` (c)).
///
/// On 2026-10-28 the Stop hook asked the agent to re-evaluate the sync
/// instruction, and the agent answered "no change" from its memory of a
/// `patch:status` the write path had silently dropped. A checkpoint that quotes
/// the current payload lets the agent compare against the document instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSyncCheckpointEvidence {
    /// The managed `status:sync` payload, `None` when the status has no block.
    pub payload: Option<String>,
    /// First 12 hex digits of the payload's SHA-256 (empty payload when none).
    pub payload_hash: String,
    /// Non-blank ordinary prose lines outside the managed block: not the queue
    /// legend and not a binary-owned operational line. An additive status write
    /// never rewrites these.
    pub outside_prose_lines: usize,
}

/// Read [`StatusSyncCheckpointEvidence`] from `content`; `Ok(None)` when the
/// document has no status component.
pub fn status_sync_checkpoint_evidence(
    content: &str,
) -> Result<Option<StatusSyncCheckpointEvidence>> {
    let components = element::parse(content).context("failed to parse components")?;
    let Some(status) = components
        .iter()
        .find(|component| component.name == "status")
    else {
        return Ok(None);
    };
    let (outside, payload) =
        crate::status_sync_tree::split_status_sync_body(status.content(content))?;
    let outside_prose_lines = outside
        .lines()
        .filter(|line| {
            !line.trim().is_empty()
                && !is_protected_status_line(line)
                && !agent_doc_frontmatter::frontmatter::is_queue_marker_legend_line(line)
        })
        .count();
    let payload_hash = agent_doc_hash::content_hash(payload.as_deref().unwrap_or(""));
    Ok(Some(StatusSyncCheckpointEvidence {
        payload,
        payload_hash: payload_hash.chars().take(12).collect(),
        outside_prose_lines,
    }))
}

/// Render [`status_sync_checkpoint_evidence`] as checkpoint text: the current
/// payload quoted verbatim, its hash, and a note when ordinary prose outside
/// the block would survive an additive write. `None` without a status
/// component or when the status body cannot be read.
pub fn render_status_sync_checkpoint_evidence(content: &str) -> Option<String> {
    let evidence = status_sync_checkpoint_evidence(content).ok().flatten()?;
    let mut rendered = match evidence.payload.as_deref().map(str::trim) {
        Some(payload) if !payload.is_empty() => {
            let quoted = payload
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "The document's current `status:sync` payload (sha256 {}), read from the \
                 authoritative document now. Compare against THIS text, not against what you \
                 remember writing: an earlier status write may not have landed.\n{quoted}",
                evidence.payload_hash,
            )
        }
        _ => "The document's `agent:status` has no `status:sync` payload yet, so no earlier \
              status write landed in it."
            .to_string(),
    };
    if evidence.outside_prose_lines > 0 {
        rendered.push_str(&format!(
            "\nOutside that block the status also holds {} line(s) of ordinary prose that an \
             additive `status set` never rewrites. If it is stale, pass a full status body \
             (prose plus its own `<!-- status:sync -->` block) to `--text`; that replaces \
             the ordinary prose too.",
            evidence.outside_prose_lines,
        ));
    }
    Some(rendered)
}

/// Apply `write --status` semantics while honoring an optional status sync
/// directive. Documents without `sync` retain the legacy destructive replace.
pub fn update_status_content(content: &str, text: &str) -> Result<String> {
    let components = element::parse(content).context("failed to parse components")?;
    let status = components
        .iter()
        .find(|component| component.name == "status")
        .context("document has no status component")?;
    let Some(directive) = status_sync_directive_attrs(content, status)? else {
        return replace_status_content(content, text);
    };
    let body = apply_status_sync_body(status.content(content), text, directive.mode)?;
    let body = reconcile_status_body_queue_legend(content, status, &body);
    Ok(status.replace_content(content, &body))
}

/// Replace the `agent:status` component body with canonical status text.
pub fn replace_status_content(content: &str, text: &str) -> Result<String> {
    let components = element::parse(content).context("failed to parse components")?;
    let status = components
        .into_iter()
        .find(|component| component.name == "status")
        .context("document has no status component")?;
    // Legacy destructive replace keeps the caller's text, except that a
    // malformed `status:sync` structure in it is folded to one region.
    let healed = normalize_status_sync_body(text)?;
    let text = healed.as_deref().map(str::trim_end).unwrap_or(text);
    let new_content = format!("\n{}\n", text);
    let new_content = reconcile_status_body_queue_legend(content, &status, &new_content);
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
pub fn apply_reported_queue_status(status_body: &str, reported_count: usize) -> Option<String> {
    let expected = (reported_count > 0).then(|| reported_queue_status_line(reported_count));
    let mut retained = String::new();
    let mut existing = Vec::new();
    for segment in status_body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if line.trim().starts_with(REPORTED_QUEUE_STATUS_PREFIX)
            || line.trim().starts_with(LEGACY_REPORTED_QUEUE_STATUS_PREFIX)
        {
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
    let Some(status) = components
        .iter()
        .find(|component| component.name == "status")
    else {
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

    fn legend() -> String {
        agent_doc_frontmatter::frontmatter::queue_marker_legend_line()
    }

    fn status_body(doc: &str) -> String {
        let status = element::parse(doc)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "status")
            .unwrap();
        status.content(doc).to_string()
    }

    fn queue_doc(status_marker: &str, status: &str) -> String {
        format!(
            "## Status {status_marker}\n{status}<!-- /agent:status -->\n\n## Run <!-- agent:run -->\n- do [#a]\n<!-- /agent:run -->\n"
        )
    }

    #[test]
    fn queue_legend_is_appended_once_and_is_a_fixed_point() {
        let doc = queue_doc("<!-- agent:status patch=replace -->", "old\n");
        let once = update_status_content(&doc, "Working.").unwrap();
        assert_eq!(status_body(&once), format!("\nWorking.\n\n{}\n", legend()));
        let twice = update_status_content(&once, "Working.").unwrap();
        assert_eq!(twice, once);
        // A response that echoes (or repeats) the legend still lands one.
        let echoed = format!("{}\nWorking.\n{}", legend(), legend());
        let thrice = update_status_content(&once, &echoed).unwrap();
        assert_eq!(status_body(&thrice).matches("Queue markers:").count(), 1);
        assert!(status_body(&thrice).ends_with(&format!("{}\n", legend())));
    }

    #[test]
    fn queue_legend_needs_a_run_component_and_honors_opt_out() {
        let no_queue =
            "## Status <!-- agent:status patch=replace -->\nold\n<!-- /agent:status -->\n";
        let out = update_status_content(no_queue, "new").unwrap();
        assert_eq!(status_body(&out), "\nnew\n");

        let opted_out = queue_doc(
            "<!-- agent:status patch=replace queue-legend=off -->",
            "old\n",
        );
        let out = update_status_content(&opted_out, &format!("new\n\n{}", legend())).unwrap();
        assert_eq!(
            status_body(&out),
            "\nnew\n",
            "opt-out also removes a stale legend"
        );

        for loop_marker in ["<!-- agent:loop -->", "<!-- agent:queue -->"] {
            let doc = format!(
                "<!-- agent:status -->\nx\n<!-- /agent:status -->\n{loop_marker}\n<!-- /{} -->\n",
                &loop_marker[5..loop_marker.len() - 4]
            );
            let out = update_status_content(&doc, "y").unwrap();
            assert!(status_body(&out).contains(&legend()), "{loop_marker}");
        }
    }

    #[test]
    fn queue_legend_never_lands_in_or_is_read_from_fenced_code() {
        let fenced = format!("```\n{}\n```", legend());
        let body = apply_queue_marker_legend(&format!("\n{fenced}\n"), true);
        assert_eq!(body, format!("\n{fenced}\n\n{}\n", legend()));

        let unterminated = "\nlog:\n```\nstill code\n";
        assert_eq!(apply_queue_marker_legend(unterminated, true), unterminated);
    }

    #[test]
    fn queue_legend_composes_with_status_sync_and_handoff_line() {
        let doc = queue_doc(
            "<!-- agent:status patch=replace sync=\"Keep release current\" -->",
            "\nprose\n",
        );
        let out = update_status_content(&doc, "Published: v2").unwrap();
        let body = status_body(&out);
        assert!(body.contains("<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->"));
        assert!(body.ends_with(&format!("<!-- /status:sync -->\n\n{}\n", legend())));
        assert_eq!(update_status_content(&out, "Published: v2").unwrap(), out);

        let handoff = apply_reported_queue_status(&body, 2).unwrap();
        assert!(handoff.starts_with(&format!("\n{REPORTED_QUEUE_STATUS_PREFIX} 2")));
        assert!(handoff.ends_with(&format!("{}\n", legend())));
        assert_eq!(apply_queue_marker_legend(&handoff, true), handoff);
    }

    #[test]
    fn queue_legend_attr_values() {
        assert!(queue_legend_attr_disables(Some("off")));
        assert!(queue_legend_attr_disables(Some("FALSE")));
        assert!(!queue_legend_attr_disables(None));
        assert!(!queue_legend_attr_disables(Some("on")));
        assert!(is_valid_queue_legend_attr_value(Some("no")));
        assert!(!is_valid_queue_legend_attr_value(Some("maybe")));
    }

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
        assert!(one.contains("📤 Queue handoff: 1 reported item awaiting coordinator integration, verification, and closeout."));
        assert!(one.contains("Operator-authored summary."));
        assert!(
            reconcile_reported_queue_status_content(&one, 1)
                .unwrap()
                .is_none()
        );

        let many = reconcile_reported_queue_status_content(&one, 3)
            .unwrap()
            .expect("a changed count should replace the summary");
        assert!(many.contains("📤 Queue handoff: 3 reported items awaiting coordinator integration, verification, and closeout."));
        assert_eq!(many.matches(REPORTED_QUEUE_STATUS_PREFIX).count(), 1);

        let cleared = reconcile_reported_queue_status_content(&many, 0)
            .unwrap()
            .expect("zero reported items should clear the summary");
        assert!(!cleared.contains(REPORTED_QUEUE_STATUS_PREFIX));
        assert!(cleared.contains("Operator-authored summary."));

        let legacy = one.replace(
            REPORTED_QUEUE_STATUS_PREFIX,
            LEGACY_REPORTED_QUEUE_STATUS_PREFIX,
        );
        let migrated = reconcile_reported_queue_status_content(&legacy, 1)
            .unwrap()
            .expect("the former pending/report marker should migrate deterministically");
        assert!(migrated.contains(REPORTED_QUEUE_STATUS_PREFIX));
        assert!(!migrated.contains(LEGACY_REPORTED_QUEUE_STATUS_PREFIX));
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
                interval_ms: None,
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

        let throttled = additive.replace(
            "sync=\"Keep the published release current\"",
            "sync=\"Keep the published release current\" interval=1h30m",
        );
        assert_eq!(
            status_sync_directive(&throttled)
                .unwrap()
                .unwrap()
                .interval_ms,
            Some(5_400_000)
        );

        let legacy: StatusSyncDirective = serde_json::from_value(serde_json::json!({
            "instruction": "Keep release current",
            "mode": "additive"
        }))
        .unwrap();
        assert_eq!(legacy.interval_ms, None);
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
            (
                "<!-- agent:status interval=5m -->",
                "interval requires a sync instruction",
            ),
            (
                "<!-- agent:status sync=release interval -->",
                "unsupported bare agent:status interval",
            ),
            (
                "<!-- agent:status sync=release interval= -->",
                "interval must contain a duration",
            ),
            (
                "<!-- agent:status sync=release interval=30s -->",
                "below the minimum `1m`",
            ),
            (
                "<!-- agent:status sync=release interval=5x -->",
                "is not a duration",
            ),
            (
                "<!-- agent:status sync=release interval=18446744073709551615 -->",
                "is too long",
            ),
            (
                "<!-- agent:status sync=release interval=1m interval=5m -->",
                "at most one interval attribute",
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

    /// `#statuspatchdrop` (c): the checkpoint quotes what the document holds,
    /// so "did the answer change" is decided against the payload, not memory.
    #[test]
    fn status_sync_checkpoint_evidence_quotes_the_current_payload_and_stale_prose() {
        let doc = concat!(
            "<!-- agent:status sync=\"##sync-release\" -->\n",
            "Main is at `b6b9af6e15` (older prose).\n\n",
            "<!-- status:sync -->\nLatest release: v0.35.558\n<!-- /status:sync -->\n\n",
            "Queue markers: 📥 pending · ⛔ blocked · 🟡 claimed · 🚧 running · ☑️ reported · ⚠️ lost · 🔄 needs reconcile · ❔ unknown · ⏭️ skipped · ⏳ awaiting operator · 🙋 operator action · 📌 pinned · 📍 agent pin ([legend](https://github.com/btakita/agent-doc/blob/main/docs/guide/queue-markers.md#queue-markers))\n",
            "📤 Queue handoff: 1 reported item awaiting coordinator integration.\n",
            "<!-- /agent:status -->\n",
        );
        let evidence = status_sync_checkpoint_evidence(doc).unwrap().unwrap();
        assert_eq!(
            evidence.payload.as_deref(),
            Some("Latest release: v0.35.558\n")
        );
        assert_eq!(
            evidence.payload_hash,
            agent_doc_hash::content_hash("Latest release: v0.35.558\n")[..12]
        );
        // The legend and the binary-owned handoff line are not ordinary prose.
        assert_eq!(evidence.outside_prose_lines, 1);

        let rendered = render_status_sync_checkpoint_evidence(doc).unwrap();
        assert!(
            rendered.contains("> Latest release: v0.35.558"),
            "{rendered}"
        );
        assert!(rendered.contains(&evidence.payload_hash), "{rendered}");
        assert!(
            rendered.contains("not against what you remember writing"),
            "{rendered}"
        );
        assert!(
            rendered.contains("1 line(s) of ordinary prose"),
            "{rendered}"
        );

        // After an additive write the evidence follows the document.
        let updated = update_status_content(doc, "Latest release: v0.35.559").unwrap();
        let rendered = render_status_sync_checkpoint_evidence(&updated).unwrap();
        assert!(
            rendered.contains("> Latest release: v0.35.559"),
            "{rendered}"
        );
        assert!(!rendered.contains("v0.35.558"), "{rendered}");

        // No managed block yet, and no ordinary prose: say so plainly.
        let empty = "<!-- agent:status sync=\"x\" -->\n<!-- /agent:status -->\n";
        let rendered = render_status_sync_checkpoint_evidence(empty).unwrap();
        assert!(
            rendered.contains("has no `status:sync` payload yet"),
            "{rendered}"
        );
        assert!(!rendered.contains("ordinary prose"), "{rendered}");
        assert_eq!(
            render_status_sync_checkpoint_evidence("no components\n"),
            None
        );
    }

    #[test]
    fn additive_status_sync_upserts_one_managed_block_and_preserves_other_bytes() {
        let doc = concat!(
            "before\n",
            "<!-- agent:status patch=replace sync=\"Keep release current\" custom=kept -->\n",
            "Operator summary.\n",
            "📤 Queue handoff: 1 reported item awaiting coordinator integration.\n",
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
            "📤 Queue handoff: 2 reported items awaiting coordinator integration.\n\n",
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

    // Exact `agent:status` components from agent-loop `tasks/agent-doc/agent-doc-bugs.md`:
    // `INCIDENT_PRIOR_STATUS` at 411c4cb1a1 and `INCIDENT_NESTED_STATUS` at dc6186d4da,
    // which `agent-doc write --status` produced from the prior one on 2026-10-20.
    const INCIDENT_PRIOR_STATUS: &str = concat!(
        "<!-- agent:status patch=replace sync=\"Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time.\" -->\n",
        "\n",
        "📤 Queue handoff: 2 reported items awaiting coordinator integration, verification, and closeout.\n",
        "Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).\n",
        "\n",
        "<!-- status:sync -->\n",
        "Latest published release: [v0.35.527](https://github.com/btakita/agent-doc/releases/tag/v0.35.527) (GH #317, #318, #319 + blocking-packet todo), published on GitHub at 2026-10-20 06:26:45 UTC (15 assets); PyPI 0.35.527 followed at 2026-10-20 06:27:34 UTC; installed locally (install-full). JetBrains packages unchanged at 0.2.720 (classic), 0.2.721 (IDE 262), 0.2.722 (IDE 263): no plugin source change. Next: v0.35.528 (GH #320 option A, background-drain busy-turn defer, orphan-drain claim re-read, state-propagation spec) waits on the #320 make check.\n",
        "<!-- /status:sync -->\n",
        "<!-- /agent:status -->\n",
    );
    const INCIDENT_NESTED_STATUS: &str = concat!(
        "<!-- agent:status patch=replace sync=\"Track the latest published agent-doc release + publication time and latest JetBrains plugin release version + publication time.\" -->\n",
        "\n",
        "📤 Queue handoff: 5 reported items awaiting coordinator integration, verification, and closeout.\n",
        "Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).\n",
        "\n",
        "<!-- status:sync -->\n",
        "Review pass done. frontendlivedispatch closed on live evidence. Two review items remain, both genuinely gated: an operator decision (jb262dynlauncherrecommen) and an external PyPI reply (pypiquotarecheck).\n",
        "\n",
        "<!-- status:sync -->\n",
        "Latest published release: [v0.35.528](https://github.com/btakita/agent-doc/releases/tag/v0.35.528) (GH #320 option A: Remote Dev split state reaches the controller; background-drain busy-turn defer, orphan-drain claim re-read, state-propagation spec), published on GitHub at 2026-10-20 07:37:15 UTC (15 assets); PyPI 0.35.528 followed at 2026-10-20 07:38:01 UTC; installed locally (agent-doc 0.35.528). JetBrains packages in v0.35.528: 0.2.723 (classic), 0.2.725 (IDE 262), 0.2.724 (IDE 263), published with the release at 2026-10-20 07:37:15 UTC; installed: IntelliJ IDEA 2026.1 classic 0.2.723, Rider 2026.2 IDE-262 0.2.725. VS Code extension 0.2.83.\n",
        "<!-- /status:sync -->\n",
        "<!-- /status:sync -->\n",
        "<!-- /agent:status -->\n",
    );

    /// The `--status` text the agent passed: its full status body, region included.
    fn incident_agent_status_text() -> String {
        let body = INCIDENT_NESTED_STATUS
            .split_once("<!-- status:sync -->\n")
            .unwrap()
            .1;
        body.rsplit_once("<!-- /status:sync -->\n<!-- /agent:status -->")
            .unwrap()
            .0
            .trim_end()
            .to_string()
    }

    fn incident_doc(status: &str) -> String {
        format!(
            "## Status\n\n{status}\n## Exchange\n\n<!-- agent:exchange -->\nkeep me\n<!-- /agent:exchange -->\n"
        )
    }

    fn assert_single_incident_region(doc: &str) {
        assert_eq!(doc.matches(STATUS_SYNC_OPEN_MARKER).count(), 1, "{doc}");
        assert_eq!(doc.matches(STATUS_SYNC_CLOSE_MARKER).count(), 1, "{doc}");
        assert_eq!(doc.matches("Review pass done.").count(), 1, "{doc}");
        assert_eq!(
            doc.matches(REPORTED_QUEUE_STATUS_PREFIX).count(),
            1,
            "{doc}"
        );
        assert!(
            doc.contains("Latest published release: [v0.35.528]"),
            "{doc}"
        );
        assert!(!doc.contains("v0.35.527"), "{doc}");
        assert!(doc.contains("<!-- agent:exchange -->\nkeep me\n<!-- /agent:exchange -->"));
        status_sync_directive(doc).expect("result must satisfy the preflight sync contract");
    }

    #[test]
    fn full_body_status_write_replaces_the_region_instead_of_nesting_it() {
        let text = incident_agent_status_text();
        assert!(text.starts_with("Review pass done."), "{text}");
        assert!(text.contains(STATUS_SYNC_OPEN_MARKER), "{text}");

        let doc = incident_doc(INCIDENT_PRIOR_STATUS);
        let updated = update_status_content(&doc, &text).unwrap();
        assert_single_incident_region(&updated);
        assert!(updated.contains(concat!(
            "📤 Queue handoff: 2 reported items awaiting coordinator integration, verification, and closeout.\n\n",
            "Review pass done.",
        )));

        let repeated = update_status_content(&updated, &text).unwrap();
        assert_eq!(
            repeated, updated,
            "a repeated full-body write must be byte-idempotent"
        );
    }

    #[test]
    fn nested_incident_status_self_heals_on_the_next_write() {
        let doc = incident_doc(INCIDENT_NESTED_STATUS);
        let err = status_sync_directive(&doc).unwrap_err();
        assert!(err.to_string().contains("duplicate or nested"), "{err:#}");

        for text in [
            incident_agent_status_text(),
            "Latest published release: [v0.35.528](https://github.com/btakita/agent-doc/releases/tag/v0.35.528)."
                .to_string(),
        ] {
            let updated = update_status_content(&doc, &text).unwrap();
            assert_single_incident_region(&updated);
            let repeated = update_status_content(&updated, &text).unwrap();
            assert_eq!(repeated, updated, "repeat must be byte-idempotent");
        }
    }

    #[test]
    fn normalize_status_sync_content_repairs_the_incident_shape_in_place() {
        let doc = incident_doc(INCIDENT_NESTED_STATUS);
        let healed = normalize_status_sync_content(&doc)
            .unwrap()
            .expect("nested shape needs repair");
        assert_single_incident_region(&healed);
        let prior_shape = INCIDENT_PRIOR_STATUS
            .replace("2 reported items", "5 reported items")
            .replace(
                INCIDENT_PRIOR_STATUS
                    .split_once("<!-- status:sync -->\n")
                    .unwrap()
                    .1
                    .split_once("<!-- /status:sync -->")
                    .unwrap()
                    .0,
                INCIDENT_NESTED_STATUS
                    .rsplit_once("<!-- status:sync -->\n")
                    .unwrap()
                    .1
                    .split_once("<!-- /status:sync -->")
                    .unwrap()
                    .0,
            );
        assert_eq!(
            healed,
            incident_doc(&prior_shape),
            "repair restores the pre-incident shape"
        );
        assert_eq!(normalize_status_sync_content(&healed).unwrap(), None);
        assert_eq!(
            normalize_status_sync_content(&incident_doc(INCIDENT_PRIOR_STATUS)).unwrap(),
            None
        );
    }

    #[test]
    fn replace_mode_full_body_write_keeps_operational_lines_and_caller_prose() {
        let doc = concat!(
            "<!-- agent:status sync=\"Keep release current\" sync-mode=replace -->\n",
            "\n📤 Queue handoff: 2 reported items awaiting coordinator integration.\n",
            "Old summary.\n\n",
            "<!-- status:sync -->\nPublished: v1\n<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n"
        );
        let text = concat!(
            "📤 Queue handoff: 1 reported item awaiting coordinator integration.\n",
            "New summary.\n\n",
            "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
        );
        let updated = update_status_content(doc, text).unwrap();
        assert_eq!(
            updated,
            concat!(
                "<!-- agent:status sync=\"Keep release current\" sync-mode=replace -->\n",
                "\n📤 Queue handoff: 2 reported items awaiting coordinator integration.\n\n",
                "New summary.\n\n",
                "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
                "<!-- /agent:status -->\n"
            )
        );
        assert_eq!(update_status_content(&updated, text).unwrap(), updated);
    }

    #[test]
    fn legacy_status_without_sync_attr_folds_nested_text() {
        let doc = "<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        let updated = update_status_content(
            doc,
            "Summary.\n\n<!-- status:sync -->\nSummary.\n\n<!-- status:sync -->\nv2\n<!-- /status:sync -->\n<!-- /status:sync -->",
        )
        .unwrap();
        assert_eq!(
            updated,
            "<!-- agent:status -->\n\nSummary.\n\n<!-- status:sync -->\nv2\n<!-- /status:sync -->\n<!-- /agent:status -->\n"
        );
    }
}
