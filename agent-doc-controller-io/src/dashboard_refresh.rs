//! Controller-owned refresh scheduling for the live dashboard projection (`gvqv`).
//!
//! ## Spec
//! - The project controller is the single owner of `.agent-doc/dashboard.md`
//!   updates for its project root. Rendering lives above this crate (it needs
//!   the fleet board), so the controller drives it through the
//!   `ProjectControllerRuntimeEffects::refresh_dashboard_projection` port.
//! - The projection is opt-in: nothing is rendered until the file exists.
//!   `agent-doc dashboard --write` (and the editor Dashboard action) creates it;
//!   deleting it switches the refresh off again.
//! - Every controller state change ([`mark_dirty_global`]) wakes the worker,
//!   which coalesces a burst over `debounce` before rendering once. A slow poll
//!   (`poll`) picks up operator document edits that never reach the controller
//!   as state events. Renders are spaced at least `min_interval` apart.
//!
//! ## Agentic Contracts
//! - `mark_dirty` is O(1) and never blocks on rendering, so it is safe on the
//!   state-event ingress path.
//! - The worker never renders while the projection file is absent, so an
//!   un-armed project pays one `stat` per poll.
//! - A refresh failure is logged once per distinct message and never stops
//!   the worker.
//!
//! ## Evals
//! - `dirty_mark_triggers_one_debounced_refresh_when_armed`
//! - `absent_projection_file_is_never_rendered`
//! - `poll_refreshes_an_armed_projection_without_a_state_event`

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub use agent_doc_frontmatter::dashboard_projection::DASHBOARD_DEFAULT_RELATIVE_PATH;

/// Default project-root-relative dashboard projection path.
pub fn dashboard_path(project_root: &Path) -> PathBuf {
    project_root.join(DASHBOARD_DEFAULT_RELATIVE_PATH)
}

/// Scheduling knobs for the refresh worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardRefreshTiming {
    /// Quiet period that coalesces a burst of state changes into one render.
    pub debounce: Duration,
    /// Fallback interval that catches document edits with no state event.
    pub poll: Duration,
    /// Minimum spacing between two renders.
    pub min_interval: Duration,
}

impl Default for DashboardRefreshTiming {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            poll: Duration::from_secs(5),
            min_interval: Duration::from_secs(1),
        }
    }
}

/// Render callback: returns `Ok(true)` when the projection bytes changed.
pub type DashboardRenderFn = dyn Fn(&Path) -> anyhow::Result<bool> + Send + Sync + 'static;

#[derive(Default)]
struct RefreshState {
    dirty: bool,
    stopped: bool,
}

/// One project's debounced dashboard refresh worker.
pub struct DashboardRefresher {
    project_root: PathBuf,
    state: Mutex<RefreshState>,
    wake: Condvar,
}

impl DashboardRefresher {
    /// Spawn the worker thread for `project_root`.
    pub fn spawn(
        project_root: PathBuf,
        timing: DashboardRefreshTiming,
        render: Arc<DashboardRenderFn>,
    ) -> std::io::Result<Arc<Self>> {
        let refresher = Arc::new(Self {
            project_root,
            state: Mutex::new(RefreshState::default()),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&refresher);
        std::thread::Builder::new()
            .name("agent-doc-dashboard".to_string())
            .spawn(move || worker.run(timing, render.as_ref()))?;
        Ok(refresher)
    }

    /// Record that controller state changed. Cheap; never renders inline.
    pub fn mark_dirty(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.dirty = true;
        }
        self.wake.notify_one();
    }

    /// Stop the worker after its current iteration.
    pub fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
        }
        self.wake.notify_one();
    }

    fn run(&self, timing: DashboardRefreshTiming, render: &DashboardRenderFn) {
        let path = dashboard_path(&self.project_root);
        let mut last_render: Option<Instant> = None;
        let mut last_error: Option<String> = None;
        loop {
            // Wait for a state change or the poll fallback.
            let woke_dirty = {
                let Ok(guard) = self.state.lock() else {
                    return;
                };
                let Ok((mut guard, _)) =
                    self.wake.wait_timeout_while(guard, timing.poll, |state| {
                        !state.dirty && !state.stopped
                    })
                else {
                    return;
                };
                if guard.stopped {
                    return;
                }
                std::mem::take(&mut guard.dirty)
            };
            if woke_dirty {
                // Coalesce the rest of the burst.
                std::thread::sleep(timing.debounce);
                if let Ok(mut guard) = self.state.lock() {
                    if guard.stopped {
                        return;
                    }
                    guard.dirty = false;
                }
            }
            if !path.is_file() {
                continue;
            }
            if let Some(last) = last_render {
                let elapsed = last.elapsed();
                if elapsed < timing.min_interval {
                    std::thread::sleep(timing.min_interval - elapsed);
                }
            }
            last_render = Some(Instant::now());
            match render(&self.project_root) {
                Ok(_) => last_error = None,
                Err(error) => {
                    let message = format!("{error:#}");
                    if last_error.as_deref() != Some(message.as_str()) {
                        agent_doc_ops_log_io::log_op(
                            &self.project_root,
                            &format!("dashboard_projection_refresh_failed error={message:?}"),
                        );
                        last_error = Some(message);
                    }
                }
            }
        }
    }
}

static GLOBAL_REFRESHER: OnceLock<Arc<DashboardRefresher>> = OnceLock::new();

/// Start this process's controller-owned refresher once. A controller process
/// serves exactly one project root; later calls are no-ops.
pub fn start_global(project_root: PathBuf, render: Arc<DashboardRenderFn>) {
    if GLOBAL_REFRESHER.get().is_some() {
        return;
    }
    match DashboardRefresher::spawn(project_root, DashboardRefreshTiming::default(), render) {
        Ok(refresher) => {
            if GLOBAL_REFRESHER.set(Arc::clone(&refresher)).is_err() {
                refresher.stop();
            }
        }
        Err(error) => eprintln!("[controller] failed to start dashboard refresher: {error}"),
    }
}

/// Wake the process's refresher, when one is running.
pub fn mark_dirty_global() {
    if let Some(refresher) = GLOBAL_REFRESHER.get() {
        refresher.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn fast_timing(poll: Duration) -> DashboardRefreshTiming {
        DashboardRefreshTiming {
            debounce: Duration::from_millis(40),
            poll,
            min_interval: Duration::from_millis(1),
        }
    }

    fn counting_render() -> (Arc<AtomicUsize>, Arc<DashboardRenderFn>) {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        let render: Arc<DashboardRenderFn> = Arc::new(move |_root: &Path| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        });
        (count, render)
    }

    fn arm(root: &Path) {
        let path = dashboard_path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            "<!-- agent-doc-dashboard v1 scope=project all=false -->\n",
        )
        .unwrap();
    }

    fn wait_for(count: &AtomicUsize, at_least: usize) -> usize {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let seen = count.load(Ordering::SeqCst);
            if seen >= at_least {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        count.load(Ordering::SeqCst)
    }

    #[test]
    fn dirty_mark_triggers_one_debounced_refresh_when_armed() {
        let dir = TempDir::new().unwrap();
        arm(dir.path());
        let (count, render) = counting_render();
        let refresher = DashboardRefresher::spawn(
            dir.path().to_path_buf(),
            fast_timing(Duration::from_secs(60)),
            render,
        )
        .unwrap();
        for _ in 0..10 {
            refresher.mark_dirty();
        }
        assert_eq!(wait_for(&count, 1), 1);
        // The burst was coalesced: no second render follows.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        refresher.mark_dirty();
        assert_eq!(wait_for(&count, 2), 2);
        refresher.stop();
    }

    #[test]
    fn absent_projection_file_is_never_rendered() {
        let dir = TempDir::new().unwrap();
        let (count, render) = counting_render();
        let refresher = DashboardRefresher::spawn(
            dir.path().to_path_buf(),
            fast_timing(Duration::from_millis(20)),
            render,
        )
        .unwrap();
        refresher.mark_dirty();
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(count.load(Ordering::SeqCst), 0);
        refresher.stop();
    }

    #[test]
    fn poll_refreshes_an_armed_projection_without_a_state_event() {
        let dir = TempDir::new().unwrap();
        arm(dir.path());
        let (count, render) = counting_render();
        let refresher = DashboardRefresher::spawn(
            dir.path().to_path_buf(),
            fast_timing(Duration::from_millis(30)),
            render,
        )
        .unwrap();
        assert!(wait_for(&count, 2) >= 2);
        refresher.stop();
    }
}
