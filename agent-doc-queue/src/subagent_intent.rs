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

/// Queue-marker attributes that dispatch every queue head to a subagent.
/// `subagents` is canonical; singular `subagent` is a compatibility alias for
/// documents that used the already-supported per-item intent spelling, and
/// `fan-out` is the explicit behavior alias.
///
/// Plan: `tasks/agent-doc/plan-queue-subagents-attribute.md`.
pub const QUEUE_SUBAGENTS_ATTRS: [&str; 3] = ["subagents", "subagent", "fan-out"];

/// True when `key` is accepted for compatibility but should be rewritten to
/// the canonical marker spelling. Keeping this separate from recognition lets
/// lint/preflight teach the migration without changing queue ownership.
pub fn is_queue_subagents_compat_alias(key: &str) -> bool {
    key.trim().eq_ignore_ascii_case("subagent")
}

/// Return a blocking diagnostic for an attribute that looks like subagent
/// ownership but is not a supported spelling. Queue planning uses this guard
/// in addition to lint so a typo can never silently become `main_inline` when
/// a caller projects a document without first running the full lint command.
pub fn queue_subagents_attr_near_miss<'a, I>(attrs: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    attrs.into_iter().find_map(|(key, _)| {
        if is_queue_subagents_attr(key) {
            return None;
        }
        let normalized = key
            .trim()
            .to_ascii_lowercase()
            .chars()
            .filter(|ch| !matches!(ch, '-' | '_'))
            .collect::<String>();
        ["subagents", "subagent", "fanout"]
            .iter()
            .any(|candidate| one_edit_or_equal(&normalized, candidate))
            .then(|| {
                format!(
                    "`{key}` looks like a subagent queue attribute but is not supported; use canonical `subagents`"
                )
            })
    })
}

fn one_edit_or_equal(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let (shorter, longer) = if left.len() <= right.len() {
        (left.as_bytes(), right.as_bytes())
    } else {
        (right.as_bytes(), left.as_bytes())
    };
    if longer.len() - shorter.len() > 1 {
        return false;
    }
    if longer.len() == shorter.len() {
        return shorter
            .iter()
            .zip(longer)
            .filter(|(left, right)| left != right)
            .count()
            <= 1;
    }
    let mut short = 0;
    let mut long = 0;
    let mut skipped = false;
    while short < shorter.len() && long < longer.len() {
        if shorter[short] == longer[long] {
            short += 1;
            long += 1;
        } else if skipped {
            return false;
        } else {
            skipped = true;
            long += 1;
        }
    }
    true
}

/// Validate queue-level subagent ownership before planning. Invalid values and
/// near-miss spellings are ownership errors, not permission to drain inline.
pub fn validate_queue_subagents_attrs<'a, I>(attrs: I) -> Result<(), String>
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    let attrs = attrs.into_iter().collect::<Vec<_>>();
    if let Some(error) = queue_subagents_attr_near_miss(attrs.iter().copied()) {
        return Err(error);
    }
    for (key, value) in attrs {
        if is_queue_subagents_attr(key) {
            parse_queue_subagents_value(value).map_err(|reason| {
                format!("invalid `{key}={value}` subagent queue attribute: {reason}")
            })?;
        }
    }
    Ok(())
}

/// Line tags that keep one queue line out of the queue-level attribute.
pub const QUEUE_SUBAGENTS_OPT_OUT_TAGS: [&str; 2] = ["[inline]", "[operator-verify]"];

/// How many dependency-ready queue heads the coordinator may dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueSubagentsCapacity {
    /// A bare `subagents` / `fan-out` marker dispatches every eligible head.
    AllEligible,
    /// An explicit `=N` value bounds concurrent claims.
    Limited(std::num::NonZeroUsize),
}

impl QueueSubagentsCapacity {
    fn most_restrictive(self, other: Self) -> Self {
        match (self, other) {
            (Self::AllEligible, capacity) | (capacity, Self::AllEligible) => capacity,
            (Self::Limited(left), Self::Limited(right)) => Self::Limited(left.min(right)),
        }
    }

    fn remaining_after(self, in_flight: usize) -> Option<usize> {
        match self {
            Self::AllEligible => None,
            Self::Limited(limit) => Some(limit.get().saturating_sub(in_flight)),
        }
    }
}

/// The queue-level subagent dispatch mode declared on the `agent:queue` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueSubagentsMode {
    /// Bare markers scale to all eligible work; `=N` selects a fixed bound.
    pub capacity: QueueSubagentsCapacity,
}

/// Parse one attribute value: empty (bare flag) means every eligible head;
/// otherwise the value is a positive concurrency limit.
pub fn parse_queue_subagents_value(value: &str) -> Result<QueueSubagentsCapacity, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(QueueSubagentsCapacity::AllEligible);
    }
    match value.parse::<std::num::NonZeroUsize>() {
        Ok(limit) => Ok(QueueSubagentsCapacity::Limited(limit)),
        _ => Err(format!(
            "expected a positive concurrency cap, got `{value}`"
        )),
    }
}

/// The queue-level subagent mode from the queue marker attributes. `None`
/// when neither spelling is present, or when every present spelling has an
/// invalid value (the attribute warning reports it; the queue drains inline).
/// When both spellings are present, the most restrictive valid capacity wins.
pub fn queue_subagents_mode<'a, I>(attrs: I) -> Option<QueueSubagentsMode>
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    attrs
        .into_iter()
        .filter(|(key, _)| is_queue_subagents_attr(key))
        .filter_map(|(_, value)| parse_queue_subagents_value(value).ok())
        .reduce(QueueSubagentsCapacity::most_restrictive)
        .map(|capacity| QueueSubagentsMode { capacity })
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
    /// Heads to dispatch now, in queue order (within the declared capacity).
    pub dispatch: Vec<String>,
    /// Subagent-eligible heads held back, either because an explicit cap is
    /// full or because an `after=` predecessor is still live in the queue.
    /// They are neither dispatched nor drained inline; a later cycle offers
    /// them once a slot frees or the predecessor closes.
    pub held: Vec<String>,
}

/// Plan the queue-level subagents attribute over the current queue.
///
/// Eligibility comes from the current state, not from "new since the last
/// seed", so a head whose dispatch was missed is offered again next cycle.
/// `eligible` are the unclaimed-or-claimed subagent-intent heads in queue
/// order, `live_heads` every live queue head, `excluded` the heads that must
/// not be dispatched again, `occupied` the claims whose workers still occupy
/// a capacity slot, and `after_deps` the `after=` / ordered-list predecessors
/// keyed by id. Keeping exclusion separate from occupancy lets a Reported
/// handback stay open for coordinator integration without blocking unrelated
/// ready work. `default_pool` bounds a bare marker (`#subagentworkpool`);
/// `None` keeps the unbounded every-eligible-head behavior.
pub fn plan_queue_subagent_dispatch(
    mode: QueueSubagentsMode,
    eligible: &[String],
    live_heads: &[String],
    excluded: &crate::queue_claim::ClaimedQueueItems,
    occupied: &crate::queue_claim::ClaimedQueueItems,
    after_deps: &std::collections::HashMap<String, Vec<String>>,
    default_pool: Option<std::num::NonZeroUsize>,
) -> QueueSubagentPlan {
    let live_ids: std::collections::HashSet<String> = live_heads
        .iter()
        .flat_map(|head| crate::queue_claim::referenced_queue_ids(head))
        .collect();
    let in_flight = live_heads
        .iter()
        .filter(|head| occupied.claims(head))
        .count();
    // `#subagentworkpool`: a bare marker is bounded by the default pool size
    // when the caller supplies one; the held heads are the refill backlog.
    let capacity = match (mode.capacity, default_pool) {
        (QueueSubagentsCapacity::AllEligible, Some(pool)) => QueueSubagentsCapacity::Limited(pool),
        (capacity, _) => capacity,
    };
    let mut remaining = capacity.remaining_after(in_flight);
    let mut plan = QueueSubagentPlan::default();
    for head in eligible {
        if excluded.claims(head) {
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
        if blocked || remaining == Some(0) {
            plan.held.push(head.clone());
        } else {
            plan.dispatch.push(head.clone());
            if let Some(slots) = &mut remaining {
                *slots -= 1;
            }
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
    fn queue_subagents_attr_spellings_and_capacities() {
        assert_eq!(
            mode(&[("subagents", "")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            })
        );
        assert_eq!(
            mode(&[("fan-out", "5")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(5).unwrap()),
            })
        );
        assert_eq!(
            mode(&[("subagents", "4"), ("fan-out", "2")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(2).unwrap()),
            })
        );
        assert_eq!(
            mode(&[("subagents", ""), ("fan-out", "2")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(2).unwrap()),
            })
        );
        assert_eq!(mode(&[("preset", "#subagents"), ("go", "")]), None);
        assert_eq!(mode(&[("subagents", "0")]), None);
        assert_eq!(mode(&[("subagents", "lots")]), None);
    }

    #[test]
    fn singular_queue_subagent_attr_is_a_compatibility_alias() {
        assert!(is_queue_subagents_compat_alias("subagent"));
        assert_eq!(
            mode(&[("subagent", "")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            })
        );
        assert_eq!(
            mode(&[("subagent", "3")]),
            Some(QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(3).unwrap()),
            })
        );
    }

    #[test]
    fn marker_attr_near_misses_fail_closed() {
        for typo in ["subagnt", "sub-agent", "subagentx", "fanout", "fan_out"] {
            let attrs = attrs(&[(typo, "")]);
            let error = validate_queue_subagents_attrs(attrs.iter().map(|(k, v)| (k, v)))
                .expect_err("subagent marker near-miss must block planning");
            assert!(
                error.contains("use canonical `subagents`"),
                "{typo}: {error}"
            );
        }
        let attrs = attrs(&[("subject", "")]);
        assert!(validate_queue_subagents_attrs(attrs.iter().map(|(k, v)| (k, v))).is_ok());
    }

    #[test]
    fn invalid_subagent_capacity_fails_closed_before_planning() {
        for value in ["0", "many"] {
            let attrs = attrs(&[("subagents", value)]);
            let error = validate_queue_subagents_attrs(attrs.iter().map(|(k, v)| (k, v)))
                .expect_err("invalid capacity must not downgrade ownership");
            assert!(error.contains("positive concurrency cap"), "{error}");
        }
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
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(2).unwrap()),
            },
            &heads,
            &heads,
            &claimed,
            &claimed,
            &Default::default(),
            None,
        );
        assert_eq!(plan.dispatch, strings(&["do [#b]"]));
        assert_eq!(plan.held, strings(&["do [#c]", "do [#d]"]));
    }

    #[test]
    fn excluded_reported_head_does_not_occupy_a_capacity_slot() {
        let heads = strings(&["do [#reported]", "do [#ready]", "do [#later]"]);
        let excluded = crate::queue_claim::ClaimedQueueItems::from_identities([
            crate::queue_claim::claim_identity("do [#reported]"),
        ]);
        let occupied = crate::queue_claim::ClaimedQueueItems::none();
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::Limited(std::num::NonZeroUsize::new(1).unwrap()),
            },
            &heads,
            &heads,
            &excluded,
            &occupied,
            &Default::default(),
            None,
        );
        assert_eq!(plan.dispatch, strings(&["do [#ready]"]));
        assert_eq!(plan.held, strings(&["do [#later]"]));
    }

    #[test]
    fn bare_attr_dispatches_every_eligible_head() {
        let heads = strings(&["do [#a]", "do [#b]", "do [#c]", "do [#d]"]);
        let claimed = crate::queue_claim::ClaimedQueueItems::from_identities([
            crate::queue_claim::claim_identity("do [#a]"),
        ]);
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            },
            &heads,
            &heads,
            &claimed,
            &claimed,
            &Default::default(),
            None,
        );
        assert_eq!(plan.dispatch, strings(&["do [#b]", "do [#c]", "do [#d]"]));
        assert!(plan.held.is_empty());
    }

    #[test]
    fn plan_holds_a_head_whose_predecessor_is_still_queued() {
        let heads = strings(&["do [#a]", "do [#b]"]);
        let deps = std::collections::HashMap::from([("b".to_string(), strings(&["a"]))]);
        let none = crate::queue_claim::ClaimedQueueItems::none();
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            },
            &heads,
            &heads,
            &none,
            &none,
            &deps,
            None,
        );
        assert_eq!(plan.dispatch, strings(&["do [#a]"]));
        assert_eq!(plan.held, strings(&["do [#b]"]));

        // Once `a` closes (leaves the queue), `b` is dispatched.
        let remaining = strings(&["do [#b]"]);
        let plan = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            },
            &remaining,
            &remaining,
            &none,
            &none,
            &deps,
            None,
        );
        assert_eq!(plan.dispatch, remaining);
        assert!(plan.held.is_empty());
    }

    #[test]
    fn independent_siblings_fan_out_but_review_waits_for_address_sha_mutation() {
        let heads = strings(&[
            "do [#lint]",
            "do [#address]",
            "do [#review-new-head-sha]",
            "do [#docs]",
        ]);
        let deps = std::collections::HashMap::from([(
            "review-new-head-sha".to_string(),
            strings(&["address"]),
        )]);
        let none = crate::queue_claim::ClaimedQueueItems::none();

        let first_pass = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            },
            &heads,
            &heads,
            &none,
            &none,
            &deps,
            None,
        );
        assert_eq!(
            first_pass.dispatch,
            strings(&["do [#lint]", "do [#address]", "do [#docs]"])
        );
        assert_eq!(
            first_pass.held,
            strings(&["do [#review-new-head-sha]"]),
            "the review must not inspect an older SHA while address work can still push"
        );

        let after_address = strings(&["do [#review-new-head-sha]"]);
        let second_pass = plan_queue_subagent_dispatch(
            QueueSubagentsMode {
                capacity: QueueSubagentsCapacity::AllEligible,
            },
            &after_address,
            &after_address,
            &none,
            &none,
            &deps,
            None,
        );
        assert_eq!(second_pass.dispatch, after_address);
        assert!(second_pass.held.is_empty());
    }

    /// `#subagentworkpool`: a bare marker is bounded by the default pool, and
    /// a slot freed by an occupied worker finishing refills the next head.
    #[test]
    fn bare_marker_is_bounded_by_the_default_pool_and_refills() {
        let heads = strings(&["do [#a]", "do [#b]", "do [#c]", "do [#d]"]);
        let none = crate::queue_claim::ClaimedQueueItems::none();
        let bare = QueueSubagentsMode {
            capacity: QueueSubagentsCapacity::AllEligible,
        };
        let pool = std::num::NonZeroUsize::new(2);
        let plan = plan_queue_subagent_dispatch(bare, &heads, &heads, &none, &none, &Default::default(), pool);
        assert_eq!(plan.dispatch, strings(&["do [#a]", "do [#b]"]));
        assert_eq!(plan.held, strings(&["do [#c]", "do [#d]"]));

        // `#a` reported (excluded, no longer occupying); `#b` still running.
        let excluded = crate::queue_claim::ClaimedQueueItems::from_identities([
            crate::queue_claim::claim_identity("do [#a]"),
            crate::queue_claim::claim_identity("do [#b]"),
        ]);
        let occupied = crate::queue_claim::ClaimedQueueItems::from_identities([
            crate::queue_claim::claim_identity("do [#b]"),
        ]);
        let refill = plan_queue_subagent_dispatch(bare, &heads, &heads, &excluded, &occupied, &Default::default(), pool);
        assert_eq!(refill.dispatch, strings(&["do [#c]"]));
        assert_eq!(refill.held, strings(&["do [#d]"]));

        let unbounded = plan_queue_subagent_dispatch(bare, &heads, &heads, &none, &none, &Default::default(), None);
        assert_eq!(unbounded.dispatch, heads);
    }
}
