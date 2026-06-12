//! # Module: template
//!
//! Template-mode support for in-place response documents. Parses structured patch
//! blocks from agent responses and applies them to named component slots in the
//! document, with boundary marker lifecycle management for CRDT-safe stream writes.
//!
//! ## Spec
//! - `parse_patches`: Scans agent response text for `<!-- patch:name -->...<!-- /patch:name -->`
//!   blocks and returns a list of `PatchBlock` values plus any unmatched text (content outside
//!   patch blocks). Markers inside fenced code blocks (``` or ~~~) and inline code spans are
//!   ignored so examples in responses are never mis-parsed as real patches.
//! - `apply_patches`: Delegates to `apply_patches_with_overrides` with an empty override map.
//!   Applies parsed patches to matching `<!-- agent:name -->` components in the document.
//! - `apply_patches_with_overrides`: Core patch application pipeline:
//!   1. Pre-patch: strips all existing boundary markers, inserts a fresh boundary at the end
//!      of the `exchange` component (keyed to the file stem).
//!   2. Applies each patch using mode resolution: stream overrides > inline attr (`patch=` or
//!      `mode=`) > `.agent-doc/config.toml ([components] section)` > built-in defaults (`exchange`/`findings`
//!      default to `append`, all others to `replace`).
//!   3. For `append` mode, uses boundary-aware insertion when a boundary marker exists.
//!   4. Patches targeting missing component names are routed as overflow to `exchange`/`output`.
//!   5. Unmatched text and overflow are merged and appended to `exchange`/`output` (or an
//!      auto-created `exchange` component if none exists).
//!   6. Post-patch: if the boundary was consumed, re-inserts a fresh one at the end of exchange.
//! - `reposition_boundary_to_end`: Removes all boundaries and inserts a new one at the end of
//!   `exchange`. Used by the IPC write path to keep boundary position current.
//! - `reposition_boundary_to_end_with_summary`: Same, with optional human-readable suffix on
//!   the boundary ID (e.g. `a0cfeb34:agent-doc`).
//! - `template_info`: Reads a document file, resolves its template mode flag, and returns a
//!   serializable `TemplateInfo` with per-component name, resolved mode, content, and line
//!   number. Used by editor plugins for rendering.
//! - `is_template_mode` (test-only): Legacy helper to detect `mode = "template"` string.
//!
//! ## Agentic Contracts
//! - `parse_patches` is pure and infallible for valid UTF-8; it returns `Ok` even for empty
//!   or patch-free responses.
//! - Patch markers inside fenced code blocks are never extracted as real patches. Agents may
//!   include example markers in code blocks without triggering unintended writes.
//! - Component patches are applied in reverse document order so earlier byte offsets remain
//!   valid throughout the operation.
//! - A boundary marker always exists at the end of `exchange` after `apply_patches_with_overrides`
//!   returns. Callers that perform incremental (CRDT/stream) writes may rely on this invariant.
//! - Missing-component patches never cause errors; content is silently routed to `exchange`/
//!   `output` with a diagnostic written to stderr.
//! - Mode precedence is deterministic: stream override > inline attr (`patch=` > `mode=`) >
//!   `config.toml ([components] section)` (`patch` key > `mode` key) > built-in default. Callers can rely on this
//!   ordering when constructing overrides for stream mode.
//! - `template_info` reads the document from disk; callers must ensure the file is flushed
//!   before calling (especially in the IPC write path).
//!
//! ## Evals
//! - `parse_single_patch`: single patch block → one `PatchBlock`, empty unmatched
//! - `parse_multiple_patches`: two sequential patch blocks → two `PatchBlock`s in order, empty unmatched
//! - `parse_with_unmatched_content`: text before and after patch block → unmatched contains both text segments
//! - `parse_empty_response`: empty string → zero patches, empty unmatched
//! - `parse_no_patches`: plain text with no markers → zero patches, full text in unmatched
//! - `parse_patches_ignores_markers_in_fenced_code_block`: agent:component markers inside ``` are preserved as content
//! - `parse_patches_ignores_patch_markers_in_fenced_code_block`: nested patch markers inside ``` are not parsed as patches
//! - `parse_patches_ignores_markers_in_tilde_fence`: patch markers inside ~~~ are ignored
//! - `parse_patches_ignores_closing_marker_in_code_block`: closing marker inside code block is skipped; real close is found
//! - `parse_patches_normal_markers_still_work`: sanity — two back-to-back patches parse correctly
//! - `apply_patches_replace`: patch to non-exchange component replaces existing content
//! - `apply_patches_unmatched_creates_exchange`: unmatched text auto-creates `<!-- agent:exchange -->` when absent
//! - `apply_patches_unmatched_appends_to_existing_exchange`: unmatched text appends to existing exchange; no duplicate component
//! - `repair_duplicate_exchange_opener_merges_two_blocks`: two complete exchange blocks → merged into one
//! - `repair_duplicate_exchange_opener_returns_none_for_single`: single exchange block → `None` (no repair needed)
//! - `apply_patches_post_patch_merges_duplicate_exchange_opener`: post-patch guard merges duplicate exchange openers created during patching
//! - `apply_patches_missing_component_routes_to_exchange`: patch targeting unknown component name appears in exchange
//! - `apply_patches_missing_component_creates_exchange`: missing component + no exchange → auto-creates exchange with overflow
//! - `inline_attr_mode_overrides_config`: `mode=replace` on tag wins over `config.toml ([components] section)` append config
//! - `inline_attr_mode_overrides_default`: `mode=replace` on exchange wins over built-in append default
//! - `no_inline_attr_falls_back_to_config`: no inline attr → `config.toml ([components] section)` append config applies
//! - `no_inline_attr_no_config_falls_back_to_default`: no attr, no config → exchange defaults to append
//! - `inline_patch_attr_overrides_config`: `patch=replace` on tag wins over `config.toml ([components] section)` append config
//! - `template_info_works`: template-mode doc → `TemplateInfo.template_mode = true`, component list populated
//! - `template_info_legacy_mode_works`: `response_mode: template` frontmatter key recognized
//! - `template_info_append_mode`: non-template doc → `template_mode = false`, empty component list
//! - `is_template_mode_detection`: `Some("template")` → true; other strings and `None` → false
//! - (aspirational) `apply_patches_boundary_invariant`: after any apply_patches call with an exchange component, a boundary marker exists at end of exchange
//! - (aspirational) `reposition_boundary_removes_stale`: multiple stale boundaries are reduced to exactly one at end of exchange

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;

use crate::component::{self, Component, find_comment_end, is_backlog_component};

/// A parsed patch directive from an agent response.
#[derive(Debug, Clone)]
pub struct PatchBlock {
    pub name: String,
    pub content: String,
    /// Attributes from the patch marker (e.g., `transfer-source="path"`).
    #[allow(dead_code)]
    pub attrs: std::collections::HashMap<String, String>,
}

impl PatchBlock {
    /// Create a PatchBlock with no attributes.
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        PatchBlock {
            name: name.into(),
            content: content.into(),
            attrs: std::collections::HashMap::new(),
        }
    }
}

/// Template info output for plugins.
#[derive(Debug, Serialize)]
pub struct TemplateInfo {
    pub template_mode: bool,
    pub components: Vec<ComponentInfo>,
}

/// Per-component info for plugin rendering.
#[derive(Debug, Serialize)]
pub struct ComponentInfo {
    pub name: String,
    pub mode: String,
    pub content: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<usize>,
}

/// Check if a document is in template mode (deprecated — use `fm.resolve_mode().is_template()`).
#[cfg(test)]
pub fn is_template_mode(mode: Option<&str>) -> bool {
    matches!(mode, Some("template"))
}

/// Parse `<!-- patch:name -->...<!-- /patch:name -->` blocks from an agent response.
///
/// Content outside patch blocks is collected as "unmatched" and returned separately.
/// Markers inside fenced code blocks (``` or ~~~) and inline code spans are ignored.
///
/// Also accepts the canonical `<!-- replace:pending -->...<!-- /replace:pending -->`
/// form as a synonym for `<!-- patch:pending -->...<!-- /patch:pending -->`. The
/// `replace:` prefix signals full-replacement semantics and is the canonical name
/// for pending mutations going forward. `patch:pending` is still parsed for one
/// release with a deprecation warning emitted to stderr. See #25ag.
///
/// `<!-- replace:icebox -->...<!-- /replace:icebox -->` is also accepted so the
/// skill has a binary-owned path to rewrite `agent:icebox` without dumping the
/// payload into exchange as unmatched content.
pub fn parse_patches(response: &str) -> Result<(Vec<PatchBlock>, String)> {
    let bytes = response.as_bytes();
    let len = bytes.len();
    let code_ranges = component::find_code_ranges(response);
    let mut patches = Vec::new();
    let mut unmatched = String::new();
    let mut pos = 0;
    let mut last_end = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            pos += 1;
            continue;
        }

        // Skip markers inside code regions
        if code_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end)
        {
            pos += 4;
            continue;
        }

        let marker_start = pos;

        // Find closing -->
        let close = match find_comment_end(bytes, pos + 4) {
            Some(c) => c,
            None => {
                pos += 4;
                continue;
            }
        };

        let inner = &response[marker_start + 4..close - 3];
        let trimmed = inner.trim();

        // Recognize two prefix forms:
        //   - `patch:<name>`     — original form (deprecated for pending component)
        //   - `replace:pending`  — canonical form for the pending component (#25ag)
        //   - `replace:icebox`   — canonical full-rewrite form for the icebox component
        let parsed_prefix: Option<(&str, &str)> = if let Some(rest) = trimmed.strip_prefix("patch:")
        {
            Some(("patch", rest))
        } else if let Some(rest) = trimmed.strip_prefix("replace:") {
            // Only tracked-work replace forms are accepted. Other
            // `replace:*` names fall through as unmatched to avoid silently
            // broadening the grammar.
            let rest_trim = rest.trim_start();
            let name_end = rest_trim
                .find(|c: char| c.is_whitespace())
                .unwrap_or(rest_trim.len());
            let name = &rest_trim[..name_end];
            if is_backlog_component(name)
                || name == component::ICEBOX_COMPONENT
                || name == component::REVIEW_COMPONENT
            {
                Some(("replace", rest))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((prefix_kind, rest)) = parsed_prefix {
            let rest = rest.trim();
            if rest.is_empty() || rest.starts_with('/') {
                pos = close;
                continue;
            }

            // Split name from attributes: "exchange transfer-source=path" -> ("exchange", attrs)
            let (name, attrs) = if let Some(space_idx) = rest.find(char::is_whitespace) {
                let name = &rest[..space_idx];
                let attr_text = rest[space_idx..].trim();
                (name, component::parse_attrs(attr_text))
            } else {
                (rest, std::collections::HashMap::new())
            };

            // Deprecation warning: `patch:pending` is deprecated in favor of
            // `replace:pending`. Warn once per parse call on first occurrence.
            if prefix_kind == "patch" && is_backlog_component(name) {
                eprintln!(
                    "warning: `<!-- patch:{} -->` is deprecated — use `<!-- replace:{} -->` instead (see #25ag)",
                    name, name
                );
            }

            // Consume trailing newline after opening marker
            let mut content_start = close;
            if content_start < len && bytes[content_start] == b'\n' {
                content_start += 1;
            }

            // Collect unmatched text before this patch block
            let before = &response[last_end..marker_start];
            let trimmed_before = before.trim();
            if !trimmed_before.is_empty() {
                if !unmatched.is_empty() {
                    unmatched.push('\n');
                }
                unmatched.push_str(trimmed_before);
            }

            // Find the matching close: <!-- /<prefix>:name --> (skipping code blocks).
            // The close must use the same prefix as the open.
            let close_marker = format!("<!-- /{}:{} -->", prefix_kind, name);
            if let Some(close_pos) =
                find_outside_code(&close_marker, response, content_start, &code_ranges)
            {
                let content = &response[content_start..close_pos];
                patches.push(PatchBlock {
                    name: name.to_string(),
                    content: content.to_string(),
                    attrs,
                });

                let mut end = close_pos + close_marker.len();
                if end < len && bytes[end] == b'\n' {
                    end += 1;
                }
                last_end = end;
                pos = end;
                continue;
            }

            // No matching close found — consume the orphaned opening marker
            // so it doesn't leak into unmatched text (#p2xm).
            let mut orphan_end = close;
            if orphan_end < len && bytes[orphan_end] == b'\n' {
                orphan_end += 1;
            }
            last_end = orphan_end;
            pos = orphan_end;
            continue;
        }

        pos = close;
    }

    // Collect any trailing unmatched text
    if last_end < len {
        let trailing = response[last_end..].trim();
        if !trailing.is_empty() {
            if !unmatched.is_empty() {
                unmatched.push('\n');
            }
            unmatched.push_str(trailing);
        }
    }

    Ok((patches, unmatched))
}

/// Apply patch blocks to a document's components.
///
/// For each patch block, finds the matching `<!-- agent:name -->` component
/// and replaces its content. Uses patch.rs mode logic (replace/append/prepend)
/// based on `.agent-doc/config.toml ([components] section)` config.
///
/// Returns the modified document. Unmatched content (outside patch blocks)
/// is appended to `<!-- agent:output -->` if it exists, or creates one at the end.
pub fn apply_patches_pure(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    summary: Option<&str>,
    component_configs: &std::collections::HashMap<String, String>,
    max_lines_configs: &std::collections::HashMap<String, usize>,
) -> Result<String> {
    apply_patches_with_overrides_pure(
        doc,
        patches,
        unmatched,
        summary,
        component_configs,
        max_lines_configs,
        &std::collections::HashMap::new(),
    )
}

fn is_exchange_turn_heading(trimmed: &str) -> bool {
    trimmed == "## User"
        || trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("## Re:")
}

fn preamble_belongs_to_exchange(preamble: &str) -> bool {
    let mut saw_nonempty = false;
    for line in preamble.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_nonempty = true;
        if trimmed.starts_with("[//]:")
            || trimmed.starts_with("<!--")
            || trimmed.starts_with('#')
            || trimmed.starts_with('-')
            || trimmed.starts_with('*')
            || trimmed.starts_with('>')
        {
            return false;
        }
    }
    saw_nonempty
}

fn is_non_exchange_section_heading(trimmed: &str) -> bool {
    let hashes = trimmed.chars().take_while(|&ch| ch == '#').count();
    if hashes == 0 || hashes > 3 || is_exchange_turn_heading(trimmed) {
        return false;
    }
    let rest = &trimmed[hashes..];
    rest.is_empty() || rest.starts_with(' ')
}

fn conversation_tail_start_in_range(
    doc: &str,
    search_start: usize,
    search_end: usize,
) -> Option<usize> {
    let code_ranges = component::find_code_ranges(doc);
    let comment_ranges = component::find_non_agent_html_comment_ranges(doc);
    let in_ignored_range = |pos: usize| {
        code_ranges
            .iter()
            .chain(comment_ranges.iter())
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let mut first_nonempty_after = None;
    let mut first_heading_start = None;
    let mut offset = 0usize;
    for line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if line_start < search_start || in_ignored_range(line_start) {
            continue;
        }
        if line_start >= search_end {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        first_nonempty_after.get_or_insert(line_start);
        if is_exchange_turn_heading(trimmed) {
            first_heading_start = Some(line_start);
            break;
        }
    }

    let heading_start = first_heading_start?;
    let first_nonempty_after = first_nonempty_after.unwrap_or(heading_start);
    if first_nonempty_after < heading_start
        && preamble_belongs_to_exchange(&doc[first_nonempty_after..heading_start])
    {
        Some(first_nonempty_after)
    } else {
        Some(heading_start)
    }
}

fn prompt_tail_range_in_region(
    doc: &str,
    search_start: usize,
    search_end: usize,
) -> Option<(usize, usize)> {
    let code_ranges = component::find_code_ranges(doc);
    let comment_ranges = component::find_non_agent_html_comment_ranges(doc);
    let in_ignored_range = |pos: usize| {
        code_ranges
            .iter()
            .chain(comment_ranges.iter())
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let mut prompt_start = None;
    let mut prompt_end = None;
    let mut offset = 0usize;

    for line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if line_start < search_start || in_ignored_range(line_start) {
            continue;
        }
        if line_start >= search_end {
            break;
        }

        let trimmed = line.trim();
        if prompt_start.is_none() {
            if trimmed.is_empty()
                || trimmed == "###"
                || trimmed.starts_with("<!--")
                || trimmed.starts_with("[//]:")
                || is_non_exchange_section_heading(trimmed)
            {
                continue;
            }
            if text_line_looks_like_prompt_target(trimmed) {
                prompt_start = Some(line_start);
                prompt_end = Some(offset);
                continue;
            }
            return None;
        }

        if trimmed.starts_with("<!--")
            || trimmed.starts_with("[//]:")
            || trimmed == "###"
            || is_non_exchange_section_heading(trimmed)
        {
            break;
        }
        prompt_end = Some(offset);
    }

    match (prompt_start, prompt_end) {
        (Some(start), Some(end)) if start < end => Some((start, end)),
        _ => None,
    }
}

fn escaped_prompt_range_outside_exchange(
    doc: &str,
    components: &[component::Component],
    exchange: &component::Component,
) -> Option<(usize, usize)> {
    let mut trailing_components: Vec<&component::Component> = components
        .iter()
        .filter(|c| c.open_start >= exchange.close_end)
        .collect();
    trailing_components.sort_by_key(|c| c.open_start);

    let mut search_start = exchange.close_end;
    for component in trailing_components {
        if search_start < component.open_start
            && let Some(range) =
                prompt_tail_range_in_region(doc, search_start, component.open_start)
        {
            return Some(range);
        }
        search_start = search_start.max(component.close_end);
    }

    if search_start < doc.len()
        && let Some(range) = prompt_tail_range_in_region(doc, search_start, doc.len())
    {
        return Some(range);
    }

    None
}

fn escaped_conversation_range_outside_exchange(
    doc: &str,
    components: &[component::Component],
    exchange: &component::Component,
) -> Option<(usize, usize)> {
    if let Some(range) = escaped_prompt_range_outside_exchange(doc, components, exchange) {
        return Some(range);
    }

    let mut trailing_components: Vec<&component::Component> = components
        .iter()
        .filter(|c| c.open_start >= exchange.close_end)
        .collect();
    trailing_components.sort_by_key(|c| c.open_start);

    let mut search_start = exchange.close_end;
    for component in trailing_components {
        if search_start < component.open_start
            && let Some(start) =
                conversation_tail_start_in_range(doc, search_start, component.open_start)
        {
            return Some((start, component.open_start));
        }
        search_start = search_start.max(component.close_end);
    }

    if search_start < doc.len() {
        if let Some(range) = prompt_tail_range_in_region(doc, search_start, doc.len()) {
            return Some(range);
        }
        if let Some(start) = conversation_tail_start_in_range(doc, search_start, doc.len()) {
            return Some((start, doc.len()));
        }
    }

    None
}

fn tail_is_safe_exchange_content(tail: &str) -> bool {
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 3usize;

    for line in tail.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
            || trimmed.starts_with("<!-- patch:")
            || trimmed.starts_with("<!-- /patch:")
        {
            return false;
        }
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                continue;
            }
        } else {
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }

        if trimmed.is_empty() || is_exchange_turn_heading(trimmed) {
            continue;
        }

        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if hashes > 0 {
            if (4..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
                continue;
            }
            return false;
        }
    }

    !in_fence
}

/// Fail closed when conversation content (prompts or response headings) would
/// land outside the live `<!-- agent:exchange -->` block. This is a write-path
/// guard — explicit `agent-doc repair` uses `repair_conversation_tail_outside_exchange`
/// instead.
pub fn guard_no_conversation_tail_outside_exchange(doc: &str) -> Result<()> {
    let components = component::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(());
    };

    let Some((tail_start, _tail_end)) =
        escaped_conversation_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(());
    };

    let tail = &doc[tail_start..];
    let first_line = tail.lines().next().unwrap_or("(empty)");
    anyhow::bail!(
        "prompt/response content would land outside `<!-- agent:exchange -->` — \
         the write path cannot place conversation content after the exchange close tag. \
         First escaped line: `{}`",
        first_line.chars().take(120).collect::<String>()
    );
}

/// Repair the safe malformed-template case where conversation content escaped
/// below `<!-- /agent:exchange -->` and now trails the document after sibling
/// components like pending/todo.
pub fn repair_conversation_tail_outside_exchange(doc: &str) -> Result<Option<String>> {
    let components = component::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };

    let Some((tail_start, tail_end)) =
        escaped_conversation_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(None);
    };

    let tail = &doc[tail_start..tail_end];
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the trailing document structure is ambiguous"
        );
    }

    let prefix = &doc[..tail_start];
    let suffix = &doc[tail_end..];
    let prefix_components = component::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during repair")?;
    let escaped = tail.trim();
    if escaped.is_empty() {
        return Ok(None);
    }

    let mut repaired = if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped, &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{escaped}\n")
        } else {
            format!("{}\n{}\n", exchange.content(prefix).trim_end(), escaped)
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(suffix);
    Ok(Some(repaired))
}

/// Repair only prompt-target drift that escaped below `<!-- /agent:exchange -->`
/// while leaving later markdown section separators outside the exchange block.
pub fn repair_prompt_tail_outside_exchange(doc: &str) -> Result<Option<String>> {
    let components = component::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };

    let Some((tail_start, tail_end)) =
        escaped_prompt_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(None);
    };

    let tail = &doc[tail_start..tail_end];
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "prompt content escaped `agent:exchange`, but the trailing document structure is ambiguous"
        );
    }

    let prefix = &doc[..tail_start];
    let suffix = &doc[tail_end..];
    let prefix_components = component::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during prompt repair")?;
    let escaped = tail.trim();
    if escaped.is_empty() {
        return Ok(None);
    }

    let mut repaired = if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped, &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{escaped}\n")
        } else {
            format!("{}\n{}\n", exchange.content(prefix).trim_end(), escaped)
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(suffix);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicate
/// `<!-- /agent:exchange -->` lands after escaped conversation content.
///
/// This shows up as `closing marker <!-- /agent:exchange --> without matching open`
/// even though the document still has the real opening exchange marker. When the
/// text between the first and second close markers is safe exchange content, move
/// that text back into the real exchange block and drop the stray second close.
pub fn repair_duplicate_exchange_close_tail(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let escaped = &doc[first_close_end..second_close_start];
    if !escaped.trim().is_empty() && !tail_is_safe_exchange_content(escaped) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the duplicate close repair suffix is ambiguous"
        );
    }

    let prefix = &doc[..first_close_end];
    let prefix_components = component::parse(prefix).context("failed to parse repair prefix")?;
    let exchange = prefix_components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during duplicate-close repair")?;

    let mut repaired = if escaped.trim().is_empty() {
        prefix.to_string()
    } else if let Some(boundary_id) = find_boundary_in_component(prefix, exchange) {
        exchange.append_with_boundary(prefix, escaped.trim(), &boundary_id)
    } else {
        let new_content = if exchange.content(prefix).trim().is_empty() {
            format!("{}\n", escaped.trim())
        } else {
            format!(
                "{}\n{}\n",
                exchange.content(prefix).trim_end(),
                escaped.trim()
            )
        };
        exchange.replace_content(prefix, &new_content)
    };

    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicate template scaffold is
/// inserted between two `<!-- /agent:exchange -->` close markers.
///
/// Unlike `repair_duplicate_exchange_close_tail`, the text between the two
/// close markers is not conversation content to move back into exchange. It is
/// a duplicated outer document scaffold (`###`, queue/backlog/done components,
/// etc.) that should be dropped while preserving the first close marker and
/// the real scaffold after the second close marker.
pub fn repair_duplicate_exchange_close_scaffold(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let duplicate_scaffold = &doc[first_close_end..second_close_start];
    if !is_safe_duplicate_template_scaffold(duplicate_scaffold) {
        return Ok(None);
    }

    let mut repaired = String::with_capacity(doc.len() - (second_close_end - first_close_end));
    repaired.push_str(&doc[..first_close_end]);
    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

/// Repair the malformed-template case where a duplicated scaffold contains
/// safe live conversation text before the stray second exchange close marker.
///
/// This is the mixed form of `repair_duplicate_exchange_close_scaffold`: the
/// duplicated queue/backlog/done scaffold should be dropped, but any ordinary
/// prompt text stranded in that duplicate segment still belongs in the live
/// exchange.
pub fn repair_duplicate_exchange_close_mixed_scaffold_tail(doc: &str) -> Result<Option<String>> {
    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";

    let Some(open_start) = doc.find(open_tag) else {
        return Ok(None);
    };
    let Some(open_end) = doc[open_start..]
        .find("-->")
        .map(|idx| open_start + idx + 3)
    else {
        return Ok(None);
    };
    let Some(first_close_start) = doc[open_end..].find(close_tag).map(|idx| open_end + idx) else {
        return Ok(None);
    };
    let first_close_end = first_close_start + close_tag.len();
    let Some(second_close_start) = doc[first_close_end..]
        .find(close_tag)
        .map(|idx| first_close_end + idx)
    else {
        return Ok(None);
    };
    let second_close_end = second_close_start + close_tag.len();

    let duplicate_segment = &doc[first_close_end..second_close_start];
    let Some(exchange_tail) = safe_exchange_tail_from_duplicate_scaffold(duplicate_segment)? else {
        return Ok(None);
    };

    let prefix = &doc[..first_close_end];
    let mut repaired = append_tail_to_exchange_end(prefix, &exchange_tail)?;
    repaired.push_str(&doc[second_close_end..]);
    Ok(Some(repaired))
}

fn safe_exchange_tail_from_duplicate_scaffold(segment: &str) -> Result<Option<String>> {
    let has_scaffold_component = segment.contains("<!-- agent:queue -->")
        || segment.contains("<!-- agent:backlog")
        || segment.contains("<!-- agent:pending")
        || segment.contains("<!-- agent:done");
    if !has_scaffold_component {
        return Ok(None);
    }

    let mut residue = segment.to_string();
    if let Ok(components) = component::parse(segment) {
        let mut ranges: Vec<(usize, usize)> = components
            .iter()
            .filter(|component| {
                matches!(
                    component.name.as_str(),
                    "queue" | "backlog" | "pending" | "icebox" | "done"
                )
            })
            .map(|component| (component.open_start, component.close_end))
            .collect();
        ranges.sort_by_key(|range| std::cmp::Reverse(range.0));
        for (start, end) in ranges {
            residue.replace_range(start..end, "");
        }
    }

    let residue = strip_html_comments(&residue);
    let tail = residue
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                None
            } else {
                Some(line.trim_end())
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(None);
    }
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the duplicate scaffold repair suffix is ambiguous"
        );
    }
    Ok(Some(tail.to_string()))
}

fn append_tail_to_exchange_end(prefix: &str, tail: &str) -> Result<String> {
    let prefix_without_boundaries = remove_all_boundaries(prefix);
    let components =
        component::parse(&prefix_without_boundaries).context("failed to parse repair prefix")?;
    let exchange = components
        .iter()
        .find(|c| c.name == "exchange")
        .context("exchange component disappeared during mixed duplicate-scaffold repair")?;

    let existing = exchange.content(&prefix_without_boundaries).trim_end();
    let mut new_content = String::new();
    if !existing.is_empty() {
        new_content.push_str(existing);
        new_content.push('\n');
    }
    new_content.push_str(tail.trim_end());
    new_content.push('\n');
    new_content.push_str(&crate::id::format_boundary_marker(
        &crate::id::new_boundary_id(),
    ));
    new_content.push('\n');
    Ok(exchange.replace_content(&prefix_without_boundaries, &new_content))
}

/// Normalize the structural template invariants required before an editor IPC
/// client mutates the visible document.
///
/// This is intentionally in the library layer so all editor integrations can
/// share the same duplicate-scaffold repair/fail-closed behavior as the binary
/// write path. Safe duplicate exchange-close scaffolds are dropped; ambiguous
/// mixed user text remains an error so the editor can refuse the visible write.
pub fn normalize_editor_visible_template_structure(doc: &str) -> Result<String> {
    let mut normalized = crate::component::strip_backlog_patch_attr(doc);
    // #queue-completed-items-escape-below-component: scrub struck queue items
    // that drifted below `<!-- /agent:queue -->` into the parking-lot comment so
    // the visible buffer never accumulates orphaned struck-queue residue.
    if let Some(repaired) = repair_queue_struck_items_escaped_below_marker(&normalized) {
        normalized = repaired;
    }
    while let Some(merged) = repair_duplicate_exchange_opener(&normalized)? {
        normalized = merged;
    }
    if let Some(cleaned) = remove_duplicate_answered_exchange_prompt_tail(&normalized) {
        normalized = cleaned;
    }
    guard_no_duplicate_prompt_residue_outside_exchange(&normalized)
        .context("template duplicate prompt residue guard failed")?;

    match guard_no_conversation_tail_outside_exchange(&normalized) {
        Ok(()) => Ok(normalized),
        Err(err)
            if err.chain().any(|cause| {
                cause
                    .to_string()
                    .contains("closing marker <!-- /agent:exchange --> without matching open")
            }) =>
        {
            if let Some(repaired) = repair_duplicate_exchange_close_scaffold(&normalized)? {
                guard_no_duplicate_prompt_residue_outside_exchange(&repaired)
                    .context("template duplicate prompt residue guard failed after duplicate-scaffold repair")?;
                guard_no_conversation_tail_outside_exchange(&repaired).with_context(
                    || "template structure guard failed after duplicate-scaffold repair",
                )?;
                return Ok(repaired);
            }
            if repair_duplicate_exchange_close_mixed_scaffold_tail(&normalized)?.is_some() {
                anyhow::bail!(
                    "mixed duplicate scaffold tail: live conversation text is interleaved with duplicated template scaffold; refusing automatic visible-document repair"
                );
            }
            if let Some(repaired) = repair_duplicate_exchange_close_tail(&normalized)? {
                guard_no_duplicate_prompt_residue_outside_exchange(&repaired).context(
                    "template duplicate prompt residue guard failed after duplicate-close repair",
                )?;
                guard_no_conversation_tail_outside_exchange(&repaired).with_context(
                    || "template structure guard failed after duplicate-close repair",
                )?;
                return Ok(repaired);
            }
            Err(err).context("template structure guard failed")
        }
        Err(err) => Err(err).context("template structure guard failed"),
    }
}

/// True when a line is a displaced struck queue item — a `- ~~…~~`
/// strikethrough whose body carries a queue-item marker (`:round_pushpin:` /
/// `:pushpin:`) or a `do`/`re` directive (`#queue-completed-items-escape-below-component`).
/// Generic user strikethrough scratch (no pushpin, no directive) is deliberately
/// NOT matched, so an ordinary `- ~~note~~` in the parking lot stays untouched.
fn is_displaced_struck_queue_item(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("- ") else {
        return false;
    };
    let rest = rest.trim();
    let Some(inner) = rest.strip_prefix("~~").and_then(|s| s.strip_suffix("~~")) else {
        return false;
    };
    let inner = inner.trim();
    inner.contains(":round_pushpin:")
        || inner.contains(":pushpin:")
        || inner.starts_with("do [#")
        || inner.starts_with("do #")
        || inner.starts_with("re [#")
        || inner.starts_with("re #")
}

/// Remove struck queue items (`- ~~…~~`) that drifted BELOW the `agent:queue`
/// closing marker into the inter-component parking lot
/// (`#queue-completed-items-escape-below-component`).
///
/// Struck queue items are completed: their canonical record is `agent:done` and
/// the live strike must stay inside the queue component span. A post-commit
/// CRDT/boundary merge can displace them past `<!-- /agent:queue -->` into the
/// neighbouring HTML comment (the "parking lot"), where they render invisibly
/// and accumulate as orphaned residue. This repair drops any displaced
/// struck-queue line that sits OUTSIDE every agent component span, leaving all
/// real component content (including legitimately struck items still inside the
/// queue) and ordinary scratch comments intact. Returns the repaired document
/// when it removed at least one displaced line.
pub fn repair_queue_struck_items_escaped_below_marker(doc: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let queue = components.iter().find(|c| c.name == "queue")?;
    let scan_start = queue.close_end;
    // Never edit content inside any agent component span.
    let protected: Vec<(usize, usize)> = components
        .iter()
        .map(|c| (c.open_start, c.close_end))
        .collect();

    let mut remove_ranges: Vec<(usize, usize)> = Vec::new();
    let mut offset = scan_start;
    for line in doc[scan_start..].split_inclusive('\n') {
        let line_start = offset;
        let line_end = offset + line.len();
        offset = line_end;
        let inside_component = protected
            .iter()
            .any(|(start, end)| line_start >= *start && line_end <= *end);
        if inside_component {
            continue;
        }
        if is_displaced_struck_queue_item(line) {
            remove_ranges.push((line_start, line_end));
        }
    }
    if remove_ranges.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(doc.len());
    let mut cursor = 0usize;
    for (start, end) in &remove_ranges {
        out.push_str(&doc[cursor..*start]);
        cursor = *end;
    }
    out.push_str(&doc[cursor..]);
    Some(out)
}

/// Remove ordinary HTML comments after `agent:exchange` when their body is a
/// duplicate or near-duplicate of prompt text already present in the exchange.
///
/// This keeps unrelated scratch comments user-owned while cleaning stale editor
/// residue such as a hidden previous copy of the current prompt.
pub fn remove_post_exchange_duplicate_prompt_comments(doc: &str) -> Option<String> {
    remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[])
}

/// Like `remove_post_exchange_duplicate_prompt_comments`, but keeps duplicate-
/// looking comment lines that were already present in `preserve_doc`.
///
/// Closeout, route, and preflight use this as an ownership proof: stale
/// duplicate residue created during the current response cycle can be scrubbed,
/// while pre-existing scratch comments below `agent:exchange` remain user-owned.
pub fn remove_post_exchange_duplicate_prompt_comments_preserving(
    doc: &str,
    preserve_doc: Option<&str>,
) -> Option<String> {
    match preserve_doc {
        Some(preserve_doc) => {
            remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[preserve_doc])
        }
        None => remove_post_exchange_duplicate_prompt_comments_preserving_docs(doc, &[]),
    }
}

/// Like `remove_post_exchange_duplicate_prompt_comments_preserving`, but accepts
/// several ownership-proof documents and preserves the union of their ordinary
/// post-exchange comment lines.
pub fn remove_post_exchange_duplicate_prompt_comments_preserving_docs(
    doc: &str,
    preserve_docs: &[&str],
) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let prompts = exchange_prompt_comment_targets(exchange.content(doc));
    if prompts.is_empty() {
        return None;
    }

    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();
    let mut preserved_comment_lines = HashSet::new();
    for preserve_doc in preserve_docs {
        preserved_comment_lines.extend(post_exchange_comment_line_preserve_set(preserve_doc));
    }

    let mut replacements = Vec::<(usize, usize, String)>::new();
    for (start, end) in component::find_non_agent_html_comment_ranges(doc) {
        if start < exchange.close_end {
            continue;
        }
        if protected_ranges
            .iter()
            .any(|(protected_start, protected_end)| {
                start >= *protected_start && end <= *protected_end
            })
        {
            continue;
        }
        let original = &doc[start..end];
        if !original.ends_with("-->") {
            continue;
        }
        let body = &doc[start + 4..end - 3];
        let Some(cleaned_body) =
            strip_duplicate_prompt_comment_body(body, &prompts, &preserved_comment_lines)
        else {
            replacements.push((start, end, empty_html_comment_like(body)));
            continue;
        };
        if cleaned_body != body {
            replacements.push((start, end, format!("<!--{}-->", cleaned_body)));
        }
    }

    if replacements.is_empty() {
        return None;
    }

    let mut cleaned = doc.to_string();
    for (start, end, replacement) in replacements.into_iter().rev() {
        cleaned.replace_range(start..end, &replacement);
    }
    Some(cleaned)
}

fn post_exchange_comment_line_preserve_set(doc: &str) -> HashSet<String> {
    let Ok(components) = component::parse(doc) else {
        return HashSet::new();
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return HashSet::new();
    };
    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();

    let mut preserved = HashSet::new();
    for (start, end) in component::find_non_agent_html_comment_ranges(doc) {
        if start < exchange.close_end {
            continue;
        }
        if protected_ranges
            .iter()
            .any(|(protected_start, protected_end)| {
                start >= *protected_start && end <= *protected_end
            })
        {
            continue;
        }
        let original = &doc[start..end];
        if !original.ends_with("-->") {
            continue;
        }
        let body = &doc[start + 4..end - 3];
        for line in comment_body_lines(body) {
            if let Some(normalized) = normalize_prompt_comment_text(line) {
                preserved.insert(normalized);
            }
        }
    }
    preserved
}

/// Remove a prompt tail after the latest exchange boundary when that tail is an
/// exact duplicate of a prompt block already answered earlier in the exchange.
///
/// This covers delayed route/editor replay that re-adds the just-answered
/// prompt after closeout. The cleanup is intentionally narrow: every
/// non-comment tail line must (a) carry the `❯ ` answered-form marker — proof it
/// is a copy of an answered prompt rather than a freshly-typed live prompt — and
/// (b) match a contiguous prompt block immediately before an existing response
/// heading. A bare unprefixed post-boundary prompt is always preserved.
pub fn remove_duplicate_answered_exchange_prompt_tail(doc: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    let exchange_content = exchange.content(doc);
    let duplicate_start = duplicate_answered_exchange_prompt_tail_start(exchange_content)?;

    let mut cleaned_exchange = exchange_content[..duplicate_start].to_string();
    if !cleaned_exchange.ends_with('\n') {
        cleaned_exchange.push('\n');
    }
    Some(exchange.replace_content(doc, &cleaned_exchange))
}

fn duplicate_answered_exchange_prompt_tail_start(exchange: &str) -> Option<usize> {
    let lines = exchange_line_spans(exchange);
    let boundary_idx = lines
        .iter()
        .rposition(|(_, _, line)| line.trim().starts_with("<!-- agent:boundary:"))?;
    let tail_start = lines
        .get(boundary_idx)
        .map(|(_, end, _)| *end)
        .unwrap_or(exchange.len());
    let tail = duplicate_exchange_tail_prompt_lines(&lines[boundary_idx + 1..])?;
    if answered_exchange_prompt_blocks_before_boundary(&lines, boundary_idx)
        .into_iter()
        .any(|block| block == tail)
    {
        Some(tail_start)
    } else {
        None
    }
}

fn exchange_line_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut offset = 0usize;
    for segment in text.split_inclusive('\n') {
        let start = offset;
        offset += segment.len();
        spans.push((start, offset, segment));
    }
    if spans.is_empty() && !text.is_empty() {
        spans.push((0, text.len(), text));
    }
    spans
}

fn duplicate_exchange_tail_prompt_lines(lines: &[(usize, usize, &str)]) -> Option<Vec<String>> {
    let mut prompt_lines = Vec::new();
    for (_, _, line) in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if is_exchange_turn_heading(trimmed) {
            return None;
        }
        // Ownership proof: only an answered-form line (carrying the `❯ ` prompt
        // marker) can be delayed-replay residue, because the marker is added by
        // the answer/normalize cycle, never by a user typing a fresh prompt. A
        // bare, unprefixed post-boundary line is a LIVE prompt the user just
        // typed — even when its text matches a previously-answered prompt (e.g.
        // a re-typed "go"/"yes"/"continue") — and must never be scrubbed.
        // Without this guard the text-only match silently ate live prompts
        // (#ipcfullprompt-recur: "go" on monsterrodholders.md).
        if !trimmed.starts_with('❯') {
            return None;
        }
        prompt_lines.push(normalize_duplicate_exchange_prompt_line(trimmed)?);
    }
    if prompt_lines.is_empty() {
        None
    } else {
        Some(prompt_lines)
    }
}

fn answered_exchange_prompt_blocks_before_boundary(
    lines: &[(usize, usize, &str)],
    boundary_idx: usize,
) -> Vec<Vec<String>> {
    let mut blocks = Vec::new();
    for response_idx in 0..boundary_idx {
        if !is_exchange_turn_heading(lines[response_idx].2.trim()) {
            continue;
        }
        let mut block = Vec::new();
        let mut cursor = response_idx;
        while cursor > 0 {
            cursor -= 1;
            let trimmed = lines[cursor].2.trim();
            if trimmed.is_empty() || trimmed.starts_with("<!--") {
                if block.is_empty() {
                    continue;
                }
                break;
            }
            if trimmed.starts_with('❯')
                && let Some(normalized) = normalize_duplicate_exchange_prompt_line(trimmed)
            {
                block.push(normalized);
                continue;
            }
            if block.is_empty()
                && looks_like_prompt_comment_target(trimmed)
                && let Some(normalized) = normalize_duplicate_exchange_prompt_line(trimmed)
            {
                block.push(normalized);
                continue;
            }
            break;
        }
        if !block.is_empty() {
            block.reverse();
            blocks.push(block);
        }
    }
    blocks
}

fn normalize_duplicate_exchange_prompt_line(line: &str) -> Option<String> {
    let unprefixed = line.trim().strip_prefix('❯').unwrap_or(line.trim()).trim();
    let normalized = unprefixed.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Fail closed when prompt text already recorded in `agent:exchange` still
/// appears as freeform Markdown after the exchange close marker.
///
/// Ordinary post-exchange HTML comments are scrubbed by
/// `remove_post_exchange_duplicate_prompt_comments`; tracked components such as
/// backlog/queue have their own mutation rules. Anything else is ambiguous
/// enough that closeout must not silently commit or dispatch it.
pub fn guard_no_duplicate_prompt_residue_outside_exchange(doc: &str) -> Result<()> {
    let components = match component::parse(doc) {
        Ok(components) => components,
        Err(err)
            if err
                .chain()
                .any(|cause| cause.to_string().contains("without matching open")) =>
        {
            return Ok(());
        }
        Err(err) => return Err(err).context("failed to parse components"),
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return Ok(());
    };
    let prompts = exchange_prompt_comment_targets(exchange.content(doc));
    if prompts.is_empty() {
        return Ok(());
    }

    let protected_ranges = components
        .iter()
        .filter(|component| component.name != "exchange")
        .map(|component| (component.open_start, component.close_end))
        .collect::<Vec<_>>();
    let in_protected_component = |pos: usize| {
        protected_ranges
            .iter()
            .any(|(start, end)| pos >= *start && pos < *end)
    };
    let ordinary_comment_ranges = component::find_non_agent_html_comment_ranges(doc);
    let in_ordinary_comment = |pos: usize| {
        ordinary_comment_ranges
            .iter()
            .any(|(start, end)| pos >= *start && pos < *end)
    };

    let mut offset = 0usize;
    for segment in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += segment.len();
        if line_start < exchange.close_end
            || in_protected_component(line_start)
            || in_ordinary_comment(line_start)
        {
            continue;
        }
        let trimmed = segment.trim();
        if !is_duplicate_prompt_comment_text(trimmed, &prompts) {
            continue;
        }
        anyhow::bail!(
            "duplicate prompt residue outside `<!-- agent:exchange -->`; refusing to commit or dispatch ambiguous Markdown tail. First duplicate line: `{}`",
            trimmed.chars().take(120).collect::<String>()
        );
    }

    Ok(())
}

fn empty_html_comment_like(body: &str) -> String {
    if body.contains('\n') {
        "<!--\n-->".to_string()
    } else {
        "<!-- -->".to_string()
    }
}

fn strip_duplicate_prompt_comment_body(
    body: &str,
    prompts: &[String],
    preserved_comment_lines: &HashSet<String>,
) -> Option<String> {
    if !body.contains('\n') && is_duplicate_prompt_comment_text(body, prompts) {
        if normalized_comment_line_is_preserved(body, preserved_comment_lines) {
            return Some(body.to_string());
        }
        return None;
    }

    let mut changed = false;
    let mut cleaned = String::new();
    for segment in body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if is_duplicate_prompt_comment_text(line, prompts) {
            if normalized_comment_line_is_preserved(line, preserved_comment_lines) {
                cleaned.push_str(segment);
                continue;
            }
            changed = true;
            continue;
        }
        cleaned.push_str(segment);
    }

    if !changed {
        return Some(body.to_string());
    }
    if cleaned.trim().is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn comment_body_lines(body: &str) -> Vec<&str> {
    if body.contains('\n') {
        body.split_inclusive('\n')
            .map(|segment| segment.strip_suffix('\n').unwrap_or(segment))
            .collect()
    } else {
        vec![body]
    }
}

fn normalized_comment_line_is_preserved(
    line: &str,
    preserved_comment_lines: &HashSet<String>,
) -> bool {
    normalize_prompt_comment_text(line)
        .map(|normalized| preserved_comment_lines.contains(&normalized))
        .unwrap_or(false)
}

fn exchange_prompt_comment_targets(exchange: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut in_response_block = false;
    let mut in_fence = false;

    for line in exchange.lines() {
        let trimmed = line.trim();
        if is_fence_delimiter(trimmed) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.starts_with("<!-- agent:boundary:") {
            in_response_block = false;
            continue;
        }
        if trimmed.starts_with("### Re:") || trimmed.starts_with("## Assistant") {
            in_response_block = true;
            continue;
        }
        if in_response_block {
            continue;
        }
        let Some(normalized) = normalize_prompt_comment_text(trimmed) else {
            continue;
        };
        if seen.insert(normalized.clone()) {
            targets.push(normalized);
        }
    }

    targets
}

fn looks_like_prompt_comment_target(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    trimmed.starts_with('❯')
        || trimmed.contains('#')
        || trimmed.ends_with('?')
        || lower.starts_with("do ")
        || lower.starts_with("fix ")
        || lower.starts_with("run ")
        || lower.starts_with("please ")
        || lower.contains(" spec-test-")
        || lower.contains(" reproduce ")
}

fn normalize_prompt_comment_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading(trimmed)
    {
        return None;
    }
    let unprefixed = trimmed.strip_prefix('❯').unwrap_or(trimmed).trim();
    let collapsed = unprefixed.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() < 32 {
        None
    } else {
        Some(collapsed)
    }
}

fn is_markdown_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

fn is_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

fn is_duplicate_prompt_comment_text(candidate: &str, prompts: &[String]) -> bool {
    let Some(candidate) = normalize_prompt_comment_text(candidate) else {
        return false;
    };
    let candidate_lower = candidate.to_lowercase();
    let candidate_tokens = prompt_comment_tokens(&candidate);
    if candidate_tokens.len() < 8 {
        return false;
    }

    prompts.iter().any(|prompt| {
        let prompt_lower = prompt.to_lowercase();
        if candidate_lower == prompt_lower
            || prompt_lower.contains(&candidate_lower)
            || candidate_lower.contains(&prompt_lower)
        {
            return true;
        }

        let prompt_tokens = prompt_comment_tokens(prompt);
        if prompt_tokens.len() < 8 {
            return false;
        }
        ordered_token_coverage(&candidate_tokens, &prompt_tokens) >= 0.85
            || ordered_token_coverage(&prompt_tokens, &candidate_tokens) >= 0.85
    })
}

fn prompt_comment_tokens(text: &str) -> Vec<String> {
    text.split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

fn ordered_token_coverage(needle: &[String], haystack: &[String]) -> f64 {
    if needle.is_empty() {
        return 0.0;
    }
    let mut matched = 0usize;
    for token in haystack {
        if needle.get(matched).is_some_and(|needle| needle == token) {
            matched += 1;
            if matched == needle.len() {
                break;
            }
        }
    }
    matched as f64 / needle.len() as f64
}

fn is_safe_duplicate_template_scaffold(segment: &str) -> bool {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("### Re:")
        || trimmed.contains("## User")
        || trimmed.contains("## Assistant")
        || trimmed.contains("❯ ")
    {
        return false;
    }

    let has_scaffold_component = trimmed.contains("<!-- agent:queue -->")
        || trimmed.contains("<!-- agent:backlog")
        || trimmed.contains("<!-- agent:pending")
        || trimmed.contains("<!-- agent:done");
    if !has_scaffold_component {
        return false;
    }
    if !duplicate_scaffold_has_only_structural_residue(trimmed) {
        return false;
    }

    let wrapped = format!("<!-- agent:scaffold -->\n{trimmed}\n<!-- /agent:scaffold -->\n");
    let Ok(components) = component::parse(&wrapped) else {
        return false;
    };
    let allowed = ["scaffold", "queue", "backlog", "pending", "icebox", "done"];
    components
        .iter()
        .all(|component| allowed.contains(&component.name.as_str()))
}

fn duplicate_scaffold_has_only_structural_residue(segment: &str) -> bool {
    let mut residue = segment.to_string();
    if let Ok(components) = component::parse(segment) {
        let mut ranges: Vec<(usize, usize)> = components
            .iter()
            .filter(|component| {
                matches!(
                    component.name.as_str(),
                    "queue" | "backlog" | "pending" | "icebox" | "done"
                )
            })
            .map(|component| (component.open_start, component.close_end))
            .collect();
        ranges.sort_by_key(|range| std::cmp::Reverse(range.0));
        for (start, end) in ranges {
            residue.replace_range(start..end, "");
        }
    }

    let residue = strip_html_comments(&residue);
    residue.lines().all(|line| {
        let trimmed = line.trim();
        trimmed.is_empty() || trimmed.starts_with('#')
    })
}

fn strip_html_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(start) = rest.find("<!--") else {
            result.push_str(rest);
            break;
        };
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 4..];
        let Some(end) = after_start.find("-->") else {
            break;
        };
        rest = &after_start[end + 3..];
    }
    result
}

/// Merge duplicate `<!-- agent:exchange -->` openers into a single exchange block.
///
/// When a template/CRDT document gains two complete exchange components (each with
/// its own opener and closer), this function merges the second block's content into
/// the first and removes the second block entirely. Returns `None` if the document
/// has zero or one exchange component.
pub fn repair_duplicate_exchange_opener(doc: &str) -> Result<Option<String>> {
    let components = match component::parse(doc) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let exchanges: Vec<&Component> = components.iter().filter(|c| c.name == "exchange").collect();
    if exchanges.len() < 2 {
        return Ok(None);
    }
    let first = exchanges[0];
    let second = exchanges[1];

    let first_content = first.content(doc).trim_end().to_string();
    let second_content = second.content(doc).trim().to_string();

    let merged = if first_content.trim().is_empty() && second_content.is_empty() {
        "\n".to_string()
    } else if first_content.trim().is_empty() {
        format!("{}\n", second_content)
    } else if second_content.is_empty() {
        format!("{}\n", first_content)
    } else {
        format!("{}\n{}\n", first_content, second_content)
    };

    let mut result = String::new();
    result.push_str(&doc[..first.open_end]);
    result.push_str(&merged);
    result.push_str(&doc[first.close_start..second.open_start]);
    result.push_str(&doc[second.close_end..]);

    if exchanges.len() > 2 {
        eprintln!(
            "[template] {} exchange components found — merged first two, {} remain",
            exchanges.len(),
            exchanges.len() - 1
        );
    }

    Ok(Some(result))
}

/// Remove a safe escaped conversation tail below `<!-- /agent:exchange -->`.
///
/// This is used when the user manually deletes a malformed trailing assistant/user
/// block and recovery should respect that edit instead of reapplying a stale
/// captured response.
pub fn strip_conversation_tail_outside_exchange(doc: &str) -> Result<Option<String>> {
    let components = component::parse(doc).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };

    let Some((tail_start, tail_end)) =
        escaped_conversation_range_outside_exchange(doc, &components, exchange)
    else {
        return Ok(None);
    };
    let tail = &doc[tail_start..tail_end];
    if !tail_is_safe_exchange_content(tail) {
        anyhow::bail!(
            "conversation content escaped `agent:exchange`, but the trailing document structure is ambiguous"
        );
    }

    let mut stripped = String::with_capacity(doc.len() - (tail_end - tail_start));
    stripped.push_str(&doc[..tail_start]);
    stripped.push_str(&doc[tail_end..]);
    Ok(Some(stripped))
}

/// Return `after` when it is exactly `before` with one safe escaped exchange
/// tail outside `agent:exchange` deleted.
pub fn deleted_conversation_tail_cleanup(before: &str, after: &str) -> Result<Option<String>> {
    if before == after || after.len() >= before.len() {
        return Ok(None);
    }

    if let Some(stripped) = strip_conversation_tail_outside_exchange(before)?
        && stripped == after
    {
        return Ok(Some(stripped));
    }

    let Some(deleted) = single_deleted_range(before, after) else {
        return Ok(None);
    };

    let components = component::parse(before).context("failed to parse components")?;
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return Ok(None);
    };
    if deleted.start < exchange.close_end {
        return Ok(None);
    }
    if components
        .iter()
        .any(|component| component.open_start < deleted.end && component.close_end > deleted.start)
    {
        return Ok(None);
    }

    let tail = &before[deleted.clone()];
    if !tail_is_safe_exchange_content(tail) || !tail_contains_exchange_signal(tail) {
        return Ok(None);
    }

    Ok(Some(after.to_string()))
}

fn single_deleted_range(before: &str, after: &str) -> Option<std::ops::Range<usize>> {
    let prefix = common_prefix_boundary_len(before, after);
    let mut before_end = before.len();
    let mut after_end = after.len();

    while before_end > prefix && after_end > prefix {
        let (before_prev, before_ch) = previous_char(before, before_end)?;
        let (after_prev, after_ch) = previous_char(after, after_end)?;
        if before_ch != after_ch {
            break;
        }
        before_end = before_prev;
        after_end = after_prev;
    }

    if prefix >= before_end {
        return None;
    }

    let mut reconstructed = String::with_capacity(after.len());
    reconstructed.push_str(&before[..prefix]);
    reconstructed.push_str(&before[before_end..]);
    if reconstructed == after {
        Some(prefix..before_end)
    } else {
        None
    }
}

fn common_prefix_boundary_len(a: &str, b: &str) -> usize {
    let mut len = 0usize;
    for ((a_idx, a_ch), (_b_idx, b_ch)) in a.char_indices().zip(b.char_indices()) {
        if a_ch != b_ch {
            break;
        }
        len = a_idx + a_ch.len_utf8();
    }
    len
}

fn previous_char(s: &str, end: usize) -> Option<(usize, char)> {
    s.get(..end)?.char_indices().next_back()
}

fn tail_contains_exchange_signal(tail: &str) -> bool {
    tail.lines().any(|line| {
        let trimmed = line.trim();
        is_exchange_turn_heading(trimmed)
            || text_line_looks_like_prompt_target(trimmed)
            || line_looks_like_prompt_preset_reference(trimmed)
    })
}

fn line_looks_like_prompt_preset_reference(line: &str) -> bool {
    let trimmed = line
        .trim_start_matches('❯')
        .trim_start()
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim_start_matches("+ ")
        .trim_start();
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let id_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    id_len > 0
        && rest[id_len..]
            .chars()
            .next()
            .is_none_or(|ch| ch.is_whitespace())
}

/// Strip trailing bare `❯` lines from exchange-bound content.
///
/// `❯` is the user's submit-prompt glyph. When an agent ends a response with a bare
/// `❯` line, the post-patch boundary marker lands directly under it, producing a
/// phantom prompt row on every cycle. This is a code-enforced invariant (see
/// `runbooks/code-enforced-directives.md`): the binary strips trailing bare-`❯`
/// lines so agents cannot produce the bug even if they forget the rule.
///
/// Only strips lines that contain nothing but `❯` and whitespace. `❯` appearing
/// inside content lines (e.g. `❯ How do I…`) is preserved. Multiple trailing bare
/// lines collapse to zero.
pub(crate) fn strip_trailing_caret_lines(content: &str) -> String {
    let trailing_nl = content.ends_with('\n');
    let mut lines: Vec<&str> = content.split('\n').collect();
    // split('\n') on a trailing-newline string yields an empty final element; ignore
    // it so we consider only real trailing lines.
    if trailing_nl {
        lines.pop();
    }
    while let Some(last) = lines.last() {
        let t = last.trim();
        if t == "❯" {
            lines.pop();
        } else {
            break;
        }
    }
    let mut out = lines.join("\n");
    if trailing_nl {
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone)]
struct PromptTailBlock {
    text: String,
    end: usize,
    pending_ids: HashSet<String>,
}

fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
}

fn extract_pending_ids(text: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let chars: Vec<char> = text.chars().collect();
    let mut idx = 0usize;
    while idx < chars.len() {
        if chars[idx] != '#' {
            idx += 1;
            continue;
        }
        let start = idx + 1;
        let mut end = start;
        while end < chars.len() {
            let ch = chars[end];
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                end += 1;
            } else {
                break;
            }
        }
        if end > start {
            let id = chars[start..end].iter().collect::<String>();
            if is_valid_pending_like_id(&id) {
                ids.insert(id);
            }
        }
        idx = end.max(idx + 1);
    }
    ids
}

fn is_valid_pending_like_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn text_line_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !is_exchange_response_heading(trimmed)
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || looks_like_prompt_directive(trimmed))
}

fn looks_like_prompt_directive(line: &str) -> bool {
    let lower = line.trim_start_matches('❯').trim().to_ascii_lowercase();
    lower == "go"
        || lower == "continue"
        || lower.starts_with("do #")
        || lower.starts_with("do [#")
        || lower.starts_with("fix #")
        || lower.starts_with("run ")
        || lower.starts_with("rerun ")
        || lower.starts_with("build ")
        || lower.starts_with("test ")
        || lower.starts_with("commit ")
        || lower.starts_with("push ")
        || lower.starts_with("verify ")
        || lower.starts_with("investigate ")
}

fn unresolved_tail_after_boundary(doc: &str, component_name: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let comp = components.iter().find(|comp| comp.name == component_name)?;
    let prefix = "<!-- agent:boundary:";
    let content_region = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);
    let mut search_from = 0usize;
    while let Some(start) = content_region[search_from..].find(prefix) {
        let abs_start = comp.open_end + search_from + start;
        if code_ranges
            .iter()
            .any(|&(cs, ce)| abs_start >= cs && abs_start < ce)
        {
            search_from += start + prefix.len();
            continue;
        }
        let line_start = search_from + start;
        let marker_end = content_region[line_start..]
            .find('\n')
            .map(|idx| line_start + idx + 1)
            .unwrap_or(content_region.len());
        return Some(content_region[marker_end..].to_string());
    }
    None
}

fn extract_prompt_tail_blocks(text: &str) -> Vec<PromptTailBlock> {
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fence_char).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    fn block_start(line: &str) -> bool {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed.starts_with("<!--")
            && !trimmed.starts_with("```")
            && !trimmed.starts_with("~~~")
            && !is_exchange_response_heading(trimmed)
            && text_line_looks_like_prompt_target(trimmed)
    }

    fn push_block(blocks: &mut Vec<PromptTailBlock>, text: &str, start: Option<usize>, end: usize) {
        let Some(start) = start else {
            return;
        };
        let raw = &text[start..end];
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        blocks.push(PromptTailBlock {
            text: trimmed.to_string(),
            end,
            pending_ids: extract_pending_ids(trimmed),
        });
    }

    let mut blocks = Vec::new();
    let mut current_start = None;
    let mut current_end = 0usize;
    let mut current_has_blank = false;
    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 3usize;
    let mut offset = 0usize;

    for segment in text.split_inclusive('\n') {
        let line = segment.trim_end_matches('\n');
        let trimmed = line.trim();
        let starts_new = current_start.is_some()
            && !in_fence
            && ((current_has_blank && !trimmed.is_empty()) || trimmed.starts_with('❯'))
            && block_start(line);
        if starts_new {
            push_block(&mut blocks, text, current_start.take(), current_end);
            current_end = 0;
            current_has_blank = false;
        }

        if current_start.is_none() {
            if !block_start(line) {
                offset += segment.len();
                continue;
            }
            current_start = Some(offset);
            current_has_blank = false;
        }

        current_end = offset + segment.len();
        if trimmed.is_empty() {
            current_has_blank = true;
        }

        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
            }
        } else if fence_close(trimmed, fence_char, fence_len) {
            in_fence = false;
        }

        offset += segment.len();
    }

    push_block(&mut blocks, text, current_start, current_end);
    blocks
}

fn find_tail_anchor_block<'a>(
    response_content: &str,
    prompt_blocks: &'a [PromptTailBlock],
) -> Result<Option<&'a PromptTailBlock>> {
    if prompt_blocks.is_empty() {
        return Ok(None);
    }

    let response_ids = extract_pending_ids(response_content);
    let candidate_idx = if response_ids.is_empty() {
        0
    } else if let Some(idx) = prompt_blocks
        .iter()
        .position(|block| !block.pending_ids.is_disjoint(&response_ids))
    {
        idx
    } else if prompt_blocks.len() == 1 {
        0
    } else {
        anyhow::bail!(
            "exchange patchback is ambiguous: response references pending ids {:?}, but none of the unresolved prompt blocks match",
            response_ids
        );
    };

    if candidate_idx > 0 {
        anyhow::bail!(
            "exchange patchback would skip older unresolved prompt(s): oldest pending block is `{}`, but the response matches later block `{}`",
            prompt_blocks[0].text.lines().next().unwrap_or("(empty)"),
            prompt_blocks[candidate_idx]
                .text
                .lines()
                .next()
                .unwrap_or("(empty)")
        );
    }

    Ok(prompt_blocks.get(candidate_idx))
}

fn append_exchange_patch_after_prompt_anchor(
    original_doc: &str,
    current_doc: &str,
    comp: &Component,
    patch_content: &str,
) -> Result<Option<String>> {
    let Some(original_tail) = unresolved_tail_after_boundary(original_doc, &comp.name) else {
        return Ok(None);
    };
    let prompt_blocks = extract_prompt_tail_blocks(&original_tail);
    let Some(anchor) = find_tail_anchor_block(patch_content, &prompt_blocks)? else {
        return Ok(None);
    };

    let content_region = comp.content(current_doc);
    let boundary_marker = find_boundary_in_component(current_doc, comp)
        .map(|id| format!("<!-- agent:boundary:{id} -->"))
        .context("exchange append anchor missing boundary marker")?;
    let boundary_pos = content_region
        .find(&boundary_marker)
        .context("exchange append anchor boundary marker not found in current content")?;
    let boundary_line_end = content_region[boundary_pos..]
        .find('\n')
        .map(|idx| boundary_pos + idx + 1)
        .unwrap_or(content_region.len());
    let user_region = &content_region[..boundary_pos];
    // `#patchback-prompt-edit-resilience`: the exact-match locate is the fast
    // path, but it fails when the prompt text drifted between the baseline
    // (`original_doc`) and the current document — e.g. the operator edits the
    // prompt (a typo fix, or adding/removing the `❯ ` prefix) after preflight
    // captured the baseline. Rather than fail closed (which forces manual
    // baseline-drop + prefix-repair retries), fall back to position-based
    // anchoring when the unresolved tail is a single prompt block: the boundary
    // has already been repositioned to the end of the component, so the sole
    // (possibly edited) prompt is the trailing content of the user region —
    // anchor the response at its end. Only the ambiguous multi-prompt-edit case
    // fails closed, now with an actionable refresh-the-baseline diagnostic.
    let insert_at = match user_region.rfind(&original_tail) {
        Some(tail_start) => tail_start + anchor.end,
        None if prompt_blocks.len() == 1 => user_region.trim_end().len(),
        None => anyhow::bail!(
            "exchange patchback prompt drifted from the baseline ({} unresolved prompt block(s) in the current tail); re-run `agent-doc preflight` to refresh the baseline before closeout. anchor: {}",
            prompt_blocks.len(),
            anchor.text.lines().next().unwrap_or("(empty)")
        ),
    };

    let new_id = crate::id::new_boundary_id();
    let new_marker = crate::id::format_boundary_marker(&new_id);
    let mut new_content =
        String::with_capacity(content_region.len() + patch_content.len() + new_marker.len() + 2);
    new_content.push_str(&user_region[..insert_at]);
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(patch_content.trim_end());
    new_content.push('\n');
    new_content.push_str(&new_marker);
    new_content.push('\n');
    new_content.push_str(&user_region[insert_at..]);
    new_content.push_str(&content_region[boundary_line_end..]);

    Ok(Some(comp.replace_content(current_doc, &new_content)))
}

/// Apply patches with per-component mode overrides (e.g., stream mode forces "replace"
/// for cumulative buffers even on append-mode components like exchange).
///
/// Pure: callers pre-load `component_configs` and `max_lines_configs` from
/// `.agent-doc/config.toml` (see `agent_doc::template_io` in the main crate
/// for the file-based wrapper).
pub fn apply_patches_with_overrides_pure(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    summary: Option<&str>,
    component_configs: &std::collections::HashMap<String, String>,
    max_lines_configs: &std::collections::HashMap<String, usize>,
    mode_overrides: &std::collections::HashMap<String, String>,
) -> Result<String> {
    // Pre-patch: ensure a fresh boundary exists in the exchange component.
    // Remove any stale boundaries from previous cycles, then insert a new one
    // at the end of the exchange. This is deterministic — belongs in the binary,
    // not the SKILL workflow.
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = crate::id::new_boundary_id_with_summary(summary);
        let marker = crate::id::format_boundary_marker(&id);
        let content = exchange.content(&result);
        let new_content = format!("{}\n{}\n", content.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
        eprintln!(
            "[template] pre-patch boundary {} inserted at end of exchange",
            id
        );
    }

    // Apply patches in reverse order (by position) to preserve byte offsets
    let components = component::parse(&result).context("failed to parse components")?;

    // Component configs were preloaded by the caller.
    let configs = component_configs;

    // Build a list of (component_index, patch) pairs, sorted by component position descending.
    // Patches targeting missing components are collected as overflow and routed to
    // exchange/output (same as unmatched content) — this avoids silent failures when
    // the agent uses a wrong component name.
    let mut ops: Vec<(usize, &PatchBlock)> = Vec::new();
    let mut overflow = String::new();
    for patch in patches {
        if let Some(idx) = components.iter().position(|c| c.name == patch.name) {
            ops.push((idx, patch));
        } else {
            let available: Vec<&str> = components.iter().map(|c| c.name.as_str()).collect();
            eprintln!(
                "[template] patch target '{}' not found, routing to exchange/output. Available: {}",
                patch.name,
                available.join(", ")
            );
            if !overflow.is_empty() {
                overflow.push('\n');
            }
            overflow.push_str(&patch.content);
        }
    }

    // Sort by position descending so replacements don't shift earlier offsets
    ops.sort_by_key(|op| std::cmp::Reverse(op.0));

    for (idx, patch) in &ops {
        let comp = &components[*idx];
        // Mode precedence: stream overrides > inline attr > config.toml ([components] section) > built-in default
        let mode = mode_overrides
            .get(&patch.name)
            .map(|s| s.as_str())
            .or_else(|| comp.patch_mode())
            .or_else(|| configs.get(&patch.name).map(|s| s.as_str()))
            .unwrap_or_else(|| default_mode(&patch.name));
        // Strip trailing bare `❯` lines for exchange-bound patches so a phantom
        // prompt row never lands above the post-patch boundary marker.
        let patch_content: std::borrow::Cow<'_, str> = if patch.name == "exchange" {
            std::borrow::Cow::Owned(strip_trailing_caret_lines(&patch.content))
        } else {
            std::borrow::Cow::Borrowed(patch.content.as_str())
        };
        // For append mode, use boundary-aware insertion when a marker exists
        if mode == "append"
            && let Some(bid) = find_boundary_in_component(&result, comp)
        {
            if patch.name == "exchange"
                && patch_content
                    .lines()
                    .any(|line| is_exchange_response_heading(line.trim_start()))
                && let Some(anchored) =
                    append_exchange_patch_after_prompt_anchor(doc, &result, comp, &patch_content)?
            {
                result = anchored;
                continue;
            }
            result = comp.append_with_boundary(&result, &patch_content, &bid);
            continue;
        }
        let new_content = apply_mode(mode, comp.content(&result), &patch_content);
        result = comp.replace_content(&result, &new_content);
    }

    // Merge overflow (from missing-component patches) with unmatched content
    let mut all_unmatched = String::new();
    let exchange_patch_present = patches.iter().any(|patch| patch.name == "exchange");
    if !overflow.is_empty() {
        all_unmatched.push_str(&overflow);
    }
    if !unmatched.is_empty() {
        if !all_unmatched.is_empty() {
            all_unmatched.push('\n');
        }
        all_unmatched.push_str(unmatched);
    }

    // Handle unmatched content
    if !all_unmatched.is_empty() {
        // Re-parse after patches applied
        let components =
            component::parse(&result).context("failed to re-parse components after patching")?;

        if let Some(output_comp) = components
            .iter()
            .find(|c| c.name == "exchange" || c.name == "output")
        {
            // Unmatched content lands in exchange/output — strip trailing bare `❯`
            // lines so a phantom prompt row never precedes the boundary marker.
            let stripped = if output_comp.name == "exchange" {
                strip_trailing_caret_lines(&all_unmatched)
            } else {
                all_unmatched.clone()
            };
            let unmatched = &stripped;
            let force_replace = output_comp.name == "exchange"
                && mode_overrides
                    .get("exchange")
                    .map(|mode| mode == "replace")
                    .unwrap_or(false)
                && !exchange_patch_present;
            if force_replace {
                let new_content = format!("{}\n", unmatched.trim_end());
                result = output_comp.replace_content(&result, &new_content);
            } else if let Some(bid) = find_boundary_in_component(&result, output_comp) {
                // Try boundary-aware append first (preserves prompt ordering)
                eprintln!(
                    "[template] unmatched content: using boundary {} for insertion",
                    &bid[..bid.len().min(8)]
                );
                result = output_comp.append_with_boundary(&result, unmatched, &bid);
            } else {
                // No boundary — plain append to exchange/output component
                let existing = output_comp.content(&result);
                let new_content = if existing.trim().is_empty() {
                    format!("{}\n", unmatched)
                } else {
                    format!("{}{}\n", existing, unmatched)
                };
                result = output_comp.replace_content(&result, &new_content);
            }
        } else {
            // Auto-create exchange component at the end — always strip trailing `❯`.
            let stripped = strip_trailing_caret_lines(&all_unmatched);
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("\n<!-- agent:exchange -->\n");
            result.push_str(&stripped);
            result.push_str("\n<!-- /agent:exchange -->\n");
        }
    }

    // Post-patch: merge duplicate exchange openers that slipped in during patching.
    while let Some(merged) = repair_duplicate_exchange_opener(&result)? {
        eprintln!("[template] post-patch: merged duplicate exchange opener");
        result = merged;
    }

    // Post-patch: remove consecutive duplicate lines from exchange (prevents agent
    // echo of user prompt when patch content starts with already-appended content).
    result = dedup_exchange_adjacent_lines(&result);

    // Post-patch: apply max_lines trimming to components that have it configured.
    // Precedence: inline attr > config.toml ([components] section) > unlimited (0).
    // Re-parse after each replacement (offsets change) and iterate up to 3 times
    // until stable — trimming one component cannot grow another, so 2 passes suffice
    // in practice; the third is a safety bound.
    {
        'stability: for _ in 0..3 {
            let Ok(components) = component::parse(&result) else {
                break;
            };
            for comp in &components {
                let max_lines = comp
                    .attrs
                    .get("max_lines")
                    .and_then(|s| s.parse::<usize>().ok())
                    .or_else(|| max_lines_configs.get(&comp.name).copied())
                    .unwrap_or(0);
                if max_lines > 0 {
                    let content = comp.content(&result);
                    let trimmed = limit_lines(content, max_lines);
                    if trimmed.len() != content.len() {
                        let trimmed = format!("{}\n", trimmed.trim_end());
                        result = comp.replace_content(&result, &trimmed);
                        // Re-parse from scratch — offsets are now stale.
                        continue 'stability;
                    }
                }
            }
            break; // No component needed trimming — stable.
        }
    }

    // Post-patch: ensure a boundary exists at the end of the exchange component.
    // This is unconditional for template docs with an exchange — the boundary must
    // always exist for checkpoint writes to work. Checking the original doc's content
    // causes a snowball: once one cycle loses the boundary, every subsequent cycle
    // also loses it because the check always finds nothing.
    {
        if let Ok(components) = component::parse(&result)
            && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
            && find_boundary_in_component(&result, exchange).is_none()
        {
            // Boundary was consumed — re-insert at end of exchange
            let id = uuid::Uuid::new_v4().to_string();
            let marker = format!("<!-- agent:boundary:{} -->", id);
            let content = exchange.content(&result);
            let new_content = format!("{}\n{}\n", content.trim_end(), marker);
            result = exchange.replace_content(&result, &new_content);
            eprintln!(
                "[template] re-inserted boundary {} at end of exchange",
                &id[..id.len().min(8)]
            );
        }
    }

    result = annotate_exchange_headings_against_baseline(&result, doc);

    Ok(result)
}

/// Reposition the boundary marker to the end of the exchange component.
///
/// Removes all existing boundaries and inserts a fresh one at the end of
/// the exchange. This is the same pre-patch logic used in
/// `apply_patches_with_overrides()`, extracted for use by the IPC write path.
///
/// Returns the document unchanged if no exchange component exists.
pub fn reposition_boundary_to_end(doc: &str) -> String {
    reposition_boundary_to_end_with_summary(doc, None)
}

/// Reposition the boundary marker without adding any ` (HEAD)` annotations.
///
/// Used by commit-time cleanup and editor-side boundary refresh so the
/// working tree can be brought back to the same clean shape as the committed
/// blob instead of introducing boundary-only dirtiness.
pub fn reposition_boundary_to_end_clean(doc: &str) -> String {
    reposition_boundary_to_end_clean_with_id(doc, None)
}

/// Reposition boundary using an explicit boundary ID when provided.
///
/// Callers use this to refresh an editor buffer to the already-committed
/// boundary marker instead of minting a new boundary-only diff locally.
pub fn reposition_boundary_to_end_clean_with_id(doc: &str, boundary_id: Option<&str>) -> String {
    reposition_boundary_to_end_clean_internal(doc, boundary_id, None)
}

/// Reposition boundary with an optional human-readable summary suffix.
///
/// The summary is slugified and appended to the boundary ID:
/// `a0cfeb34:agent-doc` instead of just `a0cfeb34`.
pub fn reposition_boundary_to_end_with_summary(doc: &str, summary: Option<&str>) -> String {
    reposition_boundary_to_end_with_baseline(doc, summary, None)
}

/// Reposition boundary with an optional human-readable summary suffix, but do
/// not add any ` (HEAD)` heading annotations.
pub fn reposition_boundary_to_end_clean_with_summary(doc: &str, summary: Option<&str>) -> String {
    reposition_boundary_to_end_clean_internal(doc, None, summary)
}

/// Like [`reposition_boundary_to_end_clean_with_summary`] but pins an explicit
/// (deterministic) boundary ID instead of minting a fresh random one. The IPC
/// patch builder seeds this from the stable `patch_id` so a single write's
/// socket / file / fallback rebuilds all carry the same boundary, preventing the
/// plugin from appending the response twice (#finalize-visible-buffer-ipc-timeout-race).
pub fn reposition_boundary_to_end_clean_with_summary_and_id(
    doc: &str,
    boundary_id: Option<&str>,
    summary: Option<&str>,
) -> String {
    reposition_boundary_to_end_clean_internal(doc, boundary_id, summary)
}

fn reposition_boundary_to_end_clean_internal(
    doc: &str,
    boundary_id: Option<&str>,
    summary: Option<&str>,
) -> String {
    let mut result = strip_transient_head_markers(&remove_all_boundaries(doc));
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = boundary_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| crate::id::new_boundary_id_with_summary(summary));
        let marker = crate::id::format_boundary_marker(&id);
        let content = exchange.content(&result).to_string();
        let new_content = format!("{}\n{}\n", content.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
    }
    result
}

/// Reposition boundary to end of exchange WITHOUT stripping `(HEAD)` markers.
///
/// Used for post-commit working-tree cleanup where `(HEAD)` annotations should
/// remain visible to the user while the committed blob stays clean.
pub fn reposition_boundary_to_end_preserve_head(doc: &str) -> String {
    reposition_boundary_to_end_preserve_head_with_id(doc, None)
}

/// Reposition boundary using an explicit ID, preserving `(HEAD)` markers.
pub fn reposition_boundary_to_end_preserve_head_with_id(
    doc: &str,
    boundary_id: Option<&str>,
) -> String {
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = boundary_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(crate::id::new_boundary_id);
        let marker = crate::id::format_boundary_marker(&id);
        let content = exchange.content(&result).to_string();
        let new_content = format!("{}\n{}\n", content.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
    }
    result
}

/// Strip transient ` (HEAD)` suffixes from markdown headings and bold-text
/// pseudo-headers, skipping fenced code blocks.
fn strip_transient_head_markers(content: &str) -> String {
    let code_ranges = component::find_code_ranges(content);
    let in_code = |pos: usize| code_ranges.iter().any(|&(s, e)| pos >= s && pos < e);

    let mut result = String::with_capacity(content.len());
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();

        let mut rewritten = line.to_string();
        if !in_code(line_start) {
            let had_newline = rewritten.ends_with('\n');
            let body = rewritten.trim_end_matches('\n').trim_end_matches('\r');
            let trimmed = body.trim_start();
            let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
            let is_markdown_heading =
                hash_count > 0 && hash_count <= 6 && trimmed[hash_count..].starts_with(' ');
            let is_bold_pseudo_header = trimmed.starts_with("**") && trimmed.ends_with("** (HEAD)");
            if (is_markdown_heading || is_bold_pseudo_header)
                && let Some(stripped) = body.strip_suffix(" (HEAD)")
            {
                rewritten = if had_newline {
                    format!("{stripped}\n")
                } else {
                    stripped.to_string()
                };
            }
        }

        result.push_str(&rewritten);
    }

    result
}

/// Reposition boundary, with an optional set of baseline `### Re:` headings
/// (typically extracted from git HEAD). When a baseline is supplied, every
/// `### Re:` heading in the current exchange whose normalized text is NOT in
/// the baseline is treated as "new this cycle" and receives a ` (HEAD)` suffix.
/// Headings already present in the baseline are stripped of any stale
/// ` (HEAD)` suffix. When the baseline is `None`, behavior matches the legacy
/// `annotate_latest_re_heading_with_head` path: only the last `### Re:` heading
/// gets the marker.
pub fn reposition_boundary_to_end_with_baseline(
    doc: &str,
    summary: Option<&str>,
    baseline_headings: Option<&std::collections::HashSet<String>>,
) -> String {
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = crate::id::new_boundary_id_with_summary(summary);
        let marker = crate::id::format_boundary_marker(&id);
        let content = exchange.content(&result).to_string();
        let annotated = annotate_re_headings_with_head(&content, baseline_headings);
        let new_content = format!("{}\n{}\n", annotated.trim_end(), marker);
        result = exchange.replace_content(&result, &new_content);
    }
    result
}

/// Extract the set of stripped `### Re:` heading lines from the `exchange`
/// component of a document. Used by the commit path to build a baseline of
/// headings already present in `git HEAD` so the reposition step can mark all
/// new-this-cycle headings (not just the last one) with ` (HEAD)`.
///
/// Returns an empty set if the document has no `exchange` component or no
/// matching headings. Headings inside fenced code blocks are skipped.
pub fn exchange_baseline_headings(doc: &str) -> std::collections::HashSet<String> {
    if let Ok(components) = component::parse(doc)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        return collect_re_headings(exchange.content(doc));
    }
    std::collections::HashSet::new()
}

pub fn annotate_exchange_headings_against_baseline(doc: &str, baseline_doc: &str) -> String {
    let Ok(components) = component::parse(doc) else {
        return doc.to_string();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return doc.to_string();
    };

    let baseline_exchange = component::parse(baseline_doc)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|c| c.name == "exchange")
                .map(|c| c.content(baseline_doc).to_string())
        })
        .unwrap_or_default();
    let baseline_headings = exchange_baseline_headings(baseline_doc);
    let content = exchange.content(doc);
    let normalize_for_compare = |value: &str| {
        strip_transient_head_markers(value)
            .lines()
            .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if normalize_for_compare(content) == normalize_for_compare(&baseline_exchange) {
        return doc.to_string();
    }
    let annotated = annotate_re_headings_with_head(content, Some(&baseline_headings));
    if annotated == content {
        return doc.to_string();
    }
    exchange.replace_content(doc, &annotated)
}

/// Collect normalized `### Re:` heading lines from a chunk of exchange content.
/// Each entry is the heading line with any trailing ` (HEAD)` suffix and
/// trailing whitespace removed. Headings inside fenced code blocks are skipped.
fn collect_re_headings(content: &str) -> std::collections::HashSet<String> {
    let code_ranges = component::find_code_ranges(content);
    let in_code = |pos: usize| code_ranges.iter().any(|&(s, e)| pos >= s && pos < e);
    let mut set = std::collections::HashSet::new();
    let mut offset = 0usize;
    for line in content.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if in_code(line_start) {
            continue;
        }
        let body = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = body.trim_start();
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if hash_count == 0 || hash_count > 6 {
            continue;
        }
        let after_hash = &trimmed[hash_count..];
        if !after_hash.starts_with(' ') {
            continue;
        }
        if !after_hash.trim_start().starts_with("Re:") {
            continue;
        }
        let stripped = body
            .trim_start()
            .trim_end()
            .trim_end_matches(" (HEAD)")
            .to_string();
        set.insert(stripped);
    }
    set
}

/// Strip ` (HEAD)` suffix from all `### Re:` heading lines, then append
/// ` (HEAD)` to every heading that is NEW relative to `baseline`. When
/// `baseline` is `None`, the legacy behavior is preserved: only the last
/// `### Re:` heading receives ` (HEAD)`. Leaves content unchanged if no such
/// heading exists. Skips headings inside fenced code blocks.
///
/// This is the symmetric counterpart to `git::strip_head_markers`: this adds
/// the marker on the working-tree / snapshot side; strip_head_markers removes
/// it on the git-staging side so the committed blob stays clean.
///
/// Operates on exchange component content, where `### Re:` is the canonical
/// response heading format (h3). We only touch `### Re:` headings — NOT bold
/// pseudo-headers (`**Re: ...**`) — matching the SKILL.md response contract.
///
/// When baseline is supplied (typically from git HEAD via
/// `exchange_baseline_headings`), a patchback containing multiple `### Re:`
/// sections in a single cycle gets ` (HEAD)` appended to EVERY new heading,
/// so every newly-added heading line shows as modified in the git gutter.
pub(crate) fn annotate_re_headings_with_head(
    content: &str,
    baseline: Option<&std::collections::HashSet<String>>,
) -> String {
    let code_ranges = component::find_code_ranges(content);
    let in_code = |pos: usize| code_ranges.iter().any(|&(s, e)| pos >= s && pos < e);

    let mut lines: Vec<String> = content
        .split_inclusive('\n')
        .map(|s| s.to_string())
        .collect();
    let mut re_indices: Vec<usize> = Vec::new();
    let mut offset = 0usize;

    for (idx, line) in lines.iter_mut().enumerate() {
        let line_start = offset;
        offset += line.len();
        if in_code(line_start) {
            continue;
        }
        let had_newline = line.ends_with('\n');
        let body_ref = line.trim_end_matches('\n').trim_end_matches('\r');
        let trimmed = body_ref.trim_start();
        let hash_count = trimmed.chars().take_while(|&c| c == '#').count();
        if hash_count == 0 || hash_count > 6 {
            continue;
        }
        let after_hash = &trimmed[hash_count..];
        if !after_hash.starts_with(' ') {
            continue;
        }
        if !after_hash.trim_start().starts_with("Re:") {
            continue;
        }
        // Strip existing (HEAD) suffix (robust against trailing whitespace).
        let stripped = body_ref.trim_end().trim_end_matches(" (HEAD)");
        *line = if had_newline {
            format!("{stripped}\n")
        } else {
            stripped.to_string()
        };
        re_indices.push(idx);
    }

    // Decide which heading lines receive (HEAD).
    // - With baseline: every Re: heading whose normalized text is NOT in the
    //   baseline set is treated as new this cycle and gets (HEAD). When the
    //   baseline filter yields zero new headings (common: a turn that doesn't
    //   open a new Re: section), fall back to marking the last Re: heading so
    //   the working tree always retains a single "head" marker. Without this
    //   fallback, the commit path strips the prior cycle's (HEAD) on line 613
    //   and leaves nothing marked — regressing the visual head pointer.
    // - Without baseline: legacy behavior — only the last Re: heading.
    let mark_indices: Vec<usize> = match baseline {
        Some(baseline_set) => {
            let filtered: Vec<usize> = re_indices
                .iter()
                .copied()
                .filter(|&idx| {
                    let line = &lines[idx];
                    let key = line
                        .trim_end_matches('\n')
                        .trim_end_matches('\r')
                        .trim_start()
                        .trim_end();
                    !baseline_set.contains(key)
                })
                .collect();
            if filtered.is_empty() {
                re_indices.last().copied().into_iter().collect()
            } else {
                filtered
            }
        }
        None => re_indices.last().copied().into_iter().collect(),
    };

    for idx in mark_indices {
        let line = &lines[idx];
        let had_newline = line.ends_with('\n');
        let body = line.trim_end_matches('\n').trim_end_matches('\r');
        lines[idx] = if had_newline {
            format!("{body} (HEAD)\n")
        } else {
            format!("{body} (HEAD)")
        };
    }

    lines.concat()
}

/// Remove all boundary markers from a document (line-level removal).
/// Skips boundaries inside fenced code blocks (lesson #13).
fn remove_all_boundaries(doc: &str) -> String {
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let code_ranges = component::find_code_ranges(doc);
    let in_code = |pos: usize| {
        code_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end)
    };
    let mut result = String::with_capacity(doc.len());
    let mut offset = 0;
    for line in doc.lines() {
        let trimmed = line.trim();
        let is_boundary = trimmed.starts_with(prefix) && trimmed.ends_with(suffix);
        if is_boundary && !in_code(offset) {
            // Skip boundary marker lines outside code blocks
            offset += line.len() + 1; // +1 for newline
            continue;
        }
        result.push_str(line);
        result.push('\n');
        offset += line.len() + 1;
    }
    if !doc.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }
    result
}

/// Find a boundary marker ID inside a component's content, skipping code blocks.
fn find_boundary_in_component(doc: &str, comp: &Component) -> Option<String> {
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let content_region = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);
    let mut search_from = 0;
    while let Some(start) = content_region[search_from..].find(prefix) {
        let abs_start = comp.open_end + search_from + start;
        if code_ranges
            .iter()
            .any(|&(cs, ce)| abs_start >= cs && abs_start < ce)
        {
            search_from += start + prefix.len();
            continue;
        }
        let after_prefix = &content_region[search_from + start + prefix.len()..];
        if let Some(end) = after_prefix.find(suffix) {
            return Some(after_prefix[..end].trim().to_string());
        }
        break;
    }
    None
}

/// Default mode for a component by name.
/// `exchange` and `findings` default to `append`; all others default to `replace`.
fn default_mode(name: &str) -> &'static str {
    match name {
        "exchange" | "findings" => "append",
        _ => "replace",
    }
}

/// Trim content to the last N lines.
fn limit_lines(content: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_lines {
        return content.to_string();
    }
    lines[lines.len() - max_lines..].join("\n")
}

/// Remove consecutive identical non-blank lines in the exchange component.
///
/// Prevents agent echoes of user prompts from creating duplicates when
/// `apply_mode("append")` concatenates existing content that already ends
/// with the first line(s) of the new patch content.
///
/// Only non-blank lines are subject to deduplication — blank lines are
/// intentional separators and are never collapsed.
fn dedup_exchange_adjacent_lines(doc: &str) -> String {
    let Ok(components) = component::parse(doc) else {
        return doc.to_string();
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return doc.to_string();
    };
    let content = exchange.content(doc);
    let mut deduped = String::with_capacity(content.len());
    let mut prev_nonempty: Option<&str> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        let is_fence = is_code_fence_delimiter(trimmed);
        if !trimmed.is_empty() && !is_fence && prev_nonempty == Some(line) {
            continue;
        }
        deduped.push_str(line);
        deduped.push('\n');
        if !trimmed.is_empty() {
            prev_nonempty = Some(line);
        }
    }
    if !content.ends_with('\n') && deduped.ends_with('\n') {
        deduped.pop();
    }
    if deduped == content {
        return doc.to_string();
    }
    exchange.replace_content(doc, &deduped)
}

fn is_code_fence_delimiter(trimmed: &str) -> bool {
    let fc = match trimmed.chars().next() {
        Some(c) if c == '`' || c == '~' => c,
        _ => return false,
    };
    let fence_len = trimmed.chars().take_while(|&c| c == fc).count();
    fence_len >= 3
}

/// Apply mode logic (replace/append/prepend).
fn apply_mode(mode: &str, existing: &str, new_content: &str) -> String {
    match mode {
        "append" => {
            let stripped = strip_leading_overlap(existing, new_content);
            format!("{}{}", existing, stripped)
        }
        "prepend" => format!("{}{}", new_content, existing),
        _ => new_content.to_string(), // "replace" default
    }
}

/// Strip the last non-blank line of `existing` from the start of `new_content` if present.
///
/// When an agent echoes the user's last prompt as the first line of its patch,
/// append mode would duplicate that line. This strips the overlap before concatenation.
/// Markdown fence delimiters are structural, so adjacent prompt/response code
/// fences must survive even when the delimiter text is identical.
fn strip_leading_overlap<'a>(existing: &str, new_content: &'a str) -> &'a str {
    let last_nonempty = existing.lines().rfind(|l| !l.trim().is_empty());
    let Some(last) = last_nonempty else {
        return new_content;
    };
    if is_code_fence_delimiter(last.trim()) {
        return new_content;
    }
    let test = format!("{}\n", last);
    if new_content.starts_with(test.as_str()) {
        &new_content[test.len()..]
    } else {
        new_content
    }
}

/// Find `needle` in `haystack` starting at `from`, skipping occurrences inside code ranges.
/// Returns the byte offset of the match within `haystack` (absolute, not relative to `from`).
fn find_outside_code(
    needle: &str,
    haystack: &str,
    from: usize,
    code_ranges: &[(usize, usize)],
) -> Option<usize> {
    let mut search_start = from;
    loop {
        let rel = haystack[search_start..].find(needle)?;
        let abs = search_start + rel;
        if code_ranges
            .iter()
            .any(|&(start, end)| abs >= start && abs < end)
        {
            // Inside a code block — skip past this occurrence
            search_start = abs + needle.len();
            continue;
        }
        return Some(abs);
    }
}

#[cfg(test)]
mod tests;
