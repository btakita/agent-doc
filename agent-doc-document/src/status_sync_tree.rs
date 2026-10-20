//! Structured model of the binary-managed `status:sync` region inside an
//! `agent:status` component body.
//!
//! The region is delimited by the neutral markers [`STATUS_SYNC_OPEN_MARKER`] and
//! [`STATUS_SYNC_CLOSE_MARKER`]. Markers are recognized from the markdown block AST
//! (`agent_doc_markdown_ast`, tree-sitter-md): only an `html_block` in top-level
//! flow whose whole text is exactly one marker counts. A marker inside a fenced
//! code block, block quote, or list item, or sharing its line with other text, is
//! ordinary prose and never opens or closes the region.
//!
//! The markers are then folded into a tree ([`StatusBodyTree`]) of byte-exact
//! prose spans and sync regions, so callers reason about *which region* holds
//! which bytes instead of splicing strings around marker text:
//!
//! - [`managed_status_sync_block`] validates the well-formed shape (at most one
//!   balanced, non-nested region) and returns its spans.
//! - [`normalize_status_sync_body`] self-heals a malformed shape (nested,
//!   duplicated, unclosed, or orphaned markers) into exactly one region.
//! - [`parse_status_write`] interprets incoming status-write text. Text that
//!   carries its own region is a full status body: its region payload becomes the
//!   sync payload and its outside prose the free-form status, so a write can never
//!   nest the caller's markers inside the existing region.

use std::collections::HashSet;
use std::ops::Range;

use agent_doc_markdown_ast::{Block, BlockKind};
use anyhow::{Context, Result, bail};

/// Neutral delimiters for the single agent-managed status-sync projection.
///
/// These deliberately are not `agent:*` markers: the sync projection is nested
/// inside `agent:status`, but it is not an independently addressable component.
pub const STATUS_SYNC_OPEN_MARKER: &str = "<!-- status:sync -->";
pub const STATUS_SYNC_CLOSE_MARKER: &str = "<!-- /status:sync -->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    Open,
    Close,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Marker {
    kind: MarkerKind,
    /// Byte span of the whole marker line, including its trailing newline.
    span: Range<usize>,
}

/// Collect sync markers from the markdown block AST of `body`, in source order.
fn sync_markers(body: &str) -> Result<Vec<Marker>> {
    let root = agent_doc_markdown_ast::parse(body)
        .context("failed to parse agent:status body as markdown")?;
    let mut markers = Vec::new();
    collect_flow_markers(&root, body, &mut markers)?;
    markers.sort_by_key(|marker| marker.span.start);
    Ok(markers)
}

fn is_marker_line(line: &str) -> bool {
    matches!(
        line.trim(),
        STATUS_SYNC_OPEN_MARKER | STATUS_SYNC_CLOSE_MARKER
    )
}

/// A fenced code block is terminated when it has both delimiters; an
/// unterminated fence runs to the end of the body and swallows what follows.
fn is_terminated_fence(block: &Block) -> bool {
    block
        .children
        .iter()
        .filter(|child| {
            matches!(&child.kind, BlockKind::Other(kind) if kind == "fenced_code_block_delimiter")
        })
        .count()
        >= 2
}

/// Walk only top-level flow (`document` and heading `section`s). Markers nested
/// in quotes, lists, or terminated code fences are content, not structure.
///
/// A delimiter line that markdown folded into some other block (an
/// unterminated code fence, an HTML block such as `<div>` or `<pre>` that runs
/// on, an indented code block) is ambiguous: a text reader sees a delimiter the
/// block tree does not, so a write keyed on the tree could append a block the
/// tree cannot see. That shape fails closed instead of being guessed at.
fn collect_flow_markers(block: &Block, body: &str, out: &mut Vec<Marker>) -> Result<()> {
    for child in &block.children {
        let text = child.text(body);
        let kind = match child.kind {
            BlockKind::Document | BlockKind::Section => {
                collect_flow_markers(child, body, out)?;
                continue;
            }
            BlockKind::HtmlBlock => match text.trim() {
                STATUS_SYNC_OPEN_MARKER => MarkerKind::Open,
                STATUS_SYNC_CLOSE_MARKER => MarkerKind::Close,
                _ if text.lines().any(is_marker_line) => bail!(
                    "agent:status hides a status:sync delimiter inside an HTML block; \
                     put the delimiter on its own line after a blank line"
                ),
                _ => continue,
            },
            BlockKind::FencedCode if is_terminated_fence(child) => continue,
            _ if text.lines().any(is_marker_line) => bail!(
                "agent:status hides a status:sync delimiter inside a {:?} block \
                 (unterminated code fence or indented code?); close the fence or move the \
                 delimiter to its own unindented line",
                child.kind
            ),
            _ => continue,
        };
        let line_start = body[..child.start_byte]
            .rfind('\n')
            .map_or(0, |idx| idx + 1);
        let mut end = child.end_byte;
        if !body[..end].ends_with('\n') && body[end..].starts_with('\n') {
            end += 1;
        }
        out.push(Marker {
            kind,
            span: line_start..end,
        });
    }
    Ok(())
}

/// Byte spans of the single well-formed managed region within a status body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedStatusSyncBlock {
    /// Start of the open-marker line.
    pub block_start: usize,
    /// First payload byte (just after the open-marker line).
    pub content_start: usize,
    /// End of the payload (start of the close-marker line).
    pub content_end: usize,
    /// End of the close-marker line, including its newline.
    pub block_end: usize,
}

/// Validate that `status_body` holds at most one balanced, non-nested sync region
/// and return its spans. Fails closed on any malformed shape; use
/// [`normalize_status_sync_body`] to repair one.
pub fn managed_status_sync_block(status_body: &str) -> Result<Option<ManagedStatusSyncBlock>> {
    let mut open: Option<Range<usize>> = None;
    let mut found = None;
    for marker in sync_markers(status_body)? {
        match marker.kind {
            MarkerKind::Open => {
                if open.is_some() || found.is_some() {
                    bail!("agent:status contains duplicate or nested status:sync blocks");
                }
                open = Some(marker.span);
            }
            MarkerKind::Close => {
                let Some(open_span) = open.take() else {
                    bail!("agent:status contains a status:sync close marker without an opener");
                };
                found = Some(ManagedStatusSyncBlock {
                    block_start: open_span.start,
                    content_start: open_span.end,
                    content_end: marker.span.start,
                    block_end: marker.span.end,
                });
            }
        }
    }
    if open.is_some() {
        bail!("agent:status contains an unclosed status:sync block");
    }
    Ok(found)
}

/// One node of a status body: byte-exact prose, a sync region, or a close
/// marker with no opener.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StatusNode {
    Prose(Range<usize>),
    Sync(SyncRegion),
    StrayClose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SyncRegion {
    children: Vec<StatusNode>,
}

impl SyncRegion {
    fn has_nested_region(&self) -> bool {
        self.children
            .iter()
            .any(|child| matches!(child, StatusNode::Sync(_)))
    }
}

/// A status body folded into prose spans and (possibly nested) sync regions.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusBodyTree<'a> {
    source: &'a str,
    nodes: Vec<StatusNode>,
}

impl<'a> StatusBodyTree<'a> {
    fn parse(source: &'a str) -> Result<Self> {
        let markers = sync_markers(source)?;
        // Stack of open regions; index 0 is the top-level node list.
        let mut stack: Vec<Vec<StatusNode>> = vec![Vec::new()];
        let mut cursor = 0usize;
        for marker in markers {
            if marker.span.start > cursor {
                stack
                    .last_mut()
                    .expect("stack is never empty")
                    .push(StatusNode::Prose(cursor..marker.span.start));
            }
            match marker.kind {
                MarkerKind::Open => stack.push(Vec::new()),
                MarkerKind::Close => {
                    if stack.len() > 1 {
                        let children = stack.pop().expect("checked depth");
                        stack
                            .last_mut()
                            .expect("stack is never empty")
                            .push(StatusNode::Sync(SyncRegion { children }));
                    } else {
                        stack[0].push(StatusNode::StrayClose);
                    }
                }
            }
            cursor = marker.span.end;
        }
        if cursor < source.len() {
            stack
                .last_mut()
                .expect("stack is never empty")
                .push(StatusNode::Prose(cursor..source.len()));
        }
        // An unclosed region extends to the end of the body.
        while stack.len() > 1 {
            let children = stack.pop().expect("checked depth");
            stack
                .last_mut()
                .expect("stack is never empty")
                .push(StatusNode::Sync(SyncRegion { children }));
        }
        Ok(Self {
            source,
            nodes: stack.pop().expect("top-level list"),
        })
    }

    fn has_markers(&self) -> bool {
        self.nodes
            .iter()
            .any(|node| !matches!(node, StatusNode::Prose(_)))
    }

    fn text(&self, span: &Range<usize>) -> &'a str {
        &self.source[span.clone()]
    }

    /// Path (child indices from the top level) to the payload region: the latest
    /// innermost region in document order, i.e. the last region with no nested
    /// region of its own.
    fn chosen_region_path(&self) -> Option<Vec<usize>> {
        fn walk(nodes: &[StatusNode], path: &mut Vec<usize>, best: &mut Option<Vec<usize>>) {
            for (idx, node) in nodes.iter().enumerate() {
                if let StatusNode::Sync(region) = node {
                    path.push(idx);
                    if region.has_nested_region() {
                        walk(&region.children, path, best);
                    } else {
                        *best = Some(path.clone());
                    }
                    path.pop();
                }
            }
        }
        let mut best = None;
        walk(&self.nodes, &mut Vec::new(), &mut best);
        best
    }

    fn region_at(&self, path: &[usize]) -> Option<&SyncRegion> {
        let mut nodes = &self.nodes;
        let mut region = None;
        for &idx in path {
            let StatusNode::Sync(next) = nodes.get(idx)? else {
                return None;
            };
            nodes = &next.children;
            region = Some(next);
        }
        region
    }

    fn prose_children(&self, region: &SyncRegion) -> String {
        region
            .children
            .iter()
            .filter_map(|child| match child {
                StatusNode::Prose(span) => Some(self.text(span)),
                _ => None,
            })
            .collect()
    }

    /// Fold the tree into exactly one region.
    ///
    /// - Top-level prose is kept byte-for-byte.
    /// - The payload comes from [`Self::chosen_region_path`].
    /// - Prose that sits inside an ancestor of the chosen region (between a
    ///   wrapping opener and the real region) is free-form status that leaked
    ///   into the wrapper; it is moved out before the region, dropping every
    ///   block whose lines already appear in the top-level prose.
    /// - Every other region is a superseded payload and is dropped, as are
    ///   orphaned close markers.
    fn normalized(&self) -> NormalizedStatusBody {
        let chosen = self.chosen_region_path();
        let payload = chosen
            .as_ref()
            .and_then(|path| self.region_at(path))
            .map(|region| self.prose_children(region));

        let mut leaked = String::new();
        if let Some(path) = &chosen {
            for depth in 1..path.len() {
                if let Some(ancestor) = self.region_at(&path[..depth]) {
                    leaked.push_str(&self.prose_children(ancestor));
                    leaked.push('\n');
                }
            }
        }

        let top_prose_lines: HashSet<&str> = self
            .nodes
            .iter()
            .filter_map(|node| match node {
                StatusNode::Prose(span) => Some(self.text(span)),
                _ => None,
            })
            .flat_map(str::lines)
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        let leaked = retain_new_blocks(&leaked, &top_prose_lines);

        let mut before = String::new();
        let mut after = String::new();
        let mut region_emitted = false;
        let mut dropped_gap = false;
        for (idx, node) in self.nodes.iter().enumerate() {
            match node {
                StatusNode::Prose(span) => {
                    let mut text = self.text(span);
                    let target = if region_emitted {
                        &mut after
                    } else {
                        &mut before
                    };
                    if dropped_gap && (target.is_empty() || target.ends_with("\n\n")) {
                        text = text.trim_start_matches('\n');
                    }
                    target.push_str(text);
                    dropped_gap = false;
                }
                StatusNode::Sync(_) if chosen.as_ref().and_then(|p| p.first()) == Some(&idx) => {
                    region_emitted = true;
                }
                StatusNode::Sync(_) | StatusNode::StrayClose => dropped_gap = true,
            }
        }
        if !region_emitted {
            // No region survives (only orphaned closers): prose only.
            before.push_str(&after);
            after.clear();
        }
        if !leaked.is_empty() {
            ensure_block_break(&mut before);
            before.push_str(&leaked);
            before.push('\n');
        }
        NormalizedStatusBody {
            before,
            payload: region_emitted.then(|| payload.unwrap_or_default()),
            after,
        }
    }
}

/// A status body with at most one region, split around it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedStatusBody {
    before: String,
    payload: Option<String>,
    after: String,
}

impl NormalizedStatusBody {
    fn render(&self) -> String {
        let mut out = self.before.clone();
        if let Some(payload) = &self.payload {
            ensure_block_break(&mut out);
            out.push_str(&render_status_sync_block(payload));
        }
        out.push_str(&self.after);
        out
    }

    fn outside_prose(&self) -> String {
        format!("{}{}", self.before, self.after)
    }
}

/// Keep the markdown blocks of `text` (AST top-level blocks) that contribute at
/// least one line not already present in `known_lines`, joined by blank lines.
fn retain_new_blocks(text: &str, known_lines: &HashSet<&str>) -> String {
    let Some(root) = agent_doc_markdown_ast::parse(text) else {
        return text.trim().to_string();
    };
    let mut blocks = Vec::new();
    collect_flow_blocks(&root, &mut blocks);
    blocks
        .into_iter()
        .map(|block| block.text(text).trim())
        .filter(|block| {
            block
                .lines()
                .map(str::trim)
                .any(|line| !line.is_empty() && !known_lines.contains(line))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn collect_flow_blocks<'b>(block: &'b Block, out: &mut Vec<&'b Block>) {
    for child in &block.children {
        match child.kind {
            BlockKind::Document | BlockKind::Section => collect_flow_blocks(child, out),
            _ => out.push(child),
        }
    }
}

/// Ensure the next block starts after a blank line, unless nothing but
/// whitespace precedes it (the body's leading framing is kept as-is).
fn ensure_block_break(out: &mut String) {
    if out.trim().is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

/// The canonical payload form: trimmed, newline-terminated, empty when blank.
pub fn normalized_status_sync_payload(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    }
}

/// Render one managed region around `payload`.
pub fn render_status_sync_block(payload: &str) -> String {
    format!(
        "{STATUS_SYNC_OPEN_MARKER}\n{}{STATUS_SYNC_CLOSE_MARKER}\n",
        normalized_status_sync_payload(payload)
    )
}

/// Self-heal a malformed `status:sync` structure in a status body.
///
/// Returns `Ok(None)` when the body is already well-formed (no markers, or one
/// balanced region), so callers can use it as an idempotent guard. Otherwise
/// returns the body folded into exactly one region (see
/// `StatusBodyTree::normalized` for the exact rules).
pub fn normalize_status_sync_body(status_body: &str) -> Result<Option<String>> {
    if managed_status_sync_block(status_body).is_ok() {
        return Ok(None);
    }
    let tree = StatusBodyTree::parse(status_body)?;
    let normalized = tree.normalized().render();
    ensure_well_formed_output(&normalized)?;
    Ok((normalized != status_body).then_some(normalized))
}

/// Postcondition for every status-sync writer: re-parse the produced body and
/// prove it holds at most one balanced, non-nested managed block. A payload
/// whose own markdown would swallow a delimiter (for example an unterminated
/// code fence) fails here instead of landing a shape preflight refuses.
pub fn ensure_well_formed_output(status_body: &str) -> Result<()> {
    managed_status_sync_block(status_body).map(|_| ()).context(
        "refusing a status:sync write whose result would not re-parse as one managed block",
    )
}

/// Split a well-formed (or normalized) status body into
/// `(outside_prose, payload)`; `payload` is `None` when no region exists.
pub fn split_status_sync_body(status_body: &str) -> Result<(String, Option<String>)> {
    let tree = StatusBodyTree::parse(status_body)?;
    let normalized = tree.normalized();
    Ok((normalized.outside_prose(), normalized.payload))
}

/// Incoming text for a status write, interpreted structurally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusWrite {
    /// The text that belongs inside the managed region.
    pub payload: String,
    /// Free-form status prose the caller placed outside its own region. `Some`
    /// only when the text carried a region and non-blank prose around it, which
    /// makes the write a full status body rather than a bare payload.
    pub prose: Option<String>,
}

/// Interpret status-write text.
///
/// Plain text (no sync markers) is the payload verbatim. Text that carries its
/// own `status:sync` markers is a full status body: it is normalized like a
/// stored body, its region payload becomes the payload, and the prose around it
/// becomes [`StatusWrite::prose`]. The caller's markers are therefore consumed,
/// never written inside the existing region.
pub fn parse_status_write(text: &str) -> Result<StatusWrite> {
    let tree = StatusBodyTree::parse(text)?;
    if !tree.has_markers() {
        return Ok(StatusWrite {
            payload: text.to_string(),
            prose: None,
        });
    }
    let normalized = tree.normalized();
    let outside = normalized.outside_prose();
    let Some(payload) = normalized.payload else {
        // Only orphaned close markers: the remaining prose is the payload.
        return Ok(StatusWrite {
            payload: outside,
            prose: None,
        });
    };
    let prose = outside.trim();
    Ok(StatusWrite {
        payload,
        prose: (!prose.is_empty()).then(|| prose.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact `agent:status` body that `agent-doc write --status` produced on
    /// 2026-10-20 (agent-loop `dc6186d4da`): the caller's full status body, block
    /// and all, was written verbatim as the payload of the existing region.
    pub(crate) const NESTED_INCIDENT_BODY: &str = concat!(
        "\n",
        "📥 Queue handoff: 5 reported items awaiting coordinator integration, verification, and closeout.\n",
        "Review pass done. Two review items remain gated (pypiquotarecheck).\n",
        "\n",
        "<!-- status:sync -->\n",
        "Review pass done. Two review items remain gated (pypiquotarecheck).\n",
        "\n",
        "<!-- status:sync -->\n",
        "Latest published release: v0.35.528.\n",
        "<!-- /status:sync -->\n",
        "<!-- /status:sync -->\n",
    );

    #[test]
    fn markers_come_from_flow_html_blocks_only() {
        let body = concat!(
            "```\n<!-- status:sync -->\n```\n",
            "> <!-- status:sync -->\n",
            "- <!-- status:sync -->\n\n",
            "inline <!-- status:sync --> text\n",
        );
        assert_eq!(managed_status_sync_block(body).unwrap(), None);
        assert_eq!(normalize_status_sync_body(body).unwrap(), None);
    }

    #[test]
    fn well_formed_block_spans_cover_marker_lines() {
        let body = "prose\n<!-- status:sync -->\nA\n<!-- /status:sync -->\nafter\n";
        let block = managed_status_sync_block(body).unwrap().unwrap();
        assert_eq!(
            &body[block.block_start..block.block_end],
            "<!-- status:sync -->\nA\n<!-- /status:sync -->\n"
        );
        assert_eq!(&body[block.content_start..block.content_end], "A\n");
    }

    #[test]
    fn rejects_nested_duplicate_unclosed_and_orphaned_shapes() {
        let err = managed_status_sync_block(NESTED_INCIDENT_BODY).unwrap_err();
        assert!(err.to_string().contains("duplicate or nested"), "{err:#}");
        let err = managed_status_sync_block(concat!(
            "<!-- status:sync -->\none\n<!-- /status:sync -->\n",
            "<!-- status:sync -->\ntwo\n<!-- /status:sync -->\n",
        ))
        .unwrap_err();
        assert!(err.to_string().contains("duplicate or nested"), "{err:#}");
        let err = managed_status_sync_block("<!-- status:sync -->\none\n").unwrap_err();
        assert!(err.to_string().contains("unclosed"), "{err:#}");
        let err = managed_status_sync_block("one\n<!-- /status:sync -->\n").unwrap_err();
        assert!(err.to_string().contains("without an opener"), "{err:#}");
    }

    #[test]
    fn normalizes_the_nested_incident_to_one_region_without_the_leaked_paragraph() {
        let healed = normalize_status_sync_body(NESTED_INCIDENT_BODY)
            .unwrap()
            .expect("nested shape must be repaired");
        assert_eq!(
            healed,
            concat!(
                "\n",
                "📥 Queue handoff: 5 reported items awaiting coordinator integration, verification, and closeout.\n",
                "Review pass done. Two review items remain gated (pypiquotarecheck).\n",
                "\n",
                "<!-- status:sync -->\n",
                "Latest published release: v0.35.528.\n",
                "<!-- /status:sync -->\n",
            )
        );
        assert_eq!(healed.matches(STATUS_SYNC_OPEN_MARKER).count(), 1);
        assert_eq!(healed.matches("Review pass done.").count(), 1);
        assert_eq!(normalize_status_sync_body(&healed).unwrap(), None);
    }

    #[test]
    fn leaked_prose_that_is_new_moves_outside_the_region() {
        let body = concat!(
            "Old summary.\n\n",
            "<!-- status:sync -->\n",
            "New summary.\n\n",
            "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
            "<!-- /status:sync -->\n",
        );
        let healed = normalize_status_sync_body(body).unwrap().unwrap();
        assert_eq!(
            healed,
            concat!(
                "Old summary.\n\n",
                "New summary.\n\n",
                "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
            )
        );
    }

    #[test]
    fn duplicate_regions_keep_the_latest_payload() {
        let body = concat!(
            "Summary.\n\n",
            "<!-- status:sync -->\nPublished: v1\n<!-- /status:sync -->\n\n",
            "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
            "tail\n",
        );
        let healed = normalize_status_sync_body(body).unwrap().unwrap();
        assert_eq!(
            healed,
            concat!(
                "Summary.\n\n",
                "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
                "tail\n",
            )
        );
    }

    #[test]
    fn unclosed_and_orphaned_markers_are_repaired() {
        let healed =
            normalize_status_sync_body("Summary.\n\n<!-- status:sync -->\nPublished: v1\n")
                .unwrap()
                .unwrap();
        assert_eq!(
            healed,
            "Summary.\n\n<!-- status:sync -->\nPublished: v1\n<!-- /status:sync -->\n"
        );
        let healed = normalize_status_sync_body("Summary.\n<!-- /status:sync -->\n")
            .unwrap()
            .unwrap();
        assert_eq!(healed, "Summary.\n");
    }

    #[test]
    fn status_write_text_with_its_own_region_is_a_full_body() {
        let write = parse_status_write(concat!(
            "Review pass done.\n\n",
            "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
        ))
        .unwrap();
        assert_eq!(write.payload, "Published: v2\n");
        assert_eq!(write.prose.as_deref(), Some("Review pass done."));

        let bare = parse_status_write("<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->")
            .unwrap();
        assert_eq!(bare.payload, "Published: v2\n");
        assert_eq!(bare.prose, None);

        let plain = parse_status_write("Published: v2").unwrap();
        assert_eq!(plain.payload, "Published: v2");
        assert_eq!(plain.prose, None);
    }

    #[test]
    fn delimiters_hidden_by_unterminated_markdown_fail_closed() {
        for body in [
            "```\ncode\n<!-- status:sync -->\nA\n<!-- /status:sync -->\n",
            "<div>\n<!-- status:sync -->\nA\n<!-- /status:sync -->\n",
            "    <!-- status:sync -->\n",
        ] {
            let err = managed_status_sync_block(body).unwrap_err();
            assert!(
                err.to_string().contains("hides a status:sync delimiter"),
                "{body:?}: {err:#}"
            );
            assert!(normalize_status_sync_body(body).is_err(), "{body:?}");
        }
        // A terminated fence quoting a delimiter is ordinary content.
        let quoted =
            "```\n<!-- status:sync -->\n```\n<!-- status:sync -->\nA\n<!-- /status:sync -->\n";
        let block = managed_status_sync_block(quoted).unwrap().unwrap();
        assert_eq!(&quoted[block.content_start..block.content_end], "A\n");
    }

    #[test]
    fn payload_that_would_swallow_the_close_delimiter_is_refused() {
        let rendered = render_status_sync_block("```\nunterminated");
        assert!(ensure_well_formed_output(&rendered).is_err());
        assert!(ensure_well_formed_output(&render_status_sync_block("```\nok\n```")).is_ok());
    }
}
