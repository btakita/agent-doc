//! Pure component-attribute warning policy for queue-related document markers.
//!
//! This module owns classification for queue-only attributes misplaced on other
//! components, backlog/icebox queue-sync attributes, and component attribute
//! typo warnings. Callers own file IO and warning transport.

use agent_doc_element::element;

use crate::document_queue::BacklogQueueSyncMode;

/// Attributes that are only meaningful on the `agent:queue` component. Seeing
/// one of these on any other component is a misplaced-attribute mistake.
const QUEUE_ONLY_COMPONENT_ATTRS: &[&str] = &["auto", "start", "go", "stop"];

/// Prompt defaults shared by `agent:queue` and `agent:exchange`.
const PROMPT_COMPONENT_ATTRS: &[&str] = &["preset", "subagents", "fan-out"];

/// Whether `key` is a flag/value attribute owned by `agent:queue`.
///
/// The parser intentionally preserves these attributes on other components so
/// preflight can report them as ignored without mutating operator text. Lint
/// adapters use the same vocabulary to avoid reclassifying a preserved bare
/// flag as malformed syntax.
pub fn is_queue_only_component_attr(key: &str) -> bool {
    QUEUE_ONLY_COMPONENT_ATTRS.contains(&key)
}

/// Whether `key` is meaningful on both prompt-bearing components.
pub fn is_prompt_component_attr(key: &str) -> bool {
    PROMPT_COMPONENT_ATTRS.contains(&key)
}

/// Whether `key` is valid on `agent:queue` without an explicit value.
pub fn is_queue_bare_flag_attr(key: &str) -> bool {
    matches!(
        key,
        "auto" | "start" | "go" | "stop" | "subagents" | "fan-out"
    )
}

/// Whether tagpath's generic missing-value finding is expected for this
/// component/attribute pair. Misplaced bare flags stay warning-only so
/// preflight can report the component-specific mistake without lint failing
/// first.
pub fn is_recognized_bare_flag_attr(component: &str, key: &str) -> bool {
    if is_queue_only_component_attr(key) {
        return component != "agent:queue" || is_queue_bare_flag_attr(key);
    }
    if !is_prompt_component_attr(key) || key == "preset" {
        return false;
    }
    !matches!(component, "agent:queue" | "agent:exchange") || matches!(key, "subagents" | "fan-out")
}

/// Component attribute keys recognized anywhere in the document, excluding the
/// queue-only set above.
const KNOWN_COMPONENT_ATTRS: &[&str] = &[
    "patch",
    "mode",
    "max_lines",
    "archive",
    "transfer-source",
    "timestamp",
    "broken",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentAttrWarning {
    pub issues: Vec<String>,
}

impl ComponentAttrWarning {
    pub fn message_body(&self) -> String {
        format!(
            "{}. The attribute is ignored (no mutation); the auto-loop triggers from `queue: start` (alias `go`) in frontmatter, the `start`/`go` marker control, or the legacy `<!-- agent:queue auto -->`.",
            self.issues.join("; ")
        )
    }
}

pub fn component_attr_warning(content: &str) -> Option<ComponentAttrWarning> {
    let components = element::parse(content).ok()?;
    let mut issues: Vec<String> = Vec::new();
    for component in &components {
        for (key, value) in &component.attrs {
            if is_queue_only_component_attr(key) {
                if component.name != "queue" {
                    issues.push(format!(
                        "`{key}` is a queue-only attribute but appears on `agent:{}` (did you mean `<!-- agent:queue {key} -->`?)",
                        component.name
                    ));
                }
            } else if is_prompt_component_attr(key) {
                if !matches!(component.name.as_str(), "queue" | "exchange") {
                    issues.push(format!(
                        "`{key}` is a prompt-component attribute but appears on `agent:{}` (use it on `agent:queue` or `agent:exchange`)",
                        component.name
                    ));
                } else if crate::subagent_intent::is_queue_subagents_attr(key)
                    && component.name == "queue"
                    && let Err(reason) = crate::subagent_intent::parse_queue_subagents_value(value)
                {
                    issues.push(format!(
                        "`{key}={value}` on `agent:queue`: {reason} (use a bare `{key}` to dispatch every eligible head)"
                    ));
                } else if crate::subagent_intent::is_queue_subagents_attr(key)
                    && component.name == "exchange"
                    && !value.trim().is_empty()
                {
                    issues.push(format!(
                        "`{key}={value}` on `agent:exchange`: expected a bare `{key}` flag because exchange prompts dispatch one at a time"
                    ));
                }
            } else if key == "queue" && matches!(component.name.as_str(), "backlog" | "pending") {
                if BacklogQueueSyncMode::parse(value).is_none() {
                    issues.push(format!(
                        "`queue={value}` on `agent:{}` is not a recognized sync mode (use `sync`, `append`, or `prepend`)",
                        component.name
                    ));
                }
            } else if key == "queue" && component.name == "icebox" {
                issues.push(
                    "`queue` on `agent:icebox` does not auto-populate `agent:queue`; move the item to `agent:backlog` or use a per-item enqueue marker".to_string(),
                );
            } else if key == "priority"
                && matches!(
                    component.name.as_str(),
                    "backlog" | "icebox" | "pending" | "queue"
                )
            {
                // Bare component-level priority is recognized for tracked-work
                // and queue ordering; per-item priority remains item syntax.
            } else if !KNOWN_COMPONENT_ATTRS.contains(&key.as_str()) {
                issues.push(format!(
                    "`{key}` on `agent:{}` is not a recognized component attribute (possible typo)",
                    component.name
                ));
            }
        }
    }
    if issues.is_empty() {
        return None;
    }
    issues.sort();
    Some(ComponentAttrWarning { issues })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_attr_warning_flags_auto_on_backlog() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog auto -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = component_attr_warning(content).expect("`auto` on agent:backlog should warn");
        let body = warning.message_body();
        assert!(body.contains("queue-only attribute"));
        assert!(body.contains("agent:backlog"));
        assert!(body.contains("agent:queue auto"));
        assert!(body.contains("no mutation"));
    }

    #[test]
    fn component_attr_warning_flags_unknown_attr_typo() {
        let content = concat!(
            "<!-- agent:backlog auot -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = component_attr_warning(content).expect("typo'd attribute should warn");
        assert!(
            warning
                .message_body()
                .contains("not a recognized component attribute")
        );
        assert!(warning.message_body().contains("auot"));
    }

    #[test]
    fn component_attr_warning_allows_queue_sync_attr_on_backlog() {
        for marker in [
            "<!-- agent:backlog queue -->",
            "<!-- agent:backlog queue=sync -->",
            "<!-- agent:backlog queue=append -->",
        ] {
            let content = format!("{marker}\n- [ ] [#x1] keep this\n<!-- /agent:backlog -->\n");
            assert!(
                component_attr_warning(&content).is_none(),
                "recognized queue sync attr must not warn: {marker}"
            );
        }
    }

    #[test]
    fn component_attr_warning_flags_queue_sync_attr_on_icebox() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:icebox queue=append -->\n",
            "- [ ] [#x1] parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        let warning = component_attr_warning(content).expect("`queue` on icebox should warn");
        let body = warning.message_body();
        assert!(body.contains("agent:icebox"));
        assert!(body.contains("does not auto-populate"));
        assert!(body.contains("per-item enqueue"));
    }

    #[test]
    fn component_attr_warning_allows_priority_attr() {
        for content in [
            "<!-- agent:backlog priority -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
            "<!-- agent:backlog priority queue -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
            "<!-- agent:queue priority -->\n- do [#a]\n<!-- /agent:queue -->\n",
        ] {
            assert!(
                component_attr_warning(content).is_none(),
                "priority attr must not warn: {content}"
            );
        }
    }

    #[test]
    fn component_attr_warning_flags_invalid_queue_mode() {
        let content = concat!(
            "<!-- agent:backlog queue=nope -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = component_attr_warning(content).expect("unrecognized queue mode should warn");
        let body = warning.message_body();
        assert!(body.contains("not a recognized sync mode"));
        assert!(body.contains("queue=nope"));
    }

    #[test]
    fn component_attr_warning_allows_queue_auto_and_known_attrs() {
        let content = concat!(
            "<!-- agent:queue auto -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:exchange patch=append max_lines=50 -->\n",
            "### Re: prior - gpt-5\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done archive=tasks/x.done.md -->\n",
            "<!-- /agent:done -->\n",
        );
        assert!(component_attr_warning(content).is_none());
    }

    #[test]
    fn component_attr_warning_allows_queue_control_markers() {
        for token in ["start", "go", "stop"] {
            let content = format!(
                "<!-- agent:queue preset=\"#p\" {token} -->\n- do #fix1\n<!-- /agent:queue -->\n",
            );
            assert!(
                component_attr_warning(&content).is_none(),
                "`{token}` on queue must be a recognized control marker"
            );
        }
    }

    #[test]
    fn component_attr_warning_allows_preset_on_queue() {
        let content = concat!(
            "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- do #fix1\n",
            "<!-- /agent:queue -->\n",
        );
        assert!(component_attr_warning(content).is_none());
    }

    #[test]
    fn component_attr_warning_allows_exchange_prompt_attrs() {
        for marker in [
            "<!-- agent:exchange subagents preset=\"#review\" -->",
            "<!-- agent:exchange fan-out preset=#review -->",
        ] {
            let content = format!("{marker}\nfix it\n<!-- /agent:exchange -->\n");
            assert!(
                component_attr_warning(&content).is_none(),
                "valid exchange prompt attrs must not warn: {marker}"
            );
        }
    }

    #[test]
    fn component_attr_warning_rejects_valued_exchange_subagents() {
        let content = "<!-- agent:exchange subagents=2 -->\nfix it\n<!-- /agent:exchange -->\n";
        let body = component_attr_warning(content)
            .expect("exchange concurrency is meaningless")
            .message_body();
        assert!(body.contains("expected a bare `subagents` flag"), "{body}");
    }

    #[test]
    fn component_attr_warning_flags_preset_on_non_prompt_component() {
        let content = concat!(
            "<!-- agent:backlog preset=\"#spec-test-build-install-commit-push\" -->\n",
            "- [ ] [#x1] keep this\n",
            "<!-- /agent:backlog -->\n",
        );
        let warning = component_attr_warning(content)
            .expect("`preset` on backlog should warn as a prompt-component attribute");
        assert!(warning.message_body().contains("prompt-component"));
    }

    #[test]
    fn component_attr_warning_queue_subagents_attr() {
        for marker in [
            "<!-- agent:queue subagents -->",
            "<!-- agent:queue subagents=2 go -->",
            "<!-- agent:queue fan-out preset=\"#spec\" -->",
        ] {
            let content = format!("{marker}\n- do [#a]\n<!-- /agent:queue -->\n");
            assert!(
                component_attr_warning(&content).is_none(),
                "valid subagents attr must not warn: {marker}"
            );
        }
        let bad = "<!-- agent:queue subagents=0 -->\n- do [#a]\n<!-- /agent:queue -->\n";
        let body = component_attr_warning(bad)
            .expect("zero cap warns")
            .message_body();
        assert!(body.contains("subagents=0"), "{body}");
        assert!(body.contains("positive concurrency cap"), "{body}");
        assert!(body.contains("every eligible head"), "{body}");

        let misplaced = "<!-- agent:backlog fan-out -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n";
        let body = component_attr_warning(misplaced)
            .expect("fan-out on backlog warns")
            .message_body();
        assert!(body.contains("prompt-component attribute"), "{body}");
        assert!(body.contains("fan-out"), "{body}");
    }
}
