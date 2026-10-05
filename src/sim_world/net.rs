//! `#netadv4` — route SimWorld's cross-process messages through the seeded
//! adversarial channel in `agent_doc_sim_net`.
//!
//! The `local` profile bypasses the channel entirely, so every existing scenario
//! stays byte-identical. Any other profile classifies each [`SimCommand`] as either
//! a local step (document edits, git commit, process control the controller owns)
//! or a message on a directed process hop ([`SimLink`]). Messages that carry the
//! sender's view of the actor generation stamp it at SEND time, so a delayed or
//! duplicated message reaches the receiver genuinely stale — the same way a
//! production `LifecycleRequest { generation, .. }` does.
//!
//! Two delivery modes:
//!
//! * [`NetMode::Rpc`] — the caller waits for its message (first copy) before the
//!   step returns, so scripted scenarios keep their step-by-step assertions.
//!   Retransmits and duplicates stay in flight as stragglers and land on later
//!   steps: the at-least-once adversary.
//! * [`NetMode::Async`] — the seed corpus: messages stay in flight across
//!   scheduler ticks, so independent requests interleave, overtake each other and
//!   arrive after later local steps.

use super::*;
use agent_doc_sim_net::{Delivery, NetEvent, NetProfile, NetStats, SimNet};
use std::collections::BTreeMap;

/// Virtual time one scheduler step represents.
pub(crate) const SIM_TICK_MS: u64 = 100;

pub(crate) const NET_PROFILE_ENV: &str = "AGENT_DOC_SIM_NET_PROFILE";
pub(crate) const NET_SEED_ENV: &str = "AGENT_DOC_SIM_NET_SEED";

/// A directed process hop as SimWorld represents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SimLink {
    /// Supervisor lifecycle, heartbeat and dispatch-proof reports.
    SupervisorToController,
    /// `agent-doc admin ...` control requests.
    CliToController,
    /// Editor-originated route dispatch and layout sync requests.
    EditorToController,
    /// tmux pane observations the controller acts on.
    TmuxToController,
    /// Controller-originated editor signals (post-commit reposition).
    ControllerToEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetMode {
    Rpc,
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NetMsg {
    pub(crate) command: SimCommand,
    /// The sender's view of the actor generation when it sent the message.
    pub(crate) generation_at_send: u64,
}

/// Families of level-state updates whose receivers apply "last arrival wins".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LevelFamily {
    ActorLifecycle,
    QueueControl,
}

/// One oracle violation observed while the network was adversarial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetFinding {
    pub(crate) kind: &'static str,
    pub(crate) detail: String,
}

#[derive(Debug)]
pub(crate) struct SimWorldNet {
    pub(crate) profile: NetProfile,
    pub(crate) mode: NetMode,
    pub(crate) net: SimNet<SimLink, NetMsg>,
    /// Generation override in force while a delivered message is applied.
    pub(crate) delivering_generation: Option<u64>,
    /// Latest applied send id per level-state family and generation.
    pub(crate) latest_applied: BTreeMap<LevelFamily, (u64, u64)>,
    pub(crate) findings: Vec<NetFinding>,
    pub(crate) reconnect_resyncs: u64,
    /// Send id of an out-of-order actor-lifecycle update whose effect is still the
    /// controller's durable lifecycle (cleared by any later lifecycle change).
    pub(crate) stale_lifecycle_in_effect: Option<u64>,
    /// Dispatch request ids the controller accepted (injected a trigger for).
    pub(crate) accepted_dispatch_ids: std::collections::BTreeSet<u64>,
}

impl SimWorldNet {
    pub(crate) fn new(profile: NetProfile, seed: u64, mode: NetMode) -> Self {
        Self {
            profile,
            mode,
            net: SimNet::for_profile(profile, seed, Delivery::AtLeastOnce),
            delivering_generation: None,
            latest_applied: BTreeMap::new(),
            findings: Vec::new(),
            reconnect_resyncs: 0,
            stale_lifecycle_in_effect: None,
            accepted_dispatch_ids: std::collections::BTreeSet::new(),
        }
    }

    pub(crate) fn stats(&self) -> NetStats {
        self.net.stats()
    }
}

/// Profile/seed requested through the environment (`make sim-net`, ad-hoc runs).
/// Unset or `local` means the perfect channel.
pub(crate) fn env_net_profile() -> Option<(NetProfile, u64)> {
    let name = std::env::var(NET_PROFILE_ENV).ok()?;
    let profile = NetProfile::parse(&name)
        .unwrap_or_else(|| panic!("{NET_PROFILE_ENV}={name:?} is not a known net profile"));
    if profile == NetProfile::Local {
        return None;
    }
    let seed = std::env::var(NET_SEED_ENV)
        .ok()
        .map(|raw| {
            raw.trim()
                .parse::<u64>()
                .unwrap_or_else(|_| panic!("{NET_SEED_ENV}={raw:?} is not a u64"))
        })
        .unwrap_or(0);
    Some((profile, seed))
}

/// Which hop, if any, a command crosses. `None` = a local step.
///
/// Kept local on purpose: document edits and closeout (the CRDT relay has its own
/// convergence suites), route-owner registration and session restarts (they mint
/// the generation everything else is fenced by), fault injection, and the
/// scripted model toggles.
pub(crate) fn link_for(command: SimCommand) -> Option<SimLink> {
    use SimCommand::*;
    Some(match command {
        SupervisorReady
        | SupervisorBusy
        | SupervisorWaitingInput
        | SupervisorBlocked
        | SupervisorClosed
        | StaleSupervisorUpdate
        | SupervisorHeartbeatReattach
        | SupervisorHeartbeatStale
        | ProveDispatchAccepted
        | PromoteStartingPromptReady => SimLink::SupervisorToController,
        AdminPauseQueue | AdminPauseQueueStale | AdminResumeQueue | AdminDrainQueue
        | AdminHandoff | AdminHandoffStale | AdminReap | AdminReapStale => SimLink::CliToController,
        DispatchRoutePrompt
        | DispatchOperatorPrompt
        | SyncProtectedGrowthManual
        | SyncProtectedGrowthPassive
        | SyncProtectedGrowthFocusVisible
        | SyncDetachableReplaceManual
        | SyncDetachableReplacePassive
        | SyncVisibleFocusPreserve
        | SyncRerequestVisibleEditorManual
        | SyncRerequestVisibleEditorPassive => SimLink::EditorToController,
        ObserveStalePane | ObserveMissingPane => SimLink::TmuxToController,
        PostCommitIpcRepositionSignal => SimLink::ControllerToEditor,
        _ => return None,
    })
}

fn level_family(command: SimCommand) -> Option<LevelFamily> {
    use SimCommand::*;
    match command {
        SupervisorReady
        | SupervisorBusy
        | SupervisorWaitingInput
        | SupervisorBlocked
        | SupervisorClosed
        | SupervisorHeartbeatReattach => Some(LevelFamily::ActorLifecycle),
        AdminPauseQueue | AdminResumeQueue | AdminDrainQueue => Some(LevelFamily::QueueControl),
        _ => None,
    }
}

impl SimWorld {
    /// Attach a network profile. `local` detaches (perfect channel, no RNG).
    pub(crate) fn with_net(mut self, profile: NetProfile, seed: u64, mode: NetMode) -> Self {
        self.net = (profile != NetProfile::Local).then(|| SimWorldNet::new(profile, seed, mode));
        self
    }

    /// The generation a command acts on: the sender's stamped view while a
    /// delivered message is applied, otherwise the current durable generation.
    pub(crate) fn observed_generation(&self) -> u64 {
        self.net
            .as_ref()
            .and_then(|net| net.delivering_generation)
            .unwrap_or(self.route.durable.generation)
    }

    /// Advance virtual time by one scheduler tick, applying every message due.
    pub(crate) fn net_tick(&mut self) -> Result<()> {
        let Some(net) = self.net.as_mut() else {
            return Ok(());
        };
        let events = net.net.advance(SIM_TICK_MS);
        self.apply_net_events(events)
    }

    /// Fairness: deliver everything still in flight.
    pub(crate) fn net_drain(&mut self) -> Result<()> {
        let Some(net) = self.net.as_mut() else {
            return Ok(());
        };
        let events = net.net.drain();
        self.apply_net_events(events)
    }

    /// Send `command` over its hop. In RPC mode, wait for its first copy.
    pub(crate) fn send_over_net(&mut self, link: SimLink, command: SimCommand) -> Result<()> {
        let generation_at_send = self.route.durable.generation;
        let net = self.net.as_mut().expect("send_over_net requires a net");
        let id = net.net.send(
            link,
            NetMsg {
                command,
                generation_at_send,
            },
        );
        if net.mode == NetMode::Rpc {
            let events = net.net.advance_until_delivered(id);
            self.apply_net_events(events)?;
        }
        Ok(())
    }

    fn apply_net_events(&mut self, events: Vec<NetEvent<SimLink, NetMsg>>) -> Result<()> {
        for event in events {
            match event {
                NetEvent::Deliver { id, msg, copy, .. } => {
                    self.apply_delivered(id, copy, msg)?;
                    self.assert_structural_invariants()?;
                }
                NetEvent::Reconnect { .. } => {
                    // The controller's durable actor record is the state both sides
                    // reconcile from; the model holds no per-connection state, so a
                    // reconnect has nothing to rebuild. Count it so a run proves the
                    // stall/reconnect path was exercised.
                    if let Some(net) = self.net.as_mut() {
                        net.reconnect_resyncs += 1;
                    }
                }
            }
        }
        Ok(())
    }

    fn actor_fact(&self) -> (u64, SupervisorLifecycle) {
        (self.route.durable.generation, self.route.durable.lifecycle)
    }

    /// A local step changed the actor lifecycle: any stale effect is overwritten.
    pub(crate) fn net_note_local_step(&mut self, before: (u64, SupervisorLifecycle)) {
        if self.actor_fact() != before
            && let Some(net) = self.net.as_mut()
        {
            net.stale_lifecycle_in_effect = None;
        }
    }

    fn apply_delivered(&mut self, id: u64, copy: u32, msg: NetMsg) -> Result<()> {
        let before = self.actor_fact();
        let acceptances_before = self.coverage.route_dispatch_acceptances;
        let findings_before = self.net.as_ref().map_or(0, |net| net.findings.len());
        let accepted_generation = msg.generation_at_send == self.route.durable.generation;
        if let Some(family) = level_family(msg.command)
            && accepted_generation
        {
            let generation = msg.generation_at_send;
            let net = self.net.as_mut().expect("delivery requires a net");
            match net.latest_applied.get(&family).copied() {
                Some((latest_generation, latest_id))
                    if latest_generation == generation && latest_id > id =>
                {
                    let detail = format!(
                        "{family:?}: {:?} (send #{id}, copy {copy}, generation {generation}) applied after newer send #{latest_id}; net seed trace tail:\n{}",
                        msg.command,
                        net.net.trace_tail(12)
                    );
                    net.findings.push(NetFinding {
                        kind: match family {
                            LevelFamily::ActorLifecycle => {
                                "stale_actor_lifecycle_applied_out_of_order"
                            }
                            LevelFamily::QueueControl => "stale_queue_control_applied_out_of_order",
                        },
                        detail,
                    });
                }
                _ => {
                    net.latest_applied.insert(family, (generation, id));
                }
            }
        }
        if let Some(net) = self.net.as_mut() {
            net.delivering_generation = Some(msg.generation_at_send);
        }
        let result = self.apply_local(msg.command);
        let after = self.actor_fact();
        let dispatch_accepted = self.coverage.route_dispatch_acceptances > acceptances_before;
        let trace = self.trace.clone();
        let seed = self.seed;
        if let Some(net) = self.net.as_mut() {
            net.delivering_generation = None;
            let stale_lifecycle = level_family(msg.command) == Some(LevelFamily::ActorLifecycle)
                && net.findings.len() > findings_before;
            if after != before {
                net.stale_lifecycle_in_effect = stale_lifecycle.then_some(id);
            }
            if dispatch_accepted && !net.accepted_dispatch_ids.insert(id) {
                net.findings.push(NetFinding {
                    kind: "duplicate_dispatch_request_injected_twice",
                    detail: format!(
                        "{:?} send #{id} copy {copy} accepted again (a second trigger injected for one request); schedule seed={seed} trace={trace:?}\nnet trace tail:\n{}",
                        msg.command,
                        net.net.trace_tail(16)
                    ),
                });
            }
            if dispatch_accepted && let Some(stale_id) = net.stale_lifecycle_in_effect {
                net.findings.push(NetFinding {
                    kind: "dispatch_accepted_on_reordered_stale_lifecycle",
                    detail: format!(
                        "{:?} accepted while the durable lifecycle {:?} came from out-of-order send #{stale_id}; schedule seed={seed} trace={trace:?}\nnet trace tail:\n{}",
                        msg.command,
                        after.1,
                        net.net.trace_tail(16)
                    ),
                });
            }
        }
        result
    }
}

/// Aggregate of one corpus run under a profile.
#[derive(Debug, Default)]
pub(crate) struct NetCorpusRun {
    pub(crate) coverage: Coverage,
    pub(crate) stats: NetStats,
    pub(crate) reconnect_resyncs: u64,
    /// `(schedule seed, net seed, finding)`.
    pub(crate) findings: Vec<(u64, u64, NetFinding)>,
    /// `(schedule seed, net seed, structural failure)`.
    pub(crate) failures: Vec<(u64, u64, String)>,
    pub(crate) schedules: usize,
}

impl NetCorpusRun {
    pub(crate) fn absorb_stats(&mut self, stats: NetStats) {
        let total = &mut self.stats;
        total.sent += stats.sent;
        total.delivered += stats.delivered;
        total.lost += stats.lost;
        total.drops += stats.drops;
        total.duplicates += stats.duplicates;
        total.reorder_holds += stats.reorder_holds;
        total.retransmits += stats.retransmits;
        total.stalls += stats.stalls;
        total.reconnects += stats.reconnects;
        total.max_latency_ms = total.max_latency_ms.max(stats.max_latency_ms);
    }

    pub(crate) fn summary(&self, label: &str) -> String {
        let s = self.stats;
        format!(
            "{label}: schedules={} failures={} findings={} sent={} delivered={} drops={} duplicates={} reorder_holds={} retransmits={} stalls={} reconnects={} reconnects_handled={} max_latency_ms={} commits={}",
            self.schedules,
            self.failures.len(),
            self.findings.len(),
            s.sent,
            s.delivered,
            s.drops,
            s.duplicates,
            s.reorder_holds,
            s.retransmits,
            s.stalls,
            s.reconnects,
            self.reconnect_resyncs,
            s.max_latency_ms,
            self.coverage.commits,
        )
    }
}

/// The channel seed one corpus schedule runs with: distinct per schedule seed,
/// so every schedule sees a different fault pattern.
pub(crate) fn mixed_net_seed(schedule_seed: u64, net_seed: u64) -> u64 {
    net_seed ^ schedule_seed.rotate_left(17)
}

/// Run the corpus schedules `seeds` under `profile`, once per net seed, collecting
/// structural failures and oracle findings instead of stopping at the first.
pub(crate) fn run_net_corpus(
    profile: NetProfile,
    seeds: std::ops::Range<u64>,
    net_seeds: &[u64],
    steps: usize,
) -> NetCorpusRun {
    let mut run = NetCorpusRun::default();
    for &net_seed in net_seeds {
        for seed in seeds.clone() {
            run.schedules += 1;
            let mixed = mixed_net_seed(seed, net_seed);
            match SimWorld::run_seed_with_net(seed, steps, profile, mixed) {
                Ok(mut world) => {
                    run.coverage.merge(world.coverage);
                    if let Some(mut net) = world.net.take() {
                        run.absorb_stats(net.stats());
                        run.reconnect_resyncs += net.reconnect_resyncs;
                        for finding in std::mem::take(&mut net.findings) {
                            run.findings.push((seed, net_seed, finding));
                        }
                    }
                }
                Err(err) => run.failures.push((seed, net_seed, err.to_string())),
            }
        }
    }
    run
}

impl Drop for SimWorldNet {
    /// A scenario that fails under an adversarial profile prints the network
    /// schedule that led there, so the seed comes with a minimal trace.
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "netadv4: profile={} stats={:?}\nnet trace (tail):\n{}",
                self.profile,
                self.net.stats(),
                self.net.trace_tail(40)
            );
        }
    }
}

/// Oracle finding classes already diagnosed as open protocol defects (see the
/// `netadv4_known_*` tests below). A corpus run may report these; any OTHER class
/// is a new finding and fails the run.
pub(crate) const KNOWN_OPEN_NET_FINDINGS: [&str; 4] = [
    // F1: `LifecycleRequest`/`SupervisorHeartbeatRequest` are fenced by generation
    // only; within one generation the controller applies them in arrival order.
    "stale_actor_lifecycle_applied_out_of_order",
    // F1 impact: a reordered Ready re-opens dispatch into a busy supervisor.
    "dispatch_accepted_on_reordered_stale_lifecycle",
    // F2: queue pause/resume/drain requests carry no sequence either.
    "stale_queue_control_applied_out_of_order",
    // F3: a retransmitted dispatch request has no idempotency key; after the first
    // copy is proven, a straggler copy injects a second trigger.
    "duplicate_dispatch_request_injected_twice",
];

/// Net seeds `make sim-net` runs each corpus schedule under.
pub(crate) const NET_CORPUS_SEEDS: [u64; 4] = [0, 1, 2, 3];

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_net_corpus(profile: NetProfile) {
        let started = Instant::now();
        let run = run_net_corpus(
            profile,
            FAST_CORPUS_SEEDS,
            &NET_CORPUS_SEEDS,
            FAST_CORPUS_STEPS,
        );
        eprintln!(
            "{} elapsed_ms={}",
            run.summary(profile.name()),
            started.elapsed().as_millis()
        );
        let mut kinds: BTreeMap<&str, (usize, u64, u64)> = BTreeMap::new();
        for (seed, net_seed, finding) in &run.findings {
            kinds.entry(finding.kind).or_insert((0, *seed, *net_seed)).0 += 1;
        }
        for (kind, (count, seed, net_seed)) in &kinds {
            eprintln!(
                "  finding {kind}: count={count} first schedule_seed={seed} net_seed={net_seed}"
            );
        }
        assert!(
            run.failures.is_empty(),
            "{profile}: structural invariant failures (schedule_seed, net_seed, error): {:#?}",
            run.failures
        );
        let new: Vec<_> = run
            .findings
            .iter()
            .filter(|(_, _, finding)| !KNOWN_OPEN_NET_FINDINGS.contains(&finding.kind))
            .collect();
        assert!(
            new.is_empty(),
            "{profile}: NEW network finding classes: {new:#?}"
        );
        // The same coverage floor the `local` corpus must reach.
        assert_fast_corpus_coverage(&run.coverage);
        // Non-vacuity: the channel really was adversarial.
        let stats = run.stats;
        assert!(stats.sent > 0 && stats.delivered >= stats.sent, "{stats:?}");
        assert_eq!(
            stats.lost, 0,
            "at-least-once delivery never loses: {stats:?}"
        );
        assert!(
            stats.drops > 0 && stats.duplicates > 0 && stats.reorder_holds > 0,
            "{stats:?}"
        );
        assert!(
            stats.stalls > 0 && stats.reconnects == stats.stalls,
            "{stats:?}"
        );
        assert!(stats.max_latency_ms >= 1_000, "{stats:?}");
    }

    fn mask_boundary_ids(text: &str) -> String {
        const MARKER: &str = "agent:boundary:";
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(MARKER) {
            out.push_str(&rest[..at + MARKER.len()]);
            rest = &rest[at + MARKER.len()..];
            let id_len = rest
                .bytes()
                .take_while(|byte| byte.is_ascii_hexdigit())
                .count();
            out.push_str("<id>");
            rest = &rest[id_len..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn local_profile_keeps_corpus_schedules_byte_identical() {
        for seed in 0..128 {
            let plain = SimWorld::run_seed_world(SimWorld::new_local(seed), FAST_CORPUS_STEPS)
                .unwrap_or_else(|err| panic!("seed {seed}: {err}"));
            let routed =
                SimWorld::run_seed_with_net(seed, FAST_CORPUS_STEPS, NetProfile::Local, 99)
                    .unwrap_or_else(|err| panic!("seed {seed}: {err}"));
            assert!(routed.net.is_none(), "local must not attach a channel");
            // Boundary markers carry fresh random ids from the production template
            // on every run; everything else must match byte for byte.
            let (plain, routed) = (
                mask_boundary_ids(&format!("{plain:?}")),
                mask_boundary_ids(&format!("{routed:?}")),
            );
            if plain != routed {
                let at = plain
                    .bytes()
                    .zip(routed.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(plain.len().min(routed.len()));
                let window =
                    |s: &str| s[at.saturating_sub(160)..(at + 80).min(s.len())].to_string();
                panic!(
                    "seed {seed}: local routing diverged at byte {at}:\n  plain:  {}\n  routed: {}",
                    window(&plain),
                    window(&routed)
                );
            }
        }
    }

    #[test]
    fn a_net_seed_reproduces_a_corpus_schedule_exactly() {
        for profile in NetProfile::ADVERSARIAL {
            for seed in [0, 8, 204] {
                let net_seed = mixed_net_seed(seed, 1);
                let first = SimWorld::run_seed_with_net(seed, FAST_CORPUS_STEPS, profile, net_seed)
                    .unwrap();
                let second =
                    SimWorld::run_seed_with_net(seed, FAST_CORPUS_STEPS, profile, net_seed)
                        .unwrap();
                assert_eq!(
                    mask_boundary_ids(&format!("{first:?}")),
                    mask_boundary_ids(&format!("{second:?}")),
                    "{profile} {seed}"
                );
            }
        }
    }

    #[test]
    #[ignore = "run by `make sim-net` (part of `make check`)"]
    fn closeout_sim_net_coder_zscaler_seed_corpus() {
        assert_net_corpus(NetProfile::CoderZscaler);
    }

    #[test]
    #[ignore = "run by `make sim-net` (part of `make check`)"]
    fn closeout_sim_net_hostile_seed_corpus() {
        assert_net_corpus(NetProfile::Hostile);
    }

    fn findings_of(seed: u64, profile: NetProfile, net_seed: u64) -> Vec<NetFinding> {
        let mut world = SimWorld::run_seed_with_net(
            seed,
            FAST_CORPUS_STEPS,
            profile,
            mixed_net_seed(seed, net_seed),
        )
        .unwrap();
        std::mem::take(&mut world.net.as_mut().unwrap().findings)
    }

    /// `#netadv4` F1, minimal trace. The supervisor reports Ready, then Busy, in
    /// one generation. The channel delivers Busy first; the late Ready is still
    /// current-generation, so the controller accepts it and a route dispatch
    /// types into a busy supervisor. Production `LifecycleRequest` carries the
    /// same generation-only fence. KNOWN OPEN DEFECT: when lifecycle updates gain
    /// a per-generation sequence, flip these assertions.
    #[test]
    fn netadv4_known_f1_reordered_ready_reopens_dispatch_into_busy_supervisor() {
        let mut world = SimWorld::new_local(4_001);
        world.apply(SimCommand::BindRouteOwner).unwrap();
        world.apply(SimCommand::SupervisorReady).unwrap();
        // Sent: Ready (t0, delayed), Busy (t1). Delivered: Busy, then Ready.
        world.apply(SimCommand::SupervisorBusy).unwrap();
        world.apply(SimCommand::SupervisorReady).unwrap();
        world.apply(SimCommand::DispatchRoutePrompt).unwrap();
        assert_eq!(
            world.coverage.route_dispatch_acceptances, 1,
            "known defect: the reordered Ready re-opens dispatch while the supervisor is busy"
        );

        // The corpus oracle finds the same class at pinned seeds.
        let found = findings_of(204, NetProfile::CoderZscaler, 0);
        assert!(
            found
                .iter()
                .any(|f| f.kind == "dispatch_accepted_on_reordered_stale_lifecycle"),
            "{found:#?}"
        );
        assert!(
            findings_of(22, NetProfile::CoderZscaler, 0)
                .iter()
                .any(|f| f.kind == "stale_actor_lifecycle_applied_out_of_order")
        );
    }

    /// `#netadv4` F3, minimal trace. One dispatch request, retransmitted because
    /// its ACK was lost. The first copy is accepted and proven; the straggler copy
    /// then passes the in-flight coalesce (nothing is in flight any more) and
    /// injects a second trigger. Production `ControllerRequest` has no request id.
    /// KNOWN OPEN DEFECT: flip when dispatch requests become idempotent by key.
    #[test]
    fn netadv4_known_f3_retransmitted_dispatch_after_proof_injects_twice() {
        let mut world = SimWorld::new_local(4_003);
        world.apply(SimCommand::BindRouteOwner).unwrap();
        world.apply(SimCommand::SupervisorReady).unwrap();
        world.apply(SimCommand::DispatchRoutePrompt).unwrap(); // copy 0
        world.apply(SimCommand::ProveDispatchAccepted).unwrap();
        world.apply(SimCommand::DispatchRoutePrompt).unwrap(); // copy 1 (retransmit)
        assert_eq!(
            world.coverage.route_dispatch_acceptances, 2,
            "known defect: one operator request, two injected triggers"
        );
        assert!(
            findings_of(8, NetProfile::Hostile, 1)
                .iter()
                .any(|f| f.kind == "duplicate_dispatch_request_injected_twice")
        );
    }

    /// `#netadv4` F2: queue control has the same last-arrival-wins shape.
    #[test]
    fn netadv4_known_f2_reordered_queue_control_is_detected() {
        assert!(
            findings_of(28, NetProfile::CoderZscaler, 0)
                .iter()
                .any(|f| f.kind == "stale_queue_control_applied_out_of_order")
        );
    }

    /// Generation fencing DOES make cross-generation stragglers safe: a delayed
    /// lifecycle update from before a handoff is rejected, not applied.
    #[test]
    fn delayed_lifecycle_from_a_prior_generation_is_rejected() {
        let mut world = SimWorld::new_local(4_004).with_net(NetProfile::Hostile, 0, NetMode::Async);
        world.apply_local(SimCommand::BindRouteOwner).unwrap();
        world.apply_local(SimCommand::SupervisorReady).unwrap();
        let stale = world.route.durable.generation;
        world.apply_local(SimCommand::AdminHandoff).unwrap();
        let blocks = world.coverage.stale_generation_blocks;
        world
            .apply_delivered(
                0,
                1,
                NetMsg {
                    command: SimCommand::SupervisorBusy,
                    generation_at_send: stale,
                },
            )
            .unwrap();
        assert_eq!(world.route.durable.lifecycle, SupervisorLifecycle::Ready);
        assert_eq!(world.coverage.stale_generation_blocks, blocks + 1);
        world.net.as_mut().unwrap().findings.clear();
    }
}
