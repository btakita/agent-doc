//! `#steeringharnessparity` SimWorld: reactive realtime steering delivered to
//! every supported harness, busy and idle.
//!
//! Each scenario drives the PRODUCTION pieces against a real temp project:
//! the shared steering derivation (`agent_doc_session_check_io::midturn_steering`),
//! the supervisor's Computed wake subject (`agent_doc_supervisor::steering_wake`),
//! the guarded idle-drain decision (`agent_doc_queue::queue`), each harness's
//! composer-readiness projection (`agent_doc_harness::project_pane_composer`),
//! and the per-harness last-mile table (`agent_doc_harness::steering_delivery`).
//! Only the agent and the operator are simulated.

use std::path::{Path, PathBuf};

use agent_doc_harness::steering_delivery::{
    BusyDelivery, IdleDelivery, SUPPORTED_HARNESSES, steering_delivery_adapter,
};
use agent_doc_harness::{HarnessConfig, PaneComposerProjection, project_pane_composer};
use agent_doc_queue::queue::{
    IdleQueueDrainDecision, IdleQueueDrainDecisionFacts,
    idle_queue_drain_decision_with_current_transition,
};
use agent_doc_session_check_io::midturn_steering::{
    self as steering, CONSUMER_CLI, CONSUMER_HOOK, SteeringReport,
};
use agent_doc_supervisor::steering_wake::{
    SteeringWakeEvent, SteeringWakeSet, SteeringWakeState, idle_drain_subject,
};

const BASELINE: &str = "---\nagent_doc_steering_debounce_ms: 2500\nprompt_presets:\n  '#subagents': 'run the remaining items in subagents'\n---\n# S\n\n<!-- agent:queue go -->\n- current task\n<!-- /agent:queue -->\n";

/// An idle, dispatch-ready composer for each supervised harness, and a pane
/// mid-turn. Captured shapes from the harness crate's own fixtures.
fn idle_pane(harness: &str) -> &'static str {
    match harness {
        "codex" => {
            "\u{1b}[1m›\u{1b}[0m \u{1b}[2mAsk Codex to do anything\u{1b}[0m\n\n  GPT-5 high · ~/work · Context 0% used\n"
        }
        "opencode" => concat!(
            "\n\n",
            "                                               ┃\n",
            "                                               ┃  Ask anything... \"Fix a TODO in the codebase\"\n",
            "                                               ┃\n",
            "                                               ┃  Build · GLM-5.2 Z.AI Coding Plan\n",
            "                                               ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n",
            "                                               tab agents  ctrl+p commands\n",
        ),
        "grok" => {
            "Grok Build 1.0.24\n╭────────────────────────╮\n│ ❯                      │\n╰──── Grok 4.6 (high) ───╯\nShift+Tab:mode  │  Ctrl+x:shortcuts\n"
        }
        _ => "prior output\n\n❯\n",
    }
}

fn busy_pane(harness: &str) -> &'static str {
    match harness {
        "grok" => "Grok Build 1.0.24\nthinking…  ctrl+c:cancel\n",
        "opencode" => "Working (21s - esc to interrupt)\n",
        _ => "Working… (esc to interrupt)\n",
    }
}

fn composer_ready(harness: &str, pane: &str) -> bool {
    matches!(
        project_pane_composer(pane, &HarnessConfig::from_agent_name(harness)),
        PaneComposerProjection::ReadyEmpty { .. }
    )
}

fn backdate(file: &Path) {
    std::fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
}

fn verbatims(report: Option<SteeringReport>) -> Vec<String> {
    report
        .map(|report| report.items.into_iter().map(|item| item.verbatim).collect())
        .unwrap_or_default()
}

struct Session {
    _dir: tempfile::TempDir,
    file: PathBuf,
}

impl Session {
    fn open() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, BASELINE).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(BASELINE), Some(BASELINE))
                .unwrap();
        steering::seed_for_cycle(
            &file,
            &cycle.cycle_id,
            BASELINE,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        Self { _dir: dir, file }
    }

    fn operator_appends(&self, line: &str) {
        let content = std::fs::read_to_string(&self.file).unwrap();
        let updated = content.replace(
            "<!-- /agent:queue -->",
            &format!("- {line}\n<!-- /agent:queue -->"),
        );
        std::fs::write(&self.file, updated).unwrap();
    }

    fn close_turn(&self) -> String {
        let content = std::fs::read_to_string(&self.file).unwrap();
        let closed = content.replace("- current task\n", "");
        std::fs::write(&self.file, &closed).unwrap();
        agent_doc_cycle_state_io::mark_committed(
            &self.file,
            "sim_commit",
            Some(&closed),
            Some(&closed),
        )
        .unwrap();
        let mut report = Vec::new();
        steering::emit_closeout_steering(&self.file, &mut report);
        String::from_utf8(report).unwrap()
    }

    /// The busy agent's channel for this harness.
    fn busy_delivery(&self, busy: BusyDelivery) -> Vec<String> {
        let consumer = match busy {
            BusyDelivery::PostToolUseHook => CONSUMER_HOOK,
            BusyDelivery::PollAndCommandBoundary => CONSUMER_CLI,
        };
        verbatims(steering::observe(&self.file, consumer, true).unwrap())
    }
}

/// One supervisor tick over an idle-or-busy pane: the Computed wake subject
/// joins the guarded drain decision. Returns whether the trigger was submitted.
fn supervisor_tick(session: &Session, wake: &SteeringWakeState, harness: &str, pane: &str) -> bool {
    let observation = steering::observe_for_wake(&session.file).unwrap();
    wake.send(SteeringWakeEvent::Observed(
        (!observation.items.is_empty()).then(|| SteeringWakeSet {
            fingerprint: observation.fingerprint.clone(),
            items: observation.items.len(),
        }),
    ));
    let subject = idle_drain_subject(None, wake.subject());
    let prompt_visible = composer_ready(harness, pane);
    let decision = idle_queue_drain_decision_with_current_transition(IdleQueueDrainDecisionFacts {
        clear_cooldown_active: false,
        prompt_visible,
        turn_active: !prompt_visible,
        self_driving_loop_active: false,
        route_submit_in_flight: false,
        current_transition_pending: false,
        active_head: subject.as_deref(),
        last_dispatched: None,
    });
    if decision != IdleQueueDrainDecision::Dispatch {
        return false;
    }
    let subject = subject.expect("dispatch implies a subject");
    let fingerprint = wake.fingerprint_for(&subject).expect("wake fingerprint");
    wake.send(SteeringWakeEvent::Delivered(fingerprint.clone()));
    steering::record_wake_receipt(&session.file, &fingerprint, observation.items.len()).unwrap();
    true
}

fn run_harness(harness: &str) {
    let adapter = steering_delivery_adapter(harness);
    let session = Session::open();
    let wake = SteeringWakeState::new();

    // 1. Busy pane, open cycle: the operator queues a subagent item.
    session.operator_appends("#subagent: https://github.com/btakita/agent-doc/issues/116");
    backdate(&session.file);
    // The supervisor never types into a busy composer.
    assert!(
        !composer_ready(harness, busy_pane(harness)),
        "{harness} busy"
    );
    assert!(
        !supervisor_tick(&session, &wake, harness, busy_pane(harness)),
        "{harness}: no wake into a busy pane"
    );
    let busy = session.busy_delivery(adapter.busy);
    assert_eq!(
        busy,
        vec!["#subagent: https://github.com/btakita/agent-doc/issues/116"],
        "{harness}: busy delivery"
    );
    assert!(
        session.busy_delivery(adapter.busy).is_empty(),
        "{harness}: busy delivery is exactly once per channel"
    );

    // 2. The turn closes. A hook harness already delivered 116, so the
    //    boundary report does not repeat it; a hookless harness gets the
    //    boundary report as its command-boundary channel.
    let report = session.close_turn();
    match adapter.busy {
        BusyDelivery::PostToolUseHook => {
            assert!(!report.contains("issues/116"), "{harness}: {report}")
        }
        BusyDelivery::PollAndCommandBoundary => {}
    }

    // 3. Idle pane: the operator queues two more items. The first is still
    //    being typed (inside the debounce): no wake.
    session.operator_appends("#subagent: https://github.com/btakita/agent-doc/issues/117");
    session.operator_appends("#subagent: https://github.com/btakita/agent-doc/issues/118");
    assert!(
        !supervisor_tick(&session, &wake, harness, idle_pane(harness)),
        "{harness}: typing is held"
    );
    backdate(&session.file);
    // A worker already claimed 118: it is in flight elsewhere.
    agent_doc_queue_io::queue_claim::claim(
        &session.file,
        "#subagent: https://github.com/btakita/agent-doc/issues/118",
        "subagent:gh-118",
        3600,
    )
    .unwrap();
    let wake_items: Vec<String> = steering::observe_for_wake(&session.file)
        .unwrap()
        .items
        .into_iter()
        .map(|item| item.verbatim)
        .collect();
    assert_eq!(
        wake_items,
        vec!["#subagent: https://github.com/btakita/agent-doc/issues/117"],
        "{harness}: the claimed item never wakes"
    );

    match adapter.idle {
        IdleDelivery::SupervisorTrigger => {
            assert!(
                composer_ready(harness, idle_pane(harness)),
                "{harness} idle"
            );
            assert!(
                supervisor_tick(&session, &wake, harness, idle_pane(harness)),
                "{harness}: the idle pane is woken"
            );
            assert!(
                !supervisor_tick(&session, &wake, harness, idle_pane(harness)),
                "{harness}: exactly one wake per steering set"
            );
            // A fresh supervisor (recycle) reloads the durable receipt.
            let recycled = SteeringWakeState::new();
            recycled.send(SteeringWakeEvent::Delivered(
                steering::load_wake_receipt(&session.file).unwrap().unwrap(),
            ));
            assert!(
                !supervisor_tick(&session, &recycled, harness, idle_pane(harness)),
                "{harness}: the receipt survives a supervisor recycle"
            );
        }
        IdleDelivery::Unavailable { gap } => {
            assert!(!gap.is_empty());
        }
    }

    // 4. The woken (or next) turn's agent channel delivers the steering the
    //    wake did not consume. The hook harnesses' hook keeps reporting after
    //    close; hookless harnesses read it at their next poll.
    let delivered = session.busy_delivery(adapter.busy);
    assert!(
        delivered
            .contains(&"#subagent: https://github.com/btakita/agent-doc/issues/117".to_string()),
        "{harness}: {delivered:?}"
    );
    assert!(
        session.busy_delivery(adapter.busy).is_empty(),
        "{harness}: and only once"
    );
}

#[test]
fn claude_code_steering_reaches_busy_and_idle_panes() {
    run_harness("claude");
}

#[test]
fn codex_steering_reaches_busy_and_idle_panes() {
    run_harness("codex");
}

#[test]
fn opencode_steering_reaches_busy_and_idle_panes() {
    run_harness("opencode");
}

#[test]
fn grok_build_steering_reaches_busy_and_idle_panes() {
    run_harness("grok");
}

#[test]
fn cursor_steering_reaches_the_agent_at_its_next_command() {
    run_harness("cursor");
}

/// `#claimedsteerwake` / `#claimfollowsedit` (#steerworks): the operator
/// annotates a head a subagent already claimed while the coordinator is idle,
/// then presses Run Agent Doc mid-typing of another line. Per harness: the
/// idle pane is woken for the claimed edit, the agent channel labels it
/// `forward_to_owner` (never a new subagent dispatch), and the explicit send
/// delivers the half-typed line at once, final.
fn run_claimed_edit_and_explicit_send(harness: &str) {
    let adapter = steering_delivery_adapter(harness);
    let session = Session::open();
    let wake = SteeringWakeState::new();
    let head = "#subagent: https://github.com/btakita/agent-doc/issues/126";
    session.operator_appends(head);
    backdate(&session.file);
    assert_eq!(session.busy_delivery(adapter.busy), vec![head.to_string()]);
    agent_doc_queue_io::queue_claim::claim(&session.file, head, "subagent:ghfix126", 3600).unwrap();
    session.close_turn();

    // Idle: annotate the claimed head.
    let content = std::fs::read_to_string(&session.file).unwrap();
    let annotated = format!("{head}: note this is in a Coder environment");
    std::fs::write(&session.file, content.replace(head, &annotated)).unwrap();
    backdate(&session.file);
    if let IdleDelivery::SupervisorTrigger = adapter.idle {
        assert!(
            supervisor_tick(&session, &wake, harness, idle_pane(harness)),
            "{harness}: an edit of a claimed head wakes the idle coordinator"
        );
    }
    let consumer = match adapter.busy {
        BusyDelivery::PostToolUseHook => CONSUMER_HOOK,
        BusyDelivery::PollAndCommandBoundary => CONSUMER_CLI,
    };
    let report = steering::observe(&session.file, consumer, true)
        .unwrap()
        .expect("delivery");
    let text = report.render().unwrap();
    assert!(
        text.contains("dispatch=forward_to_owner") && text.contains("owner=subagent:ghfix126"),
        "{harness}: {text}"
    );
    assert!(!text.contains("dispatch=subagent"), "{harness}: {text}");

    // Run Agent Doc while the operator's next line is still unfinished.
    session.operator_appends("also publish the");
    assert!(
        session.busy_delivery(adapter.busy).is_empty(),
        "{harness}: held"
    );
    steering::record_explicit_send(&session.file).unwrap();
    let sent = steering::observe(&session.file, consumer, true)
        .unwrap()
        .expect("explicit send");
    assert_eq!(sent.items.len(), 1, "{harness}: {sent:?}");
    assert!(sent.items[0].explicit, "{harness}");
    assert!(
        sent.render().unwrap().contains("sent=explicit"),
        "{harness}"
    );
}

#[test]
fn claimed_edit_and_explicit_send_reach_every_harness() {
    for harness in ["claude", "codex", "opencode", "grok", "cursor"] {
        run_claimed_edit_and_explicit_send(harness);
    }
}

/// Parity: the scenarios above cover every supported harness.
#[test]
fn every_supported_harness_has_a_steering_scenario() {
    let covered = ["claude", "codex", "opencode", "grok", "cursor"];
    for harness in SUPPORTED_HARNESSES {
        assert!(
            covered.contains(&harness),
            "{harness} has no SimWorld scenario"
        );
    }
}
