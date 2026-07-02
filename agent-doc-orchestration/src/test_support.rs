//! Test-only synchronization helpers for orchestration tests that mutate
//! process-global state (environment variables).
//!
//! Mirrors the main crate's `test_support`. Each crate's tests compile into a
//! separate test executable, so a crate-local `TEST_ENV_LOCK` is sufficient to
//! serialize env-mutating tests within *this* crate's test process — no
//! cross-crate sharing is required (or possible) for a `#[cfg(test)]` static.

use std::path::Path;
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

    let guard = agent_doc_harness::prompt_source::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ProcessGlobalLockGuard {
        _guard: Some(guard),
    }
}

#[cfg(test)]
pub(crate) fn seed_live_plugin_owner_lease(file: &str) {
    let pid = std::process::id();
    assert!(
        agent_doc_plugin_owner::try_acquire_plugin_owner(file, &format!("test-editor-{pid}"), pid),
        "test setup should acquire a live plugin-owner lease"
    );
}

#[cfg(test)]
pub(crate) fn patch_with_heading(heading: &str) -> agent_doc_template::PatchBlock {
    agent_doc_template::PatchBlock::new("exchange", format!("{heading}\n\nbody line one\n"))
}

#[cfg(test)]
pub(crate) fn init_repo_with_doc(
    dir: &std::path::Path,
    name: &str,
    body: &str,
) -> std::path::PathBuf {
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["init", "-q", "--initial-branch=main"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["config", "user.email", "test@example.com"])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["add", name])
        .status()
        .unwrap();
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();
    path
}

#[cfg(test)]
pub(crate) fn drift_baseline() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) fn drift_content_ours() -> String {
    // baseline + a substantial `### Re:` response (well over the 100-byte
    // stale-drift threshold) so adopting it is a real wedge.
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Implemented the fix and verified it end to end. The response body is long\n",
        "enough to clear the stale-snapshot-reset-drift threshold so the wedge shape\n",
        "is genuinely detected by the recovery discriminator under test here.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) fn start_live_prompt_drift_ack_listener(
    project_root: &Path,
    ack_content: String,
) -> std::thread::JoinHandle<()> {
    let root = project_root.to_path_buf();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::thread::spawn(move || {
        let root_clone = root.clone();
        let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
            let v: serde_json::Value = serde_json::from_str(msg).ok()?;
            let patch_id = v
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let ack_dir = root_clone.join(".agent-doc/ack-content");
            let _ = std::fs::create_dir_all(&ack_dir);
            if let Some(file_path) = v.get("file").and_then(|value| value.as_str()) {
                let _ = std::fs::write(file_path, &ack_content);
            }
            let _ = std::fs::write(ack_dir.join(format!("{patch_id}.md")), &ack_content);
            Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
        });
    })
}

#[cfg(test)]
pub(crate) fn start_ack_without_content_listener(
    project_root: &Path,
) -> std::thread::JoinHandle<()> {
    let root = project_root.to_path_buf();
    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::thread::spawn(move || {
        let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
            let v: serde_json::Value = serde_json::from_str(msg).ok()?;
            let patch_id = v
                .get("patch_id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            Some(serde_json::json!({"type": "ack", "id": patch_id}).to_string())
        });
    })
}

#[cfg(test)]
pub(crate) fn wait_for_live_prompt_drift_listener(project_root: &Path) {
    for _ in 0..100 {
        if agent_doc_ipc_io::is_listener_active(project_root) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("fake socket listener did not start within 1s");
}

#[cfg(test)]
pub(crate) fn compact_convergence_source() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #a\n",
        "### Re: do #a — opus-4-8\n\n",
        "A long historical response body that compaction will archive and replace\n",
        "with a summary marker so the exchange shrinks substantially past the\n",
        "stale-drift threshold and a genuine convergence patch is produced.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#a]\n",
        "- do [#b]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) fn compact_convergence_compacted() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "*Compacted. Content archived to `.agent-doc/archives/test.md`*\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#a]\n",
        "- do [#b]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) fn queue_consume_convergence_source() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#fix]\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) fn queue_consume_convergence_target() -> String {
    concat!(
        "---\nsession: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #fix\n",
        "### Re: do #fix — opus-4-8\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue go -->\n",
        "- ~~do [#fix]~~\n",
        "<!-- /agent:queue -->\n",
    )
    .to_string()
}

#[cfg(test)]
pub(crate) const HALT_QUEUE_DOC: &str = concat!(
    "---\n",
    "queue_active: true\n",
    "---\n\n",
    "<!-- agent:queue auto -->\n",
    "- do [#foo]\n",
    "- do [#bar]\n",
    "<!-- /agent:queue -->\n",
);
