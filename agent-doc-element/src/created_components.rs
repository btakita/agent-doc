//! Which managed components a write creates (`#ghrecvpolicy`, GH #40).
//!
//! The recipient-side `[receive] auto_create_components` policy governs a
//! sender creating a component that the document lacks. Every scaffold the
//! binary adds (`agent:run` from route dispatch, `agent:review` for a prompt
//! blocker, `agent:done`, `agent:todo`, `agent:recap`, ...) shows up as a
//! component name present after a write and absent before it, so the I/O
//! adapters compare the two cuts here instead of teaching every pure scaffold
//! helper about principals.

use std::collections::BTreeSet;

/// Component names (as the component parser names them, so `run` is `queue`)
/// that `after` has and
/// `before` lacks. Empty for the overwhelmingly common write that only edits
/// existing components.
///
/// A cheap marker scan runs first, so a write that introduces no new
/// `<!-- agent:NAME` opener never parses either document. Candidates are then
/// confirmed with the component parser, which ignores markers quoted inside
/// code fences or inline code.
pub fn created_component_names(before: &str, after: &str) -> Vec<String> {
    let scanned_after = scanned_opener_names(after);
    if scanned_after.is_empty() {
        return Vec::new();
    }
    let scanned_before = scanned_opener_names(before);
    if scanned_after.is_subset(&scanned_before) {
        return Vec::new();
    }
    let parsed = |doc: &str| -> Option<BTreeSet<String>> {
        crate::element::parse(doc)
            .ok()
            .map(|components| components.into_iter().map(|c| c.name).collect())
    };
    // An unparseable `before` cannot prove a component absent; fall back to
    // its scanned markers so a broken document never reports every component
    // as new.
    let before_names = parsed(before).unwrap_or_else(|| {
        scanned_before
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    });
    parsed(after)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !before_names.contains(name))
        .collect()
}

/// Names after every `<!-- agent:` opener, up to the first character that
/// cannot be part of a component name.
fn scanned_opener_names(doc: &str) -> BTreeSet<&str> {
    const OPENER: &str = "<!-- agent:";
    doc.match_indices(OPENER)
        .filter_map(|(at, _)| {
            let rest = &doc[at + OPENER.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(rest.len());
            (end > 0).then(|| &rest[..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "# Doc\n\n<!-- agent:exchange -->\nhello\n<!-- /agent:exchange -->\n";

    #[test]
    fn editing_existing_components_creates_nothing() {
        let after = BASE.replace("hello", "hello again");
        assert!(created_component_names(BASE, &after).is_empty());
        assert!(created_component_names(BASE, BASE).is_empty());
        assert!(created_component_names("plain\n", "plain text\n").is_empty());
    }

    #[test]
    fn a_new_component_is_reported_by_its_marker_name() {
        let after = format!("{BASE}\n<!-- agent:run go -->\n- do it\n<!-- /agent:run -->\n");
        assert_eq!(
            created_component_names(BASE, &after),
            vec!["queue".to_string()]
        );
        let after = format!("{BASE}\n<!-- agent:review -->\n<!-- /agent:review -->\n");
        assert_eq!(
            created_component_names(BASE, &after),
            vec!["review".to_string()]
        );
    }

    #[test]
    fn a_marker_quoted_in_code_is_not_a_created_component() {
        let after = BASE.replace(
            "hello",
            "hello\n\n```md\n<!-- agent:review -->\n<!-- /agent:review -->\n```",
        );
        assert!(created_component_names(BASE, &after).is_empty());
    }

    #[test]
    fn removing_a_component_creates_nothing() {
        let before = format!("{BASE}\n<!-- agent:todo -->\n<!-- /agent:todo -->\n");
        assert!(created_component_names(&before, BASE).is_empty());
    }
}
