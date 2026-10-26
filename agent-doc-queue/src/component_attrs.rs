//! Pure component-attribute warning policy for queue-related document markers.
//!
//! This module owns classification for queue-only attributes misplaced on other
//! components, backlog/icebox queue-sync attributes, and component attribute
//! typo warnings. Callers own file IO and warning transport.

use agent_doc_element::element;

use crate::document_queue::BacklogQueueSyncMode;

/// Attributes that are only meaningful on the `agent:queue` component. Seeing
/// one of these on any other component is a misplaced-attribute mistake.
const QUEUE_ONLY_COMPONENT_ATTRS: &[&str] = &[
    "auto",
    "start",
    "go",
    "stop",
    "id",
    "after",
    "model",
    "effort",
    crate::queue_auto_clear::QUEUE_CLEAR_ATTR,
    crate::queue_auto_clear::QUEUE_DISABLE_AUTO_CLEAR_ATTR,
    crate::queue_schedule::QUEUE_TIME_ATTR,
    crate::queue_schedule::QUEUE_LOOP_ATTR,
];

/// Prompt defaults shared by `agent:queue` and `agent:exchange`.
const PROMPT_COMPONENT_ATTRS: &[&str] = &["preset", "presets", "subagents", "fan-out"];
/// `queue-legend` toggles the binary-owned queue marker legend line
/// (`#queuemarkerlegend`); `queue-legend=off` opts the document out.
const STATUS_COMPONENT_ATTRS: &[&str] = &["sync", "sync-mode", "queue-legend"];

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

/// Whether `key` is part of the complete public `agent:queue` marker
/// vocabulary. Mutation commands and lint reconciliation share this owner so a
/// newly supported queue attribute cannot be writable-but-warned or
/// lint-clean-but-silently-ignored.
pub fn is_queue_component_attr(key: &str) -> bool {
    is_queue_only_component_attr(key)
        || is_prompt_component_attr(key)
        || key == crate::queue_schedule::QUEUE_INTERVAL_ATTR
        || key == "priority"
}

/// Whether `key` is part of the complete public `agent:status` marker
/// vocabulary. Status projection and lint reconciliation share this owner so a
/// supported sync policy cannot be writable-but-warned.
pub fn is_status_component_attr(key: &str) -> bool {
    STATUS_COMPONENT_ATTRS.contains(&key) || key == crate::queue_schedule::QUEUE_INTERVAL_ATTR
}

/// Whether `key` is valid on `agent:queue` without an explicit value.
pub fn is_queue_bare_flag_attr(key: &str) -> bool {
    matches!(
        key,
        "auto"
            | "start"
            | "go"
            | "stop"
            | "subagents"
            | "fan-out"
            | "clear"
            | "disable-auto-clear"
            | "loop"
    )
}

/// Whether a tagpath component label denotes the active queue model.
pub fn is_queue_marker_component(component: &str) -> bool {
    matches!(component, "agent:queue" | "agent:run" | "agent:loop")
}

/// Whether a component (bare name, e.g. `review`) accepts the bare `priority`
/// ordering attribute (`#backlog-priority-attribute`, `#reviewpriority`).
///
/// Tracked-work lists (`backlog` / legacy `pending`, `icebox`, `review`) and
/// operator todos accept priority ordering; `queue` sorts its `do [#id]`
/// prompts by source-item rank. Preflight's attribute warning and the lint
/// adapter both consult this one vocabulary.
pub fn is_priority_component(name: &str) -> bool {
    matches!(
        name,
        "backlog" | "icebox" | "pending" | "review" | "todo" | "queue" | "run" | "loop"
    )
}

/// Whether tagpath's generic missing-value finding is expected for this
/// component/attribute pair. Misplaced bare flags stay warning-only so
/// preflight can report the component-specific mistake without lint failing
/// first.
pub fn is_recognized_bare_flag_attr(component: &str, key: &str) -> bool {
    if key == "priority" {
        // tagpath's own bare-`priority` allowlist predates `agent:review
        // priority` (`#reviewpriority`); reconcile against agent-doc's vocabulary.
        return component
            .strip_prefix("agent:")
            .is_some_and(is_priority_component);
    }
    if is_queue_only_component_attr(key) {
        return !is_queue_marker_component(component) || is_queue_bare_flag_attr(key);
    }
    if !is_prompt_component_attr(key) {
        return false;
    }
    if !is_queue_marker_component(component) && component != "agent:exchange" {
        return true;
    }
    matches!(key, "subagents" | "fan-out")
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
    pub has_fatal_schedule_issue: bool,
}

impl ComponentAttrWarning {
    pub fn message_body(&self) -> String {
        if self.has_fatal_schedule_issue {
            return format!(
                "{}. Invalid schedule syntax is fail-closed: only the affected queue is held until corrected; sibling queues remain independent. Other invalid attributes are ignored (no mutation).",
                self.issues.join("; ")
            );
        }
        format!(
            "{}. The attribute is ignored (no mutation); the auto-loop triggers from `queue: start` (alias `go`) in frontmatter, the `start`/`go` marker control, or the legacy `<!-- agent:queue auto -->`.",
            self.issues.join("; ")
        )
    }
}

pub fn component_attr_warning(content: &str) -> Option<ComponentAttrWarning> {
    let components = element::parse(content).ok()?;
    let mut issues: Vec<String> = Vec::new();
    let mut has_fatal_schedule_issue = false;
    for component in &components {
        if component.name == "queue" {
            // `#queueschedule`: invalid `interval`/`time`/`loop` values are
            // reported here and hold only the queue that declares them.
            let (_, schedule_issues) =
                crate::queue_schedule::queue_schedule_spec_for_attrs(&component.attrs);
            has_fatal_schedule_issue |= schedule_issues.iter().any(|issue| issue.fatal);
            issues.extend(
                schedule_issues
                    .into_iter()
                    .map(|issue| format!("`{}` on `agent:queue`: {}", issue.attr, issue.message)),
            );
        }
        for (key, value) in &component.attrs {
            if key == crate::queue_schedule::QUEUE_INTERVAL_ATTR {
                if !matches!(component.name.as_str(), "queue" | "status") {
                    issues.push(format!(
                        "`interval` is a queue-or-status attribute but appears on `agent:{}` (use it on `agent:queue`, or on `agent:status` with `sync`)",
                        component.name,
                    ));
                } else if component.name == "status" && !component.attrs.contains_key("sync") {
                    issues.push(
                        "`interval` on `agent:status` requires a non-empty `sync` instruction"
                            .to_string(),
                    );
                }
            } else if is_queue_only_component_attr(key) {
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
            } else if STATUS_COMPONENT_ATTRS.contains(&key.as_str()) {
                if component.name != "status" {
                    issues.push(format!(
                        "`{key}` is a status-only attribute but appears on `agent:{}` (use it on `agent:status`)",
                        component.name
                    ));
                } else if key == "sync-mode" && !component.attrs.contains_key("sync") {
                    issues.push(
                        "`sync-mode` on `agent:status` requires a non-empty `sync` instruction"
                            .to_string(),
                    );
                } else if key == "queue-legend"
                    && !matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "" | "on" | "off" | "true" | "false" | "yes" | "no" | "1" | "0"
                    )
                {
                    issues.push(format!(
                        "`queue-legend={value}` on `agent:status` is not recognized (use `queue-legend=off` to hide the queue marker legend)"
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
            } else if key == "priority" && is_priority_component(&component.name) {
                // Bare component-level priority is recognized for tracked-work
                // (backlog/icebox/review) and queue ordering; per-item priority
                // remains item syntax.
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
    Some(ComponentAttrWarning {
        issues,
        has_fatal_schedule_issue,
    })
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

    /// `#queuemarkerlegend`: `queue-legend` is a recognized status attribute.
    #[test]
    fn queue_legend_is_a_status_attribute_with_on_off_values() {
        for value in ["queue-legend=off", "queue-legend=on", "queue-legend"] {
            let status =
                format!("<!-- agent:status patch=replace {value} -->\n<!-- /agent:status -->\n");
            assert!(
                component_attr_warning(&status).is_none(),
                "{value}: {:?}",
                component_attr_warning(&status)
            );
        }
        let bad = "<!-- agent:status queue-legend=maybe -->\n<!-- /agent:status -->\n";
        let warning = component_attr_warning(bad).expect("unknown queue-legend value warns");
        assert!(warning.message_body().contains("queue-legend=maybe"));
        let misplaced = "<!-- agent:backlog queue-legend=off -->\n<!-- /agent:backlog -->\n";
        let warning = component_attr_warning(misplaced).expect("queue-legend off status warns");
        assert!(warning.message_body().contains("status-only attribute"));
    }

    #[test]
    fn clear_is_queue_only_and_valid_as_a_bare_queue_flag() {
        let queue = "<!-- agent:queue clear subagents -->\n- a\n<!-- /agent:queue -->\n";
        assert!(component_attr_warning(queue).is_none());

        let backlog =
            "<!-- agent:backlog clear -->\n- [ ] [#x1] keep this\n<!-- /agent:backlog -->\n";
        let warning = component_attr_warning(backlog).expect("clear on backlog must warn");
        assert!(warning.message_body().contains("queue-only attribute"));
        assert!(warning.message_body().contains("agent:queue clear"));
    }

    #[test]
    fn component_attr_warning_accepts_valid_schedule_and_flags_bad_values() {
        let valid = "<!-- agent:queue interval=30m time=09:00,17:30 loop=3 -->\n- a\n<!-- /agent:queue -->\n";
        assert!(
            component_attr_warning(valid).is_none(),
            "{:?}",
            component_attr_warning(valid)
        );
        let bare_loop = "<!-- agent:queue loop -->\n- a\n<!-- /agent:queue -->\n";
        assert!(component_attr_warning(bare_loop).is_none());
        let bad = "<!-- agent:queue interval=soon time=25:00 -->\n- a\n<!-- /agent:queue -->\n";
        let warning = component_attr_warning(bad).expect("bad schedule values warn");
        assert_eq!(warning.issues.len(), 2, "{:?}", warning.issues);
        assert!(warning.has_fatal_schedule_issue);
        assert!(warning.message_body().contains("fail-closed"));
        let misplaced =
            "<!-- agent:backlog interval=5m -->\n- [ ] [#x1] a\n<!-- /agent:backlog -->\n";
        assert!(
            component_attr_warning(misplaced)
                .unwrap()
                .message_body()
                .contains("queue-or-status attribute")
        );
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
            "<!-- agent:icebox priority -->\n- [ ] [#a] x\n<!-- /agent:icebox -->\n",
            "<!-- agent:review priority -->\n- [/] [#a] x\n<!-- /agent:review -->\n",
            "<!-- agent:todo priority -->\n- [ ] [#a] x\n<!-- /agent:todo -->\n",
        ] {
            assert!(
                component_attr_warning(content).is_none(),
                "priority attr must not warn: {content}"
            );
        }
    }

    /// `#reviewpriority`: `agent:review` accepts `priority` but not `queue` —
    /// gated review work never auto-populates `agent:queue` — and `priority`
    /// stays unrecognized on components that carry no tracked-work ordering.
    #[test]
    fn component_attr_warning_review_accepts_priority_but_not_queue() {
        let warning = component_attr_warning(
            "<!-- agent:review priority queue -->\n- [/] [#a] x\n<!-- /agent:review -->\n",
        )
        .expect("queue on agent:review must warn");
        assert_eq!(warning.issues.len(), 1, "{:?}", warning.issues);
        assert!(
            warning.issues[0].contains("`queue` on `agent:review`"),
            "{:?}",
            warning.issues
        );

        let warning =
            component_attr_warning("<!-- agent:exchange priority -->\n<!-- /agent:exchange -->\n")
                .expect("priority on agent:exchange must still warn");
        assert!(
            warning.issues[0].contains("`priority` on `agent:exchange`"),
            "{:?}",
            warning.issues
        );
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
    fn component_attr_warning_allows_queue_id_and_after_dependencies() {
        let content = concat!(
            "<!-- agent:queue id=release-a -->\n",
            "- release A\n",
            "<!-- /agent:queue -->\n",
            "<!-- agent:queue id=release-b after=release-a -->\n",
            "- release B\n",
            "<!-- /agent:queue -->\n",
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
    fn component_attr_warning_accepts_plural_presets_alias_on_prompt_components() {
        for component in ["queue", "exchange"] {
            let content = format!(
                "<!-- agent:{component} presets=\"#auth\" -->\nfix it\n<!-- /agent:{component} -->\n"
            );
            assert!(
                component_attr_warning(&content).is_none(),
                "{component}: {:?}",
                component_attr_warning(&content)
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
    fn bare_preset_is_warning_only_when_misplaced_but_malformed_on_prompt_components() {
        assert!(is_recognized_bare_flag_attr("agent:backlog", "preset"));
        assert!(!is_recognized_bare_flag_attr("agent:queue", "preset"));
        assert!(!is_recognized_bare_flag_attr("agent:run", "preset"));
        assert!(!is_recognized_bare_flag_attr("agent:loop", "preset"));
        assert!(!is_recognized_bare_flag_attr("agent:exchange", "preset"));
    }

    /// `#reviewpriority`: the lint adapter's bare-`priority` verdict follows the
    /// same component vocabulary as preflight's attribute warning.
    #[test]
    fn bare_priority_is_recognized_on_tracked_work_and_queue_components_only() {
        for component in [
            "agent:backlog",
            "agent:pending",
            "agent:icebox",
            "agent:review",
            "agent:queue",
            "agent:run",
            "agent:loop",
        ] {
            assert!(
                is_recognized_bare_flag_attr(component, "priority"),
                "{component} must accept bare priority"
            );
        }
        for component in ["agent:exchange", "agent:done", "agent:status", "review"] {
            assert!(
                !is_recognized_bare_flag_attr(component, "priority"),
                "{component} must not accept bare priority"
            );
        }
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

    #[test]
    fn status_sync_attributes_are_status_only_and_require_an_instruction() {
        assert!(is_status_component_attr("sync"));
        assert!(is_status_component_attr("sync-mode"));
        assert!(is_status_component_attr("queue-legend"));
        assert!(is_status_component_attr("interval"));
        assert!(!is_status_component_attr("unknown"));

        let valid = concat!(
            "<!-- agent:status sync=\"Keep release current\" sync-mode=replace interval=5m -->\n",
            "old\n",
            "<!-- /agent:status -->\n",
        );
        assert!(component_attr_warning(valid).is_none());

        let misplaced =
            "<!-- agent:exchange sync=\"Keep release current\" -->\nq\n<!-- /agent:exchange -->\n";
        let body = component_attr_warning(misplaced)
            .expect("sync outside status warns")
            .message_body();
        assert!(body.contains("status-only attribute"), "{body}");

        let misplaced_interval =
            "<!-- agent:exchange interval=5m -->\nq\n<!-- /agent:exchange -->\n";
        let body = component_attr_warning(misplaced_interval)
            .expect("interval outside queue/status warns")
            .message_body();
        assert!(body.contains("queue-or-status attribute"), "{body}");

        let orphaned_mode =
            "<!-- agent:status sync-mode=replace -->\nold\n<!-- /agent:status -->\n";
        let body = component_attr_warning(orphaned_mode)
            .expect("sync-mode without sync warns")
            .message_body();
        assert!(
            body.contains("requires a non-empty `sync` instruction"),
            "{body}"
        );

        let orphaned_interval = "<!-- agent:status interval=5m -->\nold\n<!-- /agent:status -->\n";
        let body = component_attr_warning(orphaned_interval)
            .expect("interval without sync warns")
            .message_body();
        assert!(
            body.contains("requires a non-empty `sync` instruction"),
            "{body}"
        );
    }
}
