//! Route invocation entrypoints and per-call state.

use crate::command::{self, RouteCommandEffects, RouteMode};
use anyhow::Result;
use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

thread_local! {
    /// Absolute per-invocation deadline shared by every route readiness phase.
    ///
    /// A relative duration here let pane resolution, startup recovery, and the
    /// dispatch-only probe each spend the full editor budget independently.
    /// Store one deadline so later phases receive only the remaining allowance.
    static WAIT_FOR_READY_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Per-invocation flag forcing route-owned document mutations to disk.
    static FORCE_DISK_ROUTE_WRITES: Cell<bool> = const { Cell::new(false) };
    /// Controller background recovery may only submit to its already-proven pane.
    ///
    /// It must never rescue a stashed pane into the visible layout, select an
    /// alternate pane, or cold-start a replacement. Foreground editor routes
    /// leave this false and retain the normal routing behavior.
    static BACKGROUND_EXISTING_PANE_ONLY: Cell<bool> = const { Cell::new(false) };
}

pub fn wait_for_ready_override() -> Option<Duration> {
    WAIT_FOR_READY_DEADLINE.with(|cell| {
        cell.get()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    })
}

pub fn force_disk_route_writes() -> bool {
    FORCE_DISK_ROUTE_WRITES.with(Cell::get)
}

pub fn background_existing_pane_only() -> bool {
    BACKGROUND_EXISTING_PANE_ONLY.with(Cell::get)
}

pub struct WaitForReadyOverrideGuard {
    previous: Option<Instant>,
}

impl WaitForReadyOverrideGuard {
    pub fn set(value: Option<Duration>) -> Self {
        let deadline = value.and_then(|duration| Instant::now().checked_add(duration));
        let previous = WAIT_FOR_READY_DEADLINE.with(|cell| cell.replace(deadline));
        Self { previous }
    }
}

impl Drop for WaitForReadyOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        WAIT_FOR_READY_DEADLINE.with(|cell| cell.set(previous));
    }
}

pub struct ForceDiskRouteWritesGuard {
    previous: bool,
}

impl ForceDiskRouteWritesGuard {
    pub fn set(value: bool) -> Self {
        let previous = FORCE_DISK_ROUTE_WRITES.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for ForceDiskRouteWritesGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        FORCE_DISK_ROUTE_WRITES.with(|cell| cell.set(previous));
    }
}

pub struct BackgroundExistingPaneOnlyGuard {
    previous: bool,
}

impl BackgroundExistingPaneOnlyGuard {
    pub fn set(value: bool) -> Self {
        let previous = BACKGROUND_EXISTING_PANE_ONLY.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for BackgroundExistingPaneOnlyGuard {
    fn drop(&mut self) {
        BACKGROUND_EXISTING_PANE_ONLY.with(|cell| cell.set(self.previous));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    file: &Path,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    effects: RouteCommandEffects,
) -> Result<()> {
    run_with_force_disk(
        file,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        false,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_force_disk(
    file: &Path,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    force_disk: bool,
    effects: RouteCommandEffects,
) -> Result<()> {
    run_with_force_disk_and_prune(
        file,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        force_disk,
        true,
        effects,
    )
}

/// Run a route with explicit control over its pre-lookup fleet prune.
/// Controller recovery work that already owns an authoritative pane sets this
/// false so one orphaned document cannot resync unrelated sessions.
#[allow(clippy::too_many_arguments)]
pub fn run_with_force_disk_and_prune(
    file: &Path,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    force_disk: bool,
    prune_before_lookup: bool,
    effects: RouteCommandEffects,
) -> Result<()> {
    run_with_tmux_with_options(
        file,
        &Tmux::default_server(),
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        force_disk,
        prune_before_lookup,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_tmux(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    effects: RouteCommandEffects,
) -> Result<()> {
    run_with_tmux_with_options(
        file,
        tmux,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        wait_for_ready,
        false,
        true,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_tmux_with_options(
    file: &Path,
    tmux: &Tmux,
    pane: Option<&str>,
    debounce_ms: u64,
    col_args: &[String],
    mode: RouteMode,
    plain_trigger: bool,
    wait_for_ready: Option<Duration>,
    force_disk: bool,
    prune_before_lookup: bool,
    effects: RouteCommandEffects,
) -> Result<()> {
    let _wait_for_ready_guard = WaitForReadyOverrideGuard::set(wait_for_ready);
    let _force_disk_guard = ForceDiskRouteWritesGuard::set(force_disk);
    command::run_with_tmux_with_options(
        file,
        tmux,
        pane,
        debounce_ms,
        col_args,
        mode,
        plain_trigger,
        prune_before_lookup,
        effects,
    )
}
