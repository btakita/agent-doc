//! Route invocation entrypoints and per-call state.

use crate::command::{self, RouteCommandEffects, RouteMode};
use agent_doc_controller::dispatch::AuthoritativeActorDispatchOrigin;
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
    /// This background route is the bounded retry after a recovered supervisor
    /// failed to drain during the preceding orphan-dispatch backoff interval.
    static BACKGROUND_RECOVERED_ACTOR_FALLBACK: Cell<bool> = const { Cell::new(false) };
    /// GH #319: who asked for this route. Distinct from the
    /// background layout restriction above: that one also covers
    /// `#stashbackgroundreentry` owner re-entry, while this names the
    /// controller-originated (automated) dispatch origin that must defer behind
    /// a live turn instead of using the operator plain-trigger steering path.
    static DISPATCH_ORIGIN: Cell<AuthoritativeActorDispatchOrigin> =
        const { Cell::new(AuthoritativeActorDispatchOrigin::Operator) };
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
/// Layout reconciliation owns the final visible pane and focus effect.
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
/// GH #228 (`#routeliveturnverdict`): a route that met an open closeout which
/// session-check classified as a live owner-scoped turn dispatched nothing
/// because that turn is already doing the work. Benign: reported with exit 0,
/// never as the `EX_TEMPFAIL` deferral or a recovery command.
static ROUTE_LIVE_OWNER_TURN: RefCell<Option<String>> = const { RefCell::new(None) };
/// `#ideriderindependentnav` P6: the editor surface client this route acts
/// for (`agent-doc route --client-id`, or the controller `editor_route`'s
/// requester). In `per_installation` mode an installation-scoped client's new
/// pane starts in its own navigation lane session.
static NAVIGATION_CLIENT: RefCell<Option<String>> = const { RefCell::new(None) };
/// GH #243: this route started or dispatched into a pane whose tmux session
/// has no attached client. The delivery stands (exit 0), but the editor must
/// tell the operator the pane is on no screen instead of reporting plain
/// success.
static ROUTE_PANE_NOT_VISIBLE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Record that this route's target pane is on no screen (GH #243).
pub fn record_route_pane_not_visible(notice: String) {
    ROUTE_PANE_NOT_VISIBLE.with(|cell| *cell.borrow_mut() = Some(notice));
}

/// Take (and clear) the not-visible notice recorded by this thread's route.
pub fn take_route_pane_not_visible() -> Option<String> {
    ROUTE_PANE_NOT_VISIBLE.with(|cell| cell.borrow_mut().take())
}

/// GH #243: when `pane` is not on an attached client's screen, log `event`,
/// print the operator notice, and record it for the editor route's result. An
/// attached session can still be invisible when the pane is in an inactive
/// window or hidden behind a zoomed sibling. Returns the notice. A visible pane
/// (or an unreadable one) records nothing.
pub fn report_pane_if_not_visible(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    event: &str,
) -> Option<String> {
    let target = agent_doc_tmux_io::unwatched_pane(tmux, pane)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{event} file={} pane={} session={} reason={} window={} watched_sessions=[{}]",
            file.display(),
            target.pane,
            target.session,
            target.reason.as_str(),
            target.window_id.as_deref().unwrap_or("none"),
            target.watched_sessions.join(","),
        ),
    );
    let notice = agent_doc_tmux_io::pane_not_visible_notice(file, &target);
    eprintln!("{notice}");
    record_route_pane_not_visible(notice.clone());
    Some(notice)
}

/// The editor surface client this route acts for, if any (P6).
pub fn navigation_client_id() -> Option<String> {
    NAVIGATION_CLIENT.with(|cell| cell.borrow().clone())
}

/// Scopes [`navigation_client_id`] to one route invocation and restores the
/// previous value on drop.
pub struct NavigationClientGuard {
    previous: Option<String>,
}

impl NavigationClientGuard {
    pub fn set(client_id: Option<&str>) -> Self {
        let client_id = client_id
            .map(str::trim)
            .filter(|client| !client.is_empty())
            .map(str::to_string);
        let previous = NAVIGATION_CLIENT.with(|cell| cell.replace(client_id));
        Self { previous }
    }
}

impl Drop for NavigationClientGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        NAVIGATION_CLIENT.with(|cell| *cell.borrow_mut() = previous);
    }
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

/// Record that this route yielded to a live owner turn (GH #228).
pub fn record_route_live_owner_turn(outcome: String) {
    ROUTE_LIVE_OWNER_TURN.with(|cell| *cell.borrow_mut() = Some(outcome));
}

/// Take (and clear) the live-owner-turn yield recorded by this thread's route.
pub fn take_route_live_owner_turn() -> Option<String> {
    ROUTE_LIVE_OWNER_TURN.with(|cell| cell.borrow_mut().take())
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

pub fn background_recovered_actor_fallback() -> bool {
    BACKGROUND_RECOVERED_ACTOR_FALLBACK.with(Cell::get)
}

/// GH #319: the dispatch origin of this thread's route invocation.
pub fn dispatch_origin() -> AuthoritativeActorDispatchOrigin {
    DISPATCH_ORIGIN.with(Cell::get)
}

pub fn cross_document_existing_pane_only() -> bool {
    CROSS_DOCUMENT_EXISTING_PANE_ONLY.with(Cell::get)
}

/// Whether this route must preserve the caller's visible tmux surface.
pub fn preserve_route_layout() -> bool {
    background_existing_pane_only() || cross_document_existing_pane_only()
}

/// Facts proving that a route is a background re-entry from its own parked
/// owner pane, rather than a foreground operator route.
///
/// The predicate is deliberately conjunctive and fail-closed:
///
/// `background_owner_reentry = current_pane_present ∧ owner_matches ∧ pane_is_stashed ∧ ¬controller_layout_owned`
///
/// Missing pane provenance, missing owner proof, or missing stash proof can
/// never manufacture background authority. The independent focus fence in
/// `route_focus` still refuses to select a pane observed in stash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundOwnerReentryFacts {
    pub current_pane_present: bool,
    pub owner_matches: bool,
    pub pane_is_stashed: bool,
    pub controller_layout_owned: bool,
}

pub const fn background_owner_reentry(facts: BackgroundOwnerReentryFacts) -> bool {
    facts.current_pane_present
        && facts.owner_matches
        && facts.pane_is_stashed
        && !facts.controller_layout_owned
}

/// Whether the controller tmux layout effect owns topology for this route
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

pub struct BackgroundRecoveredActorFallbackGuard {
    previous: bool,
}

impl BackgroundRecoveredActorFallbackGuard {
    pub fn set(value: bool) -> Self {
        let previous = BACKGROUND_RECOVERED_ACTOR_FALLBACK.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for BackgroundRecoveredActorFallbackGuard {
    fn drop(&mut self) {
        BACKGROUND_RECOVERED_ACTOR_FALLBACK.with(|cell| cell.set(self.previous));
    }
}

/// Scopes [`dispatch_origin`] to one route invocation and restores the previous
/// value on drop.
pub struct DispatchOriginGuard {
    previous: AuthoritativeActorDispatchOrigin,
}

impl DispatchOriginGuard {
    pub fn set(value: AuthoritativeActorDispatchOrigin) -> Self {
        let previous = DISPATCH_ORIGIN.with(|cell| cell.replace(value));
        Self { previous }
    }
}

impl Drop for DispatchOriginGuard {
    fn drop(&mut self) {
        DISPATCH_ORIGIN.with(|cell| cell.set(self.previous));
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
    let background_owner_reentry = background_owner_reentry(BackgroundOwnerReentryFacts {
        current_pane_present: current_pane.is_some(),
        owner_matches: current_pane.as_deref().is_some_and(|pane| {
            agent_doc_sync_io::sync::pane_process_tree_owns_document(tmux, pane, file)
        }),
        pane_is_stashed: current_pane
            .as_deref()
            .is_some_and(|pane| agent_doc_sync_io::sync::pane_in_stash_window(tmux, pane)),
        controller_layout_owned: layout_owned_by_controller(),
    });
    // `#stashbackgroundreentry`: a supervisor/hook route launched from a
    // document's own pane while that pane is parked in stash is background
    // delivery. It may inject into the proven pane, but it cannot rescue the
    // pane, select its window, or otherwise overwrite the foreground editor
    // layout. Preserve an explicit outer background guard as well.
    let _background_owner_reentry_guard = BackgroundExistingPaneOnlyGuard::set(
        background_existing_pane_only() || background_owner_reentry,
    );
    if background_owner_reentry {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_background_owner_reentry file={} pane={} authority=background_existing_pane_only focus_effect=none pane_effect=none (#stashbackgroundreentry)",
                file.display(),
                current_pane.as_deref().unwrap_or("none"),
            ),
        );
    }
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
    use super::{
        BackgroundOwnerReentryFacts, automatic_cross_document_route, background_owner_reentry,
    };

    #[test]
    fn background_owner_reentry_requires_all_authority_facts() {
        for current_pane_present in [false, true] {
            for owner_matches in [false, true] {
                for pane_is_stashed in [false, true] {
                    for controller_layout_owned in [false, true] {
                        let facts = BackgroundOwnerReentryFacts {
                            current_pane_present,
                            owner_matches,
                            pane_is_stashed,
                            controller_layout_owned,
                        };
                        assert_eq!(
                            background_owner_reentry(facts),
                            current_pane_present
                                && owner_matches
                                && pane_is_stashed
                                && !controller_layout_owned,
                            "truth-table row: {facts:?}",
                        );
                    }
                }
            }
        }
    }

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

    /// GH #243: a dispatch into a pane no client can currently see is reported
    /// (ops log + recorded notice for the editor route). Attaching its session
    /// makes the active pane visible, while an inactive window still reports.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn dispatch_into_an_off_screen_pane_reports_the_pane_not_visible() {
        use super::{report_pane_if_not_visible, take_route_pane_not_visible};
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc-a.md");
        std::fs::write(&doc, "# a\n").unwrap();
        let tmux = agent_doc_tmux_io::isolated_test_tmux("gh243-route-not-visible");
        let pane = tmux.new_session("0", dir.path()).unwrap();
        let _watched = tmux.new_session("1", dir.path()).unwrap();
        let _client = tmux.attach_control_mode(Some("1")).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while agent_doc_tmux_io::session_attached_client_count(&tmux, "1") == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "client never attached"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = take_route_pane_not_visible();

        let notice =
            report_pane_if_not_visible(&tmux, &doc, &pane, "route_dispatch_target_not_visible")
                .expect("client-less session must be reported");
        assert!(notice.starts_with(agent_doc_tmux_io::PANE_NOT_VISIBLE_MARKER));
        assert!(
            notice.contains(&format!("pane {pane} of tmux session '0'")),
            "{notice}"
        );
        assert!(notice.contains("attach-session -t 0"), "{notice}");
        assert_eq!(
            take_route_pane_not_visible().as_deref(),
            Some(notice.as_str())
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains(&format!(
                "route_dispatch_target_not_visible file={} pane={pane} session=0 reason=unattached_session window=none watched_sessions=[1]",
                doc.display()
            )),
            "{ops}"
        );

        let _client0 = tmux.attach_control_mode(Some("0")).unwrap();
        while agent_doc_tmux_io::session_attached_client_count(&tmux, "0") == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "client never attached"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert_eq!(
            report_pane_if_not_visible(&tmux, &doc, &pane, "route_dispatch_target_not_visible"),
            None
        );
        assert_eq!(take_route_pane_not_visible(), None);

        let hidden_pane = tmux
            .raw_cmd(&[
                "new-window",
                "-d",
                "-t",
                "0:",
                "-n",
                "stash",
                "-P",
                "-F",
                "#{pane_id}",
            ])
            .unwrap();
        let hidden_pane = hidden_pane.trim();
        let notice = report_pane_if_not_visible(
            &tmux,
            &doc,
            hidden_pane,
            "route_live_owner_target_not_visible",
        )
        .expect("inactive window in an attached session must be reported");
        assert!(notice.contains("inactive tmux window 'stash'"), "{notice}");
        assert!(notice.contains("select-window"), "{notice}");
        assert!(notice.contains("select-pane"), "{notice}");
        assert_eq!(
            take_route_pane_not_visible().as_deref(),
            Some(notice.as_str())
        );
        let ops = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops.contains(&format!(
                "route_live_owner_target_not_visible file={} pane={hidden_pane} session=0 reason=inactive_window",
                doc.display()
            )),
            "{ops}"
        );
    }
}
