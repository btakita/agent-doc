//! Process-scoped retry projection for the supervisor's durable registry row.
//!
//! The idle-watch tick is the external event source. A failed durable sink receipt
//! stays pending, and a computed projection schedules the next idempotent attempt
//! with bounded exponential backoff until one receipt succeeds.

use agent_doc_state_scope::ProcessScope;
use lazily::{Computed, Source, ThreadSafeContext};

const MAX_RETRY_DELAY_TICKS: u64 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PublicationReceipt {
    attempt: u32,
    completed: bool,
    next_retry_tick: u64,
}

impl Default for PublicationReceipt {
    fn default() -> Self {
        Self {
            attempt: 0,
            completed: false,
            // The failed startup publication is retried on the first idle tick,
            // never synchronously on the PTY/startup path.
            next_retry_tick: 1,
        }
    }
}

/// Lazily-backed process state for a failed supervisor registry publication.
pub struct RegistryPublicationRetryState {
    ctx: ThreadSafeContext,
    pending: Source<bool>,
    idle_tick: Source<u64>,
    due_attempt: Computed<Option<u32>>,
    receipt: Source<PublicationReceipt>,
}

impl RegistryPublicationRetryState {
    /// Join the supervisor process graph.
    pub fn new_in(scope: &ProcessScope) -> Self {
        let ctx = scope.ctx().clone();
        let pending = ctx.source(false);
        let idle_tick = ctx.source(0_u64);
        let receipt = ctx.source(PublicationReceipt::default());
        let due_attempt = ctx.computed(move |ctx| {
            let receipt = ctx.get(&receipt);
            (ctx.get(&pending)
                && !receipt.completed
                && ctx.get(&idle_tick) >= receipt.next_retry_tick)
                .then_some(receipt.attempt.saturating_add(1))
        });
        Self {
            ctx,
            pending,
            idle_tick,
            due_attempt,
            receipt,
        }
    }

    /// Record that startup left a durable registry publication pending.
    pub fn request_retry(&self) {
        self.ctx.set(&self.pending, true);
    }

    /// Publish one external idle-watch tick into the process graph.
    pub fn observe_idle_tick(&self) {
        let next = self.ctx.get(&self.idle_tick).saturating_add(1);
        self.ctx.set(&self.idle_tick, next);
    }

    /// Drive the idempotent durable sink whenever the derived retry is due.
    pub fn on_due(
        &self,
        sink: impl Fn(u32) -> Result<(), String> + Send + Sync + 'static,
    ) -> lazily::Effect {
        let due_attempt = self.due_attempt;
        let idle_tick = self.idle_tick;
        let receipt = self.receipt;
        self.ctx.effect(move |ctx| {
            let Some(attempt) = ctx.get(&due_attempt) else {
                return;
            };
            let attempted_at = ctx.get(&idle_tick);
            let completed = sink(attempt).is_ok();
            let delay = retry_delay_ticks(attempt);
            ctx.set(
                &receipt,
                PublicationReceipt {
                    attempt,
                    completed,
                    next_retry_tick: attempted_at.saturating_add(delay),
                },
            );
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
