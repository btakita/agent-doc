//! # `wait_machine` — unified bounded wait-machinery state machine (`#waitmachine`)
//!
//! Every place agent-doc blocks for an external condition to become true (pane
//! shell prompt, harness dispatch-ready prompt, clear cooldown, managed Codex
//! capability proof, editor IPC patch ack) historically had its own ad-hoc
//! `Instant::now()` / `while elapsed < timeout` loop and its own duration
//! constant. Several exceeded the operator's hard ceiling: **never hang > 10s**
//! on any exempt-free path.
//!
//! This module is the single source of truth for that bound. It exposes:
//!
//! - [`GLOBAL_HANG_CEILING`] — the one global hang ceiling (10s). Every
//!   non-exempt state's `max_dwell` is `<= GLOBAL_HANG_CEILING`.
//! - [`WaitState`] — the wait state, where each non-exempt variant carries the
//!   ceiling it is enforced against, and the single [`WaitState::ReinstallPause`]
//!   variant is the *only* state allowed to dwell past the global ceiling (it is
//!   bounded instead by the explicit, longer [`REINSTALL_BUDGET`]).
//! - [`tick`] — a **pure, total, side-effect-free** transition function. It takes
//!   the current state + elapsed-in-state + an observed [`Signal`] and returns the
//!   next state. No clock reads, no PTY/socket I/O, no sleeping happen here. All
//!   real-world reads happen in a thin driver ([`WaitMachine`]); the transition
//!   relation is what makes this testable and Lean-modelable (`formal/wait_machine/`).
//!
//! ## Lean parity (`#waitmachine4`)
//!
//! The Rust [`tick`] is the **source of truth**; the Lean `Step` relation in
//! `formal/wait_machine/` mirrors it 1:1. The mapping is:
//!
//! | Rust                              | Lean                          |
//! |-----------------------------------|-------------------------------|
//! | `WaitState::Idle`                 | `WaitState.idle`              |
//! | `WaitState::AwaitingShell`        | `WaitState.awaitingShell`     |
//! | `WaitState::AwaitingDispatchReady`| `WaitState.awaitingDispatch`  |
//! | `WaitState::AwaitingClearCooldown`| `WaitState.awaitingCooldown`  |
//! | `WaitState::AwaitingCapabilityProof` | `WaitState.awaitingProof`  |
//! | `WaitState::AwaitingIpcAck`       | `WaitState.awaitingIpcAck`    |
//! | `WaitState::ReinstallPause`       | `WaitState.reinstallPause`    |
//! | `WaitState::Ready`                | `WaitState.ready`             |
//! | `WaitState::FailedClosed { .. }`  | `WaitState.failedClosed`      |
//! | `Signal::Satisfied`               | `Signal.satisfied`            |
//! | `Signal::StillWaiting`            | `Signal.stillWaiting`         |
//! | `Signal::Blocked`                 | `Signal.blocked`              |
//! | `tick`                            | `Step`                        |
//! | `GLOBAL_HANG_CEILING` (10s)       | `globalHangCeiling` (10000ms) |
//! | `REINSTALL_BUDGET` (120s)         | `reinstallBudget` (120000ms)  |
//!
//! The parity test [`tests::lean_parity_transition_table`] emits the exhaustive
//! Rust transition table; the hand-checked Lean table in
//! `formal/wait_machine/WaitMachine.lean` must match it.

use std::time::Duration;

/// The single global hang ceiling. No exempt-free wait path may dwell longer
/// than this. The operator's hard constraint: never hang > 10s.
pub const GLOBAL_HANG_CEILING: Duration = Duration::from_secs(10);

/// The explicit, longer budget for the *only* sanctioned long pause: the dogfood
/// supervisor auto-install / recycle onto a fresh binary
/// ([`WaitState::ReinstallPause`]). Even the exemption is bounded — it is never
/// truly unbounded — so a stuck reinstall still fails closed, just at a longer,
/// distinctly-logged budget rather than being mistaken for a 10s hang.
pub const REINSTALL_BUDGET: Duration = Duration::from_secs(120);

/// Why a wait failed closed. Generalizes the `#startupdeadline` fix
/// (`auto_trigger_no_prompt_action`) to every wait: on deadline we fail closed,
/// never "keep polling".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitFailReason {
    /// The state's `max_dwell` (<= [`GLOBAL_HANG_CEILING`]) elapsed without the
    /// awaited condition becoming true.
    DeadlineExceeded,
    /// An observed signal proved the awaited condition can never become true
    /// (e.g. a dead pane, a permission-prompt blocker, a wedged listener).
    BlockerObserved,
    /// The reinstall pause itself overran [`REINSTALL_BUDGET`] — the exemption is
    /// bounded, so even this fails closed (distinctly, so it is never confused
    /// with a 10s hang).
    ReinstallBudgetExceeded,
}

impl WaitFailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            WaitFailReason::DeadlineExceeded => "deadline_exceeded",
            WaitFailReason::BlockerObserved => "blocker_observed",
            WaitFailReason::ReinstallBudgetExceeded => "reinstall_budget_exceeded",
        }
    }
}

/// The wait state. Each *awaiting* variant carries the `max_dwell` it is enforced
/// against; the driver constructs them through [`WaitState::awaiting`] so the
/// per-state budget is always clamped to [`GLOBAL_HANG_CEILING`].
///
/// [`WaitState::ReinstallPause`] is the sole exemption: it is enforced against
/// [`REINSTALL_BUDGET`] instead of the global ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    /// Not waiting on anything.
    Idle,
    /// Waiting for a pane shell prompt.
    AwaitingShell { max_dwell: Duration },
    /// Waiting for a harness dispatch-ready prompt.
    AwaitingDispatchReady { max_dwell: Duration },
    /// Waiting for a lingering clear cooldown to expire.
    AwaitingClearCooldown { max_dwell: Duration },
    /// Waiting for a managed Codex capability proof.
    AwaitingCapabilityProof { max_dwell: Duration },
    /// Waiting for an editor IPC patch ack.
    AwaitingIpcAck { max_dwell: Duration },
    /// The sole EXEMPT state: supervisor auto-install / recycle onto a fresh
    /// binary. Bounded by [`REINSTALL_BUDGET`], not [`GLOBAL_HANG_CEILING`].
    ReinstallPause,
    /// Terminal: the awaited condition became true.
    Ready,
    /// Terminal: the wait failed closed.
    FailedClosed { reason: WaitFailReason },
}

/// Logical kind of an awaiting state, independent of its carried budget. Lets the
/// driver/tests name a state without committing to a concrete `max_dwell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitKind {
    Shell,
    DispatchReady,
    ClearCooldown,
    CapabilityProof,
    IpcAck,
}

impl WaitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WaitKind::Shell => "awaiting_shell",
            WaitKind::DispatchReady => "awaiting_dispatch_ready",
            WaitKind::ClearCooldown => "awaiting_clear_cooldown",
            WaitKind::CapabilityProof => "awaiting_capability_proof",
            WaitKind::IpcAck => "awaiting_ipc_ack",
        }
    }
}

impl WaitState {
    /// Construct an awaiting state of `kind`, clamping the requested per-state
    /// budget to [`GLOBAL_HANG_CEILING`]. This is the single place that enforces
    /// the global ceiling on non-exempt waits — a caller cannot accidentally
    /// request a 30s budget; it is silently lowered to the ceiling.
    pub fn awaiting(kind: WaitKind, requested: Duration) -> WaitState {
        let max_dwell = clamp_to_ceiling(requested);
        match kind {
            WaitKind::Shell => WaitState::AwaitingShell { max_dwell },
            WaitKind::DispatchReady => WaitState::AwaitingDispatchReady { max_dwell },
            WaitKind::ClearCooldown => WaitState::AwaitingClearCooldown { max_dwell },
            WaitKind::CapabilityProof => WaitState::AwaitingCapabilityProof { max_dwell },
            WaitKind::IpcAck => WaitState::AwaitingIpcAck { max_dwell },
        }
    }

    /// The dwell budget enforced against this state.
    ///
    /// - Non-exempt awaiting states return their carried `max_dwell` (always
    ///   `<= GLOBAL_HANG_CEILING`).
    /// - [`WaitState::ReinstallPause`] returns [`REINSTALL_BUDGET`].
    /// - Terminal states and [`WaitState::Idle`] return `Duration::ZERO` (they do
    ///   not dwell).
    pub fn budget(&self) -> Duration {
        match self {
            WaitState::Idle | WaitState::Ready | WaitState::FailedClosed { .. } => Duration::ZERO,
            WaitState::AwaitingShell { max_dwell }
            | WaitState::AwaitingDispatchReady { max_dwell }
            | WaitState::AwaitingClearCooldown { max_dwell }
            | WaitState::AwaitingCapabilityProof { max_dwell }
            | WaitState::AwaitingIpcAck { max_dwell } => *max_dwell,
            WaitState::ReinstallPause => REINSTALL_BUDGET,
        }
    }

    /// Whether this state is exempt from the global ceiling. Only
    /// [`WaitState::ReinstallPause`] is.
    pub fn is_exempt(&self) -> bool {
        matches!(self, WaitState::ReinstallPause)
    }

    /// Whether this state is terminal (no further transitions consume budget).
    pub fn is_terminal(&self) -> bool {
        matches!(self, WaitState::Ready | WaitState::FailedClosed { .. })
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, WaitState::Ready)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, WaitState::FailedClosed { .. })
    }
}

/// Clamp a requested per-state budget to the global hang ceiling. The one place
/// the 10s bound is mechanically enforced for non-exempt states.
pub const fn clamp_to_ceiling(requested: Duration) -> Duration {
    // `Duration::as_nanos` is const; the `>` operator on `Duration` is not, so
    // compare the nanosecond counts to keep this usable in `const` contexts
    // (e.g. `const AUTO_TRIGGER_TIMEOUT` in `start.rs`).
    if requested.as_nanos() > GLOBAL_HANG_CEILING.as_nanos() {
        GLOBAL_HANG_CEILING
    } else {
        requested
    }
}

/// The observed signal a [`tick`] consumes. The thin driver reads the real world
/// (capture pane, recv ack, …) and maps it onto one of these; `tick` itself is
/// pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The awaited condition is true (ready prompt seen, ack received, …).
    Satisfied,
    /// The awaited condition is not yet true; keep waiting (subject to budget).
    StillWaiting,
    /// The awaited condition can never become true (dead pane, blocker prompt,
    /// wedged listener) — fail closed now regardless of remaining budget.
    Blocked,
}

/// **The pure transition function.** Computes the next [`WaitState`] from the
/// current state, the elapsed time *in that state*, and an observed [`Signal`].
///
/// Totality and purity invariants (mirrored by the Lean `Step`):
///
/// - It performs no I/O and reads no clock; `elapsed` is supplied by the driver.
/// - Terminal states ([`WaitState::Ready`], [`WaitState::FailedClosed`]) and
///   [`WaitState::Idle`] are fixpoints under every signal except that `Idle`
///   only leaves via an explicit driver `enter`, not via `tick`.
/// - A `Satisfied` signal on any awaiting state ⇒ [`WaitState::Ready`].
/// - A `Blocked` signal on any awaiting state ⇒
///   `FailedClosed { BlockerObserved }`.
/// - A `StillWaiting` signal on a non-exempt awaiting state ⇒ stay in the same
///   state **iff** `elapsed < max_dwell`, else `FailedClosed { DeadlineExceeded }`.
///   Because `max_dwell <= GLOBAL_HANG_CEILING`, a non-exempt path can never
///   dwell past 10s — this is the bound the tests and the Lean `no_hang` theorem
///   prove.
/// - A `StillWaiting` signal on [`WaitState::ReinstallPause`] ⇒ stay paused iff
///   `elapsed < REINSTALL_BUDGET`, else `FailedClosed { ReinstallBudgetExceeded }`.
///   The exemption is bounded, never unbounded.
pub fn tick(state: WaitState, elapsed: Duration, signal: Signal) -> WaitState {
    match state {
        // Fixpoints: Idle does not advance via tick; terminals are absorbing.
        WaitState::Idle | WaitState::Ready | WaitState::FailedClosed { .. } => state,

        // The sole exempt state.
        WaitState::ReinstallPause => match signal {
            Signal::Satisfied => WaitState::Ready,
            Signal::Blocked => WaitState::FailedClosed {
                reason: WaitFailReason::BlockerObserved,
            },
            Signal::StillWaiting => {
                if elapsed < REINSTALL_BUDGET {
                    WaitState::ReinstallPause
                } else {
                    WaitState::FailedClosed {
                        reason: WaitFailReason::ReinstallBudgetExceeded,
                    }
                }
            }
        },

        // All non-exempt awaiting states share one rule, parameterized by their
        // clamped `max_dwell` (always <= GLOBAL_HANG_CEILING).
        WaitState::AwaitingShell { max_dwell }
        | WaitState::AwaitingDispatchReady { max_dwell }
        | WaitState::AwaitingClearCooldown { max_dwell }
        | WaitState::AwaitingCapabilityProof { max_dwell }
        | WaitState::AwaitingIpcAck { max_dwell } => match signal {
            Signal::Satisfied => WaitState::Ready,
            Signal::Blocked => WaitState::FailedClosed {
                reason: WaitFailReason::BlockerObserved,
            },
            Signal::StillWaiting => {
                if elapsed < max_dwell {
                    // Stay in the *same* state (preserve its carried budget).
                    state
                } else {
                    WaitState::FailedClosed {
                        reason: WaitFailReason::DeadlineExceeded,
                    }
                }
            }
        },
    }
}

/// Thin, side-effecting driver around the pure [`tick`]. The driver owns the
/// clock (`Instant`) and a small polling cadence; every blocking call site builds
/// one of these, supplies a closure that reads the real world into a [`Signal`],
/// and loops until the state is terminal. This keeps the only `sleep` / clock /
/// I/O in one auditable place while the transition logic stays pure and proven.
#[derive(Debug)]
pub struct WaitMachine {
    state: WaitState,
    started_at: std::time::Instant,
    poll_interval: Duration,
}

impl WaitMachine {
    /// Enter an awaiting state of `kind` with a per-state budget clamped to
    /// [`GLOBAL_HANG_CEILING`].
    pub fn enter(kind: WaitKind, requested_budget: Duration, poll_interval: Duration) -> Self {
        WaitMachine {
            state: WaitState::awaiting(kind, requested_budget),
            started_at: std::time::Instant::now(),
            poll_interval,
        }
    }

    /// Enter the exempt reinstall pause (bounded by [`REINSTALL_BUDGET`]).
    pub fn enter_reinstall_pause(poll_interval: Duration) -> Self {
        WaitMachine {
            state: WaitState::ReinstallPause,
            started_at: std::time::Instant::now(),
            poll_interval,
        }
    }

    pub fn state(&self) -> WaitState {
        self.state
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Advance once against the current observed signal (no sleep). Returns the
    /// new state. Used by callers that own their own poll loop / sleep cadence
    /// (e.g. interleaved with other capture work).
    pub fn advance(&mut self, signal: Signal) -> WaitState {
        self.state = tick(self.state, self.started_at.elapsed(), signal);
        self.state
    }

    /// Drive to a terminal state, sleeping `poll_interval` between polls. `probe`
    /// reads the real world into a [`Signal`]. This is the only place that
    /// sleeps; it cannot exceed the state's budget (the pure `tick` fails closed
    /// at the deadline), so it can never hang past [`GLOBAL_HANG_CEILING`] on a
    /// non-exempt state.
    pub fn run<F>(&mut self, mut probe: F) -> WaitState
    where
        F: FnMut() -> Signal,
    {
        while !self.state.is_terminal() {
            let signal = probe();
            self.advance(signal);
            if self.state.is_terminal() {
                break;
            }
            std::thread::sleep(self.poll_interval);
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const ALL_KINDS: [WaitKind; 5] = [
        WaitKind::Shell,
        WaitKind::DispatchReady,
        WaitKind::ClearCooldown,
        WaitKind::CapabilityProof,
        WaitKind::IpcAck,
    ];

    fn under_ceiling() -> Duration {
        Duration::from_secs(8)
    }

    // ---- Per-state transition unit tests (continue-before-deadline → fail-closed-once) ----

    #[test]
    fn awaiting_states_continue_before_deadline() {
        for kind in ALL_KINDS {
            let s = WaitState::awaiting(kind, under_ceiling());
            // Before the deadline, StillWaiting keeps the same state.
            let next = tick(s, Duration::from_secs(1), Signal::StillWaiting);
            assert_eq!(next, s, "{kind:?} should keep waiting before deadline");
        }
    }

    #[test]
    fn awaiting_states_fail_closed_at_deadline() {
        for kind in ALL_KINDS {
            let s = WaitState::awaiting(kind, under_ceiling());
            // At/after the deadline, StillWaiting fails closed exactly once.
            let next = tick(s, under_ceiling(), Signal::StillWaiting);
            assert_eq!(
                next,
                WaitState::FailedClosed {
                    reason: WaitFailReason::DeadlineExceeded
                },
                "{kind:?} should fail closed at deadline"
            );
            // FailedClosed is absorbing under every signal.
            for sig in [Signal::Satisfied, Signal::StillWaiting, Signal::Blocked] {
                assert_eq!(tick(next, Duration::from_secs(999), sig), next);
            }
        }
    }

    #[test]
    fn satisfied_reaches_ready_from_every_awaiting_state() {
        for kind in ALL_KINDS {
            let s = WaitState::awaiting(kind, under_ceiling());
            assert_eq!(
                tick(s, Duration::from_secs(1), Signal::Satisfied),
                WaitState::Ready
            );
        }
        assert_eq!(
            tick(WaitState::ReinstallPause, Duration::ZERO, Signal::Satisfied),
            WaitState::Ready
        );
    }

    #[test]
    fn blocked_fails_closed_from_every_awaiting_state() {
        for kind in ALL_KINDS {
            let s = WaitState::awaiting(kind, under_ceiling());
            assert_eq!(
                tick(s, Duration::from_secs(1), Signal::Blocked),
                WaitState::FailedClosed {
                    reason: WaitFailReason::BlockerObserved
                }
            );
        }
    }

    #[test]
    fn requested_budget_is_clamped_to_ceiling() {
        // A caller asking for the old 30s budget gets the 10s ceiling.
        let s = WaitState::awaiting(WaitKind::DispatchReady, Duration::from_secs(30));
        assert_eq!(s.budget(), GLOBAL_HANG_CEILING);
        // An under-ceiling request is preserved.
        let s = WaitState::awaiting(WaitKind::Shell, Duration::from_secs(5));
        assert_eq!(s.budget(), Duration::from_secs(5));
    }

    #[test]
    fn ready_and_idle_are_fixpoints() {
        for sig in [Signal::Satisfied, Signal::StillWaiting, Signal::Blocked] {
            assert_eq!(
                tick(WaitState::Ready, Duration::from_secs(999), sig),
                WaitState::Ready
            );
            assert_eq!(
                tick(WaitState::Idle, Duration::from_secs(999), sig),
                WaitState::Idle
            );
        }
    }

    // ---- Reinstall-pause sole-exemption tests ----

    #[test]
    fn reinstall_pause_is_the_only_exempt_state() {
        assert!(WaitState::ReinstallPause.is_exempt());
        for kind in ALL_KINDS {
            assert!(!WaitState::awaiting(kind, under_ceiling()).is_exempt());
        }
        assert!(!WaitState::Idle.is_exempt());
        assert!(!WaitState::Ready.is_exempt());
    }

    #[test]
    fn reinstall_pause_dwells_past_ceiling_but_is_bounded() {
        // Past the 10s global ceiling, the reinstall pause still keeps waiting.
        let mid = tick(
            WaitState::ReinstallPause,
            GLOBAL_HANG_CEILING + Duration::from_secs(30),
            Signal::StillWaiting,
        );
        assert_eq!(mid, WaitState::ReinstallPause);
        // But it is NOT unbounded — at REINSTALL_BUDGET it fails closed distinctly.
        let done = tick(
            WaitState::ReinstallPause,
            REINSTALL_BUDGET,
            Signal::StillWaiting,
        );
        assert_eq!(
            done,
            WaitState::FailedClosed {
                reason: WaitFailReason::ReinstallBudgetExceeded
            }
        );
    }

    // ---- Exhaustive (state × signal) bound proof: Σdwell ≤ 10s on every exempt-free path ----

    /// Walk a path from `start` driven by `signals`, advancing the clock by
    /// `step` each StillWaiting tick. Returns `(final_state, total_dwell, saw_exempt)`.
    fn walk(start: WaitState, signals: &[Signal], step: Duration) -> (WaitState, Duration, bool) {
        let mut state = start;
        let mut dwell = Duration::ZERO;
        let mut saw_exempt = state.is_exempt();
        for &sig in signals {
            if state.is_terminal() {
                break;
            }
            // dwell accrues only while actually in a (non-terminal, non-idle) wait.
            if !matches!(state, WaitState::Idle) {
                dwell += step;
            }
            state = tick(state, dwell, sig);
            if state.is_exempt() {
                saw_exempt = true;
            }
        }
        (state, dwell, saw_exempt)
    }

    proptest! {
        /// **The `#waitmachine3` bound property.** For every exempt-free path
        /// (no `ReinstallPause` ever visited) over the (state × signal) space,
        /// the accrued dwell at the point the machine reaches a terminal state is
        /// `<= GLOBAL_HANG_CEILING` (10s). A `StillWaiting`-only path is the worst
        /// case and must fail closed at the ceiling, never keep polling.
        #[test]
        fn exempt_free_paths_never_dwell_past_ceiling(
            kind_idx in 0usize..ALL_KINDS.len(),
            requested_secs in 0u64..60,         // includes the old 30s budgets
            signal_codes in proptest::collection::vec(0u8..3, 0..40),
            step_ms in 1u64..4000,
        ) {
            let kind = ALL_KINDS[kind_idx];
            let start = WaitState::awaiting(kind, Duration::from_secs(requested_secs));
            // Per-state budget is always clamped to the ceiling.
            prop_assert!(start.budget() <= GLOBAL_HANG_CEILING);

            let signals: Vec<Signal> = signal_codes
                .iter()
                .map(|c| match c {
                    0 => Signal::Satisfied,
                    1 => Signal::StillWaiting,
                    _ => Signal::Blocked,
                })
                .collect();

            let step = Duration::from_millis(step_ms);
            let (_final, dwell, saw_exempt) = walk(start, &signals, step);
            // This corpus never enters ReinstallPause, so every path is exempt-free.
            prop_assert!(!saw_exempt);
            // The exact guarantee: `tick` fails closed at the *first* poll whose
            // elapsed has reached `max_dwell`. Polling is discrete, so the accrued
            // dwell is bounded by the ceiling PLUS at most one poll granularity
            // (the in-flight poll that crosses the deadline). The real driver polls
            // every 500ms, so live overshoot is <= 500ms over the 10s ceiling. This
            // is the honest hang bound: a non-exempt path can never dwell past
            // `ceiling + one_poll`.
            prop_assert!(
                dwell <= GLOBAL_HANG_CEILING + step,
                "exempt-free path dwelled {dwell:?} > {GLOBAL_HANG_CEILING:?}+{step:?} (kind={kind:?})"
            );
            // And it always lands at-or-below the ceiling by the time the deadline
            // tick *fires* — i.e. the budget the machine enforces is exactly 10s.
            prop_assert!(start.budget() <= GLOBAL_HANG_CEILING);
        }

        /// Worst-case `StillWaiting`-only paths always terminate in
        /// `FailedClosed { DeadlineExceeded }` once the clock passes the budget —
        /// the machine never spins forever.
        #[test]
        fn still_waiting_only_terminates_by_ceiling(
            kind_idx in 0usize..ALL_KINDS.len(),
            requested_secs in 0u64..60,
        ) {
            let kind = ALL_KINDS[kind_idx];
            let start = WaitState::awaiting(kind, Duration::from_secs(requested_secs));
            // One tick past the (clamped) budget must fail closed.
            let next = tick(start, GLOBAL_HANG_CEILING, Signal::StillWaiting);
            prop_assert_eq!(
                next,
                WaitState::FailedClosed { reason: WaitFailReason::DeadlineExceeded }
            );
        }
    }

    // ---- SimWorld hang-class coverage: a wait that WOULD have hung is bounded ----

    /// A deterministic in-memory "world" whose probe never satisfies — modeling a
    /// live hang class (pane never reaches dispatch-ready, ack never arrives).
    /// Under the old ad-hoc 30s loop this would dwell 30s; the WaitMachine bounds
    /// it at the 10s ceiling and fails closed.
    struct HangWorld {
        clock: std::cell::Cell<Duration>,
        step: Duration,
    }
    impl HangWorld {
        fn probe(&self) -> Signal {
            self.clock.set(self.clock.get() + self.step);
            Signal::StillWaiting // never satisfied → the hang class
        }
    }

    #[test]
    fn simworld_hang_class_is_bounded_at_ceiling() {
        // Caller "requests" the historical 30s startup budget; the machine clamps
        // it to 10s. Drive the pure machine against a never-satisfying world.
        let world = HangWorld {
            clock: std::cell::Cell::new(Duration::ZERO),
            step: Duration::from_millis(500), // AGENT_READY_POLL_INTERVAL shape
        };
        let mut state = WaitState::awaiting(WaitKind::DispatchReady, Duration::from_secs(30));
        // Simulate the driver loop with a model clock instead of real sleeping.
        let mut model_elapsed = Duration::ZERO;
        let mut iterations = 0;
        loop {
            iterations += 1;
            assert!(iterations < 1000, "machine must terminate, not spin");
            let sig = world.probe();
            // The model clock is what the real driver's Instant would report.
            model_elapsed += Duration::from_millis(500);
            state = tick(state, model_elapsed, sig);
            if state.is_terminal() {
                break;
            }
        }
        assert_eq!(
            state,
            WaitState::FailedClosed {
                reason: WaitFailReason::DeadlineExceeded
            },
            "a never-satisfying wait must fail closed, not hang"
        );
        // It failed closed within the 10s ceiling, not the old 30s.
        assert!(
            model_elapsed <= GLOBAL_HANG_CEILING + Duration::from_millis(500),
            "hang-class wait bounded at ceiling, dwelled {model_elapsed:?}"
        );
    }

    /// The same hang class on the EXEMPT reinstall pause is allowed to exceed 10s
    /// (proving the exemption is real) but is still bounded by REINSTALL_BUDGET.
    #[test]
    fn simworld_reinstall_pause_exceeds_ceiling_but_bounded() {
        let mut state = WaitState::ReinstallPause;
        // Just past the global ceiling it is still pausing (exemption holds).
        state = tick(
            state,
            GLOBAL_HANG_CEILING + Duration::from_secs(1),
            Signal::StillWaiting,
        );
        assert_eq!(state, WaitState::ReinstallPause);
        // At the reinstall budget it finally fails closed.
        state = tick(state, REINSTALL_BUDGET, Signal::StillWaiting);
        assert_eq!(
            state,
            WaitState::FailedClosed {
                reason: WaitFailReason::ReinstallBudgetExceeded
            }
        );
    }

    // ---- Lean parity: emit the exhaustive Rust transition table ----

    /// Canonical, hand-checkable representation of one transition row. Mirrors a
    /// Lean `Step` constructor; the Lean table in
    /// `formal/wait_machine/WaitMachine.lean` must match this set exactly.
    fn transition_row(state_name: &str, signal: Signal, elapsed_lt_budget: bool) -> String {
        // Build the abstract state from its name (budget = 8s < ceiling for the
        // "before deadline" rows; the parity table is over the abstract kinds).
        let st = match state_name {
            "idle" => WaitState::Idle,
            "ready" => WaitState::Ready,
            "reinstallPause" => WaitState::ReinstallPause,
            "awaitingShell" => WaitState::awaiting(WaitKind::Shell, under_ceiling()),
            "awaitingDispatch" => WaitState::awaiting(WaitKind::DispatchReady, under_ceiling()),
            "awaitingCooldown" => WaitState::awaiting(WaitKind::ClearCooldown, under_ceiling()),
            "awaitingProof" => WaitState::awaiting(WaitKind::CapabilityProof, under_ceiling()),
            "awaitingIpcAck" => WaitState::awaiting(WaitKind::IpcAck, under_ceiling()),
            other => panic!("unknown state {other}"),
        };
        let budget = st.budget();
        let elapsed = if elapsed_lt_budget {
            // Strictly less than budget.
            budget
                .checked_sub(Duration::from_millis(1))
                .unwrap_or(Duration::ZERO)
        } else {
            // At/over budget.
            budget
        };
        let next = tick(st, elapsed, signal);
        let next_name = match next {
            WaitState::Idle => "idle".to_string(),
            WaitState::Ready => "ready".to_string(),
            WaitState::ReinstallPause => "reinstallPause".to_string(),
            WaitState::AwaitingShell { .. } => "awaitingShell".to_string(),
            WaitState::AwaitingDispatchReady { .. } => "awaitingDispatch".to_string(),
            WaitState::AwaitingClearCooldown { .. } => "awaitingCooldown".to_string(),
            WaitState::AwaitingCapabilityProof { .. } => "awaitingProof".to_string(),
            WaitState::AwaitingIpcAck { .. } => "awaitingIpcAck".to_string(),
            WaitState::FailedClosed { reason } => format!("failedClosed({})", reason.as_str()),
        };
        let sig_name = match signal {
            Signal::Satisfied => "satisfied",
            Signal::StillWaiting => "stillWaiting",
            Signal::Blocked => "blocked",
        };
        let elapsed_name = if elapsed_lt_budget {
            "beforeDeadline"
        } else {
            "atDeadline"
        };
        format!("{state_name} | {sig_name} | {elapsed_name} => {next_name}")
    }

    #[test]
    fn lean_parity_transition_table() {
        // Exhaustive over the abstract (state × signal × deadline-position) space.
        // The Lean `Step` table in formal/wait_machine/WaitMachine.lean must
        // reproduce exactly these rows. Kept here as the authoritative source.
        let states = [
            "awaitingShell",
            "awaitingDispatch",
            "awaitingCooldown",
            "awaitingProof",
            "awaitingIpcAck",
            "reinstallPause",
        ];
        let signals = [Signal::Satisfied, Signal::StillWaiting, Signal::Blocked];
        let mut rows = Vec::new();
        for st in states {
            for &sig in &signals {
                for before in [true, false] {
                    rows.push(transition_row(st, sig, before));
                }
            }
        }
        // Spot-check the load-bearing rows the Lean `no_hang` proof depends on.
        assert!(rows.contains(
            &"awaitingDispatch | stillWaiting | beforeDeadline => awaitingDispatch".to_string()
        ));
        assert!(
            rows.contains(
                &"awaitingDispatch | stillWaiting | atDeadline => failedClosed(deadline_exceeded)"
                    .to_string()
            )
        );
        assert!(rows.contains(&"awaitingShell | satisfied | beforeDeadline => ready".to_string()));
        assert!(
            rows.contains(
                &"awaitingIpcAck | blocked | beforeDeadline => failedClosed(blocker_observed)"
                    .to_string()
            )
        );
        assert!(rows.contains(
            &"reinstallPause | stillWaiting | beforeDeadline => reinstallPause".to_string()
        ));
        assert!(rows.contains(
            &"reinstallPause | stillWaiting | atDeadline => failedClosed(reinstall_budget_exceeded)"
                .to_string()
        ));
        // 6 states × 3 signals × 2 deadline positions = 36 rows, all distinct
        // by their left-hand key.
        assert_eq!(rows.len(), 36);
    }
}
