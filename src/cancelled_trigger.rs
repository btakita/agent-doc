//! `#cancelturnprompt`: withdraw agent-doc's own trigger after Claude Code
//! restores a cancelled prompt into the composer.
//!
//! Live 2026-10-11 (agent-loop `tasks/agent-doc/agent-doc-bugs.md`, pane %149):
//! Run Agent Doc typed `/agent-doc <file>` at 05:59:23 and Cancel Turn sent
//! `C-c` at 05:59:30, while the prompt's `UserPromptSubmit` preflight hook was
//! still running. Claude Code abandons such a prompt and puts it back in the
//! composer, with no prompt record, no interrupt record and no `Stop` hook. The
//! composer then held agent-doc's trigger as if the operator had typed it:
//! Clear Session Context had nothing safe to type onto, the next Enter re-ran
//! the cancelled turn, and the turn-active lease stayed live until the
//! abandoned-prompt settle window (`#sdkclearwedge`) or the TTL.
//!
//! The policy lives in
//! [`agent_doc_controller::dispatch::cancelled_trigger_composer_action`]; this
//! module only observes the pane, sends the one withdrawal key, and verifies.
//! [`CancelledTriggerPane`] is the seam: production drives tmux, tests drive a
//! simulated composer on a virtual clock.

use std::path::Path;
use std::time::Duration;

use agent_doc_controller::dispatch::{
    CancelledTriggerComposerAction, cancelled_trigger_composer_action,
};

/// How long Cancel Turn watches for Claude Code to restore the cancelled
/// prompt. The 2026-10-11 restore (prompt snapshot) landed 0.7s after `C-c`.
pub(crate) const RESTORE_WINDOW: Duration = Duration::from_millis(2_000);
/// How long the withdrawal key has to take effect before agent-doc stops and
/// reports it unverified (it never sends a second key).
pub(crate) const WITHDRAW_VERIFY_WINDOW: Duration = Duration::from_millis(600);
/// How long to wait for the abandoned prompt's snapshot before leaving the
/// lease to the ordinary settle-window rule.
pub(crate) const SNAPSHOT_WINDOW: Duration = Duration::from_millis(2_000);
pub(crate) const POLL: Duration = Duration::from_millis(100);
/// GH #317: how long a withdrawal waits for a turn-active lease before it
/// concludes the withdrawn trigger was never submitted. The lease is written by
/// the harness's own `UserPromptSubmit` hook (`turn-status active`), which runs
/// alongside preflight the moment a prompt is accepted; this only covers that
/// hook's process start-up, should an Enter have landed just before `C-u`.
pub(crate) const UNSUBMITTED_LEASE_WINDOW: Duration = Duration::from_millis(1_000);

/// One composer observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposerObservation {
    /// The operator-draft preview, `None` for an empty, busy or absent composer.
    pub draft: Option<String>,
    pub busy_cue: bool,
}

/// The pane, harness and clock the withdrawal acts on.
pub(crate) trait CancelledTriggerPane {
    /// Observe the composer; `None` when the pane cannot be captured.
    fn observe(&mut self) -> Option<ComposerObservation>;
    /// Is a route submit in flight for this document? Errors read as `true`.
    fn route_submit_in_flight(&mut self) -> bool;
    /// Send the single key that empties the composer line.
    fn send_withdraw_key(&mut self) -> anyhow::Result<()>;
    fn sleep(&mut self, duration: Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WithdrawOutcome {
    /// The restored trigger was in the composer and is gone now.
    Withdrawn,
    /// No trigger was restored within the window.
    NotRestored,
    /// The composer holds operator input; untouched.
    OperatorDraftKept { preview: String },
    /// An exact trigger stayed held (route in flight / busy) until the window
    /// closed; untouched.
    Held,
    /// The pane could not be observed; untouched.
    Unobservable,
    /// The withdrawal key was sent but the trigger is still there.
    Unverified,
    /// Sending the key failed.
    SendFailed { error: String },
}

impl WithdrawOutcome {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Withdrawn => "withdrawn",
            Self::NotRestored => "not_restored",
            Self::OperatorDraftKept { .. } => "operator_draft_kept",
            Self::Held => "held",
            Self::Unobservable => "unobservable",
            Self::Unverified => "unverified",
            Self::SendFailed { .. } => "send_failed",
        }
    }
}

fn decide(
    pane: &mut impl CancelledTriggerPane,
    triggers: &[&str],
) -> Option<(CancelledTriggerComposerAction, Option<String>)> {
    let observation = pane.observe()?;
    // Only ask the controller when there is something to protect.
    let route_in_flight = observation.draft.is_some() && pane.route_submit_in_flight();
    let action = cancelled_trigger_composer_action(
        observation.draft.as_deref(),
        triggers,
        observation.busy_cue,
        route_in_flight,
    );
    Some((action, observation.draft))
}

/// Watch the composer for up to `window` and withdraw agent-doc's restored
/// trigger once it is observed idle. `window == Duration::ZERO` observes once.
pub(crate) fn withdraw_restored_trigger(
    pane: &mut impl CancelledTriggerPane,
    triggers: &[&str],
    window: Duration,
) -> WithdrawOutcome {
    let mut waited = Duration::ZERO;
    loop {
        let pending = match decide(pane, triggers) {
            None => WithdrawOutcome::Unobservable,
            Some((CancelledTriggerComposerAction::KeepOperatorDraft, draft)) => {
                return WithdrawOutcome::OperatorDraftKept {
                    preview: draft.unwrap_or_default(),
                };
            }
            Some((CancelledTriggerComposerAction::WithdrawRestoredTrigger, _)) => {
                return send_and_verify(pane, triggers);
            }
            Some((CancelledTriggerComposerAction::Hold, _)) => WithdrawOutcome::Held,
            Some((CancelledTriggerComposerAction::WaitForRestore, _)) => {
                WithdrawOutcome::NotRestored
            }
        };
        if waited >= window {
            return pending;
        }
        pane.sleep(POLL);
        waited += POLL;
    }
}

fn send_and_verify(pane: &mut impl CancelledTriggerPane, triggers: &[&str]) -> WithdrawOutcome {
    if let Err(error) = pane.send_withdraw_key() {
        return WithdrawOutcome::SendFailed {
            error: format!("{error:#}"),
        };
    }
    let mut waited = Duration::ZERO;
    while waited < WITHDRAW_VERIFY_WINDOW {
        pane.sleep(POLL);
        waited += POLL;
        if let Some(observation) = pane.observe() {
            let still_there = observation.draft.as_deref().is_some_and(|draft| {
                cancelled_trigger_composer_action(Some(draft), triggers, false, false)
                    == CancelledTriggerComposerAction::WithdrawRestoredTrigger
            });
            if !still_there {
                return WithdrawOutcome::Withdrawn;
            }
        }
    }
    WithdrawOutcome::Unverified
}

/// The trigger spellings agent-doc injects for `file`: the absolute path and,
/// when `file` is under `base_dir`, the project-relative path.
pub(crate) fn trigger_candidates(
    harness: &agent_doc_harness::HarnessConfig,
    file: &Path,
    base_dir: &Path,
) -> Vec<String> {
    let mut triggers = vec![harness.trigger_command(&file.display().to_string())];
    if let Ok(relative) = file.strip_prefix(base_dir) {
        let relative = harness.trigger_command(&relative.display().to_string());
        if !triggers.contains(&relative) {
            triggers.push(relative);
        }
    }
    triggers
}

/// Production pane: tmux capture through the shared composer projection.
pub(crate) struct TmuxCancelledTriggerPane<'a> {
    pub tmux: &'a tmux_router::Tmux,
    pub pane: &'a str,
    pub file: &'a Path,
    pub harness: &'a agent_doc_harness::HarnessConfig,
    pub source: &'static str,
}

impl CancelledTriggerPane for TmuxCancelledTriggerPane<'_> {
    fn observe(&mut self) -> Option<ComposerObservation> {
        let capture = agent_doc_tmux_io::capture_pane_with_ansi(self.tmux, self.pane).ok()?;
        let cursor_y = agent_doc_tmux_io::pane_cursor_y(self.tmux, self.pane);
        let draft = match agent_doc_harness::project_pane_composer_at_cursor(
            &capture,
            self.harness,
            cursor_y,
        ) {
            agent_doc_harness::PaneComposerProjection::OperatorDraft { preview } => Some(preview),
            _ => None,
        };
        Some(ComposerObservation {
            draft,
            busy_cue: self.harness.has_busy_cue(&capture),
        })
    }

    fn route_submit_in_flight(&mut self) -> bool {
        agent_doc_controller_io::project_controller::route_submit_in_flight_for_file(self.file)
            .unwrap_or(true)
    }

    fn send_withdraw_key(&mut self) -> anyhow::Result<()> {
        agent_doc_tmux_io::send_key_logged(
            self.tmux,
            self.pane,
            "C-u",
            agent_doc_tmux_io::input_diag::InputDiagSink::new(
                Some(self.file),
                agent_doc_ops_log_io::log_op,
            ),
            self.source,
        )
        .map_err(|err| anyhow::anyhow!("{err}"))
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// After a withdrawal, retire the lease of the abandoned prompt as soon as its
/// snapshot lands (no settle window: the withdrawal removed the ambiguity), and
/// settle the dispatch receipt that turn's preflight promoted to `running`.
/// Returns the ops-log fields describing the result.
///
/// GH #317: when the pane still holds NO turn-active lease once
/// [`UNSUBMITTED_LEASE_WINDOW`] has passed, the withdrawn trigger never reached `UserPromptSubmit` — it was typed but never
/// submitted as a turn (live 2026-10-20, pane %50: no lease, no
/// preflight, no `dispatch_turn_started`). There is no abandoned prompt
/// snapshot to wait for, and its dispatch receipt is still `accepted`; left
/// alone it answered `dispatch_turn_pending` and refused the very clear that
/// withdrew it, for the whole pre-turn grace. The withdrawal proves that turn
/// will never run, so its receipt is settled here and the clear can proceed.
pub(crate) fn retire_withdrawn_prompt_turn(
    file: &Path,
    pane: &str,
    window: Duration,
    sleep: &mut impl FnMut(Duration),
) -> String {
    let mut waited = Duration::ZERO;
    loop {
        if let Some(retired) =
            agent_doc_turn_status_io::reclaim_withdrawn_prompt_turn_for_pane_for_file(file, pane)
        {
            // `#infraclearsession`: the retirement already settled the pane's
            // receipt; this file-scoped settle covers a document with no
            // `documents.pane_id` row yet and is otherwise a no-op.
            let settled = retired.settled_dispatches
                + agent_doc_controller_io::project_controller::settle_dispatch_turn_ended_by_harness_for_file(
                    file,
                    retired.written_at,
                )
                .unwrap_or(0);
            return format!(
                "lease=retired reason={} lease_written_at={} snapshot_at={} settled_dispatches={}",
                retired.reason, retired.written_at, retired.interrupted_at, settled
            );
        }
        // GH #317: no lease at all once the hook-latency window has passed —
        // the withdrawn trigger was never submitted, so no snapshot will come.
        if waited >= UNSUBMITTED_LEASE_WINDOW.min(window)
            && agent_doc_turn_status_io::turn_active_marker_for_pane_for_file(file, pane).is_none()
        {
            return settle_withdrawn_unsubmitted_trigger(file);
        }
        if waited >= window {
            return "lease=kept reason=no_abandoned_snapshot_for_lease note=settle_window_rule_applies"
                .to_string();
        }
        sleep(POLL);
        waited += POLL;
    }
}

/// GH #317: settle the never-started dispatch receipt of a withdrawn trigger
/// that never reached `UserPromptSubmit`. Returns the ops-log fields.
fn settle_withdrawn_unsubmitted_trigger(file: &Path) -> String {
    let withdrawn_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    let settled =
        agent_doc_controller_io::project_controller::settle_withdrawn_unsubmitted_dispatch_turn_for_file(
            file,
            withdrawn_at,
        );
    format!(
        "lease=none reason=trigger_never_submitted settled_pending_dispatches={}",
        settled.map_or_else(|err| format!("error:{err:#}"), |count| count.to_string())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABS: &str =
        "/agent-doc /home/brian/work/btakita/agent-loop/tasks/agent-doc/agent-doc-bugs.md";

    /// SimWorld composer on a virtual clock: Claude Code's restore lands at
    /// `restore_at`, `C-u` empties a single-line composer (unless `deaf`), a
    /// route submit may be in flight until `route_until`, and the busy cue
    /// lingers until `busy_until`.
    struct SimComposer {
        now: Duration,
        draft: Option<String>,
        restore_at: Option<(Duration, String)>,
        busy_until: Duration,
        route_until: Duration,
        deaf: bool,
        keys: Vec<Duration>,
        unobservable: bool,
    }

    impl SimComposer {
        fn new() -> Self {
            Self {
                now: Duration::ZERO,
                draft: None,
                restore_at: None,
                busy_until: Duration::ZERO,
                route_until: Duration::ZERO,
                deaf: false,
                keys: Vec::new(),
                unobservable: false,
            }
        }

        fn restore(mut self, at_ms: u64, text: &str) -> Self {
            self.restore_at = Some((Duration::from_millis(at_ms), text.to_string()));
            self
        }

        fn apply_due(&mut self) {
            if let Some((at, text)) = self.restore_at.clone()
                && self.now >= at
            {
                self.draft = Some(text);
                self.restore_at = None;
            }
        }
    }

    impl CancelledTriggerPane for SimComposer {
        fn observe(&mut self) -> Option<ComposerObservation> {
            self.apply_due();
            if self.unobservable {
                return None;
            }
            Some(ComposerObservation {
                draft: self.draft.clone(),
                busy_cue: self.now < self.busy_until,
            })
        }

        fn route_submit_in_flight(&mut self) -> bool {
            self.now < self.route_until
        }

        fn send_withdraw_key(&mut self) -> anyhow::Result<()> {
            self.keys.push(self.now);
            if !self.deaf {
                self.draft = None;
            }
            Ok(())
        }

        fn sleep(&mut self, duration: Duration) {
            self.now += duration;
            self.apply_due();
        }
    }

    #[test]
    fn incident_restore_after_cancel_is_withdrawn_with_one_key() {
        // The 2026-10-11 restore landed ~0.7s after Cancel Turn's C-c, while
        // Claude's busy cue still showed for a moment.
        let mut pane = SimComposer::new().restore(700, &format!("❯ {ABS}"));
        pane.busy_until = Duration::from_millis(400);
        let outcome = withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW);
        assert_eq!(outcome, WithdrawOutcome::Withdrawn);
        assert_eq!(pane.keys, vec![Duration::from_millis(700)]);
        assert_eq!(pane.draft, None);
    }

    #[test]
    fn a_turn_that_really_ran_leaves_an_empty_composer_and_gets_no_key() {
        let mut pane = SimComposer::new();
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW),
            WithdrawOutcome::NotRestored
        );
        assert!(pane.keys.is_empty());
    }

    #[test]
    fn operator_text_is_never_withdrawn() {
        for text in [
            "please look at sdk.md instead".to_string(),
            format!("{ABS} but skip the tests"),
        ] {
            let mut pane = SimComposer::new().restore(300, &text);
            let outcome = withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW);
            assert!(
                matches!(outcome, WithdrawOutcome::OperatorDraftKept { .. }),
                "{text:?} -> {outcome:?}"
            );
            assert!(pane.keys.is_empty());
            assert_eq!(pane.draft.as_deref(), Some(text.as_str()));
        }
    }

    #[test]
    fn fails_closed_while_a_route_types_or_the_turn_still_runs() {
        // A route submit in flight for the whole window: the trigger may be a
        // fresh injection about to get its Enter.
        let mut pane = SimComposer::new().restore(0, ABS);
        pane.route_until = Duration::from_secs(60);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW),
            WithdrawOutcome::Held
        );
        assert!(pane.keys.is_empty());
        // A busy cue that never clears: the turn may still be running.
        let mut pane = SimComposer::new().restore(0, ABS);
        pane.busy_until = Duration::from_secs(60);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW),
            WithdrawOutcome::Held
        );
        assert!(pane.keys.is_empty());
        // Once the route settles inside the window, the trigger is withdrawn.
        let mut pane = SimComposer::new().restore(0, ABS);
        pane.route_until = Duration::from_millis(500);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW),
            WithdrawOutcome::Withdrawn
        );
        assert_eq!(pane.keys, vec![Duration::from_millis(500)]);
    }

    #[test]
    fn an_ineffective_key_is_reported_and_never_repeated() {
        let mut pane = SimComposer::new().restore(0, ABS);
        pane.deaf = true;
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], RESTORE_WINDOW),
            WithdrawOutcome::Unverified
        );
        assert_eq!(pane.keys.len(), 1);
    }

    #[test]
    fn an_unobservable_pane_is_untouched() {
        let mut pane = SimComposer::new().restore(0, ABS);
        pane.unobservable = true;
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], Duration::ZERO),
            WithdrawOutcome::Unobservable
        );
        assert!(pane.keys.is_empty());
    }

    #[test]
    fn single_observation_mode_for_the_clear_guard() {
        let mut pane = SimComposer::new().restore(0, ABS);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], Duration::ZERO),
            WithdrawOutcome::Withdrawn
        );
        // Nothing restored yet: one look, no waiting.
        let mut pane = SimComposer::new().restore(700, ABS);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], Duration::ZERO),
            WithdrawOutcome::NotRestored
        );
        assert_eq!(pane.now, Duration::ZERO);
    }

    /// The production observation path: Claude's composer projection must
    /// report the restored trigger as a draft the policy recognizes, and the
    /// emptied composer as no draft. Pane shape from %149 on 2026-10-11 (the
    /// `ops.log` tail `⏵⏵ bypass permissions on · 1 shell`).
    #[test]
    fn claude_projection_of_the_restored_trigger_is_withdrawable() {
        let harness = agent_doc_harness::HarnessConfig::from_agent_name("claude");
        let restored = format!(
            concat!(
                "  Pushed fix/autosync-preserve-two-pane.\n",
                "✻ Worked for 13s\n",
                "────────────────────────────────────────\n",
                "❯ {}\n",
                "────────────────────────────────────────\n",
                "  Opus 5.5 ctx:31% ~/work/btakita/agent-loop brian@cachyos-x8664\n",
                "  ⏵⏵ bypass permissions on · 1 shell\n",
            ),
            ABS
        );
        let draft = match agent_doc_harness::project_pane_composer_at_cursor(
            &restored,
            &harness,
            Some(3),
        ) {
            agent_doc_harness::PaneComposerProjection::OperatorDraft { preview } => preview,
            other => panic!("restored trigger must project as a draft, got {other:?}"),
        };
        assert!(!harness.has_busy_cue(&restored));
        assert_eq!(
            cancelled_trigger_composer_action(Some(&draft), &[ABS], false, false),
            CancelledTriggerComposerAction::WithdrawRestoredTrigger
        );
        let emptied = restored.replace(&format!("❯ {ABS}"), "❯ ");
        assert!(!matches!(
            agent_doc_harness::project_pane_composer_at_cursor(&emptied, &harness, Some(3)),
            agent_doc_harness::PaneComposerProjection::OperatorDraft { .. }
        ));
    }

    #[test]
    fn trigger_candidates_cover_absolute_and_project_relative_paths() {
        let harness = agent_doc_harness::HarnessConfig::from_agent_name("claude");
        let base = Path::new("/home/brian/work/btakita/agent-loop");
        let file = base.join("tasks/agent-doc/agent-doc-bugs.md");
        let triggers = trigger_candidates(&harness, &file, base);
        assert_eq!(
            triggers,
            vec![
                ABS.to_string(),
                "/agent-doc tasks/agent-doc/agent-doc-bugs.md".to_string()
            ]
        );
    }

    /// A scratch project holding one session document and its state DB.
    struct ScratchProject {
        _dir: tempfile::TempDir,
        root: std::path::PathBuf,
        doc: std::path::PathBuf,
    }

    impl ScratchProject {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
            let doc = root.join("tasks/agent-doc/agent-doc-bugs.md");
            std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
            std::fs::write(&doc, "---\nagent_doc_session: session-1\n---\n\nBody.\n").unwrap();
            Self {
                _dir: dir,
                root,
                doc,
            }
        }

        /// Run Agent Doc accepted `secs_ago`: a `dispatch_only_reopen` receipt
        /// whose turn never started.
        fn accept_dispatch(&self, secs_ago: u64) {
            let document_id = agent_doc_session_actor_io::canonical_document_id_in(
                &self.root,
                &self.doc.to_string_lossy(),
            );
            let conn = agent_doc_sqlite::state_store::open_state_db(&self.root).unwrap();
            agent_doc_sqlite::state_store::insert_dispatch_attempt_in_db(
                &conn,
                &agent_doc_sqlite::state_store::DispatchAttemptInsert {
                    document_id: &document_id,
                    generation: 6,
                    command_kind: "dispatch_only_reopen",
                    accepted_stage: Some("ready"),
                    failed_stage: None,
                    diagnostic_payload: "{}",
                    result_status: "accepted",
                    proof_scope: "accepted_only",
                    dispatch_start_proven: false,
                },
            )
            .unwrap();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            conn.execute(
                "UPDATE dispatch_attempts SET timestamp = ?1 WHERE id = last_insert_rowid()",
                [i64::try_from(now - secs_ago).unwrap()],
            )
            .unwrap();
        }

        fn dispatch_turn_pending(&self) -> bool {
            agent_doc_controller_io::project_controller::dispatch_turn_pending_for_file(&self.doc)
                .unwrap()
        }
    }

    /// GH #317 replay (agent-loop, pane %50, Claude Code, 2026-10-20): Run Agent
    /// Doc typed the trigger at 01:58:24 but it never became a turn — no
    /// `UserPromptSubmit`, so no turn-active lease and no preflight. Clear
    /// Session Context withdrew the leftover trigger from the composer
    /// (`outcome=withdrawn lease=kept reason=no_abandoned_snapshot_for_lease`)
    /// and was then refused on `evidence=dispatch_turn_pending` by the very
    /// receipt it had just withdrawn. The withdrawal now settles that receipt.
    #[test]
    fn withdrawing_a_never_submitted_trigger_releases_its_pending_dispatch() {
        let project = ScratchProject::new();
        project.accept_dispatch(9);
        assert!(project.dispatch_turn_pending());

        // The composer still holds the trigger, idle, no route in flight.
        let mut pane = SimComposer::new().restore(0, ABS);
        assert_eq!(
            withdraw_restored_trigger(&mut pane, &[ABS], Duration::ZERO),
            WithdrawOutcome::Withdrawn
        );

        let mut waited = Duration::ZERO;
        let detail = retire_withdrawn_prompt_turn(&project.doc, "%50", SNAPSHOT_WINDOW, &mut |d| {
            waited += d
        });
        assert_eq!(
            detail,
            "lease=none reason=trigger_never_submitted settled_pending_dispatches=1"
        );
        assert_eq!(
            waited, UNSUBMITTED_LEASE_WINDOW,
            "no abandoned snapshot will ever land; only the hook-latency window is waited"
        );
        assert!(
            !project.dispatch_turn_pending(),
            "the clear that withdrew the trigger is no longer refused on dispatch_turn_pending"
        );
    }

    /// One withdrawal settles one receipt: a second Run Agent Doc stacked behind
    /// the withdrawn trigger is still a pending turn.
    #[test]
    fn a_stacked_second_dispatch_stays_pending_after_one_withdrawal() {
        let project = ScratchProject::new();
        project.accept_dispatch(9);
        project.accept_dispatch(3);
        let detail =
            retire_withdrawn_prompt_turn(&project.doc, "%50", SNAPSHOT_WINDOW, &mut |_| {});
        assert!(detail.ends_with("settled_pending_dispatches=1"), "{detail}");
        assert!(project.dispatch_turn_pending());
    }

    /// The race the hook-latency window covers: the Enter landed just before
    /// `C-u`, Claude Code accepted the prompt, and its `UserPromptSubmit` hook
    /// writes the turn-active lease 300ms later. That turn did start, so its
    /// receipt must not be settled as never submitted.
    #[test]
    fn a_lease_written_inside_the_hook_window_keeps_the_dispatch() {
        let project = ScratchProject::new();
        project.accept_dispatch(2);
        let mut now = Duration::ZERO;
        let mut lease_written = false;
        let detail = retire_withdrawn_prompt_turn(&project.doc, "%50", SNAPSHOT_WINDOW, &mut |d| {
            now += d;
            if !lease_written && now >= Duration::from_millis(300) {
                agent_doc_turn_status_io::write_turn_active_marker(&project.root, "%50").unwrap();
                lease_written = true;
            }
        });
        assert_eq!(
            detail,
            "lease=kept reason=no_abandoned_snapshot_for_lease note=settle_window_rule_applies"
        );
        assert!(project.dispatch_turn_pending());
    }
}
