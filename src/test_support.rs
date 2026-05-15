#[cfg(test)]
thread_local! {
    static PROCESS_GLOBAL_LOCK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct ProcessGlobalLockGuard {
    _guard: Option<std::sync::MutexGuard<'static, ()>>,
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

    let guard = crate::harness_prompt::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    #[test]
    fn env_lock_uses_harness_prompt_lock() {
        let guard = super::env_lock();
        assert!(
            crate::harness_prompt::TEST_ENV_LOCK.try_lock().is_err(),
            "test_support::env_lock must serialize with harness/session-check env guards"
        );
        drop(guard);
        assert!(
            crate::harness_prompt::TEST_ENV_LOCK.try_lock().is_ok(),
            "shared env lock should be available after test_support guard drops"
        );
    }

    #[test]
    fn scoped_current_dir_uses_harness_prompt_lock() {
        let tmp = tempfile::TempDir::new().unwrap();
        let scoped = super::ScopedCurrentDir::set(tmp.path());
        assert!(
            crate::harness_prompt::TEST_ENV_LOCK.try_lock().is_err(),
            "ScopedCurrentDir must hold the shared process-global test lock"
        );
        drop(scoped);
        assert!(
            crate::harness_prompt::TEST_ENV_LOCK.try_lock().is_ok(),
            "shared lock should be available after cwd guard drops"
        );
    }
}
