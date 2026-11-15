//! Process-scoped retry of the supervisor's durable registry row.
//!
//! The idle-watch tick is the external event source. A failed startup
//! publication is one Lazily retained intent (`#implementlazilyowned`):
//! `lazily::EffectCasCore` owns its attempt number, its due tick, and its
//! receipt. Each idle tick claims the intent if it is due, runs the idempotent
//! durable sink outside the core lock, and records the outcome: success is the
//! intent's receipt, a failure is `Retryable` due after a bounded exponential
//! backoff counted in idle ticks.

use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use agent_doc_state_scope::ProcessScope;
use lazily::{
    EffectCasClaim, EffectCasCore, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
    Source, ThreadSafeContext,
};

const MAX_RETRY_DELAY_TICKS: u64 = 32;

const PUBLICATION_GENERATION: u64 = 0;
const PUBLICATION_EFFECT: &str = "supervisor-registry-publication";

/// Key: the registry row. Authority: the row's publication state. Logical
/// time: the idle tick.
type PublicationCore = EffectCasCore<(), String, (), String, String>;

fn lock(core: &Mutex<PublicationCore>) -> MutexGuard<'_, PublicationCore> {
    core.lock()
}

/// Lazily-backed process state for a failed supervisor registry publication.
pub struct RegistryPublicationRetryState {
    ctx: ThreadSafeContext,
    pending: Source<bool>,
    idle_tick: Source<u64>,
    core: Arc<Mutex<PublicationCore>>,
}

impl RegistryPublicationRetryState {
    /// Join the supervisor process graph.
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let pending = ctx.source(false);
        let idle_tick = ctx.source(0_u64);
        Self {
            ctx,
            pending,
            idle_tick,
            core: Arc::new(Mutex::new(PublicationCore::new(PUBLICATION_GENERATION))),
        }
    }

    /// Record that startup left a durable registry publication pending: desire
    /// the publication as the Lazily intent the idle ticks claim.
    pub fn request_retry(&self) {
        {
            let mut core = lock(&self.core);
            if core.state(&()).is_none()
                && let Ok(identity) = EffectIdentity::new(format!("{PUBLICATION_EFFECT}#1"))
            {
                let accepted = core
                    .upsert_desired(EffectIntent {
                        identity,
                        epoch: 1,
                        key: (),
                        expected: "unpublished".to_string(),
                        plan: EffectPlan::single(()),
                    })
                    .1;
                debug_assert!(matches!(accepted, EffectCasUpsert::Accepted));
            }
        }
        self.ctx.set(&self.pending, true);
    }

    /// Publish one external idle-watch tick into the process graph.
    pub fn observe_idle_tick(&self) {
        let next = self.ctx.get(&self.idle_tick).saturating_add(1);
        self.ctx.set(&self.idle_tick, next);
    }

    /// Drive the idempotent durable sink whenever Lazily makes the retained
    /// publication claimable.
    pub fn on_due(
        &self,
        sink: impl Fn(u32) -> Result<(), String> + Send + Sync + 'static,
    ) -> lazily::Effect {
        let pending = self.pending;
        let idle_tick = self.idle_tick;
        let core = Arc::clone(&self.core);
        self.ctx.effect(move |ctx| {
            let requested = ctx.get(&pending);
            let tick = ctx.get(&idle_tick);
            // The failed startup publication is retried on the first idle tick,
            // never synchronously on the PTY/startup path.
            if !requested || tick == 0 {
                return;
            }
            let attempt = match lock(&core).claim(&(), PUBLICATION_GENERATION, tick).1 {
                EffectCasClaim::Claimed(attempt) => attempt,
                EffectCasClaim::Waiting { .. }
                | EffectCasClaim::Busy
                | EffectCasClaim::Empty
                | EffectCasClaim::StaleGeneration { .. } => return,
            };
            let number = attempt.identity.attempt;
            let outcome = sink(number);
            let mut core = lock(&core);
            match outcome {
                Ok(()) => {
                    core.record_applied(
                        &(),
                        &attempt.identity,
                        "published".to_string(),
                        "published".to_string(),
                    );
                }
                Err(error) => {
                    core.record_retryable(
                        &(),
                        &attempt.identity,
                        error,
                        tick.saturating_add(retry_delay_ticks(number)),
                    );
                }
            }
        })
    }
}

fn retry_delay_ticks(attempt: u32) -> u64 {
    1_u64
        .checked_shl(attempt.saturating_sub(1).min(5))
        .unwrap_or(MAX_RETRY_DELAY_TICKS)
        .min(MAX_RETRY_DELAY_TICKS)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn failed_publication_retries_on_idle_ticks_with_backoff_until_success() {
        let scope = ProcessScope::new();
        let state = RegistryPublicationRetryState::new_in(&scope);
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&attempts);
        let _effect = state.on_due(move |attempt| {
            seen.lock().unwrap().push(attempt);
            (attempt >= 3)
                .then_some(())
                .ok_or_else(|| "database is locked".to_string())
        });

        state.request_retry();
        assert!(attempts.lock().unwrap().is_empty());

        state.observe_idle_tick();
        assert_eq!(*attempts.lock().unwrap(), vec![1]);
        state.observe_idle_tick();
        assert_eq!(*attempts.lock().unwrap(), vec![1, 2]);
        state.observe_idle_tick();
        assert_eq!(*attempts.lock().unwrap(), vec![1, 2]);
        state.observe_idle_tick();
        assert_eq!(*attempts.lock().unwrap(), vec![1, 2, 3]);

        for _ in 0..64 {
            state.observe_idle_tick();
        }
        assert_eq!(*attempts.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn no_startup_failure_never_arms_the_sink() {
        let scope = ProcessScope::new();
        let state = RegistryPublicationRetryState::new_in(&scope);
        let attempts = Arc::new(Mutex::new(0_u32));
        let seen = Arc::clone(&attempts);
        let _effect = state.on_due(move |_| {
            *seen.lock().unwrap() += 1;
            Ok(())
        });

        for _ in 0..64 {
            state.observe_idle_tick();
        }
        assert_eq!(*attempts.lock().unwrap(), 0);
    }
}
