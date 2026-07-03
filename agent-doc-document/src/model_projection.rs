//! Pure model-projection policy for markdown overlay baselines.
//!
//! This module owns content-only decisions around the structured markdown
//! overlay model. It deliberately does not know about paths, locks, ops logs,
//! environment flags, or editor state storage.

use anyhow::{Context, Result};

/// Build the persisted structured-overlay state for markdown content.
pub fn overlay_state_from_markdown(content: &str) -> Vec<u8> {
    agent_doc_markdown_ast::crdt::OverlayCrdtDoc::from_markdown(content).encode_state()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayProjectionSource {
    Structured,
    RebuiltFromFallback { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayProjection {
    pub markdown: String,
    pub source: OverlayProjectionSource,
}

/// Project structured-overlay state bytes back to markdown.
///
/// `fallback_markdown` is passed through to legacy-state migration when callers
/// are decoding a sidecar that may have been written by an older runtime.
pub fn project_overlay_state(bytes: &[u8], fallback_markdown: Option<&str>) -> Result<String> {
    project_overlay_state_with_source(bytes, fallback_markdown)
        .map(|projection| projection.markdown)
}

pub fn project_overlay_state_with_source(
    bytes: &[u8],
    fallback_markdown: Option<&str>,
) -> Result<OverlayProjection> {
    match agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(bytes) {
        Ok(overlay) => {
            let markdown = overlay
                .to_markdown()
                .context("failed to project overlay state")?;
            Ok(OverlayProjection {
                markdown,
                source: OverlayProjectionSource::Structured,
            })
        }
        Err(err) => Ok(OverlayProjection {
            markdown: fallback_markdown.unwrap_or("").to_string(),
            source: OverlayProjectionSource::RebuiltFromFallback {
                error: err.to_string(),
            },
        }),
    }
}

/// Round-trip markdown through the same persist-reload-project pipeline used by
/// model-backed baselines.
pub fn project_overlay_roundtrip(content: &str) -> Result<String> {
    let state = overlay_state_from_markdown(content);
    project_overlay_state(&state, None)
}

/// Whether the structured overlay model projects `content` back to
/// byte-identical markdown.
pub fn overlay_projection_is_byte_stable(content: &str) -> bool {
    matches!(project_overlay_roundtrip(content), Ok(projected) if projected == content)
}

/// Byte offset of the first difference between `a` and `b`, or the shorter
/// length when one is a prefix of the other. Returns `None` when equal.
pub fn first_diff_byte(a: &str, b: &str) -> Option<usize> {
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .position(|(x, y)| x != y)
        .or(if a.len() == b.len() {
            None
        } else {
            Some(a.len().min(b.len()))
        })
}

/// Which markdown source should be used as the model-projected baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelBaselineSource {
    /// The structured overlay projection is authoritative.
    Model,
    /// The legacy markdown cache diverged, so it remains the non-regressing
    /// backstop while the cache still exists.
    MdBackstop,
}

/// Pure decision returned after cross-checking a model projection against the
/// legacy markdown baseline cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBaselineResolution {
    pub content: String,
    pub source: ModelBaselineSource,
    pub projected_len: usize,
    pub md_len: usize,
    pub diverged: bool,
    pub first_diff_byte: Option<usize>,
}

/// Decide the authoritative merge baseline from a model projection and optional
/// legacy markdown cache.
pub fn resolve_model_baseline_projection(
    projection: String,
    md_baseline: Option<&str>,
) -> ModelBaselineResolution {
    let projected_len = projection.len();
    let md_len = md_baseline.map(|m| m.len()).unwrap_or(0);

    match md_baseline {
        Some(md) if md != projection => ModelBaselineResolution {
            content: md.to_string(),
            source: ModelBaselineSource::MdBackstop,
            projected_len,
            md_len,
            diverged: true,
            first_diff_byte: first_diff_byte(md, &projection),
        },
        _ => ModelBaselineResolution {
            content: projection,
            source: ModelBaselineSource::Model,
            projected_len,
            md_len,
            diverged: false,
            first_diff_byte: None,
        },
    }
}

/// Whether an overlay projection carries content the cycle baseline lacks.
///
/// Line-multiset containment classifies additions and within-line edits as
/// unbaselined content. A pure deletion or shrink is only distinguishable from
/// a stale subset when the caller supplies an external pending-editor-op signal.
pub fn overlay_carries_unbaselined_content(
    overlay_md: &str,
    baseline: &str,
    has_pending_editor_ops: bool,
) -> bool {
    use std::collections::HashMap;

    let mut baseline_lines: HashMap<&str, usize> = HashMap::new();
    for line in baseline.lines() {
        *baseline_lines.entry(line).or_insert(0) += 1;
    }
    for line in overlay_md.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match baseline_lines.get_mut(line) {
            Some(count) if *count > 0 => *count -= 1,
            _ => return true,
        }
    }
    overlay_md != baseline && has_pending_editor_ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_projection_byte_stable(label: &str, content: &str) {
        let projected = project_overlay_roundtrip(content).unwrap();
        assert_eq!(
            projected, content,
            "#mps overlay projection not byte-stable for {label}"
        );
        assert!(overlay_projection_is_byte_stable(content), "{label}");
    }

    #[test]
    fn projection_byte_stable_inline_shape() {
        let inline = concat!(
            "---\nagent_doc_format: inline\n---\n\n",
            "## User\n\nDo the thing.\n\n",
            "## Assistant\n\nDid the thing.\n"
        );
        assert_projection_byte_stable("inline", inline);
    }

    #[test]
    fn projection_byte_stable_template_queue_shape() {
        let template = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Queue\n\n<!-- agent:queue -->\n",
            "- do [#a]\n- do [#b]\n",
            "<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("template_queue", template);
    }

    #[test]
    fn projection_byte_stable_exchange_append_shape() {
        let exchange = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} first question\n\n",
            "### Re: first question \u{2014} opus-4-8\n\n",
            "First answer here.\n\n",
            "\u{276f} second question\n",
            "<!-- /agent:exchange -->\n"
        );
        assert_projection_byte_stable("exchange_append", exchange);
    }

    #[test]
    fn projection_byte_stable_boundary_marker_shape() {
        let with_boundary = concat!(
            "---\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n<!-- agent:exchange patch=append -->\n",
            "\u{276f} q\n\n### Re: q \u{2014} opus-4-8\n\nA.\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        assert_projection_byte_stable("boundary_marker", with_boundary);
    }

    #[test]
    fn projection_byte_stable_empty_and_unicode() {
        assert_projection_byte_stable("empty", "");
        assert_projection_byte_stable("plain", "just a line of prose\n");
        assert_projection_byte_stable(
            "unicode",
            "# T\u{e9}l\u{e9} \u{1f680}\n\n- caf\u{e9} \u{2014} r\u{e9}sum\u{e9}\n",
        );
    }

    #[test]
    fn model_baseline_resolution_uses_model_when_cache_matches_or_absent() {
        let matched = resolve_model_baseline_projection("same\n".to_string(), Some("same\n"));
        assert_eq!(matched.source, ModelBaselineSource::Model);
        assert_eq!(matched.content, "same\n");
        assert!(!matched.diverged);
        assert_eq!(matched.first_diff_byte, None);

        let absent = resolve_model_baseline_projection("only\n".to_string(), None);
        assert_eq!(absent.source, ModelBaselineSource::Model);
        assert_eq!(absent.content, "only\n");
        assert_eq!(absent.md_len, 0);
    }

    #[test]
    fn model_baseline_resolution_prefers_md_backstop_on_divergence() {
        let resolved = resolve_model_baseline_projection("abx".to_string(), Some("abc"));
        assert_eq!(resolved.source, ModelBaselineSource::MdBackstop);
        assert_eq!(resolved.content, "abc");
        assert!(resolved.diverged);
        assert_eq!(resolved.projected_len, 3);
        assert_eq!(resolved.md_len, 3);
        assert_eq!(resolved.first_diff_byte, Some(2));
    }

    #[test]
    fn first_diff_byte_locates_mismatch() {
        assert_eq!(first_diff_byte("abc", "abc"), None);
        assert_eq!(first_diff_byte("abc", "abx"), Some(2));
        assert_eq!(first_diff_byte("abc", "ab"), Some(2));
        assert_eq!(first_diff_byte("ab", "abcd"), Some(2));
    }

    #[test]
    fn overlay_classifier_detects_added_or_edited_lines() {
        let baseline = "a\nb\n";
        assert!(overlay_carries_unbaselined_content(
            "a\nb\nc\n",
            baseline,
            false
        ));
        assert!(overlay_carries_unbaselined_content(
            "a\nB\n", baseline, false
        ));
    }

    #[test]
    fn overlay_classifier_uses_pending_ops_for_subset_deletions() {
        let baseline = "a\nb\n";
        let subset = "a\n";
        assert!(!overlay_carries_unbaselined_content(
            subset, baseline, false
        ));
        assert!(overlay_carries_unbaselined_content(subset, baseline, true));
    }

    #[test]
    fn overlay_classifier_handles_duplicate_lines_as_multiset() {
        let baseline = "a\na\nb\n";
        assert!(!overlay_carries_unbaselined_content(
            "a\nb\n", baseline, false
        ));
        assert!(overlay_carries_unbaselined_content(
            "a\na\na\nb\n",
            baseline,
            false
        ));
    }
}
