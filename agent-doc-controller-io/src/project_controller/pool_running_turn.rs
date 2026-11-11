//! `#subagentworkpool`: paste a pool refill into a turn that is already running.
//!
//! The pure decision is [`agent_doc_controller::pool_running_turn`]. This module
//! is the controller tick: one capture when dispatch debt exists, and a paste
//! only while the pane proves a live turn. An idle composer is left untouched.
//! A Grok status line that says Enter would interrupt a blocked wait is left
//! untouched too (`#blockedwaitentercancels`): that Enter cancels the turn the
//! worker is bound to. The refill stays owed until the wait ends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::live_turn_follow_up::{
    current_steering_cycle_id, mark_live_turn_follow_up_queued,
    refresh_outstanding_live_turn_follow_up, stage_live_turn_follow_up,
};
use agent_doc_controller::live_turn_follow_up::LiveTurnFollowUpSlot;
use agent_doc_controller::pool_running_turn::{
    PoolRunningTurnFollowUpDecision, PoolRunningTurnFollowUpObservation,
    PoolRunningTurnFollowUpSkip, pool_running_turn_follow_up_decision,
};
use agent_doc_harness::HarnessConfig;
use agent_doc_harness::steering_delivery::{BusyDelivery, steering_delivery_adapter};
use agent_doc_queue::subagent_pool::PoolRunningTurnRefill;
use agent_doc_sqlite::state_store;
use anyhow::Result;

use super::pool_heartbeat::pool_heartbeat_document_text;
use super::*;

const POOL_RUNNING_TURN_TICK_INTERVAL: Duration = Duration::from_secs(15);
const POOL_RUNNING_TURN_RECEIPT_KEY_PREFIX: &str = "subagent_pool_running_turn_refill";
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);
const SUBMIT_SETTLE: Duration = Duration::from_millis(80);
const SUBMIT_POLLS: usize = 4;

fn hook_injects(harness: &str) -> bool {
    steering_delivery_adapter(harness).busy == BusyDelivery::PostToolUseHook
}

fn receipt_key(document_hash: &str) -> String {
    format!("{POOL_RUNNING_TURN_RECEIPT_KEY_PREFIX}:{document_hash}")
}

fn record_coalesced_refill(
    file: &Path,
    project_root: &Path,
    pane: &str,
    refill: &PoolRunningTurnRefill,
) {
    let recorded = record_receipt(file, project_root, &refill.fingerprint);
    let fingerprint = refill.fingerprint.get(..12).unwrap_or(&refill.fingerprint);
    let outcome = match recorded {
        Ok(()) => format!(
            "subagent_pool_running_turn_refill_coalesced file={} pane={pane} fingerprint={fingerprint} next_heads={} unstarted={} (#steercoalesce)",
            file.display(),
            refill.next_heads,
            refill.unstarted,
        ),
        Err(err) => format!(
            "subagent_pool_running_turn_refill_receipt_failed file={} fingerprint={fingerprint} error={err:#} (#subagentworkpool)",
            file.display(),
        ),
    };
    agent_doc_ops_log_io::log_op(file, &outcome);
}

fn queue_paused(conn: &state_store::Connection, project_root: &Path, file: &Path) -> Result<bool> {
    let document_id = file
        .canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned();
    Ok(state_store::load_effective_queue_control_from_db(
        conn,
        &document_id,
        &project_root.to_string_lossy(),
    )?
    .is_some_and(|control| control.state == "paused"))
}

fn already_delivered(
    conn: &state_store::Connection,
    file: &Path,
    fingerprint: &str,
) -> Result<bool> {
    let key = receipt_key(&agent_doc_fs::document_state_hash(file)?);
    Ok(state_store::load_project_runtime_state_from_db(conn, &key)?
        .is_some_and(|stored| stored == fingerprint))
}

fn record_receipt(file: &Path, project_root: &Path, fingerprint: &str) -> Result<()> {
    let conn = open_state_db(project_root)?;
    state_store::upsert_project_runtime_state_in_db(
        &conn,
        &receipt_key(&agent_doc_fs::document_state_hash(file)?),
        fingerprint,
        controller_model_pressure_now_secs().saturating_mul(1000),
    )
}

/// What one document owes the running turn, before the pane is captured.
pub(super) fn pool_running_turn_obligation(
    conn: &state_store::Connection,
    project_root: &Path,
    file: &Path,
    harness: &str,
    refill: Option<&PoolRunningTurnRefill>,
) -> Result<PoolRunningTurnObligation> {
    let Some(refill) = refill else {
        return Ok(PoolRunningTurnObligation::Skip(
            PoolRunningTurnFollowUpSkip::NothingToDispatch,
        ));
    };
    let observation = PoolRunningTurnFollowUpObservation {
        next_heads: refill.next_heads,
        unstarted: refill.unstarted,
        delivered: already_delivered(conn, file, &refill.fingerprint)?,
        queue_paused: queue_paused(conn, project_root, file)?,
        hook_injects: hook_injects(harness),
        // Capture is the caller's next step. Asking with the turn live reports
        // whether a paste is still owed; `TurnNotLive` is decided after capture.
        turn_live: true,
    };
    Ok(match pool_running_turn_follow_up_decision(observation) {
        PoolRunningTurnFollowUpDecision::FollowUp => {
            PoolRunningTurnObligation::Capture(refill.clone())
        }
        PoolRunningTurnFollowUpDecision::Skip(skip) => PoolRunningTurnObligation::Skip(skip),
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PoolRunningTurnObligation {
    Skip(PoolRunningTurnFollowUpSkip),
    Capture(PoolRunningTurnRefill),
}

enum PasteOutcome {
    Delivered,
    TurnNotLive,
    /// Grok would cancel the blocked wait on Enter. The worker stays bound.
    BlockedWait,
    NotProven(String),
}

/// Why a capture-proven live turn must not receive Enter.
///
/// `None` means the paste may proceed. Only Grok's blocked-wait hint is a
/// hold: other harnesses queue or hook-inject, and a Grok turn that is merely
/// busy queues the follow-up.
fn running_turn_refill_enter_hold(harness_name: &str, capture: &str) -> Option<&'static str> {
    if steering_delivery_adapter(harness_name).harness != "grok" {
        return None;
    }
    agent_doc_harness::grok_enter_cancels_blocked_wait(capture)
        .then_some("blocked_wait_enter_cancels")
}

fn paste_into_live_turn(pane: &str, harness_name: &str, text: &str) -> PasteOutcome {
    let tmux = tmux_router::Tmux::default_server();
    let harness = HarnessConfig::from_agent_name(harness_name);
    let capture =
        match agent_doc_tmux_io::capture_pane_with_ansi_bounded(&tmux, pane, CAPTURE_TIMEOUT) {
            Ok(capture) => capture,
            Err(err) => {
                return PasteOutcome::NotProven(format!("the pane could not be read ({err:#})"));
            }
        };
    if !harness.has_busy_cue(&capture) {
        return PasteOutcome::TurnNotLive;
    }
    if running_turn_refill_enter_hold(harness_name, &capture).is_some() {
        return PasteOutcome::BlockedWait;
    }
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(&tmux, pane);
    match agent_doc_harness::steering_delivery::running_turn_follow_up_composer_at_cursor(
        harness_name,
        &capture,
        cursor_y,
    ) {
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {}
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft => {
            return PasteOutcome::NotProven("the composer holds an unsent draft".to_string());
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked { reason } => {
            return PasteOutcome::NotProven(reason);
        }
        agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
            return PasteOutcome::NotProven("the composer could not be read".to_string());
        }
    }
    if let Err(err) =
        agent_doc_tmux_io::paste_bracketed_text_and_submit(&tmux, pane, text, SUBMIT_SETTLE)
    {
        return PasteOutcome::NotProven(format!("tmux did not accept the follow-up ({err:#})"));
    }
    match prove_follow_up_left_the_composer(&tmux, pane, harness_name) {
        Ok(()) => PasteOutcome::Delivered,
        Err(reason) => PasteOutcome::NotProven(reason),
    }
}

fn prove_follow_up_left_the_composer(
    tmux: &tmux_router::Tmux,
    pane: &str,
    harness_name: &str,
) -> Result<(), String> {
    let mut last = String::from("the submitted refill could not be proven in the busy composer");
    for attempt in 0..2 {
        for _ in 0..SUBMIT_POLLS {
            std::thread::sleep(SUBMIT_SETTLE);
            let capture =
                agent_doc_tmux_io::capture_pane_with_ansi_bounded(tmux, pane, CAPTURE_TIMEOUT)
                    .map_err(|err| format!("the pane could not be read ({err:#})"))?;
            match agent_doc_harness::steering_delivery::running_turn_follow_up_composer_at_cursor(
                harness_name,
                &capture,
                agent_doc_tmux_io::pane_cursor_y(tmux, pane),
            ) {
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Empty => {
                    return Ok(());
                }
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Draft => {
                    last = "the composer still holds the refill draft".to_string();
                }
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Blocked {
                    reason,
                } => return Err(reason),
                agent_doc_harness::steering_delivery::RunningTurnFollowUpComposer::Unreadable => {
                    last = "the composer could not be read after the paste".to_string();
                }
            }
        }
        if attempt == 0 && last.contains("draft") {
            agent_doc_tmux_io::send_key(tmux, pane, "Enter")
                .map_err(|err| format!("tmux did not accept a follow-up submit retry ({err:#})"))?;
        }
    }
    Err(last)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefillHold {
    TurnNotLive,
    BlockedWaitEnterCancels,
}

#[derive(Default)]
pub(super) struct PoolRunningTurnTick {
    last_run: Option<Instant>,
    held: HashMap<PathBuf, RefillHold>,
}

impl PoolRunningTurnTick {
    fn hold(&mut self, file: &Path, hold: RefillHold, reason: &str, next_heads: usize) {
        if self.held.insert(file.to_path_buf(), hold) == Some(hold) {
            return;
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "subagent_pool_running_turn_refill_held file={} reason={reason} next_heads={next_heads} (#subagentworkpool)",
                file.display(),
            ),
        );
    }

    pub(super) fn run_if_due(&mut self, runtime: &Arc<ControllerRuntime>, now: Instant) {
        if self.last_run.is_some_and(|last| {
            now.saturating_duration_since(last) < POOL_RUNNING_TURN_TICK_INTERVAL
        }) {
            return;
        }
        self.last_run = Some(now);
        if runtime.successor_promoted() {
            return;
        }
        if let Err(err) = self.sweep(runtime) {
            eprintln!("[controller] subagent pool running-turn refill tick failed: {err:#}");
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
            // A quiet gap longer than the compact-token ceiling must not mint
            // a second queue row for the cycle that is still running.
            let now_ms = controller_model_pressure_now_secs().saturating_mul(1000);
            if let Err(err) = refresh_outstanding_live_turn_follow_up(&file, now_ms) {
                agent_doc_ops_log_io::log_op(
                    &file,
                    &format!(
                        "live_turn_follow_up_refresh_failed file={} error={err:#} (#steercoalesce)",
                        file.display(),
                    ),
                );
            }
            let step = (|| -> Result<PoolRunningTurnObligation> {
                let Some(content) = pool_heartbeat_document_text(&file)? else {
                    return Ok(PoolRunningTurnObligation::Skip(
                        PoolRunningTurnFollowUpSkip::NothingToDispatch,
                    ));
                };
                let refill =
                    effects.subagent_pool_running_turn_refill(&file, &content, &record.harness)?;
                pool_running_turn_obligation(
                    &conn,
                    &project_root,
                    &file,
                    &record.harness,
                    refill.as_ref(),
                )
            })();
            let obligation = match step {
                Ok(obligation) => obligation,
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "subagent_pool_running_turn_refill_failed file={} error={err:#} (#subagentworkpool)",
                            file.display(),
                        ),
                    );
                    continue;
                }
            };
            let PoolRunningTurnObligation::Capture(refill) = obligation else {
                if let PoolRunningTurnObligation::Skip(skip) = obligation {
                    self.held.remove(&file);
                    let _ = skip;
                }
                continue;
            };
            let cycle = match current_steering_cycle_id(&file) {
                Ok(cycle) => cycle.filter(|cycle| !cycle.trim().is_empty()),
                Err(err) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "subagent_pool_running_turn_refill_not_delivered file={} reason=cycle_unreadable error={err:#} (#steercoalesce)",
                            file.display(),
                        ),
                    );
                    continue;
                }
            };
            // No steering cycle: paste the refill itself, as before this
            // follow-up existed. With a cycle, the first Enter is the compact
            // reference and every later snapshot updates that payload.
            let queued_notice = if let Some(cycle) = cycle.as_deref() {
                match stage_live_turn_follow_up(
                    &file,
                    cycle,
                    LiveTurnFollowUpSlot::Pool,
                    &refill.text,
                    now_ms,
                ) {
                    Ok(staged) if staged.needs_queue_paste => Some(staged.notice),
                    Ok(_) => {
                        self.held.remove(&file);
                        record_coalesced_refill(&file, &project_root, &record.pane_id, &refill);
                        continue;
                    }
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            &file,
                            &format!(
                                "subagent_pool_running_turn_refill_not_delivered file={} reason=stage_failed error={err:#} (#steercoalesce)",
                                file.display(),
                            ),
                        );
                        continue;
                    }
                }
            } else {
                Some(refill.text.clone())
            };
            let Some(text) = queued_notice else {
                continue;
            };
            match paste_into_live_turn(&record.pane_id, &record.harness, &text) {
                PasteOutcome::Delivered => {
                    self.held.remove(&file);
                    if let Some(cycle) = cycle.as_deref()
                        && let Err(err) = mark_live_turn_follow_up_queued(&file, cycle)
                    {
                        agent_doc_ops_log_io::log_op(
                            &file,
                            &format!(
                                "live_turn_follow_up_mark_failed file={} cycle={cycle} error={err:#} (#steercoalesce)",
                                file.display(),
                            ),
                        );
                    }
                    let recorded = record_receipt(&file, &project_root, &refill.fingerprint);
                    let fingerprint = refill.fingerprint.get(..12).unwrap_or(&refill.fingerprint);
                    let outcome = match recorded {
                        Ok(()) => format!(
                            "subagent_pool_running_turn_refill_delivered file={} pane={} fingerprint={fingerprint} next_heads={} unstarted={} (#subagentworkpool)",
                            file.display(),
                            record.pane_id,
                            refill.next_heads,
                            refill.unstarted,
                        ),
                        Err(err) => format!(
                            "subagent_pool_running_turn_refill_receipt_failed file={} fingerprint={fingerprint} error={err:#} (#subagentworkpool)",
                            file.display(),
                        ),
                    };
                    agent_doc_ops_log_io::log_op(&file, &outcome);
                }
                PasteOutcome::TurnNotLive => {
                    self.hold(
                        &file,
                        RefillHold::TurnNotLive,
                        "turn_not_live",
                        refill.next_heads,
                    );
                }
                PasteOutcome::BlockedWait => {
                    self.hold(
                        &file,
                        RefillHold::BlockedWaitEnterCancels,
                        "blocked_wait_enter_cancels",
                        refill.next_heads,
                    );
                }
                PasteOutcome::NotProven(reason) => {
                    agent_doc_ops_log_io::log_op(
                        &file,
                        &format!(
                            "subagent_pool_running_turn_refill_not_delivered file={} pane={} reason={} (#subagentworkpool)",
                            file.display(),
                            record.pane_id,
                            reason.replace('\n', " "),
                        ),
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refill(next_heads: usize) -> PoolRunningTurnRefill {
        PoolRunningTurnRefill {
            fingerprint: format!("heads-{next_heads}"),
            text: format!("claim the {next_heads} new heads"),
            next_heads,
            unstarted: 0,
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        file: PathBuf,
        conn: state_store::Connection,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        std::fs::write(&file, "---\nsession: sid\n---\n\n# Plan\n").unwrap();
        let conn = open_state_db(&root).unwrap();
        Fixture {
            _dir: dir,
            root,
            file,
            conn,
        }
    }

    /// Restoring a stashed Grok pane must not press Enter while the turn is
    /// blocked on its worker. The hint is the status line Grok prints for that
    /// wait; a still-running count without it still queues.
    #[test]
    fn a_blocked_wait_holds_the_grok_refill_and_a_busy_turn_does_not() {
        let busy = "╭──╮\n│ ❯ │\n╰──╯\nCtrl+c:cancel │ Ctrl+x:shortcuts\n";
        assert_eq!(
            running_turn_refill_enter_hold("grok", busy),
            None,
            "a generating turn queues the refill"
        );
        assert_eq!(
            running_turn_refill_enter_hold("grok", &format!("◎  1 subagent still running\n{busy}"),),
            None
        );
        assert_eq!(
            running_turn_refill_enter_hold(
                "grok",
                &format!("◎  1 subagent still running · send a message to interrupt\n{busy}"),
            ),
            Some("blocked_wait_enter_cancels")
        );
        assert_eq!(
            running_turn_refill_enter_hold(
                "claude",
                "◎  waiting · send a message to interrupt\nesc to interrupt\n",
            ),
            None,
            "claude injects through its hook and never takes this paste"
        );
    }

    /// The stuck pool: Grok, two new heads, nothing delivered yet. The tick
    /// must still owe a capture of the live turn.
    #[test]
    fn two_new_heads_on_a_hookless_harness_owe_a_live_turn_paste() {
        let fixture = fixture();
        let owed = refill(2);
        let obligation = pool_running_turn_obligation(
            &fixture.conn,
            &fixture.root,
            &fixture.file,
            "grok",
            Some(&owed),
        )
        .unwrap();
        assert_eq!(obligation, PoolRunningTurnObligation::Capture(owed));
    }

    #[test]
    fn a_delivered_fingerprint_does_not_paste_again() {
        let fixture = fixture();
        let owed = refill(2);
        record_receipt(&fixture.file, &fixture.root, &owed.fingerprint).unwrap();
        let obligation = pool_running_turn_obligation(
            &fixture.conn,
            &fixture.root,
            &fixture.file,
            "grok",
            Some(&owed),
        )
        .unwrap();
        assert_eq!(
            obligation,
            PoolRunningTurnObligation::Skip(PoolRunningTurnFollowUpSkip::AlreadyDelivered)
        );
    }

    #[test]
    fn claude_keeps_the_hook_and_does_not_owe_a_paste() {
        let fixture = fixture();
        let obligation = pool_running_turn_obligation(
            &fixture.conn,
            &fixture.root,
            &fixture.file,
            "claude",
            Some(&refill(2)),
        )
        .unwrap();
        assert_eq!(
            obligation,
            PoolRunningTurnObligation::Skip(PoolRunningTurnFollowUpSkip::HookInjects)
        );
    }
}
