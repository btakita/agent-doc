//! Pure template response materialization policy.

use anyhow::Result;

use crate::PatchBlock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateResponseWriteProof {
    pub explicit_components: Vec<String>,
    pub unmatched_len: usize,
}

impl TemplateResponseWriteProof {
    pub fn has_real_body(&self) -> bool {
        !self.explicit_components.is_empty() || self.unmatched_len > 0
    }
}

pub fn template_response_write_proof(
    patches: &[PatchBlock],
    unmatched: &str,
) -> TemplateResponseWriteProof {
    TemplateResponseWriteProof {
        explicit_components: patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !agent_doc_element::element::is_backlog_component(&patch.name))
            .filter(|patch| !agent_doc_element::element::is_review_component(&patch.name))
            .filter(|patch| !patch.content.trim().is_empty())
            .map(|patch| patch.name.clone())
            .collect(),
        unmatched_len: unmatched.trim().len(),
    }
}

pub fn ensure_template_response_write_proof(patches: &[PatchBlock], unmatched: &str) -> Result<()> {
    let proof = template_response_write_proof(patches, unmatched);
    if proof.has_real_body() {
        return Ok(());
    }

    anyhow::bail!(
        "template response contains no real response-body write — include at least one non-empty response patch or non-empty unmatched response body"
    );
}

pub fn ensure_strict_template_response_heading(
    patches: &[PatchBlock],
    unmatched: &str,
) -> Result<()> {
    if template_response_has_heading(patches, unmatched) {
        return Ok(());
    }

    anyhow::bail!(
        "strict template closeout response must include a `### Re:` response heading in `patch:exchange` or unmatched response body"
    );
}

pub fn ensure_strict_template_response_heading_for_current_doc(
    current_content: &str,
    patches: &[PatchBlock],
    unmatched: &str,
) -> Result<()> {
    match ensure_strict_template_response_heading(patches, unmatched) {
        Ok(()) => Ok(()),
        Err(_)
            if live_exchange_tail_proves_streamed_response_heading(
                current_content,
                patches,
                unmatched,
            ) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

pub fn template_response_has_heading(patches: &[PatchBlock], unmatched: &str) -> bool {
    response_text_has_heading(unmatched)
        || patches.iter().any(|patch| {
            patch.name == "exchange"
                && !patch.content.trim().is_empty()
                && response_text_has_heading(&patch.content)
        })
}

pub fn live_exchange_tail_proves_streamed_response_heading(
    current_content: &str,
    patches: &[PatchBlock],
    unmatched: &str,
) -> bool {
    if !unmatched.trim().is_empty() {
        return false;
    }

    let mut non_empty = patches
        .iter()
        .filter(|patch| !patch.content.trim().is_empty());
    let Some(patch) = non_empty.next() else {
        return false;
    };
    if non_empty.next().is_some() || patch.name != "exchange" {
        return false;
    }

    let Ok(components) = agent_doc_element::element::parse(current_content) else {
        return false;
    };
    let Some(exchange) = components
        .iter()
        .rev()
        .find(|component| component.name == "exchange")
    else {
        return false;
    };
    let exchange_content = exchange.content(current_content);
    let Some(tail_start) = offset_after_last_prompt_line(exchange_content) else {
        return false;
    };

    response_text_has_heading(&exchange_content[tail_start..])
}

pub fn offset_after_last_prompt_line(text: &str) -> Option<usize> {
    let mut offset = 0usize;
    let mut last = None;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('❯') {
            last = Some(offset + line.len());
        }
        offset += line.len();
    }
    last
}

pub fn response_text_has_heading(text: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("##### Re:")
            || trimmed.starts_with("###### Re:")
            || trimmed.starts_with("## Re:")
    })
}

pub fn same_ignoring_trailing_newlines(left: &str, right: &str) -> bool {
    left.trim_end_matches('\n') == right.trim_end_matches('\n')
}

pub fn serialize_template_response(patches: &[PatchBlock], unmatched: &str) -> String {
    let mut out = String::new();
    for patch in patches {
        out.push_str("<!-- patch:");
        out.push_str(&patch.name);
        if !patch.attrs.is_empty() {
            let mut attrs: Vec<_> = patch.attrs.iter().collect();
            attrs.sort_by_key(|(left, _)| *left);
            for (key, value) in attrs {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                out.push_str(&value.replace('"', "&quot;"));
                out.push('"');
            }
        }
        out.push_str(" -->\n");
        out.push_str(&patch.content);
        if !patch.content.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("<!-- /patch:");
        out.push_str(&patch.name);
        out.push_str(" -->\n");
    }
    if !unmatched.trim().is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(unmatched.trim());
        out.push('\n');
    }
    out
}

pub fn response_materialization_probe(patches: &[PatchBlock], unmatched: &str) -> String {
    let mut selected = patches
        .iter()
        .filter(|patch| patch.name == "exchange")
        .cloned()
        .collect::<Vec<_>>();
    let selected_exchange = !selected.is_empty();
    if selected.is_empty() && unmatched.trim().is_empty() {
        selected = patches
            .iter()
            .filter(|patch| patch.name != "frontmatter")
            .filter(|patch| !agent_doc_element::element::is_backlog_component(&patch.name))
            .filter(|patch| !agent_doc_element::element::is_review_component(&patch.name))
            .cloned()
            .collect();
    }
    let probe_unmatched = if selected_exchange { "" } else { unmatched };
    materialized_template_response(&selected, probe_unmatched)
}

pub fn materialized_template_response(patches: &[PatchBlock], unmatched: &str) -> String {
    let mut out = String::new();
    for patch in patches {
        push_materialization_segment(&mut out, &patch.content);
    }
    push_materialization_segment(&mut out, unmatched);
    out
}

pub fn push_materialization_segment(out: &mut String, segment: &str) {
    let segment = segment.trim_matches(|c| c == '\n' || c == '\r');
    if segment.trim().is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(segment);
    out.push('\n');
}

pub fn reject_marker_response_with_zero_patches(
    marker_count: usize,
    patch_count: usize,
) -> Result<()> {
    if patch_count == 0 && marker_count > 0 {
        anyhow::bail!(
            "template patchback parsed zero patches despite {marker_count} patch marker(s); refusing to capture a malformed response"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_response_write_proof_rejects_empty_response_shells() {
        let patches = vec![
            PatchBlock::new("exchange", ""),
            PatchBlock::new("frontmatter", "agent: codex"),
        ];
        let err = ensure_template_response_write_proof(&patches, "").unwrap_err();
        assert!(err.to_string().contains("no real response-body write"));
    }

    #[test]
    fn template_response_write_proof_counts_real_response_body() {
        let patches = vec![
            PatchBlock::new("frontmatter", "agent: codex"),
            PatchBlock::new("backlog", "- [ ] follow-up"),
            PatchBlock::new("review", "- [ ] check"),
            PatchBlock::new("exchange", "### Re: item\n\nDone."),
        ];
        let proof = template_response_write_proof(&patches, "");
        assert_eq!(proof.explicit_components, vec!["exchange"]);
        assert!(proof.has_real_body());
    }

    #[test]
    fn strict_template_response_heading_accepts_exchange_patch_heading() {
        let patches = vec![PatchBlock::new(
            "exchange",
            "### Re: queue head - gpt-5\n\nAnswered.\n",
        )];

        ensure_strict_template_response_heading(&patches, "").unwrap();
    }

    #[test]
    fn strict_template_response_heading_accepts_unmatched_heading() {
        ensure_strict_template_response_heading(&[], "### Re: queue head - gpt-5\n\nAnswered.\n")
            .unwrap();
    }

    #[test]
    fn strict_template_response_heading_rejects_body_only_exchange_patch() {
        let patches = vec![PatchBlock::new(
            "exchange",
            "- changed paths\n- verification\n",
        )];

        let err = ensure_strict_template_response_heading(&patches, "").unwrap_err();

        assert!(
            err.to_string()
                .contains("strict template closeout response")
        );
    }

    #[test]
    fn strict_template_response_heading_accepts_streamed_visible_prefix() {
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #stream. spec-test-build-install-commit-push\n",
            "<!-- patch:exchange -->\n",
            "### Re: streamed - gpt-5\n",
            "<!-- agent:boundary:streamed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let patches = vec![PatchBlock::new("exchange", "\nImplemented and verified.\n")];

        ensure_strict_template_response_heading_for_current_doc(current, &patches, "").unwrap();
    }

    #[test]
    fn strict_template_response_heading_rejects_prior_heading_before_live_prompt() {
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n\n",
            "Done.\n",
            "❯ do #new. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let patches = vec![PatchBlock::new("exchange", "\nImplemented and verified.\n")];

        let err = ensure_strict_template_response_heading_for_current_doc(current, &patches, "")
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("strict template closeout response")
        );
    }

    #[test]
    fn materialization_probe_uses_patch_body_not_patch_markers() {
        let patches = vec![PatchBlock::new(
            "exchange",
            "### Re: materialized — gpt-5\n\nCommitted through boundary insertion.\n",
        )];

        let probe = response_materialization_probe(&patches, "");

        assert!(probe.contains("### Re: materialized — gpt-5"));
        assert!(!probe.contains("<!-- patch:exchange -->"));
        assert!(!probe.contains("<!-- /patch:exchange -->"));
    }

    #[test]
    fn materialization_probe_uses_non_tracked_patch_when_no_exchange_or_unmatched() {
        let patches = vec![
            PatchBlock::new("frontmatter", "agent: codex"),
            PatchBlock::new("backlog", "- [ ] follow-up"),
            PatchBlock::new("findings", "Shipped."),
        ];

        let probe = response_materialization_probe(&patches, "");

        assert_eq!(probe, "Shipped.\n");
    }

    #[test]
    fn serialize_template_response_preserves_sorted_attrs_and_unmatched_tail() {
        let mut patch = PatchBlock::new("exchange", "Body");
        patch
            .attrs
            .insert("z".to_string(), "\"quoted\"".to_string());
        patch.attrs.insert("a".to_string(), "first".to_string());

        let serialized = serialize_template_response(&[patch], "tail");

        assert!(
            serialized.contains("<!-- patch:exchange a=\"first\" z=\"&quot;quoted&quot;\" -->")
        );
        assert!(serialized.ends_with("tail\n"));
    }

    #[test]
    fn marker_bearing_zero_patch_parse_is_rejected_before_capture() {
        let err = reject_marker_response_with_zero_patches(1, 0).unwrap_err();

        assert!(
            err.to_string()
                .contains("parsed zero patches despite 1 patch marker")
        );
        assert!(reject_marker_response_with_zero_patches(0, 0).is_ok());
        assert!(reject_marker_response_with_zero_patches(2, 1).is_ok());
    }

    #[test]
    fn trailing_newline_comparison_is_stable() {
        assert!(same_ignoring_trailing_newlines("a\n", "a"));
        assert!(!same_ignoring_trailing_newlines("a\nb", "a"));
    }
}
