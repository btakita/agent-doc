//! # Module: element
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

use anyhow::Result;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;
use std::fmt;

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

/// Structural failures emitted while parsing document components.
///
/// Callers that implement narrowly scoped repairs should branch on these
/// variants instead of coupling recovery behavior to the rendered diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentParseError {
    InvalidName {
        name: String,
    },
    Mismatched {
        open_name: String,
        close_name: String,
    },
    UnmatchedClose {
        name: String,
    },
    Unclosed {
        name: String,
    },
}

impl fmt::Display for ComponentParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { name } => write!(f, "invalid component name: '{name}'"),
            Self::Mismatched {
                open_name,
                close_name,
            } => write!(
                f,
                "mismatched component: opened '{open_name}' but closed '{close_name}'"
            ),
            Self::UnmatchedClose { name } => write!(
                f,
                "closing marker for component '{name}' without matching open marker"
            ),
            Self::Unclosed { name } => write!(
                f,
                "unclosed component: <!-- agent:{name} --> without matching close"
            ),
        }
    }
}

impl std::error::Error for ComponentParseError {}

/// Return the component name when an error chain contains an unmatched close.
pub fn unmatched_component_close_name(error: &anyhow::Error) -> Option<&str> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<ComponentParseError>()
            .and_then(|parse_error| match parse_error {
                ComponentParseError::UnmatchedClose { name } => Some(name.as_str()),
                _ => None,
            })
    })
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

            // Replace the boundary marker with response content + the SAME
            // boundary, moved to the new end of the component.
            //
            // `#boundarystableid`: this used to mint a fresh id on every append,
            // which is why 162 snapshots of one document held 162 distinct
            // boundary ids. The marker is a position, not an event: re-minting
            // gives the live projection and the committed blob different ids for
            // the same boundary, so they differ by exactly that line and no later
            // cycle can settle it — each side keeps re-minting its own. Moving the
            // existing id keeps the two convergent.
            //
            // The old comment claimed this matched `apply_patches_with_overrides`,
            // but that path mints only when the boundary was LOST (its snowball
            // guard), never on an ordinary append. The two were never actually
            // matched.
            let new_marker = crate::id::format_boundary_marker(boundary_id);
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

fn is_agent_boundary_marker(comment_text: &str) -> bool {
    let trimmed = comment_text.trim();
    if let Some(rest) = trimmed.strip_prefix("/agent:") {
        rest.starts_with("boundary:")
    } else if let Some(rest) = trimmed.strip_prefix("agent:") {
        rest.split_whitespace()
            .next()
            .is_some_and(|name| name.starts_with("boundary:"))
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
///
/// Values MAY be quoted with `"` or `'`. Quoting does two things: it lets a value
/// contain spaces, and — the reason this exists — it is a form agent-doc's own
/// writers emit, so the parser has to accept it.
///
/// This used to keep the value verbatim, quotes included, on the assumption that
/// "values are unquoted (no quote support needed)". A compact wrote
/// `agent:done archive="tasks/x.done.md"` and every subsequent preflight and
/// session-check on that document hard-failed with
/// `archive=… must point to a .done.md file` — because the parsed value ended in a
/// quote character, so the `.done.md` suffix check could never match. The document
/// was unusable until the quotes were hand-stripped. A parser that rejects what
/// the writers emit is the bug; be liberal here.
pub fn parse_attrs(attr_text: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    for (key, value) in split_attr_tokens(attr_text) {
        if key.is_empty() {
            continue;
        }
        match value {
            Some(value) if !value.is_empty() => {
                attrs.insert(key, value);
            }
            // `key=` with an empty value is not a flag — it is a malformed pair,
            // and dropping it preserves the previous behaviour.
            Some(_) => {}
            None => {
                attrs.insert(key, String::new());
            }
        }
    }
    attrs
}

/// Split an attribute string into `(key, Some(value) | None)` pairs, honouring
/// quotes around values so a quoted value may contain spaces.
///
/// `None` marks a bare token (a boolean flag); `Some("")` marks a `key=` pair
/// with no value.
fn split_attr_tokens(attr_text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut chars = attr_text.chars().peekable();

    loop {
        // Skip separating whitespace.
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        // Key: up to `=` or whitespace. A key is never quoted.
        let mut key = String::new();
        let mut saw_eq = false;
        while let Some(&c) = chars.peek() {
            if c == '=' {
                saw_eq = true;
                chars.next();
                break;
            }
            if c.is_whitespace() {
                break;
            }
            key.push(c);
            chars.next();
        }

        if !saw_eq {
            out.push((key, None));
            continue;
        }

        // Value: quoted (spaces allowed, closing quote consumed) or bare.
        let mut value = String::new();
        match chars.peek().copied() {
            Some(quote @ ('"' | '\'')) => {
                chars.next();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == quote {
                        closed = true;
                        break;
                    }
                    value.push(c);
                }
                // An unterminated quote is tolerated: everything to end-of-input
                // is the value. Failing closed here would make one stray quote
                // unparse the whole marker, which is the failure mode this
                // function is being fixed for.
                let _ = closed;
            }
            _ => {
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    value.push(c);
                    chars.next();
                }
            }
        }
        out.push((key, Some(value)));
    }

    out
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

/// Find byte ranges of same-line double-quoted spans (`"..."`) — `#0kjc`/`#mdastquote`.
///
/// A `<!-- agent:* -->` marker (or other tag-looking token) embedded inside a
/// double-quoted prose span — for example a queue/backlog item that quotes a
/// marker by name like `"<!-- agent:queue -->"` — is content, not a real
/// document tag, and must not be parsed as one (same family as the code-span /
/// code-fence masking in [`find_code_ranges`], and as `#lintfence`).
///
/// Quotes are scoped to a single line, so an unterminated quote only masks the
/// rest of its own line (fail-safe: it can never swallow a real marker on a
/// later line). The opening `"` is included in the returned span. A real
/// marker's own attribute value (for example `preset="..."`) sits to the right
/// of the marker's `<!--`, so the marker's start byte is never inside one of
/// these spans and stays parseable. Mirrors the `\"`-escape handling of
/// `crate::gate_verify`'s `strip_quoted_spans` (`#gng8`).
pub fn find_quoted_ranges(doc: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    for raw_line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += raw_line.len();
        let bytes = raw_line.as_bytes();
        let line_content_len = raw_line.trim_end_matches(['\n', '\r']).len();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] != b'"' {
                i += 1;
                continue;
            }
            let span_start = line_start + i;
            let mut j = i + 1;
            let mut closed = false;
            while j < bytes.len() {
                match bytes[j] {
                    b'\\' => {
                        j += 2;
                        continue;
                    }
                    b'"' => {
                        closed = true;
                        break;
                    }
                    b'\n' | b'\r' => break,
                    _ => j += 1,
                }
            }
            if closed {
                ranges.push((span_start, line_start + j + 1));
                i = j + 1;
            } else {
                // Unterminated quote: mask to end of line (fail-safe), then stop
                // scanning this line.
                ranges.push((span_start, line_start + line_content_len));
                break;
            }
        }
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
    let quoted_ranges = find_quoted_ranges(doc);
    let in_code = |pos: usize| {
        code_ranges
            .iter()
            .chain(quoted_ranges.iter())
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
            if !is_agent_marker(inner) && !is_agent_boundary_marker(inner) {
                ranges.push((pos, len));
                break;
            }
            pos += 4;
            continue;
        };
        let inner = &doc[pos + 4..end - 3];
        if !is_agent_marker(inner) && !is_agent_boundary_marker(inner) {
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
    let quoted_ranges = find_quoted_ranges(doc);
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

        // Skip markers inside code regions or double-quoted prose spans
        // (a quoted `"<!-- agent:* -->"` is content, not a real marker — `#0kjc`).
        if code_ranges
            .iter()
            .chain(quoted_ranges.iter())
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

        if is_agent_boundary_marker(trimmed) {
            pos = close;
            continue;
        }

        if let Some(name) = trimmed.strip_prefix("/agent:") {
            // Closing marker
            if !is_valid_name(name) {
                return Err(ComponentParseError::InvalidName {
                    name: name.to_string(),
                }
                .into());
            }
            match stack.pop() {
                Some((open_name, open_attrs, open_start, open_end)) => {
                    if open_name != name {
                        return Err(ComponentParseError::Mismatched {
                            open_name,
                            close_name: name.to_string(),
                        }
                        .into());
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
                None => {
                    return Err(ComponentParseError::UnmatchedClose {
                        name: name.to_string(),
                    }
                    .into());
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("agent:") {
            // Skip boundary markers — these are not component markers
            if is_agent_boundary_marker(trimmed) {
                pos = close;
                continue;
            }
            // Opening marker — may have attributes: `agent:NAME key=value`
            let mut parts = rest.splitn(2, |c: char| c.is_whitespace());
            let name = parts.next().unwrap_or("");
            let attr_text = parts.next().unwrap_or("");
            if !is_valid_name(name) {
                return Err(ComponentParseError::InvalidName {
                    name: name.to_string(),
                }
                .into());
            }
            let attrs = parse_attrs(attr_text);
            stack.push((name.to_string(), attrs, marker_start, marker_end));
        }

        pos = close;
    }

    if let Some((name, _, _, _)) = stack.last() {
        return Err(ComponentParseError::Unclosed { name: name.clone() }.into());
    }

    templates.sort_by_key(|t| t.open_start);
    Ok(templates)
}

/// Singleton components that must appear at most once in a structurally valid
/// document. Mirrors the singleton list compaction's item-count guard uses
/// (`#compactdropitem`). Components not listed here (e.g. `log`) may repeat.
const SINGLETON_COMPONENT_NAMES: &[&str] = &[
    EXCHANGE_COMPONENT,
    "status",
    "queue",
    BACKLOG_COMPONENT,
    REVIEW_COMPONENT,
    ICEBOX_COMPONENT,
    BACKLOG_DONE_COMPONENT,
];

/// Map a component name to its canonical singleton key, collapsing the legacy
/// backlog alias (`pending` → `backlog`). Returns `None` for names that may
/// legitimately repeat.
fn canonical_singleton_name(name: &str) -> Option<&'static str> {
    if is_backlog_component(name) {
        return Some(BACKLOG_COMPONENT);
    }
    SINGLETON_COMPONENT_NAMES
        .iter()
        .copied()
        .find(|&s| s == name)
}

/// True when `marker` contains an odd number of unescaped double-quotes — an
/// unterminated quoted attribute value (e.g. `<!-- agent:queue tasks=" -->`, a
/// CRDT merge-boundary split). Backslash escapes are honored to match
/// [`find_quoted_ranges`] semantics.
fn marker_has_unterminated_quote(marker: &str) -> bool {
    let mut open = false;
    let mut escaped = false;
    for ch in marker.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => open = !open,
            _ => {}
        }
    }
    open
}

fn byte_in_ranges(offset: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| offset >= start && offset < end)
}

/// A raw line from `split_inclusive('\n')` decomposed into the three pieces the
/// marker scanners need.
///
/// Both the malformed-comment detector and its repair walked lines with the same
/// four-line `trim_end_matches` / `trim_start_matches` / re-slice dance, which is
/// easy to get subtly out of step (the detector needs `indent.len()` as a byte
/// offset; the repair needs `indent` and `newline` verbatim to rebuild the line).
/// Reassembling `indent + content + newline` always reproduces the input exactly.
struct RawLine<'a> {
    /// Leading spaces/tabs.
    indent: &'a str,
    /// The line with indentation and line ending removed.
    content: &'a str,
    /// The trailing `\n`, `\r\n`, or `""` on a final unterminated line.
    newline: &'a str,
}

fn split_raw_line(raw_line: &str) -> RawLine<'_> {
    let without_newline = raw_line.trim_end_matches(['\n', '\r']);
    let newline = &raw_line[without_newline.len()..];
    let content = without_newline.trim_start_matches([' ', '\t']);
    let indent = &without_newline[..without_newline.len() - content.len()];
    RawLine {
        indent,
        content,
        newline,
    }
}

/// Repair a boundary marker whose same-line `-->` terminator was lost, splicing
/// the trailing remainder back onto its own line (`#boundarysplice`).
///
/// A CRDT cell merge can drop the terminator and weld the following line onto the
/// marker, producing `<!-- agent:boundary:72eb14f4> **Queue prompt:**`. That trips
/// [`malformed_agent_comment_reason`], and because the integrity gate refuses the
/// canonical *before* any repair runs, the document wedges permanently: every
/// closeout is retained, and the guidance to "retry only `session-check`" can
/// never converge because nothing rewrites the text.
///
/// This is safe to repair automatically precisely because a boundary marker is
/// transient binary-owned scaffolding, not operator prose — `reposition_boundary_*`
/// and `collapse_adjacent_boundary_markers` already rewrite it at will. The repair
/// is also lossless: the terminator is restored and every trailing byte is
/// preserved on the following line rather than discarded.
///
/// Deliberately scoped to `agent:boundary` only. Component openers carry
/// attributes and still fail closed; the attribute-free closing exchange marker
/// has its own stricter [`repair_malformed_exchange_close_comment`] gate.
pub fn repair_malformed_boundary_comment(doc: &str) -> Option<String> {
    const PREFIX: &str = "<!-- agent:boundary:";
    let code_ranges = find_code_ranges(doc);
    let quoted_ranges = find_quoted_ranges(doc);

    let mut out = String::with_capacity(doc.len() + 16);
    let mut repaired = false;
    let mut offset = 0usize;

    for raw_line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += raw_line.len();

        let RawLine {
            indent,
            content: trimmed,
            newline,
        } = split_raw_line(raw_line);

        let malformed = trimmed.starts_with(PREFIX)
            && !trimmed.contains("-->")
            && !byte_in_ranges(line_start + indent.len(), &code_ranges)
            && !byte_in_ranges(line_start + indent.len(), &quoted_ranges);

        if !malformed {
            out.push_str(raw_line);
            continue;
        }

        let rest = &trimmed[PREFIX.len()..];
        let id_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')))
            .unwrap_or(rest.len());
        let (id, remainder) = rest.split_at(id_len);
        if id.is_empty() {
            out.push_str(raw_line);
            continue;
        }

        repaired = true;
        out.push_str(indent);
        out.push_str(PREFIX);
        out.push_str(id);
        out.push_str(" -->");

        let remainder = remainder.strip_prefix(' ').unwrap_or(remainder);
        // Drop what is left of the eaten terminator. A merge that ate only part
        // of ` -->` leaves `--`, `->`, or `-` behind; that is scaffolding, and
        // "preserving" it would inject a stray `--` line into the document. Only
        // a `-`-leading remainder is debris — a `>`-leading one is a blockquote
        // from the welded line and is real content.
        let remainder = match remainder
            .strip_prefix("--")
            .or(remainder.strip_prefix("->"))
        {
            Some(rest) => rest.strip_prefix('>').unwrap_or(rest),
            None => remainder.strip_prefix('-').unwrap_or(remainder),
        };
        let remainder = remainder.strip_prefix(' ').unwrap_or(remainder);

        if remainder.is_empty() {
            out.push_str(newline);
        } else {
            out.push('\n');
            out.push_str(remainder);
            out.push_str(newline);
        }
    }

    repaired.then_some(out)
}

/// Repair the one unambiguous malformed component marker: the closing exchange
/// marker. Unlike an opener it has no attributes to infer, and it is
/// binary-owned scaffolding. The repair is allowed only when the document has
/// exactly one sound exchange opener, no sound close, and exactly one truncated
/// close outside code/quotes. Any welded remainder is preserved on its own line.
pub fn repair_malformed_exchange_close_comment(doc: &str) -> Option<String> {
    const OPEN_PREFIX: &str = "<!-- agent:exchange";
    const CLOSE_PREFIX: &str = "<!-- /agent:exchange";
    const CLOSE: &str = "<!-- /agent:exchange -->";

    let code_ranges = find_code_ranges(doc);
    let quoted_ranges = find_quoted_ranges(doc);
    let mut offset = 0usize;
    let mut open_count = 0usize;
    let mut sound_close_count = 0usize;
    let mut malformed_close_count = 0usize;

    for raw_line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += raw_line.len();
        let RawLine {
            indent,
            content: trimmed,
            ..
        } = split_raw_line(raw_line);
        let marker_offset = line_start + indent.len();
        if byte_in_ranges(marker_offset, &code_ranges)
            || byte_in_ranges(marker_offset, &quoted_ranges)
        {
            continue;
        }
        if trimmed.starts_with(OPEN_PREFIX) && trimmed.contains("-->") {
            open_count += 1;
        }
        if trimmed.trim() == CLOSE {
            sound_close_count += 1;
        } else if trimmed.starts_with(CLOSE_PREFIX) {
            malformed_close_count += 1;
        }
    }
    if open_count != 1 || sound_close_count != 0 || malformed_close_count != 1 {
        return None;
    }

    let mut out = String::with_capacity(doc.len() + 8);
    let mut repaired = false;
    offset = 0;
    for raw_line in doc.split_inclusive('\n') {
        let line_start = offset;
        offset += raw_line.len();
        let RawLine {
            indent,
            content: trimmed,
            newline,
        } = split_raw_line(raw_line);
        let marker_offset = line_start + indent.len();
        if repaired
            || byte_in_ranges(marker_offset, &code_ranges)
            || byte_in_ranges(marker_offset, &quoted_ranges)
            || !trimmed.starts_with(CLOSE_PREFIX)
            || trimmed.trim() == CLOSE
        {
            out.push_str(raw_line);
            continue;
        }

        let remainder = &trimmed[CLOSE_PREFIX.len()..];
        // Do not guess if the prefix is actually a longer component name.
        if remainder
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | ':'))
        {
            return None;
        }
        let remainder = remainder.strip_prefix(' ').unwrap_or(remainder);
        let remainder = match remainder
            .strip_prefix("--")
            .or(remainder.strip_prefix("->"))
        {
            Some(rest) => rest.strip_prefix('>').unwrap_or(rest),
            None => remainder.strip_prefix('-').unwrap_or(remainder),
        };
        let remainder = remainder.strip_prefix(' ').unwrap_or(remainder);

        repaired = true;
        out.push_str(indent);
        out.push_str(CLOSE);
        if remainder.is_empty() {
            out.push_str(newline);
        } else {
            out.push('\n');
            out.push_str(remainder);
            out.push_str(newline);
        }
    }
    repaired.then_some(out)
}

/// Detect agent-looking HTML comment markers that were truncated before their
/// same-line `-->` terminator. The normal parser only sees complete comments,
/// so a CRDT/editor split like `<!-- /agent:exchange --` can otherwise be
/// skipped or consume a later unrelated terminator.
pub fn malformed_agent_comment_reason(doc: &str) -> Option<String> {
    let code_ranges = find_code_ranges(doc);
    let quoted_ranges = find_quoted_ranges(doc);
    let mut offset = 0usize;
    for (line_idx, raw_line) in doc.split_inclusive('\n').enumerate() {
        let RawLine {
            indent,
            content: trimmed,
            ..
        } = split_raw_line(raw_line);
        let marker_start = offset + indent.len();
        offset += raw_line.len();

        if !trimmed.starts_with("<!--") {
            continue;
        }
        if byte_in_ranges(marker_start, &code_ranges)
            || byte_in_ranges(marker_start, &quoted_ranges)
        {
            continue;
        }
        let inner = trimmed["<!--".len()..].trim_start();
        if (inner.starts_with("agent:") || inner.starts_with("/agent:")) && !trimmed.contains("-->")
        {
            let preview = trimmed.chars().take(80).collect::<String>();
            return Some(format!(
                "malformed_agent_comment:line{}:{preview}",
                line_idx + 1
            ));
        }
    }
    standalone_orphan_comment_terminator_reason(doc, &code_ranges, &quoted_ranges)
}

fn standalone_orphan_comment_terminator_reason(
    doc: &str,
    code_ranges: &[(usize, usize)],
    quoted_ranges: &[(usize, usize)],
) -> Option<String> {
    let (line_start, _) =
        standalone_orphan_comment_terminator_ranges(doc, code_ranges, quoted_ranges)
            .into_iter()
            .next()?;
    let line = doc[..line_start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    Some(format!("orphan_html_comment_terminator:line{line}"))
}

fn standalone_orphan_comment_terminator_ranges(
    doc: &str,
    code_ranges: &[(usize, usize)],
    quoted_ranges: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut cursor = 0usize;
    let mut comment_open = false;
    let mut ranges = Vec::new();
    while cursor < doc.len() {
        let opener = doc[cursor..].find("<!--").map(|offset| cursor + offset);
        let closer = doc[cursor..].find("-->").map(|offset| cursor + offset);
        let (marker_start, is_opener) = match (opener, closer) {
            (Some(open), Some(close)) if open <= close => (open, true),
            (Some(_), Some(close)) => (close, false),
            (Some(open), None) => (open, true),
            (None, Some(close)) => (close, false),
            (None, None) => break,
        };
        let marker_len = if is_opener { "<!--".len() } else { "-->".len() };
        cursor = marker_start + marker_len;
        if byte_in_ranges(marker_start, code_ranges) || byte_in_ranges(marker_start, quoted_ranges)
        {
            continue;
        }
        if is_opener {
            comment_open = true;
            continue;
        }
        if comment_open {
            comment_open = false;
            continue;
        }

        let line_start = doc[..marker_start].rfind('\n').map_or(0, |pos| pos + 1);
        let line_end = doc[marker_start..]
            .find('\n')
            .map_or(doc.len(), |relative| marker_start + relative);
        if doc[line_start..line_end].trim() == "-->" {
            let remove_end = if line_end < doc.len() {
                line_end + 1
            } else {
                line_end
            };
            ranges.push((line_start, remove_end));
        }
    }
    ranges
}

/// Remove only standalone orphan `-->` lines outside fenced code, blockquotes,
/// and balanced HTML comments. This is the narrow recovery counterpart to the
/// structural gate: malformed editor authority can be classified as
/// repair-required, registered, and then normalized without accepting the
/// orphan terminator as a valid steady state.
pub fn repair_standalone_orphan_comment_terminators(doc: &str) -> Option<String> {
    let code_ranges = find_code_ranges(doc);
    let quoted_ranges = find_quoted_ranges(doc);
    let ranges = standalone_orphan_comment_terminator_ranges(doc, &code_ranges, &quoted_ranges);
    if ranges.is_empty() {
        return None;
    }

    let mut repaired = String::with_capacity(doc.len());
    let mut cursor = 0usize;
    for (start, end) in ranges {
        repaired.push_str(&doc[cursor..start]);
        cursor = end;
    }
    repaired.push_str(&doc[cursor..]);
    Some(repaired)
}

/// Remove the one recoverable component-close duplication produced when a
/// response replay races editor convergence.
///
/// The repair is intentionally narrow: the unmatched close must be the only
/// mismatch, the same component must already have closed successfully, and
/// removing that one complete marker line must restore structural integrity.
/// Marker-looking text in code or blockquotes is ignored.
pub fn repair_single_unmatched_duplicate_component_close(doc: &str) -> Option<String> {
    let ignored = find_code_ranges(doc)
        .into_iter()
        .chain(find_quoted_ranges(doc))
        .collect::<Vec<_>>();
    let mut stack: Vec<String> = Vec::new();
    let mut balanced_closes = HashMap::<String, usize>::new();
    let mut unmatched: Option<(usize, usize)> = None;
    let mut offset = 0usize;

    for line in doc.split_inclusive('\n') {
        let line_start = offset;
        let line_end = line_start + line.len();
        offset = line_end;
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let marker_start = line_start + leading;
        if ignored
            .iter()
            .any(|&(start, end)| marker_start >= start && marker_start < end)
        {
            continue;
        }

        let trimmed = line.trim();
        let Some(inner) = trimmed
            .strip_prefix("<!--")
            .and_then(|value| value.strip_suffix("-->"))
            .map(str::trim)
        else {
            continue;
        };
        if inner.starts_with("agent:boundary:") {
            continue;
        }
        if let Some(name) = inner.strip_prefix("/agent:") {
            if stack.last().is_some_and(|open| open == name) {
                stack.pop();
                *balanced_closes.entry(name.to_string()).or_default() += 1;
            } else if stack.is_empty()
                && balanced_closes.get(name).copied().unwrap_or_default() > 0
                && unmatched.is_none()
            {
                unmatched = Some((line_start, line_end));
            } else {
                return None;
            }
        } else if let Some(rest) = inner.strip_prefix("agent:") {
            let name = rest.split_whitespace().next().unwrap_or_default();
            if name.is_empty() {
                return None;
            }
            stack.push(name.to_string());
        }
    }
    if !stack.is_empty() {
        return None;
    }
    let (start, end) = unmatched?;
    let mut repaired = String::with_capacity(doc.len() - (end - start));
    repaired.push_str(&doc[..start]);
    repaired.push_str(&doc[end..]);
    structural_corruption_reason(&repaired)
        .is_none()
        .then_some(repaired)
}

/// Collapse one exact document suffix that a replica refresh appended after the
/// already-complete document.
///
/// The repair is deliberately narrower than generic deduplication: the retained
/// prefix must already be structurally sound, the appended bytes must be an
/// exact suffix of that prefix, and the duplicated suffix must carry agent-doc
/// structure. Those proofs distinguish binary-owned refresh scaffolding from
/// repeated operator prose.
pub fn repair_duplicated_document_suffix(doc: &str) -> Option<String> {
    structural_corruption_reason(doc)?;

    if let Some(prefix) = exact_duplicated_document_prefix(doc) {
        return Some(prefix);
    }

    // A torn editor replay can append an exact suffix and then progressively
    // publish only pieces of the final binary-owned comment terminator. Keep
    // this recovery deliberately narrow: the duplicated body still has to be
    // byte-for-byte exact, and the only additional bytes may be a short
    // `-->` fragment split by whitespace.
    for (debris_start, _) in doc.char_indices().rev() {
        let debris = &doc[debris_start..];
        if debris.len() > 16 {
            break;
        }
        if !is_comment_terminator_debris(debris) {
            continue;
        }
        if let Some(prefix) = exact_duplicated_document_prefix(&doc[..debris_start]) {
            return Some(prefix);
        }
    }
    None
}

fn exact_duplicated_document_prefix(doc: &str) -> Option<String> {
    for (split, _) in doc.char_indices().rev() {
        let prefix = &doc[..split];
        let suffix = &doc[split..];
        if prefix.len() < suffix.len()
            || (!suffix.contains("<!-- agent:") && !suffix.contains("<!-- /agent:"))
            || !prefix.ends_with(suffix)
            || structural_corruption_reason(prefix).is_some()
        {
            continue;
        }
        return Some(prefix.to_string());
    }
    None
}

fn is_comment_terminator_debris(value: &str) -> bool {
    value.starts_with('-')
        && value.contains('>')
        && value
            .chars()
            .all(|ch| matches!(ch, '-' | '>' | ' ' | '\t' | '\r' | '\n'))
}

/// Repair a closing `agent:queue` marker welded to the final queue-item line.
///
/// A historical progressive-editor merge could append both an obsolete typing
/// suffix and the binary-owned close marker to the complete queue prompt:
///
/// `... Directories should be created as needed.t should create a c<!-- /agent:queue -->`
///
/// The marker split is lossless. The suffix prune is deliberately narrower:
/// only the tail after the last sentence terminator is removed, and only when
/// that non-whitespace tail already occurs verbatim in the completed prefix.
/// This proves it is a repeated typing snapshot rather than novel operator text.
pub fn repair_welded_queue_close_marker(doc: &str) -> Option<String> {
    let components = parse(doc).ok()?;
    let mut repaired = doc.to_string();
    let mut changed = false;

    for component in components
        .iter()
        .rev()
        .filter(|component| component.name == "queue")
    {
        let marker_start = component.close_start;
        let marker_end = find_comment_end(repaired.as_bytes(), marker_start)?;
        let line_start = repaired[..marker_start]
            .rfind('\n')
            .map_or(0, |position| position + 1);
        let line_end = repaired[marker_end..]
            .find('\n')
            .map_or(repaired.len(), |relative| marker_end + relative);
        let prefix = &repaired[line_start..marker_start];
        let suffix = &repaired[marker_end..line_end];
        if prefix.trim().is_empty() {
            continue;
        }
        if !suffix.trim().is_empty() {
            return None;
        }

        let queue_line = prefix.trim_end();
        let mut completed_line = queue_line;
        if let Some((terminator_start, terminator)) =
            queue_line
                .char_indices()
                .rev()
                .find(|(position, character)| {
                    matches!(character, '.' | '!' | '?')
                        && *position + character.len_utf8() < queue_line.len()
                })
        {
            let completed_end = terminator_start + terminator.len_utf8();
            let repeated_tail = &queue_line[completed_end..];
            let completed_prefix = &queue_line[..completed_end];
            if repeated_tail.len() >= 12
                && repeated_tail.contains(char::is_whitespace)
                && !repeated_tail.starts_with(char::is_whitespace)
                && completed_prefix.contains(repeated_tail)
            {
                completed_line = completed_prefix;
            }
        }

        let marker = &repaired[marker_start..marker_end];
        let replacement = format!("{completed_line}\n{marker}");
        repaired.replace_range(line_start..marker_end, &replacement);
        changed = true;
    }

    (changed && structural_corruption_reason(&repaired).is_none()).then_some(repaired)
}

/// Split operator text that got appended AFTER a component marker back onto its
/// own line (`#markertrailingtypo`).
///
/// [`repair_welded_queue_close_marker`] handles text welded BEFORE a marker and
/// explicitly bails when there is also text after it. Nothing handled the
/// trailing-only case, so a single stray keystroke on a marker line wedged the
/// entire session: `structural_corruption_reason` returns
/// `non_standalone_component_marker:<name>:<kind>:line<n>`, every write refuses,
/// and — because the poisoned copy also lives in the durable intent — every
/// reconnect re-fails identically. Observed 2026-08-23 on
/// `tasks/software/tsift.md`: an operator meant to press their tmux prefix
/// (`Ctrl+B`) and hit it in the IDE instead, where it is *toggle bold*, leaving
/// `<!-- /agent:queue -->****`. Four characters, and the document could not be
/// written by any path.
///
/// The two sides are not symmetric in risk, which is why only this one repairs:
///
/// - Text BEFORE a marker means the operator's line and binary scaffolding have
///   been merged into one line, and which bytes belong to which is a guess.
///   Fail closed — that stays [`repair_welded_queue_close_marker`]'s narrow,
///   evidence-gated job.
/// - Text AFTER a well-formed, line-leading marker is unambiguous: it is already
///   on the far side of the marker, so moving it to the next line changes which
///   *line* it sits on and nothing else. The bytes are preserved verbatim; this
///   never deletes operator text.
///
/// Returns `None` when no marker carries a trailing artifact, so sound documents
/// are never rewritten.
pub fn repair_trailing_text_after_component_marker(doc: &str) -> Option<String> {
    let components = parse(doc).ok()?;
    // Collect every marker, then apply strictly back-to-front so each earlier
    // offset stays valid against the growing string.
    let mut markers: Vec<usize> = components
        .iter()
        .flat_map(|component| [component.open_start, component.close_start])
        .collect();
    markers.sort_unstable();

    let mut repaired = doc.to_string();
    let mut changed = false;
    for marker_start in markers.into_iter().rev() {
        let marker_end = find_comment_end(repaired.as_bytes(), marker_start)?;
        let line_start = repaired[..marker_start]
            .rfind('\n')
            .map_or(0, |position| position + 1);
        let line_end = repaired[marker_end..]
            .find('\n')
            .map_or(repaired.len(), |relative| marker_end + relative);
        // A marker that does not lead its own line is the ambiguous case above.
        // The final `structural_corruption_reason` gate would reject such a
        // half-repair anyway; this early-out states the intent and keeps the
        // function from rewriting a line it has no business touching.
        if !repaired[line_start..marker_start].trim().is_empty() {
            continue;
        }
        let trailing = repaired[marker_end..line_end].trim();
        if trailing.is_empty() {
            continue;
        }
        let replacement = format!("\n{trailing}");
        repaired.replace_range(marker_end..line_end, &replacement);
        changed = true;
    }

    (changed && structural_corruption_reason(&repaired).is_none()).then_some(repaired)
}

/// Detect structural corruption that must never be adopted as a snapshot base
/// (`#dupcontent`). Returns `Some(reason)` when the document is corrupt:
/// - a parse failure (mismatched / unclosed markers),
/// - more than one opening marker for a singleton component (duplicate block),
/// - an unterminated double-quoted attribute on a component opening marker.
///
/// Returns `None` when the document is structurally sound. Used to fail closed
/// before adopting a `content_ours` / visible-write IPC buffer whose
/// prior CRDT merge produced duplicate singleton blocks or a split attribute,
/// so the corrupt buffer never reaches disk.
pub fn structural_corruption_reason(doc: &str) -> Option<String> {
    if let Some(reason) = malformed_agent_comment_reason(doc) {
        return Some(reason);
    }

    let code_ranges = find_code_ranges(doc);
    let boundary_count = doc
        .match_indices("<!-- agent:boundary:")
        .filter(|(start, _)| {
            !code_ranges
                .iter()
                .any(|&(code_start, code_end)| *start >= code_start && *start < code_end)
        })
        .filter(|(start, _)| {
            let line_start = doc[..*start].rfind('\n').map_or(0, |pos| pos + 1);
            let line_end = doc[*start..]
                .find('\n')
                .map_or(doc.len(), |relative| *start + relative);
            let trimmed = doc[line_start..line_end].trim();
            trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
        })
        .count();
    if boundary_count > 1 {
        return Some(format!("duplicate_exchange_boundary:{boundary_count}"));
    }

    let components = match parse(doc) {
        Ok(c) => c,
        Err(e) => return Some(format!("parse_error: {e}")),
    };

    // Duplicate singleton component blocks.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for c in &components {
        if let Some(canonical) = canonical_singleton_name(&c.name) {
            *counts.entry(canonical).or_insert(0) += 1;
        }
    }
    let mut dupes: Vec<(&str, usize)> = counts
        .iter()
        .filter(|&(_, &n)| n > 1)
        .map(|(&k, &n)| (k, n))
        .collect();
    if !dupes.is_empty() {
        dupes.sort();
        let detail = dupes
            .iter()
            .map(|(k, n)| format!("{k}={n}"))
            .collect::<Vec<_>>()
            .join(",");
        return Some(format!("duplicate_singleton_component:{detail}"));
    }

    // Unterminated double-quoted attribute in a component opening marker.
    for c in &components {
        let marker = &doc[c.open_start..c.open_end];
        if marker_has_unterminated_quote(marker) {
            return Some(format!("malformed_attr:{}", c.name));
        }

        // Component markers are binary-owned scaffolding and must occupy their
        // own line. The parser intentionally accepts a balanced marker wherever
        // it occurs, but that made `operator text<!-- /agent:queue -->` look
        // structurally valid after a stale component merge welded the closing
        // marker onto a progressively typed queue line.
        for (kind, marker_start) in [("open", c.open_start), ("close", c.close_start)] {
            let marker_end = find_comment_end(doc.as_bytes(), marker_start)?;
            let line_start = doc[..marker_start].rfind('\n').map_or(0, |pos| pos + 1);
            let line_end = doc[marker_end..]
                .find('\n')
                .map_or(doc.len(), |relative| marker_end + relative);
            if !doc[line_start..marker_start].trim().is_empty()
                || !doc[marker_end..line_end].trim().is_empty()
            {
                let line = doc[..marker_start]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                return Some(format!(
                    "non_standalone_component_marker:{}:{kind}:line{line}",
                    c.name
                ));
            }
        }
    }

    None
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
mod tests {
    use super::*;

    #[test]
    fn single_range() {
        let doc = "before\n<!-- agent:status -->\nHello\n<!-- /agent:status -->\nafter\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "status");
        assert_eq!(ranges[0].content(doc), "Hello\n");
    }

    #[test]
    fn nested_ranges() {
        let doc = "\
<!-- agent:outer -->
<!-- agent:inner -->
content
<!-- /agent:inner -->
<!-- /agent:outer -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        // Sorted by open_start — outer first
        assert_eq!(ranges[0].name, "outer");
        assert_eq!(ranges[1].name, "inner");
        assert_eq!(ranges[1].content(doc), "content\n");
    }

    #[test]
    fn siblings() {
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b -->
beta
<!-- /agent:b -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].name, "a");
        assert_eq!(ranges[0].content(doc), "alpha\n");
        assert_eq!(ranges[1].name, "b");
        assert_eq!(ranges[1].content(doc), "beta\n");
    }

    #[test]
    fn no_ranges() {
        let doc = "# Just a document\n\nWith no range templates.\n";
        let ranges = parse(doc).unwrap();
        assert!(ranges.is_empty());
    }

    #[test]
    fn unmatched_open_error() {
        let doc = "<!-- agent:orphan -->\nContent\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("unclosed component"));
    }

    #[test]
    fn unmatched_close_error() {
        let doc = "Content\n<!-- /agent:orphan -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("without matching open"));
        assert_eq!(unmatched_component_close_name(&err), Some("orphan"));
    }

    #[test]
    fn mismatched_names_error() {
        let doc = "<!-- agent:foo -->\n<!-- /agent:bar -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("mismatched"));
    }

    #[test]
    fn invalid_name() {
        let doc = "<!-- agent:-bad -->\n<!-- /agent:-bad -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("invalid component name"));
    }

    #[test]
    fn name_validation() {
        assert!(is_valid_name("status"));
        assert!(is_valid_name("my-section"));
        assert!(is_valid_name("a1"));
        assert!(is_valid_name("A"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("-bad"));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has_underscore"));
    }

    #[test]
    fn content_extraction() {
        let doc = "<!-- agent:x -->\nfoo\nbar\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges[0].content(doc), "foo\nbar\n");
    }

    #[test]
    fn replace_roundtrip() {
        let doc = "before\n<!-- agent:s -->\nold\n<!-- /agent:s -->\nafter\n";
        let ranges = parse(doc).unwrap();
        let new_doc = ranges[0].replace_content(doc, "new\n");
        assert_eq!(
            new_doc,
            "before\n<!-- agent:s -->\nnew\n<!-- /agent:s -->\nafter\n"
        );
        // Re-parse should work
        let ranges2 = parse(&new_doc).unwrap();
        assert_eq!(ranges2.len(), 1);
        assert_eq!(ranges2[0].content(&new_doc), "new\n");
    }

    #[test]
    fn is_agent_marker_yes() {
        assert!(is_agent_marker(" agent:status "));
        assert!(is_agent_marker("/agent:status"));
        assert!(is_agent_marker("agent:my-thing"));
        assert!(is_agent_marker(" /agent:A1 "));
    }

    #[test]
    fn is_agent_marker_no() {
        assert!(!is_agent_marker("just a comment"));
        assert!(!is_agent_marker("agent:"));
        assert!(!is_agent_marker("/agent:"));
        assert!(!is_agent_marker("agent:-bad"));
        assert!(!is_agent_marker("some agent:fake stuff"));
    }

    #[test]
    fn regular_comments_ignored() {
        let doc = "<!-- just a comment -->\n<!-- agent:x -->\ndata\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn regular_comments_with_multibyte_preview_boundary_ignored() {
        let doc = "\
<!-- 123456789012345678❯ ordinary comment -->
<!-- agent:x -->
data
<!-- /agent:x -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn multiline_comment_ignored() {
        let doc = "\
<!--
multi
line
comment
-->
<!-- agent:s -->
content
<!-- /agent:s -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "s");
    }

    #[test]
    fn empty_content() {
        let doc = "<!-- agent:empty --><!-- /agent:empty -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].content(doc), "");
    }

    #[test]
    fn markers_in_fenced_code_block_ignored() {
        let doc = "\
<!-- agent:real -->
content
<!-- /agent:real -->
```markdown
<!-- agent:fake -->
this is just an example
<!-- /agent:fake -->
```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_inline_code_ignored() {
        let doc = "\
Use `<!-- agent:example -->` markers for components.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_tilde_fence_ignored() {
        let doc = "\
<!-- agent:x -->
data
<!-- /agent:x -->
~~~
<!-- agent:y -->
example
<!-- /agent:y -->
~~~
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn markers_in_indented_fenced_code_block_ignored() {
        // CommonMark allows up to 3 spaces before fence opener
        let doc = "\
<!-- agent:exchange -->
Content here.
<!-- /agent:exchange -->

  ```markdown
  <!-- agent:fake -->
  demo without closing tag
  ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn indented_fence_inside_component_ignored() {
        // Indented code block inside a component should not cause mismatched errors
        let doc = "\
<!-- agent:exchange -->
Here's how to set up:

   ```markdown
   <!-- agent:status -->
   Your status here
   ```

Done explaining.
<!-- /agent:exchange -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn deeply_indented_fence_ignored() {
        // Tabs and many spaces should still be detected as a fence
        let doc = "\
<!-- agent:x -->
ok
<!-- /agent:x -->
      ```
      <!-- agent:y -->
      inside fence
      ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn indented_fence_code_ranges_detected() {
        let doc = "before\n  ```\n  code\n  ```\nafter\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
    }

    #[test]
    fn code_ranges_detected() {
        let doc = "before\n```\ncode\n```\nafter `inline` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 2);
        // Fenced block
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
        // Inline span
        assert!(doc[ranges[1].0..ranges[1].1].contains("inline"));
    }

    #[test]
    fn code_ranges_double_backtick() {
        // CommonMark: `` `<!--` `` is a code span containing `<!--`
        let doc = "text `` `<!--` `` more\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert!(
            span.contains("<!--"),
            "double-backtick span should contain <!--: {:?}",
            span
        );
    }

    #[test]
    fn code_ranges_double_backtick_does_not_match_single() {
        // `` should not match a single ` close
        let doc = "text `` foo ` bar `` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert_eq!(span, "`` foo ` bar ``");
    }

    #[test]
    fn double_backtick_comment_before_agent_marker() {
        // Regression: `` `<!--` `` followed by agent marker should not confuse the parser
        let doc = "\
<!-- agent:exchange -->\n\
text `` `<!--` `` description\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(components[0].content(doc).contains("new content here"));
    }

    #[test]
    fn quoted_agent_marker_in_prose_is_not_parsed() {
        // #0kjc: a marker quoted by name in prose is content, not a real marker.
        // Without quote-masking the dangling `<!-- agent:queue -->` inside the
        // quotes would open an unmatched component and error the whole parse.
        let doc = "\
<!-- agent:exchange -->\n\
Operator note: \"<!-- agent:queue -->\" attributes were dropped on compaction.\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(components[0].content(doc).contains("new content here"));
        assert!(
            components[0]
                .content(doc)
                .contains("attributes were dropped")
        );
    }

    #[test]
    fn quoted_id_token_in_prose_is_content() {
        // #0kjc: a `#id`-looking token in a quoted span is prose, never a tag.
        let doc = "\
<!-- agent:exchange -->\n\
The operator asked to clear \"#advance-review\" but no such line exists.\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert!(components[0].content(doc).contains("#advance-review"));
    }

    #[test]
    fn fenced_agent_marker_in_prose_is_not_parsed() {
        // Regression guard: code-fence masking (pre-#0kjc) must keep working — a
        // marker inside a fenced block stays content.
        let doc = "\
<!-- agent:exchange -->\n\
```\n\
<!-- agent:queue -->\n\
```\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(components[0].content(doc).contains("new content here"));
    }

    #[test]
    fn real_marker_with_quoted_preset_attr_still_parses() {
        // #0kjc safety: a real marker carrying a double-quoted attribute value
        // must NOT be masked — its `<!--` precedes the quotes, so its start byte
        // is never inside a quoted span.
        let doc = "\
<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" priority go -->\n\
- :pushpin: do [#thing]\n\
<!-- /agent:queue -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "queue");
        // The marker is NOT masked (proving the safety property); the attr
        // parser stores the raw quoted value, same as before this change.
        assert!(
            components[0]
                .attrs
                .get("preset")
                .is_some_and(|v| v.contains("#spec-test-build-install-commit-push")),
            "preset attr should survive quote-masking, got {:?}",
            components[0].attrs.get("preset")
        );
    }

    #[test]
    fn find_quoted_ranges_is_same_line_and_fail_safe() {
        // Balanced same-line span is masked; a span never crosses a newline.
        let doc = "a \"bc\" d\n\"ef\n";
        let ranges = find_quoted_ranges(doc);
        // First line: `"bc"` (bytes 2..6). Second line: unterminated `"ef`
        // masks to end-of-line only (not into the next line).
        assert_eq!(ranges.len(), 2);
        assert_eq!(&doc[ranges[0].0..ranges[0].1], "\"bc\"");
        assert_eq!(&doc[ranges[1].0..ranges[1].1], "\"ef");
    }

    // --- Inline attribute tests ---

    #[test]
    fn parse_component_with_mode_attr() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(
            components[0].attrs.get("mode").map(|s| s.as_str()),
            Some("append")
        );
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn parse_component_with_multiple_attrs() {
        let doc = "<!-- agent:log mode=prepend timestamp=true -->\nData\n<!-- /agent:log -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "log");
        assert_eq!(
            components[0].attrs.get("mode").map(|s| s.as_str()),
            Some("prepend")
        );
        assert_eq!(
            components[0].attrs.get("timestamp").map(|s| s.as_str()),
            Some("true")
        );
    }

    #[test]
    fn parse_component_no_attrs_backward_compat() {
        let doc = "<!-- agent:status -->\nOK\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "status");
        assert!(components[0].attrs.is_empty());
    }

    #[test]
    fn is_agent_marker_with_attrs() {
        assert!(is_agent_marker(" agent:exchange mode=append "));
        assert!(is_agent_marker("agent:status mode=replace"));
        assert!(is_agent_marker("agent:log mode=prepend timestamp=true"));
    }

    #[test]
    fn bounded_preview_clamps_to_char_boundary_inside_multibyte() {
        // Regression (`#utf8p`): the parser peeks up to 20 bytes past a `<!--`
        // to decide whether it is an agent marker. When that window ended inside
        // a multibyte glyph such as `❯` (3 bytes), the old raw
        // `&doc[peek_start..peek_start + 20]` slice panicked with
        // "byte index N is not a char boundary", which aborted the cdylib and
        // crashed the host editor (SIGABRT on the `agent-doc-crdt-*` thread).
        // `bounded_preview` must clamp the upper bound back to a char boundary.
        let doc = format!("<!--{}❯ trailing", "x".repeat(18));
        // peek_start = 4; peek_start + 20 = 24 lands on the 3rd byte of `❯`
        // (which occupies bytes 22..25), so a raw slice would panic here.
        let preview = bounded_preview(&doc, 4, 20);
        assert!(doc.is_char_boundary(4 + preview.len()));
        assert_eq!(preview, "x".repeat(18)); // stops just before `❯`
    }

    #[test]
    fn parse_does_not_panic_on_prompt_glyph_adjacent_to_agent_marker() {
        // Mirrors the live reitrades.md shape: `❯` prompt lines sitting directly
        // against agent component / boundary markers. Parsing must stay panic-free
        // and still recover the real `exchange` component.
        let doc = "<!-- agent:exchange -->\n❯ tighten this up — simpler — lower rates\n<!-- agent:boundary:0d861cfa -->\n❯ His platform is in Rust & Vue.\n<!-- /agent:exchange -->\n";
        let components = parse(doc).expect("prompt glyphs near markers must not panic");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
    }

    #[test]
    fn parse_skips_boundary_close_artifact_as_non_component() {
        let doc = "<!-- agent:exchange -->\nanswer\n<!-- /agent:boundary:a37c9696 -->\n<!-- /agent:exchange -->\n";
        let components = parse(doc).expect("boundary close artifact must not be a component");
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(
            components[0]
                .content(doc)
                .contains("<!-- /agent:boundary:a37c9696 -->")
        );
    }

    #[test]
    fn boundary_close_artifact_is_not_an_ordinary_comment_range() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "answer\n",
            "<!-- /agent:boundary:a37c9696 -->\n",
            "<!-- /agent:exchange -->\n"
        );
        assert!(
            find_non_agent_html_comment_ranges(doc).is_empty(),
            "boundary close artifacts are transient agent comments, not ordinary comments"
        );
    }

    #[test]
    fn closing_tag_unchanged_with_attrs() {
        // Closing tags never have attributes
        let doc = "<!-- agent:status mode=replace -->\n- [x] Done\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        let new_doc = components[0].replace_content(doc, "- [ ] Todo\n");
        assert!(new_doc.contains("<!-- agent:status mode=replace -->"));
        assert!(new_doc.contains("<!-- /agent:status -->"));
        assert!(new_doc.contains("- [ ] Todo"));
    }

    #[test]
    fn parse_component_with_patch_attr() {
        let doc = "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(components[0].patch_mode(), Some("append"));
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn patch_attr_takes_precedence_over_mode() {
        let doc = "<!-- agent:exchange patch=replace mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("replace"));
    }

    #[test]
    fn mode_attr_backward_compat() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("append"));
    }

    #[test]
    fn no_patch_or_mode_attr() {
        let doc = "<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), None);
    }

    // --- Inline backtick code span exclusion tests ---

    #[test]
    fn single_backtick_component_tag_ignored() {
        // A component tag wrapped in single backticks should not be parsed
        let doc = "\
Use `<!-- agent:pending -->` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn double_backtick_component_tag_ignored() {
        // A component tag wrapped in double backticks should not be parsed
        let doc = "\
Use ``<!-- agent:pending -->`` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn component_tags_not_in_backticks_still_work() {
        // Tags outside of any backticks are parsed normally
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b patch=append -->
beta
<!-- /agent:b -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].name, "a");
        assert_eq!(components[1].name, "b");
        assert_eq!(components[1].patch_mode(), Some("append"));
    }

    #[test]
    fn mixed_backtick_and_real_tags() {
        // Some tags in backticks (ignored), some not (parsed)
        let doc = "\
Here is an example: `<!-- agent:fake -->` and ``<!-- /agent:fake -->``.
<!-- agent:real -->
real content
<!-- /agent:real -->
Another example: `<!-- agent:also-fake patch=replace -->` is just documentation.
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "real content\n");
    }

    #[test]
    fn inline_code_mid_line_with_surrounding_text_ignored() {
        // Edge case: component tag inside inline code span on a line with other content
        // before and after — must not be parsed as a real component marker.
        let doc = "\
Wrap markers like `<!-- agent:status -->` in backticks to show them literally.
<!-- agent:real -->
actual content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "actual content\n");
    }

    #[test]
    fn parse_attrs_unit() {
        let attrs = parse_attrs("mode=append");
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));

        let attrs = parse_attrs("mode=replace timestamp=true");
        assert_eq!(attrs.len(), 2);

        let attrs = parse_attrs("");
        assert!(attrs.is_empty());

        // Bare tokens (no =) are parsed as boolean flags with empty string values
        let attrs = parse_attrs("mode=append broken novalue=");
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));
        assert_eq!(attrs.get("broken").map(|s| s.as_str()), Some(""));

        // auto flag (used by agent:queue)
        let attrs = parse_attrs("auto");
        assert_eq!(attrs.len(), 1);
        assert!(attrs.contains_key("auto"));
    }

    #[test]
    fn parse_attrs_strips_surrounding_quotes() {
        // The regression this exists for: a compact wrote
        // `agent:done archive="tasks/software/lazily.done.md"`, the value kept its
        // quotes, and the `.done.md` suffix check could never match — so every
        // preflight and session-check on that document hard-failed with
        // "must point to a .done.md file" until the quotes were hand-stripped.
        let attrs = parse_attrs(r#"archive="tasks/software/lazily.done.md""#);
        assert_eq!(
            attrs.get("archive").map(|s| s.as_str()),
            Some("tasks/software/lazily.done.md"),
            "a double-quoted value must parse without its quotes"
        );

        // Single quotes too.
        let attrs = parse_attrs("archive='tasks/x.done.md'");
        assert_eq!(
            attrs.get("archive").map(|s| s.as_str()),
            Some("tasks/x.done.md")
        );

        // The unquoted form every other session document uses keeps working.
        let attrs = parse_attrs("archive=tasks/x.done.md");
        assert_eq!(
            attrs.get("archive").map(|s| s.as_str()),
            Some("tasks/x.done.md")
        );

        // Both forms must agree — that they did not is the whole defect.
        assert_eq!(
            parse_attrs(r#"archive="tasks/x.done.md""#).get("archive"),
            parse_attrs("archive=tasks/x.done.md").get("archive"),
            "quoted and unquoted spellings must parse to the same value"
        );
    }

    #[test]
    fn parse_attrs_quoted_value_may_contain_spaces() {
        // Quoting exists so a value can hold spaces; whitespace splitting alone
        // would have truncated this to `two`.
        let attrs = parse_attrs(r#"preset="two words" mode=append"#);
        assert_eq!(attrs.get("preset").map(|s| s.as_str()), Some("two words"));
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));
        assert_eq!(attrs.len(), 2);
    }

    #[test]
    fn parse_attrs_quotes_do_not_disturb_neighbours() {
        // A quoted value must not swallow the attributes after it. This is the
        // real marker from the document that broke.
        let attrs = parse_attrs(r##"preset="#spec-test-commit-push" priority go"##);
        assert_eq!(
            attrs.get("preset").map(|s| s.as_str()),
            Some("#spec-test-commit-push")
        );
        assert_eq!(attrs.get("priority").map(|s| s.as_str()), Some(""));
        assert_eq!(attrs.get("go").map(|s| s.as_str()), Some(""));
        assert_eq!(attrs.len(), 3);
    }

    #[test]
    fn parse_attrs_unterminated_quote_is_tolerated() {
        // Failing closed on a stray quote would make one typo unparse an entire
        // marker — the same class of outage this fix removes. Take the rest of the
        // input as the value instead.
        let attrs = parse_attrs(r#"archive="tasks/x.done.md"#);
        assert_eq!(
            attrs.get("archive").map(|s| s.as_str()),
            Some("tasks/x.done.md")
        );
    }

    #[test]
    fn append_with_boundary_skips_code_block() {
        // Boundary marker inside a code block should be ignored;
        // the real marker outside should be used.
        let boundary_id = "real-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             ```\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             ```\n\
             more user text\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result =
            comp.append_with_boundary(&doc, "### Re: Response\n\nContent here.", boundary_id);

        // Response should replace the REAL marker (outside code block),
        // not the one inside the code block.
        assert!(result.contains("### Re: Response"));
        assert!(result.contains("more user text"));
        // The code block example should be preserved
        assert!(result.contains(&format!("<!-- agent:boundary:{boundary_id} -->\n```")));
        // The real marker should be consumed (replaced by response)
        assert!(!result.contains(&format!(
            "more user text\n<!-- agent:boundary:{boundary_id} -->\n<!-- /agent:exchange -->"
        )));
    }

    #[test]
    fn append_with_boundary_no_code_block() {
        // Normal case: boundary marker not in a code block
        let boundary_id = "simple-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(&doc, "### Re: Answer\n\nDone.", boundary_id);

        assert!(result.contains("### Re: Answer"));
        assert!(result.contains("user prompt"));

        // `#boundarystableid`: the marker MOVES to the new end of the component,
        // it is not re-minted. The id is a stable position marker, not a
        // per-response event — re-minting made the live projection and the
        // committed blob disagree by exactly this line, permanently.
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "exactly one boundary must survive an append:\n{result}"
        );
        assert!(
            result.contains(&format!("<!-- agent:boundary:{boundary_id} -->")),
            "the boundary id must be carried forward, not re-minted:\n{result}"
        );

        // ...and it must be carried forward to the END, after the appended
        // response — not left at its original position above it.
        let boundary_at = result
            .find(&format!("<!-- agent:boundary:{boundary_id} -->"))
            .expect("boundary present");
        let response_at = result.find("### Re: Answer").expect("response present");
        assert!(
            response_at < boundary_at,
            "the boundary must move to the end of the component:\n{result}"
        );
    }

    /// `#boundarystableid`: repeated appends must keep ONE stable id. The
    /// re-minting this replaced is what produced 162 distinct boundary ids across
    /// one document's snapshots, guaranteeing live/committed divergence.
    #[test]
    fn repeated_appends_keep_one_stable_boundary_id() {
        let boundary_id = "stable01";
        let mut doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        for turn in 0..3 {
            let components = parse(&doc).unwrap();
            let comp = components
                .iter()
                .find(|c| c.name == "exchange")
                .expect("exchange component");
            doc = comp.append_with_boundary(
                &doc,
                &format!("### Re: Turn {turn}\n\nBody {turn}."),
                boundary_id,
            );
        }
        assert_eq!(
            doc.matches("<!-- agent:boundary:").count(),
            1,
            "boundaries must never accumulate across turns:\n{doc}"
        );
        assert!(
            doc.contains(&format!("<!-- agent:boundary:{boundary_id} -->")),
            "the id must be identical after three appends:\n{doc}"
        );
        for turn in 0..3 {
            assert!(
                doc.contains(&format!("### Re: Turn {turn}")),
                "turn {turn} must survive:\n{doc}"
            );
        }
    }

    #[test]
    fn append_with_boundary_skips_already_present_content() {
        let boundary_id = "simple-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             ### Re: Duplicate — gpt-5 (HEAD)\n\
             \n\
             Already applied.\n\
             <!-- agent:boundary:old-id -->\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(
            &doc,
            "### Re: Duplicate — gpt-5\n\nAlready applied.\n",
            boundary_id,
        );

        assert_eq!(result, doc);
    }

    #[test]
    fn append_with_caret_skips_already_present_content() {
        let doc = "<!-- agent:exchange patch=append -->\n\
                   User prompt.\n\
                   ### Re: Duplicate — gpt-5\n\
                   \n\
                   Already applied.\n\
                   <!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        let comp = &components[0];
        let result =
            comp.append_with_caret(doc, "### Re: Duplicate — gpt-5\n\nAlready applied.\n", None);

        assert_eq!(result, doc);
    }

    // --- strip_comments tests (moved from diff.rs) ---

    #[test]
    fn strip_comments_removes_html_comment() {
        let result = strip_comments("before\n<!-- a comment -->\nafter\n");
        assert_eq!(result, "before\nafter\n");
    }

    #[test]
    fn non_agent_html_comment_ranges_cover_multiline_body() {
        let doc = concat!(
            "before\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "-->\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let ranges = find_non_agent_html_comment_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let hidden = doc.find("do #hidden").unwrap();
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| hidden >= start && hidden < end),
            "ordinary comment body should be inside a non-agent comment range"
        );
        assert!(
            ranges
                .iter()
                .all(|&(start, end)| !doc[start..end].contains("agent:exchange")),
            "agent component markers must not be treated as ordinary comments"
        );
    }

    #[test]
    fn non_agent_html_comment_ranges_cover_unterminated_tail() {
        let doc = concat!(
            "before\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "still typing\n"
        );

        let ranges = find_non_agent_html_comment_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(
            &doc[start..end],
            "<!--\ndo #hidden. spec-test-build-install-commit-push\nstill typing\n"
        );
        assert_eq!(end, doc.len());
    }

    #[test]
    fn strip_comments_preserves_agent_markers() {
        let input = "text\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        let result = strip_comments(input);
        assert!(result.contains("<!-- agent:status -->"));
        assert!(result.contains("<!-- /agent:status -->"));
    }

    #[test]
    fn strip_comments_removes_link_ref() {
        let result = strip_comments("[//]: # (hidden note)\nvisible\n");
        assert_eq!(result, "visible\n");
    }

    #[test]
    fn html_comment_in_content_does_not_eat_close_marker() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#abc] Rule: first line (not starting with ### or ❯ or <!-- ) gets prefix
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        assert!(comps[0].content(doc).contains("<!-- )"));
    }

    #[test]
    fn nested_html_comment_like_text_in_exchange() {
        let doc = "\
<!-- agent:exchange -->
User typed <!-- some note --> in the text.
<!-- /agent:exchange -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "exchange");
    }

    #[test]
    fn pending_item_with_literal_html_comment_opener() {
        // Regression: a pending item containing `<!-- ` (literal HTML comment start)
        // must not consume the `<!-- /agent:pending -->` close marker.
        let doc = "\
<!-- agent:pending -->
- [ ] [#r7hw] Component parser: non-agent `<!-- ` in content ate close markers
- [ ] [#xyz] Another item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        let content = comps[0].content(doc);
        assert!(content.contains("#r7hw"));
        assert!(content.contains("#xyz"));
    }

    #[test]
    fn multiple_non_agent_html_comments_in_pending() {
        // Multiple `<!-- ... -->` fragments that are NOT agent markers.
        let doc = "\
<!-- agent:pending -->
- [ ] [#a] Fix rule: skip lines starting with <!-- or -->
- [ ] [#b] Handle <!-- partial comment
- [ ] [#c] Normal item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        let content = comps[0].content(doc);
        assert!(content.contains("#a"));
        assert!(content.contains("#b"));
        assert!(content.contains("#c"));
    }

    #[test]
    fn exchange_and_pending_both_with_html_comments_in_content() {
        // Both components contain non-agent HTML comments — parser must handle
        // siblings correctly without eating across component boundaries.
        let doc = "\
<!-- agent:exchange -->
The rule checks for <!-- prefix before deciding.
### Re: topic — opus
Fix applied to skip non-agent <!-- sequences.
<!-- /agent:exchange -->

<!-- agent:pending -->
- [ ] [#cfdy] Verify <!-- in pending items
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].name, "exchange");
        assert_eq!(comps[1].name, "pending");
        assert!(comps[0].content(doc).contains("<!-- prefix"));
        assert!(comps[1].content(doc).contains("#cfdy"));
    }

    #[test]
    fn is_backlog_component_accepts_both_names() {
        assert!(is_backlog_component("backlog"));
        assert!(is_backlog_component("pending"));
        assert!(!is_backlog_component("exchange"));
        assert!(!is_backlog_component("status"));
        assert!(!is_backlog_component("pending-done"));
        assert!(!is_backlog_component("backlog-done"));
        assert!(!is_backlog_component("done"));
    }

    #[test]
    fn is_backlog_done_component_accepts_only_canonical_done() {
        assert!(is_backlog_done_component("done"));
        assert!(!is_backlog_done_component("backlog-done"));
        assert!(!is_backlog_done_component("pending-done"));
        assert!(!is_backlog_done_component("backlog"));
        assert!(!is_backlog_done_component("pending"));
        assert!(!is_backlog_done_component("icebox"));
    }

    #[test]
    fn is_review_component_accepts_name() {
        assert!(is_review_component("review"));
        assert!(!is_review_component("backlog"));
        assert!(!is_review_component("icebox"));
    }

    #[test]
    fn is_icebox_component_accepts_name() {
        assert!(is_icebox_component("icebox"));
        assert!(!is_icebox_component("backlog"));
        assert!(!is_icebox_component("exchange"));
        assert!(!is_icebox_component("icebox-archive"));
    }

    #[test]
    fn tracked_work_component_includes_review() {
        assert!(is_tracked_work_component("backlog"));
        assert!(is_tracked_work_component("pending"));
        assert!(is_tracked_work_component("review"));
        assert!(is_tracked_work_component("icebox"));
        assert!(!is_tracked_work_component("done"));
    }

    #[test]
    fn icebox_component_parsed() {
        let doc = "\
<!-- agent:icebox -->
- Parked idea
<!-- /agent:icebox -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "icebox");
        assert!(is_icebox_component(&comps[0].name));
        assert!(comps[0].content(doc).contains("Parked idea"));
    }

    #[test]
    fn backlog_component_parsed_from_new_marker() {
        let doc = "\
<!-- agent:backlog -->
- [ ] [#abc] First item
<!-- /agent:backlog -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "backlog");
        assert!(is_backlog_component(&comps[0].name));
        assert!(comps[0].content(doc).contains("#abc"));
    }

    #[test]
    fn legacy_pending_marker_still_parsed() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#xyz] Legacy item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        assert!(is_backlog_component(&comps[0].name));
    }

    #[test]
    fn strip_backlog_patch_attr_removes_patch_replace() {
        let doc = "<!-- agent:backlog patch=replace -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_removes_pending_patch_replace() {
        let doc = "<!-- agent:pending patch=replace -->\n- item\n<!-- /agent:pending -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:pending -->\n- item\n<!-- /agent:pending -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_removes_mode_replace() {
        let doc = "<!-- agent:backlog mode=replace -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_preserves_other_attrs() {
        let doc =
            "<!-- agent:backlog patch=replace max_lines=50 -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog max_lines=50 -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_noop_for_exchange() {
        let doc = "<!-- agent:exchange patch=append -->\ncontent\n<!-- /agent:exchange -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(result, doc);
    }

    #[test]
    fn strip_backlog_patch_attr_noop_when_no_attr() {
        let doc = "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(result, doc);
    }

    #[test]
    fn converge_queue_auto_strips_auto() {
        let doc = "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, false).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_adds_auto() {
        let doc = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, true).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_preserves_other_attrs() {
        let doc = "<!-- agent:queue auto patch=append -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, false).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue patch=append -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_normalizes_boolean_attrs_without_corrupting_preset() {
        let doc = concat!(
            "<!-- agent:queue auto=true priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->\n",
            "- do [#x]\n",
            "<!-- /agent:queue -->\n",
        );
        let result = converge_queue_auto(doc, false).expect("tag normalized");
        assert_eq!(
            result,
            concat!(
                "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->\n",
                "- do [#x]\n",
                "<!-- /agent:queue -->\n",
            )
        );
    }

    #[test]
    fn converge_queue_auto_noop_when_already_matching() {
        let active = "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n";
        assert_eq!(converge_queue_auto(active, true), None);
        let inactive = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
        assert_eq!(converge_queue_auto(inactive, false), None);
    }

    #[test]
    fn converge_queue_auto_none_without_queue_component() {
        let doc = "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n";
        assert_eq!(converge_queue_auto(doc, false), None);
    }

    // #prompt-duplicated-while-typing: dedup must be agnostic to the `❯ ` user-prompt
    // prefix, since a synthesized exchange patch and the live buffer can differ only
    // by it. Otherwise the prompt re-appends and duplicates while typing.
    #[test]
    fn append_patch_already_present_ignores_user_prompt_prefix() {
        assert!(
            append_patch_already_present("expand the section", "❯ expand the section"),
            "bare buffer vs prefixed patch must dedup"
        );
        assert!(
            append_patch_already_present("❯ expand the section", "expand the section"),
            "prefixed buffer vs bare patch must dedup"
        );
        assert!(
            append_patch_already_present("❯ expand the section", "❯ expand the section"),
            "both prefixed must dedup"
        );
    }

    #[test]
    fn append_patch_distinct_prompts_not_deduped() {
        assert!(
            !append_patch_already_present("❯ expand the section", "❯ summarize the section"),
            "genuinely distinct prompts must not be treated as duplicates"
        );
    }

    // #prompt-duplicated-while-typing (L1 structural buffer-snapshot race),
    // captured live in tasks/resume.md 2026-05-29: the synthesized patch held a
    // half-typed snapshot ("- [#s93y]: Add to") while the live buffer had been
    // typed further ("- [#s93y]: Add to resume"). The two regions are identical
    // except that one line; the old `contains` check broke at the divergence and
    // re-appended the entire tail. Dedup must recognize the snapshot relationship.
    #[test]
    fn append_patch_dedupes_midblock_typing_snapshot() {
        let live = concat!(
            "- [#yfg1]: I dropped the real-time asset-generation. Kept the investor demos.\n",
            "- [#bndq]: confirmed\n",
            "- do [#s75b]\n",
            "- do [#vapk]\n",
            "- [#s93y]: Add to resume\n",
            "- [#ca8w]\n",
            "Add SampleOrders and sample portal to the resume if not already added.",
        );
        let snapshot = concat!(
            "- [#yfg1]: I dropped the real-time asset-generation. Kept the investor demos.\n",
            "- [#bndq]: confirmed\n",
            "- do [#s75b]\n",
            "- do [#vapk]\n",
            "- [#s93y]: Add to\n",
            "- [#ca8w]\n",
            "Add SampleOrders and sample portal to the resume if not already added.",
        );
        assert!(
            append_patch_already_present(live, snapshot),
            "earlier keystroke snapshot must dedup against the live, further-typed region"
        );
    }

    #[test]
    fn append_patch_typing_snapshot_does_not_collapse_distinct_lines() {
        // A genuinely distinct prompt block (not a prefix snapshot) must still
        // append — the divergent line is not a prefix of the live line.
        let live = "- do task one\n- and the second thing\n- finally";
        let distinct = "- do task one\n- and a different thing\n- finally";
        assert!(
            !append_patch_already_present(live, distinct),
            "a mid-line replacement (not a prefix) is a distinct edit, not a snapshot"
        );
    }

    #[test]
    fn append_with_boundary_does_not_duplicate_typing_snapshot() {
        // The boundary stays high in the exchange; the live region below it has
        // been typed past the synthesized patch's snapshot. Appending the stale
        // snapshot must be a full no-op (document unchanged), not a duplication.
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- agent:boundary:70bccf9b -->\n",
            "- [#yfg1]: ok\n",
            "- [#s93y]: Add to resume\n",
            "- [#ca8w]\n",
            "Add SampleOrders and sample portal to the resume if not already added.\n",
            "<!-- /agent:exchange -->\n",
        );
        let components = parse(doc).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let snapshot = concat!(
            "- [#yfg1]: ok\n",
            "- [#s93y]: Add to\n",
            "- [#ca8w]\n",
            "Add SampleOrders and sample portal to the resume if not already added.",
        );
        let result = exchange.append_with_boundary(doc, snapshot, "70bccf9b");
        assert_eq!(
            result, doc,
            "stale typing-snapshot append must be a no-op:\n{result}"
        );
        assert_eq!(
            result.matches("<!-- /agent:exchange -->").count(),
            1,
            "exchange close marker must not duplicate:\n{result}"
        );
    }

    #[test]
    fn append_with_caret_does_not_duplicate_prefixed_prompt() {
        // The live buffer already holds the bare typed prompt after the boundary; a
        // synthesized exchange patch carries the same prompt with the `❯ ` prefix.
        // Appending must be a no-op (single copy), not a duplicate.
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- agent:boundary:abc12345 -->\n",
            "expand the Confidential AI Startup section\n",
            "<!-- /agent:exchange -->\n",
        );
        let components = parse(doc).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let result =
            exchange.append_with_caret(doc, "❯ expand the Confidential AI Startup section", None);
        assert_eq!(
            result
                .matches("expand the Confidential AI Startup section")
                .count(),
            1,
            "prefixed synthesized patch must not duplicate the already-typed prompt:\n{result}"
        );
    }

    // --- #dupcontent: structural_corruption_reason ---

    const CLEAN_DOC: &str = "<!-- agent:status -->\nok\n<!-- /agent:status -->\n\
<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n\
<!-- agent:queue preset=\"#spec-test-commit-push\" priority go -->\n- do [#x]\n<!-- /agent:queue -->\n\
<!-- agent:backlog -->\n- [ ] [#y] thing\n<!-- /agent:backlog -->\n";

    #[test]
    fn structural_corruption_clean_doc_is_sound() {
        assert_eq!(structural_corruption_reason(CLEAN_DOC), None);
    }

    #[test]
    fn structural_corruption_flags_duplicate_singleton_queue() {
        // The live #dupcontent corruption: two agent:queue blocks ingested from
        // a bad content_ours CRDT merge.
        let doc = "<!-- agent:queue -->\n- a\n<!-- /agent:queue -->\n\
<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n\
<!-- agent:queue preset=\"#spec-test-commit-push\" priority go -->\n- b\n<!-- /agent:queue -->\n";
        let reason = structural_corruption_reason(doc).expect("two queue blocks must be flagged");
        assert!(
            reason.contains("duplicate_singleton_component") && reason.contains("queue=2"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn structural_corruption_flags_duplicate_exchange_boundary() {
        let doc = "<!-- agent:exchange -->\nq\n<!-- agent:boundary:first -->\n\
                   response\n<!-- agent:boundary:second -->\n<!-- /agent:exchange -->\n";
        assert_eq!(
            structural_corruption_reason(doc).as_deref(),
            Some("duplicate_exchange_boundary:2")
        );
    }

    #[test]
    fn structural_corruption_ignores_boundary_examples_inside_code_fences() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "```md\n",
            "<!-- agent:boundary:example -->\n",
            "```\n",
            "<!-- agent:boundary:real -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert_eq!(structural_corruption_reason(doc), None);
    }

    #[test]
    fn structural_corruption_flags_backlog_alias_duplicate() {
        // Legacy `pending` alias + canonical `backlog` collapse to one singleton.
        let doc = "<!-- agent:backlog -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n\
<!-- agent:pending -->\n- [ ] [#b] y\n<!-- /agent:pending -->\n";
        let reason = structural_corruption_reason(doc).expect("backlog+pending must be flagged");
        assert!(reason.contains("backlog=2"), "reason was: {reason}");
    }

    #[test]
    fn structural_corruption_flags_unterminated_quote_attr() {
        // The truncated merge-boundary marker `<!-- agent:queue tasks=" -->`.
        let doc = "<!-- agent:exchange -->\nq\n<!-- /agent:exchange -->\n\
<!-- agent:queue tasks=\" -->\n- a\n<!-- /agent:queue -->\n";
        let reason =
            structural_corruption_reason(doc).expect("unterminated quote attr must be flagged");
        assert!(
            reason.starts_with("malformed_attr:queue"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn structural_corruption_flags_component_markers_welded_to_text() {
        let welded_close = "<!-- agent:queue -->\n- do [#x] complete prompt<!-- /agent:queue -->\n";
        assert_eq!(
            structural_corruption_reason(welded_close).as_deref(),
            Some("non_standalone_component_marker:queue:close:line2")
        );

        let welded_open =
            "operator text<!-- agent:queue -->\n- do [#x] complete prompt\n<!-- /agent:queue -->\n";
        assert_eq!(
            structural_corruption_reason(welded_open).as_deref(),
            Some("non_standalone_component_marker:queue:open:line1")
        );
    }

    /// `#markertrailingtypo` — the 2026-08-23 `tsift.md` wedge.
    ///
    /// The operator meant to press their tmux prefix and hit `Ctrl+B` in the IDE,
    /// where it is toggle-bold. Four characters on the close-marker line made
    /// every write path refuse the document.
    #[test]
    fn a_stray_keystroke_after_a_close_marker_is_split_off_not_refused() {
        let typo = concat!(
            "<!-- agent:queue priority go -->\n",
            "- release + publish + install\n",
            "<!-- /agent:queue -->****\n",
        );
        assert_eq!(
            structural_corruption_reason(typo).as_deref(),
            Some("non_standalone_component_marker:queue:close:line3"),
            "precondition: this is the shape that wedged the session"
        );

        let repaired =
            repair_trailing_text_after_component_marker(typo).expect("trailing typo must repair");
        assert_eq!(
            repaired,
            concat!(
                "<!-- agent:queue priority go -->\n",
                "- release + publish + install\n",
                "<!-- /agent:queue -->\n",
                "****\n",
            )
        );
        assert_eq!(structural_corruption_reason(&repaired), None);
        // Lossless: the operator's bytes survive, they just move off the marker.
        assert!(repaired.contains("****"));
    }

    /// The open marker has the same failure and the same unambiguous fix — the
    /// trailing text is already inside the component, so it becomes a body line.
    #[test]
    fn a_stray_keystroke_after_an_open_marker_is_split_off_too() {
        let typo = "<!-- agent:queue -->**\n- do [#x] a prompt\n<!-- /agent:queue -->\n";
        assert!(structural_corruption_reason(typo).is_some());

        let repaired =
            repair_trailing_text_after_component_marker(typo).expect("trailing typo must repair");
        assert_eq!(
            repaired,
            "<!-- agent:queue -->\n**\n- do [#x] a prompt\n<!-- /agent:queue -->\n"
        );
        assert_eq!(structural_corruption_reason(&repaired), None);
    }

    /// Text BEFORE a marker stays fail-closed: which bytes are the operator's and
    /// which are scaffolding is a guess, and guessing there loses operator text.
    ///
    /// The second case is the one that matters — leading AND trailing text on one
    /// line, exactly what `repair_welded_queue_close_marker` declines. A careless
    /// implementation repairs the trailing half and leaves an ambiguous line
    /// half-rewritten; this must decline outright.
    #[test]
    fn text_welded_before_a_marker_is_not_touched_by_the_trailing_repair() {
        let welded = "<!-- agent:queue -->\n- do [#x] complete prompt<!-- /agent:queue -->\n";
        assert_eq!(repair_trailing_text_after_component_marker(welded), None);

        let welded_both_sides =
            "<!-- agent:queue -->\n- do [#x] complete prompt<!-- /agent:queue -->****\n";
        assert_eq!(
            repair_trailing_text_after_component_marker(welded_both_sides),
            None,
            "an ambiguous line must not be half-repaired into a different wedge"
        );
    }

    /// A sound document must never be rewritten — this repair runs on every
    /// durable-intent seam.
    #[test]
    fn a_sound_document_is_left_byte_identical() {
        let sound = "<!-- agent:queue -->\n- do [#x] a prompt\n<!-- /agent:queue -->\n\n**bold**\n";
        assert_eq!(repair_trailing_text_after_component_marker(sound), None);
    }

    #[test]
    fn welded_queue_close_repair_prunes_only_a_proven_progressive_suffix() {
        let corrupted = concat!(
            "<!-- agent:queue -->\n",
            "- do [#fresh-project-supervisor-log]: agent-doc start should create a configurable path to the supervisor log file. Directories should be created as needed.t should create a c",
            "<!-- /agent:queue -->\n",
        );
        let expected = concat!(
            "<!-- agent:queue -->\n",
            "- do [#fresh-project-supervisor-log]: agent-doc start should create a configurable path to the supervisor log file. Directories should be created as needed.\n",
            "<!-- /agent:queue -->\n",
        );

        assert_eq!(
            repair_welded_queue_close_marker(corrupted).as_deref(),
            Some(expected),
        );
        assert_eq!(structural_corruption_reason(expected), None);
    }

    #[test]
    fn welded_queue_close_repair_preserves_novel_queue_text() {
        let corrupted =
            "<!-- agent:queue -->\n- do [#x] novel operator text<!-- /agent:queue -->\n";
        let expected =
            "<!-- agent:queue -->\n- do [#x] novel operator text\n<!-- /agent:queue -->\n";
        assert_eq!(
            repair_welded_queue_close_marker(corrupted).as_deref(),
            Some(expected),
        );
    }

    #[test]
    fn structural_corruption_flags_truncated_agent_comment() {
        let doc = "<!-- agent:queue -->\n- a\n<!-- /agent:queue ->\n\
<!-- /agent:exchange --\n";
        let reason =
            structural_corruption_reason(doc).expect("truncated agent marker must be flagged");
        assert!(
            reason.starts_with("malformed_agent_comment:"),
            "reason was: {reason}"
        );
    }

    #[test]
    fn structural_corruption_flags_standalone_orphan_comment_terminator() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "> 📌 do [#dbj7]\n\n",
            "### Re: #dbj7 — gpt-5\n\n",
            "Response body.\n\n",
            "-->\n",
            "<!-- /agent:exchange -->\n",
        );
        assert_eq!(
            structural_corruption_reason(doc).as_deref(),
            Some("orphan_html_comment_terminator:line8")
        );
    }

    #[test]
    fn structural_corruption_allows_balanced_and_quoted_comment_terminators() {
        let doc = concat!(
            "<!--\nordinary multiline comment\n-->\n",
            "> -->\n",
            "```md\n-->\n```\n",
        );
        assert_eq!(structural_corruption_reason(doc), None);
    }

    #[test]
    fn orphan_comment_terminator_repair_is_narrow_and_clears_the_gate() {
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "> 📌 do [#dbj7]\n\n",
            "### Re: #dbj7 — gpt-5\n\n",
            "Response body.\n\n",
            " -->\n",
            "<!-- balanced\n-->\n",
            "> -->\n",
            "```md\n-->\n```\n",
            "<!-- /agent:exchange -->\n",
        );
        let repaired = repair_standalone_orphan_comment_terminators(doc)
            .expect("the standalone orphan must be repairable");

        assert!(!repaired.contains("\n -->\n"));
        assert!(repaired.contains("<!-- balanced\n-->\n"));
        assert!(repaired.contains("> -->\n"));
        assert!(repaired.contains("```md\n-->\n```\n"));
        assert_eq!(structural_corruption_reason(&repaired), None);
    }

    #[test]
    fn structural_corruption_ignores_truncated_agent_comment_inside_code_fence() {
        let doc = "```md\n<!-- /agent:queue ->\n<!-- /agent:exchange --\n```\n";
        assert_eq!(structural_corruption_reason(doc), None);
    }

    /// `#boundarysplice`: the exact live corruption — a CRDT cell merge dropped
    /// the boundary terminator and welded the next line onto the marker, wedging
    /// every closeout behind the integrity gate with no self-heal.
    #[test]
    fn repair_malformed_boundary_comment_splits_welded_remainder_losslessly() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic\n\nBody.\n<!-- agent:boundary:72eb14f4> **Queue prompt:**\n<!-- /agent:exchange -->\n";
        let repaired =
            repair_malformed_boundary_comment(doc).expect("welded boundary must be repaired");

        assert!(
            repaired.contains("<!-- agent:boundary:72eb14f4 -->\n> **Queue prompt:**\n"),
            "terminator restored and remainder preserved on its own line: {repaired}"
        );
        assert_eq!(
            structural_corruption_reason(&repaired),
            None,
            "repaired document must clear the integrity gate: {repaired}"
        );
        // Lossless: every non-marker character survives.
        assert!(repaired.contains("**Queue prompt:**"));
        assert!(repaired.contains("Body."));
        assert!(repaired.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn repair_malformed_boundary_comment_handles_bare_truncation_and_summary_ids() {
        let bare = "<!-- agent:boundary:abc123\n";
        let repaired = repair_malformed_boundary_comment(bare).expect("bare truncation repaired");
        assert_eq!(repaired, "<!-- agent:boundary:abc123 -->\n");

        let summary = "<!-- agent:boundary:deadbeef:before-compact> tail\n";
        let repaired =
            repair_malformed_boundary_comment(summary).expect("summary-suffix id repaired");
        assert_eq!(
            repaired,
            "<!-- agent:boundary:deadbeef:before-compact -->\n> tail\n"
        );
    }

    /// A partially-eaten terminator leaves debris (`--`, `->`, `-`) that is
    /// scaffolding, not content. Preserving it "losslessly" would inject a stray
    /// `--` line into the operator's document.
    #[test]
    fn repair_malformed_boundary_comment_drops_partial_terminator_debris() {
        for (input, want) in [
            (
                "<!-- agent:boundary:abc123 --\n",
                "<!-- agent:boundary:abc123 -->\n",
            ),
            (
                "<!-- agent:boundary:abc123 ->\n",
                "<!-- agent:boundary:abc123 -->\n",
            ),
            (
                "<!-- agent:boundary:abc123 -\n",
                "<!-- agent:boundary:abc123 -->\n",
            ),
        ] {
            assert_eq!(
                repair_malformed_boundary_comment(input).as_deref(),
                Some(want),
                "terminator debris must not survive as document content: {input:?}"
            );
        }

        // Real content after the debris still survives, on its own line.
        assert_eq!(
            repair_malformed_boundary_comment("<!-- agent:boundary:abc123 -- trailing text\n")
                .as_deref(),
            Some("<!-- agent:boundary:abc123 -->\ntrailing text\n")
        );
        // A blockquote remainder starts with `>` and is content, never debris.
        assert_eq!(
            repair_malformed_boundary_comment("<!-- agent:boundary:abc123> **Queue prompt:**\n")
                .as_deref(),
            Some("<!-- agent:boundary:abc123 -->\n> **Queue prompt:**\n")
        );
    }

    #[test]
    fn repair_malformed_boundary_comment_leaves_sound_and_out_of_scope_documents_alone() {
        // Already well-formed.
        assert_eq!(
            repair_malformed_boundary_comment("<!-- agent:boundary:abc123 -->\n"),
            None
        );
        // Component markers carry attributes and structure — still fail closed.
        assert_eq!(
            repair_malformed_boundary_comment("<!-- agent:exchange patch=append\n"),
            None
        );
        assert_eq!(
            repair_malformed_boundary_comment("<!-- /agent:exchange --\n"),
            None
        );
        // Inside a fence it is documentation, not scaffolding.
        assert_eq!(
            repair_malformed_boundary_comment("```md\n<!-- agent:boundary:abc123> x\n```\n"),
            None
        );
    }

    #[test]
    fn repair_malformed_exchange_close_preserves_concurrent_queue_edits() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prompt.\n",
            "### Re: Prompt. — gpt-5\n\nAnswered.\n",
            "<!-- /agent:exchange -- <!-- agent:queue -->\n",
            "- do [#added-during-closeout]\n",
            "<!-- /agent:queue -->\n",
        );
        let repaired = repair_malformed_exchange_close_comment(doc)
            .expect("the attribute-free closing marker is unambiguous");

        assert!(repaired.contains("<!-- /agent:exchange -->\n<!-- agent:queue -->\n"));
        assert!(repaired.contains("- do [#added-during-closeout]\n"));
        assert_eq!(structural_corruption_reason(&repaired), None);
    }

    #[test]
    fn repair_malformed_exchange_close_fails_closed_when_ambiguous() {
        assert_eq!(
            repair_malformed_exchange_close_comment("<!-- /agent:exchange --\n"),
            None,
            "a close without its matching opener has no repair authority"
        );
        assert_eq!(
            repair_malformed_exchange_close_comment(concat!(
                "<!-- agent:exchange -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- /agent:exchange --\n",
            )),
            None,
            "a sound close plus malformed close is duplicate structure, not truncation"
        );
        assert_eq!(
            repair_malformed_exchange_close_comment(concat!(
                "<!-- agent:exchange -->\n",
                "```md\n<!-- /agent:exchange --\n```\n",
            )),
            None,
            "documentation inside a code fence is not binary scaffolding"
        );
    }

    #[test]
    fn repair_duplicated_document_suffix_collapses_partial_component_tail() {
        let sound = concat!(
            "---\nagent_doc_session: suffix-repair\n---\n\n",
            "<!-- agent:review -->\n",
            "- [/] Long #agent-doc-feature diagnosis, even though the live editor owns the cut.\n",
            "- [/] Keep this operator-authored row.\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] Future item.\n",
            "<!-- /agent:icebox -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- completed work archived -->\n",
            "<!-- /agent:done -->",
        );
        let duplicated_tail = sound
            .find("oc-feature")
            .map(|start| &sound[start..])
            .expect("fixture contains the mid-token suffix");
        assert!(!sound.ends_with('\n'));
        assert!(duplicated_tail.starts_with("oc-feature"));
        let corrupted = format!("{sound}{duplicated_tail}");

        assert!(structural_corruption_reason(&corrupted).is_some());
        assert_eq!(
            repair_duplicated_document_suffix(&corrupted).as_deref(),
            Some(sound)
        );
    }

    #[test]
    fn repair_duplicated_document_suffix_requires_exact_structural_evidence() {
        let sound = concat!(
            "<!-- agent:review -->\n",
            "- [/] Repeated operator prose.\n",
            "<!-- /agent:review -->\n",
        );
        assert_eq!(
            repair_duplicated_document_suffix(&format!("{sound}Repeated operator prose.\n")),
            None,
            "plain repeated prose is never binary-owned repair authority"
        );
        assert_eq!(
            repair_duplicated_document_suffix(&format!(
                "{sound}<!-- /agent:review -->\nchanged tail\n"
            )),
            None,
            "a non-exact structural tail must fail closed"
        );
        assert_eq!(
            repair_duplicated_document_suffix(&format!(
                "{sound}peated changed operator prose.\n<!-- /agent:review -->\n"
            )),
            None,
            "a non-exact mid-token structural tail must fail closed"
        );
    }

    #[test]
    fn repair_duplicated_document_suffix_drops_only_trailing_terminator_debris() {
        let sound = concat!(
            "<!-- agent:review -->\n",
            "- [/] Keep the operator-authored diagnosis audited.\n",
            "<!-- /agent:review -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] Future item.\n",
            "<!-- /agent:icebox -->\n\n",
            "<!-- agent:done -->\n",
            "<!-- completed work archived -->\n",
            "<!-- /agent:done -->",
        );
        let duplicated_tail = sound
            .find(" audited.")
            .map(|start| &sound[start..])
            .expect("fixture contains the mid-line duplicated suffix");

        assert_eq!(
            repair_duplicated_document_suffix(&format!("{sound}{duplicated_tail}--\n>")).as_deref(),
            Some(sound),
        );
        assert_eq!(
            repair_duplicated_document_suffix(&format!(
                "{sound}{duplicated_tail}--\n>operator text\n"
            )),
            None,
            "novel trailing operator text must remain outside repair authority"
        );
    }

    #[test]
    fn structural_corruption_ignores_truncated_agent_comment_inside_quotes() {
        let doc = "\"<!-- /agent:queue ->\"\n";
        assert_eq!(structural_corruption_reason(doc), None);
    }

    #[test]
    fn structural_corruption_allows_balanced_quoted_attr() {
        // A real queue marker carrying a balanced quoted attribute is sound.
        let doc = "<!-- agent:queue preset=\"#a\" go -->\n- a\n<!-- /agent:queue -->\n";
        assert_eq!(structural_corruption_reason(doc), None);
    }

    #[test]
    fn structural_corruption_allows_repeated_non_singleton() {
        // `log` is not a singleton; repeats must NOT be flagged.
        let doc = "<!-- agent:log -->\na\n<!-- /agent:log -->\n\
<!-- agent:log -->\nb\n<!-- /agent:log -->\n";
        assert_eq!(structural_corruption_reason(doc), None);
    }

    #[test]
    fn structural_corruption_flags_parse_failure() {
        // Unclosed marker → parse error → corrupt.
        let doc = "<!-- agent:queue -->\n- a\n";
        let reason = structural_corruption_reason(doc).expect("unclosed marker must be flagged");
        assert!(reason.starts_with("parse_error"), "reason was: {reason}");
    }
}
