//! Durable addressed-at annotations for queue rows.
//!
//! The annotation is durable markdown truth. Editors may rank it, but the
//! rank and highlight are projections and never enter queue identity.

use std::collections::HashMap;

use agent_doc_markdown_ast::{mutations::item_nodes_all, overlay::STRUCK_ANNOTATION_SEPARATOR};
use anyhow::Result;

pub const ADDRESSED_AT_ANNOTATION_PREFIX: &str = "⟦addressed-at:";
pub const ADDRESSED_AT_ANNOTATION_SUFFIX: &str = "⟧";

pub fn addressed_at_annotation(timestamp: &str) -> String {
    format!("{ADDRESSED_AT_ANNOTATION_PREFIX}{timestamp}{ADDRESSED_AT_ANNOTATION_SUFFIX}")
}

/// Add one durable timestamp to every queue row that changed from live in
/// `before` to struck in `after`. Existing rows and annotations are untouched.
///
/// Node keys are stable across a strike, including free-text rows whose
/// visible strike explanation lives outside the `~~...~~` wrapper. Replacing
/// from the end keeps every AST byte span valid while preserving the original
/// row bytes and any nested child block.
pub fn annotate_newly_struck_rows(before: &str, after: &str, timestamp: &str) -> Result<String> {
    if timestamp.trim().is_empty() || before == after {
        return Ok(after.to_string());
    }
    let before_nodes = item_nodes_all(before, "queue")?;
    let before_by_key: HashMap<_, _> = before_nodes
        .into_iter()
        .map(|node| (node.node_key, node.item.struck))
        .collect();
    let mut replacements = item_nodes_all(after, "queue")?
        .into_iter()
        .filter(|node| {
            let source = &after[node.item.start_byte..node.item.end_byte];
            node.item.struck
                && source.trim_start().starts_with("- ")
                && before_by_key.get(&node.node_key) == Some(&false)
                && !source.contains(ADDRESSED_AT_ANNOTATION_PREFIX)
        })
        .map(|node| {
            let replacement =
                annotate_source_row(&after[node.item.start_byte..node.item.end_byte], timestamp);
            (node.item.start_byte, node.item.end_byte, replacement)
        })
        .collect::<Vec<_>>();
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));

    let mut updated = after.to_string();
    for (start, end, replacement) in replacements {
        updated.replace_range(start..end, &replacement);
    }
    Ok(updated)
}

fn annotate_source_row(source: &str, timestamp: &str) -> String {
    let (first, rest) = source
        .split_once('\n')
        .map_or((source, None), |(first, rest)| (first, Some(rest)));
    let token = addressed_at_annotation(timestamp);
    let annotated_first = if first.contains(STRUCK_ANNOTATION_SEPARATOR) {
        format!("{} {token}", first.trim_end())
    } else {
        format!(
            "{}{}addressed this cycle {token}",
            first.trim_end(),
            STRUCK_ANNOTATION_SEPARATOR,
        )
    };
    match rest {
        Some(rest) => format!("{annotated_first}\n{rest}"),
        None => annotated_first,
    }
}

#[cfg(test)]
mod tests {
    use super::{ADDRESSED_AT_ANNOTATION_PREFIX, annotate_newly_struck_rows};

    #[test]
    fn annotates_only_rows_newly_struck_in_this_projection() {
        let before = "<!-- agent:run -->\n- live report\n- ~~older~~ — auto-struck: addressed this cycle ⟦addressed-at:2026-10-28T12:00:00.000Z⟧\n- do [#ship]\n<!-- /agent:run -->\n";
        let after = "<!-- agent:run -->\n- ~~live report~~ — auto-struck: answered this cycle (#ftstrike)\n- ~~older~~ — auto-struck: addressed this cycle ⟦addressed-at:2026-10-28T12:00:00.000Z⟧\n- do [#ship]\n<!-- /agent:run -->\n";

        let updated =
            annotate_newly_struck_rows(before, after, "2026-10-29T15:49:00.000Z").unwrap();

        assert!(updated.contains(
            "- ~~live report~~ — auto-struck: answered this cycle (#ftstrike) ⟦addressed-at:2026-10-29T15:49:00.000Z⟧"
        ), "{updated}");
        assert_eq!(updated.matches(ADDRESSED_AT_ANNOTATION_PREFIX).count(), 2);
        assert!(updated.contains("- do [#ship]\n"));
    }

    #[test]
    fn id_backed_and_nested_rows_keep_their_bytes_and_children() {
        let before =
            "<!-- agent:run -->\n- do [#ship]\n  - publish\n- sibling\n<!-- /agent:run -->\n";
        let after = "<!-- agent:run -->\n- ~~do [#ship]~~\n  - ~~publish~~\n- sibling\n<!-- /agent:run -->\n";

        let updated =
            annotate_newly_struck_rows(before, after, "2026-10-29T15:50:00.000Z").unwrap();

        assert!(updated.contains(
            "- ~~do [#ship]~~ — auto-struck: addressed this cycle ⟦addressed-at:2026-10-29T15:50:00.000Z⟧\n  - ~~publish~~"
        ), "{updated}");
        assert!(updated.contains("- sibling\n"));
    }

    #[test]
    fn rerunning_is_idempotent() {
        let before = "<!-- agent:run -->\n- work\n- pending\n<!-- /agent:run -->\n";
        let after = "<!-- agent:run -->\n- ~~work~~\n- pending\n<!-- /agent:run -->\n";
        let once = annotate_newly_struck_rows(before, after, "2026-10-29T15:51:00.000Z").unwrap();
        let twice = annotate_newly_struck_rows(before, &once, "2026-10-29T15:52:00.000Z").unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn multiline_done_fences_are_not_annotated() {
        let before = "<!-- agent:run -->\n---\nmultiline\n---\n- pending\n<!-- /agent:run -->\n";
        let after = "<!-- agent:run -->\n~~~done\nmultiline\n~~~\n- pending\n<!-- /agent:run -->\n";
        let updated =
            annotate_newly_struck_rows(before, after, "2026-10-29T15:53:00.000Z").unwrap();
        assert_eq!(updated, after);
    }
}
