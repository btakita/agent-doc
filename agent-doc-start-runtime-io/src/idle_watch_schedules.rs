//! Lazily-owned retained intents of the idle-watch supervisor
//! (`#implementlazilyowned`, plan step 8b).
//!
//! The idle watch retains two background effects across attempts: resuming a
//! captured finalize, and the document-local queue projection self-heal. Each
//! used to keep its own retained intent — an attempt counter, a retry deadline,
//! and a `due` latch — next to the worker that ran it. Both now live in a
//! [`LatestIntentSchedule`], where `lazily::EffectCasCore` owns the intent's
//! identity, its attempt number, its retry eligibility, its single flight, and
//! latest-wins supersession. The idle watch keeps only the physical wait (its
//! poll tick), the pace curves, the worker, and the verdict latches that are
//! not retry state (an operator gate, an unlandable capture).

use std::time::{Duration, Instant};

use agent_doc_effect_cas::latest_intent::{
    IntentAttempt, IntentClaim, IntentRetry, LatestIntentSchedule,
};
use agent_doc_repair_command_io::CapturedFinalizeResumeKey;

/// The idle watch owns one document, so each schedule has one key.
const KEY: &str = "document";

pub(crate) type ScheduleAttempt = IntentAttempt<&'static str>;

fn operation(key: &CapturedFinalizeResumeKey) -> String {
    format!(
        "{}:{}:{}",
        key.cycle_id, key.capture_id, key.response_sha256
    )
}

/// Non-retry verdict state of the captured operation the schedule retains.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedCapture {
    key: CapturedFinalizeResumeKey,
    /// `#needsoperatorstateedge`: the last attempt latched an operator gate
    /// that only a document-state edge retires.
    needs_operator: bool,
    /// GH 90: the captured bytes are deterministically refused. No document
    /// edge retires this: only a different operation key does.
    unlandable: bool,
    /// Whether the intent's due time was already published as an effect-retry
    /// edge, or needs none (a state edge owns the next attempt).
    trigger_published: bool,
}

impl RetainedCapture {
    fn new(key: CapturedFinalizeResumeKey, trigger_published: bool) -> Self {
        Self {
            key,
            needs_operator: false,
            unlandable: false,
            trigger_published,
        }
    }
}

/// The retained captured-finalize resume of one document.
///
/// Per-outcome attempt resets are Lazily epochs: an outcome that restarts the
/// count records the claimed attempt terminal and desires a fresh epoch, so
/// the next claim's attempt number is exactly the pre-migration
/// `attempts + 1` (a lease wait restarts at 1; an operator or unlandable
/// verdict counts as the first refusal of the fresh epoch, so the next attempt
/// is 2).
#[derive(Debug, Default)]
pub(crate) struct CapturedFinalizeResumeSchedule {
    schedule: LatestIntentSchedule<&'static str>,
    retained: Option<RetainedCapture>,
}

impl CapturedFinalizeResumeSchedule {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn desire_fresh(&mut self, key: &CapturedFinalizeResumeKey) {
        let operation = operation(key);
        // The identity is built from validated ids; a refusal only means the
        // key stays without a retained intent and is not started.
        let _ = self.schedule.desire(
            KEY,
            "captured-finalize-resume",
            operation.clone(),
            operation,
        );
    }

    /// Observe the captured operation currently eligible for recovery. A new
    /// operation key is a fresh Lazily epoch (latest wins), claimable at once
    /// with no timed retry edge; a state edge owns its first attempt.
    pub(crate) fn observe_operation(&mut self, key: &CapturedFinalizeResumeKey) {
        if self
            .retained
            .as_ref()
            .is_some_and(|retained| &retained.key == key)
        {
            return;
        }
        self.desire_fresh(key);
        self.retained = Some(RetainedCapture::new(key.clone(), true));
    }

    /// No captured operation is retained any more: Lazily drops the intent.
    pub(crate) fn clear(&mut self, now: Instant) {
        self.schedule
            .withdraw(&KEY, "captured_operation_absent", now);
        self.retained = None;
    }

    pub(crate) fn retained_key(&self) -> Option<&CapturedFinalizeResumeKey> {
        self.retained.as_ref().map(|retained| &retained.key)
    }

    pub(crate) fn needs_operator(&self) -> bool {
        self.retained
            .as_ref()
            .is_some_and(|retained| retained.needs_operator)
    }

    pub(crate) fn is_unlandable(&self, key: &CapturedFinalizeResumeKey) -> bool {
        self.retained
            .as_ref()
            .is_some_and(|retained| &retained.key == key && retained.unlandable)
    }

    /// Whether Lazily makes the retained intent claimable at `now`.
    pub(crate) fn is_due(&self, now: Instant) -> bool {
        self.schedule.is_due(&KEY, now)
    }

    /// Claim the retained intent's next attempt.
    pub(crate) fn claim(&mut self, now: Instant) -> Option<ScheduleAttempt> {
        match self.schedule.claim(&KEY, now) {
            IntentClaim::Claimed(attempt) => Some(attempt),
            IntentClaim::Waiting { .. } | IntentClaim::Busy | IntentClaim::Empty => None,
        }
    }

    /// Publish the retained intent's due time as one effect-retry edge.
    /// Returns whether the caller must publish it now.
    pub(crate) fn take_due_edge(&mut self, now: Instant) -> bool {
        let due = self.schedule.is_due(&KEY, now);
        let Some(retained) = self.retained.as_mut() else {
            return false;
        };
        if retained.needs_operator || retained.trigger_published || !due {
            return false;
        }
        retained.trigger_published = true;
        true
    }

    /// New document evidence: retire any operator gate and make the retained
    /// intent due now (preempting a deadline derived from the previous
    /// document state). The document edge itself owns the next attempt, so no
    /// timed edge is published on top of it. Returns whether an operator gate
    /// cleared.
    pub(crate) fn observe_document_edge(&mut self, now: Instant) -> bool {
        let Some(retained) = self.retained.as_mut() else {
            return false;
        };
        let operator_gate_cleared = retained.needs_operator;
        retained.needs_operator = false;
        retained.trigger_published = true;
        self.schedule.retime(&KEY, now);
        operator_gate_cleared
    }

    /// The attempt reached a terminal receipt (committed, superseded by the
    /// controller, settled, or closed without a commit).
    pub(crate) fn completed(&mut self, attempt: &ScheduleAttempt, receipt: &str) {
        self.schedule.applied(attempt, receipt);
        self.retained = None;
    }

    /// The attempt is waiting for a controller state edge, not a timer: the
    /// epoch retires and the next observation desires a fresh one.
    pub(crate) fn awaiting_state_edge(&mut self, attempt: &ScheduleAttempt) {
        self.schedule
            .superseded(attempt, "awaiting_controller_state_edge");
        self.retained = None;
    }

    /// A live owner holds the closeout lease until `now + delay`: Lazily
    /// records the attempt superseded by that lease and retains a fresh epoch
    /// claimable at the lease deadline.
    pub(crate) fn lease_held(
        &mut self,
        attempt: &ScheduleAttempt,
        key: &CapturedFinalizeResumeKey,
        delay: Duration,
        now: Instant,
    ) {
        self.schedule.superseded(attempt, "live_owner_lease");
        self.desire_fresh(key);
        self.schedule.retime(&KEY, now + delay);
        self.retained = Some(RetainedCapture::new(key.clone(), false));
    }

    /// The attempt failed transiently: Lazily retains the same intent,
    /// claimable after `pace(attempt number)`. Returns the attempt number and
    /// the delay.
    pub(crate) fn retry_after(
        &mut self,
        attempt: &ScheduleAttempt,
        key: &CapturedFinalizeResumeKey,
        reason: &str,
        pace: impl FnOnce(u32) -> Duration,
        now: Instant,
    ) -> (u32, Duration) {
        let number = attempt.attempt();
        let delay = pace(number);
        self.retained = match self.schedule.retry_after(attempt, reason, delay, now) {
            IntentRetry::Retrying { .. } => Some(RetainedCapture::new(key.clone(), false)),
            // A newer operation was desired while this attempt ran; it owns
            // the key now.
            IntentRetry::Superseded | IntentRetry::Unknown => None,
        };
        (number, delay)
    }

    /// Restart the count as a fresh epoch whose first refusal is the verdict
    /// just recorded, so the next claimed attempt is number 2.
    fn restart_after_verdict(
        &mut self,
        key: &CapturedFinalizeResumeKey,
        reason: &str,
        now: Instant,
    ) {
        self.desire_fresh(key);
        if let Some(verdict) = self.claim(now) {
            self.schedule
                .retry_after(&verdict, reason, Duration::ZERO, now);
        }
    }

    /// GH 90: the captured bytes can never land. Lazily rejects the intent;
    /// only a different operation key desires another. Returns whether this
    /// key was already reported unlandable.
    pub(crate) fn unlandable(
        &mut self,
        attempt: &ScheduleAttempt,
        key: &CapturedFinalizeResumeKey,
        reason: &str,
    ) -> bool {
        let already_reported = self.is_unlandable(key);
        self.schedule.rejected(attempt, reason);
        self.retained = Some(RetainedCapture {
            unlandable: true,
            ..RetainedCapture::new(key.clone(), true)
        });
        already_reported
    }

    /// The attempt returned an operator-required verdict; `latched` is
    /// whether the trigger graph latched it (`#retainconv`). Either the
    /// operator gate or the already-pending state edge owns the next attempt.
    pub(crate) fn needs_operator_verdict(
        &mut self,
        attempt: &ScheduleAttempt,
        key: &CapturedFinalizeResumeKey,
        latched: bool,
        reason: &str,
        now: Instant,
    ) {
        self.schedule.superseded(attempt, "operator_gate");
        self.restart_after_verdict(key, reason, now);
        self.retained = Some(RetainedCapture {
            needs_operator: latched,
            ..RetainedCapture::new(key.clone(), true)
        });
    }
}

/// The document-local queue projection self-heal intent.
///
/// `due` is "Lazily retains an intent", the retry deadline is its ready time,
/// and the worker holds its single flight. The operator-typing quiet window
/// stays the caller's debounce: it is not retry state.
#[derive(Debug)]
pub(crate) struct QueueProjectionSelfHealSchedule {
    schedule: LatestIntentSchedule<&'static str>,
}

impl QueueProjectionSelfHealSchedule {
    /// A supervisor reconciles derived queue state once on start.
    pub(crate) fn new() -> Self {
        let mut schedule = Self {
            schedule: LatestIntentSchedule::new(),
        };
        schedule.request();
        schedule
    }

    /// Desire a repair. An already-retained intent keeps its ready time; one
    /// requested while an attempt runs is the newer intent that runs after it.
    pub(crate) fn request(&mut self) {
        if !self.schedule.is_pending(&KEY) {
            let _ = self.schedule.desire(
                KEY,
                "queue-projection-self-heal",
                "document_delivery",
                "repair",
            );
        }
    }

    /// Drop a retained repair that is no longer wanted.
    pub(crate) fn withdraw(&mut self, observed: &str, now: Instant) {
        self.schedule.withdraw(&KEY, observed, now);
    }

    pub(crate) fn is_pending(&self) -> bool {
        self.schedule.is_pending(&KEY)
    }

    pub(crate) fn is_due(&self, now: Instant) -> bool {
        self.schedule.is_due(&KEY, now)
    }

    pub(crate) fn claim(&mut self, now: Instant) -> Option<ScheduleAttempt> {
        match self.schedule.claim(&KEY, now) {
            IntentClaim::Claimed(attempt) => Some(attempt),
            IntentClaim::Waiting { .. } | IntentClaim::Busy | IntentClaim::Empty => None,
        }
    }

    pub(crate) fn settled(&mut self, attempt: &ScheduleAttempt) {
        self.schedule.applied(attempt, "settled");
    }

    /// The attempt failed transiently, or never ran: the repair stays
    /// retained and is due after `delay`. A repair requested while it ran
    /// waits the same delay.
    pub(crate) fn defer(
        &mut self,
        attempt: &ScheduleAttempt,
        reason: &str,
        delay: Duration,
        now: Instant,
    ) -> Instant {
        let at = now + delay;
        if !matches!(
            self.schedule.retry_after(attempt, reason, delay, now),
            IntentRetry::Retrying { .. }
        ) {
            self.schedule.retime(&KEY, at);
        }
        at
    }

    /// The operator's newer cut superseded the attempt: Lazily records it
    /// `Superseded { observed }` and retains one new repair, due after the
    /// full quiet window. Returns the instant it is due.
    pub(crate) fn superseded_after_quiet(
        &mut self,
        attempt: &ScheduleAttempt,
        observed: &str,
        quiet: Duration,
        now: Instant,
    ) -> Instant {
        self.schedule.superseded(attempt, observed);
        self.request();
        let at = now + quiet;
        self.schedule.retime(&KEY, at);
        at
    }

    /// The attempt was superseded with no watermark to retire its losing
    /// edges: nothing is retained until a new document edge requests repair.
    pub(crate) fn cancelled(&mut self, attempt: &ScheduleAttempt, observed: &str, now: Instant) {
        self.schedule.superseded(attempt, observed);
        self.withdraw(observed, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(capture: &str) -> CapturedFinalizeResumeKey {
        CapturedFinalizeResumeKey {
            cycle_id: "cycle-a".to_string(),
            capture_id: capture.to_string(),
            response_sha256: "response-a".to_string(),
        }
    }

    fn pace(attempt: u32) -> Duration {
        Duration::from_secs(u64::from(attempt) * 2)
    }

    fn started(schedule: &mut CapturedFinalizeResumeSchedule, now: Instant) -> ScheduleAttempt {
        assert!(schedule.is_due(now), "the retained intent must be due");
        schedule.claim(now).expect("a due intent claims")
    }

    #[test]
    fn retryable_effects_count_lazily_attempts_and_wait_for_their_pace() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key = key("capture-a");
        schedule.observe_operation(&key);
        assert!(
            !schedule.take_due_edge(now),
            "a fresh operation needs no timed edge"
        );
        let first = started(&mut schedule, now);
        assert_eq!(first.attempt(), 1);
        assert!(!schedule.is_due(now), "the single flight is held");
        assert_eq!(
            schedule.retry_after(&first, &key, "refused", pace, now),
            (1, Duration::from_secs(2))
        );
        assert!(!schedule.is_due(now));
        assert!(!schedule.take_due_edge(now));
        let due = now + Duration::from_secs(2);
        assert!(
            schedule.take_due_edge(due),
            "the due time is one effect-retry edge"
        );
        assert!(!schedule.take_due_edge(due), "and only one");
        let second = started(&mut schedule, due);
        assert_eq!(second.attempt(), 2);
        assert_eq!(
            schedule.retry_after(&second, &key, "refused", pace, due).0,
            2
        );
    }

    #[test]
    fn a_lease_wait_restarts_the_count_and_is_due_at_the_deadline() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key = key("capture-a");
        schedule.observe_operation(&key);
        let first = started(&mut schedule, now);
        schedule.retry_after(&first, &key, "refused", pace, now);
        let second = started(&mut schedule, now + Duration::from_secs(2));
        assert_eq!(second.attempt(), 2);
        let lease = Duration::from_secs(45);
        schedule.lease_held(&second, &key, lease, now);
        assert!(!schedule.is_due(now + Duration::from_secs(44)));
        assert!(schedule.take_due_edge(now + lease));
        assert_eq!(started(&mut schedule, now + lease).attempt(), 1);
    }

    #[test]
    fn a_document_edge_preempts_the_deadline_and_retires_the_operator_gate() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key = key("capture-a");
        schedule.observe_operation(&key);
        let first = started(&mut schedule, now);
        schedule.needs_operator_verdict(&first, &key, true, "conflict", now);
        assert!(schedule.needs_operator());
        assert!(
            !schedule.take_due_edge(now),
            "an operator gate publishes no timed edge"
        );
        assert!(schedule.observe_document_edge(now));
        assert!(!schedule.needs_operator());
        assert!(
            !schedule.observe_document_edge(now),
            "the gate cleared once"
        );
        let second = started(&mut schedule, now);
        assert_eq!(
            second.attempt(),
            2,
            "the verdict was the fresh epoch's first refusal"
        );

        schedule.lease_held(&second, &key, Duration::from_secs(300), now);
        assert!(!schedule.is_due(now));
        assert!(!schedule.observe_document_edge(now));
        assert!(
            schedule.is_due(now),
            "a document edge preempts a lease deadline"
        );
        assert!(
            !schedule.take_due_edge(now),
            "the edge owns the next attempt"
        );
    }

    #[test]
    fn an_unlandable_capture_is_rejected_until_the_key_changes() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key_a = key("capture-a");
        schedule.observe_operation(&key_a);
        let first = started(&mut schedule, now);
        assert!(!schedule.unlandable(&first, &key_a, "refused bytes"));
        assert!(schedule.is_unlandable(&key_a));
        schedule.observe_document_edge(now);
        schedule.observe_operation(&key_a);
        assert!(
            schedule.is_unlandable(&key_a),
            "a document edge does not retire it"
        );
        assert!(!schedule.is_due(now), "Lazily rejected the intent");

        let key_b = key("capture-b");
        schedule.observe_operation(&key_b);
        assert!(!schedule.is_unlandable(&key_b));
        assert_eq!(started(&mut schedule, now).attempt(), 1);
    }

    #[test]
    fn a_terminal_receipt_or_state_wait_retires_the_epoch() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key = key("capture-a");
        schedule.observe_operation(&key);
        let first = started(&mut schedule, now);
        schedule.retry_after(&first, &key, "refused", pace, now);
        let second = started(&mut schedule, now + Duration::from_secs(2));
        schedule.awaiting_state_edge(&second);
        assert_eq!(schedule.retained_key(), None);
        schedule.observe_operation(&key);
        let third = started(&mut schedule, now);
        assert_eq!(third.attempt(), 1, "a state wait restarts the count");
        schedule.completed(&third, "committed");
        assert_eq!(schedule.retained_key(), None);
        schedule.observe_operation(&key);
        schedule.clear(now);
        assert!(!schedule.is_due(now));
        assert_eq!(schedule.retained_key(), None);
    }

    #[test]
    fn a_new_operation_supersedes_the_retained_one() {
        let mut schedule = CapturedFinalizeResumeSchedule::new();
        let now = Instant::now();
        let key_a = key("capture-a");
        schedule.observe_operation(&key_a);
        let first = started(&mut schedule, now);
        let key_b = key("capture-b");
        schedule.observe_operation(&key_b);
        schedule.retry_after(&first, &key_a, "refused", pace, now);
        assert_eq!(
            schedule.retained_key(),
            None,
            "the newer operation owns the key"
        );
        schedule.observe_operation(&key_b);
        let next = started(&mut schedule, now);
        assert_eq!(
            (next.payload(), next.attempt()),
            ("cycle-a:capture-b:response-a", 1)
        );
    }

    #[test]
    fn queue_projection_repair_is_one_retained_intent() {
        let quiet = Duration::from_secs(2);
        let backoff = Duration::from_secs(30);
        let now = Instant::now();
        let mut schedule = QueueProjectionSelfHealSchedule::new();
        assert!(
            schedule.is_pending() && schedule.is_due(now),
            "start reconciles once"
        );
        let first = schedule.claim(now).expect("due");
        assert!(!schedule.is_pending());
        schedule.request();
        schedule.defer(&first, "controller unavailable", backoff, now);
        assert!(schedule.is_pending());
        assert!(
            !schedule.is_due(now + quiet),
            "a repair requested mid-attempt waits the same backoff"
        );
        let second = schedule.claim(now + backoff).expect("due after backoff");

        let at = schedule.superseded_after_quiet(&second, "edge@900", quiet, now);
        assert_eq!(at, now + quiet);
        assert!(schedule.is_pending() && !schedule.is_due(now));
        let third = schedule
            .claim(at)
            .expect("one new attempt after the quiet window");
        schedule.request();
        schedule.settled(&third);
        assert!(schedule.is_due(at), "an edge during the attempt runs again");

        let fourth = schedule.claim(at).expect("due");
        schedule.request();
        schedule.cancelled(&fourth, "missing watermark", at);
        assert!(
            !schedule.is_pending(),
            "a cancelled attempt awaits a new edge"
        );
        schedule.request();
        schedule.withdraw("no request", at);
        assert!(!schedule.is_pending());
    }
}
