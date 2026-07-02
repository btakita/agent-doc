//! Commit-seam document-integrity guard.
//!
//! Operator-visible document text is authoritative: a commit must never
//! *regress* frontmatter the operator kept. A legitimate pending prompt makes
//! the committed snapshot's **body** smaller than the live working-tree file,
//! but it never drops top-level frontmatter keys -- only a corrupt snapshot does
//! (a stale-base CRDT merge, or a `no_liveness_signals` synthetic auto-reap that
//! serialized a scaffold/empty base). Persisting such a snapshot poisons `HEAD`;
//! preflight's commit step then collapses the snapshot back to the corrupt
//! `HEAD` every cycle, so `doc != snapshot` never converges and the supervisor
//! spins the cycle (`suprecyclespin` `cycle_never_closed`).
//!
//! This guard detects the *provable* regression and lets the commit path
//! self-heal (overlay the authoritative live frontmatter, regenerate the
//! snapshot in the background) instead of persisting the corruption. See
//! `#boundaryaccum` / stale-CRDT recovery notes and the "operator-visible text
//! is authoritative ... snapshots are backup, not hot-path authority" contract
//! in `AGENTS.md`.

use std::collections::BTreeSet;

/// Top-level YAML frontmatter keys in `content`.
///
/// Returns an empty set when there is no frontmatter block or the block does
/// not parse as a YAML mapping -- the guard only fires on a *provable* key drop,
/// so "can't tell" degrades to "no keys" (no false positive).
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

/// Frontmatter keys that `to_commit` would drop, restricted to keys present in
/// **both** the prior committed `head` and the authoritative `live_file`.
///
/// Restricting to `head`/`live_file` intersection keeps legitimate edits from tripping the
/// guard:
/// - operator *adds* a key (in `live_file`, not yet in `head`/snapshot) -> not in
///   the intersection -> ignored;
/// - operator *removes* a key (gone from `live_file`) -> not in the intersection
///   -> ignored.
///
/// A non-empty result means the snapshot lost a key the operator still has and
/// that was already committed -- a corrupt drop, never a normal edit. Result is
/// sorted (`BTreeSet` order) for stable diagnostics.
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

/// Rebuild `to_commit` with its frontmatter block replaced by the authoritative
/// frontmatter from `live_file`, preserving `to_commit`'s body verbatim.
///
/// Frontmatter is config, never selectively committed, so it is always taken
/// from the operator-authoritative live document; the body (components /
/// exchange) stays sourced from `to_commit` so selective response staging is
/// unaffected. This lets a corrupt-frontmatter snapshot self-heal at the commit
/// seam instead of poisoning HEAD or failing closed.
///
/// If `live_file` has no frontmatter there is nothing authoritative to apply, so
/// `to_commit` is returned unchanged.
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
