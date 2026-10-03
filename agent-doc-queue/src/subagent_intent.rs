//! Built-in subagent-intent vocabulary (`#subagentintent`).
//!
//! `#subagents` (and its spellings `#subagent`, `#sub-agents`, `#sub-agent`)
//! asks the agent to dispatch a queue item to a background subagent. The tag
//! carries that meaning on its own, whether or not the document registers it
//! in `prompt_presets`. Every consumer that classifies queue text — steering
//! dispatch, preflight's `queue_subagent_dispatch`, and queue drainability —
//! reads this one list, so a spelling recognised by one is recognised by all.
//!
//! The live miss this fixes (agent-doc-bugs.md, 2026-10-03): the document
//! registered only `'#subagents'`. The operator queued
//! `#subagent: https://…/issues/116`. Steering classified it `subagent`, but
//! queue drainability read `#subagent` as a reference to a tracked backlog item
//! `subagent`, found no such item, and judged every such head non-drainable, so
//! the idle supervisor never woke the session.

/// The canonical subagent-intent tag names, lowercased and without `#`.
pub const SUBAGENT_INTENT_TAGS: [&str; 4] = ["subagents", "subagent", "sub-agents", "sub-agent"];

/// True when `name` (with or without a leading `#`, any case) is a built-in
/// subagent-intent tag.
pub fn is_subagent_intent_tag(name: &str) -> bool {
    let name = name.trim().trim_start_matches('#').to_ascii_lowercase();
    SUBAGENT_INTENT_TAGS.contains(&name.as_str())
}

/// True when free text (a preset expansion body) asks for subagent dispatch.
pub fn text_requests_subagents(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    ["subagent", "sub-agent", "sub agent"]
        .iter()
        .any(|needle| body.contains(needle))
}

/// True when any `#word` token in `text` is a subagent-intent tag.
pub fn carries_subagent_intent_tag(text: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '#' | '-' | '_')))
        .filter(|token| token.starts_with('#'))
        .any(is_subagent_intent_tag)
}

/// Queue-marker attributes that dispatch every queue head to a subagent
/// (`<!-- agent:queue subagents -->`, alias `fan-out`), so the operator does
/// not have to tag each line. Plan: `tasks/agent-doc/plan-queue-subagents-attribute.md`.
pub const QUEUE_SUBAGENTS_ATTRS: [&str; 2] = ["subagents", "fan-out"];

/// Concurrent-claim cap when the attribute is a bare flag.
pub const DEFAULT_QUEUE_SUBAGENTS_CAP: usize = 3;

/// Line tags that keep one queue line out of the queue-level attribute.
pub const QUEUE_SUBAGENTS_OPT_OUT_TAGS: [&str; 2] = ["[inline]", "[operator-verify]"];

/// The queue-level subagent dispatch mode declared on the `agent:queue` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueSubagentsMode {
    /// Maximum number of heads in flight at once (`subagents=N`).
    pub max_concurrent: usize,
}

/// Parse one attribute value: empty (bare flag) is the default cap, otherwise
/// a positive integer.
pub fn parse_queue_subagents_value(value: &str) -> Result<usize, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(DEFAULT_QUEUE_SUBAGENTS_CAP);
    }
    match value.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!(
            "expected a positive concurrency cap, got `{value}`"
        )),
    }
}

/// The queue-level subagent mode from the queue marker attributes. `None`
/// when neither spelling is present, or when every present spelling has an
/// invalid value (the attribute warning reports it; the queue drains inline).
/// When both spellings are present, the smaller valid cap wins.
pub fn queue_subagents_mode<'a, I>(attrs: I) -> Option<QueueSubagentsMode>
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    attrs
        .into_iter()
        .filter(|(key, _)| is_queue_subagents_attr(key))
        .filter_map(|(_, value)| parse_queue_subagents_value(value).ok())
        .min()
        .map(|max_concurrent| QueueSubagentsMode { max_concurrent })
}

/// True when `key` is a spelling of the queue-level subagents attribute.
pub fn is_queue_subagents_attr(key: &str) -> bool {
    QUEUE_SUBAGENTS_ATTRS.contains(&key.trim().to_ascii_lowercase().as_str())
}

/// True when a queue line carries a tag that keeps it out of the queue-level
/// subagents attribute (`[inline]`, `[operator-verify]`).
pub fn opts_out_of_queue_subagents(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    QUEUE_SUBAGENTS_OPT_OUT_TAGS
        .iter()
        .any(|tag| line.contains(tag))
}

/// What the queue-level subagents attribute does with the current queue.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct QueueSubagentPlan {
    /// Heads to dispatch now, in queue order (within the concurrency cap).
    pub dispatch: Vec<String>,
    /// Subagent-eligible heads held back, either because the cap is full or
    /// because an `after=` predecessor is still live in the queue. They are
    /// neither dispatched nor drained inline; a later cycle offers them once
    /// a slot frees or the predecessor closes.
    pub held: Vec<String>,
}

/// Plan the queue-level subagents attribute over the current queue.
///
/// Eligibility comes from the current state, not from "new since the last
/// seed", so a head whose dispatch was missed is offered again next cycle.
/// `eligible` are the unclaimed-or-claimed subagent-intent heads in queue
/// order, `live_heads` every live queue head, `claimed` the worker claims,
/// and `after_deps` the `after=` / ordered-list predecessors keyed by id.
pub fn plan_queue_subagent_dispatch(
    mode: QueueSubagentsMode,
    eligible: &[String],
    live_heads: &[String],
    claimed: &crate::queue_claim::ClaimedQueueItems,
    after_deps: &std::collections::HashMap<String, Vec<String>>,
) -> QueueSubagentPlan {
    let live_ids: std::collections::HashSet<String> = live_heads
        .iter()
        .flat_map(|head| crate::queue_claim::referenced_queue_ids(head))
        .collect();
    let in_flight = live_heads
        .iter()
        .filter(|head| claimed.claims(head))
        .count();
    let mut slots = mode.max_concurrent.saturating_sub(in_flight);
    let mut plan = QueueSubagentPlan::default();
    for head in eligible {
        if claimed.claims(head) {
            continue;
        }
        let blocked = crate::queue_claim::referenced_queue_ids(head)
            .iter()
            .any(|id| {
                after_deps.iter().any(|(key, deps)| {
                    key.trim_start_matches('#').eq_ignore_ascii_case(id)
                        && deps.iter().any(|dep| {
                            let dep = dep.trim_start_matches('#').to_ascii_lowercase();
                            dep != *id && live_ids.contains(&dep)
                        })
                })
            });
        if blocked || slots == 0 {
            plan.held.push(head.clone());
        } else {
            plan.dispatch.push(head.clone());
            slots -= 1;
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_is_recognised() {
        for tag in ["#subagents", "#subagent", "#Sub-Agent", "sub-agents"] {
            assert!(is_subagent_intent_tag(tag), "{tag}");
        }
        assert!(!is_subagent_intent_tag("#subagentx"));
        assert!(!is_subagent_intent_tag("#gh-fix"));
    }

    #[test]
    fn tag_is_found_inside_queue_text() {
        assert!(carries_subagent_intent_tag(
            "#subagent: https://github.com/btakita/agent-doc/issues/118"
        ));
        assert!(carries_subagent_intent_tag("do [#a] #sub-agents"));
        assert!(!carries_subagent_intent_tag("release + publish"));
    }

    fn attrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn mode(pairs: &[(&str, &str)]) -> Option<QueueSubagentsMode> {
        let attrs = attrs(pairs);
        queue_subagents_mode(attrs.iter().map(|(k, v)| (k, v)))
    }

    #[test]
    fn queue_subagents_attr_spellings_and_caps() {
        assert_eq!(
            mode(&[("subagents", "")]),
            Some(QueueSubagentsMode {
                max_concurrent: DEFAULT_QUEUE_SUBAGENTS_CAP
            })
        );
        assert_eq!(
            mode(&[("fan-out", "5")]),
            Some(QueueSubagentsMode { max_concurrent: 5 })
        );
        assert_eq!(
            mode(&[("subagents", "4"), ("fan-out", "2")]),
            Some(QueueSubagentsMode { max_concurrent: 2 })
        );
        assert_eq!(mode(&[("preset", "#subagents"), ("go", "")]), None);
        assert_eq!(mode(&[("subagents", "0")]), None);
        assert_eq!(mode(&[("subagents", "lots")]), None);
    }

    #[test]
    fn queue_subagents_opt_out_tags() {
        assert!(opts_out_of_queue_subagents("do [#a] [inline]"));
        assert!(opts_out_of_queue_subagents(
            "[Operator-Verify] check the pane"
        ));
        assert!(!opts_out_of_queue_subagents("do [#a]"));
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plan_dispatches_within_the_cap_and_holds_the_rest() {
        let heads = strings(&["do [#a]", "do [#b]", "do [#c]", "do [#d]"]);
        let claimed = crate::queue_claim::ClaimedQueueItems::from_identities([
            crate::queue_claim::claim_identity("do [#a]"),
        ]);
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode { max_concurrent: 2 },
            &heads,
            &heads,
            &claimed,
            &Default::default(),
        );
        assert_eq!(plan.dispatch, strings(&["do [#b]"]));
        assert_eq!(plan.held, strings(&["do [#c]", "do [#d]"]));
    }

    #[test]
    fn plan_holds_a_head_whose_predecessor_is_still_queued() {
        let heads = strings(&["do [#a]", "do [#b]"]);
        let deps = std::collections::HashMap::from([("b".to_string(), strings(&["a"]))]);
        let none = crate::queue_claim::ClaimedQueueItems::none();
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode { max_concurrent: 3 },
            &heads,
            &heads,
            &none,
            &deps,
        );
        assert_eq!(plan.dispatch, strings(&["do [#a]"]));
        assert_eq!(plan.held, strings(&["do [#b]"]));

        // Once `a` closes (leaves the queue), `b` is dispatched.
        let remaining = strings(&["do [#b]"]);
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode { max_concurrent: 3 },
            &remaining,
            &remaining,
            &none,
            &deps,
        );
        assert_eq!(plan.dispatch, remaining);
        assert!(plan.held.is_empty());
    }
}
