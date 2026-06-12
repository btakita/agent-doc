//! # Module: component
//!
//! ## Spec
//! - Defines `Component`, the parsed representation of a bounded document region delimited by
//!   `<!-- agent:name [attrs...] -->` (open) and `<!-- /agent:name -->` (close) HTML comments.
//! - `parse(doc)` scans the raw document bytes for `<!--` / `-->` comment pairs, builds a
//!   stack-based nesting model, and returns all `Component` values sorted by `open_start`.
//! - Markers that appear inside fenced code blocks (backtick or tilde) or inline code spans are
//!   skipped; `find_code_ranges(doc)` uses the `pulldown-cmark` AST (CommonMark-compliant) to
//!   locate these regions.
//! - `is_agent_marker(comment_text)` classifies whether the inner text of a comment is an
//!   agent open/close marker vs. an ordinary HTML comment.
//! - Component names must match `[a-zA-Z0-9][a-zA-Z0-9-]*`; invalid names, unmatched opens,
//!   unmatched closes, and mismatched open/close pairs all return `Err`.
//! - `boundary:*` prefixed markers (`<!-- agent:boundary:ID -->`) are recognised and skipped
//!   during component parsing; they are not treated as component open tags.
//! - Opening markers may carry space-separated `key=value` inline attributes
//!   (e.g., `patch=append`). `patch=` takes precedence over the legacy `mode=` alias;
//!   `patch_mode()` encapsulates this lookup.
//! - The non-agent comment fast path only previews comment text on UTF-8 char boundaries, so
//!   ordinary prose comments near multibyte glyphs never panic the parser.
//! - Marker `open_end` / `close_end` byte offsets include a trailing newline when present,
//!   so that content slices are clean.
//! - `replace_content(doc, new_content)` rebuilds the document preserving both markers,
//!   replacing only the bytes between them.
//! - `append_with_caret(doc, content, caret_offset)` appends content after existing text, or
//!   inserts before the caret line when the caret falls inside the component.
//! - `append_with_boundary(doc, content, boundary_id)` locates `<!-- agent:boundary:ID -->`
//!   inside the component (skipping any occurrence that lives inside a code block), replaces it
//!   with the new content, and re-inserts a fresh boundary marker; falls back to
//!   `append_with_caret` when no boundary is found.
//!
//! ## Agentic Contracts
//! - `parse` is pure and deterministic: identical input always yields identical output.
//! - All byte offsets (`open_start`, `open_end`, `close_start`, `close_end`) are valid UTF-8
//!   char boundaries within the document string they were parsed from.
//! - `content(doc)` returns exactly `&doc[open_end..close_start]` — no allocation.
//! - `replace_content` and `append_with_*` never mutate the original `&str`; they return a
//!   new `String` with all offsets consistent for a fresh `parse` call.
//! - Markers inside any code region (fenced block or inline span) are never parsed as
//!   components and never mutated by any append/replace operation.
//! - `append_with_boundary` always re-inserts a boundary marker with a fresh UUID, preserving
//!   the invariant that exactly one boundary exists inside the component after each write.
//! - Unknown or malformed `key=value` tokens in inline attributes are silently discarded;
//!   they never cause a parse error.
//!
//! ## Evals
//! - single_range: doc with one component → one `Component`, correct name and content slice
//! - nested_ranges: outer + inner components → two entries sorted outer-first
//! - siblings: two adjacent components → two entries, each with correct content
//! - no_ranges: plain markdown → empty vec, no error
//! - unmatched_open_error: open without close → `Err` containing "unclosed component"
//! - unmatched_close_error: close without open → `Err` containing "without matching open"
//! - mismatched_names_error: `<!-- agent:foo -->…<!-- /agent:bar -->` → `Err` "mismatched"
//! - invalid_name: name starting with `-` → `Err` "invalid component name"
//! - markers_in_fenced_code_block_ignored: marker inside ``` block → not parsed as component
//! - markers_in_inline_code_ignored: marker inside `` `…` `` span → not parsed
//! - markers_in_tilde_fence_ignored: marker inside ~~~ block → not parsed
//! - markers_in_indented_fenced_code_block_ignored: up-to-3-space-indented fence → not parsed
//! - double_backtick_comment_before_agent_marker: `` `<!--` `` followed by real marker → one component
//! - parse_component_with_patch_attr: `patch=append` on opening tag → `patch_mode()` = "append"
//! - patch_attr_takes_precedence_over_mode: both `patch=` and `mode=` present → `patch=` wins
//! - mode_attr_backward_compat: only `mode=append` present → `patch_mode()` = "append"
//! - replace_roundtrip: replace content, re-parse → one component with new content
//! - append_with_boundary_no_code_block: boundary found → content inserted, old ID consumed, new boundary present
//! - append_with_boundary_skips_code_block: boundary inside code block skipped, real boundary used

use anyhow::{Result, bail};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

/// Canonical component name for the backlog/pending component.
pub const BACKLOG_COMPONENT: &str = "backlog";

/// Legacy alias for the backlog component.
pub const BACKLOG_ALIAS: &str = "pending";

/// Canonical component name for the completed/reaped backlog archive.
pub const BACKLOG_DONE_COMPONENT: &str = "done";

/// Canonical component name for tracked work awaiting human review.
pub const REVIEW_COMPONENT: &str = "review";

/// Canonical component name for the icebox component.
pub const ICEBOX_COMPONENT: &str = "icebox";

/// Canonical component name for the conversation exchange.
const EXCHANGE_COMPONENT: &str = "exchange";

/// Check whether a component name refers to the backlog component
/// (accepts both canonical `"backlog"` and legacy `"pending"`).
pub fn is_backlog_component(name: &str) -> bool {
    name == BACKLOG_COMPONENT || name == BACKLOG_ALIAS
}

/// Check whether a component name refers to the completed/reaped backlog archive.
pub fn is_backlog_done_component(name: &str) -> bool {
    name == BACKLOG_DONE_COMPONENT
}

/// Check whether a component name refers to the review component.
pub fn is_review_component(name: &str) -> bool {
    name == REVIEW_COMPONENT
}

/// Check whether a component name refers to the icebox component.
pub fn is_icebox_component(name: &str) -> bool {
    name == ICEBOX_COMPONENT
}

/// Check whether a component name refers to tracked work that participates in
/// `do #id` closeout (`agent:backlog` / legacy `agent:pending`,
/// `agent:review`, and `agent:icebox`).
pub fn is_tracked_work_component(name: &str) -> bool {
    is_backlog_component(name) || is_review_component(name) || is_icebox_component(name)
}

/// Strip deprecated `patch=...` (and legacy `mode=...`) attributes from
/// backlog/pending component opening tags. The backlog component's patch mode
/// is always `replace` by built-in default; the inline attribute is redundant
/// since the binary owns all backlog mutations via `--pending-*` flags.
///
/// Emits a deprecation warning to stderr for each stripped attribute.
pub fn strip_backlog_patch_attr(doc: &str) -> String {
    let mut result = doc.to_string();
    let mut warned = false;

    for name in &[BACKLOG_COMPONENT, BACKLOG_ALIAS] {
        let prefix = format!("<!-- agent:{} ", name);
        while let Some(start) = result.find(&prefix) {
            let tag_start = start;
            let rest = &result[start + prefix.len()..];
            let Some(end_rel) = rest.find("-->") else {
                break;
            };
            let attrs_text = &rest[..end_rel];
            let tag_end = start + prefix.len() + end_rel + 3; // past `-->`

            let tokens: Vec<&str> = attrs_text.split_whitespace().collect();
            let has_patch_or_mode = tokens.iter().any(|t| {
                t.split_once('=')
                    .map(|(k, _)| k == "patch" || k == "mode")
                    .unwrap_or(false)
            });

            if !has_patch_or_mode {
                break;
            }

            if !warned {
                eprintln!(
                    "[deprecation] patch/mode attribute on agent:{} is deprecated and will be stripped — \
                     the binary owns backlog mutations via --pending-* flags",
                    name
                );
                warned = true;
            }

            let remaining: Vec<&str> = tokens
                .iter()
                .filter(|t| {
                    t.split_once('=')
                        .map(|(k, _)| k != "patch" && k != "mode")
                        .unwrap_or(true)
                })
                .copied()
                .collect();

            let new_tag = if remaining.is_empty() {
                format!("<!-- agent:{} -->", name)
            } else {
                format!("<!-- agent:{} {} -->", name, remaining.join(" "))
            };

            result.replace_range(tag_start..tag_end, &new_tag);
        }
    }

    result
}

/// A parsed component in a document.
///
/// Components are bounded regions marked by `<!-- agent:name -->...<!-- /agent:name -->`.
/// Opening tags may contain inline attributes: `<!-- agent:name key=value -->`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    /// Inline attributes parsed from the opening tag (e.g., `patch=append`).
    pub attrs: HashMap<String, String>,
    /// Byte offset of `<` in opening marker.
    pub open_start: usize,
    /// Byte offset past `>` in opening marker (includes trailing newline if present).
    pub open_end: usize,
    /// Byte offset of `<` in closing marker.
    pub close_start: usize,
    /// Byte offset past `>` in closing marker (includes trailing newline if present).
    pub close_end: usize,
}

impl Component {
    /// Extract the content between the opening and closing markers.
    #[allow(dead_code)] // public API — used by tests and future consumers
    pub fn content<'a>(&self, doc: &'a str) -> &'a str {
        &doc[self.open_end..self.close_start]
    }

    /// Get the patch mode from inline attributes.
    ///
    /// Checks `patch=` first, falls back to `mode=` for backward compatibility.
    pub fn patch_mode(&self) -> Option<&str> {
        self.attrs
            .get("patch")
            .map(|s| s.as_str())
            .or_else(|| self.attrs.get("mode").map(|s| s.as_str()))
    }

    /// Replace the content between markers, returning the new document.
    /// The markers themselves are preserved.
    pub fn replace_content(&self, doc: &str, new_content: &str) -> String {
        let mut result = String::with_capacity(doc.len() + new_content.len());
        result.push_str(&doc[..self.open_end]);
        result.push_str(new_content);
        result.push_str(&doc[self.close_start..]);
        result
    }

    /// Append content into this component, inserting before the caret position
    /// if the caret is inside the component. Falls back to normal append if the
    /// caret is outside the component.
    ///
    /// `caret_offset`: byte offset of the caret in the document. Pass `None` for
    /// normal append behavior.
    pub fn append_with_caret(
        &self,
        doc: &str,
        content: &str,
        caret_offset: Option<usize>,
    ) -> String {
        let existing = &doc[self.open_end..self.close_start];
        if append_patch_already_present(existing, content) {
            return doc.to_string();
        }

        if let Some(caret) = caret_offset {
            // Check if caret is inside this component
            if caret > self.open_end && caret <= self.close_start {
                // Find the line boundary before the caret
                let insert_at = doc[..caret]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(self.open_end);

                // Clamp to component bounds
                let insert_at = insert_at.max(self.open_end);
                let prior = &doc[self.open_end..insert_at];

                let mut result = String::with_capacity(doc.len() + content.len() + 1);
                result.push_str(&doc[..insert_at]);
                if needs_exchange_heading_separator(&self.name, prior, content) {
                    result.push('\n');
                }
                result.push_str(content.trim_end());
                result.push('\n');
                result.push_str(&doc[insert_at..]);
                return result;
            }
        }

        // Normal append: add after existing content
        let mut result = String::with_capacity(doc.len() + content.len() + 1);
        result.push_str(&doc[..self.open_end]);
        let trimmed_existing = existing.trim_end();
        result.push_str(trimmed_existing);
        if needs_exchange_heading_separator(&self.name, trimmed_existing, content) {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(content.trim_end());
        result.push('\n');
        result.push_str(&doc[self.close_start..]);
        result
    }

    /// Append content into this component at the boundary marker position.
    ///
    /// Finds `<!-- agent:boundary:ID -->` inside the component. If found,
    /// inserts content at the line start of the boundary marker (replacing
    /// the marker). Falls back to normal append if the boundary is not found.
    pub fn append_with_boundary(&self, doc: &str, content: &str, boundary_id: &str) -> String {
        let boundary_marker = format!("<!-- agent:boundary:{} -->", boundary_id);
        let content_region = &doc[self.open_end..self.close_start];
        if append_patch_already_present(content_region, content) {
            return doc.to_string();
        }
        let code_ranges = find_code_ranges(doc);

        // Search for boundary marker, skipping matches inside code blocks
        let mut search_from = 0;
        let found_pos = loop {
            match content_region[search_from..].find(&boundary_marker) {
                Some(rel_pos) => {
                    let abs_pos = self.open_end + search_from + rel_pos;
                    if code_ranges
                        .iter()
                        .any(|&(cs, ce)| abs_pos >= cs && abs_pos < ce)
                    {
                        // Inside a code block — skip and keep searching
                        search_from += rel_pos + boundary_marker.len();
                        continue;
                    }
                    break Some(abs_pos);
                }
                None => break None,
            }
        };

        if let Some(abs_pos) = found_pos {
            // Find start of the line containing the marker
            let line_start = doc[..abs_pos]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(self.open_end)
                .max(self.open_end);

            // Find end of the marker line (including trailing newline)
            let marker_end = abs_pos + boundary_marker.len();
            let line_end = if marker_end < self.close_start
                && doc.as_bytes().get(marker_end) == Some(&b'\n')
            {
                marker_end + 1
            } else {
                marker_end
            };
            let line_end = line_end.min(self.close_start);

            // Replace the boundary marker with response content + new boundary.
            // The boundary is consumed and re-inserted, matching the binary's
            // post-patch behavior in apply_patches_with_overrides().
            let new_id = crate::id::new_boundary_id();
            let new_marker = crate::id::format_boundary_marker(&new_id);
            let mut result = String::with_capacity(doc.len() + content.len() + new_marker.len());
            let prior = &doc[self.open_end..line_start];
            result.push_str(&doc[..line_start]);
            if needs_exchange_heading_separator(&self.name, prior, content) {
                result.push('\n');
            }
            result.push_str(content.trim_end());
            result.push('\n');
            result.push_str(&new_marker);
            result.push('\n');
            result.push_str(&doc[line_end..]);
            return result;
        }

        // Boundary not found — fall back to normal append
        self.append_with_caret(doc, content, None)
    }
}

/// Converge the `agent:queue` opening-tag `auto` attribute to `want_auto`.
///
/// A content patch (the only editor-visible IPC seam for template documents)
/// replaces a component's *body* between its markers — it cannot add or remove
/// an attribute on the `<!-- agent:queue auto -->` opening tag. After a queue
/// halt strips `auto` on disk, a live route-owned editor buffer that still shows
/// `<!-- agent:queue auto -->` re-diverges from the committed inactive snapshot on
/// its next flush, regenerating the snapshot/HEAD drift loop on every preflight
/// (`#adoc-queue-ipc-buffer-divergence`). Editor plugins call this (via the
/// `agent_doc_converge_queue_auto` FFI export) to converge the live buffer's queue
/// tag to the committed shape.
///
/// Returns `Some(new_doc)` when the tag changed, `None` when there is no `queue`
/// component or the tag already matches `want_auto`. Other opening-tag attributes
/// (e.g. `patch=append`) are preserved.
pub fn converge_queue_auto(doc: &str, want_auto: bool) -> Option<String> {
    let components = parse(doc).ok()?;
    let queue = components.iter().find(|c| c.name == "queue")?;
    let raw_tag = &doc[queue.open_start..queue.open_end];
    let new_tag = set_auto_in_queue_tag(raw_tag, want_auto);
    if new_tag == *raw_tag {
        return None;
    }
    let mut result = String::with_capacity(doc.len());
    result.push_str(&doc[..queue.open_start]);
    result.push_str(&new_tag);
    result.push_str(&doc[queue.open_end..]);
    Some(result)
}

/// Rebuild an `agent:queue` opening tag with the `auto` attribute present or
/// absent per `want_auto`, preserving every other token (the `agent:queue` name,
/// `patch=…`, etc.) and any trailing whitespace/newline after `-->`.
fn set_auto_in_queue_tag(tag: &str, want_auto: bool) -> String {
    let Some((mut tokens, tail)) = queue_tag_tokens(tag) else {
        return tag.to_string();
    };
    tokens = tokens
        .into_iter()
        .map(|token| normalize_queue_tag_token(&token))
        .collect();
    let has_auto = tokens.iter().any(|token| token == "auto");
    if want_auto == has_auto {
        return format!("<!-- {} -->{}", tokens.join(" "), tail);
    }
    if want_auto {
        // Insert `auto` right after the `agent:queue` name token so it stays the
        // first attribute (matches how route/queue activation writes the tag).
        let insert_at = if tokens.is_empty() { 0 } else { 1 };
        tokens.insert(insert_at, "auto".to_string());
    } else {
        tokens.retain(|token| token != "auto");
    }
    format!("<!-- {} -->{}", tokens.join(" "), tail)
}

fn queue_tag_tokens(tag: &str) -> Option<(Vec<String>, &str)> {
    let close_idx = tag.find("-->")?;
    let core = &tag[..close_idx + 3];
    let tail = &tag[close_idx + 3..];
    let inner = core
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim();
    Some((split_marker_tokens(inner), tail))
}

fn split_marker_tokens(inner: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        if ch.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn normalize_queue_tag_token(token: &str) -> String {
    let Some((key, value)) = token.split_once('=') else {
        return token.to_string();
    };
    if is_queue_boolean_attr(key) && value.eq_ignore_ascii_case("true") {
        return key.to_string();
    }
    if key == "preset"
        && let Some(stripped) = strip_malformed_true_suffix(value)
    {
        return format!("{key}={stripped}");
    }
    token.to_string()
}

fn is_queue_boolean_attr(key: &str) -> bool {
    matches!(key, "auto" | "priority" | "go" | "start" | "stop")
}

fn strip_malformed_true_suffix(value: &str) -> Option<&str> {
    let stripped = value.strip_suffix("=true")?;
    if (stripped.starts_with('"') && stripped.ends_with('"'))
        || (stripped.starts_with('\'') && stripped.ends_with('\''))
    {
        Some(stripped)
    } else {
        None
    }
}

/// Valid name: `[a-zA-Z0-9][a-zA-Z0-9-]*`
fn is_valid_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn append_patch_already_present(existing: &str, content: &str) -> bool {
    let patch = normalize_append_patch_content(content);
    if patch.is_empty() {
        return false;
    }
    let existing_norm = normalize_append_patch_content(existing);
    if existing_norm.contains(&patch) {
        return true;
    }
    // #prompt-duplicated-while-typing (L1 structural buffer-snapshot race): a
    // synthesized boundary-aware exchange patch carries an *earlier* keystroke
    // snapshot of the live user region (T1 — e.g. a half-typed line "Add to ")
    // while the editor buffer kept being typed (T2 — "Add to resume"). The two
    // texts then diverge mid-region, so the `contains` substring check above
    // misses the duplicate and the whole tail re-appends from the divergence
    // point. Recognize when the patch aligns to a contiguous run of the existing
    // region under per-line prefix matching (each patch line is a prefix of, or
    // equal to, the aligned existing line) and treat it as already present.
    // This can only *suppress* re-appending text the live region already
    // supersedes — the no-op keeps the longer live buffer — so it can never
    // delete content or collapse a genuinely distinct prompt.
    patch_is_typing_snapshot_of(&existing_norm, &patch)
}

/// True when `patch` is an in-progress keystroke-snapshot of a contiguous run
/// already present in `existing`: every patch line equals, or is a non-empty
/// prefix of, the aligned existing line, with at least one such prefix
/// divergence (an exact run is already handled by the `contains` check). Both
/// inputs must already be normalized via `normalize_append_patch_content`.
fn patch_is_typing_snapshot_of(existing: &str, patch: &str) -> bool {
    let e: Vec<&str> = existing.lines().collect();
    let p: Vec<&str> = patch.lines().collect();
    if p.is_empty() || p.len() > e.len() {
        return false;
    }
    'outer: for start in 0..=(e.len() - p.len()) {
        let mut saw_prefix_divergence = false;
        for (j, pl) in p.iter().enumerate() {
            let el = e[start + j];
            if pl == &el {
                continue;
            }
            // An empty patch line must match exactly — `starts_with("")` is
            // always true and would let a blank line align to anything.
            if !pl.is_empty() && el.starts_with(pl) {
                saw_prefix_divergence = true;
                continue;
            }
            continue 'outer;
        }
        if saw_prefix_divergence {
            return true;
        }
    }
    false
}

fn needs_exchange_heading_separator(component_name: &str, prior: &str, content: &str) -> bool {
    component_name == EXCHANGE_COMPONENT
        && !prior.trim().is_empty()
        && !prior.ends_with("\n\n")
        && starts_with_response_heading(content)
}

fn starts_with_response_heading(content: &str) -> bool {
    content.trim_start().starts_with("### Re:")
}

fn normalize_append_patch_content(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with(" -->") {
                None
            } else if markdown_heading_has_head_marker(line) {
                Some(strip_user_prompt_prefix(line.trim_end_matches(" (HEAD)")))
            } else {
                Some(strip_user_prompt_prefix(line))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Strip a leading `❯ ` / `❯` user-prompt prefix from a line for dedup comparison.
///
/// The exchange user-prompt normalization adds a `❯ ` prefix to user lines, but a
/// synthesized boundary-aware exchange patch and the live editor buffer can differ
/// only by that prefix (one normalized, the other not). Without stripping it, the
/// `append_patch_already_present` `contains` check misses the duplicate and the
/// prompt is re-appended — the `#prompt-duplicated-while-typing` duplication. The
/// `❯` glyph is a presentation prefix, not prompt content, so stripping it for
/// comparison cannot drop a genuinely distinct prompt. Lines without the prefix are
/// returned unchanged.
fn strip_user_prompt_prefix(line: &str) -> String {
    let trimmed_start = line.trim_start();
    if let Some(rest) = trimmed_start.strip_prefix("❯ ") {
        rest.to_string()
    } else if let Some(rest) = trimmed_start.strip_prefix('❯') {
        rest.trim_start().to_string()
    } else {
        line.to_string()
    }
}

fn markdown_heading_has_head_marker(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed.as_bytes().get(hashes) == Some(&b' ')
        && line.ends_with(" (HEAD)")
}

/// True if the text inside `<!-- ... -->` is an agent component marker.
///
/// Matches `agent:NAME [attrs...]` (open) or `/agent:NAME` (close).
pub fn is_agent_marker(comment_text: &str) -> bool {
    let trimmed = comment_text.trim();
    if let Some(rest) = trimmed.strip_prefix("/agent:") {
        is_valid_name(rest)
    } else if let Some(rest) = trimmed.strip_prefix("agent:") {
        // Opening marker may have attributes after the name: `agent:NAME key=value`
        let name_part = rest.split_whitespace().next().unwrap_or("");
        is_valid_name(name_part)
    } else {
        false
    }
}

fn bounded_preview(doc: &str, start: usize, max_bytes: usize) -> &str {
    debug_assert!(doc.is_char_boundary(start));
    let mut end = doc.len().min(start.saturating_add(max_bytes));
    while end > start && !doc.is_char_boundary(end) {
        end -= 1;
    }
    &doc[start..end]
}

/// Parse `key=value` pairs from the attribute portion of an opening marker.
///
/// Given the text after `agent:NAME `, parses space-separated `key=value` pairs.
/// Values are unquoted (no quote support needed for simple mode values).
pub fn parse_attrs(attr_text: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    for token in attr_text.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            if !key.is_empty() && !value.is_empty() {
                attrs.insert(key.to_string(), value.to_string());
            }
        } else if !token.is_empty() {
            attrs.insert(token.to_string(), String::new());
        }
    }
    attrs
}

/// Find byte ranges of code regions (fenced code blocks + inline code spans).
/// Markers inside these ranges are treated as literal text, not component markers.
///
/// Uses `pulldown-cmark` AST parsing with `offset_iter()` to accurately detect
/// code regions per the CommonMark spec.
pub fn find_code_ranges(doc: &str) -> Vec<(usize, usize)> {
    let t = std::time::Instant::now();
    let mut ranges = Vec::new();
    let parser = Parser::new_ext(doc, Options::empty());
    let mut iter = parser.into_offset_iter();
    while let Some((event, range)) = iter.next() {
        match event {
            // Inline code span: `code` or ``code``
            Event::Code(_) => {
                ranges.push((range.start, range.end));
            }
            // Fenced or indented code block: consume until End(CodeBlock)
            Event::Start(Tag::CodeBlock(_)) => {
                let block_start = range.start;
                let mut block_end = range.end;
                for (inner_event, inner_range) in iter.by_ref() {
                    block_end = inner_range.end;
                    if matches!(inner_event, Event::End(TagEnd::CodeBlock)) {
                        break;
                    }
                }
                ranges.push((block_start, block_end));
            }
            _ => {}
        }
    }
    let elapsed = t.elapsed().as_millis();
    if elapsed > 0 {
        eprintln!("[perf] find_code_ranges: {}ms", elapsed);
    }
    ranges
}

/// Find byte ranges for ordinary HTML comments, skipping code spans/blocks and
/// preserving `agent:` markers for the component parser.
///
/// If an ordinary comment is currently unterminated, treat the rest of the
/// document as comment body. Users can be editing a scratch comment while an
/// agent-doc cycle is running; prompt-like text in that transiently broken
/// comment must not be interpreted as escaped exchange content and moved or
/// stripped by repair.
pub fn find_non_agent_html_comment_ranges(doc: &str) -> Vec<(usize, usize)> {
    let code_ranges = find_code_ranges(doc);
    let in_code = |pos: usize| {
        code_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let bytes = doc.as_bytes();
    let len = bytes.len();
    let mut ranges = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            pos += 1;
            continue;
        }
        if in_code(pos) {
            pos += 4;
            continue;
        }
        let Some(end) = find_comment_end(bytes, pos + 4) else {
            let inner = &doc[pos + 4..];
            if !is_agent_marker(inner) && !inner.trim_start().starts_with("agent:boundary:") {
                ranges.push((pos, len));
                break;
            }
            pos += 4;
            continue;
        };
        let inner = &doc[pos + 4..end - 3];
        if !is_agent_marker(inner) && !inner.trim().starts_with("agent:boundary:") {
            ranges.push((pos, end));
        }
        pos = end;
    }
    ranges
}

/// Parse all components from a document.
///
/// Uses a stack for nesting. Returns components sorted by `open_start`.
/// Errors on unmatched open/close markers or invalid names.
/// Skips markers inside fenced code blocks and inline code spans.
pub fn parse(doc: &str) -> Result<Vec<Component>> {
    let bytes = doc.as_bytes();
    let len = bytes.len();
    let code_ranges = find_code_ranges(doc);
    let mut templates: Vec<Component> = Vec::new();
    // Stack of (name, attrs, open_start, open_end)
    let mut stack: Vec<(String, HashMap<String, String>, usize, usize)> = Vec::new();
    let mut pos = 0;

    while pos + 4 <= len {
        // Look for `<!--`
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

        // Quick peek: only consume `<!-- ... -->` if the text after `<!--`
        // starts with ` agent:` or ` /agent:` (with optional whitespace).
        // Non-agent `<!--` sequences (e.g., `<!-- )` in prose) must NOT be
        // consumed — their `-->` search would eat real component close markers.
        let peek_start = pos + 4;
        let peek = bounded_preview(doc, peek_start, 20);
        let peek_trimmed = peek.trim_start();
        if !peek_trimmed.starts_with("agent:") && !peek_trimmed.starts_with("/agent:") {
            pos += 1;
            continue;
        }

        let marker_start = pos;

        // Find closing `-->`
        let close = match find_comment_end(bytes, pos + 4) {
            Some(c) => c,
            None => {
                pos += 4;
                continue;
            }
        };

        // close points to the byte after `>`
        let inner = &doc[marker_start + 4..close - 3]; // between `<!--` and `-->`
        let trimmed = inner.trim();

        // Determine end offset — consume trailing newline if present
        let mut marker_end = close;
        if marker_end < len && bytes[marker_end] == b'\n' {
            marker_end += 1;
        }

        if let Some(name) = trimmed.strip_prefix("/agent:") {
            // Closing marker
            if !is_valid_name(name) {
                bail!("invalid component name: '{}'", name);
            }
            match stack.pop() {
                Some((open_name, open_attrs, open_start, open_end)) => {
                    if open_name != name {
                        bail!(
                            "mismatched component: opened '{}' but closed '{}'",
                            open_name,
                            name
                        );
                    }
                    templates.push(Component {
                        name: name.to_string(),
                        attrs: open_attrs,
                        open_start,
                        open_end,
                        close_start: marker_start,
                        close_end: marker_end,
                    });
                }
                None => bail!(
                    "closing marker <!-- /agent:{} --> without matching open",
                    name
                ),
            }
        } else if let Some(rest) = trimmed.strip_prefix("agent:") {
            // Skip boundary markers — these are not component markers
            if rest.starts_with("boundary:") {
                pos = close;
                continue;
            }
            // Opening marker — may have attributes: `agent:NAME key=value`
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let name = parts.next().unwrap_or("");
            let attr_text = parts.next().unwrap_or("");
            if !is_valid_name(name) {
                bail!("invalid component name: '{}'", name);
            }
            let attrs = parse_attrs(attr_text);
            stack.push((name.to_string(), attrs, marker_start, marker_end));
        }

        pos = close;
    }

    if let Some((name, _, _, _)) = stack.last() {
        bail!(
            "unclosed component: <!-- agent:{} --> without matching close",
            name
        );
    }

    templates.sort_by_key(|t| t.open_start);
    Ok(templates)
}

/// Find the end of an HTML comment (`-->`), returning byte offset past `>`.
pub fn find_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

/// Strip comments from document content for diff comparison.
///
/// Removes:
/// - HTML comments `<!-- ... -->` (single and multiline) — EXCEPT agent range markers
/// - Link reference comments `[//]: # (...)`
///
/// Skips `<!--` sequences inside fenced code blocks and inline backtick spans
/// to prevent code examples containing `<!--` from being misinterpreted as
/// comment starts.
///
/// This function is the shared implementation used by both `diff::compute` (binary)
/// and external crates like `eval-runner`.
pub fn strip_comments(content: &str) -> String {
    let code_ranges = find_code_ranges(content);
    let in_code = |pos: usize| {
        code_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end)
    };

    let mut result = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        // Check for link reference comment: `[//]: # (...)`
        if bytes[pos] == b'['
            && !in_code(pos)
            && is_line_start_at(bytes, pos)
            && let Some(end) = match_link_ref_comment_at(bytes, pos)
        {
            pos = end;
            continue;
        }

        // Check for HTML comment: `<!-- ... -->`
        if pos + 4 <= len
            && &bytes[pos..pos + 4] == b"<!--"
            && !in_code(pos)
            && let Some((end, inner)) = match_html_comment_at(content, pos)
        {
            if is_agent_marker(inner) {
                // Preserve agent markers — copy them through
                result.push_str(&content[pos..end]);
                pos = end;
            } else {
                // Strip the comment (and trailing newline if on its own line)
                let mut skip_to = end;
                if is_line_start_at(bytes, pos) && skip_to < len && bytes[skip_to] == b'\n' {
                    skip_to += 1;
                }
                pos = skip_to;
            }
            continue;
        }

        result.push(content[pos..].chars().next().unwrap());
        pos += content[pos..].chars().next().unwrap().len_utf8();
    }

    result
}

/// True if `pos` is at the start of a line (pos == 0 or bytes[pos-1] == '\n').
fn is_line_start_at(bytes: &[u8], pos: usize) -> bool {
    pos == 0 || bytes[pos - 1] == b'\n'
}

/// Match `[//]: # (...)` starting at `pos`. Returns byte offset past the line end.
fn match_link_ref_comment_at(bytes: &[u8], pos: usize) -> Option<usize> {
    let prefix = b"[//]: # (";
    let len = bytes.len();
    if pos + prefix.len() > len || &bytes[pos..pos + prefix.len()] != prefix {
        return None;
    }
    let mut i = pos + prefix.len();
    while i < len && bytes[i] != b')' && bytes[i] != b'\n' {
        i += 1;
    }
    if i < len && bytes[i] == b')' {
        i += 1;
        if i < len && bytes[i] == b'\n' {
            i += 1;
        }
        Some(i)
    } else {
        None
    }
}

/// Match `<!-- ... -->` starting at `pos`. Returns (end_offset, inner_text).
fn match_html_comment_at(content: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = pos + 4;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            let inner = &content[pos + 4..i];
            return Some((i + 3, inner));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests;
