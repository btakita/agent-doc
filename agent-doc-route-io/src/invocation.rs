//! Route invocation entrypoints and per-call state.

use crate::command::{self, RouteCommandEffects, RouteMode};
use anyhow::Result;
use std::cell::Cell;
use std::path::Path;
use std::time::Duration;
use tmux_router::Tmux;

thread_local! {
    /// Per-invocation override for bounded authoritative-actor ready waits.
    static WAIT_FOR_READY_OVERRIDE: Cell<Option<Duration>> = const { Cell::new(None) };
    /// Per-invocation flag forcing route-owned document mutations to disk.
    static FORCE_DISK_ROUTE_WRITES: Cell<bool> = const { Cell::new(false) };
}

pub fn wait_for_ready_override() -> Option<Duration> {
    WAIT_FOR_READY_OVERRIDE.with(|cell| cell.get())
}

pub fn force_disk_route_writes() -> bool {
    FORCE_DISK_ROUTE_WRITES.with(Cell::get)
}

pub struct WaitForReadyOverrideGuard {
    previous: Option<Duration>,
}

impl WaitForReadyOverrideGuard {
    pub fn set(value: Option<Duration>) -> Self {
        let previous = WAIT_FOR_READY_OVERRIDE.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for WaitForReadyOverrideGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        WAIT_FOR_READY_OVERRIDE.with(|cell| cell.set(previous));
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
