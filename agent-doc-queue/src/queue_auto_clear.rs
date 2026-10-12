//! Queue auto-clear policy (`#bgclearpolicy`).
//!
//! An active `agent:queue` drains each item in a fresh agent context by
//! default: at the idle boundary after one item's proven closeout and before
//! the next head is dispatched, the supervisor resets the harness context and
//! then dispatches the next head to that fresh context.
//!
//! `<!-- agent:queue disable-auto-clear -->` turns this off; the queue then
//! drains in the same context and only an explicit operator Clear resets the
//! session.
//!
//! The rule is deliberately "every item boundary", not "only when accretion
//! warrants it": it is the contract the skill already states ("the drain
//! re-dispatches the next item to a freshly-`/clear`ed agent"), it needs no
//! threshold tuning, and accretion is the wrong input anyway — session
//! accretion measures the document's exchange size, which a context clear does
//! not change. Only compaction reduces it.
//!
//! Why background clearing was turned off on 2026-06-30 (`45ff25f994`), and how
//! this policy avoids each cause:
//!
//! 1. The supervisor typed `/clear` into Codex while Codex still considered a
//!    task in progress (`'/clear' is disabled while a task is in progress`),
//!    from a `[focused-cycle]` head-tag reset (`#qfocsup`) re-armed on every
//!    tick. Here Codex is never typed `/clear`: its context reset is a
//!    supervised fresh restart ([`QueueAutoClearDelivery::RestartFresh`]), and
//!    a clear is attempted at most once per item boundary (the baseline moves
//!    as soon as one is sent).
//! 2. Durable clear markers without provenance let a restarted supervisor
//!    press Enter on a stale `/clear` draft. Auto-clear markers carry the
//!    [`QUEUE_AUTO_CLEAR_SOURCE`] provenance, and the orphan-draft recovery that
//!    converted any visible `/clear` into a supervisor clear stays off.
//! 3. The reset fired from head tags and accretion thresholds regardless of
//!    whether an item had actually completed in that context. Here it fires
//!    only after a committed cycle that advanced the queue head, with an empty
//!    composer, no unanswered exchange steering, and no subagent claim in
//!    flight.
//!
//! This module is pure: callers read the document, cycle state, pane, and
//! claims, then pass the observed facts in.

use agent_doc_element::element;

/// Queue-marker attribute that turns queue auto-clear off.
pub const QUEUE_DISABLE_AUTO_CLEAR_ATTR: &str = "disable-auto-clear";

/// Provenance recorded on the context-clear projection of an auto-clear.
pub const QUEUE_AUTO_CLEAR_SOURCE: &str = "queue_auto_clear";

/// Whether queue auto-clear is enabled for an `agent:queue` marker's
/// attributes. Enabled unless `disable-auto-clear` is present as a bare flag
/// (or with a truthy value); `disable-auto-clear=false` keeps it enabled.
pub fn queue_auto_clear_enabled_for_attrs<'a, I>(attrs: I) -> bool
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    !attrs
        .into_iter()
        .any(|(key, value)| key == QUEUE_DISABLE_AUTO_CLEAR_ATTR && attr_value_is_truthy(value))
}

fn attr_value_is_truthy(value: &str) -> bool {
    let value = value.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "false" | "0" | "no" | "off"
    )
}

/// Queue auto-clear setting declared by a document, or `None` when the document
/// has no `agent:queue` component (or cannot be parsed).
pub fn queue_auto_clear_setting(content: &str) -> Option<bool> {
    let components = element::parse(content).ok()?;
    let queue = components
        .iter()
        .find(|component| component.name == "queue")?;
    Some(queue_auto_clear_enabled_for_attrs(&queue.attrs))
}

/// Whether the document's queue auto-clears between items. A document without
/// a queue component has nothing to drain, so this reports `false` for it.
pub fn queue_auto_clear_enabled(content: &str) -> bool {
    queue_auto_clear_setting(content).unwrap_or(false)
}

/// The context state the supervisor last observed as "fresh": the last
/// committed cycle and the queue head at that moment. A clear is due once a
/// newer committed cycle exists AND the queue head moved past `head`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueAutoClearBaseline {
    /// The latest committed cycle when the baseline was taken. `None` when no
    /// cycle had committed yet, or the latest cycle was still open (the turn in
    /// progress then counts as this context's work once it commits).
    pub committed_cycle_id: Option<String>,
    /// The active queue head when the baseline was taken.
    pub head: Option<String>,
}

impl QueueAutoClearBaseline {
    pub fn observe(latest_cycle: Option<LatestCycle<'_>>, head: Option<&str>) -> Self {
        Self {
            committed_cycle_id: latest_cycle
                .filter(|cycle| cycle.committed)
                .map(|cycle| cycle.cycle_id.to_string()),
            head: head.map(str::to_string),
        }
    }
}

/// The document's latest cycle, as read from cycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatestCycle<'a> {
    pub cycle_id: &'a str,
    pub committed: bool,
}

/// One live composer observation of the owned pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AutoClearComposer {
    /// The pane could be captured and showed a rendered harness frame.
    pub observed: bool,
    /// The composer is an empty, dispatch-ready prompt.
    pub dispatch_ready: bool,
    /// The harness shows a busy cue (a turn is running).
    pub busy: bool,
    /// The composer holds unsent operator text.
    pub operator_draft: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueAutoClearFacts<'a> {
    /// `disable-auto-clear` is absent from the queue marker.
    pub enabled: bool,
    pub baseline: &'a QueueAutoClearBaseline,
    /// The latest cycle; `None` when there is none or it could not be read.
    pub latest_cycle: Option<LatestCycle<'a>>,
    pub active_head: Option<&'a str>,
    pub composer: AutoClearComposer,
    /// Settled exchange steering a committed cycle left unanswered.
    pub exchange_steering_pending: bool,
    /// Queue items claimed by a subagent that is still working.
    pub claims_in_flight: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAutoClearDecision {
    Clear,
    SkipDisabled,
    SkipNoActiveHead,
    /// No cycle has committed in this context since the baseline.
    SkipNoCompletedItem,
    /// The latest cycle is still open (mid-turn).
    SkipCycleOpen,
    /// A cycle committed, but the queue head did not move past the item this
    /// context worked on (an exchange prompt, or the item is not done yet).
    SkipHeadNotAdvanced,
    SkipSubagentClaimInFlight,
    SkipSteeringPending,
    SkipComposerUnobservable,
    SkipComposerNotReady,
    SkipOperatorDraft,
}

impl QueueAutoClearDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::SkipDisabled => "disabled",
            Self::SkipNoActiveHead => "no_active_head",
            Self::SkipNoCompletedItem => "no_completed_item",
            Self::SkipCycleOpen => "cycle_open",
            Self::SkipHeadNotAdvanced => "head_not_advanced",
            Self::SkipSubagentClaimInFlight => "subagent_claim_in_flight",
            Self::SkipSteeringPending => "steering_pending",
            Self::SkipComposerUnobservable => "composer_unobservable",
            Self::SkipComposerNotReady => "composer_not_ready",
            Self::SkipOperatorDraft => "operator_draft",
        }
    }

    /// Skips that mean "this boundary is not one an item completed at" rather
    /// than "a clear is due but unsafe right now". Callers log only the latter.
    pub fn is_not_a_boundary(self) -> bool {
        matches!(
            self,
            Self::SkipDisabled
                | Self::SkipNoActiveHead
                | Self::SkipNoCompletedItem
                | Self::SkipHeadNotAdvanced
        )
    }
}

/// Decide whether the supervisor resets the agent context before dispatching
/// `active_head`. The caller has already proven the idle drain boundary
/// (prompt visible, no live turn or route submit, prior cycle converged).
pub fn queue_auto_clear_decision(facts: QueueAutoClearFacts<'_>) -> QueueAutoClearDecision {
    if !facts.enabled {
        return QueueAutoClearDecision::SkipDisabled;
    }
    let Some(head) = facts.active_head else {
        return QueueAutoClearDecision::SkipNoActiveHead;
    };
    let Some(latest) = facts.latest_cycle else {
        return QueueAutoClearDecision::SkipNoCompletedItem;
    };
    if !latest.committed {
        return QueueAutoClearDecision::SkipCycleOpen;
    }
    if facts.baseline.committed_cycle_id.as_deref() == Some(latest.cycle_id) {
        return QueueAutoClearDecision::SkipNoCompletedItem;
    }
    if facts.baseline.head.as_deref() == Some(head) {
        return QueueAutoClearDecision::SkipHeadNotAdvanced;
    }
    if facts.claims_in_flight {
        return QueueAutoClearDecision::SkipSubagentClaimInFlight;
    }
    if facts.exchange_steering_pending {
        return QueueAutoClearDecision::SkipSteeringPending;
    }
    if !facts.composer.observed {
        return QueueAutoClearDecision::SkipComposerUnobservable;
    }
    if facts.composer.operator_draft {
        return QueueAutoClearDecision::SkipOperatorDraft;
    }
    if facts.composer.busy || !facts.composer.dispatch_ready {
        return QueueAutoClearDecision::SkipComposerNotReady;
    }
    QueueAutoClearDecision::Clear
}

/// How an auto-clear reaches the harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueAutoClearDelivery {
    /// Type the harness-native clear (`/clear`, OpenCode `/new`) into the live
    /// child. The process and its background agents/shells survive.
    TypeNativeClear,
    /// Supervised fresh restart: the harness's own clear exits the process
    /// (Codex), so typing it would race the child's exit.
    RestartFresh,
}

impl QueueAutoClearDelivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TypeNativeClear => "native_clear",
            Self::RestartFresh => "restart_fresh",
        }
    }
}

pub fn queue_auto_clear_delivery(native_clear_keeps_process: bool) -> QueueAutoClearDelivery {
    if native_clear_keeps_process {
        QueueAutoClearDelivery::TypeNativeClear
    } else {
        QueueAutoClearDelivery::RestartFresh
    }
}

/// Whether the in-session drain (Claude `/loop`, the Codex Stop-hook
/// continuation, OpenCode's in-turn self-drive) yields the next item to the
/// supervisor so it can clear the context before dispatching it.
///
/// Only a ready supervisor that has kept its last drain promise is handed the
/// boundary; otherwise the in-session drain keeps going in the same context,
/// which is the safe fallback (no stall).
pub fn in_session_drain_yields_to_auto_clear(
    auto_clear_enabled: bool,
    continuation_required: bool,
    supervisor_ready: bool,
    supervisor_handoff_overdue: bool,
) -> bool {
    auto_clear_enabled && continuation_required && supervisor_ready && !supervisor_handoff_overdue
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(marker: &str) -> String {
        format!("<!-- agent:queue {marker} -->\n- do [#a]\n<!-- /agent:queue -->\n")
    }

    #[test]
    fn auto_clear_is_on_by_default_for_a_queue() {
        assert_eq!(queue_auto_clear_setting(&doc("go")), Some(true));
        assert!(queue_auto_clear_enabled(&doc("priority go")));
    }

    #[test]
    fn disable_auto_clear_bare_flag_turns_it_off() {
        assert_eq!(
            queue_auto_clear_setting(&doc("disable-auto-clear go")),
            Some(false)
        );
        assert_eq!(
            queue_auto_clear_setting(&doc("go disable-auto-clear subagents")),
            Some(false)
        );
    }

    #[test]
    fn disable_auto_clear_value_forms() {
        assert_eq!(
            queue_auto_clear_setting(&doc("disable-auto-clear=true")),
            Some(false)
        );
        assert_eq!(
            queue_auto_clear_setting(&doc("disable-auto-clear=\"true\"")),
            Some(false)
        );
        assert_eq!(
            queue_auto_clear_setting(&doc("disable-auto-clear=false")),
            Some(true)
        );
    }

    #[test]
    fn no_queue_component_has_no_setting() {
        let content = "<!-- agent:exchange disable-auto-clear -->\nhi\n<!-- /agent:exchange -->\n";
        assert_eq!(queue_auto_clear_setting(content), None);
        assert!(!queue_auto_clear_enabled(content));
    }

    fn ready_composer() -> AutoClearComposer {
        AutoClearComposer {
            observed: true,
            dispatch_ready: true,
            busy: false,
            operator_draft: false,
        }
    }

    fn baseline(cycle: Option<&str>, head: Option<&str>) -> QueueAutoClearBaseline {
        QueueAutoClearBaseline {
            committed_cycle_id: cycle.map(str::to_string),
            head: head.map(str::to_string),
        }
    }

    fn facts<'a>(
        baseline: &'a QueueAutoClearBaseline,
        latest: Option<LatestCycle<'a>>,
        head: Option<&'a str>,
    ) -> QueueAutoClearFacts<'a> {
        QueueAutoClearFacts {
            enabled: true,
            baseline,
            latest_cycle: latest,
            active_head: head,
            composer: ready_composer(),
            exchange_steering_pending: false,
            claims_in_flight: false,
        }
    }

    const COMMITTED_B: LatestCycle<'static> = LatestCycle {
        cycle_id: "cycle-b",
        committed: true,
    };

    #[test]
    fn clears_at_the_item_boundary_after_a_committed_cycle_advanced_the_head() {
        let base = baseline(Some("cycle-a"), Some("do [#a]"));
        assert_eq!(
            queue_auto_clear_decision(facts(&base, Some(COMMITTED_B), Some("do [#b]"))),
            QueueAutoClearDecision::Clear
        );
    }

    #[test]
    fn a_turn_in_progress_at_baseline_counts_once_it_commits() {
        // Baseline taken while cycle-b was open: no committed id recorded.
        let base = QueueAutoClearBaseline::observe(
            Some(LatestCycle {
                cycle_id: "cycle-b",
                committed: false,
            }),
            Some("do [#a]"),
        );
        assert_eq!(base.committed_cycle_id, None);
        assert_eq!(
            queue_auto_clear_decision(facts(&base, Some(COMMITTED_B), Some("do [#b]"))),
            QueueAutoClearDecision::Clear
        );
    }

    #[test]
    fn never_clears_mid_turn() {
        let base = baseline(Some("cycle-a"), Some("do [#a]"));
        let open = LatestCycle {
            cycle_id: "cycle-b",
            committed: false,
        };
        assert_eq!(
            queue_auto_clear_decision(facts(&base, Some(open), Some("do [#b]"))),
            QueueAutoClearDecision::SkipCycleOpen
        );
        let mut busy = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        busy.composer.busy = true;
        assert_eq!(
            queue_auto_clear_decision(busy),
            QueueAutoClearDecision::SkipComposerNotReady
        );
    }

    #[test]
    fn at_most_one_clear_per_boundary() {
        // After a clear the caller re-baselines to the current cycle + head.
        let base = QueueAutoClearBaseline::observe(Some(COMMITTED_B), Some("do [#b]"));
        assert_eq!(
            queue_auto_clear_decision(facts(&base, Some(COMMITTED_B), Some("do [#b]"))),
            QueueAutoClearDecision::SkipNoCompletedItem
        );
    }

    #[test]
    fn a_committed_exchange_turn_that_did_not_advance_the_head_keeps_context() {
        let base = baseline(Some("cycle-a"), Some("do [#b]"));
        assert_eq!(
            queue_auto_clear_decision(facts(&base, Some(COMMITTED_B), Some("do [#b]"))),
            QueueAutoClearDecision::SkipHeadNotAdvanced
        );
    }

    #[test]
    fn operator_draft_steering_and_claims_suppress_the_clear() {
        let base = baseline(Some("cycle-a"), Some("do [#a]"));
        let mut draft = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        draft.composer.operator_draft = true;
        draft.composer.dispatch_ready = false;
        assert_eq!(
            queue_auto_clear_decision(draft),
            QueueAutoClearDecision::SkipOperatorDraft
        );

        let mut steering = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        steering.exchange_steering_pending = true;
        assert_eq!(
            queue_auto_clear_decision(steering),
            QueueAutoClearDecision::SkipSteeringPending
        );

        let mut claims = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        claims.claims_in_flight = true;
        assert_eq!(
            queue_auto_clear_decision(claims),
            QueueAutoClearDecision::SkipSubagentClaimInFlight
        );

        let mut unobserved = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        unobserved.composer = AutoClearComposer::default();
        assert_eq!(
            queue_auto_clear_decision(unobserved),
            QueueAutoClearDecision::SkipComposerUnobservable
        );
    }

    #[test]
    fn disable_auto_clear_suppresses_the_clear() {
        let base = baseline(Some("cycle-a"), Some("do [#a]"));
        let mut disabled = facts(&base, Some(COMMITTED_B), Some("do [#b]"));
        disabled.enabled = false;
        assert_eq!(
            queue_auto_clear_decision(disabled),
            QueueAutoClearDecision::SkipDisabled
        );
    }

    #[test]
    fn codex_restarts_while_claude_and_opencode_type_the_clear() {
        assert_eq!(
            queue_auto_clear_delivery(true),
            QueueAutoClearDelivery::TypeNativeClear
        );
        assert_eq!(
            queue_auto_clear_delivery(false),
            QueueAutoClearDelivery::RestartFresh
        );
    }

    #[test]
    fn in_session_drain_yields_only_to_a_ready_supervisor() {
        assert!(in_session_drain_yields_to_auto_clear(
            true, true, true, false
        ));
        assert!(!in_session_drain_yields_to_auto_clear(
            false, true, true, false
        ));
        assert!(!in_session_drain_yields_to_auto_clear(
            true, false, true, false
        ));
        assert!(!in_session_drain_yields_to_auto_clear(
            true, true, false, false
        ));
        assert!(!in_session_drain_yields_to_auto_clear(
            true, true, true, true
        ));
    }
}
