//! `#paneoomrunaway`: a resident-memory ceiling for the long-lived supervisor.
//!
//! A route-owned supervisor lives in its tmux pane for days. When one of its
//! data structures grows without bound, nothing in the process notices: the
//! growth only ends when `systemd-oomd` kills the whole pane scope, harness
//! included, after the machine has swapped for minutes. On 2026-10-27 the
//! detection screen model reflowed forever (see `screen::MIN_SCREEN_COLS`) and
//! six `sdk-demo.md` panes reached 135-142 GB each before being killed.
//!
//! The guard samples this process's resident set every
//! [`MEMORY_GUARD_INTERVAL`]. It logs a growth trail at each doubling from
//! [`FIRST_REPORT_BYTES`], and once the resident set reaches the ceiling it
//! logs `supervisor_memory_ceiling_exceeded` and aborts the supervisor. A loud,
//! named abort at a few gigabytes beats an anonymous oomd kill at 140 GB.
//!
//! The policy ([`MemoryGuardState::observe`]) is pure; the sampler and the
//! thread are the effects.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Ceiling override in MiB. `0` disables the guard.
pub const SUPERVISOR_RSS_LIMIT_ENV: &str = "AGENT_DOC_SUPERVISOR_RSS_LIMIT_MB";
/// Default ceiling. Healthy supervisors run at 40-300 MiB.
pub const DEFAULT_SUPERVISOR_RSS_LIMIT_BYTES: u64 = 4 << 30;
/// First growth-trail report; later reports fire at each doubling.
pub const FIRST_REPORT_BYTES: u64 = 512 << 20;
/// Sampling period. At the observed 1.5 GB/s growth this overshoots the
/// ceiling by under a gigabyte.
pub const MEMORY_GUARD_INTERVAL: Duration = Duration::from_millis(500);

const MIB: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryGuardPolicy {
    pub limit_bytes: u64,
}

impl MemoryGuardPolicy {
    /// Policy from the [`SUPERVISOR_RSS_LIMIT_ENV`] value. `None` when the
    /// guard is disabled (`0`). An unparsable value keeps the default: a typo
    /// must not silently remove the ceiling.
    pub fn from_env_value(value: Option<&str>) -> Option<Self> {
        let limit_bytes = match value.map(str::trim).filter(|v| !v.is_empty()) {
            None => DEFAULT_SUPERVISOR_RSS_LIMIT_BYTES,
            Some(raw) => match raw.parse::<u64>() {
                Ok(0) => return None,
                Ok(mib) => mib.saturating_mul(MIB),
                Err(_) => DEFAULT_SUPERVISOR_RSS_LIMIT_BYTES,
            },
        };
        Some(Self { limit_bytes })
    }

    pub fn from_env() -> Option<Self> {
        Self::from_env_value(std::env::var(SUPERVISOR_RSS_LIMIT_ENV).ok().as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryGuardVerdict {
    Steady,
    /// The resident set crossed `threshold_bytes` (a doubling step).
    Report {
        rss_bytes: u64,
        threshold_bytes: u64,
    },
    /// The resident set reached the ceiling.
    Exceeded {
        rss_bytes: u64,
        limit_bytes: u64,
    },
}

#[derive(Debug, Clone)]
pub struct MemoryGuardState {
    next_report_bytes: u64,
}

impl Default for MemoryGuardState {
    fn default() -> Self {
        Self {
            next_report_bytes: FIRST_REPORT_BYTES,
        }
    }
}

impl MemoryGuardState {
    /// Classify one resident-set sample. Each doubling threshold reports once.
    pub fn observe(&mut self, policy: MemoryGuardPolicy, rss_bytes: u64) -> MemoryGuardVerdict {
        if rss_bytes >= policy.limit_bytes {
            return MemoryGuardVerdict::Exceeded {
                rss_bytes,
                limit_bytes: policy.limit_bytes,
            };
        }
        if rss_bytes < self.next_report_bytes {
            return MemoryGuardVerdict::Steady;
        }
        let mut threshold = self.next_report_bytes;
        while threshold.saturating_mul(2) <= rss_bytes && threshold < u64::MAX / 2 {
            threshold *= 2;
        }
        self.next_report_bytes = threshold.saturating_mul(2);
        MemoryGuardVerdict::Report {
            rss_bytes,
            threshold_bytes: threshold,
        }
    }
}

/// The log line for a verdict, or `None` for [`MemoryGuardVerdict::Steady`].
pub fn verdict_log_line(verdict: MemoryGuardVerdict, pid: u32) -> Option<String> {
    match verdict {
        MemoryGuardVerdict::Steady => None,
        MemoryGuardVerdict::Report {
            rss_bytes,
            threshold_bytes,
        } => Some(format!(
            "supervisor_memory_growth pid={pid} rss_mb={} threshold_mb={} (#paneoomrunaway)",
            rss_bytes / MIB,
            threshold_bytes / MIB
        )),
        MemoryGuardVerdict::Exceeded {
            rss_bytes,
            limit_bytes,
        } => Some(format!(
            "supervisor_memory_ceiling_exceeded pid={pid} rss_mb={} limit_mb={} action=abort \
             override_env={SUPERVISOR_RSS_LIMIT_ENV} (#paneoomrunaway)",
            rss_bytes / MIB,
            limit_bytes / MIB
        )),
    }
}

/// This process's resident set in bytes (`/proc/self/statm`). `None` where
/// procfs is unavailable, which leaves the guard idle.
pub fn current_rss_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    #[cfg(unix)]
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    #[cfg(not(unix))]
    let page = 4096;
    let page = u64::try_from(page).ok().filter(|p| *p > 0)?;
    Some(resident_pages.saturating_mul(page))
}

/// A running guard thread. Dropping it stops the thread.
pub struct MemoryGuard {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl MemoryGuard {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            let _ = handle.join();
        }
    }
}

impl Drop for MemoryGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawn the guard. `sample` reads the resident set, `log` records each
/// verdict line, and `on_exceeded` runs once when the ceiling is reached (the
/// supervisor aborts there; tests record it). The thread exits after
/// `on_exceeded` returns.
pub fn spawn_memory_guard<S, L, E>(
    policy: MemoryGuardPolicy,
    interval: Duration,
    mut sample: S,
    log: L,
    on_exceeded: E,
) -> std::io::Result<MemoryGuard>
where
    S: FnMut() -> Option<u64> + Send + 'static,
    L: Fn(&str) + Send + 'static,
    E: FnOnce(&str) + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let handle = std::thread::Builder::new()
        .name("memory-guard".into())
        .spawn(move || {
            let pid = std::process::id();
            let mut state = MemoryGuardState::default();
            while !stop_thread.load(Ordering::Relaxed) {
                if let Some(rss) = sample() {
                    let verdict = state.observe(policy, rss);
                    if let Some(line) = verdict_log_line(verdict, pid) {
                        log(&line);
                        if matches!(verdict, MemoryGuardVerdict::Exceeded { .. }) {
                            on_exceeded(&line);
                            return;
                        }
                    }
                }
                std::thread::park_timeout(interval);
            }
        })?;
    Ok(MemoryGuard {
        stop,
        handle: Some(handle),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn policy(limit_mib: u64) -> MemoryGuardPolicy {
        MemoryGuardPolicy {
            limit_bytes: limit_mib * MIB,
        }
    }

    #[test]
    fn env_value_sets_disables_or_keeps_the_default_ceiling() {
        assert_eq!(
            MemoryGuardPolicy::from_env_value(None),
            Some(MemoryGuardPolicy {
                limit_bytes: DEFAULT_SUPERVISOR_RSS_LIMIT_BYTES
            })
        );
        assert_eq!(MemoryGuardPolicy::from_env_value(Some("0")), None);
        assert_eq!(
            MemoryGuardPolicy::from_env_value(Some("256")),
            Some(policy(256))
        );
        assert_eq!(
            MemoryGuardPolicy::from_env_value(Some("lots")),
            Some(MemoryGuardPolicy {
                limit_bytes: DEFAULT_SUPERVISOR_RSS_LIMIT_BYTES
            }),
            "a typo must not remove the ceiling"
        );
    }

    /// The 2026-10-27 growth curve in miniature: a resident set climbing at a
    /// steady rate reports each doubling once, then trips the ceiling.
    #[test]
    fn growth_reports_each_doubling_once_then_trips_the_ceiling() {
        let policy = policy(4096);
        let mut state = MemoryGuardState::default();
        let mut verdicts = Vec::new();
        for mib in (100..=5000).step_by(150) {
            let verdict = state.observe(policy, mib * MIB);
            if verdict != MemoryGuardVerdict::Steady {
                verdicts.push(verdict);
            }
            if matches!(verdict, MemoryGuardVerdict::Exceeded { .. }) {
                break;
            }
        }
        let thresholds: Vec<u64> = verdicts
            .iter()
            .filter_map(|v| match v {
                MemoryGuardVerdict::Report {
                    threshold_bytes, ..
                } => Some(threshold_bytes / MIB),
                _ => None,
            })
            .collect();
        assert_eq!(thresholds, [512, 1024, 2048]);
        assert!(matches!(
            verdicts.last(),
            Some(MemoryGuardVerdict::Exceeded { limit_bytes, .. }) if *limit_bytes == 4096 * MIB
        ));
    }

    #[test]
    fn a_jump_past_several_doublings_reports_the_highest_once() {
        let mut state = MemoryGuardState::default();
        assert_eq!(
            state.observe(policy(1 << 20), 3000 * MIB),
            MemoryGuardVerdict::Report {
                rss_bytes: 3000 * MIB,
                threshold_bytes: 2048 * MIB
            }
        );
        assert_eq!(
            state.observe(policy(1 << 20), 3500 * MIB),
            MemoryGuardVerdict::Steady
        );
    }

    #[test]
    fn steady_supervisor_never_reports() {
        let mut state = MemoryGuardState::default();
        for mib in [40, 120, 300, 310, 305] {
            assert_eq!(
                state.observe(policy(4096), mib * MIB),
                MemoryGuardVerdict::Steady
            );
        }
    }

    #[test]
    fn exceeded_line_names_the_ceiling_and_the_override() {
        let line = verdict_log_line(
            MemoryGuardVerdict::Exceeded {
                rss_bytes: 4200 * MIB,
                limit_bytes: 4096 * MIB,
            },
            7,
        )
        .unwrap();
        assert!(line.starts_with("supervisor_memory_ceiling_exceeded pid=7 rss_mb=4200"));
        assert!(line.contains("limit_mb=4096"));
        assert!(line.contains(SUPERVISOR_RSS_LIMIT_ENV));
    }

    #[test]
    fn current_rss_reads_procfs() {
        if !std::path::Path::new("/proc/self/statm").exists() {
            return;
        }
        let rss = current_rss_bytes().expect("rss");
        assert!(rss > 0);
    }

    /// The guard thread end to end: a real allocation pushes this process's
    /// resident set past a ceiling set just above the current one, and the
    /// guard fires `on_exceeded` with the ceiling line.
    #[test]
    fn guard_thread_trips_on_real_growth() {
        let Some(baseline) = current_rss_bytes() else {
            return;
        };
        let policy = MemoryGuardPolicy {
            limit_bytes: baseline + 64 * MIB,
        };
        let (tx, rx) = mpsc::channel::<String>();
        let mut guard = spawn_memory_guard(
            policy,
            Duration::from_millis(10),
            current_rss_bytes,
            |_| {},
            move |line| {
                let _ = tx.send(line.to_string());
            },
        )
        .expect("spawn guard");
        // Touch every page so the allocation is resident, not just reserved.
        let ballast = std::hint::black_box(vec![1u8; 160 * MIB as usize]);
        let line = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("guard must trip on real growth");
        assert!(
            line.starts_with("supervisor_memory_ceiling_exceeded"),
            "{line}"
        );
        drop(ballast);
        guard.stop();
    }

    #[test]
    fn guard_thread_stays_quiet_and_stops_cleanly() {
        let (tx, rx) = mpsc::channel::<String>();
        let mut guard = spawn_memory_guard(
            policy(4096),
            Duration::from_millis(5),
            || Some(100 * MIB),
            move |line| {
                let _ = tx.send(line.to_string());
            },
            |_| panic!("steady supervisor must not trip"),
        )
        .expect("spawn guard");
        std::thread::sleep(Duration::from_millis(50));
        guard.stop();
        assert!(rx.try_recv().is_err());
    }
}
