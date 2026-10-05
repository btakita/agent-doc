//! `#netadv4` — seeded, deterministic network-conditions layer for the agent-doc
//! SimWorld simulators.
//!
//! The SimWorld harnesses model cross-process hops (editor ↔ controller ↔
//! supervisor/tmux, CLI ↔ controller) as atomic steps over a perfect channel. This
//! crate gives them an adversarial channel instead: latency drawn from a
//! distribution, jitter with occasional spikes, forced reordering, message loss,
//! duplication, and half-open stalls that end in a reconnect. Every fault is drawn
//! from a seeded RNG that is private to the network layer, so
//!
//! * the same `(profile, seed)` reproduces the exact same delivery schedule, and
//! * a simulator can keep its own schedule RNG untouched, so switching the profile
//!   changes only what the network does, never which commands are scheduled.
//!
//! Delivery semantics follow the plan's "Message loss" section
//! (`tasks/agent-doc/plan-network-adversarial-correctness.md`):
//!
//! * [`Delivery::AtLeastOnce`] — the sender retransmits until acknowledged. A lost
//!   request arrives late (after the retransmit timeout); a lost ACK means the
//!   receiver sees the message twice. Receivers must therefore be idempotent.
//! * [`Delivery::FireAndForget`] — a lost message never arrives. This is the
//!   "Wedge" variant: a protocol that needs a message to make progress must stall.
//!
//! Each send is modelled as its own connection, matching agent-doc's
//! connect-per-request IPC: independent requests can overtake each other.

use std::collections::BTreeMap;
use std::fmt;

/// Private seeded RNG (splitmix64). Deliberately not shared with the simulator's
/// schedule RNG.
#[derive(Clone, Debug)]
pub struct NetRng(u64);

impl NetRng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0xA076_1D64_78BD_642F)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform draw from `lo..=hi` (returns `lo` when the range is empty).
    pub fn range_inclusive(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            lo
        } else {
            lo + self.next_u64() % (hi - lo + 1)
        }
    }

    /// Bernoulli draw with probability `permille / 1000`. A zero probability
    /// consumes no randomness.
    pub fn chance(&mut self, permille: u16) -> bool {
        permille > 0 && (self.next_u64() % 1000) < u64::from(permille)
    }
}

/// Latency distribution: a uniform base, uniform jitter on top, and an occasional
/// spike (the long tail a proxy like Zscaler adds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyModel {
    pub min_ms: u64,
    pub max_ms: u64,
    pub jitter_ms: u64,
    pub spike_permille: u16,
    pub spike_max_ms: u64,
}

impl LatencyModel {
    pub const ZERO: Self = Self {
        min_ms: 0,
        max_ms: 0,
        jitter_ms: 0,
        spike_permille: 0,
        spike_max_ms: 0,
    };

    pub fn sample(&self, rng: &mut NetRng) -> u64 {
        let base = rng.range_inclusive(self.min_ms, self.max_ms);
        let jitter = rng.range_inclusive(0, self.jitter_ms);
        let spike = if rng.chance(self.spike_permille) {
            rng.range_inclusive(0, self.spike_max_ms)
        } else {
            0
        };
        base + jitter + spike
    }

    /// Largest latency this model can produce.
    pub fn upper_bound_ms(&self) -> u64 {
        self.max_ms.max(self.min_ms) + self.jitter_ms + self.spike_max_ms
    }
}

/// Network conditions. Probabilities are per mille (per 1000 sends).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetConditions {
    pub latency: LatencyModel,
    /// Probability that a message is held back by `reorder_hold_ms`, letting later
    /// sends overtake it.
    pub reorder_permille: u16,
    pub reorder_hold_ms: u64,
    pub drop_permille: u16,
    pub duplicate_permille: u16,
    /// Probability, per send on a healthy link, that the link enters a silent
    /// half-open stall (no reset) lasting `stall_min_ms..=stall_max_ms`. The stall
    /// ends in a reconnect.
    pub stall_permille: u16,
    pub stall_min_ms: u64,
    pub stall_max_ms: u64,
    /// Sender retransmit timeout for [`Delivery::AtLeastOnce`].
    pub retransmit_timeout_ms: u64,
}

impl NetConditions {
    /// Perfect channel: zero latency, no faults. The SimWorld default.
    pub const LOCAL: Self = Self {
        latency: LatencyModel::ZERO,
        reorder_permille: 0,
        reorder_hold_ms: 0,
        drop_permille: 0,
        duplicate_permille: 0,
        stall_permille: 0,
        stall_min_ms: 0,
        stall_max_ms: 0,
        retransmit_timeout_ms: 0,
    };

    /// Coder remote workspace with Zscaler in the path: tens to hundreds of ms of
    /// variable latency, jitter with spikes, rare silent stalls, small loss.
    pub const CODER_ZSCALER: Self = Self {
        latency: LatencyModel {
            min_ms: 20,
            max_ms: 180,
            jitter_ms: 120,
            spike_permille: 50,
            spike_max_ms: 600,
        },
        reorder_permille: 20,
        reorder_hold_ms: 250,
        drop_permille: 10,
        duplicate_permille: 5,
        stall_permille: 4,
        stall_min_ms: 1_500,
        stall_max_ms: 8_000,
        retransmit_timeout_ms: 1_000,
    };

    /// Aggressive adversary: heavy reordering, loss and duplication.
    pub const HOSTILE: Self = Self {
        latency: LatencyModel {
            min_ms: 0,
            max_ms: 400,
            jitter_ms: 300,
            spike_permille: 100,
            spike_max_ms: 1_500,
        },
        reorder_permille: 250,
        reorder_hold_ms: 600,
        drop_permille: 150,
        duplicate_permille: 150,
        stall_permille: 30,
        stall_min_ms: 500,
        stall_max_ms: 5_000,
        retransmit_timeout_ms: 300,
    };

    /// True when the channel is perfect (no latency, no faults).
    pub fn is_perfect(&self) -> bool {
        *self == Self::LOCAL
    }
}

/// Named profiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NetProfile {
    Local,
    CoderZscaler,
    Hostile,
}

impl NetProfile {
    pub const ALL: [NetProfile; 3] = [Self::Local, Self::CoderZscaler, Self::Hostile];
    /// Profiles that inject faults.
    pub const ADVERSARIAL: [NetProfile; 2] = [Self::CoderZscaler, Self::Hostile];

    pub fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::CoderZscaler => "coder_zscaler",
            Self::Hostile => "hostile",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|profile| profile.name() == name.trim())
    }

    pub fn conditions(self) -> NetConditions {
        match self {
            Self::Local => NetConditions::LOCAL,
            Self::CoderZscaler => NetConditions::CODER_ZSCALER,
            Self::Hostile => NetConditions::HOSTILE,
        }
    }
}

impl fmt::Display for NetProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Sender retransmits until acknowledged: every message eventually arrives at
    /// least once; loss turns into delay or duplication.
    AtLeastOnce,
    /// One shot: a lost message never arrives.
    FireAndForget,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    pub sent: u64,
    pub delivered: u64,
    /// Messages that never arrive (fire-and-forget loss).
    pub lost: u64,
    /// Drop faults injected (request or ACK lost).
    pub drops: u64,
    pub duplicates: u64,
    pub reorder_holds: u64,
    pub retransmits: u64,
    pub stalls: u64,
    pub reconnects: u64,
    pub max_latency_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetTraceKind {
    Send,
    Deliver { copy: u32, latency_ms: u64 },
    Lost,
    DropRequest,
    DropAck,
    Duplicate,
    ReorderHold,
    StallStart { until_ms: u64 },
    HeldByStall,
    Reconnect { connection: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetTraceEntry<L> {
    pub at_ms: u64,
    pub link: L,
    pub id: u64,
    pub kind: NetTraceKind,
}

impl<L: fmt::Debug> fmt::Display for NetTraceEntry<L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "t={}ms {:?} #{} {:?}",
            self.at_ms, self.link, self.id, self.kind
        )
    }
}

/// An event the simulator must act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent<L, M> {
    Deliver {
        link: L,
        id: u64,
        /// 0 for the first copy to arrive, 1+ for retransmits/duplicates.
        copy: u32,
        sent_at_ms: u64,
        delivered_at_ms: u64,
        msg: M,
    },
    /// A half-open stall ended; the link reconnected with a new connection
    /// generation. Receivers should resync from durable state.
    Reconnect {
        link: L,
        connection: u64,
        at_ms: u64,
    },
}

#[derive(Clone, Debug)]
enum Pending<L, M> {
    Copy {
        link: L,
        id: u64,
        sent_at_ms: u64,
        msg: M,
    },
    Reconnect {
        link: L,
    },
}

#[derive(Clone, Copy, Debug, Default)]
struct LinkState {
    stalled_until_ms: u64,
    connection: u64,
}

/// The adversarial channel. `L` names a link (a directed process hop); `M` is the
/// message payload.
#[derive(Clone, Debug)]
pub struct SimNet<L, M> {
    conditions: NetConditions,
    delivery: Delivery,
    rng: NetRng,
    now_ms: u64,
    next_id: u64,
    next_order: u64,
    pending: BTreeMap<(u64, u64), Pending<L, M>>,
    copies_delivered: BTreeMap<u64, u32>,
    links: BTreeMap<L, LinkState>,
    stats: NetStats,
    trace: Vec<NetTraceEntry<L>>,
}

impl<L, M> SimNet<L, M>
where
    L: Copy + Ord + fmt::Debug,
    M: Clone,
{
    pub fn new(conditions: NetConditions, seed: u64, delivery: Delivery) -> Self {
        Self {
            conditions,
            delivery,
            rng: NetRng::new(seed),
            now_ms: 0,
            next_id: 0,
            next_order: 0,
            pending: BTreeMap::new(),
            copies_delivered: BTreeMap::new(),
            links: BTreeMap::new(),
            stats: NetStats::default(),
            trace: Vec::new(),
        }
    }

    pub fn for_profile(profile: NetProfile, seed: u64, delivery: Delivery) -> Self {
        Self::new(profile.conditions(), seed, delivery)
    }

    pub fn conditions(&self) -> NetConditions {
        self.conditions
    }

    pub fn delivery(&self) -> Delivery {
        self.delivery
    }

    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    pub fn stats(&self) -> NetStats {
        self.stats
    }

    pub fn trace(&self) -> &[NetTraceEntry<L>] {
        &self.trace
    }

    /// The last `n` trace entries, one per line — a minimal repro trace.
    pub fn trace_tail(&self, n: usize) -> String {
        let start = self.trace.len().saturating_sub(n);
        self.trace[start..]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Scheduled copies and reconnects not yet delivered.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// True while `id` has a copy scheduled for delivery.
    pub fn is_pending(&self, id: u64) -> bool {
        self.pending.values().any(
            |pending| matches!(pending, Pending::Copy { id: pending_id, .. } if *pending_id == id),
        )
    }

    fn record(&mut self, link: L, id: u64, kind: NetTraceKind) {
        self.trace.push(NetTraceEntry {
            at_ms: self.now_ms,
            link,
            id,
            kind,
        });
    }

    fn schedule(&mut self, due_ms: u64, pending: Pending<L, M>) {
        let order = self.next_order;
        self.next_order += 1;
        self.pending.insert((due_ms, order), pending);
    }

    fn sample_latency(&mut self) -> u64 {
        let latency = self.conditions.latency;
        latency.sample(&mut self.rng)
    }

    /// Send `msg` over `link`. Returns the message id. The draw order below is
    /// fixed, so a seed reproduces exactly.
    pub fn send(&mut self, link: L, msg: M) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.sent += 1;
        self.record(link, id, NetTraceKind::Send);
        let now = self.now_ms;
        let conditions = self.conditions;

        let mut state = self.links.get(&link).copied().unwrap_or_default();
        if now >= state.stalled_until_ms && self.rng.chance(conditions.stall_permille) {
            let duration = self
                .rng
                .range_inclusive(conditions.stall_min_ms, conditions.stall_max_ms);
            state.stalled_until_ms = now + duration;
            self.stats.stalls += 1;
            self.record(
                link,
                id,
                NetTraceKind::StallStart {
                    until_ms: state.stalled_until_ms,
                },
            );
            self.schedule(state.stalled_until_ms, Pending::Reconnect { link });
        }
        self.links.insert(link, state);

        let mut dues: Vec<u64> = Vec::new();
        if now < state.stalled_until_ms {
            // Half-open: the bytes vanish into a stream that never resets.
            self.record(link, id, NetTraceKind::HeldByStall);
            match self.delivery {
                Delivery::AtLeastOnce => {
                    self.stats.retransmits += 1;
                    let latency = self.sample_latency();
                    dues.push(state.stalled_until_ms + latency);
                }
                Delivery::FireAndForget => {}
            }
        } else if self.rng.chance(conditions.drop_permille) {
            self.stats.drops += 1;
            let ack_lost = self.rng.chance(500);
            match (self.delivery, ack_lost) {
                (Delivery::FireAndForget, _) => {
                    self.record(link, id, NetTraceKind::DropRequest);
                }
                (Delivery::AtLeastOnce, false) => {
                    self.record(link, id, NetTraceKind::DropRequest);
                    self.stats.retransmits += 1;
                    let latency = self.sample_latency();
                    dues.push(now + conditions.retransmit_timeout_ms + latency);
                }
                (Delivery::AtLeastOnce, true) => {
                    // The receiver got it; the sender cannot tell and resends.
                    self.record(link, id, NetTraceKind::DropAck);
                    self.stats.retransmits += 1;
                    self.stats.duplicates += 1;
                    let first = self.sample_latency();
                    let resend = self.sample_latency();
                    dues.push(now + first);
                    dues.push(now + conditions.retransmit_timeout_ms + resend);
                }
            }
        } else {
            let mut due = now + self.sample_latency();
            if self.rng.chance(conditions.reorder_permille) {
                due += conditions.reorder_hold_ms;
                self.stats.reorder_holds += 1;
                self.record(link, id, NetTraceKind::ReorderHold);
            }
            dues.push(due);
        }

        if !dues.is_empty() && self.rng.chance(conditions.duplicate_permille) {
            let extra = self.sample_latency();
            dues.push(dues[0] + extra);
            self.stats.duplicates += 1;
            self.record(link, id, NetTraceKind::Duplicate);
        }

        if dues.is_empty() {
            self.stats.lost += 1;
            self.record(link, id, NetTraceKind::Lost);
        }
        for due in dues {
            self.schedule(
                due,
                Pending::Copy {
                    link,
                    id,
                    sent_at_ms: now,
                    msg: msg.clone(),
                },
            );
        }
        id
    }

    fn pop_due(&mut self, until_ms: u64) -> Option<NetEvent<L, M>> {
        let (&key, _) = self.pending.iter().next()?;
        if key.0 > until_ms {
            return None;
        }
        let pending = self.pending.remove(&key).expect("key present");
        self.now_ms = self.now_ms.max(key.0);
        Some(match pending {
            Pending::Copy {
                link,
                id,
                sent_at_ms,
                msg,
            } => {
                let copy = {
                    let count = self.copies_delivered.entry(id).or_insert(0);
                    let copy = *count;
                    *count += 1;
                    copy
                };
                let latency_ms = self.now_ms - sent_at_ms;
                self.stats.delivered += 1;
                self.stats.max_latency_ms = self.stats.max_latency_ms.max(latency_ms);
                self.record(link, id, NetTraceKind::Deliver { copy, latency_ms });
                NetEvent::Deliver {
                    link,
                    id,
                    copy,
                    sent_at_ms,
                    delivered_at_ms: self.now_ms,
                    msg,
                }
            }
            Pending::Reconnect { link } => {
                let state = self.links.entry(link).or_default();
                state.connection += 1;
                let connection = state.connection;
                self.stats.reconnects += 1;
                self.record(link, u64::MAX, NetTraceKind::Reconnect { connection });
                NetEvent::Reconnect {
                    link,
                    connection,
                    at_ms: self.now_ms,
                }
            }
        })
    }

    /// Advance the clock by `dt_ms`, returning every event due by then, in
    /// delivery order.
    pub fn advance(&mut self, dt_ms: u64) -> Vec<NetEvent<L, M>> {
        let until = self.now_ms + dt_ms;
        let mut events = Vec::new();
        while let Some(event) = self.pop_due(until) {
            events.push(event);
        }
        self.now_ms = until;
        events
    }

    /// RPC wait: advance until the first copy of `id` is delivered, returning it
    /// and everything due before it. Copies of `id` scheduled later (retransmits,
    /// duplicates) stay in flight as stragglers. Returns only already-due events
    /// when `id` has no pending copy (lost or already delivered).
    pub fn advance_until_delivered(&mut self, id: u64) -> Vec<NetEvent<L, M>> {
        let target = self
            .pending
            .iter()
            .find_map(|(key, pending)| match pending {
                Pending::Copy { id: pending_id, .. } if *pending_id == id => Some(key.0),
                _ => None,
            });
        let until = target.unwrap_or(self.now_ms);
        let mut events = Vec::new();
        while let Some(event) = self.pop_due(until) {
            let done = matches!(&event, NetEvent::Deliver { id: event_id, .. } if *event_id == id);
            events.push(event);
            if done {
                break;
            }
        }
        events
    }

    /// Deliver everything still in flight (fairness: the channel eventually
    /// delivers every scheduled copy).
    pub fn drain(&mut self) -> Vec<NetEvent<L, M>> {
        let mut events = Vec::new();
        while let Some(event) = self.pop_due(u64::MAX) {
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Link {
        A,
        B,
    }

    fn run(profile: NetProfile, seed: u64, delivery: Delivery) -> (Vec<String>, NetStats) {
        let mut net = SimNet::<Link, u32>::for_profile(profile, seed, delivery);
        let mut log = Vec::new();
        for step in 0..400u32 {
            let link = if step % 3 == 0 { Link::B } else { Link::A };
            net.send(link, step);
            for event in net.advance(50) {
                log.push(format!("{event:?}"));
            }
        }
        for event in net.drain() {
            log.push(format!("{event:?}"));
        }
        (log, net.stats())
    }

    #[test]
    fn a_seed_reproduces_exactly() {
        for profile in NetProfile::ALL {
            for seed in 0..8 {
                assert_eq!(
                    run(profile, seed, Delivery::AtLeastOnce),
                    run(profile, seed, Delivery::AtLeastOnce),
                    "{profile} seed {seed}"
                );
            }
        }
        assert_ne!(
            run(NetProfile::Hostile, 1, Delivery::AtLeastOnce).0,
            run(NetProfile::Hostile, 2, Delivery::AtLeastOnce).0
        );
    }

    #[test]
    fn local_is_a_perfect_in_order_zero_latency_channel() {
        let mut net =
            SimNet::<Link, u32>::for_profile(NetProfile::Local, 7, Delivery::FireAndForget);
        for value in 0..100 {
            let id = net.send(Link::A, value);
            let events = net.advance_until_delivered(id);
            assert_eq!(events.len(), 1);
            assert!(matches!(
                &events[0],
                NetEvent::Deliver { msg, copy: 0, sent_at_ms: 0, delivered_at_ms: 0, .. } if *msg == value
            ));
        }
        assert_eq!(net.in_flight(), 0);
        let stats = net.stats();
        assert_eq!((stats.sent, stats.delivered), (100, 100));
        assert_eq!(
            (
                stats.lost,
                stats.drops,
                stats.duplicates,
                stats.reorder_holds,
                stats.stalls
            ),
            (0, 0, 0, 0, 0)
        );
        assert!(NetProfile::Local.conditions().is_perfect());
    }

    #[test]
    fn at_least_once_eventually_delivers_every_message() {
        for profile in NetProfile::ALL {
            for seed in 0..16 {
                let (log, stats) = run(profile, seed, Delivery::AtLeastOnce);
                assert_eq!(stats.lost, 0, "{profile} seed {seed}");
                assert!(stats.delivered >= stats.sent, "{profile} seed {seed}");
                for value in 0..400u32 {
                    let needle = format!("msg: {value} }}");
                    assert!(
                        log.iter().any(|line| line.contains(&needle)),
                        "{profile} seed {seed} lost {value}"
                    );
                }
            }
        }
    }

    #[test]
    fn hostile_fire_and_forget_loses_reorders_and_duplicates() {
        let (_, stats) = run(NetProfile::Hostile, 3, Delivery::FireAndForget);
        assert!(stats.lost > 0, "{stats:?}");
        assert!(stats.duplicates > 0, "{stats:?}");
        assert!(stats.reorder_holds > 0, "{stats:?}");
        assert!(
            stats.stalls > 0 && stats.reconnects == stats.stalls,
            "{stats:?}"
        );
    }

    #[test]
    fn coder_zscaler_latency_is_tens_to_hundreds_of_ms() {
        let mut net =
            SimNet::<Link, u32>::for_profile(NetProfile::CoderZscaler, 11, Delivery::AtLeastOnce);
        let mut latencies = Vec::new();
        for value in 0..2_000 {
            let id = net.send(Link::A, value);
            for event in net.advance_until_delivered(id) {
                if let NetEvent::Deliver {
                    sent_at_ms,
                    delivered_at_ms,
                    copy: 0,
                    id: event_id,
                    ..
                } = event
                    && event_id == id
                {
                    latencies.push(delivered_at_ms - sent_at_ms);
                }
            }
        }
        latencies.sort_unstable();
        let median = latencies[latencies.len() / 2];
        assert!((20..=500).contains(&median), "median={median}");
        assert!(*latencies.first().unwrap() >= 20);
        assert!(
            *latencies.last().unwrap() >= 1_000,
            "stalls/retransmits add a tail"
        );
        let stats = net.stats();
        assert!(stats.stalls > 0 && stats.drops > 0, "{stats:?}");
    }

    #[test]
    fn reorder_lets_a_later_send_overtake_an_earlier_one() {
        let mut overtakes = 0;
        for seed in 0..32 {
            let mut net =
                SimNet::<Link, u32>::for_profile(NetProfile::Hostile, seed, Delivery::AtLeastOnce);
            net.send(Link::A, 1);
            net.send(Link::A, 2);
            let order: Vec<u32> = net
                .drain()
                .into_iter()
                .filter_map(|event| match event {
                    NetEvent::Deliver { msg, copy: 0, .. } => Some(msg),
                    _ => None,
                })
                .collect();
            if order.first() == Some(&2) {
                overtakes += 1;
            }
        }
        assert!(overtakes > 0);
    }

    #[test]
    fn profile_names_round_trip() {
        for profile in NetProfile::ALL {
            assert_eq!(NetProfile::parse(profile.name()), Some(profile));
        }
        assert_eq!(NetProfile::parse("lan"), None);
    }
}
