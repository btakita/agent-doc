//! Route invocation entrypoints and per-call state.

use crate::command::{self, RouteCommandEffects, RouteMode};
use anyhow::Result;
use std::cell::{Cell, RefCell};
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
    /// A no-layout route invoked from a pane that owns another document may
    /// reuse an already-proven target, but must not change tmux topology or
    /// focus. This is the automatic child-route counterpart to controller
    /// background recovery.
    static CROSS_DOCUMENT_EXISTING_PANE_ONLY: Cell<bool> = const { Cell::new(false) };
    /// `#routelaterescue`: the controller `editor_route` has already projected
    /// and observed this route's layout before dispatch. From then on the
    /// layout plane is the sole tmux topology writer: a pane found in stash at
    /// dispatch time was stashed by a NEWER publication (possibly another
    /// project root's controller sharing the window), so route must dispatch
    /// to it in place instead of raw-joining it back as an extra column.
    static LAYOUT_OWNED_BY_CONTROLLER: Cell<bool> = const { Cell::new(false) };
/// Layout reconciliation owns the final visible pane and focus projection.
///
/// Pane provisioning performed inside that transaction must remain
/// focus-neutral until tmux-router has placed every pane. Standalone route
/// startup leaves this false and retains its immediate-focus behavior.
static DEFER_STARTUP_FOCUS_TO_LAYOUT: Cell<bool> = const { Cell::new(false) };
/// GH 91: a route that returned `Ok` without dispatching — its prompt or the
/// existing queue head is waiting behind an open closeout — records the
/// operator-facing outcome here so the controller's editor route reports it
/// instead of "dispatched".
static ROUTE_DEFERRAL: RefCell<Option<String>> = const { RefCell::new(None) };
/// `#claimedsteerwake`: a route that dispatched no new trigger but handed the
/// operator's explicit send to the owning turn reports it here (exit 0).
static ROUTE_STEERING_DELIVERY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Exit code for an editor route that completed without dispatching
/// (`EX_TEMPFAIL`): the work is deferred behind an open closeout. Non-zero so
/// the controller never settles it as `applied=true`.
pub const ROUTE_DEFERRED_EXIT_CODE: i32 = 75;

/// Record that this route invocation deferred instead of dispatching.
pub fn record_route_deferral(outcome: String) {
    ROUTE_DEFERRAL.with(|cell| *cell.borrow_mut() = Some(outcome));
}

/// Record that this route delivered the operator's explicit send to the
/// owning turn instead of dispatching a new trigger (`#claimedsteerwake`).
pub fn record_route_steering_delivery(outcome: String) {
    ROUTE_STEERING_DELIVERY.with(|cell| *cell.borrow_mut() = Some(outcome));
}

/// Take (and clear) the steering delivery recorded by this thread's route.
pub fn take_route_steering_delivery() -> Option<String> {
    ROUTE_STEERING_DELIVERY.with(|cell| cell.borrow_mut().take())
}

/// Take (and clear) the deferral recorded by this thread's route invocation.
pub fn take_route_deferral() -> Option<String> {
    ROUTE_DEFERRAL.with(|cell| cell.borrow_mut().take())
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

pub fn cross_document_existing_pane_only() -> bool {
    CROSS_DOCUMENT_EXISTING_PANE_ONLY.with(Cell::get)
}

/// Whether this route must preserve the caller's visible tmux surface.
pub fn preserve_route_layout() -> bool {
    background_existing_pane_only() || cross_document_existing_pane_only()
}

/// Whether the controller layout projection owns tmux topology for this route
/// (`#routelaterescue`). See [`LayoutOwnedByControllerGuard`].
pub fn layout_owned_by_controller() -> bool {
    LAYOUT_OWNED_BY_CONTROLLER.with(Cell::get)
}

pub fn defer_startup_focus_to_layout() -> bool {
    DEFER_STARTUP_FOCUS_TO_LAYOUT.with(Cell::get)
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

pub struct CrossDocumentExistingPaneOnlyGuard {
    previous: bool,
}

impl CrossDocumentExistingPaneOnlyGuard {
    pub fn set(value: bool) -> Self {
        let previous = CROSS_DOCUMENT_EXISTING_PANE_ONLY.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for CrossDocumentExistingPaneOnlyGuard {
    fn drop(&mut self) {
        CROSS_DOCUMENT_EXISTING_PANE_ONLY.with(|cell| cell.set(self.previous));
    }
}

fn automatic_cross_document_route(
    has_layout_columns: bool,
    explicit_background_route: bool,
    foreign_owner: Option<&str>,
) -> bool {
    !has_layout_columns && !explicit_background_route && foreign_owner.is_some()
}

/// Scopes [`layout_owned_by_controller`] to one controller `editor_route`
/// invocation and restores the previous value on drop.
pub struct LayoutOwnedByControllerGuard {
    previous: bool,
}

impl LayoutOwnedByControllerGuard {
    pub fn set(value: bool) -> Self {
        let previous = LAYOUT_OWNED_BY_CONTROLLER.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for LayoutOwnedByControllerGuard {
    fn drop(&mut self) {
        LAYOUT_OWNED_BY_CONTROLLER.with(|cell| cell.set(self.previous));
    }
}

pub struct DeferStartupFocusToLayoutGuard {
    previous: bool,
}

impl DeferStartupFocusToLayoutGuard {
    pub fn set(value: bool) -> Self {
        let previous = DEFER_STARTUP_FOCUS_TO_LAYOUT.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for DeferStartupFocusToLayoutGuard {
    fn drop(&mut self) {
        DEFER_STARTUP_FOCUS_TO_LAYOUT.with(|cell| cell.set(self.previous));
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
        &agent_doc_tmux_io::configured_tmux(),
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
    // Only explicit process-context pane evidence is accepted here. Falling
    // back to tmux's ambient active pane would misclassify IDE/plugin routes as
    // cross-document whenever an unrelated managed pane happened to be active.
    let current_pane = agent_doc_tmux_io::current_live_pane_id_from_env_or_override(tmux);
    let foreign_owner = current_pane.as_deref().and_then(|current_pane| {
        agent_doc_sync_io::sync::pane_owned_document_other_than(tmux, current_pane, file)
    });
    let cross_document_existing_pane_only = automatic_cross_document_route(
        !col_args.is_empty(),
        background_existing_pane_only(),
        foreign_owner.as_deref(),
    );
    let _cross_document_guard =
        CrossDocumentExistingPaneOnlyGuard::set(cross_document_existing_pane_only);
    if cross_document_existing_pane_only {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_cross_document_existing_pane_only file={} current_pane={} pane_owns={} focus_effect=preserved pane_effect=none",
                file.display(),
                current_pane.as_deref().unwrap_or("none"),
                foreign_owner.as_deref().unwrap_or("none"),
            ),
        );
    }
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

#[cfg(test)]
mod tests {
    use super::automatic_cross_document_route;

    #[test]
    fn automatic_cross_document_route_requires_foreign_owner_without_layout_columns() {
        assert!(automatic_cross_document_route(
            false,
            false,
            Some("other.md")
        ));
        assert!(!automatic_cross_document_route(
            true,
            false,
            Some("other.md")
        ));
        assert!(!automatic_cross_document_route(
            false,
            true,
            Some("other.md")
        ));
        assert!(!automatic_cross_document_route(false, false, None));
    }

    /// `#routelaterescue`: the controller-owned layout scope is per invocation
    /// and restores on drop, and it does not widen into the background /
    /// cross-document `preserve_route_layout` policy (an editor route may still
    /// cold-start or focus; it only loses the raw stash rejoin).
    #[test]
    fn layout_owned_by_controller_guard_is_scoped_and_independent_of_preserve_layout() {
        use super::{
            LayoutOwnedByControllerGuard, layout_owned_by_controller, preserve_route_layout,
        };

        assert!(!layout_owned_by_controller());
        {
            let _outer = LayoutOwnedByControllerGuard::set(true);
            assert!(layout_owned_by_controller());
            assert!(!preserve_route_layout());
            {
                let _inner = LayoutOwnedByControllerGuard::set(false);
                assert!(!layout_owned_by_controller());
            }
            assert!(layout_owned_by_controller());
        }
        assert!(!layout_owned_by_controller());
    }
}
