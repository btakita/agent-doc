//! Cross-document sweep helpers for preflight.
//!
//! ## Bounded sweep (GH #127)
//!
//! The sweep commits OTHER tracked documents from inside the admitting
//! document's `UserPromptSubmit` preflight. Unbounded, it walked every sibling
//! in the session registry (an owner lookup, a freshness probe and a commit per
//! document) and was measured at 23-42s of the 85s admission deadline in all
//! six refusals of one session — a prompt for document A refused because
//! documents B..Z needed committing.
//!
//! The sweep is maintenance, not admission, so it now runs under its own
//! budget ([`sweep_budget`]): at most [`DEFAULT_SWEEP_BUDGET`] (operator
//! override [`SWEEP_BUDGET_ENV`]) and never more than a quarter of the
//! admission time left. While it runs, the thread's admission deadline is
//! narrowed to that budget, so a controller RPC inside one sibling's step
//! cannot outlive it either. Siblings it does not reach are deferred, not
//! dropped: their order is rotated from a persisted cursor
//! ([`order_from_cursor`]), so the next preflight from any document resumes
//! where this one stopped and every sibling is reached eventually.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use agent_doc_debounce::admission_deadline::{self, AdmissionDeadline};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOwner {
    pane: String,
    source: String,
}

enum ActorSweepOwner {
    Active(SweepOwner),
    Inactive,
    Unknown,
}

fn actor_sweep_owner(audit_file: &Path, root: &Path, doc_path: &Path) -> ActorSweepOwner {
    match agent_doc_controller_io::project_controller::authoritative_actor_binding(root, doc_path) {
        Ok(Some(record)) if record.state.as_str() != "closed" => {
            ActorSweepOwner::Active(SweepOwner {
                pane: record.pane_id,
                source: format!("actor:{}", record.state.as_str()),
            })
        }
        Ok(Some(_)) => ActorSweepOwner::Inactive,
        Ok(None) => ActorSweepOwner::Unknown,
        Err(err) => {
            eprintln!(
                "[preflight] sweep: owner warning for {}: {}",
                doc_path.display(),
                err
            );
            agent_doc_ops_log_io::log_op(
                audit_file,
                &format!(
                    "foreign_owned_sweep_owner_warning file={} error={}",
                    doc_path.display(),
                    err.to_string().replace('\n', " ")
                ),
            );
            ActorSweepOwner::Unknown
        }
    }
}

fn registry_sweep_owner(
    root: &Path,
    registry: &tmux_router::Registry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    let key = tmux_router::registry::canonical_registry_key_in(root, &doc_path.to_string_lossy());
    registry.get(&key).and_then(|entry| {
        (!entry.pane.trim().is_empty()).then(|| SweepOwner {
            pane: entry.pane.clone(),
            source: "durable_registry".to_string(),
        })
    })
}

pub fn sweep_owner_for_doc(
    audit_file: &Path,
    root: &Path,
    registry: &tmux_router::Registry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    match actor_sweep_owner(audit_file, root, doc_path) {
        ActorSweepOwner::Active(owner) => Some(owner),
        ActorSweepOwner::Inactive | ActorSweepOwner::Unknown => {
            registry_sweep_owner(root, registry, doc_path)
        }
    }
}

pub fn current_sweep_owner(
    audit_file: &Path,
    root: &Path,
    registry: &tmux_router::Registry,
    current_doc: &Path,
) -> Option<SweepOwner> {
    sweep_owner_for_doc(audit_file, root, registry, current_doc)
}

pub fn log_and_skip_foreign_owned_sweep_if_needed(
    audit_file: &Path,
    doc_path: &Path,
    current_owner: Option<&SweepOwner>,
    sibling_owner: Option<&SweepOwner>,
) -> bool {
    let (Some(current_owner), Some(sibling_owner)) = (current_owner, sibling_owner) else {
        return false;
    };
    if !agent_doc_workflow::preflight_policy::should_skip_foreign_owned_sweep(
        Some(current_owner.pane.as_str()),
        Some(sibling_owner.pane.as_str()),
    ) {
        return false;
    };

    eprintln!(
        "[preflight] sweep: skipping {} (foreign-owned by pane {}; current owner pane {})",
        doc_path.display(),
        sibling_owner.pane,
        current_owner.pane
    );
    agent_doc_ops_log_io::log_op(
        audit_file,
        &format!(
            "foreign_owned_sweep_skip file={} owner_pane={} owner_source={} current_pane={} current_source={}",
            doc_path.display(),
            sibling_owner.pane,
            sibling_owner.source,
            current_owner.pane,
            current_owner.source
        ),
    );
    true
}

/// Sweep budget when no override is set (GH #127).
pub const DEFAULT_SWEEP_BUDGET: Duration = Duration::from_secs(8);

/// Operator override for [`DEFAULT_SWEEP_BUDGET`], in whole seconds. `0`
/// defers the whole sweep out of admission; each sibling is still committed by
/// its own preflight.
pub const SWEEP_BUDGET_ENV: &str = "AGENT_DOC_PREFLIGHT_SWEEP_BUDGET_SECS";

/// The sweep may spend at most `1 / ADMISSION_SHARE_DIVISOR` of the admission
/// time left when it starts.
pub const ADMISSION_SHARE_DIVISOR: u32 = 4;

/// Pure parse of [`SWEEP_BUDGET_ENV`]: unset or unparsable falls back to
/// [`DEFAULT_SWEEP_BUDGET`]; `0` is honoured.
pub fn resolve_sweep_budget(raw: Option<&str>) -> Duration {
    raw.and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_SWEEP_BUDGET)
}

/// The budget one sweep may spend: the configured budget, capped at a quarter
/// of the admission time left (`None` = no admission deadline, e.g. a CLI
/// preflight).
pub fn sweep_budget(configured: Duration, admission_remaining: Option<Duration>) -> Duration {
    match admission_remaining {
        Some(left) => configured.min(left / ADMISSION_SHARE_DIVISOR),
        None => configured,
    }
}

/// [`sweep_budget`] from the process environment and this thread's installed
/// admission deadline.
pub fn current_sweep_budget() -> Duration {
    sweep_budget(
        resolve_sweep_budget(std::env::var(SWEEP_BUDGET_ENV).ok().as_deref()),
        admission_deadline::remaining(),
    )
}

/// Where the next sweep starts: the first sibling the last bounded sweep did
/// not reach. Best-effort fairness state; losing it only restarts the rotation.
pub fn sweep_cursor_path(root: &Path) -> PathBuf {
    root.join(".agent-doc").join("preflight-sweep-cursor")
}

pub fn load_sweep_cursor(root: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(sweep_cursor_path(root)).ok()?;
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Persist `next` as the cursor, or clear it when the sweep reached everything.
pub fn store_sweep_cursor(root: &Path, next: Option<&Path>) {
    let path = sweep_cursor_path(root);
    let result = match next {
        Some(next) => path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, format!("{}\n", next.display()))),
        None => match std::fs::remove_file(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        },
    };
    if let Err(err) = result {
        eprintln!(
            "[preflight] sweep: cursor warning for {}: {}",
            path.display(),
            err
        );
    }
}

/// Sort `candidates` and rotate them to start at the first one at or after
/// `cursor`, so a sweep resumes where the last bounded one stopped. Duplicates
/// are removed.
pub fn order_from_cursor(mut candidates: Vec<PathBuf>, cursor: Option<&Path>) -> Vec<PathBuf> {
    candidates.sort();
    candidates.dedup();
    if let Some(cursor) = cursor {
        let start = candidates.partition_point(|candidate| candidate.as_path() < cursor);
        let len = candidates.len().max(1);
        candidates.rotate_left(start % len);
    }
    candidates
}

/// What one bounded sweep did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedSweepOutcome {
    /// Siblings whose step ran.
    pub visited: usize,
    /// Siblings the budget did not reach, in sweep order. The first is the
    /// next sweep's cursor.
    pub deferred: Vec<PathBuf>,
    /// The budget the sweep ran under.
    pub budget: Duration,
    /// Wall time the sweep spent.
    pub elapsed: Duration,
}

/// Run `step` over `candidates` in order until `budget` is spent (GH #127).
///
/// The budget is checked before each sibling, and the thread's admission
/// deadline is narrowed to the sweep's own deadline while `step` runs, so every
/// clamped wait inside a step ends with the sweep rather than with admission.
/// `step` returns `false` when it could not finish its sibling because the
/// sweep deadline ran out; that sibling is deferred with the rest.
pub fn run_bounded_sweep(
    candidates: Vec<PathBuf>,
    budget: Duration,
    mut step: impl FnMut(&Path) -> bool,
) -> BoundedSweepOutcome {
    let started = Instant::now();
    let sweep_deadline_at = started + budget;
    let narrowed = match admission_deadline::current() {
        Some(outer) => AdmissionDeadline {
            at: outer.at.min(sweep_deadline_at),
            budget: outer.budget,
        },
        None => AdmissionDeadline {
            at: sweep_deadline_at,
            budget,
        },
    };
    let _narrowed = admission_deadline::install(narrowed);

    let mut visited = 0;
    let mut deferred = Vec::new();
    let mut remaining = candidates.into_iter();
    for doc in remaining.by_ref() {
        if narrowed.remaining_at(Instant::now()).is_zero() {
            deferred.push(doc);
            break;
        }
        if !step(&doc) {
            deferred.push(doc);
            break;
        }
        visited += 1;
    }
    deferred.extend(remaining);
    BoundedSweepOutcome {
        visited,
        deferred,
        budget,
        elapsed: started.elapsed(),
    }
}

/// True when the sweep's (narrowed) deadline is spent: a step that sees this
/// after a lookup must not act on a lookup the deadline may have cut short.
pub fn sweep_deadline_exhausted() -> bool {
    admission_deadline::exhausted()
}

#[cfg(test)]
mod bounded_sweep_tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn sweep_budget_is_capped_at_a_quarter_of_the_admission_time_left() {
        assert_eq!(
            sweep_budget(Duration::from_secs(8), Some(Duration::from_secs(80))),
            Duration::from_secs(8)
        );
        assert_eq!(
            sweep_budget(Duration::from_secs(8), Some(Duration::from_secs(20))),
            Duration::from_secs(5)
        );
        assert_eq!(
            sweep_budget(Duration::from_secs(8), Some(Duration::ZERO)),
            Duration::ZERO
        );
        assert_eq!(
            sweep_budget(Duration::from_secs(8), None),
            Duration::from_secs(8)
        );
    }

    #[test]
    fn sweep_budget_override_parses_and_honours_zero() {
        assert_eq!(resolve_sweep_budget(None), DEFAULT_SWEEP_BUDGET);
        assert_eq!(resolve_sweep_budget(Some("junk")), DEFAULT_SWEEP_BUDGET);
        assert_eq!(resolve_sweep_budget(Some(" 3 ")), Duration::from_secs(3));
        assert_eq!(resolve_sweep_budget(Some("0")), Duration::ZERO);
    }

    #[test]
    fn order_from_cursor_resumes_at_the_first_deferred_sibling() {
        let candidates = paths(&["/p/c.md", "/p/a.md", "/p/b.md", "/p/a.md"]);
        assert_eq!(
            order_from_cursor(candidates.clone(), None),
            paths(&["/p/a.md", "/p/b.md", "/p/c.md"])
        );
        assert_eq!(
            order_from_cursor(candidates.clone(), Some(Path::new("/p/b.md"))),
            paths(&["/p/b.md", "/p/c.md", "/p/a.md"])
        );
        // A cursor whose document left the registry resumes at its successor.
        assert_eq!(
            order_from_cursor(candidates.clone(), Some(Path::new("/p/bb.md"))),
            paths(&["/p/c.md", "/p/a.md", "/p/b.md"])
        );
        // Past the end wraps to the start.
        assert_eq!(
            order_from_cursor(candidates, Some(Path::new("/p/z.md"))),
            paths(&["/p/a.md", "/p/b.md", "/p/c.md"])
        );
        assert!(order_from_cursor(Vec::new(), Some(Path::new("/p/a.md"))).is_empty());
    }

    /// GH #127: a sweep whose siblings cost more than its budget stops at the
    /// budget and defers the rest instead of spending the admission deadline.
    #[test]
    fn a_sweep_exceeding_its_budget_defers_the_rest() {
        let candidates: Vec<PathBuf> = (0..30)
            .map(|i| PathBuf::from(format!("/p/{i:02}.md")))
            .collect();
        let budget = Duration::from_millis(60);
        let outcome = run_bounded_sweep(candidates.clone(), budget, |_| {
            std::thread::sleep(Duration::from_millis(20));
            true
        });
        assert!(outcome.visited >= 1 && outcome.visited < 30, "{outcome:?}");
        assert_eq!(outcome.visited + outcome.deferred.len(), 30);
        assert_eq!(outcome.deferred[0], candidates[outcome.visited]);
        // Overshoot is bounded by one step, never by the remaining siblings.
        assert!(
            outcome.elapsed < budget + Duration::from_millis(200),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_zero_budget_defers_every_sibling_without_running_a_step() {
        let candidates = paths(&["/p/a.md", "/p/b.md"]);
        let outcome = run_bounded_sweep(candidates.clone(), Duration::ZERO, |_| {
            panic!("no step may run on a zero budget")
        });
        assert_eq!(outcome.visited, 0);
        assert_eq!(outcome.deferred, candidates);
    }

    #[test]
    fn a_step_cut_short_by_the_sweep_deadline_is_deferred() {
        let candidates = paths(&["/p/a.md", "/p/b.md", "/p/c.md"]);
        let mut calls = 0;
        let outcome = run_bounded_sweep(candidates, Duration::from_secs(5), |_| {
            calls += 1;
            calls < 2
        });
        assert_eq!(outcome.visited, 1);
        assert_eq!(outcome.deferred, paths(&["/p/b.md", "/p/c.md"]));
    }

    /// The sweep narrows the thread's admission deadline to its own while a
    /// step runs (so clamped waits end with the sweep), and restores the
    /// admission deadline afterwards untouched.
    #[test]
    fn the_sweep_narrows_and_then_restores_the_admission_deadline() {
        let outer = AdmissionDeadline {
            at: Instant::now() + Duration::from_secs(60),
            budget: Duration::from_secs(90),
        };
        let _outer = admission_deadline::install(outer);
        let mut seen = None;
        run_bounded_sweep(paths(&["/p/a.md"]), Duration::from_secs(2), |_| {
            seen = admission_deadline::remaining();
            true
        });
        let seen = seen.expect("a deadline is installed during the step");
        assert!(seen <= Duration::from_secs(2), "{seen:?}");
        assert_eq!(admission_deadline::current(), Some(outer));
    }

    #[test]
    fn the_cursor_round_trips_and_clears() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(load_sweep_cursor(tmp.path()), None);
        store_sweep_cursor(tmp.path(), Some(Path::new("/p/b.md")));
        assert_eq!(
            load_sweep_cursor(tmp.path()),
            Some(PathBuf::from("/p/b.md"))
        );
        store_sweep_cursor(tmp.path(), None);
        assert_eq!(load_sweep_cursor(tmp.path()), None);
        store_sweep_cursor(tmp.path(), None);
    }
}
