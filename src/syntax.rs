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
    Prompt,
    ResponseHeading,
    TrackedId,
    LabelTag,
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
    collect_line_tokens(doc, &code_ranges, &mut tokens);
    collect_label_tags(doc, &code_ranges, &mut tokens);
    collect_tracked_ids(doc, &code_ranges, &mut tokens);

    tokens.sort_by_key(|token| (token.start, token.end));
    tokens
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
}
