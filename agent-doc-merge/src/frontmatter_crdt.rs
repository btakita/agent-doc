//! Frontmatter-aware CRDT merge policy.
//!
//! This is the pure merge wrapper used by orchestration write paths and
//! realtime editor broadcast planning. It reconciles leading YAML frontmatter
//! field-wise, then delegates the body to the per-component CRDT merge.

use anyhow::{Context, Result};

/// CRDT-based merge: conflict-free merge using Yrs CRDT.
///
/// Returns `(merged_text, new_crdt_state)`. `base_state` is the CRDT state from
/// the last write, or `None` on first use. The returned state encodes the full
/// merged text and can be used as the base state for the next cycle.
pub fn merge_contents_crdt(
    base_state: Option<&[u8]>,
    ours: &str,
    theirs: &str,
) -> Result<(String, Vec<u8>)> {
    let merged = merge_frontmatter_aware(base_state, ours, theirs)?;
    let doc = crate::crdt::CrdtDoc::from_text(&merged);
    let state = doc.encode_state();
    Ok((merged, state))
}

/// Frontmatter-aware per-component CRDT merge (`#fmreset`).
///
/// Wraps [`crate::crdt::merge_by_component`] so the leading YAML frontmatter
/// region is reconciled field-wise instead of as a text blob. The field-wise
/// merge keeps operator edits to operator-owned keys, keeps agent managed-key
/// writes, and merges concurrent operator-edit plus agent-write key-by-key.
fn merge_frontmatter_aware(base_state: Option<&[u8]>, ours: &str, theirs: &str) -> Result<String> {
    use agent_doc_frontmatter::frontmatter as fm;

    let (ours_fm, ours_body) = fm::split_frontmatter_parts(ours);
    let (theirs_fm, theirs_body) = fm::split_frontmatter_parts(theirs);

    if ours_fm == theirs_fm {
        return crate::crdt::merge_by_component(base_state, ours, theirs)
            .context("CRDT merge failed");
    }

    let base_text = match base_state {
        Some(bytes) => crate::crdt::CrdtDoc::decode_state(bytes)
            .map(|d| d.to_text())
            .ok(),
        None => None,
    };
    let (base_fm, base_body) = match base_text.as_deref() {
        Some(t) => {
            let (f, b) = fm::split_frontmatter_parts(t);
            (f.map(str::to_string), Some(b.to_string()))
        }
        None => (None, None),
    };

    let Some(merged_fm) = fm::merge_frontmatter_3way(base_fm.as_deref(), ours_fm, theirs_fm) else {
        return crate::crdt::merge_by_component(base_state, ours, theirs)
            .context("CRDT merge failed");
    };

    let body_base_state = base_body
        .as_deref()
        .map(|b| crate::crdt::CrdtDoc::from_text(b).encode_state());
    let merged_body =
        crate::crdt::merge_by_component(body_base_state.as_deref(), ours_body, theirs_body)
            .context("CRDT merge failed (body)")?;

    Ok(format!("{merged_fm}{merged_body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_contents_crdt_merges_frontmatter_fieldwise() {
        let base = concat!(
            "---\n",
            "agent_doc_session: base\n",
            "queue: old\n",
            "---\n",
            "\n",
            "<!-- agent:exchange -->\n",
            "base\n",
            "<!-- /agent:exchange -->\n",
        );
        let ours = base.replace("agent_doc_session: base\n", "agent_doc_session: agent\n");
        let theirs = base.replace("queue: old\n", "queue: operator\n");
        let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();

        let (merged, state) = merge_contents_crdt(Some(&base_state), &ours, &theirs).unwrap();

        assert!(merged.contains("agent_doc_session: agent"));
        assert!(merged.contains("queue: operator"));
        assert!(!state.is_empty());
    }

    #[test]
    fn merge_contents_crdt_state_prevents_duplicate_user_edits_next_cycle() {
        let base = "base\n";
        let ours1 = "base\nagent one\n";
        let theirs1 = "base\nuser one\n";
        let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged1, state1) = merge_contents_crdt(Some(&base_state), ours1, theirs1).unwrap();
        assert!(merged1.contains("agent one"));
        assert!(merged1.contains("user one"));

        let ours2 = format!("{merged1}agent two\n");
        let theirs2 = merged1.clone();
        let (merged2, _state2) = merge_contents_crdt(Some(&state1), &ours2, &theirs2).unwrap();

        assert_eq!(merged2.matches("user one").count(), 1);
        assert!(merged2.contains("agent two"));
    }
}
