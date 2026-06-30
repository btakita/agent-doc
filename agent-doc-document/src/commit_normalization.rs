//! Pure committed-document normalization used before comparing or staging text.

use crate::transient_markers::normalize_transient_agent_doc_markers;
use agent_doc_prompt_lines::{
    line_looks_like_markdown_list_item, line_looks_like_prompt_prefix_repair_start,
};

fn is_response_heading_line(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
}

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

fn prefix_prompt_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('❯') || line_looks_like_markdown_list_item(trimmed)
    {
        return None;
    }
    let indent_len = line.len() - trimmed.len();
    Some(format!("{}❯ {}", &line[..indent_len], trimmed))
}

fn answered_prompt_prelude_start(lines: &[&str]) -> Option<usize> {
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if line_looks_like_prompt_prefix_repair_start(trimmed, false) {
            return Some(idx);
        }
    }
    None
}

pub fn canonicalize_answered_prompt_prefixes(exchange_content: &str) -> String {
    // When a response heading is present, the contiguous prose block
    // immediately above it is the user prelude for that turn. Canonicalize
    // that prelude back to `❯ ...` so answered prompts keep their visual
    // marker after staging/cleanup instead of collapsing to bare lines.

    let lines: Vec<&str> = exchange_content.split_inclusive('\n').collect();
    if lines.is_empty() {
        return exchange_content.to_string();
    }

    let mut line_in_fence = vec![false; lines.len()];
    let mut in_fence = false;
    let mut fence_char = '\0';
    let mut fence_len = 0usize;
    for idx in 0..lines.len() {
        let line = lines[idx].trim_end_matches('\n');
        let trimmed = line.trim();
        if !in_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_fence = true;
                fence_char = fc;
                fence_len = fl;
                line_in_fence[idx] = true;
                continue;
            }
        } else {
            line_in_fence[idx] = true;
            if fence_close(trimmed, fence_char, fence_len) {
                in_fence = false;
            }
            continue;
        }
    }

    let mut prefix_targets = vec![false; lines.len()];
    for idx in 0..lines.len() {
        let trimmed = lines[idx].trim_end_matches('\n').trim();
        if line_in_fence[idx] || !is_response_heading_line(trimmed) {
            continue;
        }

        let mut block_indices = Vec::new();
        let mut cursor = idx;
        let mut stopped_on_response_heading = false;
        while cursor > 0 {
            cursor -= 1;
            let line = lines[cursor].trim_end_matches('\n');
            let trimmed = line.trim();
            if line_in_fence[cursor]
                || trimmed.is_empty()
                || trimmed.starts_with("<!--")
                || is_response_heading_line(trimmed)
            {
                stopped_on_response_heading =
                    !line_in_fence[cursor] && is_response_heading_line(trimmed);
                break;
            }
            block_indices.push(cursor);
        }
        block_indices.reverse();
        if block_indices.is_empty() {
            continue;
        }
        // A prose block that butts directly against a preceding `### Re:`
        // heading with no blank-line / comment separator is the trailing body
        // of that response, not a fresh user prelude. Never canonicalize those
        // lines into `❯ ` prompt prefixes.
        if stopped_on_response_heading {
            continue;
        }

        let block_lines: Vec<&str> = block_indices
            .iter()
            .map(|&line_idx| lines[line_idx])
            .collect();
        let Some(prefix_start) = answered_prompt_prelude_start(&block_lines) else {
            continue;
        };
        for line_idx in block_indices.into_iter().skip(prefix_start) {
            prefix_targets[line_idx] = true;
        }
    }

    let mut normalized = String::with_capacity(exchange_content.len());
    let mut changed = false;
    for (idx, segment) in lines.iter().enumerate() {
        let line = segment.trim_end_matches('\n');
        if prefix_targets[idx] {
            if let Some(prefixed) = prefix_prompt_line(line) {
                normalized.push_str(&prefixed);
                changed |= prefixed != line;
            } else {
                normalized.push_str(line);
            }
        } else {
            normalized.push_str(line);
        }
        if segment.ends_with('\n') {
            normalized.push('\n');
        }
    }

    if changed {
        normalized
    } else {
        exchange_content.to_string()
    }
}

pub fn normalize_committed_exchange_artifacts(content: &str) -> String {
    let transient = normalize_transient_agent_doc_markers(content);
    let body = match agent_doc_frontmatter::frontmatter::parse(&transient) {
        Ok((_, body)) => body,
        Err(_) => return transient,
    };
    let prefix_len = transient.len().saturating_sub(body.len());
    let Ok(components) = agent_doc_element::element::parse(body) else {
        return transient;
    };

    let mut rebuilt = String::with_capacity(transient.len());
    rebuilt.push_str(&transient[..prefix_len]);
    let mut last = 0usize;
    let mut changed = false;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        rebuilt.push_str(&body[last..comp.open_end]);
        if comp.name == "exchange" {
            let normalized = canonicalize_answered_prompt_prefixes(comp.content(body));
            changed |= normalized != comp.content(body);
            rebuilt.push_str(&normalized);
        } else {
            rebuilt.push_str(comp.content(body));
        }
        rebuilt.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    rebuilt.push_str(&body[last..]);

    if changed { rebuilt } else { transient }
}

pub fn normalize_component_content_for_absorb(content: &str) -> String {
    normalize_transient_agent_doc_markers(content)
        .trim()
        .to_string()
}

pub fn redact_component_contents_for_absorb(body: &str) -> Option<String> {
    let components = agent_doc_element::element::parse(body).ok()?;
    let mut redacted = String::with_capacity(body.len());
    let mut last = 0usize;
    for comp in components {
        if comp.open_end < last {
            continue;
        }
        redacted.push_str(&body[last..comp.open_end]);
        redacted.push_str(&body[comp.close_start..comp.close_end]);
        last = comp.close_end;
    }
    redacted.push_str(&body[last..]);
    Some(redacted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_answered_prompt_prefixes_uses_opt_in_prompt_start() {
        let exchange = "\
### Re: sync latency — gpt-5

The current tree has already started making this accountable.
### Re: closeout guard — gpt-5

No additional prompt-bearing change was present.
Please rerun the deploy check.
### Re: deploy check — gpt-5

Done.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            normalized
                .contains("\nThe current tree has already started making this accountable.\n"),
            "plain assistant prose before the next response heading must stay bare:\n{normalized}"
        );
        assert!(
            !normalized
                .contains("\n❯ The current tree has already started making this accountable.\n"),
            "assistant prose must not become a prompt by default:\n{normalized}"
        );
        assert!(
            normalized.contains("\n❯ Please rerun the deploy check.\n"),
            "soft prompt requests before a response heading should still be canonicalized:\n{normalized}"
        );
    }

    #[test]
    fn canonicalize_answered_prompt_prefixes_never_prefixes_duplicate_response_body() {
        let exchange = "\
❯ do [#fix-thing]
### Re: fix thing — opus-4-8
**Scope/honesty:** narrow.
**Commits:** abc123.
### Re: fix thing — opus-4-8 (HEAD)
**Scope/honesty:** narrow.
**Commits:** abc123.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            !normalized.contains("❯ **Scope/honesty:**"),
            "duplicate response body must not be rewritten as a prompt:\n{normalized}"
        );
        assert!(
            !normalized.contains("❯ **Commits:**"),
            "duplicate response body must not be rewritten as a prompt:\n{normalized}"
        );
        assert_eq!(
            normalized.matches('❯').count(),
            1,
            "exactly the existing user prompt keeps its marker:\n{normalized}"
        );
    }

    #[test]
    fn canonicalize_answered_prompt_prefixes_preserves_markdown_lists() {
        let exchange = "\
Please compare these options:
- keep this bullet bare
  - keep this nested bullet bare
1. keep this ordered bullet bare
### Re: options — gpt-5

Done.
";

        let normalized = canonicalize_answered_prompt_prefixes(exchange);

        assert!(
            normalized.starts_with(
                "❯ Please compare these options:\n- keep this bullet bare\n  - keep this nested bullet bare\n1. keep this ordered bullet bare\n"
            ),
            "prompt prose should be prefixed without rewriting markdown list items:\n{normalized}"
        );
        assert!(
            !normalized.contains("\n❯ - keep this bullet bare")
                && !normalized.contains("\n❯   - keep this nested bullet bare")
                && !normalized.contains("\n❯ 1. keep this ordered bullet bare"),
            "markdown list items must not receive prompt prefixes:\n{normalized}"
        );
    }

    #[test]
    fn normalize_committed_exchange_artifacts_canonicalizes_exchange_component() {
        let doc = "\
---
agent_doc_session: test
agent_doc_format: template
---

## Exchange

<!-- agent:exchange patch=append -->
Please rerun the deploy check.
### Re: deploy check — gpt-5

Done.
<!-- /agent:exchange -->
";

        let normalized = normalize_committed_exchange_artifacts(doc);

        assert!(normalized.contains("agent_doc_session: test"));
        assert!(normalized.contains("\n❯ Please rerun the deploy check.\n"));
        assert!(normalized.contains("\n### Re: deploy check — gpt-5\n"));
    }

    #[test]
    fn redact_component_contents_handles_nested_components() {
        let body = r#"## Status

<!-- agent:status patch=replace -->
Status content here.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
Some exchange content.
Add <!-- agent:queue -->...<!-- /agent:queue --> to the template.
More content.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] task
<!-- /agent:pending -->
"#;
        let result = redact_component_contents_for_absorb(body);
        assert!(result.is_some(), "should not panic on nested components");
        let redacted = result.unwrap();
        assert!(
            redacted.contains("<!-- agent:status patch=replace -->"),
            "should contain status open marker"
        );
        assert!(
            redacted.contains("<!-- /agent:status -->"),
            "should contain status close marker"
        );
        assert!(
            !redacted.contains("Status content here."),
            "should redact status content"
        );
        assert!(
            !redacted.contains("Some exchange content."),
            "should redact exchange content (including nested markers)"
        );
    }
}
