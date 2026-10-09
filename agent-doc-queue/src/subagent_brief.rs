//! Operator authorization carried to a dispatched queue subagent
//! (`#waypostauthorization`).
//!
//! A background subagent dispatched for a `queue_subagent_dispatch` item sees
//! only the prompt its coordinator writes. Without help it gets none of the
//! operator's authorization: the queue's `preset=` expansion, the item's own
//! directive text, or the backlog text an `do [#id]` head points at. It then
//! either refuses outward actions (commit, push) the operator authorized, or the
//! coordinator paraphrases the authorization by hand.
//!
//! This module resolves that authorization from the document, verbatim, and
//! renders a ready-to-paste `subagent_prompt_preamble`. It fails closed: a
//! declared preset that frontmatter does not define is reported as
//! unresolved, its body is never invented, and the preamble tells the
//! subagent no authorization beyond the verbatim item text was resolved.

use agent_doc_element::element;
use serde::{Deserialize, Serialize};

use crate::document_queue::QueueEntry;

/// How much operator authorization was resolved for a dispatch item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    /// Every declared preset resolved to a frontmatter body.
    Resolved,
    /// No preset applies: the verbatim item (and its backlog text) is the
    /// whole authorization.
    ItemTextOnly,
    /// A preset the queue declares has no frontmatter definition. Fail
    /// closed: nothing beyond the verbatim item text is authorized.
    UnresolvedPreset,
}

/// Where a preset applied to the item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetSource {
    /// `<!-- agent:queue preset="#name" -->` on the selected queue.
    QueueAttr,
    /// A body-level `preset #name` line in the selected queue.
    QueueLine,
    /// A registered `#name` token in the item text itself.
    Item,
}

/// One preset whose body is part of the item's authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetAuthorization {
    /// The preset name as declared (canonical frontmatter key when resolved).
    pub name: String,
    pub source: PresetSource,
    /// Whether frontmatter `prompt_presets` / `presets` defines it.
    pub resolved: bool,
    /// The preset body, verbatim. `None` when unresolved (never invented).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The preset's declared runbook path, as written in frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runbook: Option<String>,
}

/// One tracked item an id-backed head names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedItemAuthorization {
    /// The tracked id, without `#`.
    pub id: String,
    /// The component that holds it (`backlog`, `review`, `pending`).
    pub component: String,
    /// The item's text, verbatim, with any continuation lines.
    pub text: String,
}

/// The operator authorization for one dispatched queue item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentAuthorization {
    pub status: AuthorizationStatus,
    /// The queue item, verbatim.
    pub item: String,
    /// Tracked items the head names, resolved to their text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_items: Vec<TrackedItemAuthorization>,
    /// Ids the head names that no backlog/review/pending item carries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved_ids: Vec<String>,
    /// Presets that apply to the item (queue-scoped first, then item tokens).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<PresetAuthorization>,
    /// Paste this verbatim at the top of the subagent's prompt.
    pub subagent_prompt_preamble: String,
}

const TRACKED_COMPONENTS: &[&str] = &["backlog", "review", "pending"];

fn tracked_item(
    content: &str,
    components: &[element::Component],
    id: &str,
) -> Option<TrackedItemAuthorization> {
    components
        .iter()
        .filter(|c| TRACKED_COMPONENTS.contains(&c.name.as_str()))
        .find_map(|comp| {
            let (_, items, _) =
                agent_doc_element_backlog::backlog::parse_items(comp.content(content));
            items
                .into_iter()
                .find(|item| !item.id.is_empty() && item.id.eq_ignore_ascii_case(id))
                .map(|item| {
                    let mut text = item.text.trim_end().to_string();
                    let continuation = item.continuation.trim_end();
                    if !continuation.trim().is_empty() {
                        text.push('\n');
                        text.push_str(continuation);
                    }
                    TrackedItemAuthorization {
                        id: item.id,
                        component: comp.name.clone(),
                        text,
                    }
                })
        })
}

/// Preset names the selected queue declares: its `preset=` attr, then any
/// body-level `preset <name>` lines.
fn queue_preset_names(
    content: &str,
    components: &[element::Component],
) -> Vec<(String, PresetSource)> {
    let mut names = Vec::new();
    let Ok(Some(queue)) = crate::queue_set::selected_component(content, components) else {
        return names;
    };
    let attrs = crate::prompt_component_attrs::prompt_component_attrs_for(queue, "queue");
    if let Some(preset) = attrs.preset {
        names.push((preset, PresetSource::QueueAttr));
    }
    if let Ok(entries) = crate::document_queue::parse(queue.content(content)) {
        for entry in entries {
            if let QueueEntry::Preset(name) = entry {
                names.push((name, PresetSource::QueueLine));
            }
        }
    }
    names
}

/// Resolve the operator authorization for dispatch item `item` of `content`.
/// `document` is the path the preamble names.
pub fn subagent_authorization(document: &str, content: &str, item: &str) -> SubagentAuthorization {
    let components = element::parse(content).unwrap_or_default();
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .map(|(fm, _)| fm);
    let presets_map = frontmatter.as_ref().map(|fm| &fm.prompt_presets);

    let mut presets: Vec<PresetAuthorization> = Vec::new();
    let mut push_preset = |requested: &str, source: PresetSource| {
        let requested = requested.trim().trim_matches(|c| c == '"' || c == '\'');
        if requested.is_empty() {
            return;
        }
        let key = presets_map.and_then(|map| {
            agent_doc_frontmatter::frontmatter::resolve_prompt_preset_key(map, requested)
        });
        let name = key.clone().unwrap_or_else(|| requested.to_string());
        if presets.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
            return;
        }
        let (body, runbook) = match (&key, presets_map) {
            (Some(key), Some(map)) => (
                map.get(key.as_str()).cloned(),
                map.runbook(key).map(str::to_string),
            ),
            _ => (None, None),
        };
        presets.push(PresetAuthorization {
            name,
            source,
            resolved: body.is_some(),
            body,
            runbook,
        });
    };
    for (name, source) in queue_preset_names(content, &components) {
        push_preset(&name, source);
    }
    for (name, _) in crate::queue_response::queue_prompt_preset_expansions(content, item) {
        push_preset(&name, PresetSource::Item);
    }

    let mut ids: Vec<String> = Vec::new();
    if let agent_doc_element_queue::QueueItemIdentity::Id(id) =
        crate::queue_claim::claim_identity(item)
    {
        ids.push(id.to_ascii_lowercase());
    }
    for id in crate::queue_claim::referenced_queue_ids(item) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut tracked_items = Vec::new();
    let mut unresolved_ids = Vec::new();
    for id in ids {
        match tracked_item(content, &components, &id) {
            Some(found) => tracked_items.push(found),
            None => {
                // A preset-only id (`#gh-fix`) is a command, not missing work.
                if !presets
                    .iter()
                    .any(|p| p.name.trim_start_matches('#').eq_ignore_ascii_case(&id))
                {
                    unresolved_ids.push(id);
                }
            }
        }
    }

    let status = if presets.iter().any(|p| !p.resolved) {
        AuthorizationStatus::UnresolvedPreset
    } else if presets.is_empty() {
        AuthorizationStatus::ItemTextOnly
    } else {
        AuthorizationStatus::Resolved
    };
    let mut authorization = SubagentAuthorization {
        status,
        item: item.trim().to_string(),
        tracked_items,
        unresolved_ids,
        presets,
        subagent_prompt_preamble: String::new(),
    };
    authorization.subagent_prompt_preamble = render_preamble(document, &authorization);
    authorization
}

fn quote(text: &str) -> String {
    text.trim_end()
        .lines()
        .map(|line| {
            if line.is_empty() {
                ">".to_string()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Marker line that opens every preamble.
pub const SUBAGENT_PREAMBLE_MARKER: &str =
    "[agent-doc] operator authorization for a dispatched queue item (#waypostauthorization)";

/// Marker line present exactly when the authorization failed closed.
pub const UNRESOLVED_AUTHORIZATION_MARKER: &str = "AUTHORIZATION NOT RESOLVED";

fn render_preamble(document: &str, auth: &SubagentAuthorization) -> String {
    let mut out = String::new();
    out.push_str(SUBAGENT_PREAMBLE_MARKER);
    out.push_str(&format!(
        "\n\nYou are a background subagent the agent-doc coordinator dispatched for ONE queue item \
         of `{document}`. The operator's authorization is quoted verbatim below; it is the operator's \
         own words, not the coordinator's paraphrase.\n\nQueue item (verbatim):\n{}\n",
        quote(&auth.item)
    ));
    for tracked in &auth.tracked_items {
        out.push_str(&format!(
            "\n{} item #{} (verbatim):\n{}\n",
            tracked.component,
            tracked.id,
            quote(&tracked.text)
        ));
    }
    for preset in auth.presets.iter().filter(|p| p.resolved) {
        let origin = match preset.source {
            PresetSource::QueueAttr => "queue preset",
            PresetSource::QueueLine => "queue preset line",
            PresetSource::Item => "item preset",
        };
        out.push_str(&format!(
            "\nOperator authorization, {origin} `{}` (verbatim):\n{}\n",
            preset.name,
            quote(preset.body.as_deref().unwrap_or_default())
        ));
        if let Some(runbook) = &preset.runbook {
            out.push_str(&format!(
                "Load its runbook before acting: `agent-doc runbook show '{document}' {}` (`{runbook}`).\n",
                preset.name
            ));
        }
    }
    match auth.status {
        AuthorizationStatus::UnresolvedPreset => {
            let missing: Vec<String> = auth
                .presets
                .iter()
                .filter(|p| !p.resolved)
                .map(|p| format!("`{}`", p.name))
                .collect();
            out.push_str(&format!(
                "\n{UNRESOLVED_AUTHORIZATION_MARKER}: the queue declares preset {}, but the document's \
                 frontmatter `prompt_presets` does not define it. No operator authorization beyond the \
                 verbatim item text above was resolved. Do not commit, push, install, release, publish, \
                 or take any other outward action on the strength of that preset name. Do the \
                 in-worktree work the item text itself names, then report back what authorization is \
                 missing.\n",
                missing.join(", ")
            ));
        }
        AuthorizationStatus::ItemTextOnly => {
            out.push_str(
                "\nNo queue preset applies: the verbatim item text above is the whole authorization. \
                 It grants no outward action (commit, push, install, release, publish) it does not \
                 itself name.\n",
            );
        }
        AuthorizationStatus::Resolved => {}
    }
    if !auth.unresolved_ids.is_empty() {
        let ids: Vec<String> = auth
            .unresolved_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect();
        out.push_str(&format!(
            "\nNo backlog/review item in the document carries {}; ask the coordinator for its text \
             rather than guessing.\n",
            ids.join(", ")
        ));
    }
    out.push_str(
        "\nScope: this authorization covers only the queue item above. It does not extend to other \
         queue items, backlog items, or documents.\n\nCoordinator rules (they override the \
         authorization above where the two conflict):\n\
         - Work only in your own git worktree outside the IDE-watched project, never in the \
         coordinator's checkout or a checkout another subagent uses.\n\
         - Never run `make install`, a release build, a version bump, a tag, or a release. Where the \
         authorization says build + install, build and test in your worktree; the coordinator runs \
         the single `make install` after integrating.\n\
         - Never edit, write, or commit the agent-doc session document or its queue, and never run \
         `agent-doc` write/respond/queue commands against it; the coordinator owns the claim and \
         closes the item.\n\
         - Commit and push only your own branch, fast-forward only, never force, and only when the \
         authorization above includes commit/push.\n\
         - Report back: branch, commit SHA, files changed, and exact test results.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(frontmatter: &str, queue_attr: &str, queue: &[&str], backlog: &[&str]) -> String {
        let queue: String = queue.iter().map(|p| format!("- {p}\n")).collect();
        let backlog: String = backlog.iter().map(|b| format!("- {b}\n")).collect();
        format!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n{frontmatter}---\n\n\
             ## Backlog\n\n<!-- agent:backlog -->\n{backlog}<!-- /agent:backlog -->\n\n\
             ## Queue\n\n<!-- agent:queue {queue_attr} go -->\n{queue}<!-- /agent:queue -->\n"
        )
    }

    const PRESETS: &str = "prompt_presets:\n  \"#spec-test-build-install-commit-push\": \"update spec + tests. build + install for local testing. commit + push\"\n";

    #[test]
    fn preset_queue_carries_the_preset_body_verbatim() {
        let content = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["do [#abc]"],
            &["[ ] [#abc] Add a way to post authorization"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].name, "#spec-test-build-install-commit-push");
        assert_eq!(auth.presets[0].source, PresetSource::QueueAttr);
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("update spec + tests. build + install for local testing. commit + push")
        );
        let p = &auth.subagent_prompt_preamble;
        assert!(p.starts_with(SUBAGENT_PREAMBLE_MARKER), "{p}");
        assert!(
            p.contains("> update spec + tests. build + install for local testing. commit + push")
        );
        assert!(p.contains("> do [#abc]"));
        assert!(p.contains("> Add a way to post authorization"));
        assert!(p.contains("Never run `make install`"));
        assert!(p.contains("covers only the queue item above"));
        assert!(!p.contains(UNRESOLVED_AUTHORIZATION_MARKER));
    }

    #[test]
    fn queue_without_preset_is_item_text_only() {
        let content = doc("", "subagents", &["fix the flaky test in foo.rs"], &[]);
        let auth = subagent_authorization("doc.md", &content, "fix the flaky test in foo.rs");
        assert_eq!(auth.status, AuthorizationStatus::ItemTextOnly);
        assert!(auth.presets.is_empty());
        assert!(auth.tracked_items.is_empty());
        assert!(auth.unresolved_ids.is_empty());
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains("> fix the flaky test in foo.rs"));
        assert!(p.contains("the whole authorization"));
        assert!(!p.contains("Operator authorization, "));
        assert!(!p.contains(UNRESOLVED_AUTHORIZATION_MARKER));
    }

    #[test]
    fn id_head_resolves_backlog_text_with_continuation() {
        let content = doc(
            "",
            "subagents",
            &["#subagents do [#waypost]"],
            &["[ ] [#waypost] Post authorization to subagents\n  - include preset bodies"],
        );
        let auth = subagent_authorization("doc.md", &content, "#subagents do [#waypost]");
        assert_eq!(auth.tracked_items.len(), 1, "{auth:?}");
        assert_eq!(auth.tracked_items[0].id, "waypost");
        assert_eq!(auth.tracked_items[0].component, "backlog");
        assert!(
            auth.tracked_items[0]
                .text
                .contains("Post authorization to subagents")
        );
        assert!(auth.tracked_items[0].text.contains("include preset bodies"));
        assert!(auth.unresolved_ids.is_empty());
        assert!(
            auth.subagent_prompt_preamble
                .contains("backlog item #waypost (verbatim)")
        );
    }

    #[test]
    fn free_text_head_has_no_tracked_items() {
        let content = doc(
            PRESETS,
            "subagents preset=\"#spec-test-build-install-commit-push\"",
            &["rename the foo module to bar"],
            &[],
        );
        let auth = subagent_authorization("doc.md", &content, "rename the foo module to bar");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert!(auth.tracked_items.is_empty());
        assert!(auth.unresolved_ids.is_empty());
        assert!(
            auth.subagent_prompt_preamble
                .contains("> rename the foo module to bar")
        );
    }

    #[test]
    fn missing_preset_fails_closed_without_inventing_a_body() {
        let content = doc(
            "",
            "subagents preset=\"#ship-it\"",
            &["do [#abc]"],
            &["[ ] [#abc] thing"],
        );
        let auth = subagent_authorization("doc.md", &content, "do [#abc]");
        assert_eq!(auth.status, AuthorizationStatus::UnresolvedPreset);
        assert_eq!(auth.presets.len(), 1);
        assert_eq!(auth.presets[0].name, "#ship-it");
        assert!(!auth.presets[0].resolved);
        assert_eq!(auth.presets[0].body, None);
        let p = &auth.subagent_prompt_preamble;
        assert!(p.contains(UNRESOLVED_AUTHORIZATION_MARKER), "{p}");
        assert!(p.contains("`#ship-it`"));
        assert!(p.contains("Do not commit, push"));
        assert!(!p.contains("Operator authorization, "));
        let json = serde_json::to_value(&auth).unwrap();
        assert_eq!(json["status"], "unresolved_preset");
        assert_eq!(json["presets"][0]["resolved"], false);
        assert!(json["presets"][0].get("body").is_none());
    }

    #[test]
    fn unknown_id_is_reported_not_invented() {
        let content = doc("", "subagents", &["do [#nope]"], &[]);
        let auth = subagent_authorization("doc.md", &content, "do [#nope]");
        assert_eq!(auth.unresolved_ids, vec!["nope".to_string()]);
        assert!(
            auth.subagent_prompt_preamble
                .contains("No backlog/review item in the document carries #nope")
        );
    }

    #[test]
    fn unhashed_preset_attr_resolves_to_hashtag_key() {
        let fm = "prompt_presets:\n  \"#review\": \"review only, no commits\"\n";
        let content = doc(fm, "subagents preset=review", &["look at foo"], &[]);
        let auth = subagent_authorization("doc.md", &content, "look at foo");
        assert_eq!(auth.status, AuthorizationStatus::Resolved);
        assert_eq!(auth.presets[0].name, "#review");
        assert_eq!(
            auth.presets[0].body.as_deref(),
            Some("review only, no commits")
        );
    }

    #[test]
    fn body_level_preset_line_is_queue_scoped_authorization() {
        let content = concat!(
            "---\nagent_doc_session: s\nagent_doc_format: template\n",
            "prompt_presets:\n  \"#review\": \"review only, no commits\"\n---\n\n",
            "<!-- agent:queue subagents go -->\npreset #review\n- look at foo\n<!-- /agent:queue -->\n",
        );
        let auth = subagent_authorization("doc.md", content, "look at foo");
        assert_eq!(auth.status, AuthorizationStatus::Resolved, "{auth:?}");
        assert_eq!(auth.presets[0].source, PresetSource::QueueLine);
        assert!(
            auth.subagent_prompt_preamble
                .contains("> review only, no commits")
        );
    }
}
