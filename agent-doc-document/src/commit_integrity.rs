//! Commit-seam document-integrity guard.

use std::collections::BTreeSet;

fn frontmatter_keys(content: &str) -> BTreeSet<String> {
    let Some(yaml) = agent_doc_frontmatter::raw_frontmatter_yaml(content) else {
        return BTreeSet::new();
    };
    match serde_yaml::from_str::<serde_yaml::Value>(yaml) {
        Ok(serde_yaml::Value::Mapping(map)) => map
            .keys()
            .filter_map(|key| key.as_str().map(str::to_string))
            .collect(),
        _ => BTreeSet::new(),
    }
}

pub fn dropped_committed_frontmatter_keys(
    to_commit: &str,
    head: &str,
    live_file: &str,
) -> Vec<String> {
    let committed = frontmatter_keys(to_commit);
    let head_keys = frontmatter_keys(head);
    let live_keys = frontmatter_keys(live_file);
    head_keys
        .intersection(&live_keys)
        .filter(|key| !committed.contains(*key))
        .cloned()
        .collect()
}

pub fn overlay_live_frontmatter(to_commit: &str, live_file: &str) -> String {
    let (live_fm, _) = agent_doc_frontmatter::split_frontmatter_parts(live_file);
    let Some(live_yaml) = live_fm else {
        return to_commit.to_string();
    };
    let (_, commit_body) = agent_doc_frontmatter::split_frontmatter_parts(to_commit);
    format!("---\n{live_yaml}\n---\n{commit_body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_FM: &str = "---\nagent_doc_session: s\nagent: claude\nclaude_model: opus\nagent_doc_format: template\nagent_doc_write: crdt\nclaude_args: --x\npending_done_guard: warn\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
    const STRIPPED_FM: &str = "---\nagent_doc_session: s\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n\n<!-- agent:boundary:cfc1efac -->\n<!-- /agent:exchange -->\n";

    #[test]
    fn flags_synthetic_reap_frontmatter_drop() {
        let dropped = dropped_committed_frontmatter_keys(STRIPPED_FM, FULL_FM, FULL_FM);
        assert_eq!(
            dropped,
            vec![
                "agent".to_string(),
                "claude_args".to_string(),
                "claude_model".to_string(),
                "pending_done_guard".to_string(),
            ]
        );
    }

    #[test]
    fn allows_pending_prompt_without_frontmatter_change() {
        let live_with_prompt = "---\nagent_doc_session: s\nagent: claude\nclaude_model: opus\nagent_doc_format: template\nagent_doc_write: crdt\nclaude_args: --x\npending_done_guard: warn\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nAnswer this please.\n<!-- /agent:exchange -->\n";
        assert!(dropped_committed_frontmatter_keys(FULL_FM, FULL_FM, live_with_prompt).is_empty());
    }

    #[test]
    fn ignores_operator_added_key() {
        let live_added = "---\nagent_doc_session: s\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nno_mcp: true\n---\n";
        let snapshot = "---\nagent_doc_session: s\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\n---\n";
        assert!(dropped_committed_frontmatter_keys(snapshot, snapshot, live_added).is_empty());
    }

    #[test]
    fn ignores_operator_removed_key() {
        let live_removed = "---\nagent_doc_session: s\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\n---\n";
        let head = "---\nagent_doc_session: s\nagent: claude\nagent_doc_format: template\nagent_doc_write: crdt\nclaude_args: --x\n---\n";
        assert!(dropped_committed_frontmatter_keys(live_removed, head, live_removed).is_empty());
    }

    #[test]
    fn overlay_restores_dropped_frontmatter_preserving_body() {
        let corrected = overlay_live_frontmatter(STRIPPED_FM, FULL_FM);
        assert!(dropped_committed_frontmatter_keys(&corrected, FULL_FM, FULL_FM).is_empty());
        assert!(corrected.contains("<!-- agent:boundary:cfc1efac -->"));
        assert!(corrected.starts_with("---\nagent_doc_session: s\n"));
        assert_eq!(corrected.matches("\n---\n").count(), 1);
    }

    #[test]
    fn overlay_noop_when_live_has_no_frontmatter() {
        let body_only = "## Exchange\n\ncontent\n";
        assert_eq!(
            overlay_live_frontmatter(STRIPPED_FM, body_only),
            STRIPPED_FM
        );
    }

    #[test]
    fn no_frontmatter_anywhere_is_safe() {
        assert!(dropped_committed_frontmatter_keys("# hi\n", "# hi\n", "# hi\n").is_empty());
    }

    #[test]
    fn untracked_first_commit_has_no_head_regression() {
        assert!(dropped_committed_frontmatter_keys(STRIPPED_FM, "", FULL_FM).is_empty());
    }
}
