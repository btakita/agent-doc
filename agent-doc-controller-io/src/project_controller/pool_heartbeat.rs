//! `#poolidleheartbeat`: the effectful half of the subagent pool heartbeat tick.
//! The module docs of `agent_doc_controller::pool_heartbeat` hold the design;
//! `agent_doc_queue::subagent_pool` owns what "due" means.
//!
//! Runs on the controller's background maintenance worker. A document costs one
//! ledger read per tick until a heartbeat could be due, and its text is read
//! only then, so a quiescent document never materializes
//! (`#orphandrainrevisiongate`).

use super::*;
use agent_doc_controller::pool_heartbeat::{
    POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS, POOL_HEARTBEAT_TICK_INTERVAL_SECS,
    PoolHeartbeatDecision, PoolHeartbeatObservation, PoolHeartbeatSkip, pool_heartbeat_decision,
};
use agent_doc_queue::subagent_pool::{PoolHeartbeatWake, heartbeat_may_be_due};

/// Env override (seconds) of [`POOL_HEARTBEAT_TICK_INTERVAL_SECS`].
const POOL_HEARTBEAT_TICK_SECS_ENV: &str = "AGENT_DOC_SUBAGENT_HEARTBEAT_TICK_SECS";
/// Durable attempt fence per document: survives a restart, and excludes a
/// handoff predecessor and successor ticking side by side.
const POOL_HEARTBEAT_ATTEMPT_SCOPE: &str = "subagent_pool_heartbeat_attempt";
/// `project_runtime_state` prefix of the last due set a wake reached.
const POOL_HEARTBEAT_RECEIPT_KEY_PREFIX: &str = "subagent_pool_heartbeat_wake";

fn pool_heartbeat_tick_interval() -> Duration {
    Duration::from_secs(
        std::env::var(POOL_HEARTBEAT_TICK_SECS_ENV)
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0)
            .unwrap_or(POOL_HEARTBEAT_TICK_INTERVAL_SECS),
    )
}

fn pool_heartbeat_receipt_key(document_hash: &str) -> String {
    format!("{POOL_HEARTBEAT_RECEIPT_KEY_PREFIX}:{document_hash}")
}

/// The document's authoritative text, or `None` while an attached editor's
/// replica is not readable. Disk is consulted only when no editor owns it.
fn pool_heartbeat_document_text(file: &Path) -> Result<Option<String>> {
    match agent_doc_crdt_relay_io::current_text_for_file_nonblocking(file)? {
        agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => Ok(Some(text)),
        agent_doc_crdt_relay_io::CurrentText::Detached => Ok(Some(
            std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?,
        )),
        agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica
        | agent_doc_crdt_relay_io::CurrentText::EditorSyncPending => Ok(None),
    }
}

/// What one tick does for one document.
#[derive(Debug, PartialEq, Eq)]
enum PoolHeartbeatStep {
    NothingDue,
    Hold(PoolHeartbeatSkip, PoolHeartbeatWake),
    /// The attempt is fenced; the caller routes.
    Dispatch(PoolHeartbeatWake),
}

fn pool_heartbeat_step(
    conn: &state_store::Connection,
    project_root: &Path,
    file: &Path,
    pane_ready: bool,
    now_secs: u64,
    wake_for: impl FnOnce(&str) -> Result<Option<PoolHeartbeatWake>>,
) -> Result<PoolHeartbeatStep> {
    let Some(ledger) = state_store::load_queue_document_state_from_db(
        conn,
        &agent_doc_hash::path_hash(file)?,
        agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND,
    )?
    else {
        return Ok(PoolHeartbeatStep::NothingDue);
    };
    let ledger =
        agent_doc_queue::queue_claim::QueueClaimLedger::from_state_payload(&ledger.payload_json)?;
    if !heartbeat_may_be_due(&ledger.claims, now_secs) {
        return Ok(PoolHeartbeatStep::NothingDue);
    }
    let Some(content) = pool_heartbeat_document_text(file)? else {
        return Ok(PoolHeartbeatStep::NothingDue);
    };
    let Some(wake) = wake_for(&content)? else {
        return Ok(PoolHeartbeatStep::NothingDue);
    };

    let document_hash = agent_doc_fs::document_state_hash(file)?;
    let document_id = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned();
    let observation = PoolHeartbeatObservation {
        delivered: state_store::load_project_runtime_state_from_db(
            conn,
            &pool_heartbeat_receipt_key(&document_hash),
        )?
        .is_some_and(|fingerprint| fingerprint == wake.fingerprint),
        queue_paused: state_store::load_effective_queue_control_from_db(
            conn,
            &document_id,
            &project_root.to_string_lossy(),
        )?
        .is_some_and(|control| control.state == "paused"),
        loop_owns_drain: state_store::load_coordination_lease_from_db(
            conn,
            agent_doc_lease::DRAIN_OWNER_SCOPE,
            &document_hash,
        )?
        .is_some_and(|lease| {
            agent_doc_lease::timestamp_is_fresh(
                lease.heartbeat_secs,
                now_secs,
                agent_doc_lease::drain_owner_ttl(),
            )
        }),
        pane_busy: !pane_ready,
        secs_since_last_attempt: state_store::load_coordination_lease_from_db(
            conn,
            POOL_HEARTBEAT_ATTEMPT_SCOPE,
            &document_hash,
        )?
        .map(|lease| now_secs.saturating_sub(lease.heartbeat_secs)),
    };
    if let PoolHeartbeatDecision::Skip(skip) =
        pool_heartbeat_decision(observation, POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS)
    {
        return Ok(PoolHeartbeatStep::Hold(skip, wake));
    }
    let claimed = state_store::claim_coordination_lease_if_expired_in_db(
        conn,
        &state_store::CoordinationLeaseRecord {
            scope_kind: POOL_HEARTBEAT_ATTEMPT_SCOPE.to_string(),
            scope_id: document_hash,
            holder: wake.fingerprint.clone(),
            holder_pid: Some(std::process::id()),
            heartbeat_secs: now_secs,
        },
        now_secs.saturating_sub(POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS),
    )?;
    Ok(if claimed {
        PoolHeartbeatStep::Dispatch(wake)
    } else {
        PoolHeartbeatStep::Hold(PoolHeartbeatSkip::RecentlyAttempted, wake)
    })
}

fn pool_heartbeat_route_invocation(
    file: &Path,
    project_root: &Path,
    pane: &str,
) -> ControllerEditorRouteInvocation {
    ControllerEditorRouteInvocation {
        file: file.to_path_buf(),
        relative_path: file
            .strip_prefix(project_root)
            .unwrap_or(file)
            .to_string_lossy()
            .into_owned(),
        pane: Some(pane.to_string()),
        layout_args: Vec::new(),
        dispatch_only: true,
        plain_trigger: true,
        wait_for_ready_secs: None,
        force_disk: false,
        prune_before_lookup: false,
        background_existing_pane_only: true,
        // A heartbeat is no queue head, so a supervisor's idle watch will never
        // pick this up: yielding to it, as an orphan drain's first attempt
        // does, would lose the wake. Route to the live actor directly.
        background_recovered_actor_fallback: true,
        // The operator's queue stays exactly as it was: waiting on workers is
        // no reason to activate a queue the operator left inactive.
        trigger_only: true,
        dispatch_origin:
            agent_doc_controller::dispatch::AuthoritativeActorDispatchOrigin::AutomatedBackground,
        layout_owned_by_controller: false,
        navigation_client_id: None,
    }
}

/// The wake a queued route is delivering.
pub(super) struct PoolHeartbeatRoute(PoolHeartbeatWake);

impl PoolHeartbeatRoute {
    /// Receipt the wake, or leave it due for the next attempt. Only a delivered
    /// route fences the set: a route deferred behind a live turn delivered
    /// nothing, and the tick retries it after the attempt interval.
    pub(super) fn settle(
        self,
        file: &Path,
        project_root: &Path,
        route: std::thread::Result<Result<ControllerEditorRouteRuntimeResult>>,
    ) {
        let wake = self.0;
        let fingerprint = wake.fingerprint.get(..12).unwrap_or(&wake.fingerprint);
        let outcome = match route {
            Ok(Ok(result)) if result.exit_code == 0 => {
                match record_pool_heartbeat_receipt(file, project_root, &wake.fingerprint) {
                    Ok(()) => format!(
                        "subagent_pool_heartbeat_wake_delivered file={} fingerprint={fingerprint} due={} expired={} (#poolidleheartbeat)",
                        file.display(),
                        wake.due,
                        wake.expired,
                    ),
                    Err(err) => format!(
                        "subagent_pool_heartbeat_wake_receipt_failed file={} fingerprint={fingerprint} error={err:#} (#poolidleheartbeat)",
                        file.display(),
                    ),
                }
            }
            Ok(Ok(result)) => format!(
                "subagent_pool_heartbeat_wake_not_delivered file={} fingerprint={fingerprint} exit_code={} output={} (#poolidleheartbeat)",
                file.display(),
                result.exit_code,
                result.output.replace('\n', "\\n"),
            ),
            Ok(Err(err)) => format!(
                "subagent_pool_heartbeat_wake_not_delivered file={} fingerprint={fingerprint} error={err:#} (#poolidleheartbeat)",
                file.display(),
            ),
            Err(_) => format!(
                "subagent_pool_heartbeat_wake_not_delivered file={} fingerprint={fingerprint} route=panicked (#poolidleheartbeat)",
                file.display(),
            ),
        };
        agent_doc_ops_log_io::log_op(file, &outcome);
    }
}

fn record_pool_heartbeat_receipt(
    file: &Path,
    project_root: &Path,
    fingerprint: &str,
) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_project_runtime_state_in_db(
        &conn,
        &pool_heartbeat_receipt_key(&agent_doc_fs::document_state_hash(file)?),
        fingerprint,
        controller_model_pressure_now_secs().saturating_mul(1000),
    )
}

/// The tick's cadence and the hold reason each document last logged, so a
/// held wake is logged on the transition and never per tick.
#[derive(Default)]
pub(super) struct PoolHeartbeatTick {
    last_run: Option<Instant>,
    held: std::collections::HashMap<PathBuf, PoolHeartbeatSkip>,
}

impl PoolHeartbeatTick {
    pub(super) fn run_if_due(&mut self, runtime: &Arc<ControllerRuntime>, now: Instant) {
        if self.last_run.is_some_and(|last| {
            now.saturating_duration_since(last) < pool_heartbeat_tick_interval()
        }) {
            return;
        }
        self.last_run = Some(now);
        // `#drainingpredecessor`: a promoted predecessor never routes.
        if runtime.successor_promoted() {
            return;
        }
        if let Err(err) = self.sweep(runtime) {
            eprintln!("[controller] subagent pool heartbeat tick failed: {err:#}");
        }
    }

    fn sweep(&mut self, runtime: &Arc<ControllerRuntime>) -> Result<()> {
        let project_root = runtime.bootstrap_snapshot()?.project_root;
        let conn = open_state_db(&project_root)?;
        let store = load_actor_store_from_db(&conn)?;
        let registry = agent_doc_session_registry_io::load_in(&project_root)?;
        let effects = runtime_effects()?;
        for record in store.values() {
            if record.pane_id.is_empty()
                || record.state == agent_doc_controller::actor::ActorState::Closed
            {
                continue;
            }
            let Some(file) = registry
                .get(&record.document_id)
                .map(|entry| PathBuf::from(&entry.file))
                .filter(|file| !file.as_os_str().is_empty())
            else {
                continue;
            };
            let step = pool_heartbeat_step(
                &conn,
                &project_root,
                &file,
                record.state == agent_doc_controller::actor::ActorState::Ready,
                controller_model_pressure_now_secs(),
                |content| effects.subagent_pool_heartbeat_wake(&file, content),
            );
            match step {
                Ok(PoolHeartbeatStep::NothingDue) => {
                    self.held.remove(&file);
                }
                Ok(PoolHeartbeatStep::Hold(skip, wake)) => {
                    if self.held.insert(file.clone(), skip) != Some(skip) {
                        agent_doc_ops_log_io::log_op(
                            &file,
                            &format!(
                                "subagent_pool_heartbeat_wake_held file={} reason={} due={} expired={} (#poolidleheartbeat)",
                                file.display(),
                                skip.as_str(),
                                wake.due,
                                wake.expired,
                            ),
                        );
                    }
                }
                Ok(PoolHeartbeatStep::Dispatch(wake)) => {
                    self.held.remove(&file);
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "subagent_pool_heartbeat_wake_dispatch file={} pane={} due={} expired={} (#poolidleheartbeat)",
                            file.display(),
                            record.pane_id,
                            wake.due,
                            wake.expired,
                        ),
                    );
                    let queued = enqueue_orphan_drain_route(OrphanDrainRouteJob {
                        invocation: pool_heartbeat_route_invocation(
                            &file,
                            &project_root,
                            &record.pane_id,
                        ),
                        project_root: project_root.clone(),
                        claims_at_decision: agent_doc_queue::queue_claim::ClaimedQueueItems::none(),
                        pool_heartbeat: Some(PoolHeartbeatRoute(wake)),
                    });
                    if !matches!(queued, Ok(true)) {
                        agent_doc_ops_log_io::log_op(
                            &file,
                            &format!(
                                "subagent_pool_heartbeat_wake_not_queued file={} outcome={queued:?} (#poolidleheartbeat)",
                                file.display(),
                            ),
                        );
                    }
                }
                Err(err) => agent_doc_ops_log_io::log_op(
                    &file,
                    &format!(
                        "subagent_pool_heartbeat_tick_failed file={} error={err:#} (#poolidleheartbeat)",
                        file.display(),
                    ),
                ),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_claim::{
        QueueClaim, QueueClaimLedger, QueueClaimState, QueueWorkerBinding,
    };

    const NOW: u64 = 1_800_000_000;
    const TTL: u64 = 7_200;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        file: PathBuf,
        conn: state_store::Connection,
    }

    /// A project whose document has one worker, bound `started_ago` seconds
    /// before [`NOW`] with a full claim window from then.
    fn fixture(started_ago: u64) -> Fixture {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        std::fs::write(&file, "---\nsession: sid\n---\n\n# Plan\n").unwrap();
        let conn = open_state_db(&root).unwrap();
        let started = NOW - started_ago;
        let mut claim: QueueClaim = serde_json::from_value(serde_json::json!({
            "identity": agent_doc_queue::queue_claim::claim_identity("do [#a]"),
            "item_text": "do [#a]",
            "owner": "subagent:a",
            "generation": 1,
            "claimed_at_secs": started,
            "expires_at_secs": started + TTL,
        }))
        .unwrap();
        claim.state = QueueClaimState::Running;
        claim.worker = Some(QueueWorkerBinding {
            worker_id: "w-a".to_string(),
            started_at_secs: started,
        });
        let ledger = QueueClaimLedger {
            claims: vec![claim],
            ..QueueClaimLedger::default()
        };
        state_store::upsert_queue_document_state_in_db(
            &conn,
            &state_store::QueueDocumentStateRecord {
                document_hash: agent_doc_hash::path_hash(&file).unwrap(),
                state_kind: agent_doc_queue::queue_claim::QUEUE_CLAIMS_STATE_KIND.to_string(),
                canonical_path: file.to_string_lossy().into_owned(),
                payload_json: serde_json::to_string(&ledger).unwrap(),
                updated_at_secs: NOW,
            },
        )
        .unwrap();
        Fixture {
            _dir: dir,
            root,
            file,
            conn,
        }
    }

    fn wake(fingerprint: &str) -> PoolHeartbeatWake {
        PoolHeartbeatWake {
            fingerprint: fingerprint.to_string(),
            due: 1,
            expired: 0,
        }
    }

    impl Fixture {
        fn step(&self, now: u64, pane_ready: bool, wake_for: Option<&str>) -> PoolHeartbeatStep {
            pool_heartbeat_step(&self.conn, &self.root, &self.file, pane_ready, now, |_| {
                Ok(wake_for.map(wake))
            })
            .unwrap()
        }
    }

    /// A worker inside the first half of its window costs a ledger read: the
    /// text and the projection stay untouched.
    #[test]
    fn a_quiescent_document_never_reaches_the_projection() {
        let fixture = fixture(60);
        let step = pool_heartbeat_step(
            &fixture.conn,
            &fixture.root,
            &fixture.file,
            true,
            NOW,
            |_| panic!("the projection ran for a worker that cannot be due"),
        )
        .unwrap();
        assert_eq!(step, PoolHeartbeatStep::NothingDue);
    }

    /// The projection is the authority once the ledger allows a due heartbeat:
    /// inside the coalesce window, or with the row gone, it says no.
    #[test]
    fn the_projection_decides_whether_a_ledger_due_heartbeat_wakes() {
        let fixture = fixture(TTL / 2);
        assert_eq!(fixture.step(NOW, true, None), PoolHeartbeatStep::NothingDue);
    }

    /// The live miss: the coordinator is parked with no tool call running and
    /// the controller reaches it, once per distinct due set.
    #[test]
    fn an_idle_coordinator_is_woken_once_per_due_set_and_a_failed_route_retries() {
        let fixture = fixture(TTL / 2);
        let route = |result| {
            PoolHeartbeatRoute(wake("set-1")).settle(&fixture.file, &fixture.root, Ok(result));
        };

        assert_eq!(
            fixture.step(NOW, true, Some("set-1")),
            PoolHeartbeatStep::Dispatch(wake("set-1"))
        );
        assert_eq!(
            fixture.step(NOW + 1, true, Some("set-1")),
            PoolHeartbeatStep::Hold(PoolHeartbeatSkip::RecentlyAttempted, wake("set-1")),
            "the attempt fence holds until the route settles"
        );

        route(Ok(ControllerEditorRouteRuntimeResult {
            exit_code: 3,
            output: "deferred behind a live turn".to_string(),
        }));
        let retry = NOW + POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS;
        assert_eq!(
            fixture.step(retry, true, Some("set-1")),
            PoolHeartbeatStep::Dispatch(wake("set-1")),
            "a deferred route delivered nothing, so it stays due"
        );

        route(Ok(ControllerEditorRouteRuntimeResult {
            exit_code: 0,
            output: "dispatched".to_string(),
        }));
        let later = retry + POOL_HEARTBEAT_MIN_ATTEMPT_INTERVAL_SECS;
        assert_eq!(
            fixture.step(later, true, Some("set-1")),
            PoolHeartbeatStep::Hold(PoolHeartbeatSkip::AlreadyDelivered, wake("set-1")),
            "a delivered due set never wakes twice"
        );
        assert_eq!(
            fixture.step(later, true, Some("set-2")),
            PoolHeartbeatStep::Dispatch(wake("set-2")),
            "a refresh or an expiry is a new due set"
        );
    }

    #[test]
    fn a_busy_pane_holds_the_wake_without_spending_an_attempt() {
        let fixture = fixture(TTL / 2);
        assert_eq!(
            fixture.step(NOW, false, Some("set-1")),
            PoolHeartbeatStep::Hold(PoolHeartbeatSkip::PaneBusy, wake("set-1"))
        );
        assert_eq!(
            fixture.step(NOW + 1, true, Some("set-1")),
            PoolHeartbeatStep::Dispatch(wake("set-1")),
            "the turn's end reaches the coordinator on the next tick"
        );
    }

    #[test]
    fn a_fresh_loop_owner_holds_the_wake() {
        let fixture = fixture(TTL / 2);
        state_store::upsert_coordination_lease_in_db(
            &fixture.conn,
            &state_store::CoordinationLeaseRecord {
                scope_kind: agent_doc_lease::DRAIN_OWNER_SCOPE.to_string(),
                scope_id: agent_doc_fs::document_state_hash(&fixture.file).unwrap(),
                holder: "loop".to_string(),
                holder_pid: None,
                heartbeat_secs: NOW,
            },
        )
        .unwrap();
        assert_eq!(
            fixture.step(NOW, true, Some("set-1")),
            PoolHeartbeatStep::Hold(PoolHeartbeatSkip::LoopOwnsDrain, wake("set-1"))
        );
    }

    /// A heartbeat is no queue head: its route must reach a live actor rather
    /// than yield to an idle watch that will never pick it up, and as automated
    /// work it must defer behind a live turn and touch no other pane.
    #[test]
    fn the_wake_route_is_background_automated_and_never_yields_to_the_idle_watch() {
        let root = Path::new("/project");
        let invocation = pool_heartbeat_route_invocation(&root.join("tasks/plan.md"), root, "%4");
        assert_eq!(invocation.relative_path, "tasks/plan.md");
        assert_eq!(invocation.pane.as_deref(), Some("%4"));
        assert!(invocation.dispatch_only && invocation.plain_trigger);
        assert!(invocation.background_existing_pane_only);
        assert!(invocation.background_recovered_actor_fallback);
        assert!(invocation.trigger_only);
        assert_eq!(
            invocation.dispatch_origin,
            agent_doc_controller::dispatch::AuthoritativeActorDispatchOrigin::AutomatedBackground
        );
    }
}
