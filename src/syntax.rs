use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualTokenKind {
    ComponentOpen,
    ComponentClose,
    PatchOpen,
    PatchClose,
    Boundary,
    ScratchComment,
    Prompt,
    ResponseHeading,
    TrackedId,
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
    collect_line_tokens(doc, &code_ranges, &mut tokens);
    collect_tracked_ids(doc, &code_ranges, &mut tokens);

    tokens.sort_by_key(|token| (token.start, token.end));
    tokens
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
        let kind = if inner.starts_with("agent:boundary:") {
            VisualTokenKind::Boundary
        } else if inner.starts_with("agent:") {
            VisualTokenKind::ComponentOpen
        } else if inner.starts_with("/agent:") {
            VisualTokenKind::ComponentClose
        } else if inner.starts_with("patch:") {
            VisualTokenKind::PatchOpen
        } else if inner.starts_with("/patch:") {
            VisualTokenKind::PatchClose
        } else {
            VisualTokenKind::ScratchComment
        };
        out.push(VisualToken { kind, start, end });
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
    }
}
