//! Test-only synchronization helpers for orchestration tests that mutate
//! process-global state (environment variables).
//!
//! Mirrors the main crate's `test_support`. Each crate's tests compile into a
//! separate test executable, so a crate-local `TEST_ENV_LOCK` is sufficient to
//! serialize env-mutating tests within *this* crate's test process — no
//! cross-crate sharing is required (or possible) for a `#[cfg(test)]` static.

use std::sync::MutexGuard;

thread_local! {
    static PROCESS_GLOBAL_LOCK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// RAII guard for the process-global test lock. Reentrant within a thread: a
/// nested `env_lock()` returns a guard that holds no inner `MutexGuard`, so the
/// outer guard owns the actual lock for the whole nesting.
pub struct ProcessGlobalLockGuard {
    _guard: Option<MutexGuard<'static, ()>>,
}

impl Drop for ProcessGlobalLockGuard {
    fn drop(&mut self) {
        PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
            let current = depth.get();
            debug_assert!(current > 0, "process-global test lock depth underflow");
            depth.set(current.saturating_sub(1));
        });
    }
}

/// Acquire the process-global env lock, serializing env-mutating tests within
/// this crate's test binary. Reentrant on the same thread.
pub fn env_lock() -> ProcessGlobalLockGuard {
    let already_held = PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current > 0
    });
    if already_held {
        return ProcessGlobalLockGuard { _guard: None };
    }

    let guard = crate::harness_prompt::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ProcessGlobalLockGuard {
        _guard: Some(guard),
    }
}
