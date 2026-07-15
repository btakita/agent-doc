//! Pure recognition and coalescing of accidentally replayed whole documents.

/// A byte-exact replay of the complete session document. This is distinct from
/// ordinary duplicated prose: the candidate must be two (or a power-of-two
/// number of) identical, structurally complete agent-doc projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactDocumentReplay<'a> {
    pub canonical: &'a str,
    pub copies: usize,
    /// Lines present only in the retained projection. Zero means every replay
    /// copy was byte-identical; non-zero means a stale copy was a strict,
    /// order-preserving subset of the retained live projection.
    pub retained_additions: usize,
}

/// Coalesce a legacy dual-delivery whole-document replay to one canonical
/// projection. Besides byte-identical copies, this accepts one deliberately
/// narrow live shape: two complete projections with the same byte-identical
/// frontmatter where one projection's lines are an order-preserving subset of
/// the other. The superset is retained, so text added while a stale replica was
/// replayed cannot be lost. Divergent or reordered copies remain fail-closed.
pub fn coalesce_exact_document_replay(content: &str) -> Option<ExactDocumentReplay<'_>> {
    let mut canonical = content;
    let mut copies = 1usize;

    loop {
        let len = canonical.len();
        if len == 0 || !len.is_multiple_of(2) {
            break;
        }
        let midpoint = len / 2;
        if !canonical.is_char_boundary(midpoint) || canonical[..midpoint] != canonical[midpoint..] {
            break;
        }
        canonical = &canonical[..midpoint];
        copies = copies.saturating_mul(2);
    }

    if copies > 1 && looks_like_complete_agent_doc_projection(canonical) {
        return Some(ExactDocumentReplay {
            canonical,
            copies,
            retained_additions: 0,
        });
    }

    coalesce_monotonic_dual_document_replay(content)
}

/// True when `content` is two complete projections sharing frontmatter, but
/// not necessarily safe to coalesce. Merge callers use this to reject a
/// divergent replay instead of feeding it to a whole-document fallback.
pub(crate) fn has_dual_complete_projection_shape(content: &str) -> bool {
    split_dual_projection(content).is_some_and(|(first, second)| {
        looks_like_complete_agent_doc_projection(first)
            && looks_like_complete_agent_doc_projection(second)
    })
}

fn coalesce_monotonic_dual_document_replay(content: &str) -> Option<ExactDocumentReplay<'_>> {
    let (first, second) = split_dual_projection(content)?;
    if !looks_like_complete_agent_doc_projection(first)
        || !looks_like_complete_agent_doc_projection(second)
    {
        return None;
    }

    let first_lines = first.lines().collect::<Vec<_>>();
    let second_lines = second.lines().collect::<Vec<_>>();
    let (canonical, retained_additions) =
        if line_sequence_is_subsequence(&second_lines, &first_lines) {
            (first, first_lines.len().saturating_sub(second_lines.len()))
        } else if line_sequence_is_subsequence(&first_lines, &second_lines) {
            (second, second_lines.len().saturating_sub(first_lines.len()))
        } else {
            return None;
        };
    (retained_additions > 0).then_some(ExactDocumentReplay {
        canonical,
        copies: 2,
        retained_additions,
    })
}

fn split_dual_projection(content: &str) -> Option<(&str, &str)> {
    let frontmatter_end = content.find("\n---\n")?.checked_add(5)?;
    let frontmatter = content.get(..frontmatter_end)?;
    let split = content.get(frontmatter_end..)?.find(frontmatter)? + frontmatter_end;
    Some((content.get(..split)?, content.get(split..)?))
}

fn line_sequence_is_subsequence(needle: &[&str], haystack: &[&str]) -> bool {
    let mut matched = 0usize;
    for line in haystack {
        if needle
            .get(matched)
            .is_some_and(|candidate| candidate == line)
        {
            matched += 1;
            if matched == needle.len() {
                return true;
            }
        }
    }
    matched == needle.len()
}

fn looks_like_complete_agent_doc_projection(content: &str) -> bool {
    content.starts_with("---\n")
        && content.contains("\nagent_doc_session:")
        && content.contains("<!-- agent:exchange")
        && content.trim_end().ends_with("<!-- /agent:done -->")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n";
    const TAIL: &str = "<!-- /agent:exchange -->\n\n<!-- agent:done -->\n<!-- /agent:done -->\n";

    #[test]
    fn divergent_complete_projections_are_recognized_but_not_coalesced() {
        let first = format!("{HEADER}<!-- agent:exchange -->\nfirst branch\n{TAIL}");
        let second = format!("{HEADER}<!-- agent:exchange -->\nsecond branch\n{TAIL}");
        let replay = format!("{first}{second}");

        assert!(has_dual_complete_projection_shape(&replay));
        assert!(coalesce_exact_document_replay(&replay).is_none());
    }
}
