// Process-global lock for the main (`agent-doc` bin) test binary. The orchestration
// cluster moved to `agent-doc-orchestration`; each crate's tests compile into a separate test
// process, so the CLI shell's env-mutating tests serialize on this lock.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::LazyLock<parking_lot::Mutex<()>> =
    std::sync::LazyLock::new(|| parking_lot::Mutex::new(()));

#[cfg(test)]
thread_local! {
    static PROCESS_GLOBAL_LOCK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct ProcessGlobalLockGuard {
    _guard: Option<parking_lot::MutexGuard<'static, ()>>,
}

#[cfg(test)]
impl Drop for ProcessGlobalLockGuard {
    fn drop(&mut self) {
        PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
            let current = depth.get();
            debug_assert!(current > 0, "process-global test lock depth underflow");
            depth.set(current.saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn env_lock() -> ProcessGlobalLockGuard {
    let already_held = PROCESS_GLOBAL_LOCK_DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current > 0
    });
    if already_held {
        return ProcessGlobalLockGuard { _guard: None };
    }

    let guard = crate::test_support::TEST_ENV_LOCK.lock();
    ProcessGlobalLockGuard {
        _guard: Some(guard),
    }
}

#[cfg(test)]
pub(crate) struct ScopedCurrentDir {
    prev_cwd: std::path::PathBuf,
    _guard: ProcessGlobalLockGuard,
}

#[cfg(test)]
impl ScopedCurrentDir {
    pub(crate) fn set(path: &std::path::Path) -> Self {
        let guard = env_lock();
        let prev_cwd = std::env::current_dir()
            .ok()
            .filter(|cwd| cwd.exists())
            .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        std::env::set_current_dir(path).unwrap();
        Self {
            prev_cwd,
            _guard: guard,
        }
    }
}

#[cfg(test)]
impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
    }
}

#[cfg(test)]
mod tests {
    /// Prove the shared lock is obtainable again after a guard drops, without
    /// racing other parallel env-mutating tests. `try_lock().is_ok()` is flaky
    /// here: between our drop and the observation, another test in this binary
    /// can grab the shared lock, so a free observation is not guaranteed even
    /// though our guard released. Re-obtaining via the blocking `lock()` proves
    /// release deterministically (it would only hang if a guard leaked the lock
    /// forever) and yields immediately once no one holds it.
    fn assert_shared_lock_reobtainable() {
        let reobtained = crate::test_support::TEST_ENV_LOCK.lock();
        drop(reobtained);
    }

    #[test]
    fn env_lock_uses_harness_prompt_lock() {
        let guard = super::env_lock();
        assert!(
            crate::test_support::TEST_ENV_LOCK.try_lock().is_none(),
            "test_support::env_lock must serialize with harness/session-check env guards"
        );
        drop(guard);
        assert_shared_lock_reobtainable();
    }

    #[test]
    fn scoped_current_dir_uses_harness_prompt_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scoped = super::ScopedCurrentDir::set(tmp.path());
        assert!(
            crate::test_support::TEST_ENV_LOCK.try_lock().is_none(),
            "ScopedCurrentDir must hold the shared process-global test lock"
        );
        drop(scoped);
        assert_shared_lock_reobtainable();
    }
}
