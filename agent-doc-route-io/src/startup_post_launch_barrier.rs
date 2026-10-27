//! Test-only interleaving barrier for route-owned start provisioning
//! (`#claimpanecasrace`).
//!
//! A route-owned provision binds the new pane (and, under controller
//! authority, a provisional `Starting` actor) before it launches the child
//! `agent-doc start`. From the launch on, the child owns the document's
//! controller actor generation: its `start_session` advances it through the
//! SQLite compare-and-swap. A provisioner write after the launch races that
//! advance. It used to happen: the provision path re-registered the pane after
//! the launch, and when the child's commit landed inside that write's
//! read-then-store window, `agent-doc claim` failed with `controller actor
//! generation compare-and-swap failed: expected 1, found 2` (about 1 run in 60
//! under load).
//!
//! The barrier makes the interleaving deterministic. When
//! [`TEST_POST_LAUNCH_BARRIER_ENV`] is set in a scoped test process
//! ([`agent_doc_fs::test_runtime_scope::test_runtime_scope`]), the provisioner
//! records the actor generation before the launch and, after the launch, waits
//! until the child has committed a newer one. Whatever the provisioner does
//! after that runs strictly after the child's `start_session`, so a regression
//! that writes the actor again shows up on every run instead of 1 in 60.
//! Production processes have no test runtime scope and never arm it.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Set to any non-empty value to arm the barrier in a scoped test process.
pub const TEST_POST_LAUNCH_BARRIER_ENV: &str = "AGENT_DOC_TEST_ROUTE_START_POST_LAUNCH_BARRIER";

/// Upper bound on the wait. The child's `start_session` precedes its harness
/// launch, so it commits even when the harness binary is missing; the bound
/// turns a child that never starts into a failed test, not a hang.
const BARRIER_TIMEOUT: Duration = Duration::from_secs(30);
const BARRIER_POLL: Duration = Duration::from_millis(20);

/// An armed barrier: the generation the provisioner left before the launch.
pub struct PostLaunchBarrier {
    project_root: PathBuf,
    document: String,
    launched_after_generation: u64,
}

impl PostLaunchBarrier {
    /// Arm the barrier before the launch, or `None` when it is not requested
    /// or this is not a scoped test process.
    pub fn arm(project_root: &Path, file: &Path) -> Result<Option<Self>> {
        let requested =
            std::env::var_os(TEST_POST_LAUNCH_BARRIER_ENV).is_some_and(|value| !value.is_empty());
        if !requested || agent_doc_fs::test_runtime_scope::test_runtime_scope().is_none() {
            return Ok(None);
        }
        let document = file.to_string_lossy().into_owned();
        let launched_after_generation = current_generation(project_root, &document)?;
        Ok(Some(Self {
            project_root: project_root.to_path_buf(),
            document,
            launched_after_generation,
        }))
    }

    /// Block until the launched child has committed a generation newer than
    /// the one recorded at [`Self::arm`].
    pub fn await_child_generation(self, file: &Path) -> Result<u64> {
        let deadline = Instant::now() + BARRIER_TIMEOUT;
        loop {
            let generation = current_generation(&self.project_root, &self.document)?;
            if generation > self.launched_after_generation {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_startup_test_post_launch_barrier_released file={} launched_after_generation={} child_generation={}",
                        file.display(),
                        self.launched_after_generation,
                        generation,
                    ),
                );
                return Ok(generation);
            }
            if Instant::now() >= deadline {
                bail!(
                    "{TEST_POST_LAUNCH_BARRIER_ENV}: the launched start for {} never advanced the actor generation past {} within {:?}",
                    file.display(),
                    self.launched_after_generation,
                    BARRIER_TIMEOUT,
                );
            }
            std::thread::sleep(BARRIER_POLL);
        }
    }
}

fn current_generation(project_root: &Path, document: &str) -> Result<u64> {
    Ok(
        agent_doc_session_actor_io::load_record_in(project_root, document)?
            .map(|record| record.generation)
            .unwrap_or(0),
    )
}
