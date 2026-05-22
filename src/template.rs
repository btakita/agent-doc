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
use std::path::Path;

use crate::component::{self, Component, find_comment_end, is_backlog_component};
use crate::project_config;

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
pub fn apply_patches(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
) -> Result<String> {
    apply_patches_with_overrides(
        doc,
        patches,
        unmatched,
        file,
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
    new_content.push_str(&crate::format_boundary_marker(&crate::new_boundary_id()));
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
    while let Some(merged) = repair_duplicate_exchange_opener(&normalized)? {
        normalized = merged;
    }
    if let Some(cleaned) = remove_post_exchange_duplicate_prompt_comments(&normalized) {
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

/// Remove ordinary HTML comments after `agent:exchange` when their body is a
/// duplicate or near-duplicate of prompt text already present in the exchange.
///
/// This keeps unrelated scratch comments user-owned while cleaning stale editor
/// residue such as a hidden previous copy of the current prompt.
pub fn remove_post_exchange_duplicate_prompt_comments(doc: &str) -> Option<String> {
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
        let Some(cleaned_body) = strip_duplicate_prompt_comment_body(body, &prompts) else {
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

    let mut offset = 0usize;
    for segment in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += segment.len();
        if line_start < exchange.close_end || in_protected_component(line_start) {
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

fn strip_duplicate_prompt_comment_body(body: &str, prompts: &[String]) -> Option<String> {
    if is_duplicate_prompt_comment_text(body, prompts) {
        return None;
    }

    let mut changed = false;
    let mut cleaned = String::new();
    for segment in body.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        if is_duplicate_prompt_comment_text(line, prompts) {
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
        let Some(normalized) = normalize_prompt_comment_text(trimmed) else {
            continue;
        };
        if in_response_block && !looks_like_prompt_comment_target(trimmed) {
            continue;
        }
        if in_response_block {
            in_response_block = false;
        }
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
    let tail_start = user_region.rfind(&original_tail).with_context(|| {
        format!(
            "failed to locate unresolved prompt tail in current exchange for anchored patchback: {}",
            anchor.text.lines().next().unwrap_or("(empty)")
        )
    })?;
    let insert_at = tail_start + anchor.end;

    let new_id = crate::new_boundary_id();
    let new_marker = crate::format_boundary_marker(&new_id);
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
pub fn apply_patches_with_overrides(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &std::collections::HashMap<String, String>,
) -> Result<String> {
    // Pre-patch: ensure a fresh boundary exists in the exchange component.
    // Remove any stale boundaries from previous cycles, then insert a new one
    // at the end of the exchange. This is deterministic — belongs in the binary,
    // not the SKILL workflow.
    let summary = file.file_stem().and_then(|s| s.to_str());
    let mut result = remove_all_boundaries(doc);
    if let Ok(components) = component::parse(&result)
        && let Some(exchange) = components.iter().find(|c| c.name == "exchange")
    {
        let id = crate::new_boundary_id_with_summary(summary);
        let marker = crate::format_boundary_marker(&id);
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

    // Load component configs
    let configs = load_component_configs(file);

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
        let max_lines_configs = load_max_lines_configs(file);
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
            .unwrap_or_else(|| crate::new_boundary_id_with_summary(summary));
        let marker = crate::format_boundary_marker(&id);
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
            .unwrap_or_else(crate::new_boundary_id);
        let marker = crate::format_boundary_marker(&id);
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
        let id = crate::new_boundary_id_with_summary(summary);
        let marker = crate::format_boundary_marker(&id);
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

pub(crate) fn annotate_exchange_headings_against_baseline(doc: &str, baseline_doc: &str) -> String {
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

/// Get template info for a document (for plugin rendering).
pub fn template_info(file: &Path) -> Result<TemplateInfo> {
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, _body) = crate::frontmatter::parse(&doc)?;
    let template_mode = fm.resolve_mode().is_template();

    let components = component::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file);

    let component_infos: Vec<ComponentInfo> = components
        .iter()
        .map(|comp| {
            let content = comp.content(&doc).to_string();
            // Inline attr > config.toml ([components] section) > built-in default
            let mode = comp
                .patch_mode()
                .map(|s| s.to_string())
                .or_else(|| configs.get(&comp.name).cloned())
                .unwrap_or_else(|| default_mode(&comp.name).to_string());
            // Compute line number from byte offset
            let line = doc[..comp.open_start].matches('\n').count() + 1;
            ComponentInfo {
                name: comp.name.clone(),
                mode,
                content,
                line,
                max_entries: None, // TODO: read from config.toml ([components] section)
            }
        })
        .collect();

    Ok(TemplateInfo {
        template_mode,
        components: component_infos,
    })
}

/// Load component mode configs from `.agent-doc/config.toml` (under [components] section).
/// Returns a map of component_name → mode string.
/// Resolves the project root by walking up from the document's parent directory.
fn load_component_configs(file: &Path) -> std::collections::HashMap<String, String> {
    let proj_cfg = load_project_from_doc(file);
    proj_cfg
        .components
        .iter()
        .map(|(name, cfg): (&String, &project_config::ComponentConfig)| {
            (name.clone(), cfg.patch.clone())
        })
        .collect()
}

/// Load max_lines settings from `.agent-doc/config.toml` (under [components] section).
/// Resolves the project root by walking up from the document's parent directory.
fn load_max_lines_configs(file: &Path) -> std::collections::HashMap<String, usize> {
    let proj_cfg = load_project_from_doc(file);
    proj_cfg
        .components
        .iter()
        .filter(|(_, cfg)| cfg.max_lines > 0)
        .map(|(name, cfg): (&String, &project_config::ComponentConfig)| {
            (name.clone(), cfg.max_lines)
        })
        .collect()
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
fn load_project_from_doc(file: &Path) -> project_config::ProjectConfig {
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            return project_config::load_project_from(&candidate);
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    // Fall back to CWD-based resolution
    project_config::load_project()
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
fn strip_leading_overlap<'a>(existing: &str, new_content: &'a str) -> &'a str {
    let last_nonempty = existing.lines().rfind(|l| !l.trim().is_empty());
    let Some(last) = last_nonempty else {
        return new_content;
    };
    let test = format!("{}\n", last);
    if new_content.starts_with(test.as_str()) {
        &new_content[test.len()..]
    } else {
        new_content
    }
}

#[allow(dead_code)]
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
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn parse_single_patch() {
        let response = "<!-- patch:status -->\nBuild passing.\n<!-- /patch:status -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "Build passing.\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_multiple_patches() {
        let response = "\
<!-- patch:status -->
All green.
<!-- /patch:status -->

<!-- patch:log -->
- New entry
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All green.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- New entry\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_with_unmatched_content() {
        let response = "Some free text.\n\n<!-- patch:status -->\nOK\n<!-- /patch:status -->\n\nTrailing text.\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert!(unmatched.contains("Some free text."));
        assert!(unmatched.contains("Trailing text."));
    }

    #[test]
    fn parse_empty_response() {
        let (patches, unmatched) = parse_patches("").unwrap();
        assert!(patches.is_empty());
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_no_patches() {
        let response = "Just a plain response with no patch blocks.";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert!(patches.is_empty());
        assert_eq!(unmatched, "Just a plain response with no patch blocks.");
    }

    #[test]
    fn apply_patches_replace() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("\nold\n"));
        assert!(result.contains("<!-- agent:status -->"));
    }

    #[test]
    fn apply_patches_unmatched_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches(doc, &[], "Extra info here", &doc_path).unwrap();
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Extra info here"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn apply_patches_unmatched_appends_to_existing_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches(doc, &[], "new stuff", &doc_path).unwrap();
        assert!(result.contains("previous"));
        assert!(result.contains("new stuff"));
        // Should not create a second exchange component
        assert_eq!(result.matches("<!-- agent:exchange -->").count(), 1);
    }

    #[test]
    fn apply_patches_missing_component_routes_to_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Missing component content should be routed to exchange
        assert!(
            result.contains("overflow data"),
            "missing patch content should appear in exchange"
        );
        assert!(
            result.contains("previous"),
            "existing exchange content should be preserved"
        );
    }

    #[test]
    fn apply_patches_missing_component_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Should auto-create exchange component
        assert!(
            result.contains("<!-- agent:exchange -->"),
            "should create exchange component"
        );
        assert!(
            result.contains("overflow data"),
            "overflow content should be in exchange"
        );
    }

    #[test]
    fn is_template_mode_detection() {
        assert!(is_template_mode(Some("template")));
        assert!(!is_template_mode(Some("append")));
        assert!(!is_template_mode(None));
    }

    #[test]
    fn template_info_works() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(info.template_mode);
        assert_eq!(info.components.len(), 1);
        assert_eq!(info.components[0].name, "status");
        assert_eq!(info.components[0].content, "content\n");
    }

    #[test]
    fn template_info_legacy_mode_works() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nresponse_mode: template\n---\n\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(info.template_mode);
    }

    #[test]
    fn template_info_append_mode() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "---\nagent_doc_format: append\n---\n\n# Doc\n";
        std::fs::write(&doc_path, doc).unwrap();

        let info = template_info(&doc_path).unwrap();
        assert!(!info.template_mode);
        assert!(info.components.is_empty());
    }

    #[test]
    fn parse_patches_ignores_markers_in_fenced_code_block() {
        let response = "\
<!-- patch:exchange -->
Here is how you use component markers:

```markdown
<!-- agent:exchange -->
example content
<!-- /agent:exchange -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("```markdown"));
        assert!(patches[0].content.contains("<!-- agent:exchange -->"));
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_patch_markers_in_fenced_code_block() {
        // Patch markers inside a code block should not be treated as real patches
        let response = "\
<!-- patch:exchange -->
Real content here.

```markdown
<!-- patch:fake -->
This is just an example.
<!-- /patch:fake -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1, "should only find the outer real patch");
        assert_eq!(patches[0].name, "exchange");
        assert!(
            patches[0].content.contains("<!-- patch:fake -->"),
            "code block content should be preserved"
        );
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_markers_in_tilde_fence() {
        let response = "\
<!-- patch:status -->
OK
<!-- /patch:status -->

~~~
<!-- patch:fake -->
example
<!-- /patch:fake -->
~~~
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        // Only the real patch should be found; the fake one inside ~~~ is ignored
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
    }

    #[test]
    fn parse_patches_ignores_closing_marker_in_code_block() {
        // The closing marker for a real patch is inside a code block,
        // so the parser should skip it and find the real closing marker outside
        let response = "\
<!-- patch:exchange -->
Example:

```
<!-- /patch:exchange -->
```

Real content continues.
<!-- /patch:exchange -->
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("Real content continues."));
    }

    #[test]
    fn parse_patches_normal_markers_still_work() {
        // Sanity check: normal patch parsing without code blocks still works
        let response = "\
<!-- patch:status -->
All systems go.
<!-- /patch:status -->
<!-- patch:log -->
- Entry 1
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All systems go.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- Entry 1\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_accepts_replace_icebox() {
        let response = "\
<!-- replace:icebox -->
- [ ] [#park1] Parked follow-up
<!-- /replace:icebox -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "icebox");
        assert_eq!(patches[0].content, "- [ ] [#park1] Parked follow-up\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_orphaned_opener_does_not_leak_into_unmatched() {
        // Bug #p2xm: an unclosed `<!-- patch:exchange -->` was leaking into
        // unmatched text and getting appended to exchange verbatim.
        let response = "\
Some real content here.
<!-- patch:exchange -->
This opener has no matching close.
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert!(
            patches.is_empty(),
            "orphaned opener should not produce a patch"
        );
        assert_eq!(
            unmatched, "Some real content here.\nThis opener has no matching close.",
            "unmatched should contain text before and after the orphaned marker, but not the marker itself"
        );
    }

    #[test]
    fn parse_patches_orphaned_opener_between_valid_patches() {
        // Orphaned opener between two valid patches — only the valid ones parse,
        // text around the orphan becomes unmatched, marker itself is consumed.
        let response = "\
<!-- patch:status -->
All good.
<!-- /patch:status -->
Interstitial text.
<!-- patch:exchange -->
<!-- patch:log -->
- Log entry
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[1].name, "log");
        assert_eq!(unmatched, "Interstitial text.");
    }

    // --- Inline attribute mode resolution tests ---

    #[test]
    fn inline_attr_mode_overrides_config() {
        // Component has mode=replace inline, but config.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        // Write config with append mode for status
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        // But the inline attr says replace
        let doc = "<!-- agent:status mode=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Inline replace should win over config append
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_attr_mode_overrides_default() {
        // exchange defaults to append, but inline says replace
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=replace -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn no_inline_attr_falls_back_to_config() {
        // No inline attr → falls back to config.toml ([components] section)
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        let doc = "<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Config says append, so both old and new should be present
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn no_inline_attr_no_config_falls_back_to_default() {
        // No inline attr, no config → built-in defaults
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // exchange defaults to append
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_config() {
        // Component has patch=replace inline, but config.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        let doc = "<!-- agent:status patch=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_mode_attr() {
        // Both patch= and mode= present; patch= wins
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc =
            "<!-- agent:exchange patch=replace mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn toml_patch_key_works() {
        // config.toml uses `[components.status]` with `patch = "append"`
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        let doc = "<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn stream_override_beats_inline_attr() {
        // Stream mode overrides should still beat inline attrs
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result =
            apply_patches_with_overrides(doc, &patches, "", &doc_path, &overrides).unwrap();
        // Stream override (replace) should win over inline attr (append)
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn exchange_replace_override_replaces_unmatched_content() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nold\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result =
            apply_patches_with_overrides(doc, &[], "Compacted summary.\n", &doc_path, &overrides)
                .unwrap();

        assert!(result.contains("Compacted summary.\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn exchange_replace_override_keeps_explicit_exchange_patch_authoritative() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nold\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "Compacted summary.\n".to_string(),
            attrs: Default::default(),
        }];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result =
            apply_patches_with_overrides(doc, &patches, "trailing note", &doc_path, &overrides)
                .unwrap();

        assert!(result.contains("Compacted summary.\n"));
        assert!(result.contains("trailing note"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn apply_patches_ignores_component_tags_in_code_blocks() {
        // Component tags inside a fenced code block should not be patch targets.
        // Only the real top-level component should receive the patch content.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
# Scaffold Guide

Here is an example of a component:

```markdown
<!-- agent:status -->
example scaffold content
<!-- /agent:status -->
```

<!-- agent:status -->
real status content
<!-- /agent:status -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "patched status\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();

        // The real component should be patched
        assert!(
            result.contains("patched status\n"),
            "real component should receive the patch"
        );
        // The code block example should be untouched
        assert!(
            result.contains("example scaffold content"),
            "code block content should be preserved"
        );
        // The code block's markers should still be there
        assert!(
            result.contains("```markdown\n<!-- agent:status -->"),
            "code block markers should be preserved"
        );
    }

    #[test]
    fn unmatched_content_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:test-uuid-123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // No patch blocks — only unmatched content (simulates skill not wrapping in patch blocks)
        let patches = vec![];
        let unmatched = "### Re: Response\n\nResponse content here.\n";

        let result = apply_patches(doc, &patches, unmatched, &file).unwrap();

        // Response should be inserted at the boundary marker position (after prompt)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after the user prompt (boundary insertion)"
        );

        // Boundary marker should be consumed (replaced by response)
        assert!(
            !result.contains("test-uuid-123"),
            "boundary marker should be consumed after insertion"
        );
    }

    #[test]
    fn explicit_patch_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:patch-uuid-456 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // Explicit patch block targeting exchange
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: Response\n\nResponse content.\n".to_string(),
            attrs: Default::default(),
        }];

        let result = apply_patches(doc, &patches, "", &file).unwrap();

        // Response should be after prompt (boundary consumed)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after user prompt"
        );

        // Boundary marker should be consumed
        assert!(
            !result.contains("patch-uuid-456"),
            "boundary marker should be consumed by explicit patch"
        );
    }

    #[test]
    fn boundary_reinserted_even_when_original_doc_has_no_boundary() {
        // Regression: the snowball bug — once one cycle loses the boundary,
        // every subsequent cycle also loses it because orig_had_boundary finds nothing.
        let dir = setup_project();
        let file = dir.path().join("test.md");
        // Document with exchange but NO boundary marker
        let doc =
            "<!-- agent:exchange patch=append -->\nUser prompt here.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        let response = "<!-- patch:exchange -->\nAgent response.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        let result = apply_patches(doc, &patches, &unmatched, &file).unwrap();

        // Must have a boundary at end of exchange, even though original had none
        assert!(
            result.contains("<!-- agent:boundary:"),
            "boundary must be re-inserted even when original doc had no boundary: {result}"
        );
    }

    #[test]
    fn boundary_survives_multiple_cycles() {
        // Simulate two consecutive write cycles — boundary must persist
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nPrompt 1.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        // Cycle 1
        let response1 = "<!-- patch:exchange -->\nResponse 1.\n<!-- /patch:exchange -->\n";
        let (patches1, unmatched1) = parse_patches(response1).unwrap();
        let result1 = apply_patches(doc, &patches1, &unmatched1, &file).unwrap();
        assert!(
            result1.contains("<!-- agent:boundary:"),
            "cycle 1 must have boundary"
        );

        // Cycle 2 — use cycle 1's output as the new doc (simulates next write)
        let response2 = "<!-- patch:exchange -->\nResponse 2.\n<!-- /patch:exchange -->\n";
        let (patches2, unmatched2) = parse_patches(response2).unwrap();
        let result2 = apply_patches(&result1, &patches2, &unmatched2, &file).unwrap();
        assert!(
            result2.contains("<!-- agent:boundary:"),
            "cycle 2 must have boundary"
        );
    }

    #[test]
    fn remove_all_boundaries_skips_code_blocks() {
        let doc = "before\n```\n<!-- agent:boundary:fake-id -->\n```\nafter\n<!-- agent:boundary:real-id -->\nend\n";
        let result = remove_all_boundaries(doc);
        // The one inside the code block should survive
        assert!(
            result.contains("<!-- agent:boundary:fake-id -->"),
            "boundary inside code block must be preserved: {result}"
        );
        // The one outside should be removed
        assert!(
            !result.contains("<!-- agent:boundary:real-id -->"),
            "boundary outside code block must be removed: {result}"
        );
    }

    #[test]
    fn reposition_boundary_moves_to_end() {
        let doc = "\
<!-- agent:exchange -->
Previous response.
<!-- agent:boundary:old-id -->
User prompt here.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        // Old boundary should be gone
        assert!(!result.contains("old-id"), "old boundary should be removed");
        // New boundary should exist
        assert!(
            result.contains("<!-- agent:boundary:"),
            "new boundary should be inserted"
        );
        // New boundary should be after the user prompt, before close tag
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let prompt_pos = result.find("User prompt here.").unwrap();
        let close_pos = result.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            boundary_pos > prompt_pos,
            "boundary should be after user prompt"
        );
        assert!(
            boundary_pos < close_pos,
            "boundary should be before close tag"
        );
    }

    #[test]
    fn reposition_boundary_no_exchange_unchanged() {
        let doc = "\
<!-- agent:output -->
Some content.
<!-- /agent:output -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("<!-- agent:boundary:"),
            "no boundary should be added to non-exchange"
        );
    }

    #[test]
    fn reposition_boundary_clean_reuses_explicit_id() {
        let doc = "\
<!-- agent:exchange -->
Previous response.
<!-- agent:boundary:old-id -->
User prompt here.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_clean_with_id(doc, Some("keep-this-id"));
        assert!(!result.contains("old-id"), "old boundary should be removed");
        assert!(
            result.contains("<!-- agent:boundary:keep-this-id -->"),
            "explicit boundary id should be reused"
        );
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "exactly one boundary should remain"
        );
    }

    #[test]
    fn reposition_appends_head_to_last_re_heading() {
        // #hdap: reposition must append ` (HEAD)` to the last `### Re:`
        // heading inside the exchange component, stripping any stale
        // `(HEAD)` suffix from earlier headings.
        let doc = "\
<!-- agent:exchange -->
### Re: older (HEAD)
old body
### Re: newer
new body
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("### Re: older (HEAD)"),
            "stale (HEAD) on prior heading must be stripped; got:\n{result}"
        );
        assert!(
            result.contains("### Re: older\n"),
            "older heading must remain (without HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "latest heading must get (HEAD); got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) in result; got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_no_re_heading_unchanged() {
        // No `### Re:` headings → no (HEAD) added, content passes through.
        let doc = "\
<!-- agent:exchange -->
User text with no response headings.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("(HEAD)"),
            "no heading → no (HEAD); got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_skips_code_fence() {
        // ### Re: inside a fenced code block must NOT be treated as a heading.
        let doc = "\
<!-- agent:exchange -->
### Re: real heading
```markdown
### Re: fake heading in code fence
```
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            result.contains("### Re: real heading (HEAD)"),
            "real heading outside fence gets (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: fake heading in code fence\n"),
            "fenced heading must be untouched; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) — fenced heading ignored; got:\n{result}"
        );
    }

    #[test]
    fn reposition_with_baseline_marks_all_new_re_headings() {
        // Patchback with multiple `### Re:` headings: every heading NOT in
        // the baseline (git HEAD) gets (HEAD); every heading IN the baseline
        // does not. This matches the "all patchback top-level headers" rule.
        let doc = "\
<!-- agent:exchange -->
### Re: old-1
body a
### Re: old-2 (HEAD)
body b
### Re: new-1
body c
### Re: new-2
body d
<!-- /agent:exchange -->";
        // Baseline contains just the two "old" headings (no (HEAD), as HEAD
        // blob is always stripped by the commit staging path).
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: old-1".to_string());
        baseline.insert("### Re: old-2".to_string());

        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));

        // Both old headings lose (HEAD).
        assert!(
            result.contains("### Re: old-1\n"),
            "old-1 must not have (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: old-2\n"),
            "old-2 must not have (HEAD); got:\n{result}"
        );
        // Both new headings get (HEAD).
        assert!(
            result.contains("### Re: new-1 (HEAD)"),
            "new-1 must get (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: new-2 (HEAD)"),
            "new-2 must get (HEAD); got:\n{result}"
        );
        // Exactly two (HEAD)s — one per new heading.
        assert_eq!(
            result.matches("(HEAD)").count(),
            2,
            "exactly two (HEAD) markers; got:\n{result}"
        );
    }

    #[test]
    fn reposition_with_empty_baseline_marks_every_re_heading() {
        // First cycle / untracked file: baseline is empty. All headings are
        // "new", so all get (HEAD).
        let doc = "\
<!-- agent:exchange -->
### Re: first
a
### Re: second
b
<!-- /agent:exchange -->";
        let baseline: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        assert!(
            result.contains("### Re: first (HEAD)"),
            "first gets (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: second (HEAD)"),
            "second gets (HEAD); got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            2,
            "exactly two (HEAD) markers; got:\n{result}"
        );
    }

    #[test]
    fn exchange_baseline_headings_extracts_stripped_re_lines() {
        let doc = "\
<!-- agent:exchange -->
### Re: one (HEAD)
body
### Re: two
more body
### Not a Re heading
body
<!-- /agent:exchange -->";
        let set = exchange_baseline_headings(doc);
        assert!(
            set.contains("### Re: one"),
            "stripped one present; got: {set:?}"
        );
        assert!(set.contains("### Re: two"), "two present; got: {set:?}");
        assert_eq!(set.len(), 2, "only Re: headings; got: {set:?}");
    }

    #[test]
    fn exchange_baseline_headings_normalizes_leading_whitespace() {
        // HEAD has an indented heading; set entry must be trim_start'd so a
        // non-indented working-tree heading matches it.
        let doc = "\
<!-- agent:exchange -->
  ### Re: indented
body
### Re: flush
more
<!-- /agent:exchange -->";
        let set = exchange_baseline_headings(doc);
        assert!(
            set.contains("### Re: indented"),
            "indented entry normalized; got: {set:?}"
        );
        assert!(
            set.contains("### Re: flush"),
            "flush entry present; got: {set:?}"
        );
    }

    #[test]
    fn reposition_with_baseline_matches_indented_heading() {
        // Baseline has "### Re: foo" (flush). Working tree has "  ### Re: foo"
        // (indented). trim_start normalization makes the lookup recognize the
        // indented heading as already-in-baseline. Because the baseline filter
        // then yields zero "new" headings, the fallback kicks in and marks the
        // last Re: heading anyway — preserving the head pointer. The key point
        // is that normalization works (the heading is recognized), not that
        // (HEAD) is absent.
        let doc = "\
<!-- agent:exchange -->
  ### Re: foo
body
### Re: bar (HEAD)
body2
<!-- /agent:exchange -->";
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: foo".to_string());
        baseline.insert("### Re: bar".to_string());
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        // Both headings are in baseline → filter is empty → fallback marks
        // the LAST Re: heading only. "### Re: foo" stays unmarked (proving
        // trim_start normalization worked — without it, foo would be
        // treated as new and also get (HEAD)).
        assert!(
            result.contains("  ### Re: foo\n"),
            "indented heading must remain unmarked; got:\n{result}"
        );
        assert!(
            result.contains("### Re: bar (HEAD)"),
            "last heading gets fallback (HEAD) marker; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) via fallback; got:\n{result}"
        );
    }

    #[test]
    fn baseline_filter_empty_falls_back_to_last_heading() {
        // When every Re: heading in the working tree is already in baseline
        // (i.e., the current turn adds no new Re: sections), the filter is
        // empty. The fallback must mark the last heading so the working tree
        // retains a single "head" marker across empty-Re cycles.
        let doc = "\
<!-- agent:exchange -->
### Re: older
body
### Re: newer (HEAD)
more
<!-- /agent:exchange -->";
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: older".to_string());
        baseline.insert("### Re: newer".to_string());
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "last heading retains (HEAD) via fallback; got:\n{result}"
        );
        assert!(
            result.contains("### Re: older\n"),
            "older heading remains unmarked; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) marker after fallback; got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_strips_multiple_stale() {
        // Multiple stale (HEAD)s on prior headings → all stripped, only last gets it.
        let doc = "\
<!-- agent:exchange -->
### Re: one (HEAD)
a
### Re: two (HEAD)
b
### Re: three
c
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) after reposition; got:\n{result}"
        );
        assert!(result.contains("### Re: three (HEAD)"));
        assert!(result.contains("### Re: one\n"));
        assert!(result.contains("### Re: two\n"));
    }

    #[test]
    fn preserve_head_keeps_head_markers_on_reposition() {
        let doc = "\
<!-- agent:exchange -->
### Re: older
body a
### Re: newer (HEAD)
body b
<!-- agent:boundary:old-id -->
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_preserve_head(doc);
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "preserve_head must keep (HEAD) on newest heading; got:\n{result}"
        );
        assert!(
            !result.contains("old-id"),
            "old boundary should be removed; got:\n{result}"
        );
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "exactly one fresh boundary; got:\n{result}"
        );
    }

    #[test]
    fn preserve_head_with_id_keeps_head_and_uses_explicit_id() {
        let doc = "\
<!-- agent:exchange -->
### Re: topic (HEAD)
body
<!-- agent:boundary:old-id -->
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_preserve_head_with_id(doc, Some("explicit-id"));
        assert!(
            result.contains("### Re: topic (HEAD)"),
            "preserve_head must keep (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("<!-- agent:boundary:explicit-id -->"),
            "explicit boundary id should be used; got:\n{result}"
        );
        assert!(
            !result.contains("old-id"),
            "old boundary gone; got:\n{result}"
        );
    }

    #[test]
    fn clean_strips_head_but_preserve_head_keeps_it() {
        let doc = "\
<!-- agent:exchange -->
### Re: first
body a
### Re: second (HEAD)
body b
<!-- /agent:exchange -->";
        let clean = reposition_boundary_to_end_clean(doc);
        let preserved = reposition_boundary_to_end_preserve_head(doc);

        assert!(
            !clean.contains("(HEAD)"),
            "clean variant must strip (HEAD); got:\n{clean}"
        );
        assert!(
            preserved.contains("### Re: second (HEAD)"),
            "preserve_head variant must keep (HEAD); got:\n{preserved}"
        );
    }

    #[test]
    fn max_lines_inline_attr_trims_content() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:log patch=replace max_lines=3 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "line1\nline2\nline3\nline4\nline5\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(!result.contains("line1"));
        assert!(!result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(result.contains("line5"));
    }

    #[test]
    fn max_lines_noop_when_under_limit() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:log patch=replace max_lines=10 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "line1\nline2\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
    }

    #[test]
    fn max_lines_from_components_toml() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.log]\npatch = \"replace\"\nmax_lines = 2\n",
        )
        .unwrap();
        let doc = "<!-- agent:log -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "a\nb\nc\nd\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(!result.contains("\na\n"));
        assert!(!result.contains("\nb\n"));
        assert!(result.contains("c"));
        assert!(result.contains("d"));
    }

    #[test]
    fn max_lines_inline_beats_toml() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.log]\nmax_lines = 1\n",
        )
        .unwrap();
        let doc = "<!-- agent:log patch=replace max_lines=3 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "a\nb\nc\nd\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Inline max_lines=3 should win over toml max_lines=1
        assert!(result.contains("b"));
        assert!(result.contains("c"));
        assert!(result.contains("d"));
    }

    #[test]
    fn parse_patch_with_transfer_source_attr() {
        let response = "<!-- patch:exchange transfer-source=\"tasks/eval-runner.md\" -->\nTransferred content.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert_eq!(patches[0].content, "Transferred content.\n");
        assert_eq!(
            patches[0].attrs.get("transfer-source"),
            Some(&"\"tasks/eval-runner.md\"".to_string())
        );
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patch_without_attrs() {
        let response = "<!-- patch:exchange -->\nContent.\n<!-- /patch:exchange -->\n";
        let (patches, _) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert!(patches[0].attrs.is_empty());
    }

    #[test]
    fn parse_patch_with_multiple_attrs() {
        let response =
            "<!-- patch:output mode=replace max_lines=50 -->\nContent.\n<!-- /patch:output -->\n";
        let (patches, _) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "output");
        assert_eq!(patches[0].attrs.get("mode"), Some(&"replace".to_string()));
        assert_eq!(patches[0].attrs.get("max_lines"), Some(&"50".to_string()));
    }

    #[test]
    fn apply_patches_dedup_exchange_adjacent_echo() {
        // Simulates the bug: agent echoes user prompt as first line of exchange patch.
        // The existing exchange already ends with the prompt line.
        // After apply_patches, the prompt should appear exactly once.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
❯ How do I configure .mise.toml?
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        // Agent echoes the prompt as first line of its response patch
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "❯ How do I configure .mise.toml?\n\n### Re: configure .mise.toml\n\nUse `[env]` section.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();

        let count = result.matches("❯ How do I configure .mise.toml?").count();
        assert_eq!(
            count, 1,
            "prompt line should appear exactly once, got:\n{result}"
        );
        assert!(
            result.contains("### Re: configure .mise.toml"),
            "response heading should be present"
        );
        assert!(
            result.contains("Use `[env]` section."),
            "response body should be present"
        );
    }

    #[test]
    fn apply_patches_dedup_preserves_blank_lines() {
        // Blank lines between sections must not be collapsed by dedup.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
Previous response.
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "\n\n### Re: something\n\nAnswer here.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        assert!(
            result.contains("Previous response."),
            "existing content preserved"
        );
        assert!(
            result.contains("### Re: something"),
            "response heading present"
        );
        // Multiple blank lines should survive (dedup only targets non-blank)
        assert!(result.contains('\n'), "blank lines preserved");
    }

    #[test]
    fn apply_patches_dedup_preserves_adjacent_code_fences() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
Some text.
```
code block 1
```
```
code block 2
```
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: test — opus-4-6\n\nResponse.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        let fence_count = result.matches("```").count();
        assert_eq!(
            fence_count, 4,
            "all four code fences must survive dedup, got:\n{result}"
        );
        assert!(
            result.contains("```\n```"),
            "adjacent code fences must be preserved"
        );
    }

    #[test]
    fn apply_patches_marks_new_exchange_headings_with_head() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
### Re: earlier — gpt-5

Existing answer.
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: latest — gpt-5\n\nFresh answer.\n".to_string(),
            attrs: Default::default(),
        }];
        let result =
            apply_patches_with_overrides(doc, &patches, "", &doc_path, &Default::default())
                .unwrap();

        assert!(
            result.contains("### Re: earlier — gpt-5\n"),
            "existing response heading must stay clean; got:\n{result}"
        );
        assert!(
            result.contains("### Re: latest — gpt-5 (HEAD)\n"),
            "new response heading must surface as transient HEAD; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one transient HEAD marker expected; got:\n{result}"
        );
    }

    #[test]
    fn apply_patches_binds_exchange_response_to_oldest_matching_unresolved_prompt() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old-anchor -->\n",
            "❯ do #wcup1. spec-test-commit-push\n\n",
            "❯ do #wcx1. spec-test-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: #wcup1 — gpt-5\n\nAlready complete.\n".to_string(),
            attrs: Default::default(),
        }];
        let result =
            apply_patches_with_overrides(doc, &patches, "", &doc_path, &Default::default())
                .unwrap();

        let wcup1_prompt = result.find("❯ do #wcup1. spec-test-commit-push").unwrap();
        let wcup1_response = result.find("### Re: #wcup1 — gpt-5 (HEAD)").unwrap();
        let new_boundary = result.rfind("<!-- agent:boundary:").unwrap();
        let wcx1_prompt = result.find("❯ do #wcx1. spec-test-commit-push").unwrap();

        assert!(
            wcup1_prompt < wcup1_response,
            "response must land after the matched older prompt:\n{result}"
        );
        assert!(
            wcup1_response < new_boundary,
            "new boundary must move behind the response:\n{result}"
        );
        assert!(
            new_boundary < wcx1_prompt,
            "newer unresolved prompts must remain after the new boundary:\n{result}"
        );
    }

    #[test]
    fn apply_patches_rejects_exchange_response_that_skips_older_unresolved_prompt() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old-anchor -->\n",
            "❯ do #wcup1. spec-test-commit-push\n\n",
            "❯ do #wcx1. spec-test-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: #wcx1 — gpt-5\n\nNot done yet.\n".to_string(),
            attrs: Default::default(),
        }];
        let err = apply_patches_with_overrides(doc, &patches, "", &doc_path, &Default::default())
            .expect_err("later-matching response should fail closed");
        assert!(
            err.to_string().contains("skip older unresolved prompt"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_mode_append_strips_leading_overlap() {
        // When new_content starts with the last non-blank line of existing,
        // apply_mode("append") should not duplicate that line.
        let existing = "❯ How do I configure .mise.toml?\n";
        let new_content = "❯ How do I configure .mise.toml?\n\n### Re: configure\n\nUse `[env]`.\n";
        let result = apply_mode("append", existing, new_content);
        let count = result.matches("❯ How do I configure .mise.toml?").count();
        assert_eq!(count, 1, "overlap line should appear exactly once");
        assert!(result.contains("### Re: configure"));
    }

    #[test]
    fn strip_trailing_caret_removes_bare_prompt_line() {
        let content = "Answer text.\n❯\n";
        assert_eq!(strip_trailing_caret_lines(content), "Answer text.\n");
    }

    #[test]
    fn strip_trailing_caret_removes_multiple_trailing_lines() {
        let content = "Answer.\n❯\n❯\n";
        assert_eq!(strip_trailing_caret_lines(content), "Answer.\n");
    }

    #[test]
    fn strip_trailing_caret_preserves_mid_content_caret() {
        // `❯` mid-content (e.g. user prompt quoted in response) must survive.
        let content = "### Re: topic\n\n❯ user question echoed\n\nAnswer.\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn strip_trailing_caret_preserves_caret_with_text() {
        // Line that starts with `❯ ` and has other text is user content; don't strip.
        let content = "Answer.\n❯ follow-up\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn strip_trailing_caret_handles_no_trailing_newline() {
        let content = "Answer.\n❯";
        assert_eq!(strip_trailing_caret_lines(content), "Answer.");
    }

    #[test]
    fn strip_trailing_caret_noop_when_no_caret() {
        let content = "Answer.\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn apply_patches_strips_trailing_caret_from_exchange() {
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n❯ prior question\n<!-- /agent:exchange -->\n";
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: thing\n\nAnswer.\n❯\n".to_string(),
            attrs: Default::default(),
        }];
        let doc_path = std::path::PathBuf::from("/tmp/test.md");
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        // Extract just the exchange component content
        let components = component::parse(&result).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let content = exchange.content(&result);
        // No bare `❯` on its own line immediately before the boundary marker.
        let has_bare_caret_before_boundary = content
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w[0].trim() == "❯" && w[1].starts_with("<!-- agent:boundary"));
        assert!(
            !has_bare_caret_before_boundary,
            "bare ❯ line must not appear before boundary marker. content:\n{}",
            content
        );
    }

    #[test]
    fn apply_patches_preserves_caret_in_non_exchange() {
        // A patch targeting a non-exchange component should preserve trailing `❯`
        // (no special rule there).
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:notes patch=replace -->\n<!-- /agent:notes -->\n";
        let patches = vec![PatchBlock {
            name: "notes".to_string(),
            content: "note body\n❯\n".to_string(),
            attrs: Default::default(),
        }];
        let doc_path = std::path::PathBuf::from("/tmp/test.md");
        let result = apply_patches(doc, &patches, "", &doc_path).unwrap();
        let components = component::parse(&result).unwrap();
        let notes = components.iter().find(|c| c.name == "notes").unwrap();
        assert!(
            notes.content(&result).contains("❯"),
            "non-exchange content retains ❯"
        );
    }

    #[test]
    fn apply_mode_append_no_overlap_unchanged() {
        // When new_content does NOT start with the last non-blank line of existing,
        // apply_mode("append") should concatenate normally.
        let existing = "Previous content.\n";
        let new_content = "### Re: something\n\nAnswer.\n";
        let result = apply_mode("append", existing, new_content);
        assert_eq!(result, "Previous content.\n### Re: something\n\nAnswer.\n");
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_tail_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();

        assert!(
            assistant < exchange_close,
            "assistant tail should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close,
            "pending should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n\n",
            "## Todo / Backlog\n\n",
            "- not conversation content\n"
        );

        let err = repair_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string().contains("escaped `agent:exchange`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_plain_trailing_suffix_after_todo() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:todo patch=replace -->\n",
            "- [ ] backlog\n",
            "<!-- /agent:todo -->\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## Assistant\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## User\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let todo_open = repaired.find("<!-- agent:todo patch=replace -->").unwrap();
        let trailing_summary = repaired
            .rfind("Exchange compacted. No new work was run in this turn.")
            .unwrap();

        assert!(
            trailing_summary < exchange_close,
            "plain trailing suffix should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close && todo_open > exchange_close,
            "sibling components should stay outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_comment_only_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "comment-only suffix should stay outside exchange"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside ordinary HTML comments must not be moved into exchange"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_unterminated_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Still typing this scratch note.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside a transiently unclosed ordinary HTML comment must not be moved into exchange"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_gap_before_backlog_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: gap — gpt-5").unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "gap response should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_prompt_before_gap_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let prompt = repaired
            .find("do [#oobprompt]. spec-test-build-install-commit-push")
            .unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            prompt < exchange_close,
            "prompt should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_tail_moves_escaped_response_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "### Re: later — gpt-5\n\n",
            "Escaped answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_tail(doc)
            .unwrap()
            .expect("duplicate close repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: later — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "escaped response should move back inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave exactly one exchange close marker"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_scaffold_drops_inserted_template_shell() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "JB `Run Agent Doc` failed on this document.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc)
            .unwrap()
            .expect("duplicate scaffold repair should apply");

        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave one exchange close:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:queue -->").count(),
            1,
            "repair should drop the duplicated queue scaffold:\n{repaired}"
        );
        assert!(repaired.contains("JB `Run Agent Doc` failed on this document."));
        assert!(component::parse(&repaired).is_ok());
    }

    #[test]
    fn repair_duplicate_exchange_close_scaffold_rejects_mixed_user_text() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc).unwrap();

        assert!(
            repaired.is_none(),
            "mixed user text must not be dropped as duplicated scaffold"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_repairs_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_editor_visible_template_structure(doc)
            .expect("safe duplicate scaffold should repair before editor write");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        guard_no_conversation_tail_outside_exchange(&repaired).unwrap();
    }

    #[test]
    fn normalize_editor_visible_template_structure_scrubs_duplicate_prompt_html_comment_body() {
        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "{prompt}\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!--\n",
                "Keep this unrelated scratch note hidden.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("editor-visible normalization should scrub duplicate prompt residue");

        let duplicate_comment = format!("\n<!--\n{prompt}\n-->\n");
        assert!(
            !repaired.contains(&duplicate_comment),
            "editor-visible normalization must scrub duplicate post-exchange prompt text:\n{repaired}"
        );
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!--\nKeep this unrelated scratch note hidden."),
            "editor-visible normalization must preserve the ordinary HTML comment shell:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"),
            "tracked-work scaffold should remain intact:\n{repaired}"
        );
        assert!(
            repaired.contains("Keep this unrelated scratch note hidden."),
            "unrelated scratch comments must stay outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_rejects_plain_markdown_duplicate() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );

        let err = guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue outside"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_rejects_plain_markdown_duplicate_residue() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "{prompt}\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let err = normalize_editor_visible_template_structure(&doc).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "editor-visible normalization must fail closed on duplicate prompt Markdown residue: {err}"
        );
    }

    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_allows_tracked_components() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:backlog -->\n",
                "{prompt}\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap();
    }

    #[test]
    fn normalize_editor_visible_template_structure_rejects_mixed_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "user typed into duplicated shell\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );

        let err = normalize_editor_visible_template_structure(doc).unwrap_err();
        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_tail_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- /agent:exchange -->\n\n",
            "## Todo / Backlog\n\n",
            "- keep me outside exchange\n",
            "<!-- /agent:exchange -->\n"
        );

        let err = repair_duplicate_exchange_close_tail(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate close repair suffix is ambiguous"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_opener_merges_two_blocks() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ first question\n\n",
            "### Re: first — opus-4-6\n\n",
            "First answer.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ second question\n\n",
            "### Re: second — opus-4-6\n\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_opener(doc)
            .unwrap()
            .expect("duplicate opener repair should apply");
        assert_eq!(
            repaired.matches("<!-- agent:exchange").count(),
            1,
            "repair should leave exactly one exchange opener:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave exactly one exchange closer:\n{repaired}"
        );
        assert!(
            repaired.contains("First answer."),
            "first block content should be preserved:\n{repaired}"
        );
        assert!(
            repaired.contains("Second answer."),
            "second block content should be preserved:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->"),
            "backlog should be preserved:\n{repaired}"
        );
        let first_pos = repaired.find("First answer.").unwrap();
        let second_pos = repaired.find("Second answer.").unwrap();
        assert!(
            first_pos < second_pos,
            "first content should appear before second:\n{repaired}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_opener_returns_none_for_single() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ question\n",
            "### Re: answer — opus-4-6\n\n",
            "Answer.\n",
            "<!-- /agent:exchange -->\n"
        );

        let result = repair_duplicate_exchange_opener(doc).unwrap();
        assert!(result.is_none(), "single exchange block should return None");
    }

    #[test]
    fn strip_conversation_tail_outside_exchange_removes_escaped_heading_tail_only() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n"
        );

        let stripped = strip_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("escaped heading tail should be removable");
        assert!(
            stripped.contains("[//]: # (leave this note outside exchange)"),
            "comment-only note should remain outside exchange:\n{stripped}"
        );
        assert!(
            !stripped.contains("## Assistant"),
            "escaped assistant tail should be removed:\n{stripped}"
        );
    }

    #[test]
    fn strip_conversation_tail_outside_exchange_removes_gap_before_backlog_only() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let stripped = strip_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("escaped gap should be removable");
        assert!(
            stripped.contains("\n###\n\n<!-- agent:backlog -->"),
            "plain gap marker should stay outside exchange:\n{stripped}"
        );
        assert!(
            !stripped.contains("### Re: gap — gpt-5"),
            "escaped response should be removed from the gap:\n{stripped}"
        );
    }

    #[test]
    fn deleted_conversation_tail_cleanup_accepts_prompt_prelude_deletion() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "I see this routed prompt sitting outside exchange.\n",
            "It should have been entered by the managed pane.\n\n",
            "do #oobtaildel. spec-test-build-install-commit-push\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let cleaned = deleted_conversation_tail_cleanup(before, after)
            .unwrap()
            .expect("prompt-prelude cleanup should be accepted");
        assert_eq!(cleaned, after);
    }

    #[test]
    fn deleted_conversation_tail_cleanup_rejects_plain_note_deletion() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- /agent:exchange -->\n\n",
            "Design note: keep this outside exchange.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before.replace("Design note: keep this outside exchange.\n\n", "");

        let cleaned = deleted_conversation_tail_cleanup(before, &after).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary note deletion should not be treated as escaped conversation cleanup"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_normal_content() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ user question\n",
            "### Re: question — opus-4-6\n\n",
            "Answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] some task\n",
            "<!-- /agent:backlog -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_session_digest() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "### Re: earlier — opus-4-6\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "# Session Digest\n\n",
            "Summary of work.\n\n",
            "❯ Why were the backlog items removed?\n",
            "### Re: backlog — opus-4-6\n\n",
            "Escaped answer.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_gap_before_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_prompt_before_gap_marker() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_without_exchange_block() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status -->\n",
            "Active\n",
            "<!-- /agent:status -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_comment_only_tail() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }
}
