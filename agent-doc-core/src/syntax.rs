use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTokenKind {
    ComponentOpen,
    ComponentClose,
    ComponentBody,
    PatchOpen,
    PatchClose,
    Boundary,
    ScratchComment,
    ScratchCommentBody,
    Prompt,
    ResponseHeading,
    TrackedId,
    LabelTag,
    /// Markdown **strong** emphasis span (`**text**` or `__text__`), rendered
    /// bold in the editor (`#editor-bold-markdown-rendering`).
    Bold,
    /// Markdown *emphasis* span (`*text*` or `_text_`), rendered italic.
    Italic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VisualToken {
    pub kind: VisualTokenKind,
    pub start: usize,
    pub end: usize,
}

pub fn collect_visual_tokens(doc: &str) -> Vec<VisualToken> {
    let code_ranges = crate::component::find_code_ranges(doc);
    let mut tokens = Vec::new();

    collect_comment_tokens(doc, &code_ranges, &mut tokens);
    collect_component_body_tokens(doc, &code_ranges, &mut tokens);
    collect_scratch_comment_body_tokens(doc, &code_ranges, &mut tokens);
    collect_line_tokens(doc, &code_ranges, &mut tokens);
    collect_label_tags(doc, &code_ranges, &mut tokens);
    collect_tracked_ids(doc, &code_ranges, &mut tokens);
    collect_emphasis_tokens(doc, &code_ranges, &mut tokens);

    tokens.sort_by_key(|token| (token.start, token.end));
    tokens
}

/// Collect markdown emphasis spans (`#editor-bold-markdown-rendering`): `**…**`
/// / `__…__` → [`VisualTokenKind::Bold`], `*…*` / `_…_` → [`VisualTokenKind::Italic`].
/// Scans per line (no cross-line matches), skips code ranges, and marks consumed
/// byte ranges so a strong run is never re-matched as italic or double-emitted.
/// Markers are ASCII so byte-index scanning stays on char boundaries; emitted
/// offsets are byte offsets (the FFI layer converts to UTF-16).
fn collect_emphasis_tokens(doc: &str, code_ranges: &[(usize, usize)], out: &mut Vec<VisualToken>) {
    let mut line_start = 0usize;
    for line in doc.split_inclusive('\n') {
        let base = line_start;
        line_start += line.len();
        let bytes = line.as_bytes();
        let mut consumed = vec![false; bytes.len()];

        // Strong emphasis first (double markers), then italic (single), so a
        // `**bold**` run is not re-matched by the `*` italic pass.
        for (marker, kind, mlen) in [
            (b'*', VisualTokenKind::Bold, 2usize),
            (b'_', VisualTokenKind::Bold, 2usize),
            (b'*', VisualTokenKind::Italic, 1usize),
            (b'_', VisualTokenKind::Italic, 1usize),
        ] {
            // Marker-char test (does not read `consumed`, so it never conflicts
            // with the mutable borrow when marking a span consumed below).
            let is_marker_chars = |i: usize| -> bool {
                if i + mlen > bytes.len() {
                    return false;
                }
                (0..mlen).all(|k| bytes[i + k] == marker)
                    // A single-marker italic must not be part of a double marker.
                    && (mlen == 2
                        || ((i == 0 || bytes[i - 1] != marker)
                            && (i + 1 >= bytes.len() || bytes[i + 1] != marker)))
            };
            let is_marker = |i: usize, consumed: &[bool]| -> bool {
                is_marker_chars(i) && (0..mlen).all(|k| !consumed[i + k])
            };
            let mut i = 0usize;
            while i + mlen <= bytes.len() {
                if is_marker(i, &consumed) {
                    let mut j = i + mlen;
                    let mut close = None;
                    while j + mlen <= bytes.len() {
                        if is_marker(j, &consumed) {
                            close = Some(j);
                            break;
                        }
                        j += 1;
                    }
                    if let Some(close) = close
                        && close > i + mlen
                    {
                        let tok_start = base + i;
                        let tok_end = base + close + mlen;
                        if !overlaps_code(tok_start, tok_end, code_ranges) {
                            out.push(VisualToken {
                                kind,
                                start: tok_start,
                                end: tok_end,
                            });
                            for slot in consumed.iter_mut().take(close + mlen).skip(i) {
                                *slot = true;
                            }
                        }
                        i = close + mlen;
                        continue;
                    }
                }
                i += 1;
            }
        }
    }
}

enum CommentKind<'a> {
    ComponentOpen(&'a str),
    ComponentClose(&'a str),
    PatchOpen,
    PatchClose,
    Boundary,
    ScratchComment,
}

fn classify_comment_inner(inner: &str) -> CommentKind<'_> {
    if inner.starts_with("agent:boundary:") {
        CommentKind::Boundary
    } else if let Some(rest) = inner.strip_prefix("agent:") {
        let name = rest.split_whitespace().next().unwrap_or_default();
        if name.is_empty() {
            CommentKind::ScratchComment
        } else {
            CommentKind::ComponentOpen(name)
        }
    } else if let Some(rest) = inner.strip_prefix("/agent:") {
        let name = rest.split_whitespace().next().unwrap_or_default();
        if name.is_empty() {
            CommentKind::ScratchComment
        } else {
            CommentKind::ComponentClose(name)
        }
    } else if inner.starts_with("patch:") {
        CommentKind::PatchOpen
    } else if inner.starts_with("/patch:") {
        CommentKind::PatchClose
    } else {
        CommentKind::ScratchComment
    }
}

fn collect_comment_tokens(doc: &str, code_ranges: &[(usize, usize)], out: &mut Vec<VisualToken>) {
    let mut search_from = 0usize;
    while let Some(rel) = doc[search_from..].find("<!--") {
        let start = search_from + rel;
        let Some(close_rel) = doc[start + 4..].find("-->") else {
            break;
        };
        let end = start + 4 + close_rel + 3;
        search_from = end;

        if overlaps_code(start, end, code_ranges) {
            continue;
        }

        let inner = doc[start + 4..end - 3].trim();
        let kind = match classify_comment_inner(inner) {
            CommentKind::ComponentOpen(_) => VisualTokenKind::ComponentOpen,
            CommentKind::ComponentClose(_) => VisualTokenKind::ComponentClose,
            CommentKind::PatchOpen => VisualTokenKind::PatchOpen,
            CommentKind::PatchClose => VisualTokenKind::PatchClose,
            CommentKind::Boundary => VisualTokenKind::Boundary,
            CommentKind::ScratchComment => VisualTokenKind::ScratchComment,
        };
        out.push(VisualToken { kind, start, end });
    }
}

fn collect_component_body_tokens(
    doc: &str,
    code_ranges: &[(usize, usize)],
    out: &mut Vec<VisualToken>,
) {
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = doc[search_from..].find("<!--") {
        let start = search_from + rel;
        let Some(close_rel) = doc[start + 4..].find("-->") else {
            break;
        };
        let end = start + 4 + close_rel + 3;
        search_from = end;

        if overlaps_code(start, end, code_ranges) {
            continue;
        }

        let inner = doc[start + 4..end - 3].trim();
        match classify_comment_inner(inner) {
            CommentKind::ComponentOpen(name) => {
                stack.push((name.to_string(), end));
            }
            CommentKind::ComponentClose(name) => {
                if let Some(idx) = stack.iter().rposition(|(open_name, _)| open_name == name) {
                    let (_, body_start) = stack.remove(idx);
                    if body_start < start && !doc[body_start..start].trim().is_empty() {
                        out.push(VisualToken {
                            kind: VisualTokenKind::ComponentBody,
                            start: body_start,
                            end: start,
                        });
                    }
                }
            }
            CommentKind::PatchOpen
            | CommentKind::PatchClose
            | CommentKind::Boundary
            | CommentKind::ScratchComment => {}
        }
    }
}

fn collect_scratch_comment_body_tokens(
    doc: &str,
    code_ranges: &[(usize, usize)],
    out: &mut Vec<VisualToken>,
) {
    let mut search_from = 0usize;
    while let Some(rel) = doc[search_from..].find("<!--") {
        let start = search_from + rel;
        let Some(close_rel) = doc[start + 4..].find("-->") else {
            break;
        };
        let end = start + 4 + close_rel + 3;
        search_from = end;

        if overlaps_code(start, end, code_ranges) {
            continue;
        }

        let inner = doc[start + 4..end - 3].trim();
        if !matches!(classify_comment_inner(inner), CommentKind::ScratchComment) {
            continue;
        }

        let body_start = start + 4;
        let body_end = end - 3;
        if body_start < body_end && !doc[body_start..body_end].trim().is_empty() {
            out.push(VisualToken {
                kind: VisualTokenKind::ScratchCommentBody,
                start: body_start,
                end: body_end,
            });
        }
    }
}

fn collect_line_tokens(doc: &str, code_ranges: &[(usize, usize)], out: &mut Vec<VisualToken>) {
    let mut offset = 0usize;
    for chunk in doc.split_inclusive('\n') {
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let line_end = offset + line.len();
        if !overlaps_code(offset, line_end, code_ranges) {
            let leading_ws = line.len() - line.trim_start_matches([' ', '\t']).len();
            let trimmed = &line[leading_ws..];
            if trimmed.starts_with("❯ ") {
                out.push(VisualToken {
                    kind: VisualTokenKind::Prompt,
                    start: offset + leading_ws,
                    end: line_end,
                });
            }
            if trimmed.starts_with("### Re:") {
                out.push(VisualToken {
                    kind: VisualTokenKind::ResponseHeading,
                    start: offset + leading_ws,
                    end: line_end,
                });
            }
        }
        offset += chunk.len();
    }
}

fn collect_tracked_ids(doc: &str, code_ranges: &[(usize, usize)], out: &mut Vec<VisualToken>) {
    let mut search_from = 0usize;
    while let Some(rel) = doc[search_from..].find("[#") {
        let start = search_from + rel;
        let rest = &doc[start + 2..];
        let Some(close_rel) = rest.find(']') else {
            break;
        };
        let end = start + 2 + close_rel + 1;
        search_from = end;

        if overlaps_code(start, end, code_ranges) {
            continue;
        }

        let inner = &rest[..close_rel];
        if !inner.is_empty()
            && inner
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            out.push(VisualToken {
                kind: VisualTokenKind::TrackedId,
                start,
                end,
            });
        }
    }
}

fn collect_label_tags(doc: &str, code_ranges: &[(usize, usize)], out: &mut Vec<VisualToken>) {
    let mut search_from = 0usize;
    while let Some(rel) = doc[search_from..].find('[') {
        let start = search_from + rel;
        let rest = &doc[start + 1..];
        let Some(close_rel) = rest.find(']') else {
            break;
        };
        let end = start + 1 + close_rel + 1;
        search_from = end;

        if overlaps_code(start, end, code_ranges) {
            continue;
        }
        if start > 0 && doc.as_bytes()[start - 1] == b'!' {
            continue;
        }
        if doc[end..].starts_with('(') {
            continue;
        }

        let inner = &rest[..close_rel];
        if inner.starts_with('#') || inner == "x" {
            continue;
        }
        if inner.len() <= 1 {
            continue;
        }
        if !inner
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
        {
            continue;
        }
        if !inner.chars().any(|ch| ch.is_ascii_alphabetic()) {
            continue;
        }

        out.push(VisualToken {
            kind: VisualTokenKind::LabelTag,
            start,
            end,
        });
    }
}

fn overlaps_code(start: usize, end: usize, code_ranges: &[(usize, usize)]) -> bool {
    code_ranges
        .iter()
        .any(|&(code_start, code_end)| start < code_end && end > code_start)
}

#[cfg(test)]
mod tests {
    use super::{VisualTokenKind, collect_visual_tokens};

    fn tokens_of(doc: &str, kind: VisualTokenKind) -> Vec<String> {
        collect_visual_tokens(doc)
            .into_iter()
            .filter(|t| t.kind == kind)
            .map(|t| doc[t.start..t.end].to_string())
            .collect()
    }

    #[test]
    fn collects_bold_emphasis_spans() {
        // #editor-bold-markdown-rendering: ** ** and __ __ → Bold.
        let doc = "this is **bold** and __also bold__ here\n";
        assert_eq!(
            tokens_of(doc, VisualTokenKind::Bold),
            vec!["**bold**".to_string(), "__also bold__".to_string()]
        );
    }

    #[test]
    fn collects_italic_emphasis_spans_not_overlapping_bold() {
        let doc = "**strong** then *soft* and _under_\n";
        assert_eq!(tokens_of(doc, VisualTokenKind::Bold), vec!["**strong**"]);
        assert_eq!(
            tokens_of(doc, VisualTokenKind::Italic),
            vec!["*soft*".to_string(), "_under_".to_string()]
        );
    }

    #[test]
    fn bold_pin_marker_renders_bold() {
        // The operator pin marker `**prioritized**` / `**pin**` is bold emphasis.
        let doc = "- **prioritized** do [#x]\n- **pin** do [#y]\n";
        assert_eq!(
            tokens_of(doc, VisualTokenKind::Bold),
            vec!["**prioritized**".to_string(), "**pin**".to_string()]
        );
    }

    #[test]
    fn emphasis_inside_code_is_ignored() {
        let doc = "before\n```\n**not bold**\n```\nafter **yes**\n";
        assert_eq!(tokens_of(doc, VisualTokenKind::Bold), vec!["**yes**"]);
    }

    #[test]
    fn unclosed_emphasis_marker_is_not_a_span() {
        let doc = "a ** dangling and b * lonely\n";
        assert!(tokens_of(doc, VisualTokenKind::Bold).is_empty());
        assert!(tokens_of(doc, VisualTokenKind::Italic).is_empty());
    }

    #[test]
    fn collects_agent_and_scratch_comments() {
        let doc = "\
<!-- agent:exchange patch=append -->
hello
<!-- /agent:exchange -->
<!-- patch:exchange -->
<!-- /patch:exchange -->
<!-- agent:boundary:abc12345 -->
<!-- scratch note -->
";
        let tokens = collect_visual_tokens(doc);
        let kinds: Vec<VisualTokenKind> = tokens.iter().map(|token| token.kind).collect();
        assert_eq!(
            kinds,
            vec![
                VisualTokenKind::ComponentOpen,
                VisualTokenKind::ComponentBody,
                VisualTokenKind::ComponentClose,
                VisualTokenKind::PatchOpen,
                VisualTokenKind::PatchClose,
                VisualTokenKind::Boundary,
                VisualTokenKind::ScratchComment,
                VisualTokenKind::ScratchCommentBody,
            ]
        );
    }

    #[test]
    fn ignores_visual_markers_inside_fenced_code() {
        let doc = "\
```md
<!-- agent:exchange -->
❯ hidden prompt
### Re: hidden
[#hide]
```
";
        assert!(collect_visual_tokens(doc).is_empty());
    }

    #[test]
    fn collects_prompts_response_headings_and_tracked_ids() {
        let doc = "\
❯ do #qey0. spec-test-build-install-commit-push
### Re: #qey0 editor highlighting — gpt-5
- [ ] [#qey0] Track this work item
- [ ] [#multi-user] Alias-style id should still highlight
- [ ] [recommended] Treat labels as metadata, not broken markdown refs
";
        let tokens = collect_visual_tokens(doc);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == VisualTokenKind::Prompt)
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == VisualTokenKind::ResponseHeading)
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == VisualTokenKind::TrackedId)
                .count(),
            2
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == VisualTokenKind::LabelTag)
        );
    }

    #[test]
    fn ignores_image_alt_text_and_checklists_for_label_tags() {
        let doc = "\
- [ ] keep checklist
![img_1.png](img_1.png)
- [recommended] keep label
";
        let tokens = collect_visual_tokens(doc);
        let label_tags: Vec<&super::VisualToken> = tokens
            .iter()
            .filter(|token| token.kind == VisualTokenKind::LabelTag)
            .collect();
        assert_eq!(label_tags.len(), 1);
        assert_eq!(
            &doc[label_tags[0].start..label_tags[0].end],
            "[recommended]"
        );
    }

    #[test]
    fn collects_multiline_scratch_comment_body() {
        let doc = "\
<!--
scratch note
```md
![img_3.png](img_3.png)
```
-->
";
        let tokens = collect_visual_tokens(doc);
        let body = tokens
            .iter()
            .find(|token| token.kind == VisualTokenKind::ScratchCommentBody)
            .expect("expected scratch comment body token");
        let body_text = &doc[body.start..body.end];
        assert!(body_text.contains("scratch note"));
        assert!(body_text.contains("![img_3.png](img_3.png)"));
    }
}
